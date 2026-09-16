use crate::{db, Database, ServiceState};
use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
    Json, Router,
};
use local_refactor_core::{
    config::{find_project_config, load_config_file},
    conventions::{ConventionSettings, PartialConventionSettings},
};
use serde::{Deserialize, Serialize};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tokio::process::Command;
use uuid::Uuid;

const MAX_FILE_PREVIEW_BYTES: u64 = 512 * 1024;

pub(crate) fn routes() -> Router<ServiceState> {
    Router::new()
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
        .map_err(FilePreviewError::Internal)?
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

pub(crate) fn resolve_repository_folder(root: &Path, relative_path: &str) -> Result<PathBuf> {
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

pub(crate) fn relative_path_from_root(root: &Path, path: &Path) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_intake::normalize_request;
    use crate::RunCreateRequest;
    use local_refactor_core::config::TestFileMode;
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
            run_kind: crate::RunKind::Refactoring,
            source_coverage_run_id: None,
            coverage_evidence: Vec::new(),
            behavior_claims: Vec::new(),
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
                run_kind: crate::RunKind::Refactoring,
                source_coverage_run_id: None,
                coverage_evidence: Vec::new(),
                behavior_claims: Vec::new(),
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
