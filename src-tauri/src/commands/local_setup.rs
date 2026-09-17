use serde::Serialize;
use std::sync::Mutex;
use tauri::{Emitter, State};

use super::system_check::{available_disk_gb, ollama_server_ready};

pub struct LocalSetupState {
    active: Mutex<Option<String>>,
}

impl LocalSetupState {
    pub fn new() -> Self { Self { active: Mutex::new(None) } }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSetupProbe {
    pub os: String,
    pub arch: String,
    pub macos_version: Option<String>,
    pub total_ram_gb: f64,
    pub available_disk_gb: Option<f64>,
    pub ollama_installed: bool,
    pub ollama_running: bool,
    pub installed_models: Vec<String>,
    pub recommended_tag: Option<String>,
    pub download_url: &'static str,
}

fn ollama_app_installed() -> bool {
    cfg!(target_os = "macos") && std::path::Path::new("/Applications/Ollama.app").is_dir()
}

fn suggested_tag(ram_gb: f64) -> Option<&'static str> {
    if ram_gb >= 24.0 { Some("qwen3.5:9b") }
    else if ram_gb >= 16.0 { Some("qwen3.5:4b") }
    else if ram_gb >= 8.0 { Some("qwen3.5:2b") }
    else { None }
}

fn disk_reserve_gb(model: &str) -> f64 {
    match model {
        "qwen3.5:2b" => 8.0,
        "qwen3.5:4b" => 10.0,
        "qwen3.5:9b" => 15.0,
        _ => f64::INFINITY,
    }
}

fn min_ram_gb(model: &str) -> Option<f64> {
    match model {
        "qwen3.5:2b" => Some(8.0),
        "qwen3.5:4b" => Some(16.0),
        "qwen3.5:9b" => Some(24.0),
        _ => None,
    }
}

fn validate_capacity(model: &str, ram_gb: f64, disk_gb: Option<f64>, installed: bool) -> Result<(), &'static str> {
    let required_ram = min_ram_gb(model).ok_or("model_not_approved")?;
    if ram_gb < required_ram { return Err("insufficient_ram"); }
    if !installed && !disk_gb.is_some_and(|gb| gb >= disk_reserve_gb(model)) {
        return Err("insufficient_disk_or_unknown");
    }
    Ok(())
}

fn exact_model_installed(tags: &[String], model: &str) -> bool {
    tags.iter().any(|tag| tag == model)
}

async fn installed_model_tags() -> Result<Vec<String>, String> {
    if !ollama_server_ready().await { return Err("server_not_ready".into()); }
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build() else { return Err("model_query_failed".into()) };
    let response = client.get("http://127.0.0.1:11434/api/tags").send().await
        .map_err(|_| "model_query_failed")?;
    if !response.status().is_success() { return Err("model_query_failed".into()); }
    let value = response.json::<serde_json::Value>().await.map_err(|_| "model_query_failed")?;
    value.get("models").and_then(|v| v.as_array()).map(|models| models.iter()
        .filter_map(|model| model.get("name").and_then(|v| v.as_str()).map(str::to_string))
        .collect()).ok_or_else(|| "model_query_failed".into())
}

fn macos_version() -> Option<String> {
    if !cfg!(target_os = "macos") { return None; }
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion").output().ok()?;
    if !output.status.success() { return None; }
    String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
}

/// Ollama's own floor: its download page states "Requires macOS 14 Sonoma or
/// later". This is deliberately stricter than the app's own
/// `minimumSystemVersion` of 12.0 -- JARVIS runs on 12 and 13 with a cloud
/// provider key, it just cannot drive a local model there.
fn supported_macos(version: Option<&str>) -> bool {
    version.and_then(|v| v.split('.').next())
        .and_then(|major| major.parse::<u32>().ok()).is_some_and(|major| major >= 14)
}

#[tauri::command]
pub async fn probe_local_setup() -> Result<LocalSetupProbe, String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    let total_ram_gb = sys.total_memory() as f64 / 1024f64.powi(3);
    let ollama_running = ollama_server_ready().await;
    let installed_models = if ollama_running { installed_model_tags().await? } else { Vec::new() };
    Ok(LocalSetupProbe {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        macos_version: macos_version(),
        total_ram_gb,
        available_disk_gb: available_disk_gb(),
        ollama_installed: ollama_app_installed(),
        ollama_running,
        installed_models,
        recommended_tag: suggested_tag(total_ram_gb).map(str::to_string),
        download_url: "https://ollama.com/download/mac",
    })
}

