use crate::{
    analyzer::{self, AnalyzerRequest},
    convention_executor, deterministic_preview,
    edit_journal::{self, JournaledEdit},
    patch_plan::{self, PatchPlanEdit, PatchPlanRepairContext},
    repository_tooling,
    run_cancellation::RunCancellationToken,
    run_status::RunStatus,
    validation, RunCreateRequest, RunMetrics, ServiceState,
};
use anyhow::{anyhow, Result};
use local_refactor_core::{
    path_policy::PathPolicy,
    rules::{rule_by_id, Language, RuleDefinition, RuleExecutionKind},
};
use std::{error::Error, fmt, path::PathBuf, sync::Arc, time::Instant};

const MAX_MODEL_PROMPT_CHARS: usize = 96_000;

#[derive(Debug)]
struct RunCancelled;

struct RepairAttempt<'a> {
    model: &'a str,
    rule: &'a RuleDefinition,
    initial_validation_output: String,
}

impl fmt::Display for RunCancelled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("run was cancelled")
    }
}

impl Error for RunCancelled {}

pub(crate) async fn run_job(
    state: ServiceState,
    id: String,
    request: RunCreateRequest,
    cancellation: RunCancellationToken,
) -> Result<()> {
    let started = Instant::now();
    let mut metrics = RunMetrics::default();
    let result = run_job_inner(&state, &id, &request, &cancellation, &mut metrics).await;
    metrics.total_run_ms = Some(elapsed_ms(started));
    if let Err(error) = state.db.set_run_metrics(&id, &metrics) {
        tracing::warn!(run_id = id, error = %error, "failed to persist run metrics");
    }
    state.cancellations.unregister(&id);

    match result {
        Ok(()) => Ok(()),
        Err(error) if error.downcast_ref::<RunCancelled>().is_some() => {
            let _ = edit_journal::revert_patches(&state.db, &id);
            transition(&state, &id, RunStatus::Cancelled, "Run cancelled by user")?;
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
                "Run failed and changes were reverted",
            )?;
            Err(anyhow!(error_message))
        }
    }
}

async fn run_job_inner(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &RunCancellationToken,
    metrics: &mut RunMetrics,
) -> Result<()> {
    let rule_ids = effective_rules(request);
    let rules = rule_ids
        .iter()
        .map(|rule_id| {
            rule_by_id(rule_id).ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let needs_model = rules
        .iter()
        .any(|rule| rule.execution_kind == RuleExecutionKind::ModelPlanned);
    let model = if needs_model {
        Some(selected_model(request)?)
    } else {
        None
    };

    if let Some(model) = model.as_deref() {
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
        let ensure_result = state
            .model_gateway
            .ensure_model_available_with_progress(
                model,
                Arc::new(move |progress| {
                    let _ = append_run_event(&progress_state, &progress_run_id, &progress.message);
                }),
            )
            .await;
        metrics.model_ensure_available_ms = Some(elapsed_ms(started));
        ensure_result?;
        check_cancelled(cancellation)?;
    }

    if !needs_model {
        execute_deterministic_run(state, id, request, cancellation, metrics).await?;
        let validation_result = run_validation(state, id, request, metrics).await?;
        check_cancelled(cancellation)?;
        if validation_result.success {
            transition(
                state,
                id,
                RunStatus::Succeeded,
                "Run completed successfully",
            )?;
            return Ok(());
        }

        append_run_event(state, id, "Validation failed; reverting run changes")?;
        return Err(anyhow!(
            "validation failed: {}",
            validation_result.output.trim()
        ));
    }

    if let Some(plan) = request.rule_selection_plan.as_ref() {
        let repository_root = request
            .repository_root_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("rule selection plan requires repository root path"))?;
        for segment in &plan.segments {
            let segment_path = if segment.relative_path.is_empty() {
                repository_root.clone()
            } else {
                repository_root.join(&segment.relative_path)
            };
            append_run_event(
                state,
                id,
                &format!(
                    "Running automatic rule segment {}",
                    if segment.relative_path.is_empty() {
                        "."
                    } else {
                        segment.relative_path.as_str()
                    }
                ),
            )?;
            let mut segment_request = request.clone();
            segment_request.target_path = Some(segment_path.to_string_lossy().to_string());
            segment_request.rules = segment.rules.clone();
            for rule_id in &segment.rules {
                let rule = rule_by_id(rule_id)
                    .ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))?;
                execute_rule(
                    state,
                    id,
                    &segment_request,
                    cancellation,
                    metrics,
                    model.as_deref(),
                    rule,
                )
                .await?;
            }
        }
    } else {
        for rule in &rules {
            execute_rule(
                state,
                id,
                request,
                cancellation,
                metrics,
                model.as_deref(),
                rule,
            )
            .await?;
        }
    }

    let validation_result = run_validation(state, id, request, metrics).await?;
    check_cancelled(cancellation)?;
    if validation_result.success {
        transition(
            state,
            id,
            RunStatus::Succeeded,
            "Run completed successfully",
        )?;
        return Ok(());
    }

    if validation_result.retryable && request.repair_budget > 0 {
        if let (Some(model), Some(repair_rule)) =
            (model.as_deref(), first_model_planned_rule(&rules))
        {
            let repaired = attempt_repairs(
                state,
                id,
                request,
                cancellation,
                metrics,
                RepairAttempt {
                    model,
                    rule: repair_rule,
                    initial_validation_output: validation_result.output.clone(),
                },
            )
            .await?;
            if repaired {
                transition(
                    state,
                    id,
                    RunStatus::Succeeded,
                    "Run completed successfully",
                )?;
                return Ok(());
            }
        }
    }

    append_run_event(state, id, "Validation failed; reverting run changes")?;
    Err(anyhow!(
        "validation failed: {}",
        validation_result.output.trim()
    ))
}

