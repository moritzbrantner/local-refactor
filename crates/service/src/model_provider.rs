use anyhow::{anyhow, Context, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone)]
pub struct OllamaProvider {
    base_url: Url,
    client: reqwest::Client,
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

#[derive(Debug, Deserialize)]
struct OllamaErrorResponse {
    error: Option<String>,
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
        Self {
            base_url: base_url.parse().expect("default Ollama URL is valid"),
            client: reqwest::Client::new(),
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

    pub async fn ensure_model_available(&self, name: &str) -> Result<()> {
        if !is_supported_model(name) {
            return Err(anyhow!("unsupported local coding model: {name}"));
        }

        if self.installed_model_names().await?.contains(name) {
            return Ok(());
        }

        self.pull_model(name).await
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

    async fn pull_model(&self, name: &str) -> Result<()> {
        let url = self.base_url.join("/api/pull")?;
        let response = self
            .client
            .post(url)
            .json(&OllamaPullRequest {
                model: name,
                stream: false,
            })
            .send()
            .await
            .with_context(|| format!("failed to ask Ollama to download {name}"))?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let detail = response
            .json::<OllamaErrorResponse>()
            .await
            .ok()
            .and_then(|body| body.error)
            .unwrap_or_else(|| "Ollama returned an error".to_string());
        Err(anyhow!("failed to download {name}: {detail} ({status})"))
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
