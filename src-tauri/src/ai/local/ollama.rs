use super::backend::{LocalBackend, ModelInfo, ToolCapability};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

pub struct OllamaBackend;

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Option<Vec<OllamaModel>>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: Option<String>,
}

/// Known model families that support native tool/function calling
const TOOL_CAPABLE_FAMILIES: &[&str] = &[
    "qwen2.5",
    "qwen2",
    "qwen3",
    "llama3.1",
    "llama3.2",
    "llama3.3",
    "llama4",
    "mistral",
    "mixtral",
    "deepseek-v3",
    "deepseek-r1",
    "command-r",
    "command-r-plus",
    "firefunction",
    "hermes",
    "nemotron",
];

fn is_tool_capable_model(model_name: &str) -> bool {
    let lower = model_name.to_lowercase();
    TOOL_CAPABLE_FAMILIES
        .iter()
        .any(|family| lower.contains(family))
}

#[async_trait]
impl LocalBackend for OllamaBackend {
    async fn list_models(&self, url: &str, _api_key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;

        let resp = client
            .get(format!("{}/api/tags", url.trim_end_matches('/')))
            .send()
            .await
            .map_err(|e| format!("Failed to connect to Ollama: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Ollama returned status {}", resp.status()));
        }

        let tags: OllamaTagsResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

        let models = tags
            .models
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| {
                let name = m.name?;
                Some(ModelInfo {
                    id: name,
                    context_length: None, // Ollama doesn't return this in /api/tags
                })
            })
            .collect();

        Ok(models)
    }

    async fn health_check(&self, url: &str, _api_key: Option<&str>) -> Result<bool, String> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;

        let resp = client
            .get(format!("{}/", url.trim_end_matches('/')))
            .send()
            .await
            .map_err(|e| format!("Ollama not reachable: {}", e))?;

        let body = resp.text().await.unwrap_or_default();
        Ok(body.contains("Ollama"))
    }

    async fn detect_tool_capability(&self, _url: &str, model: &str) -> ToolCapability {
        if is_tool_capable_model(model) {
            ToolCapability::Native
        } else {
            ToolCapability::PromptInjected
        }
    }
}
