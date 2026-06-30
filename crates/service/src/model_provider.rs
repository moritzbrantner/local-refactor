use crate::patch_plan::{build_prompt, PatchPlanModelRequest};
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[derive(Clone)]
pub struct OllamaProvider {
    base_url: Url,
    client: reqwest::Client,
    command: OsString,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsResponse {
    pub provider: &'static str,
    pub models: Vec<ModelSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub name: String,
    pub label: &'static str,
    pub description: &'static str,
    pub downloaded: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Debug, Serialize)]
struct OllamaPullRequest<'a> {
    model: &'a str,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
    format: &'static str,
    options: OllamaGenerateOptions,
}

#[derive(Debug, Serialize)]
struct OllamaGenerateOptions {
    temperature: u8,
    num_predict: u16,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaErrorResponse {
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaPullProgress {
    status: Option<String>,
    digest: Option<String>,
    total: Option<u64>,
    completed: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDownloadProgress {
    pub message: String,
}

const LOCAL_CODING_MODELS: &[LocalCodingModel] = &[
    LocalCodingModel {
        name: "qwen2.5-coder:7b",
        label: "Qwen2.5 Coder 7B",
        description: "Balanced local coding model for everyday refactors.",
    },
    LocalCodingModel {
        name: "deepseek-coder:6.7b",
        label: "DeepSeek Coder 6.7B",
        description: "Alternative coding model with stronger code-completion bias.",
    },
];

#[derive(Debug, Clone, Copy)]
struct LocalCodingModel {
    name: &'static str,
    label: &'static str,
    description: &'static str,
}

impl OllamaProvider {
    pub fn from_env() -> Self {
        let base_url = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
        Self::with_base_url_and_command(
            base_url.parse().expect("default Ollama URL is valid"),
            ollama_command_from_env(),
        )
    }

    #[allow(dead_code)]
    pub fn with_base_url(base_url: Url) -> Self {
        Self::with_base_url_and_command(base_url, "ollama")
    }

    pub fn with_base_url_and_command(base_url: Url, command: impl Into<OsString>) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
            command: command.into(),
        }
    }

    pub async fn list_models(&self) -> Result<ModelsResponse> {
        let installed = self.installed_model_names().await?;

        Ok(ModelsResponse {
            provider: "ollama",
            models: LOCAL_CODING_MODELS
                .iter()
                .map(|model| ModelSummary {
                    name: model.name.to_string(),
                    label: model.label,
                    description: model.description,
                    downloaded: installed.contains(model.name),
                })
                .collect(),
        })
    }

    #[allow(dead_code)]
    pub async fn ensure_model_available(&self, name: &str) -> Result<()> {
        self.ensure_model_available_with_progress(name, |_| {})
            .await
    }

    pub async fn ensure_model_available_with_progress(
        &self,
        name: &str,
        mut on_progress: impl FnMut(ModelDownloadProgress),
    ) -> Result<()> {
        if !is_supported_model(name) {
            return Err(anyhow!("unsupported local coding model: {name}"));
        }

        match self.installed_model_names().await {
            Ok(installed) if installed.contains(name) => Ok(()),
            Ok(_) => self.pull_model(name, &mut on_progress).await,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    model = name,
                    "could not list Ollama models; attempting to start Ollama"
                );
                let start_error = if let Err(start_error) = self.start_local_ollama().await {
                    tracing::warn!(
                        error = %start_error,
                        model = name,
                        "could not start Ollama automatically; attempting download anyway"
                    );
                    Some(format!("{start_error:#}"))
                } else {
                    None
                };

                match self.pull_model(name, &mut on_progress).await {
                    Ok(()) => Ok(()),
                    Err(download_error) => {
                        let startup_context = start_error
                            .map(|start_error| {
                                format!("; automatic Ollama startup failed ({start_error})")
                            })
                            .unwrap_or_default();
                        Err(download_error).with_context(|| {
                            format!(
                                "Ollama model list was unavailable ({error}){startup_context}; failed to download {name}"
                            )
                        })
                    }
                }
            }
        }
    }

    pub async fn generate_patch_plan(
        &self,
        model: &str,
        request: PatchPlanModelRequest,
    ) -> Result<String> {
        if !is_supported_model(model) {
            return Err(anyhow!("unsupported local coding model: {model}"));
        }

        let url = self.base_url.join("/api/generate")?;
        let response = self
            .client
            .post(url)
            .json(&OllamaGenerateRequest {
                model,
                prompt: build_prompt(&request),
                stream: false,
                format: "json",
                options: OllamaGenerateOptions {
                    temperature: 0,
                    num_predict: 2200,
                },
            })
            .send()
            .await
            .with_context(|| format!("failed to request patch plan from {model}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let detail = response
                .json::<OllamaErrorResponse>()
                .await
                .ok()
                .and_then(|body| body.error)
                .unwrap_or_else(|| "Ollama returned an error".to_string());
            return Err(anyhow!(
                "failed to generate patch plan with {model}: {detail} ({status})"
            ));
        }

        let body = response
            .json::<OllamaGenerateResponse>()
            .await
            .with_context(|| "failed to decode Ollama patch-plan response")?;
        if let Some(error) = body.error {
            return Err(anyhow!(
                "failed to generate patch plan with {model}: {error}"
            ));
        }
        body.response
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("Ollama returned an empty patch-plan response"))
    }

    async fn start_local_ollama(&self) -> Result<()> {
        if !is_localhost_url(&self.base_url) {
            return Err(anyhow!(
                "automatic Ollama startup is only supported for localhost URLs"
            ));
        }

        Command::new(&self.command)
            .arg("serve")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                anyhow!(
                    "failed to start {}: {error}. Install Ollama, start it manually, or set OLLAMA_COMMAND to the Ollama executable path",
                    self.command.to_string_lossy()
                )
            })?;

        self.wait_for_ollama().await
    }

    async fn wait_for_ollama(&self) -> Result<()> {
        let mut last_error = None;
        for _ in 0..40 {
            match self
                .client
                .get(self.base_url.join("/api/tags")?)
                .send()
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        match last_error {
            Some(error) => Err(error).with_context(|| "Ollama did not become reachable"),
            None => Err(anyhow!("Ollama did not become reachable")),
        }
    }

    async fn installed_model_names(&self) -> Result<BTreeSet<String>> {
        let url = self.base_url.join("/api/tags")?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .with_context(|| "failed to connect to Ollama")?
            .error_for_status()
            .with_context(|| "Ollama returned an error")?
            .json::<OllamaTagsResponse>()
            .await
            .with_context(|| "failed to decode Ollama model list")?;

        Ok(response
            .models
            .into_iter()
            .map(|model| model.name)
            .collect())
    }

    async fn pull_model(
        &self,
        name: &str,
        on_progress: &mut impl FnMut(ModelDownloadProgress),
    ) -> Result<()> {
        let url = self.base_url.join("/api/pull")?;
        let response = self
            .client
            .post(url)
            .json(&OllamaPullRequest {
                model: name,
                stream: true,
            })
            .send()
            .await
            .with_context(|| format!("failed to ask Ollama to download {name}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let detail = response
                .json::<OllamaErrorResponse>()
                .await
                .ok()
                .and_then(|body| body.error)
                .unwrap_or_else(|| "Ollama returned an error".to_string());
            return Err(anyhow!("failed to download {name}: {detail} ({status})"));
        }

        let mut buffer = Vec::new();
        let mut last_message = None;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(
                &chunk.with_context(|| format!("failed while downloading {name}"))?,
            );

            while let Some(newline_index) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = buffer.drain(..=newline_index).collect::<Vec<_>>();
                handle_pull_progress_line(name, &line, &mut last_message, on_progress)?;
            }
        }

        if !buffer.is_empty() {
            handle_pull_progress_line(name, &buffer, &mut last_message, on_progress)?;
        }

        Ok(())
    }
}