async fn execute_rule(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &RunCancellationToken,
    metrics: &mut RunMetrics,
    model: Option<&str>,
    rule: &RuleDefinition,
) -> Result<()> {
    check_cancelled(cancellation)?;
    transition(
        state,
        id,
        RunStatus::Analyzing,
        &format!("Collecting mutable {} files", rule.language.display_name()),
    )?;
    let started = Instant::now();
    let prepared = prepare_source_request(request, rule.language);
    add_elapsed_ms(&mut metrics.file_collection_ms, started);
    let (policy, files) = prepared?;
    check_cancelled(cancellation)?;

    match rule.execution_kind {
        RuleExecutionKind::Deterministic => {
            if let Some(reason) = convention_rule_disabled_reason(request, rule.id) {
                state.db.append_event(id, &reason)?;
            } else if convention_executor::is_service_convention_rule(rule.id) {
                transition(
                    state,
                    id,
                    RunStatus::Planning,
                    &format!("Running deterministic convention rule {}", rule.id),
                )?;
                let started = Instant::now();
                let (edits, diagnostics) =
                    convention_executor::plan_rule(request, rule.id, &files, &mut |file| {
                        std::fs::read_to_string(file)
                            .map_err(|error| anyhow!("failed to read {file}: {error}"))
                    })
                    .await?;
                add_elapsed_ms(&mut metrics.analyzer_planning_ms, started);
                check_cancelled(cancellation)?;

                let started = Instant::now();
                let edit_result = apply_analyzer_response(
                    state,
                    id,
                    &policy,
                    analyzer::AnalyzerResponse { edits, diagnostics },
                );
                add_elapsed_ms(&mut metrics.edit_application_ms, started);
                edit_result?;
            } else if rule.language == Language::TypeScript {
                transition(
                    state,
                    id,
                    RunStatus::Planning,
                    &format!("Running TypeScript analyzer worker for {}", rule.id),
                )?;
                let started = Instant::now();
                let analyzer_response = analyzer::run(
                    &state.analyzer_script,
                    AnalyzerRequest {
                        files: files
                            .into_iter()
                            .map(analyzer::AnalyzerSourceFile::Path)
                            .collect(),
                        rules: vec![rule.id.to_string()],
                    },
                )
                .await;
                add_elapsed_ms(&mut metrics.analyzer_planning_ms, started);
                let analyzer_response = analyzer_response?;
                check_cancelled(cancellation)?;

                let started = Instant::now();
                let edit_result = apply_analyzer_response(state, id, &policy, analyzer_response);
                add_elapsed_ms(&mut metrics.edit_application_ms, started);
                edit_result?;
            } else {
                return Err(anyhow!(
                    "deterministic rule {} is not available for {}",
                    rule.id,
                    rule.language.display_name()
                ));
            }
        }
        RuleExecutionKind::ModelPlanned => {
            let model = model.ok_or_else(|| anyhow!("model-planned rule requires a model"))?;
            execute_model_planned_rule(
                state,
                id,
                request,
                cancellation,
                metrics,
                model,
                rule,
                &policy,
                &files,
                None,
                "Applying model patch-plan edits",
            )
            .await?;
        }
    }

    Ok(())
}

fn convention_rule_disabled_reason(request: &RunCreateRequest, rule_id: &str) -> Option<String> {
    let conventions = request.convention_snapshot.as_ref()?;
    match rule_id {
        "normalize-imports" if !conventions.typescript.ordering.imports => {
            Some("Skipped normalize-imports: TypeScript import ordering is disabled".to_string())
        }
        "sort-typescript-class-members" if !conventions.typescript.ordering.class_members => Some(
            "Skipped sort-typescript-class-members: TypeScript class member ordering is disabled"
                .to_string(),
        ),
        _ => None,
    }
}

