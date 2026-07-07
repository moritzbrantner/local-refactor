mod analyzer;
mod convention_executor;
mod coverage;
mod db;
mod deterministic_preview;
mod diff;
mod edit_journal;
mod model_provider;
mod patch_plan;
mod repository_source;
mod run_cancellation;
mod run_executor;
mod run_intake;
mod run_status;
mod validation;

use analyzer::AnalyzerRequest;
use analyzer::AnalyzerSourceFile;
use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
};
use local_refactor_core::{
    config::{ConfigLayer, TestFileMode},
    conventions::ConventionSettings,
    coverage::{BehaviorClaim, CoverageEvidenceItem},
    rule_selection::RuleSelectionPlan,
    rules::{rule_by_id, Language, RuleExecutionKind, INITIAL_RULES},
};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    net::SocketAddr,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};
use tokio::sync::broadcast;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

pub use db::{Database, RunEvent, RunMetrics};
pub use model_provider::{ModelDownloadProgress, ModelSummary, ModelsResponse};
pub use patch_plan::{
    PatchPlanEdit, PatchPlanModelRequest, PatchPlanRepairContext, PatchPlanSourceFile,
};
#[cfg(test)]
pub(crate) use run_intake::effective_config_for_paths;
pub(crate) use run_intake::{
    candidate_file_preview_for_request, effective_config_for, effective_rules, normalize_request,
    patch_plan_request, prepare_source_request, request_target_path,
    rule_selection_plan_for_request, selected_model, validation_root,
};

pub type ModelProgressSink = Arc<dyn Fn(ModelDownloadProgress) + Send + Sync>;
pub type ModelFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait ModelGateway: Send + Sync {
    fn list_models(&self) -> ModelFuture<'_, ModelsResponse>;

    fn ensure_model_available_with_progress<'a>(
        &'a self,
        name: &'a str,
        on_progress: ModelProgressSink,
    ) -> ModelFuture<'a, ()>;

    fn generate_patch_plan<'a>(
        &'a self,
        model: &'a str,
        request: PatchPlanModelRequest,
    ) -> ModelFuture<'a, String>;
}

impl ModelGateway for model_provider::OllamaProvider {
    fn list_models(&self) -> ModelFuture<'_, ModelsResponse> {
        Box::pin(async move { self.list_models().await })
    }

    fn ensure_model_available_with_progress<'a>(
        &'a self,
        name: &'a str,
        on_progress: ModelProgressSink,
    ) -> ModelFuture<'a, ()> {
        Box::pin(async move {
            self.ensure_model_available_with_progress(name, |progress| on_progress(progress))
                .await
        })
    }

    fn generate_patch_plan<'a>(
        &'a self,
        model: &'a str,
        request: PatchPlanModelRequest,
    ) -> ModelFuture<'a, String> {
        Box::pin(async move { self.generate_patch_plan(model, request).await })
    }
}

#[derive(Clone)]
pub struct ServiceState {
    db: Arc<Database>,
    analyzer_script: PathBuf,
    model_gateway: Arc<dyn ModelGateway>,
    events: broadcast::Sender<db::RunEvent>,
    cancellations: run_cancellation::RunCancellationRegistry,
}

