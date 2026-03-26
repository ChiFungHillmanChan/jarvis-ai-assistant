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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
