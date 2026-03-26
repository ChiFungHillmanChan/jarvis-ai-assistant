use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GpuInfo {
    pub has_metal: bool,
    pub has_cuda: bool,
    pub cuda_version: Option<String>,
    pub vram_gb: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaStatus {
    pub installed: bool,
    pub running: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VllmStatus {
    pub installed: bool,
    pub python_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemCompatibility {
    pub os: String,
    pub arch: String,
    pub total_ram_gb: f64,
    pub available_ram_gb: f64,
    pub gpu: GpuInfo,
    pub ollama: OllamaStatus,
    pub vllm: VllmStatus,
    pub recommended_max_params: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrerequisiteCheck {
    pub name: String,
    pub description: String,
    pub passed: bool,
    pub required: bool,
    pub fix_command: Option<String>,
    pub fix_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecommendedModel {
    pub id: String,
    pub name: String,
    pub param_size: String,
    pub download_size_gb: f64,
    pub min_ram_gb: u32,
    pub tool_capable: bool,
    pub description: String,
    pub already_pulled: bool,
}

/// Map total RAM in GB to recommended max model parameter size.
pub fn ram_to_recommendation(total_ram_gb: f64) -> &'static str {
    if total_ram_gb >= 32.0 {
        "30B+"
    } else if total_ram_gb >= 16.0 {
        "13B"
    } else if total_ram_gb >= 8.0 {
        "7B"
    } else {
        "3B"
    }
}
