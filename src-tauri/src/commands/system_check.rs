use crate::ai::local::{self, backend::BackendType};
use crate::db::Database;
use serde::Serialize;
use std::process::Command;
use std::sync::Arc;
use tauri::State;

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

/// Check if a binary is on PATH.
fn binary_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run a command and capture stdout, trimmed. Returns None on failure.
fn run_command(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Detect GPU capabilities.
fn detect_gpu() -> GpuInfo {
    let has_metal = std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64";

    let mut has_cuda = false;
    let mut cuda_version: Option<String> = None;
    let mut vram_gb: Option<f64> = None;

    if let Some(output) = run_command("nvidia-smi", &["--query-gpu=driver_version", "--format=csv,noheader,nounits"]) {
        has_cuda = true;
        cuda_version = Some(output);
    }
    if has_cuda {
        if let Some(vram_str) = run_command("nvidia-smi", &["--query-gpu=memory.total", "--format=csv,noheader,nounits"]) {
            vram_gb = vram_str.trim().parse::<f64>().ok().map(|mb| mb / 1024.0);
        }
    }

    GpuInfo { has_metal, has_cuda, cuda_version, vram_gb }
}

/// Detect Ollama installation and running status.
async fn detect_ollama() -> OllamaStatus {
    let installed = binary_exists("ollama");
    let version = run_command("ollama", &["--version"]);

    let running = if installed {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap_or_default();
        client.get("http://localhost:11434/")
            .send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    } else {
        false
    };

    OllamaStatus { installed, running, version }
}

/// Detect vLLM / Python availability.
fn detect_vllm() -> VllmStatus {
    let python_available = binary_exists("python3");
    let installed = python_available
        && run_command("python3", &["-c", "import vllm; print(vllm.__version__)"]).is_some();

    VllmStatus { installed, python_available }
}

/// Detect system RAM using sysinfo.
fn detect_ram() -> (f64, f64) {
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_memory();
    let total = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let available = sys.available_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    (total, available)
}

#[tauri::command]
pub async fn check_system_compatibility() -> Result<SystemCompatibility, String> {
    let (total_ram_gb, available_ram_gb) = detect_ram();
    let gpu = detect_gpu();
    let ollama = detect_ollama().await;
    let vllm = detect_vllm();
    let recommended_max_params = ram_to_recommendation(total_ram_gb).to_string();

    Ok(SystemCompatibility {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        total_ram_gb,
        available_ram_gb,
        gpu,
        ollama,
        vllm,
        recommended_max_params,
    })
}

#[tauri::command]
pub async fn check_backend_prerequisites(
    backend_type: String,
    url: Option<String>,
) -> Result<Vec<PrerequisiteCheck>, String> {
    let os = std::env::consts::OS;
    let mut checks = Vec::new();

    match backend_type.as_str() {
        "ollama" => {
            let installed = binary_exists("ollama");
            let install_cmd = if os == "macos" {
                "brew install ollama"
            } else {
                "curl -fsSL https://ollama.com/install.sh | sh"
            };
            checks.push(PrerequisiteCheck {
                name: "Ollama installed".to_string(),
                description: "Ollama binary available on PATH".to_string(),
                passed: installed,
                required: true,
                fix_command: Some(install_cmd.to_string()),
                fix_label: Some("Install Ollama".to_string()),
            });

            let running = if installed {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(3))
                    .build().unwrap_or_default();
                client.get("http://localhost:11434/").send().await
                    .map(|r| r.status().is_success()).unwrap_or(false)
            } else {
                false
            };
            checks.push(PrerequisiteCheck {
                name: "Ollama running".to_string(),
                description: "Ollama server responding on localhost:11434".to_string(),
                passed: running,
                required: true,
                fix_command: Some("ollama serve".to_string()),
                fix_label: Some("Start Ollama".to_string()),
            });

            let has_models = if running {
                let backend = local::get_backend(&BackendType::Ollama);
                backend.list_models("http://localhost:11434", None).await
                    .map(|m| !m.is_empty()).unwrap_or(false)
            } else {
                false
            };
            checks.push(PrerequisiteCheck {
                name: "Models available".to_string(),
                description: "At least one model pulled and ready".to_string(),
                passed: has_models,
                required: true,
                fix_command: None,
                fix_label: Some("Pull a model below".to_string()),
            });
        }
        "vllm" => {
            let python = binary_exists("python3");
            let python_cmd = if os == "macos" { "brew install python" } else { "apt install python3" };
            checks.push(PrerequisiteCheck {
                name: "Python 3 available".to_string(),
                description: "python3 binary on PATH".to_string(),
                passed: python,
                required: true,
                fix_command: Some(python_cmd.to_string()),
                fix_label: Some("Install Python".to_string()),
            });

            let gpu = detect_gpu();
            checks.push(PrerequisiteCheck {
                name: "NVIDIA GPU + CUDA".to_string(),
                description: if gpu.has_cuda {
                    format!("CUDA detected (driver {})", gpu.cuda_version.as_deref().unwrap_or("unknown"))
                } else if gpu.has_metal {
                    "Apple Silicon detected (Metal). vLLM has experimental macOS support.".to_string()
                } else {
                    "No GPU acceleration detected".to_string()
                },
                passed: gpu.has_cuda || gpu.has_metal,
                required: true,
                fix_command: None,
                fix_label: Some("vLLM requires a supported GPU".to_string()),
            });

            let vllm_installed = python
                && run_command("python3", &["-c", "import vllm; print(vllm.__version__)"]).is_some();
            checks.push(PrerequisiteCheck {
                name: "vLLM installed".to_string(),
                description: "vLLM Python package available".to_string(),
                passed: vllm_installed,
                required: true,
                fix_command: Some("pip install vllm".to_string()),
                fix_label: Some("Install vLLM".to_string()),
            });

            if let Some(ref endpoint_url) = url {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build().unwrap_or_default();
                let reachable = client.get(format!("{}/health", endpoint_url.trim_end_matches('/')))
                    .send().await.map(|r| r.status().is_success()).unwrap_or(false);
                checks.push(PrerequisiteCheck {
                    name: "Server running".to_string(),
                    description: format!("vLLM server at {}", endpoint_url),
                    passed: reachable,
                    required: true,
                    fix_command: Some("vllm serve <model> --host 0.0.0.0 --port 8000".to_string()),
                    fix_label: Some("Start vLLM server".to_string()),
                });
            }
        }
        "generic" => {
            if let Some(ref endpoint_url) = url {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build().unwrap_or_default();
                let reachable = client.get(format!("{}/v1/models", endpoint_url.trim_end_matches('/')))
                    .send().await.map(|r| r.status().is_success()).unwrap_or(false);
                checks.push(PrerequisiteCheck {
                    name: "Server reachable".to_string(),
                    description: format!("OpenAI-compatible server at {}", endpoint_url),
                    passed: reachable,
                    required: true,
                    fix_command: None,
                    fix_label: Some("Ensure your server is running".to_string()),
                });

                if reachable {
                    let backend = local::get_backend(&BackendType::Generic);
                    let has_models = backend.list_models(endpoint_url, None).await
                        .map(|m| !m.is_empty()).unwrap_or(false);
                    checks.push(PrerequisiteCheck {
                        name: "Models available".to_string(),
                        description: "At least one model served by endpoint".to_string(),
                        passed: has_models,
                        required: true,
                        fix_command: None,
                        fix_label: Some("Check your server configuration".to_string()),
                    });
                }
            } else {
                checks.push(PrerequisiteCheck {
                    name: "Endpoint URL required".to_string(),
                    description: "Enter your server URL to check connectivity".to_string(),
                    passed: false,
                    required: true,
                    fix_command: None,
                    fix_label: None,
                });
            }
        }
        _ => return Err(format!("Unknown backend type: {}", backend_type)),
    }

    Ok(checks)
}