async fn execute_deterministic_run(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &RunCancellationToken,
    metrics: &mut RunMetrics,
) -> Result<()> {
    transition(
        state,
        id,
        RunStatus::Analyzing,
        "Collecting mutable TypeScript files",
    )?;
    check_cancelled(cancellation)?;
    transition(
        state,
        id,
        RunStatus::Planning,
        "Running TypeScript analyzer worker for deterministic preview",
    )?;
    if let Some(plan) = request.rule_selection_plan.as_ref() {
        for segment in &plan.segments {
            append_run_event(
                state,
                id,
                &format!(
                    "Running automatic rule segment {}",
                    if segment.relative_path.is_empty() {
                        "."
                    } else {
                        segment.relative_path.as_str()
                    }
                ),
            )?;
        }
    }
    let started = Instant::now();
    let plan = deterministic_preview::plan(&state.analyzer_script, request).await;
    add_elapsed_ms(&mut metrics.analyzer_planning_ms, started);
    let plan = plan?;
    metrics.file_collection_ms = Some(metrics.file_collection_ms.unwrap_or(0));
    deterministic_preview::validate_fingerprint(
        &plan,
        request
            .expected_deterministic_preview_fingerprint
            .as_deref(),
    )?;
    check_cancelled(cancellation)?;

    for diagnostic in plan.response.diagnostics {
        append_run_event(state, id, &diagnostic)?;
    }

    if plan.response.files.is_empty() {
        append_run_event(state, id, "Analyzer produced no edits")?;
        return Ok(());
    }

    transition(
        state,
        id,
        RunStatus::Editing,
        "Applying deterministic analyzer edits",
    )?;
    let started = Instant::now();
    for step in plan.steps {
        if step.edits.is_empty() {
            continue;
        }
        edit_journal::apply_edits(&state.db, id, &step.policy, step.edits.clone())?;
        append_applied_events(state, id, step.edits)?;
    }
    add_elapsed_ms(&mut metrics.edit_application_ms, started);
    Ok(())
}

async fn attempt_repairs(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &RunCancellationToken,
    metrics: &mut RunMetrics,
    repair: RepairAttempt<'_>,
) -> Result<bool> {
    let mut validation_output = repair.initial_validation_output;
    for attempt in 1..=request.repair_budget {
        check_cancelled(cancellation)?;
        transition(
            state,
            id,
            RunStatus::Repairing,
            &format!(
                "Attempting model repair {attempt}/{}",
                request.repair_budget
            ),
        )?;
        let started = Instant::now();
        let prepared = prepare_source_request(request, repair.rule.language);
        add_elapsed_ms(&mut metrics.file_collection_ms, started);
        let (policy, files) = prepared?;
        execute_model_planned_rule(
            state,
            id,
            request,
            cancellation,
            metrics,
            repair.model,
            repair.rule,
            &policy,
            &files,
            Some(PatchPlanRepairContext {
                attempt,
                validation_output: truncated_validation_output(&validation_output),
            }),
            "Applying model repair patch-plan edits",
        )
        .await?;

        let validation_result = run_validation(state, id, request, metrics).await?;
        check_cancelled(cancellation)?;
        if validation_result.success {
            return Ok(true);
        }
        if !validation_result.retryable {
            return Err(anyhow!(
                "validation became non-repairable during model repair: {}",
                validation_result.output.trim()
            ));
        }
        validation_output = validation_result.output;
    }

    Err(anyhow!(
        "validation failed after {} repair attempt(s): {}",
        request.repair_budget,
        validation_output.trim()
    ))
}

#[allow(clippy::too_many_arguments)]
async fn execute_model_planned_rule(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &RunCancellationToken,
    metrics: &mut RunMetrics,
    model: &str,
    rule: &RuleDefinition,
    policy: &PathPolicy,
    files: &[String],
    repair_context: Option<PatchPlanRepairContext>,
    apply_message: &str,
) -> Result<()> {
    if files.is_empty() {
        append_run_event(
            state,
            id,
            &format!(
                "No mutable {} files found for {}",
                rule.language.display_name(),
                rule.id
            ),
        )?;
        return Ok(());
    }

    let total = files.len();
    for (index, file) in files.iter().enumerate() {
        check_cancelled(cancellation)?;
        transition(
            state,
            id,
            RunStatus::Planning,
            &format!(
                "Requesting model patch plan for {} ({}/{})",
                rule.id,
                index + 1,
                total
            ),
        )?;
        let single_file = vec![file.clone()];
        let patch_request =
            patch_plan_request(rule, request, policy, &single_file, repair_context.clone())?;
        preflight_model_prompt(rule, &patch_request)?;

        let started = Instant::now();
        let model_response = state
            .model_gateway
            .generate_patch_plan(model, patch_request.clone())
            .await;
        add_elapsed_ms(&mut metrics.model_planning_ms, started);
        let model_response = model_response?;
        check_cancelled(cancellation)?;

        let started = Instant::now();
        let edits = patch_plan::parse_patch_plan_response(&patch_request, policy, &model_response);
        add_elapsed_ms(&mut metrics.patch_plan_validation_ms, started);
        let edits = edits?;

        let started = Instant::now();
        let edit_result = apply_patch_plan_response(state, id, policy, edits, apply_message);
        add_elapsed_ms(&mut metrics.edit_application_ms, started);
        edit_result?;
    }

    Ok(())
}

