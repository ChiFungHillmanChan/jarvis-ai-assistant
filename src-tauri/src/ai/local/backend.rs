use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackendType {
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "vllm")]
    Vllm,
    #[serde(rename = "generic")]
    Generic,
}

impl BackendType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "ollama" => BackendType::Ollama,
            "vllm" => BackendType::Vllm,
            _ => BackendType::Generic,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            BackendType::Ollama => "ollama",
            BackendType::Vllm => "vllm",
            BackendType::Generic => "generic",
        }
    }
}

/// The wire form has to be the snake_case one `src/lib/types.ts` declares for
/// `LocalModel.supports_tools` ("native" | "prompt_injected" | "chat_only").
/// Serde's default was the variant name, so the webview compared "Native"
/// against "native", never matched, and labelled every local model "chat".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    Native,
    PromptInjected,
    ChatOnly,
}

impl ToolCapability {
    pub fn from_str(s: &str) -> Self {
        match s {
            "native" => ToolCapability::Native,
            "prompt_injected" => ToolCapability::PromptInjected,
            "chat_only" => ToolCapability::ChatOnly,
            _ => ToolCapability::PromptInjected,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            ToolCapability::Native => "native",
            ToolCapability::PromptInjected => "prompt_injected",
            ToolCapability::ChatOnly => "chat_only",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalEndpoint {
    pub id: String,
    pub name: String,
    pub url: String,
    pub backend_type: BackendType,
    pub api_key: Option<String>,
    pub use_tls: bool,
    pub connection_timeout_ms: u32,
    pub keep_alive_minutes: u32,
    pub is_active: bool,
    pub last_health_check: Option<String>,
    pub last_health_status: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModel {
    pub id: String,
    pub endpoint_id: String,
    pub context_length: u32,
    pub supports_tools: ToolCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub context_length: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointHealth {
    pub reachable: bool,
    pub model_count: u32,
    pub latency_ms: u64,
}

#[async_trait]
pub trait LocalBackend: Send + Sync {
    async fn list_models(&self, url: &str, api_key: Option<&str>) -> Result<Vec<ModelInfo>, String>;
    async fn health_check(&self, url: &str, api_key: Option<&str>) -> Result<bool, String>;
    async fn detect_tool_capability(&self, url: &str, model: &str) -> ToolCapability;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `list_local_models` serializes this enum straight to the webview, and
    /// `src/lib/types.ts` declares the three snake_case strings Settings
    /// compares against. Serde's default is the variant name, so without
    /// `rename_all` the webview matched "Native" against "native", no branch
    /// hit, and every local model was relabelled "chat". Nothing else in the
    /// suite serializes this type, so the attribute is checked here.
    #[test]
    fn tool_capability_serializes_as_the_typescript_contract() {
        for cap in [
            ToolCapability::Native,
            ToolCapability::PromptInjected,
            ToolCapability::ChatOnly,
        ] {
            let wire = serde_json::to_value(&cap).expect("serialize");
            assert_eq!(
                wire,
                serde_json::Value::String(cap.as_str().to_string()),
                "the wire form must be as_str(), not the variant name"
            );
            assert_eq!(
                serde_json::from_value::<ToolCapability>(wire).expect("deserialize"),
                cap,
                "the round trip must survive"
            );
            assert_eq!(ToolCapability::from_str(cap.as_str()), cap);
        }
    }

    /// The other half of the contract: the strings the webview compares
    /// against are declared in TypeScript, so a rename on either side has to
    /// fail here rather than silently relabel every model.
    #[test]
    fn types_ts_declares_the_same_three_strings() {
        let types_ts = include_str!("../../../../src/lib/types.ts");
        assert!(
            types_ts.contains(r#"supports_tools: "native" | "prompt_injected" | "chat_only""#),
            "src/lib/types.ts must declare LocalModel.supports_tools as the three snake_case ids"
        );
    }
}
