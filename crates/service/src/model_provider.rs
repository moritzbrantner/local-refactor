use anyhow::{Context, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct OllamaProvider {
    base_url: Url,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsResponse {
    pub provider: &'static str,
    pub models: Vec<ModelSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
}

impl OllamaProvider {
    pub fn from_env() -> Self {
        let base_url = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
        Self {
            base_url: base_url.parse().expect("default Ollama URL is valid"),
        }
    }

    pub async fn list_models(&self) -> Result<ModelsResponse> {
        let url = self.base_url.join("/api/tags")?;
        let response = reqwest::get(url)
            .await
            .with_context(|| "failed to connect to Ollama")?
            .error_for_status()
            .with_context(|| "Ollama returned an error")?
            .json::<OllamaTagsResponse>()
            .await
            .with_context(|| "failed to decode Ollama model list")?;

        Ok(ModelsResponse {
            provider: "ollama",
            models: response
                .models
                .into_iter()
                .map(|model| ModelSummary { name: model.name })
                .collect(),
        })
    }
}