impl ServiceState {
    pub fn new(
        db: Arc<Database>,
        analyzer_script: PathBuf,
        model_gateway: Arc<dyn ModelGateway>,
    ) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            db,
            analyzer_script,
            model_gateway,
            events,
            cancellations: run_cancellation::RunCancellationRegistry::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCreateRequest {
    #[serde(default)]
    target_path: Option<String>,
    #[serde(default)]
    repository_id: Option<String>,
    #[serde(default)]
    repository_root_path: Option<String>,
    #[serde(default)]
    target_relative_path: Option<String>,
    #[serde(default)]
    rules: Vec<String>,
    #[serde(default)]
    rule_selection_plan: Option<RuleSelectionPlan>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    test_file_mode: Option<TestFileMode>,
    #[serde(default)]
    validation_commands: Vec<String>,
    #[serde(default)]
    protected_paths: Vec<String>,
    #[serde(default = "default_repair_budget")]
    repair_budget: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_deterministic_preview_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    convention_snapshot: Option<ConventionSettings>,
    #[serde(default)]
    run_kind: RunKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_coverage_run_id: Option<String>,
    #[serde(default)]
    coverage_evidence: Vec<CoverageEvidenceItem>,
    #[serde(default)]
    behavior_claims: Vec<BehaviorClaim>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RunKind {
    #[default]
    Refactoring,
    CoverageSolidification,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CandidateFilePreviewRequest {
    #[serde(flatten)]
    run: RunCreateRequest,
    #[serde(default)]
    limit_per_group: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeterministicPreviewApplyRequest {
    run: RunCreateRequest,
    preview_fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectiveConfigQuery {
    target_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListRunsQuery {
    #[serde(default)]
    repository_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RulesResponse<'a> {
    rules: &'a [local_refactor_core::rules::RuleDefinition],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuleSelectionPlanResponse {
    plan: RuleSelectionPlan,
    effective_config: local_refactor_core::EffectiveConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunCreatedResponse {
    id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffResponse {
    run_id: String,
    files: Vec<FileDiff>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileDiff {
    file_path: String,
    rule_id: String,
    summary: String,
    diff: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunReviewResponse {
    run: db::RunRecord,
    events: Vec<db::RunEvent>,
    diff: DiffResponse,
    metrics: RunMetrics,
}

fn default_repair_budget() -> u32 {
    2
}

pub async fn serve_from_env() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "local_refactor_service=info,tower_http=info".into()),
        )
        .init();

    let db = Arc::new(Database::open(default_db_path()?)?);
    let analyzer_script = find_analyzer_script()?;
    let state = ServiceState::new(
        db,
        analyzer_script,
        Arc::new(model_provider::OllamaProvider::from_env()),
    );
    let app = router(state);

    let addr = service_addr()?;
    tracing::info!("local-refactor service listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

pub fn router(state: ServiceState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/rules", get(rules))
        .route("/api/models", get(models))
        .route("/api/config/effective", get(effective_config))
        .route("/api/rule-selection/plan", post(rule_selection_plan))
        .merge(repository_source::routes())
        .route("/api/analyze", post(analyze))
        .route("/api/runs", post(create_run).get(list_runs))
        .route(
            "/api/runs/deterministic-preview",
            post(deterministic_preview),
        )
        .route(
            "/api/runs/deterministic-preview/apply",
            post(apply_deterministic_preview),
        )
        .route(
            "/api/runs/candidate-file-preview",
            post(candidate_file_preview),
        )
        .route("/api/runs/{id}", get(get_run))
        .route("/api/runs/{id}/review", get(run_review))
        .route("/api/runs/{id}/cancel", post(cancel_run))
        .route("/api/runs/{id}/revert", post(revert_run))
        .route("/api/runs/{id}/events", get(run_events))
        .route("/api/runs/{id}/diff", get(run_diff))
        .route(
            "/api/coverage/evidence-preview",
            post(coverage::coverage_evidence_preview),
        )
        .route("/api/coverage/runs", post(coverage::create_coverage_run))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn rules() -> Json<RulesResponse<'static>> {
    Json(RulesResponse {
        rules: INITIAL_RULES,
    })
}

async fn models(State(state): State<ServiceState>) -> impl IntoResponse {
    match state.model_gateway.list_models().await {
        Ok(models) => Json(models).into_response(),
        Err(error) => Json(serde_json::json!({
                "provider": "ollama",
                "models": model_provider::configured_model_summaries(),
                "error": error.to_string()
        }))
        .into_response(),
    }
}

async fn effective_config(Query(query): Query<EffectiveConfigQuery>) -> impl IntoResponse {
    match effective_config_for(Path::new(&query.target_path), ConfigLayer::default()) {
        Ok(config) => Json(config).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn rule_selection_plan(
    State(state): State<ServiceState>,
    Json(request): Json<RunCreateRequest>,
) -> impl IntoResponse {
    match rule_selection_plan_for_request(&state.db, request) {
        Ok((plan, effective_config)) => Json(RuleSelectionPlanResponse {
            plan,
            effective_config,
        })
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn candidate_file_preview(
    State(state): State<ServiceState>,
    Json(request): Json<CandidateFilePreviewRequest>,
) -> impl IntoResponse {
    match candidate_file_preview_for_request(&state.db, request.run, request.limit_per_group) {
        Ok(preview) => Json(preview).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn analyze(
    State(state): State<ServiceState>,
    Json(request): Json<RunCreateRequest>,
) -> impl IntoResponse {
    let request = match normalize_request(&state.db, request) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    let rules = effective_rules(&request);
    let analyzer_rules = match analyzer_rule_ids(&rules) {
        Ok(rules) => rules,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    match prepare_source_request(&request, Language::TypeScript).map(|prepared| {
        (
            prepared.0,
            AnalyzerRequest {
                files: prepared
                    .1
                    .into_iter()
                    .map(AnalyzerSourceFile::Path)
                    .collect(),
                rules: analyzer_rules,
            },
        )
    }) {
        Ok((_policy, analyzer_request)) => {
            match analyzer::run(&state.analyzer_script, analyzer_request).await {
                Ok(response) => Json(response).into_response(),
                Err(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": error.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn deterministic_preview(
    State(state): State<ServiceState>,
    Json(request): Json<RunCreateRequest>,
) -> impl IntoResponse {
    let request = match normalize_request(&state.db, request) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    match deterministic_preview::plan(&state.analyzer_script, &request).await {
        Ok(plan) => Json(plan.response).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn apply_deterministic_preview(
    State(state): State<ServiceState>,
    Json(request): Json<DeterministicPreviewApplyRequest>,
) -> impl IntoResponse {
    let mut run = match normalize_request(&state.db, request.run) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    let plan = match deterministic_preview::plan(&state.analyzer_script, &run).await {
        Ok(plan) => plan,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };
    if let Err(error) =
        deterministic_preview::validate_fingerprint(&plan, Some(&request.preview_fingerprint))
    {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response();
    }

    let id = Uuid::new_v4().to_string();
    run.expected_deterministic_preview_fingerprint = Some(request.preview_fingerprint);
    match state.db.insert_run(&id, &run) {
        Ok(()) => {
            let state_for_job = state.clone();
            let id_for_job = id.clone();
            let cancellation = state.cancellations.register(&id);
            tokio::spawn(async move {
                if let Err(error) =
                    run_executor::run_job(state_for_job, id_for_job.clone(), run, cancellation)
                        .await
                {
                    tracing::error!(run_id = id_for_job, error = %error, "run failed");
                }
            });
            (StatusCode::ACCEPTED, Json(RunCreatedResponse { id })).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

fn analyzer_rule_ids(rule_ids: &[String]) -> Result<Vec<String>> {
    let mut analyzer_rules = Vec::new();
    for rule_id in rule_ids {
        let rule =
            rule_by_id(rule_id).ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))?;
        if rule.language != Language::TypeScript
            || rule.execution_kind != RuleExecutionKind::Deterministic
        {
            return Err(anyhow!(
                "/api/analyze only supports deterministic TypeScript rules"
            ));
        }
        analyzer_rules.push(rule.id.to_string());
    }
    Ok(analyzer_rules)
}

async fn create_run(
    State(state): State<ServiceState>,
    Json(request): Json<RunCreateRequest>,
) -> impl IntoResponse {
    let id = Uuid::new_v4().to_string();
    let request = match normalize_request(&state.db, request) {
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
                    run_executor::run_job(state_for_job, id_for_job.clone(), request, cancellation)
                        .await
                {
                    tracing::error!(run_id = id_for_job, error = %error, "run failed");
                }
            });
            (StatusCode::ACCEPTED, Json(RunCreatedResponse { id })).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn list_runs(
    State(state): State<ServiceState>,
    Query(query): Query<ListRunsQuery>,
) -> impl IntoResponse {
    match state.db.list_runs(query.repository_id.as_deref()) {
        Ok(runs) => Json(runs).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn get_run(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match state.db.get_run(&id) {
        Ok(Some(run)) => Json(run).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn cancel_run(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let active = state.cancellations.cancel(&id);
    let result = if active {
        append_run_event(&state, &id, "Run cancellation requested")
    } else {
        transition(
            &state,
            &id,
            run_status::RunStatus::Cancelled,
            "Run cancelled by user",
        )
    };
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn revert_run(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match edit_journal::revert_patches(&state.db, &id) {
        Ok(_) => {
            let _ = transition(
                &state,
                &id,
                run_status::RunStatus::Reverted,
                "Run changes reverted",
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn run_events(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> Sse<
    impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use axum::response::sse::Event;
    use futures_util::stream;
    use futures_util::StreamExt;

    let history = state.db.events_for_run(&id).unwrap_or_default();
    let rx = state.events.subscribe();
    let id_filter = id.clone();

    let history_stream = stream::iter(history.into_iter().map(|event| {
        Ok(Event::default()
            .event("run-event")
            .data(serde_json::to_string(&event).unwrap_or_default()))
    }));

    let live_stream = futures_util::stream::unfold(rx, move |mut rx| {
        let id_filter = id_filter.clone();
        async move {
            loop {
                match rx.recv().await {
                    Ok(event) if event.run_id == id_filter => {
                        let sse = Event::default()
                            .event("run-event")
                            .data(serde_json::to_string(&event).unwrap_or_default());
                        return Some((Ok(sse), rx));
                    }
                    Ok(_) => continue,
                    Err(_) => return None,
                }
            }
        }
    });

    Sse::new(history_stream.chain(live_stream))
}

async fn run_diff(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match diff_for_run(&state.db, &id) {
        Ok(diff) => Json(diff).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn run_review(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let run = match state.db.get_run(&id) {
        Ok(Some(run)) => run,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    match (
        state.db.events_for_run(&id),
        diff_for_run(&state.db, &id),
        state.db.metrics_for_run(&id),
    ) {
        (Ok(events), Ok(diff), Ok(metrics)) => Json(RunReviewResponse {
            run,
            events,
            diff,
            metrics: metrics.unwrap_or_default(),
        })
        .into_response(),
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

fn diff_for_run(db: &Database, id: &str) -> Result<DiffResponse> {
    let patches = db.patches_for_run(id)?;
    Ok(DiffResponse {
        run_id: id.to_string(),
        files: patches
            .into_iter()
            .map(|patch| FileDiff {
                file_path: patch.file_path.clone(),
                rule_id: patch.rule_id.clone(),
                summary: patch.summary.clone(),
                diff: diff::unified(
                    &patch.file_path,
                    &patch.original_content,
                    &patch.new_content,
                ),
            })
            .collect(),
    })
}

fn transition(
    state: &ServiceState,
    run_id: &str,
    status: run_status::RunStatus,
    message: &str,
) -> Result<()> {
    state.db.update_status(run_id, status.as_str())?;
    append_run_event(state, run_id, message)
}

fn append_run_event(state: &ServiceState, run_id: &str, message: &str) -> Result<()> {
    let event = state.db.append_event(run_id, message)?;
    let _ = state.events.send(event);
    Ok(())
}

fn default_db_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("LOCAL_REFACTOR_DB") {
        return Ok(PathBuf::from(path));
    }

    let base = dirs::data_dir()
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| anyhow!("could not resolve data directory"))?;
    let dir = base.join("local-refactor");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("local-refactor.sqlite"))
}

fn service_addr() -> Result<SocketAddr> {
    if let Ok(addr) = std::env::var("LOCAL_REFACTOR_ADDR") {
        return addr
            .parse()
            .with_context(|| format!("invalid LOCAL_REFACTOR_ADDR: {addr}"));
    }

    let port = std::env::var("LOCAL_REFACTOR_PORT")
        .ok()
        .map(|value| {
            value
                .parse::<u16>()
                .with_context(|| format!("invalid LOCAL_REFACTOR_PORT: {value}"))
        })
        .transpose()?
        .unwrap_or(7373);
    Ok(SocketAddr::from(([127, 0, 0, 1], port)))
}

fn find_analyzer_script() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("LOCAL_REFACTOR_ANALYZER") {
        return Ok(PathBuf::from(path));
    }

    let cwd = std::env::current_dir()?;
    for candidate in [
        cwd.join("workers/typescript-analyzer/src/main.ts"),
        cwd.parent()
            .unwrap_or(&cwd)
            .join("workers/typescript-analyzer/src/main.ts"),
    ] {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow!("could not find TypeScript analyzer worker"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use local_refactor_core::rule_selection::{
        RuleSelectionReason, RuleSelectionReasonSource, RuleSelectionSegment,
    };
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("local-refactor.sqlite")).unwrap();
        (dir, db)
    }

    fn write_file(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn repository_request(
        db: &Database,
        repo: &TempDir,
        target_relative_path: &str,
        rules: Vec<&str>,
    ) -> RunCreateRequest {
        let repository = db
            .upsert_repository("repo-1", "Repo", &repo.path().to_string_lossy())
            .unwrap();
        RunCreateRequest {
            target_path: None,
            repository_id: Some(repository.id),
            repository_root_path: None,
            target_relative_path: Some(target_relative_path.to_string()),
            rules: rules.into_iter().map(str::to_string).collect(),
            rule_selection_plan: None,
            model: None,
            test_file_mode: Some(TestFileMode::ReadOnly),
            validation_commands: Vec::new(),
            protected_paths: Vec::new(),
            repair_budget: 2,
            expected_deterministic_preview_fingerprint: None,
            convention_snapshot: None,
            run_kind: RunKind::Refactoring,
            source_coverage_run_id: None,
            coverage_evidence: Vec::new(),
            behavior_claims: Vec::new(),
        }
    }

    #[test]
    fn effective_config_uses_default_refactor_rules_when_repository_has_no_rules_file() {
        let repo = tempfile::tempdir().unwrap();
        let default_dir = tempfile::tempdir().unwrap();
        let default_path = default_dir.path().join("refactor-rules.toml");
        std::fs::write(
            &default_path,
            r#"
rules = ["from-default"]
validationCommands = ["echo default validation"]
"#,
        )
        .unwrap();

        let config = effective_config_for_paths(
            repo.path(),
            ConfigLayer::default(),
            Some(default_path),
            None,
        )
        .unwrap();

        assert_eq!(config.rules, vec!["from-default"]);
        assert!(config.validation_commands.is_empty());
    }

    #[test]
    fn repository_refactor_rules_override_default_refactor_rules() {
        let repo = tempfile::tempdir().unwrap();
        let default_dir = tempfile::tempdir().unwrap();
        let default_path = default_dir.path().join("refactor-rules.toml");
        std::fs::write(&default_path, r#"rules = ["from-default"]"#).unwrap();
        std::fs::write(
            repo.path().join("refactor-rules.toml"),
            r#"rules = ["from-repository"]"#,
        )
        .unwrap();

        let config = effective_config_for_paths(
            repo.path(),
            ConfigLayer::default(),
            Some(default_path),
            None,
        )
        .unwrap();

        assert_eq!(config.rules, vec!["from-repository"]);
    }

    #[test]
    fn normalize_request_rejects_unsupported_model() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();

        let error = normalize_request(
            &db,
            RunCreateRequest {
                target_path: Some(repo.path().to_string_lossy().to_string()),
                repository_id: None,
                repository_root_path: None,
                target_relative_path: None,
                rules: vec!["add-documentation-comments".to_string()],
                rule_selection_plan: None,
                model: Some("unknown-model:latest".to_string()),
                test_file_mode: None,
                validation_commands: Vec::new(),
                protected_paths: Vec::new(),
                repair_budget: 2,
                expected_deterministic_preview_fingerprint: None,
                convention_snapshot: None,
                run_kind: RunKind::Refactoring,
                source_coverage_run_id: None,
                coverage_evidence: Vec::new(),
                behavior_claims: Vec::new(),
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("unsupported local coding model"));
    }

    #[test]
    fn candidate_preview_excludes_tests_when_tests_are_read_only() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/app.ts"),
            "export const value = true;\n",
        );
        write_file(
            &repo.path().join("src/app.test.ts"),
            "export const testValue = true;\n",
        );

        let preview = candidate_file_preview_for_request(
            &db,
            repository_request(&db, &repo, "src", vec!["simplify-conditional"]),
            Some(50),
        )
        .unwrap();

        assert_eq!(preview.total_candidate_files, 1);
        assert_eq!(preview.groups[0].files[0].relative_path, "src/app.ts");
    }

    #[test]
    fn candidate_preview_includes_tests_when_tests_are_mutable() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/app.ts"),
            "export const value = true;\n",
        );
        write_file(
            &repo.path().join("src/app.test.ts"),
            "export const testValue = true;\n",
        );
        let mut request = repository_request(&db, &repo, "src", vec!["simplify-conditional"]);
        request.test_file_mode = Some(TestFileMode::Mutable);

        let preview = candidate_file_preview_for_request(&db, request, Some(50)).unwrap();
        let files = preview.groups[0]
            .files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(preview.total_candidate_files, 2);
        assert_eq!(files, vec!["src/app.test.ts", "src/app.ts"]);
    }

    #[test]
    fn candidate_preview_excludes_protected_paths() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/app.ts"),
            "export const value = true;\n",
        );
        write_file(
            &repo.path().join("src/generated/client.ts"),
            "export const generated = true;\n",
        );
        let mut request = repository_request(&db, &repo, "src", vec!["simplify-conditional"]);
        request.protected_paths = vec!["generated/**".to_string()];

        let preview = candidate_file_preview_for_request(&db, request, Some(50)).unwrap();

        assert_eq!(preview.total_candidate_files, 1);
        assert_eq!(preview.groups[0].files[0].relative_path, "src/app.ts");
    }

    #[test]
    fn candidate_preview_groups_automatic_segments_by_rule() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/app.ts"),
            "export const value = true;\n",
        );
        write_file(
            &repo.path().join("other/app.ts"),
            "export const other = true;\n",
        );
        let mut request = repository_request(&db, &repo, ".", Vec::new());
        request.rule_selection_plan = Some(RuleSelectionPlan {
            target_relative_path: ".".to_string(),
            segments: vec![RuleSelectionSegment {
                relative_path: "src".to_string(),
                rules: vec!["simplify-conditional".to_string()],
                reasons: vec![RuleSelectionReason {
                    rule_id: "simplify-conditional".to_string(),
                    source: RuleSelectionReasonSource::Fallback,
                    message: "TypeScript source files are present".to_string(),
                }],
            }],
        });

        let preview = candidate_file_preview_for_request(&db, request, Some(50)).unwrap();

        assert_eq!(preview.groups.len(), 1);
        assert_eq!(
            preview.groups[0].segment_relative_path.as_deref(),
            Some("src")
        );
        assert_eq!(preview.groups[0].rule_id, "simplify-conditional");
        assert_eq!(preview.groups[0].total_files, 1);
        assert_eq!(preview.groups[0].files[0].relative_path, "src/app.ts");
    }

    #[test]
    fn candidate_preview_groups_manual_rules_by_rule() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/app.ts"),
            "export const value = true;\n",
        );
        write_file(
            &repo.path().join("src/lib.rs"),
            "pub fn value() -> bool { true }\n",
        );

        let preview = candidate_file_preview_for_request(
            &db,
            repository_request(
                &db,
                &repo,
                "src",
                vec!["simplify-conditional", "rust-extract-helper-function"],
            ),
            Some(50),
        )
        .unwrap();

        assert_eq!(preview.groups.len(), 2);
        assert_eq!(preview.groups[0].rule_id, "rust-extract-helper-function");
        assert_eq!(preview.groups[0].files[0].relative_path, "src/lib.rs");
        assert_eq!(preview.groups[1].rule_id, "simplify-conditional");
        assert_eq!(preview.groups[1].files[0].relative_path, "src/app.ts");
    }

    #[test]
    fn candidate_preview_caps_groups_and_sorts_paths() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(&repo.path().join("src/c.ts"), "export const c = true;\n");
        write_file(&repo.path().join("src/a.ts"), "export const a = true;\n");
        write_file(&repo.path().join("src/b.ts"), "export const b = true;\n");

        let preview = candidate_file_preview_for_request(
            &db,
            repository_request(&db, &repo, "src", vec!["simplify-conditional"]),
            Some(2),
        )
        .unwrap();
        let group = &preview.groups[0];
        let files = group
            .files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(preview.total_candidate_files, 3);
        assert_eq!(group.total_files, 3);
        assert_eq!(group.hidden_files, 1);
        assert_eq!(files, vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn candidate_preview_rejects_invalid_protected_globs() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/app.ts"),
            "export const value = true;\n",
        );
        let mut request = repository_request(&db, &repo, "src", vec!["simplify-conditional"]);
        request.protected_paths = vec!["[".to_string()];

        let error = candidate_file_preview_for_request(&db, request, Some(50)).unwrap_err();

        assert!(error.to_string().contains("invalid protected path glob"));
    }

    #[test]
    fn candidate_preview_does_not_create_run_history() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/app.ts"),
            "export const value = true;\n",
        );

        candidate_file_preview_for_request(
            &db,
            repository_request(&db, &repo, "src", vec!["simplify-conditional"]),
            Some(50),
        )
        .unwrap();

        assert!(db.list_runs(None).unwrap().is_empty());
    }
}