fn handle_pull_progress_line(
    model: &str,
    line: &[u8],
    last_message: &mut Option<String>,
    on_progress: &mut impl FnMut(ModelDownloadProgress),
) -> Result<()> {
    let line = String::from_utf8_lossy(line).trim().to_string();
    if line.is_empty() {
        return Ok(());
    }

    let progress = serde_json::from_str::<OllamaPullProgress>(&line)
        .with_context(|| "failed to decode Ollama download progress")?;
    if let Some(error) = progress.error {
        return Err(anyhow!("failed to download {model}: {error}"));
    }

    let Some(message) = format_pull_progress(model, &progress) else {
        return Ok(());
    };

    if last_message.as_deref() == Some(message.as_str()) {
        return Ok(());
    }

    *last_message = Some(message.clone());
    on_progress(ModelDownloadProgress { message });
    Ok(())
}

fn format_pull_progress(model: &str, progress: &OllamaPullProgress) -> Option<String> {
    let status = progress.status.as_deref()?;
    match (progress.completed, progress.total) {
        (Some(completed), Some(total)) if total > 0 => {
            let percent = completed.saturating_mul(100) / total;
            Some(format!(
                "Downloading {model}: {percent}% ({}/{}){}",
                format_bytes(completed),
                format_bytes(total),
                progress
                    .digest
                    .as_deref()
                    .and_then(short_digest)
                    .map(|digest| format!(" {digest}"))
                    .unwrap_or_default()
            ))
        }
        _ => Some(format!("{model}: {status}")),
    }
}

