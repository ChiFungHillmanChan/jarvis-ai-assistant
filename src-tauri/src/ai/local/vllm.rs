use super::backend::{LocalBackend, ModelInfo, ToolCapability};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

pub struct VllmBackend;

#[derive(Deserialize)]
struct VllmModelsResponse {
    data: Option<Vec<VllmModel>>,
}

#[derive(Deserialize)]
struct VllmModel {
    id: Option<String>,
    max_model_len: Option<u32>,
}

#[async_trait]
impl LocalBackend for VllmBackend {
    async fn list_models(&self, url: &str, api_key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;

        let mut req = client.get(format!("{}/v1/models", url.trim_end_matches('/')));
        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("Failed to connect to vLLM: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("vLLM returned status {}", resp.status()));
        }

        let models_resp: VllmModelsResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse vLLM response: {}", e))?;

        let models = models_resp
            .data
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| {
                let id = m.id?;
                Some(ModelInfo {
                    id,
                    context_length: m.max_model_len,
                })
            })
            .collect();

        Ok(models)
    }

    async fn health_check(&self, url: &str, api_key: Option<&str>) -> Result<bool, String> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;

        let mut req = client.get(format!("{}/health", url.trim_end_matches('/')));
        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        match req.send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => {
                // Some vLLM versions don't have /health, try /v1/models
                let mut fallback = client.get(format!("{}/v1/models", url.trim_end_matches('/')));
                if let Some(key) = api_key {
                    fallback = fallback.header("Authorization", format!("Bearer {}", key));
                }
                match fallback.send().await {
                    Ok(resp) => Ok(resp.status().is_success()),
                    Err(e) => Err(format!("vLLM not reachable: {}", e)),
                }
            }
        }
    }

    async fn detect_tool_capability(&self, _url: &str, _model: &str) -> ToolCapability {
        // vLLM with --enable-auto-tool-choice supports native tool calling
        // Default to native since most vLLM deployments for tool-use models enable this
        ToolCapability::Native
    }
}
