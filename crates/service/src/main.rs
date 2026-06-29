mod analyzer;
mod db;
mod diff;
mod model_provider;
mod validation;

use analyzer::{AnalyzerRequest, AnalyzerResponse};
use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
};
use db::{Database, PatchRecord};
use local_refactor_core::{
    config::{
        default_protected_paths, find_project_config, load_config_file, ConfigLayer,
        EffectiveConfig, TestFileMode,
    },
    path_policy::{PathDecision, PathPolicy},
    rules::INITIAL_RULES,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::broadcast;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: Arc<Database>,
    analyzer_script: PathBuf,
    ollama: model_provider::OllamaProvider,
    events: broadcast::Sender<db::RunEvent>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunCreateRequest {
    target_path: String,
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
struct DiffResponse {
    run_id: String,
    files: Vec<FileDiff>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileDiff {
    file_path: String,
    diff: String,
}

fn default_repair_budget() -> u32 {
    2
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "local_refactor_service=info,tower_http=info".into()),
        )
        .init();

    let db = Arc::new(Database::open(default_db_path()?)?);
    let analyzer_script = find_analyzer_script()?;
    let (events, _) = broadcast::channel(256);
    let state = AppState {
        db,
        analyzer_script,
        ollama: model_provider::OllamaProvider::from_env(),
        events,
    };

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/rules", get(rules))
        .route("/api/models", get(models))
        .route("/api/config/effective", get(effective_config))
        .route("/api/analyze", post(analyze))
        .route("/api/runs", post(create_run).get(list_runs))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/runs/{id}/cancel", post(cancel_run))
        .route("/api/runs/{id}/revert", post(revert_run))
        .route("/api/runs/{id}/events", get(run_events))
        .route("/api/runs/{id}/diff", get(run_diff))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 7373));
    tracing::info!("local-refactor service listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn rules() -> Json<RulesResponse<'static>> {
    Json(RulesResponse {
        rules: INITIAL_RULES,
    })
}

async fn models(State(state): State<AppState>) -> impl IntoResponse {
    match state.ollama.list_models().await {
        Ok(models) => Json(models).into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "provider": "ollama",
                "models": [],
                "error": error.to_string()
            })),
        )
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

async fn analyze(
    State(state): State<AppState>,
    Json(request): Json<RunCreateRequest>,
) -> impl IntoResponse {
    let request = match normalize_request(request) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };

    match prepare_analyzer_request(&request).and_then(|prepared| {
        Ok((
            prepared.0,
            AnalyzerRequest {
                files: prepared.1,
                rules: effective_rules(&request),
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

async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<RunCreateRequest>,
) -> impl IntoResponse {
    let id = Uuid::new_v4().to_string();
    let request = match normalize_request(request) {
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
                if let Err(error) = run_job(state_for_job, id_for_job.clone(), request).await {
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

async fn list_runs(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.list_runs() {
        Ok(runs) => Json(runs).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn get_run(
    State(state): State<AppState>,
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
    State(state): State<AppState>,
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
    State(state): State<AppState>,
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
    State(state): State<AppState>,
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
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    match state.db.patches_for_run(&id) {
        Ok(patches) => Json(DiffResponse {
            run_id: id,
            files: patches
                .into_iter()
                .map(|patch| FileDiff {
                    file_path: patch.file_path.clone(),
                    diff: diff::unified(
                        &patch.file_path,
                        &patch.original_content,
                        &patch.new_content,
                    ),
                })
                .collect(),
        })
        .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn run_job(state: AppState, id: String, request: RunCreateRequest) -> Result<()> {
    transition(
        &state,
        &id,
        "analyzing",
        "Collecting mutable TypeScript files",
    )?;
    let (policy, files) = prepare_analyzer_request(&request)?;

    transition(
        &state,
        &id,
        "planning",
        "Running TypeScript analyzer worker",
    )?;
    let analyzer_response = analyzer::run(
        &state.analyzer_script,
        AnalyzerRequest {
            files,
            rules: effective_rules(&request),
        },
    )
    .await?;

    apply_analyzer_edits(&state, &id, &policy, analyzer_response)?;

    transition(&state, &id, "validating", "Running validation checks")?;
    let validation_dir = validation_root(&request.target_path);
    let validation_result =
        validation::run_commands(&validation_dir, &request.validation_commands).await?;
    state
        .db
        .set_validation_output(&id, &validation_result.output)?;

    if validation_result.success {
        transition(&state, &id, "succeeded", "Run completed successfully")?;
        return Ok(());
    }

    transition(
        &state,
        &id,
        "repairing",
        "Validation failed; deterministic V1 cannot repair yet, reverting run changes",
    )?;
    revert_patches(&state, &id)?;
    state.db.set_error(
        &id,
        &format!("validation failed: {}", validation_result.output),
    )?;
    transition(
        &state,
        &id,
        "failed",
        "Run failed and changes were reverted",
    )?;
    Ok(())
}

fn apply_analyzer_edits(
    state: &AppState,
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
        })?;
        std::fs::write(&file_path, edit.new_content)
            .with_context(|| format!("failed to write {}", edit.file_path))?;
        state.db.append_event(
            run_id,
            &format!("Applied {} to {}", edit.rule_id, edit.file_path),
        )?;
    }

    Ok(())
}

fn prepare_analyzer_request(request: &RunCreateRequest) -> Result<(PathPolicy, Vec<String>)> {
    let target_path = std::fs::canonicalize(&request.target_path)
        .with_context(|| format!("target path does not exist: {}", request.target_path))?;
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
        .mutable_source_files()?
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

fn normalize_request(mut request: RunCreateRequest) -> Result<RunCreateRequest> {
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

    let config = effective_config_for(Path::new(&request.target_path), run_layer)?;
    request.rules = config.rules;
    request.protected_paths = config.protected_paths;
    request.validation_commands = config.validation_commands;
    request.test_file_mode = Some(config.test_file_mode);
    Ok(request)
}

fn effective_config_for(target_path: &Path, run_layer: ConfigLayer) -> Result<EffectiveConfig> {
    let mut config = EffectiveConfig::default();

    if let Some(global_path) = global_config_path().filter(|path| path.exists()) {
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

fn validation_root(target_path: &str) -> PathBuf {
    let path = PathBuf::from(target_path);
    if path.is_file() {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        path
    }
}

fn transition(state: &AppState, run_id: &str, status: &str, message: &str) -> Result<()> {
    state.db.update_status(run_id, status)?;
    let event = state.db.append_event(run_id, message)?;
    let _ = state.events.send(event);
    Ok(())
}

fn revert_patches(state: &AppState, run_id: &str) -> Result<()> {
    let patches = state.db.patches_for_run(run_id)?;
    for patch in patches.into_iter().rev() {
        std::fs::write(&patch.file_path, patch.original_content)
            .with_context(|| format!("failed to revert {}", patch.file_path))?;
    }
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