fn preflight_model_prompt(
    rule: &RuleDefinition,
    request: &patch_plan::PatchPlanModelRequest,
) -> Result<()> {
    let prompt_chars = patch_plan::build_prompt(request).chars().count();
    if prompt_chars <= MAX_MODEL_PROMPT_CHARS {
        return Ok(());
    }

    let file_path = request
        .files
        .first()
        .map(|file| file.relative_path.as_str())
        .unwrap_or("<no file>");
    Err(anyhow!(
        "model-planned request for {} is too large for {} ({} chars); target a smaller file or folder",
        rule.id,
        file_path,
        prompt_chars
    ))
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
    let validation_dir = validation_root(request_target_path(request)?);
    let started = Instant::now();
    let validation_result = if request.validation_commands.is_empty() {
        repository_tooling::run_final_gate(&validation_dir).await
    } else {
        validation::run_commands(&validation_dir, &request.validation_commands).await
    };
    add_elapsed_ms(&mut metrics.validation_ms, started);
    let validation_result = validation_result?;
    state
        .db
        .set_validation_output(id, &validation_result.output)?;
    Ok(validation_result)
}

fn apply_analyzer_response(
    state: &ServiceState,
    run_id: &str,
    policy: &PathPolicy,
    response: analyzer::AnalyzerResponse,
) -> Result<()> {
    for diagnostic in response.diagnostics {
        state.db.append_event(run_id, &diagnostic)?;
    }

    if response.edits.is_empty() {
        state
            .db
            .append_event(run_id, "Analyzer produced no edits")?;
        return Ok(());
    }

    transition(
        state,
        run_id,
        RunStatus::Editing,
        "Applying deterministic analyzer edits",
    )?;

    let edits = response
        .edits
        .into_iter()
        .map(JournaledEdit::from)
        .collect::<Vec<_>>();
    edit_journal::apply_edits(&state.db, run_id, policy, edits.clone())?;
    append_applied_events(state, run_id, edits)
}

fn apply_patch_plan_response(
    state: &ServiceState,
    run_id: &str,
    policy: &PathPolicy,
    edits: Vec<PatchPlanEdit>,
    message: &str,
) -> Result<()> {
    if edits.is_empty() {
        state
            .db
            .append_event(run_id, "Patch plan produced no edits")?;
        return Ok(());
    }

    transition(state, run_id, RunStatus::Editing, message)?;

    let edits = edits
        .into_iter()
        .map(JournaledEdit::from)
        .collect::<Vec<_>>();
    edit_journal::apply_edits(&state.db, run_id, policy, edits.clone())?;
    append_applied_events(state, run_id, edits)
}

fn append_applied_events(
    state: &ServiceState,
    run_id: &str,
    edits: Vec<JournaledEdit>,
) -> Result<()> {
    for edit in edits {
        state.db.append_event(
            run_id,
            &format!("Applied {} to {}", edit.rule_id, edit.file_path.display()),
        )?;
    }
    Ok(())
}

fn first_model_planned_rule<'a>(rules: &'a [&'a RuleDefinition]) -> Option<&'a RuleDefinition> {
    rules
        .iter()
        .copied()
        .find(|rule| rule.execution_kind == RuleExecutionKind::ModelPlanned)
}

fn check_cancelled(cancellation: &RunCancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        return Err(anyhow!(RunCancelled));
    }
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

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn add_elapsed_ms(slot: &mut Option<u64>, started: Instant) {
    let elapsed = elapsed_ms(started);
    *slot = Some(slot.unwrap_or(0).saturating_add(elapsed));
}

fn truncated_validation_output(output: &str) -> String {
    const MAX_VALIDATION_OUTPUT_CHARS: usize = 20_000;
    let char_count = output.chars().count();
    if char_count <= MAX_VALIDATION_OUTPUT_CHARS {
        return output.to_string();
    }
    output
        .chars()
        .skip(char_count - MAX_VALIDATION_OUTPUT_CHARS)
        .collect()
}

use crate::{
    effective_rules, patch_plan_request, prepare_source_request, request_target_path,
    selected_model, validation_root,
};
