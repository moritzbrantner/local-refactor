mod analyzer;
mod db;
mod diff;
mod model_provider;
mod patch_plan;
mod validation;

use analyzer::{AnalyzerRequest, AnalyzerResponse};
use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Sse},
    routing::{get, patch, post},
    Json, Router,
};
use db::{PatchAction, PatchRecord};
use local_refactor_core::{
    config::{
        default_protected_paths, find_project_config, load_config_file, ConfigLayer,
        EffectiveConfig, TestFileMode,
    },
    path_policy::{PathDecision, PathPolicy},
    rules::{rule_by_id, Language, RuleExecutionKind, INITIAL_RULES},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    future::Future,
    io::ErrorKind,
    net::SocketAddr,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Instant,
};
use tokio::process::Command;
use tokio::sync::broadcast;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

pub use db::{Database, RunEvent, RunMetrics};
pub use model_provider::{ModelDownloadProgress, ModelSummary, ModelsResponse};
pub use patch_plan::{PatchPlanEdit, PatchPlanModelRequest, PatchPlanSourceFile};

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
    model: Option<String>,
    #[serde(default)]
    test_file_mode: Option<TestFileMode>,
    #[serde(default)]
    validation_commands: Vec<String>,
    #[serde(default)]
    protected_paths: Vec<String>,
    #[serde(default = "default_repair_budget")]
    repair_budget: u32,
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
struct RepositoryPickResponse {
    repository: Option<db::RepositoryRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderChildrenQuery {
    #[serde(default)]
    path: Option<String>,
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
        .route("/api/analyze", post(analyze))
        .route("/api/runs", post(create_run).get(list_runs))
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

    match prepare_source_request(&request, Language::TypeScript).and_then(|prepared| {
        Ok((
            prepared.0,
            AnalyzerRequest {
                files: prepared.1,
                rules: analyzer_rules,
            },
        ))
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
            tokio::spawn(async move {
                let failure_state = state_for_job.clone();
                if let Err(error) = run_job(state_for_job, id_for_job.clone(), request).await {
                    tracing::error!(run_id = id_for_job, error = %error, "run failed");
                    let _ = failure_state.db.set_error(&id_for_job, &error.to_string());
                    let _ = transition(
                        &failure_state,
                        &id_for_job,
                        "failed",
                        &format!("Run failed: {error}"),
                    );
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
    match transition(&state, &id, "cancelled", "Run cancelled by user") {
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
    match revert_patches(&state, &id) {
        Ok(()) => {
            let _ = transition(&state, &id, "reverted", "Run changes reverted");
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

async fn run_job(state: ServiceState, id: String, request: RunCreateRequest) -> Result<()> {
    let started = Instant::now();
    let mut metrics = RunMetrics::default();
    let result = run_job_inner(&state, &id, &request, &mut metrics).await;
    metrics.total_run_ms = Some(elapsed_ms(started));
    if let Err(error) = state.db.set_run_metrics(&id, &metrics) {
        tracing::warn!(run_id = id, error = %error, "failed to persist run metrics");
    }
    result
}

async fn run_job_inner(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    metrics: &mut RunMetrics,
) -> Result<()> {
    let model = selected_model(&request)?;
    let rules = effective_rules(&request);
    transition(
        state,
        id,
        "preparing",
        &format!("Ensuring local model {model} is downloaded"),
    )?;
    let progress_state = state.clone();
    let progress_run_id = id.to_string();
    let started = Instant::now();
    let ensure_result = state
        .model_gateway
        .ensure_model_available_with_progress(
            &model,
            Arc::new(move |progress| {
                let _ = append_run_event(&progress_state, &progress_run_id, &progress.message);
            }),
        )
        .await;
    metrics.model_ensure_available_ms = Some(elapsed_ms(started));
    ensure_result?;

    for rule_id in rules {
        let rule =
            rule_by_id(&rule_id).ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))?;
        transition(
            state,
            id,
            "analyzing",
            &format!("Collecting mutable {} files", rule.language.display_name()),
        )?;
        let started = Instant::now();
        let prepared = prepare_source_request(request, rule.language);
        add_elapsed_ms(&mut metrics.file_collection_ms, started);
        let (policy, files) = prepared?;

        match rule.execution_kind {
            RuleExecutionKind::Deterministic => {
                if rule.language != Language::TypeScript {
                    return Err(anyhow!(
                        "deterministic analyzer rules are only available for TypeScript"
                    ));
                }
                transition(
                    state,
                    id,
                    "planning",
                    &format!("Running TypeScript analyzer worker for {}", rule.id),
                )?;
                let started = Instant::now();
                let analyzer_response = analyzer::run(
                    &state.analyzer_script,
                    AnalyzerRequest {
                        files,
                        rules: vec![rule.id.to_string()],
                    },
                )
                .await;
                add_elapsed_ms(&mut metrics.analyzer_planning_ms, started);
                let analyzer_response = analyzer_response?;

                let started = Instant::now();
                let edit_result = apply_analyzer_edits(state, id, &policy, analyzer_response);
                add_elapsed_ms(&mut metrics.edit_application_ms, started);
                edit_result?;
            }
            RuleExecutionKind::ModelPlanned => {
                transition(
                    state,
                    id,
                    "planning",
                    &format!("Requesting model patch plan for {}", rule.id),
                )?;
                let patch_request = patch_plan_request(rule, &request, &policy, &files)?;
                let started = Instant::now();
                let model_response = state
                    .model_gateway
                    .generate_patch_plan(&model, patch_request.clone())
                    .await;
                add_elapsed_ms(&mut metrics.model_planning_ms, started);
                let model_response = model_response?;

                let started = Instant::now();
                let edits =
                    patch_plan::parse_patch_plan_response(&patch_request, &policy, &model_response);
                add_elapsed_ms(&mut metrics.patch_plan_validation_ms, started);
                let edits = edits?;

                let started = Instant::now();
                let edit_result = apply_patch_plan_edits(state, id, &policy, edits);
                add_elapsed_ms(&mut metrics.edit_application_ms, started);
                edit_result?;
            }
        }
    }

    transition(state, id, "validating", "Running validation checks")?;
    let validation_dir = validation_root(request_target_path(&request)?);
    let started = Instant::now();
    let validation_result =
        validation::run_commands(&validation_dir, &request.validation_commands).await;
    metrics.validation_ms = Some(elapsed_ms(started));
    let validation_result = validation_result?;
    state
        .db
        .set_validation_output(id, &validation_result.output)?;

    if validation_result.success {
        transition(state, id, "succeeded", "Run completed successfully")?;
        return Ok(());
    }

    transition(
        state,
        id,
        "repairing",
        "Validation failed; deterministic V1 cannot repair yet, reverting run changes",
    )?;
    revert_patches(state, id)?;
    state.db.set_error(
        id,
        &format!("validation failed: {}", validation_result.output),
    )?;
    transition(state, id, "failed", "Run failed and changes were reverted")?;
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn add_elapsed_ms(slot: &mut Option<u64>, started: Instant) {
    let elapsed = elapsed_ms(started);
    *slot = Some(slot.unwrap_or(0).saturating_add(elapsed));
}

fn apply_analyzer_edits(
    state: &ServiceState,
    run_id: &str,
    policy: &PathPolicy,
    response: AnalyzerResponse,
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
        "editing",
        "Applying deterministic analyzer edits",
    )?;

    for edit in response.edits {
        let file_path = PathBuf::from(&edit.file_path);
        if policy.decision_for(&file_path) != PathDecision::Mutable {
            return Err(anyhow!(
                "analyzer attempted to edit non-mutable path {}",
                edit.file_path
            ));
        }

        let current = std::fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read {}", edit.file_path))?;
        if current != edit.original_content {
            return Err(anyhow!(
                "external modification conflict while editing {}",
                edit.file_path
            ));
        }

        state.db.insert_patch(PatchRecord {
            run_id: run_id.to_string(),
            file_path: edit.file_path.clone(),
            original_content: edit.original_content.clone(),
            new_content: edit.new_content.clone(),
            rule_id: edit.rule_id.clone(),
            summary: edit.summary.clone(),
            action: PatchAction::Update,
        })?;
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&file_path, edit.new_content)
            .with_context(|| format!("failed to write {}", edit.file_path))?;
        state.db.append_event(
            run_id,
            &format!("Applied {} to {}", edit.rule_id, edit.file_path),
        )?;
    }

    Ok(())
}

fn apply_patch_plan_edits(
    state: &ServiceState,
    run_id: &str,
    policy: &PathPolicy,
    edits: Vec<PatchPlanEdit>,
) -> Result<()> {
    if edits.is_empty() {
        state
            .db
            .append_event(run_id, "Patch plan produced no edits")?;
        return Ok(());
    }

    transition(state, run_id, "editing", "Applying model patch-plan edits")?;

    for edit in edits {
        let file_path = PathBuf::from(&edit.file_path);
        if policy.decision_for(&file_path) != PathDecision::Mutable {
            return Err(anyhow!(
                "patch plan attempted to edit non-mutable path {}",
                edit.file_path
            ));
        }

        match edit.action {
            PatchAction::Update => {
                let current = std::fs::read_to_string(&file_path)
                    .with_context(|| format!("failed to read {}", edit.file_path))?;
                if current != edit.original_content {
                    return Err(anyhow!(
                        "external modification conflict while editing {}",
                        edit.file_path
                    ));
                }
            }
            PatchAction::Create => {
                if file_path.exists() {
                    return Err(anyhow!("create target already exists: {}", edit.file_path));
                }
            }
        }

        state.db.insert_patch(PatchRecord {
            run_id: run_id.to_string(),
            file_path: edit.file_path.clone(),
            original_content: edit.original_content.clone(),
            new_content: edit.new_content.clone(),
            rule_id: edit.rule_id.clone(),
            summary: edit.summary.clone(),
            action: edit.action,
        })?;
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&file_path, edit.new_content)
            .with_context(|| format!("failed to write {}", edit.file_path))?;
        state.db.append_event(
            run_id,
            &format!("Applied {} to {}", edit.rule_id, edit.file_path),
        )?;
    }

    Ok(())
}

fn patch_plan_request(
    rule: &local_refactor_core::rules::RuleDefinition,
    run_request: &RunCreateRequest,
    policy: &PathPolicy,
    files: &[String],
) -> Result<PatchPlanModelRequest> {
    let sources = files
        .iter()
        .map(|file| {
            let path = PathBuf::from(file);
            let relative = path
                .strip_prefix(policy.target_root())
                .with_context(|| format!("{} is outside target root", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            Ok(PatchPlanSourceFile {
                relative_path: relative,
                content,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(PatchPlanModelRequest {
        rule_id: rule.id.to_string(),
        language: rule.language,
        rule_name: rule.name.to_string(),
        rule_description: rule.description.to_string(),
        target_root: policy.target_root().to_path_buf(),
        files: sources,
        validation_commands: run_request.validation_commands.clone(),
        allowed_writes: rule.allowed_writes,
    })
}

fn prepare_source_request(
    request: &RunCreateRequest,
    language: Language,
) -> Result<(PathPolicy, Vec<String>)> {
    let target_path = request_target_path(request)?;
    let target_path = std::fs::canonicalize(target_path)
        .with_context(|| format!("target path does not exist: {target_path}"))?;
    let target_root = if target_path.is_file() {
        target_path
            .parent()
            .ok_or_else(|| anyhow!("target file has no parent directory"))?
            .to_path_buf()
    } else {
        target_path
    };

    let mut protected = default_protected_paths();
    protected.extend(request.protected_paths.iter().cloned());

    let policy = PathPolicy::new(
        target_root,
        &protected,
        request.test_file_mode.unwrap_or_default(),
    )?;
    let files = policy
        .mutable_source_files(language)?
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();

    Ok((policy, files))
}

fn effective_rules(request: &RunCreateRequest) -> Vec<String> {
    if request.rules.is_empty() {
        return vec!["simplify-conditional".to_string()];
    }

    request
        .rules
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_request(db: &Database, mut request: RunCreateRequest) -> Result<RunCreateRequest> {
    resolve_request_target(db, &mut request)?;
    let run_layer = ConfigLayer {
        rules: if request.rules.is_empty() {
            None
        } else {
            Some(request.rules.clone())
        },
        protected_paths: if request.protected_paths.is_empty() {
            None
        } else {
            Some(request.protected_paths.clone())
        },
        validation_commands: if request.validation_commands.is_empty() {
            None
        } else {
            Some(request.validation_commands.clone())
        },
        test_file_mode: request.test_file_mode,
    };

    let target_path = request_target_path(&request)?;
    let config = effective_config_for(Path::new(target_path), run_layer)?;
    request.rules = config.rules;
    request.protected_paths = config.protected_paths;
    request.validation_commands = config.validation_commands;
    request.test_file_mode = Some(config.test_file_mode);
    request.model = Some(selected_model(&request)?);
    Ok(request)
}

fn selected_model(request: &RunCreateRequest) -> Result<String> {
    let requested_model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model = match requested_model {
        Some(model) => model,
        None => model_provider::default_model_name(),
    };

    if !model_provider::is_supported_model(model) {
        return Err(anyhow!("unsupported local coding model: {model}"));
    }

    Ok(model.to_string())
}

fn resolve_request_target(db: &Database, request: &mut RunCreateRequest) -> Result<()> {
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
        let target = resolve_repository_folder(&root, relative)?;
        request.target_path = Some(target.to_string_lossy().to_string());
        request.repository_root_path = Some(root.to_string_lossy().to_string());
        request.target_relative_path = Some(relative_path_from_root(&root, &target));
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

fn request_target_path(request: &RunCreateRequest) -> Result<&str> {
    request
        .target_path
        .as_deref()
        .ok_or_else(|| anyhow!("run target path was not resolved"))
}

fn effective_config_for(target_path: &Path, run_layer: ConfigLayer) -> Result<EffectiveConfig> {
    effective_config_for_paths(
        target_path,
        run_layer,
        default_refactor_rules_path(),
        global_config_path(),
    )
}

fn effective_config_for_paths(
    target_path: &Path,
    run_layer: ConfigLayer,
    default_path: Option<PathBuf>,
    global_path: Option<PathBuf>,
) -> Result<EffectiveConfig> {
    let mut config = EffectiveConfig::default();

    if let Some(default_path) = default_path.filter(|path| path.exists()) {
        let default_layer = load_config_file(&default_path)?;
        config.apply_layer(ConfigLayer {
            rules: default_layer.rules,
            ..ConfigLayer::default()
        });
    }

    if let Some(global_path) = global_path.filter(|path| path.exists()) {
        config.apply_layer(load_config_file(&global_path)?);
    }

    if let Some(project_config) = find_project_config(target_path) {
        config.apply_layer(load_config_file(&project_config)?);
    }

    if config.validation_commands.is_empty() {
        config.validation_commands = validation::detect_commands(target_path);
    }

    config.apply_layer(run_layer);
    Ok(config)
}

fn global_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("local-refactor/config.toml"))
}

fn default_refactor_rules_path() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(|workspace_root| workspace_root.join("refactor-rules.toml"))
}

fn validation_root(target_path: &str) -> PathBuf {
    let path = PathBuf::from(target_path);
    if path.is_file() {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        path
    }
}

fn transition(state: &ServiceState, run_id: &str, status: &str, message: &str) -> Result<()> {
    state.db.update_status(run_id, status)?;
    append_run_event(state, run_id, message)
}

fn append_run_event(state: &ServiceState, run_id: &str, message: &str) -> Result<()> {
    let event = state.db.append_event(run_id, message)?;
    let _ = state.events.send(event);
    Ok(())
}

fn revert_patches(state: &ServiceState, run_id: &str) -> Result<()> {
    let patches = state.db.patches_for_run(run_id)?;
    for patch in patches.into_iter().rev() {
        match patch.action {
            PatchAction::Update => {
                std::fs::write(&patch.file_path, patch.original_content)
                    .with_context(|| format!("failed to revert {}", patch.file_path))?;
            }
            PatchAction::Create => match std::fs::remove_file(&patch.file_path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to remove {}", patch.file_path));
                }
            },
        }
    }
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
            model: None,
            test_file_mode: Some(TestFileMode::ReadOnly),
            validation_commands: Vec::new(),
            protected_paths: Vec::new(),
            repair_budget: 2,
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
                model: None,
                test_file_mode: None,
                validation_commands: Vec::new(),
                protected_paths: Vec::new(),
                repair_budget: 2,
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
        assert_eq!(
            request.model.as_deref(),
            Some(model_provider::default_model_name())
        );
        assert_eq!(
            run.model.as_deref(),
            Some(model_provider::default_model_name())
        );
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
                rules: Vec::new(),
                model: Some("unknown-model:latest".to_string()),
                test_file_mode: None,
                validation_commands: Vec::new(),
                protected_paths: Vec::new(),
                repair_budget: 2,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("unsupported local coding model"));
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