#[tauri::command]
pub async fn prepare_local_ai(
    app_handle: tauri::AppHandle,
    state: State<'_, LocalSetupState>,
    scope: String,
    model: String,
    job_id: String,
) -> Result<String, String> {
    if scope != "mail" && scope != "assistant" { return Err("invalid_scope".into()); }
    if min_ram_gb(&model).is_none() { return Err("model_not_approved".into()); }
    if uuid::Uuid::parse_str(&job_id).is_err() { return Err("invalid_job_id".into()); }
    {
        let mut active = state.active.lock().map_err(|_| "setup_busy")?;
        if active.is_some() { return Err("setup_busy".into()); }
        *active = Some(job_id.clone());
    }
    let emit = |stage: &str, error_code: Option<&str>| {
        let _ = app_handle.emit("local-setup-progress", serde_json::json!({
            "jobId": job_id, "stage": stage, "model": model,
            "completedBytes": null, "totalBytes": null, "errorCode": error_code,
        }));
    };
    let emit_active = |stage: &str, error_code: Option<&str>| -> Result<(), String> {
        let active = state.active.lock().map_err(|_| "setup_busy")?;
        if active.as_deref() != Some(&job_id) { return Err("cancelled".into()); }
        emit(stage, error_code);
        Ok(())
    };
    emit_active("checking", None)?;
    let result = async {
        let probe = probe_local_setup().await?;
        if state.active.lock().map_err(|_| "setup_busy")?.as_deref() != Some(&job_id) { return Err("cancelled".into()); }
        if probe.os != "macos" { return Err("unsupported_os".to_string()); }
        if !supported_macos(probe.macos_version.as_deref()) { return Err("unsupported_os_version".into()); }
        if probe.total_ram_gb < min_ram_gb(&model).unwrap() { return Err("insufficient_ram".into()); }
        if !probe.ollama_running {
            if !probe.ollama_installed { emit_active("needs_install", Some("ollama_not_installed"))?; return Ok(()); }
            emit_active("starting", None)?;
            let status = std::process::Command::new("/usr/bin/open")
                .arg("-a").arg("/Applications/Ollama.app")
                .status().map_err(|_| "ollama_start_failed")?;
            if !status.success() { return Err("ollama_start_failed".into()); }
            for _ in 0..20 {
                if state.active.lock().map_err(|_| "setup_busy")?.as_deref() != Some(&job_id) { return Err("cancelled".into()); }
                if ollama_server_ready().await { break; }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            if !ollama_server_ready().await { return Err("server_not_ready".into()); }
        }
        let installed_models = if probe.ollama_running { probe.installed_models } else { installed_model_tags().await? };
        validate_capacity(&model, probe.total_ram_gb, probe.available_disk_gb, exact_model_installed(&installed_models, &model))
            .map_err(str::to_string)?;
        let active = state.active.lock().map_err(|_| "setup_busy")?;
        if active.as_deref() != Some(&job_id) { return Err("cancelled".into()); }
        emit("server_ready", None);
        Ok(())
    }.await;
    if let Err(ref code) = result { emit(if code == "cancelled" { "cancelled" } else { "failed" }, Some(code)); }
    let mut active = state.active.lock().map_err(|_| "setup_busy")?;
    if active.as_deref() == Some(&job_id) { *active = None; }
    result.map(|_| job_id)
}

#[tauri::command]
pub fn cancel_local_setup(state: State<'_, LocalSetupState>, job_id: String) -> Result<(), String> {
    let mut active = state.active.lock().map_err(|_| "setup_busy")?;
    if active.as_deref() != Some(&job_id) { return Err("job_mismatch".into()); }
    *active = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_models_follow_report_ram_thresholds() {
        assert_eq!(validate_capacity("qwen3.5:2b", 8.0, Some(8.0), false), Ok(()));
        assert_eq!(validate_capacity("qwen3.5:4b", 8.0, Some(20.0), false), Err("insufficient_ram"));
        assert_eq!(validate_capacity("qwen3.5:9b", 16.0, Some(20.0), false), Err("insufficient_ram"));
        assert_eq!(validate_capacity("qwen3.5:27b", 64.0, Some(100.0), false), Err("model_not_approved"));
    }

    #[test]
    fn exact_installed_model_does_not_need_download_reserve() {
        assert!(!exact_model_installed(&["qwen3.5:2b".into()], "qwen3.5:4b"));
        assert!(exact_model_installed(&["qwen3.5:4b".into()], "qwen3.5:4b"));
        assert_eq!(validate_capacity("qwen3.5:4b", 16.0, Some(0.1), true), Ok(()));
        assert_eq!(validate_capacity("qwen3.5:4b", 16.0, None, true), Ok(()));
    }

    #[test]
    fn new_model_needs_known_disk_reserve() {
        assert_eq!(validate_capacity("qwen3.5:2b", 8.0, None, false), Err("insufficient_disk_or_unknown"));
        assert_eq!(validate_capacity("qwen3.5:2b", 8.0, Some(7.9), false), Err("insufficient_disk_or_unknown"));
        assert_eq!(validate_capacity("qwen3.5:4b", 16.0, Some(9.9), false), Err("insufficient_disk_or_unknown"));
        assert_eq!(validate_capacity("qwen3.5:9b", 24.0, Some(14.9), false), Err("insufficient_disk_or_unknown"));
    }

    #[test]
    fn recommendation_uses_real_ram_threshold() {
        assert_eq!(suggested_tag(7.9), None);
        assert_eq!(suggested_tag(8.0), Some("qwen3.5:2b"));
        assert_eq!(suggested_tag(16.0), Some("qwen3.5:4b"));
        assert_eq!(suggested_tag(24.0), Some("qwen3.5:9b"));
        assert_eq!(suggested_tag(32.0), Some("qwen3.5:9b"));
        assert!(!supported_macos(Some("13.6")));
        assert!(supported_macos(Some("14.0")));
    }
}
