use super::backend::{LocalBackend, ModelInfo, ToolCapability};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

pub struct GenericBackend;

#[derive(Deserialize)]
struct ModelsResponse {
    data: Option<Vec<GenericModel>>,
}

#[derive(Deserialize)]
struct GenericModel {
    id: Option<String>,
}

#[async_trait]
impl LocalBackend for GenericBackend {
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
            .map_err(|e| format!("Failed to connect to endpoint: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Endpoint returned status {}", resp.status()));
        }

        let models_resp: ModelsResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let models = models_resp
            .data
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| {
                let id = m.id?;
                Some(ModelInfo {
                    id,
                    context_length: None,
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

        let mut req = client.get(format!("{}/v1/models", url.trim_end_matches('/')));
        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        match req.send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(e) => Err(format!("Endpoint not reachable: {}", e)),
        }
    }

    async fn detect_tool_capability(&self, _url: &str, _model: &str) -> ToolCapability {
        // Generic endpoints: default to prompt injection since we can't know
        ToolCapability::PromptInjected
    }
}
