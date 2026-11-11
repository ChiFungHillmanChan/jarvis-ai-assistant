use super::backend::{LocalBackend, ModelInfo, ToolCapability};
use async_trait::async_trait;
use reqwest::Client;

pub struct OllamaBackend;

use crate::ai::models::TOOL_CAPABLE_FAMILIES;

/// The models in Ollama's /api/tags reply that run on this Mac, by name.
/// Ollama also lists cloud models there; it forwards their prompts to
/// ollama.com, so they are not offered as a local choice.
pub fn models_on_this_mac(tags: &serde_json::Value) -> Vec<String> {
    let remote = |model: &serde_json::Value, field: &str| model.get(field).and_then(|v| v.as_str()).is_some_and(|v| !v.trim().is_empty());
    let mut models: Vec<String> = tags
        .get("models")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
        .filter(|m| !remote(m, "remote_model") && !remote(m, "remote_host"))
        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .filter(|name| !crate::ai::mail_policy::is_ollama_cloud_model(name))
        .collect();
    models.sort();
    models
}

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

        let tags: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

        Ok(models_on_this_mac(&tags)
            .into_iter()
            // Ollama doesn't return the context length in /api/tags
            .map(|id| ModelInfo { id, context_length: None })
            .collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A model on this Mac, as setup and Settings list it. A cloud model is
    /// listed by Ollama next to the local ones; it is told apart by its name
    /// or by the remote fields Ollama reports for it.
    #[test]
    fn only_models_that_run_on_this_mac_are_listed() {
        let tags = serde_json::json!({ "models": [
            { "name": "qwen3:8b" },
            { "name": "gpt-oss:120b-cloud", "remote_model": "gpt-oss:120b", "remote_host": "https://ollama.com:443" },
            { "name": "glm-4.6:cloud" },
            { "name": "private-assistant:latest", "remote_host": "https://ollama.com:443" },
            { "name": "alias:latest", "remote_model": "gpt-oss:120b" },
            { "name": "llama3.2:latest", "remote_model": "", "remote_host": " " },
        ]});
        assert_eq!(models_on_this_mac(&tags), ["llama3.2:latest", "qwen3:8b"]);
        assert!(models_on_this_mac(&serde_json::json!({})).is_empty());
    }

    /// Settings lists an endpoint's models through `list_models`. A cloud
    /// model under an alias has no "-cloud" in its name, so the mail rules
    /// cannot refuse it later; it must not be offered here.
    #[tokio::test]
    async fn the_models_listed_for_an_ollama_endpoint_leave_out_cloud_models() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request).await;
            let body = r#"{"models":[{"name":"qwen3:8b"},{"name":"private-assistant:latest","remote_host":"https://ollama.com:443"},{"name":"glm-4.6:cloud"}]}"#;
            let reply = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body);
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        let listed: Vec<String> = OllamaBackend.list_models(&url, None).await.unwrap().into_iter().map(|model| model.id).collect();
        assert_eq!(listed, ["qwen3:8b"]);
    }
}