fn short_digest(digest: &str) -> Option<&str> {
    digest
        .rsplit_once(':')
        .map(|(_, value)| value)
        .or(Some(digest))
        .map(|value| &value[..value.len().min(12)])
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let value = bytes as f64;
    if value >= GIB {
        format!("{:.1} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub fn default_model_name() -> &'static str {
    LOCAL_CODING_MODELS[0].name
}

pub fn configured_model_summaries() -> Vec<ModelSummary> {
    LOCAL_CODING_MODELS
        .iter()
        .map(|model| ModelSummary {
            name: model.name.to_string(),
            label: model.label,
            description: model.description,
            downloaded: false,
        })
        .collect()
}

pub fn is_supported_model(name: &str) -> bool {
    LOCAL_CODING_MODELS.iter().any(|model| model.name == name)
}

fn is_localhost_url(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn ollama_command_from_env() -> OsString {
    if let Some(command) = std::env::var_os("OLLAMA_COMMAND") {
        return command;
    }

    find_ollama_command().unwrap_or_else(|| "ollama".into())
}

fn find_ollama_command() -> Option<OsString> {
    if command_on_path("ollama") {
        return Some("ollama".into());
    }

    common_ollama_paths()
        .into_iter()
        .find(|path| path.exists())
        .map(PathBuf::into_os_string)
}

fn command_on_path(command: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&path_var).any(|dir| {
        let candidate = dir.join(command);
        candidate.is_file()
    })
}

fn common_ollama_paths() -> Vec<PathBuf> {
    [
        "/usr/local/bin/ollama",
        "/opt/homebrew/bin/ollama",
        "/usr/bin/ollama",
        "/snap/bin/ollama",
        "/Applications/Ollama.app/Contents/Resources/ollama",
    ]
    .into_iter()
    .map(Path::new)
    .map(Path::to_path_buf)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::StatusCode,
        routing::{get, post},
        Json, Router,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio::time::{sleep, Instant};

    #[derive(Clone)]
    struct MockOllamaState {
        tags_fail: bool,
        installed_models: Vec<&'static str>,
        pulled_models: Arc<Mutex<Vec<String>>>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PullBody {
        model: String,
        stream: bool,
    }

    #[tokio::test]
    async fn ensure_model_available_pulls_missing_model() {
        let state = MockOllamaState {
            tags_fail: false,
            installed_models: Vec::new(),
            pulled_models: Arc::new(Mutex::new(Vec::new())),
        };
        let pulled_models = state.pulled_models.clone();
        let provider = mock_provider(state).await;

        provider
            .ensure_model_available("qwen2.5-coder:7b")
            .await
            .unwrap();

        assert_eq!(
            pulled_models.lock().unwrap().as_slice(),
            ["qwen2.5-coder:7b"]
        );
    }

    #[tokio::test]
    async fn ensure_model_available_reports_download_progress() {
        let state = MockOllamaState {
            tags_fail: false,
            installed_models: Vec::new(),
            pulled_models: Arc::new(Mutex::new(Vec::new())),
        };
        let provider = mock_provider(state).await;
        let mut messages = Vec::new();

        provider
            .ensure_model_available_with_progress("qwen2.5-coder:7b", |progress| {
                messages.push(progress.message);
            })
            .await
            .unwrap();

        assert!(messages
            .iter()
            .any(|message| message.contains("Downloading qwen2.5-coder:7b: 50%")));
    }

    #[tokio::test]
    async fn ensure_model_available_still_pulls_when_model_list_is_unavailable() {
        let state = MockOllamaState {
            tags_fail: true,
            installed_models: Vec::new(),
            pulled_models: Arc::new(Mutex::new(Vec::new())),
        };
        let pulled_models = state.pulled_models.clone();
        let provider = mock_provider(state).await;

        provider
            .ensure_model_available("qwen2.5-coder:7b")
            .await
            .unwrap();

        assert_eq!(
            pulled_models.lock().unwrap().as_slice(),
            ["qwen2.5-coder:7b"]
        );
    }

    #[tokio::test]
    async fn ensure_model_available_starts_local_ollama_when_unreachable() {
        let reserved_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = reserved_listener.local_addr().unwrap();
        drop(reserved_listener);

        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ollama-serve-started");
        let command = dir.path().join("ollama");
        write_ollama_serve_script(&command, &marker);

        let provider = OllamaProvider::with_base_url_and_command(
            format!("http://{addr}").parse().unwrap(),
            command,
        );

        let handle =
            tokio::spawn(async move { provider.ensure_model_available("qwen2.5-coder:7b").await });

        wait_for_file(&marker).await;

        let state = MockOllamaState {
            tags_fail: false,
            installed_models: Vec::new(),
            pulled_models: Arc::new(Mutex::new(Vec::new())),
        };
        let pulled_models = state.pulled_models.clone();
        serve_mock_ollama_on_addr(addr, state).await;

        handle.await.unwrap().unwrap();
        assert_eq!(
            pulled_models.lock().unwrap().as_slice(),
            ["qwen2.5-coder:7b"]
        );
    }

    #[tokio::test]
    async fn ensure_model_available_error_explains_missing_ollama_command() {
        let reserved_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = reserved_listener.local_addr().unwrap();
        drop(reserved_listener);

        let dir = tempfile::tempdir().unwrap();
        let command = dir.path().join("missing-ollama");
        let provider = OllamaProvider::with_base_url_and_command(
            format!("http://{addr}").parse().unwrap(),
            command.clone(),
        );

        let error = provider
            .ensure_model_available("qwen2.5-coder:7b")
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("automatic Ollama startup failed"));
        assert!(error.contains("Install Ollama"));
        assert!(error.contains(&command.to_string_lossy().to_string()));
    }

    async fn mock_provider(state: MockOllamaState) -> OllamaProvider {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        serve_mock_ollama(listener, state);

        OllamaProvider::with_base_url(format!("http://{addr}").parse().unwrap())
    }

    async fn serve_mock_ollama_on_addr(addr: std::net::SocketAddr, state: MockOllamaState) {
        let listener = TcpListener::bind(addr).await.unwrap();
        serve_mock_ollama(listener, state);
    }

    fn serve_mock_ollama(listener: TcpListener, state: MockOllamaState) {
        let app = Router::new()
            .route("/api/tags", get(mock_tags))
            .route("/api/pull", post(mock_pull))
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
    }

    async fn mock_tags(
        State(state): State<MockOllamaState>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        if state.tags_fail {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "model list unavailable" })),
            );
        }

        (
            StatusCode::OK,
            Json(json!({
                "models": state
                    .installed_models
                    .iter()
                    .map(|name| json!({ "name": name }))
                    .collect::<Vec<_>>()
            })),
        )
    }

    async fn mock_pull(State(state): State<MockOllamaState>, Json(body): Json<PullBody>) -> String {
        assert!(body.stream);
        state.pulled_models.lock().unwrap().push(body.model);
        format!(
            "{}\n{}\n",
            json!({
                "status": "downloading",
                "digest": "sha256:1234567890abcdef",
                "total": 100,
                "completed": 50
            }),
            json!({ "status": "success" })
        )
    }

    async fn wait_for_file(path: &std::path::Path) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if path.exists() {
                return;
            }
            sleep(Duration::from_millis(25)).await;
        }
        panic!("timed out waiting for {}", path.display());
    }

    #[cfg(unix)]
    fn write_ollama_serve_script(path: &std::path::Path, marker: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(
            path,
            format!(
                "#!/usr/bin/env sh\nif [ \"$1\" = \"serve\" ]; then\ntouch '{}'\nsleep 5\nexit 0\nfi\nexit 1\n",
                marker.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }
}