#[tauri::command]
pub async fn get_recommended_models(
    db: State<'_, Arc<Database>>,
    endpoint_id: Option<String>,
) -> Result<Vec<RecommendedModel>, String> {
    let (total_ram_gb, _) = detect_ram();

    let mut pulled_ids: Vec<String> = Vec::new();
    if let Some(ref eid) = endpoint_id {
        let (url, backend_type_str, api_key) = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT url, backend_type, api_key FROM local_endpoints WHERE id = ?1",
                rusqlite::params![eid],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)),
            ).map_err(|e| format!("Endpoint not found: {}", e))?
        };
        let backend_type = BackendType::from_str(&backend_type_str);
        let backend = local::get_backend(&backend_type);
        if let Ok(models) = backend.list_models(&url, api_key.as_deref()).await {
            pulled_ids = models.into_iter().map(|m| m.id).collect();
        }
    }

    Ok(get_curated_models(total_ram_gb, &pulled_ids))
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

    #[test]
    fn test_binary_exists_known() {
        assert!(binary_exists("which"));
    }

    #[test]
    fn test_binary_exists_unknown() {
        assert!(!binary_exists("nonexistent_binary_xyz_999"));
    }

    #[test]
    fn test_detect_ram_returns_positive() {
        let (total, available) = detect_ram();
        assert!(total > 0.0, "total RAM should be positive, got {}", total);
        assert!(available >= 0.0, "available RAM should be non-negative, got {}", available);
        assert!(total >= available, "total should >= available");
    }

    #[test]
    fn test_detect_gpu_no_crash() {
        let gpu = detect_gpu();
        if std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64" {
            assert!(gpu.has_metal);
        }
    }
}
