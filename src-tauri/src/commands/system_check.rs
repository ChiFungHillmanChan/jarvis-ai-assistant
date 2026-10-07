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
    pub available_disk_gb: Option<f64>,
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

/// Size of a catalog model in ai/models.rs.
pub fn curated_model_size(id: &str) -> Option<f64> {
    crate::ai::models::local_model(id).map(|m| m.download_gb)
}

/// Get curated models filtered by available RAM.
/// `pulled_ids` is a list of model IDs already present on the endpoint.
pub fn get_curated_models(total_ram_gb: f64, pulled_ids: &[String]) -> Vec<RecommendedModel> {
    crate::ai::models::LOCAL_MODELS
        .iter()
        .filter(|m| (m.min_ram_gb as f64) <= total_ram_gb)
        .map(|m| RecommendedModel {
            id: m.id.to_string(),
            name: m.label.to_string(),
            param_size: m.param_size.to_string(),
            download_size_gb: m.download_gb,
            min_ram_gb: m.min_ram_gb,
            tool_capable: m.tool_capable,
            description: m.description.to_string(),
            already_pulled: pulled_ids.iter().any(|p| p == m.id),
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
    let app_installed = std::env::consts::OS == "macos"
        && std::path::Path::new("/Applications/Ollama.app").exists();
    let installed = binary_exists("ollama") || app_installed;
    let version = if binary_exists("ollama") { run_command("ollama", &["--version"]) } else { None };

    let running = ollama_server_ready().await;

    OllamaStatus { installed, running, version }
}

pub async fn ollama_server_ready() -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build() else { return false };
    let Ok(response) = client.get("http://127.0.0.1:11434/api/version").send().await else { return false };
    if !response.status().is_success() { return false; }
    response.json::<serde_json::Value>().await.ok()
        .and_then(|value| value.get("version").and_then(|v| v.as_str()).map(str::to_string))
        .is_some_and(|version| !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

pub fn available_disk_gb() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        let model_dir = std::env::var("OLLAMA_MODELS").ok()
            .or_else(|| std::env::var("HOME").ok().map(|home| format!("{home}/.ollama")))?;
        let path = std::path::Path::new(&model_dir);
        let probe_path = if path.exists() { path } else { path.parent()? };
        let output = run_command("df", &["-k", probe_path.to_str()?])?;
        let fields: Vec<_> = output.lines().nth(1)?.split_whitespace().collect();
        return fields.get(3)?.parse::<f64>().ok().map(|kb| kb / 1024.0 / 1024.0);
    }
    #[cfg(not(target_os = "macos"))]
    { None }
}

/// True when the macOS Command Line Tools are actually installed.
///
/// `/usr/bin/python3` on a stock Mac is a stub: `which python3` succeeds, but
/// running it pops the "install the command line developer tools" dialog in the
/// user's face -- and `check_system_compatibility` runs unprompted, so a user
/// who never asked about vLLM got the dialog anyway. `xcode-select -p` exits
/// non-zero until the tools are present and never prompts, so it is the safe
/// gate to check before executing python3 at all.
fn developer_tools_installed() -> bool {
    if std::env::consts::OS != "macos" {
        return true;
    }
    Command::new("xcode-select")
        .arg("-p")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Detect vLLM / Python availability.
fn detect_vllm() -> VllmStatus {
    let python_available = binary_exists("python3");
    let installed = python_available
        && developer_tools_installed()
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
        available_disk_gb: available_disk_gb(),
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
            let installed = binary_exists("ollama") || (os == "macos" && std::path::Path::new("/Applications/Ollama.app").is_dir());
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

            let running = ollama_server_ready().await;
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
        // The stored column is a Keychain marker, not the secret.
        let api_key = crate::integration_keys::resolve_local_endpoint_key(eid, api_key.as_deref());
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
    fn exact_tag_is_required_for_pulled_model() {
        let models = get_curated_models(16.0, &["qwen2.5:7b".into()]);
        assert!(!models.iter().find(|m| m.id == "qwen2.5:14b").unwrap().already_pulled);
    }

    #[test]
    fn qwen35_recommendations_match_ram() {
        assert!(get_curated_models(8.0, &[]).iter().any(|m| m.id == "qwen3.5:2b"));
        assert!(!get_curated_models(8.0, &[]).iter().any(|m| m.id == "qwen3.5:4b"));
        assert!(get_curated_models(16.0, &[]).iter().any(|m| m.id == "qwen3.5:4b"));
        assert!(!get_curated_models(16.0, &[]).iter().any(|m| m.id == "qwen3.5:9b"));
        assert!(get_curated_models(24.0, &[]).iter().any(|m| m.id == "qwen3.5:9b"));
        assert!(!get_curated_models(16.0, &[]).iter().any(|m| m.id == "qwen3.5:27b"));
    }

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
        assert_eq!(models.len(), 12);
        assert!(models.iter().any(|m| m.id == "qwen2.5:32b"));
        assert!(!models.iter().any(|m| m.id == "llama3.3:70b"));
    }

    #[test]
    fn test_curated_models_already_pulled() {
        let pulled = vec!["llama3.2:3b".to_string(), "mistral:7b-instruct".to_string()];
        let models = get_curated_models(8.0, &pulled);
        let llama = models.iter().find(|m| m.id == "llama3.2:3b").unwrap();
        assert!(llama.already_pulled);
        let mistral = models.iter().find(|m| m.id == "mistral:7b").unwrap();
        assert!(!mistral.already_pulled);
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
