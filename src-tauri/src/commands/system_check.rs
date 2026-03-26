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

/// Curated model list for Ollama recommendations.
const CURATED_MODELS: &[(&str, &str, &str, f64, u32, bool, &str)] = &[
    // (id, name, param_size, download_gb, min_ram_gb, tool_capable, description)
    ("llama3.2:3b", "Llama 3.2 3B", "3B", 2.0, 8, true, "Fast, good for chat and basic tasks"),
    ("qwen2.5:7b", "Qwen 2.5 7B", "7B", 4.7, 8, true, "Strong tool use, multilingual"),
    ("llama3.2:latest", "Llama 3.2 8B", "8B", 4.9, 8, true, "Balanced general-purpose"),
    ("mistral:7b", "Mistral 7B", "7B", 4.1, 8, true, "Fast reasoning and tool use"),
    ("qwen3:8b", "Qwen 3 8B", "8B", 4.9, 8, true, "Latest Qwen, strong tool calling"),
    ("deepseek-r1:7b", "DeepSeek R1 7B", "7B", 4.7, 8, false, "Deep reasoning, no tool support"),
    ("qwen2.5:14b", "Qwen 2.5 14B", "14B", 9.0, 16, true, "Higher quality tool use"),
    ("llama3.3:latest", "Llama 3.3 13B", "13B", 8.0, 16, true, "Strong all-around"),
    ("deepseek-r1:14b", "DeepSeek R1 14B", "14B", 9.0, 16, false, "Strong reasoning at 14B"),
    ("qwen2.5:32b", "Qwen 2.5 32B", "32B", 20.0, 32, true, "Near-cloud quality"),
    ("deepseek-r1:32b", "DeepSeek R1 32B", "32B", 20.0, 32, false, "Best local reasoning"),
    ("llama3.1:70b", "Llama 3.1 70B", "70B", 40.0, 64, true, "Cloud-tier quality, needs heavy hardware"),
];

/// Get curated models filtered by available RAM.
/// `pulled_ids` is a list of model IDs already present on the endpoint.
pub fn get_curated_models(total_ram_gb: f64, pulled_ids: &[String]) -> Vec<RecommendedModel> {
    CURATED_MODELS
        .iter()
        .filter(|(_, _, _, _, min_ram, _, _)| (*min_ram as f64) <= total_ram_gb)
        .map(|(id, name, param_size, dl_gb, min_ram, tools, desc)| {
            let already_pulled = pulled_ids.iter().any(|p| p.contains(id.split(':').next().unwrap_or(id)));
            RecommendedModel {
                id: id.to_string(),
                name: name.to_string(),
                param_size: param_size.to_string(),
                download_size_gb: *dl_gb,
                min_ram_gb: *min_ram,
                tool_capable: *tools,
                description: desc.to_string(),
                already_pulled,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ram_to_recommendation_boundaries() {
        assert_eq!(ram_to_recommendation(4.0), "3B");
        assert_eq!(ram_to_recommendation(7.9), "3B");
        assert_eq!(ram_to_recommendation(8.0), "7B");
        assert_eq!(ram_to_recommendation(15.9), "7B");
        assert_eq!(ram_to_recommendation(16.0), "13B");
        assert_eq!(ram_to_recommendation(31.9), "13B");
        assert_eq!(ram_to_recommendation(32.0), "30B+");
        assert_eq!(ram_to_recommendation(64.0), "30B+");
    }

    #[test]
    fn test_curated_models_8gb() {
        let models = get_curated_models(8.0, &[]);
        assert!(models.iter().all(|m| m.min_ram_gb <= 8));
        assert_eq!(models.len(), 6);
        assert!(models.iter().any(|m| m.id == "llama3.2:3b"));
        assert!(!models.iter().any(|m| m.id == "qwen2.5:14b"));
    }

    #[test]
    fn test_curated_models_16gb() {
        let models = get_curated_models(16.0, &[]);
        assert_eq!(models.len(), 9);
        assert!(models.iter().any(|m| m.id == "qwen2.5:14b"));
        assert!(!models.iter().any(|m| m.id == "qwen2.5:32b"));
    }

    #[test]
    fn test_curated_models_32gb() {
        let models = get_curated_models(32.0, &[]);
        assert_eq!(models.len(), 11);
        assert!(models.iter().any(|m| m.id == "qwen2.5:32b"));
        assert!(!models.iter().any(|m| m.id == "llama3.1:70b"));
    }

    #[test]
    fn test_curated_models_already_pulled() {
        let pulled = vec!["llama3.2:3b".to_string(), "mistral:7b-instruct".to_string()];
        let models = get_curated_models(8.0, &pulled);
        let llama = models.iter().find(|m| m.id == "llama3.2:3b").unwrap();
        assert!(llama.already_pulled);
        let mistral = models.iter().find(|m| m.id == "mistral:7b").unwrap();
        assert!(mistral.already_pulled);
        let qwen = models.iter().find(|m| m.id == "qwen2.5:7b").unwrap();
        assert!(!qwen.already_pulled);
    }

    #[test]
    fn test_curated_models_4gb_only_small() {
        let models = get_curated_models(4.0, &[]);
        assert_eq!(models.len(), 0);
    }
}
