use crate::{
    edit_journal, model_provider, patch_plan, run_intake, run_status::RunStatus, validation,
    RunCreateRequest, RunKind, RunMetrics, ServiceState,
};
use anyhow::{anyhow, Context, Result};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use local_refactor_core::{
    config::{default_protected_paths, ConfigLayer, TestFileMode},
    coverage::{
        canonical_target_root, collect_evidence, is_known_solidification_rule, CoverageEvidenceItem,
    },
    path_policy::PathPolicy,
    rules::{AllowedWrites, Language, RulePlanningContext, StackContext},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoverageEvidencePreviewRequest {
    #[serde(flatten)]
    run: RunCreateRequest,
    #[serde(default)]
    evidence_rules: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoverageEvidencePreviewResponse {
    target_relative_path: String,
    validation_commands: Vec<String>,
    evidence: Vec<CoverageEvidenceItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoverageRunCreateRequest {
    #[serde(flatten)]
    run: RunCreateRequest,
}

#[derive(Debug)]
struct CoverageRunCancelled;

impl fmt::Display for CoverageRunCancelled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("run was cancelled")
    }
}

impl Error for CoverageRunCancelled {}

pub(crate) async fn coverage_evidence_preview(
    State(state): State<ServiceState>,
    Json(request): Json<CoverageEvidencePreviewRequest>,
) -> impl IntoResponse {
    match preview(&state, request) {
        Ok(response) => Json(response).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub(crate) async fn create_coverage_run(
    State(state): State<ServiceState>,
    Json(request): Json<CoverageRunCreateRequest>,
) -> impl IntoResponse {
    let id = Uuid::new_v4().to_string();
    let request = match normalize_coverage_run(&state, request.run) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    match state.db.insert_run(&id, &request) {
        Ok(()) => {
            let state_for_job = state.clone();
            let id_for_job = id.clone();
            let cancellation = state.cancellations.register(&id);
            tokio::spawn(async move {
                if let Err(error) =
                    run_coverage_job(state_for_job, id_for_job.clone(), request, cancellation).await
                {
                    tracing::error!(run_id = id_for_job, error = %error, "coverage run failed");
                }
            });
            (StatusCode::ACCEPTED, Json(crate::RunCreatedResponse { id })).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

fn preview(
    state: &ServiceState,
    request: CoverageEvidencePreviewRequest,
) -> Result<CoverageEvidencePreviewResponse> {
    let mut run = normalize_coverage_base(state, request.run)?;
    let target_path = run_intake::request_target_path(&run)?;
    let target_root = canonical_target_root(Path::new(target_path))?;
    let display_root = run
        .repository_root_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| target_root.clone());
    let evidence = collect_evidence(
        &target_root,
        &display_root,
        &request.evidence_rules,
        &run.protected_paths,
    )?;
    let target_relative_path = run
        .target_relative_path
        .take()
        .unwrap_or_else(|| relative_path(&display_root, &target_root));

    Ok(CoverageEvidencePreviewResponse {
        target_relative_path,
        validation_commands: run.validation_commands,
        evidence,
    })
}

fn normalize_coverage_run(
    state: &ServiceState,
    mut run: RunCreateRequest,
) -> Result<RunCreateRequest> {
    run = normalize_coverage_base(state, run)?;
    if run.validation_commands.is_empty() {
        return Err(anyhow!(
            "coverage solidification requires validation commands"
        ));
    }
    if run.coverage_evidence.is_empty() {
        let target_path = run_intake::request_target_path(&run)?;
        let target_root = canonical_target_root(Path::new(target_path))?;
        let display_root = run
            .repository_root_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| target_root.clone());
        run.coverage_evidence = collect_evidence(
            &target_root,
            &display_root,
            &Vec::new(),
            &run.protected_paths,
        )?;
    }
    if run.coverage_evidence.is_empty() {
        return Err(anyhow!(
            "coverage solidification requires coverage evidence"
        ));
    }
    for rule in &run.rules {
        if !is_known_solidification_rule(rule) {
            return Err(anyhow!("unknown coverage solidification rule: {rule}"));
        }
    }
    if run.rules.is_empty() {
        run.rules = vec!["characterize-public-entrypoint".to_string()];
    }
    let model = run
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| model_provider::default_model_name().to_string());
    if !model_provider::is_supported_model(&model) {
        return Err(anyhow!("unsupported local coding model: {model}"));
    }
    run.model = Some(model);
    run.run_kind = RunKind::CoverageSolidification;
    run.test_file_mode = Some(TestFileMode::Mutable);
    Ok(run)
}

fn normalize_coverage_base(
    state: &ServiceState,
    mut run: RunCreateRequest,
) -> Result<RunCreateRequest> {
    run.rules.retain(|rule| !rule.trim().is_empty());
    resolve_request_target(&state.db, &mut run)?;
    let run_layer = ConfigLayer {
        rules: None,
        protected_paths: if run.protected_paths.is_empty() {
            None
        } else {
            Some(run.protected_paths.clone())
        },
        validation_commands: if run.validation_commands.is_empty() {
            None
        } else {
            Some(run.validation_commands.clone())
        },
        test_file_mode: Some(TestFileMode::Mutable),
        conventions: None,
    };
    let target_path = run_intake::request_target_path(&run)?;
    let config = run_intake::effective_config_for(Path::new(target_path), run_layer)?;
    run.protected_paths = config.protected_paths;
    run.validation_commands = config.validation_commands;
    run.test_file_mode = Some(TestFileMode::Mutable);
    if run.rules.is_empty() {
        run.rules = vec!["characterize-public-entrypoint".to_string()];
    }
    Ok(run)
}

async fn run_coverage_job(
    state: ServiceState,
    id: String,
    request: RunCreateRequest,
    cancellation: crate::run_cancellation::RunCancellationToken,
) -> Result<()> {
    let started = Instant::now();
    let mut metrics = RunMetrics::default();
    let result = run_coverage_job_inner(&state, &id, &request, &cancellation, &mut metrics).await;
    metrics.total_run_ms = Some(elapsed_ms(started));
    let _ = state.db.set_run_metrics(&id, &metrics);
    state.cancellations.unregister(&id);

    match result {
        Ok(()) => Ok(()),
        Err(error) if error.downcast_ref::<CoverageRunCancelled>().is_some() => {
            let _ = edit_journal::revert_patches(&state.db, &id);
            transition(
                &state,
                &id,
                RunStatus::Cancelled,
                "Coverage solidification cancelled by user",
            )?;
            Ok(())
        }
        Err(error) => {
            let revert_result = edit_journal::revert_patches(&state.db, &id);
            let error_message = match revert_result {
                Ok(_) => format!("{error:#}"),
                Err(revert_error) => {
                    format!("{error:#}; additionally failed to revert patches: {revert_error:#}")
                }
            };
            state.db.set_error(&id, &error_message)?;
            transition(
                &state,
                &id,
                RunStatus::Failed,
                "Coverage solidification failed and changes were reverted",
            )?;
            Err(anyhow!(error_message))
        }
    }
}

async fn run_coverage_job_inner(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &crate::run_cancellation::RunCancellationToken,
    metrics: &mut RunMetrics,
) -> Result<()> {
    let model = request
        .model
        .as_deref()
        .ok_or_else(|| anyhow!("coverage solidification requires a model"))?;
    transition(
        state,
        id,
        RunStatus::Preparing,
        &format!("Ensuring local model {model} is downloaded"),
    )?;
    check_cancelled(cancellation)?;
    let progress_state = state.clone();
    let progress_run_id = id.to_string();
    let started = Instant::now();
    state
        .model_gateway
        .ensure_model_available_with_progress(
            model,
            Arc::new(move |progress| {
                let _ = append_run_event(&progress_state, &progress_run_id, &progress.message);
            }),
        )
        .await?;
    metrics.model_ensure_available_ms = Some(elapsed_ms(started));

    let target_path = run_intake::request_target_path(request)?;
    let target_root = canonical_target_root(Path::new(target_path))?;
    let display_root = request
        .repository_root_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| target_root.clone());
    let policy = PathPolicy::new(
        target_root.clone(),
        &effective_protected_paths(request),
        TestFileMode::Mutable,
    )?;
    let files = coverage_context_files(request, &display_root)?;
    metrics.file_collection_ms = Some(0);

    let mut all_claims = Vec::new();
    for rule in &request.rules {
        check_cancelled(cancellation)?;
        transition(
            state,
            id,
            RunStatus::Planning,
            &format!("Requesting coverage patch plan for {rule}"),
        )?;
        let patch_request = coverage_patch_request(rule, request, &policy, &files)?;
        let started = Instant::now();
        let response = state
            .model_gateway
            .generate_patch_plan(model, patch_request.clone())
            .await?;
        add_elapsed_ms(&mut metrics.model_planning_ms, started);
        check_cancelled(cancellation)?;

        let started = Instant::now();
        let (edits, claims) =
            patch_plan::parse_coverage_patch_plan_response(&patch_request, &policy, &response)?;
        add_elapsed_ms(&mut metrics.patch_plan_validation_ms, started);
        if edits.is_empty() {
            append_run_event(state, id, "Coverage model produced no test edits")?;
            continue;
        }
        transition(
            state,
            id,
            RunStatus::Editing,
            "Applying coverage solidification test edits",
        )?;
        let started = Instant::now();
        edit_journal::apply_edits(
            &state.db,
            id,
            &policy,
            edits.clone().into_iter().map(Into::into).collect(),
        )?;
        add_elapsed_ms(&mut metrics.edit_application_ms, started);
        for edit in edits {
            append_run_event(
                state,
                id,
                &format!("Applied {} to {}", edit.rule_id, edit.file_path),
            )?;
        }
        all_claims.extend(claims);
    }
    state.db.set_behavior_claims(id, &all_claims)?;

    let validation_result = run_validation(state, id, request, metrics).await?;
    check_cancelled(cancellation)?;
    if validation_result.success {
        transition(
            state,
            id,
            RunStatus::Succeeded,
            "Coverage solidification completed successfully",
        )?;
        return Ok(());
    }

    append_run_event(state, id, "Validation failed; reverting coverage changes")?;
    Err(anyhow!(
        "validation failed: {}",
        validation_result.output.trim()
    ))
}

fn coverage_patch_request(
    rule_id: &str,
    request: &RunCreateRequest,
    policy: &PathPolicy,
    files: &[PathBuf],
) -> Result<patch_plan::PatchPlanModelRequest> {
    let sources = files
        .iter()
        .map(|path| {
            let relative = path
                .strip_prefix(policy.target_root())
                .with_context(|| format!("{} is outside target root", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            Ok(patch_plan::PatchPlanSourceFile {
                relative_path: relative,
                content,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let language = dominant_language(&request.coverage_evidence);

    Ok(patch_plan::PatchPlanModelRequest {
        rule_id: rule_id.to_string(),
        language,
        rule_name: coverage_rule_name(rule_id).to_string(),
        rule_description: coverage_rule_description(rule_id).to_string(),
        target_root: policy.target_root().to_path_buf(),
        files: sources,
        validation_commands: request.validation_commands.clone(),
        allowed_writes: AllowedWrites::MultiFileWithinTarget,
        planning_context: coverage_planning_context(),
        test_file_mode: TestFileMode::Mutable,
        stack_contexts: default_stack_contexts(language),
        coverage_evidence: request.coverage_evidence.clone(),
        repair_context: None,
    })
}

fn coverage_context_files(request: &RunCreateRequest, target_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for evidence in &request.coverage_evidence {
        files.insert(target_root.join(&evidence.source_path));
        for test in &evidence.nearby_test_paths {
            files.insert(target_root.join(test));
        }
    }
    for path in likely_existing_test_files(target_root)? {
        files.insert(path);
    }
    Ok(files.into_iter().filter(|path| path.exists()).collect())
}

fn likely_existing_test_files(target_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(target_root) {
        let entry = entry?;
        if entry.file_type().is_file()
            && local_refactor_core::path_policy::is_test_file(entry.path())
        {
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn effective_protected_paths(request: &RunCreateRequest) -> Vec<String> {
    let mut protected = default_protected_paths();
    protected.extend(request.protected_paths.clone());
    protected
}

fn coverage_planning_context() -> RulePlanningContext {
    RulePlanningContext {
        workflow_rules: vec![
            "Return only raw coverage-patch-plan-v1 JSON; do not return Markdown.".to_string(),
            "Write tests only; production source files are read-only context.".to_string(),
        ],
        preservation_rules: vec![
            "Characterize current behavior through public entrypoints.".to_string(),
            "Do not change runtime behavior or public contracts.".to_string(),
        ],
        structure_rules: vec![
            "Place tests in the owning test layer and follow nearby test style.".to_string(),
            "Prefer small, explicit assertions over broad truthiness.".to_string(),
        ],
        test_refactoring_rules: vec![
            "Do not skip, delete, or loosen existing tests.".to_string(),
            "Every test edit must support at least one behavior claim.".to_string(),
        ],
        forbidden_actions: vec![
            "Do not edit production code.".to_string(),
            "Do not import private helpers or expose internals for testing.".to_string(),
            "Do not change validation commands.".to_string(),
        ],
        stack_rules: Vec::new(),
    }
}

fn dominant_language(evidence: &[CoverageEvidenceItem]) -> Language {
    evidence
        .first()
        .map(|item| item.language)
        .unwrap_or(Language::TypeScript)
}

fn default_stack_contexts(language: Language) -> Vec<StackContext> {
    match language {
        Language::TypeScript => vec![StackContext::TypeScriptBackend],
        Language::Rust => vec![StackContext::RustBackend],
    }
}

fn coverage_rule_name(rule_id: &str) -> &'static str {
    match rule_id {
        "characterize-branch-and-error-behavior" => "Characterize Branch And Error Behavior",
        "characterize-boundary-inputs" => "Characterize Boundary Inputs",
        _ => "Characterize Public Entrypoint",
    }
}

fn coverage_rule_description(rule_id: &str) -> &'static str {
    match rule_id {
        "characterize-branch-and-error-behavior" => {
            "Adds tests for existing branch, error, empty, invalid, and fallback behavior."
        }
        "characterize-boundary-inputs" => {
            "Adds tests around parsing, validation, normalization, sorting, and transformation boundaries."
        }
        _ => "Adds tests for observable behavior through public APIs.",
    }
}

async fn run_validation(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    metrics: &mut RunMetrics,
) -> Result<validation::ValidationResult> {
    transition(
        state,
        id,
        RunStatus::Validating,
        "Running validation checks",
    )?;
    let validation_dir = run_intake::validation_root(run_intake::request_target_path(request)?);
    let started = Instant::now();
    let validation_result =
        validation::run_commands(&validation_dir, &request.validation_commands).await?;
    add_elapsed_ms(&mut metrics.validation_ms, started);
    state
        .db
        .set_validation_output(id, &validation_result.output)?;
    Ok(validation_result)
}

fn resolve_request_target(db: &crate::Database, request: &mut RunCreateRequest) -> Result<()> {
    if let Some(repository_id) = request.repository_id.as_deref() {
        let repository = db
            .get_repository(repository_id)?
            .ok_or_else(|| anyhow!("repository source was not found"))?;
        let relative = request
            .target_relative_path
            .as_deref()
            .ok_or_else(|| anyhow!("target relative path is required with repositoryId"))?;
        let root = std::fs::canonicalize(&repository.root_path)
            .with_context(|| format!("repository path is unavailable: {}", repository.root_path))?;
        let target = crate::repository_source::resolve_repository_folder(&root, relative)?;
        request.target_path = Some(target.to_string_lossy().to_string());
        request.repository_root_path = Some(root.to_string_lossy().to_string());
        request.target_relative_path = Some(relative_path(&root, &target));
        return Ok(());
    }

    let target_path = request
        .target_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("targetPath is required"))?;
    request.target_path = Some(target_path.to_string());
    Ok(())
}

fn transition(state: &ServiceState, run_id: &str, status: RunStatus, message: &str) -> Result<()> {
    state.db.update_status(run_id, status.as_str())?;
    append_run_event(state, run_id, message)
}

fn append_run_event(state: &ServiceState, run_id: &str, message: &str) -> Result<()> {
    let event = state.db.append_event(run_id, message)?;
    let _ = state.events.send(event);
    Ok(())
}

fn check_cancelled(cancellation: &crate::run_cancellation::RunCancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        return Err(anyhow!(CoverageRunCancelled));
    }
    Ok(())
}

fn add_elapsed_ms(slot: &mut Option<u64>, started: Instant) {
    let elapsed = elapsed_ms(started);
    *slot = Some(slot.unwrap_or(0).saturating_add(elapsed));
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

fn relative_path(root: &Path, path: &Path) -> String {
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();
    if relative.is_empty() {
        ".".to_string()
    } else {
        relative
    }
}
