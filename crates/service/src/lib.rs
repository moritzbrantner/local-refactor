mod analyzer;
mod convention_executor;
mod db;
mod deterministic_preview;
mod diff;
mod edit_journal;
mod model_provider;
mod patch_plan;
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
    routing::{get, patch, post},
    Json, Router,
};
use local_refactor_core::{
    config::{find_project_config, load_config_file, ConfigLayer, TestFileMode},
    conventions::{ConventionSettings, PartialConventionSettings},
    rule_selection::RuleSelectionPlan,
    rules::{rule_by_id, Language, RuleExecutionKind, INITIAL_RULES},
};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    io::ErrorKind,
    net::SocketAddr,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};
use tokio::process::Command;
use tokio::sync::broadcast;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

const MAX_FILE_PREVIEW_BYTES: u64 = 512 * 1024;

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryCreateRequest {
    path: String,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryUpdateRequest {
    label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryConventionsResponse {
    repository_id: String,
    project_config: Option<ConventionSettings>,
    local_override: Option<PartialConventionSettings>,
    effective: ConventionSettings,
    diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryPickResponse {
    repository: Option<db::RepositoryRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderChildrenQuery {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryFilePreviewQuery {
    path: String,
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
struct FolderChildrenResponse {
    repository_id: String,
    path: String,
    entries: Vec<FolderEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderEntry {
    name: String,
    relative_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryFilePreviewResponse {
    repository_id: String,
    relative_path: String,
    language: PreviewLanguage,
    content: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
enum PreviewLanguage {
    #[serde(rename = "typescript")]
    TypeScript,
    #[serde(rename = "rust")]
    Rust,
    #[serde(rename = "text")]
    Text,
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
        .route(
            "/api/repositories",
            get(list_repositories).post(create_repository),
        )
        .route("/api/repositories/pick", post(pick_repository))
        .route(
            "/api/repositories/{id}",
            patch(update_repository).delete(delete_repository),
        )
        .route("/api/repositories/{id}/folders", get(repository_folders))
        .route(
            "/api/repositories/{id}/conventions",
            get(repository_conventions),
        )
        .route(
            "/api/repositories/{id}/conventions/local-override",
            patch(update_repository_convention_override),
        )
        .route(
            "/api/repositories/{id}/file-preview",
            get(repository_file_preview),
        )
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

async fn list_repositories(State(state): State<ServiceState>) -> impl IntoResponse {
    match state.db.list_repositories() {
        Ok(repositories) => Json(repositories).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn create_repository(
    State(state): State<ServiceState>,
    Json(request): Json<RepositoryCreateRequest>,
) -> impl IntoResponse {
    match upsert_repository_source(&state.db, &request.path, request.label.as_deref()).await {
        Ok(repository) => Json(repository).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn pick_repository(State(state): State<ServiceState>) -> impl IntoResponse {
    let selected_path = match pick_repository_folder().await {
        Ok(Some(path)) => path,
        Ok(None) => {
            return Json(RepositoryPickResponse { repository: None }).into_response();
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response();
        }
    };

    let selected_path = selected_path.to_string_lossy().to_string();
    match upsert_repository_source(&state.db, &selected_path, None).await {
        Ok(repository) => Json(RepositoryPickResponse {
            repository: Some(repository),
        })
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn update_repository(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<RepositoryUpdateRequest>,
) -> impl IntoResponse {
    let label = request.label.trim();
    if label.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "repository label cannot be empty" })),
        )
            .into_response();
    }

    match state.db.update_repository_label(&id, label) {
        Ok(Some(repository)) => Json(repository).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn delete_repository(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match state.db.delete_repository(&id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn repository_folders(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<FolderChildrenQuery>,
) -> impl IntoResponse {
    let repository = match state.db.get_repository(&id) {
        Ok(Some(repository)) => repository,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    let requested_path = query.path.as_deref().unwrap_or(".");
    let root = match std::fs::canonicalize(&repository.root_path) {
        Ok(root) => root,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };
    let folder = match resolve_repository_folder(&root, requested_path) {
        Ok(folder) => folder,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    match folder_entries(&root, &folder) {
        Ok(entries) => Json(FolderChildrenResponse {
            repository_id: id,
            path: relative_path_from_root(&root, &folder),
            entries,
        })
        .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn repository_conventions(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match repository_conventions_response(&state.db, &id) {
        Ok(Some(response)) => Json(response).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn update_repository_convention_override(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<PartialConventionSettings>,
) -> impl IntoResponse {
    match state.db.set_repository_convention_override(&id, &request) {
        Ok(Some(_)) => match repository_conventions_response(&state.db, &id) {
            Ok(Some(response)) => Json(response).into_response(),
            Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response(),
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn repository_file_preview(
    State(state): State<ServiceState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<RepositoryFilePreviewQuery>,
) -> impl IntoResponse {
    match repository_file_preview_response(&state.db, &id, &query.path) {
        Ok(preview) => Json(preview).into_response(),
        Err(FilePreviewError::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Err(FilePreviewError::TooLarge(message)) => (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response(),
        Err(FilePreviewError::BadRequest(error)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
        Err(FilePreviewError::Internal(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
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

async fn git_root_for(path: &str) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .await
        .with_context(|| "failed to run git")?;

    if !output.status.success() {
        return Err(anyhow!("path is not inside a Git work tree"));
    }

    let root = String::from_utf8(output.stdout)
        .map_err(|_| anyhow!("git returned a non-UTF-8 repository path"))?;
    let root = root.trim();
    if root.is_empty() {
        return Err(anyhow!("git returned an empty repository path"));
    }

    std::fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize Git repository root: {root}"))
}

async fn upsert_repository_source(
    db: &Database,
    path: &str,
    label: Option<&str>,
) -> Result<db::RepositoryRecord> {
    let root = git_root_for(path).await?;
    let label = label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| repository_label_from_root(&root));
    let root_path = root.to_string_lossy().to_string();

    db.upsert_repository(&Uuid::new_v4().to_string(), &label, &root_path)
}

async fn pick_repository_folder() -> Result<Option<PathBuf>> {
    tokio::task::spawn_blocking(pick_repository_folder_blocking)
        .await
        .context("folder picker task failed")?
}

fn pick_repository_folder_blocking() -> Result<Option<PathBuf>> {
    match std::env::consts::OS {
        "macos" => run_folder_picker_command(
            "osascript",
            &[
                "-e",
                r#"POSIX path of (choose folder with prompt "Select a Git repository root")"#,
            ],
        ),
        "windows" => run_folder_picker_command(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; $dialog.Description = 'Select a Git repository root'; if ($dialog.ShowDialog() -eq 'OK') { $dialog.SelectedPath }",
            ],
        ),
        _ => run_first_available_folder_picker(&[
            (
                "zenity",
                &[
                    "--file-selection",
                    "--directory",
                    "--modal",
                    "--width=900",
                    "--height=650",
                    "--title=Select a Git repository root",
                ][..],
            ),
            ("kdialog", &["--getexistingdirectory", "."][..]),
        ]),
    }
}

fn run_first_available_folder_picker(commands: &[(&str, &[&str])]) -> Result<Option<PathBuf>> {
    let mut missing = Vec::new();
    for (program, args) in commands {
        match run_folder_picker_command(program, args) {
            Ok(path) => return Ok(path),
            Err(error) if command_was_not_found(error.as_ref()) => {
                missing.push((*program).to_string());
            }
            Err(error) => return Err(error),
        }
    }

    Err(anyhow!(
        "no folder picker is available; install one of: {}",
        missing.join(", ")
    ))
}

fn run_folder_picker_command(program: &str, args: &[&str]) -> Result<Option<PathBuf>> {
    let output = std::process::Command::new(program)
        .args(args)
        .env("GTK_USE_PORTAL", "1")
        .output()
        .with_context(|| format!("failed to open folder picker with {program}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if picker_exit_was_cancel(output.status.code(), stderr.trim()) {
            return Ok(None);
        }
        return Err(anyhow!(
            "folder picker exited with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let path = String::from_utf8(output.stdout)
        .map_err(|_| anyhow!("folder picker returned a non-UTF-8 path"))?;
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }

    Ok(Some(PathBuf::from(path)))
}

fn picker_exit_was_cancel(code: Option<i32>, stderr: &str) -> bool {
    if code == Some(1) && stderr.is_empty() {
        return true;
    }

    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("user canceled") || stderr.contains("cancelled")
}

fn command_was_not_found(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
            return io_error.kind() == ErrorKind::NotFound;
        }
        current = error.source();
    }
    false
}

fn repository_label_from_root(root: &Path) -> String {
    root.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Repository")
        .to_string()
}

fn repository_conventions_response(
    db: &Database,
    repository_id: &str,
) -> Result<Option<RepositoryConventionsResponse>> {
    let Some(repository) = db.get_repository(repository_id)? else {
        return Ok(None);
    };
    let root = std::fs::canonicalize(&repository.root_path)
        .with_context(|| format!("repository path is unavailable: {}", repository.root_path))?;
    let project_config = project_convention_settings(&root)?;
    let local_override = db.get_repository_convention_override(repository_id)?;
    let mut effective = project_config.clone().unwrap_or_default();
    if let Some(override_settings) = local_override.clone() {
        effective.apply_partial(override_settings);
    }
    let diagnostics = convention_diagnostics(&root, &effective);

    Ok(Some(RepositoryConventionsResponse {
        repository_id: repository_id.to_string(),
        project_config,
        local_override,
        effective,
        diagnostics,
    }))
}

fn project_convention_settings(root: &Path) -> Result<Option<ConventionSettings>> {
    let Some(config_path) = find_project_config(root) else {
        return Ok(None);
    };
    let layer = load_config_file(&config_path)?;
    let Some(conventions) = layer.conventions else {
        return Ok(None);
    };
    let mut settings = ConventionSettings::default();
    settings.apply_partial(conventions);
    Ok(Some(settings))
}

fn convention_diagnostics(root: &Path, settings: &ConventionSettings) -> Vec<String> {
    let mut diagnostics = Vec::new();
    if settings.typescript.formatter.enabled {
        if settings.typescript.formatter.require_config && find_prettier_config(root).is_none() {
            diagnostics.push(
                "TypeScript formatter requires a Prettier config, but none was found.".to_string(),
            );
        }
        if find_prettier_command(root).is_none() {
            diagnostics.push(
                "TypeScript formatter command was not found; install prettier locally or on PATH."
                    .to_string(),
            );
        }
    }
    if settings.rust.formatter.enabled {
        if settings.rust.formatter.require_config && find_rustfmt_config(root).is_none() {
            diagnostics.push(
                "Rust formatter requires rustfmt.toml or .rustfmt.toml, but none was found."
                    .to_string(),
            );
        }
        if find_command_on_path("rustfmt").is_none() {
            diagnostics.push("rustfmt command was not found on PATH.".to_string());
        }
    }
    diagnostics
}

fn find_prettier_config(root: &Path) -> Option<PathBuf> {
    [
        ".prettierrc",
        ".prettierrc.json",
        ".prettierrc.yml",
        ".prettierrc.yaml",
        ".prettierrc.toml",
        "prettier.config.js",
        "prettier.config.cjs",
        "prettier.config.mjs",
        "prettier.config.ts",
    ]
    .into_iter()
    .map(|name| root.join(name))
    .find(|path| path.exists())
}

fn find_prettier_command(root: &Path) -> Option<PathBuf> {
    let local = root.join("node_modules/.bin/prettier");
    if local.exists() {
        return Some(local);
    }
    find_command_on_path("prettier")
}

fn find_rustfmt_config(root: &Path) -> Option<PathBuf> {
    ["rustfmt.toml", ".rustfmt.toml"]
        .into_iter()
        .map(|name| root.join(name))
        .find(|path| path.exists())
}

fn find_command_on_path(command: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|path| path.join(command))
        .find(|path| path.exists())
}

#[derive(Debug)]
enum FilePreviewError {
    NotFound,
    TooLarge(String),
    BadRequest(anyhow::Error),
    Internal(anyhow::Error),
}

fn repository_file_preview_response(
    db: &Database,
    repository_id: &str,
    relative_path: &str,
) -> Result<RepositoryFilePreviewResponse, FilePreviewError> {
    let repository = db
        .get_repository(repository_id)
        .map_err(|error| FilePreviewError::Internal(error.into()))?
        .ok_or(FilePreviewError::NotFound)?;
    let root = std::fs::canonicalize(&repository.root_path)
        .map_err(|error| FilePreviewError::BadRequest(error.into()))?;
    let path =
        resolve_repository_file(&root, relative_path).map_err(FilePreviewError::BadRequest)?;
    let metadata =
        std::fs::metadata(&path).map_err(|error| FilePreviewError::BadRequest(error.into()))?;
    if metadata.len() > MAX_FILE_PREVIEW_BYTES {
        return Err(FilePreviewError::TooLarge(format!(
            "file preview is limited to {} bytes",
            MAX_FILE_PREVIEW_BYTES
        )));
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|error| FilePreviewError::BadRequest(error.into()))?;

    Ok(RepositoryFilePreviewResponse {
        repository_id: repository.id,
        relative_path: relative_path_from_root(&root, &path),
        language: preview_language_for_path(&path),
        content,
        size_bytes: metadata.len(),
    })
}

fn resolve_repository_folder(root: &Path, relative_path: &str) -> Result<PathBuf> {
    let trimmed = relative_path.trim();
    let relative = Path::new(trimmed);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "repository folder must be a relative path inside the repository root"
        ));
    }

    let candidate = if trimmed.is_empty() || trimmed == "." {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    let folder = std::fs::canonicalize(&candidate)
        .with_context(|| format!("repository folder does not exist: {}", candidate.display()))?;
    if !folder.starts_with(root) {
        return Err(anyhow!(
            "repository folder must stay inside the repository root"
        ));
    }
    if !folder.is_dir() {
        return Err(anyhow!("repository target must be a directory"));
    }
    Ok(folder)
}

fn resolve_repository_file(root: &Path, relative_path: &str) -> Result<PathBuf> {
    let trimmed = relative_path.trim();
    let relative = Path::new(trimmed);
    if trimmed.is_empty()
        || trimmed == "."
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!(
            "repository file must be a relative path inside the repository root"
        ));
    }

    let candidate = root.join(relative);
    let metadata = std::fs::symlink_metadata(&candidate)
        .with_context(|| format!("repository file does not exist: {}", candidate.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(anyhow!("repository file preview does not follow symlinks"));
    }
    if !metadata.file_type().is_file() {
        return Err(anyhow!("repository file preview target must be a file"));
    }

    let file = std::fs::canonicalize(&candidate)
        .with_context(|| format!("repository file does not exist: {}", candidate.display()))?;
    if !file.starts_with(root) {
        return Err(anyhow!(
            "repository file must stay inside the repository root"
        ));
    }
    Ok(file)
}

fn folder_entries(root: &Path, folder: &Path) -> Result<Vec<FolderEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(folder)
        .with_context(|| format!("failed to read folder {}", folder.display()))?
    {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden_picker_dir(&name) {
            continue;
        }

        let path = entry.path();
        entries.push(FolderEntry {
            name,
            relative_path: relative_path_from_root(root, &path),
        });
    }

    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn relative_path_from_root(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.as_os_str().is_empty() {
        return ".".to_string();
    }
    relative.to_string_lossy().replace('\\', "/")
}

fn preview_language_for_path(path: &Path) -> PreviewLanguage {
    match path.extension().and_then(|value| value.to_str()) {
        Some("ts" | "tsx" | "js" | "jsx") => PreviewLanguage::TypeScript,
        Some("rs") => PreviewLanguage::Rust,
        _ => PreviewLanguage::Text,
    }
}

fn is_hidden_picker_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | "dist" | "build" | ".next" | "coverage"
    )
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
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("local-refactor.sqlite")).unwrap();
        (dir, db)
    }

    fn init_git_repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let status = StdCommand::new("git")
            .arg("init")
            .arg(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        dir
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
        }
    }

    #[tokio::test]
    async fn git_root_normalizes_subfolder_to_repository_root() {
        let repo = init_git_repo();
        let src = repo.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        let root = git_root_for(src.to_str().unwrap()).await.unwrap();

        assert_eq!(root, std::fs::canonicalize(repo.path()).unwrap());
    }

    #[tokio::test]
    async fn git_root_rejects_non_git_paths() {
        let dir = tempfile::tempdir().unwrap();

        let error = git_root_for(dir.path().to_str().unwrap())
            .await
            .unwrap_err();

        assert!(error.to_string().contains("Git work tree"));
    }

    #[tokio::test]
    async fn repository_source_uses_selected_git_root_without_custom_label() {
        let (_dir, db) = temp_db();
        let repo = init_git_repo();
        let src = repo.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        let repository = upsert_repository_source(&db, src.to_str().unwrap(), None)
            .await
            .unwrap();

        assert_eq!(
            repository.root_path,
            repo.path().canonicalize().unwrap().to_string_lossy()
        );
        assert_eq!(
            repository.label,
            repo.path().file_name().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn upserting_repository_keeps_one_record_per_root_path() {
        let (_dir, db) = temp_db();
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_string_lossy().to_string();

        let first = db.upsert_repository("repo-1", "First", &root_path).unwrap();
        let second = db
            .upsert_repository("repo-2", "Second", &root_path)
            .unwrap();
        let repositories = db.list_repositories().unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].label, "First");
    }

    #[test]
    fn deleting_repository_source_does_not_delete_run_history() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        let src = repo.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let repository = db
            .upsert_repository("repo-1", "Repo", &repo.path().to_string_lossy())
            .unwrap();

        let request = RunCreateRequest {
            target_path: Some(src.to_string_lossy().to_string()),
            repository_id: Some(repository.id.clone()),
            repository_root_path: Some(repo.path().to_string_lossy().to_string()),
            target_relative_path: Some("src".to_string()),
            rules: vec!["simplify-conditional".to_string()],
            rule_selection_plan: None,
            model: None,
            test_file_mode: Some(TestFileMode::ReadOnly),
            validation_commands: Vec::new(),
            protected_paths: Vec::new(),
            repair_budget: 2,
            expected_deterministic_preview_fingerprint: None,
            convention_snapshot: None,
        };
        db.insert_run("run-1", &request).unwrap();

        assert!(db.delete_repository(&repository.id).unwrap());
        let run = db.get_run("run-1").unwrap().unwrap();

        assert_eq!(run.repository_id.as_deref(), Some(repository.id.as_str()));
        assert_eq!(run.target_relative_path.as_deref(), Some("src"));
    }

    #[test]
    fn repository_mode_request_resolves_and_stores_absolute_target() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        let src = repo.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let repository = db
            .upsert_repository("repo-1", "Repo", &repo.path().to_string_lossy())
            .unwrap();

        let request = normalize_request(
            &db,
            RunCreateRequest {
                target_path: None,
                repository_id: Some(repository.id.clone()),
                repository_root_path: None,
                target_relative_path: Some("src".to_string()),
                rules: Vec::new(),
                rule_selection_plan: None,
                model: None,
                test_file_mode: None,
                validation_commands: Vec::new(),
                protected_paths: Vec::new(),
                repair_budget: 2,
                expected_deterministic_preview_fingerprint: None,
                convention_snapshot: None,
            },
        )
        .unwrap();
        db.insert_run("run-1", &request).unwrap();
        let run = db.get_run("run-1").unwrap().unwrap();

        assert_eq!(
            request.target_path.as_deref(),
            Some(src.canonicalize().unwrap().to_string_lossy().as_ref())
        );
        assert_eq!(run.repository_id.as_deref(), Some(repository.id.as_str()));
        assert_eq!(
            run.repository_root_path.as_deref(),
            Some(
                repo.path()
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(run.target_relative_path.as_deref(), Some("src"));
        assert_eq!(request.model.as_deref(), None);
        assert_eq!(run.model.as_deref(), None);
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

    #[test]
    fn repository_folder_resolution_rejects_traversal_syntax() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        let root = repo.path().canonicalize().unwrap();

        assert!(resolve_repository_folder(&root, "src/..").is_err());
        assert!(resolve_repository_folder(&root, "../outside").is_err());
        assert!(resolve_repository_folder(&root, "/tmp").is_err());
    }

    #[test]
    fn repository_file_preview_reads_safe_relative_file() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/app.ts"),
            "export const value: boolean = true;\n",
        );
        let repository = db
            .upsert_repository("repo-1", "Repo", &repo.path().to_string_lossy())
            .unwrap();

        let preview = repository_file_preview_response(&db, &repository.id, "src/app.ts").unwrap();

        assert_eq!(preview.repository_id, repository.id);
        assert_eq!(preview.relative_path, "src/app.ts");
        assert_eq!(preview.language, PreviewLanguage::TypeScript);
        assert_eq!(preview.content, "export const value: boolean = true;\n");
        assert_eq!(preview.size_bytes, 36);

        let json = serde_json::to_value(&preview).unwrap();
        assert_eq!(json["language"], "typescript");
    }

    #[test]
    fn repository_file_preview_detects_rust_files() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(&repo.path().join("src/lib.rs"), "pub fn value() {}\n");
        let repository = db
            .upsert_repository("repo-1", "Repo", &repo.path().to_string_lossy())
            .unwrap();

        let preview = repository_file_preview_response(&db, &repository.id, "src/lib.rs").unwrap();

        assert_eq!(preview.language, PreviewLanguage::Rust);
    }

    #[test]
    fn repository_file_preview_rejects_traversal_syntax() {
        let repo = tempfile::tempdir().unwrap();
        write_file(&repo.path().join("src/app.ts"), "");
        let root = repo.path().canonicalize().unwrap();

        assert!(resolve_repository_file(&root, "../outside.ts").is_err());
        assert!(resolve_repository_file(&root, "/tmp/outside.ts").is_err());
    }

    #[test]
    fn repository_file_preview_rejects_directories() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        let root = repo.path().canonicalize().unwrap();

        let error = resolve_repository_file(&root, "src").unwrap_err();

        assert!(error.to_string().contains("target must be a file"));
    }

    #[cfg(unix)]
    #[test]
    fn repository_file_preview_rejects_symlink_files() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join("linked.ts")).unwrap();
        let root = repo.path().canonicalize().unwrap();

        let error = resolve_repository_file(&root, "linked.ts").unwrap_err();

        assert!(error.to_string().contains("does not follow symlinks"));
    }

    #[cfg(unix)]
    #[test]
    fn repository_file_preview_rejects_files_outside_root_after_canonicalization() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write_file(
            &outside.path().join("app.ts"),
            "export const outside = true;\n",
        );
        std::os::unix::fs::symlink(outside.path(), repo.path().join("linked")).unwrap();
        let root = repo.path().canonicalize().unwrap();

        let error = resolve_repository_file(&root, "linked/app.ts").unwrap_err();

        assert!(error.to_string().contains("must stay inside"));
    }

    #[test]
    fn repository_file_preview_rejects_large_files() {
        let (_dir, db) = temp_db();
        let repo = tempfile::tempdir().unwrap();
        write_file(
            &repo.path().join("src/large.ts"),
            &"a".repeat((MAX_FILE_PREVIEW_BYTES + 1) as usize),
        );
        let repository = db
            .upsert_repository("repo-1", "Repo", &repo.path().to_string_lossy())
            .unwrap();

        let error =
            repository_file_preview_response(&db, &repository.id, "src/large.ts").unwrap_err();

        assert!(matches!(error, FilePreviewError::TooLarge(_)));
    }

    #[test]
    fn folder_entries_hide_noisy_directories() {
        let repo = tempfile::tempdir().unwrap();
        for name in [
            ".git",
            "node_modules",
            "dist",
            "build",
            ".next",
            "coverage",
            "src",
        ] {
            std::fs::create_dir_all(repo.path().join(name)).unwrap();
        }
        let root = repo.path().canonicalize().unwrap();

        let entries = folder_entries(&root, &root).unwrap();
        let names = entries
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["src"]);
    }

    #[cfg(unix)]
    #[test]
    fn folder_entries_do_not_follow_symlink_directories() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join("linked")).unwrap();
        let root = repo.path().canonicalize().unwrap();

        let entries = folder_entries(&root, &root).unwrap();
        let names = entries
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["src"]);
    }
}
