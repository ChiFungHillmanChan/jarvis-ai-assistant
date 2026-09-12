use crate::db::Database;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};
use tauri::async_runtime::JoinHandle;

/// Managed state for tracking active model pulls.
pub struct ModelPullState {
    pub active: Mutex<Option<ActivePull>>,
}

pub struct ActivePull {
    pub job_id: String,
    pub endpoint_id: String,
    pub model: String,
    pub handle: JoinHandle<()>,
}

impl ModelPullState {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }
}

/// Download percentage for one Ollama pull progress line.
///
/// Named so the tests can assert against the shipped expression instead of
/// re-typing it: the obvious rearrangement `(completed / total) * 100`
/// truncates to 0 for every in-progress chunk and pins the onboarding download
/// bar at 0% through a multi-GB pull.
fn pull_percent(completed: u64, total: u64) -> u64 {
    if total > 0 {
        (completed * 100) / total
    } else {
        0
    }
}

#[derive(Deserialize)]
struct PullProgressLine {
    status: Option<String>,
    completed: Option<u64>,
    total: Option<u64>,
    error: Option<String>,
}

fn pull_stream_succeeded<'a>(lines: impl Iterator<Item = &'a str>) -> Result<bool, String> {
    let mut succeeded = false;
    for line in lines {
        let progress: PullProgressLine = serde_json::from_str(line).map_err(|_| "invalid_progress".to_string())?;
        if progress.error.is_some() { return Err("pull_failed".into()); }
        if succeeded { return Err("invalid_progress".into()); }
        succeeded = progress.status.as_deref() == Some("success");
    }
    Ok(succeeded)
}

async fn exact_model_installed(client: &reqwest::Client, model: &str) -> bool {
    let Ok(response) = client.get("http://127.0.0.1:11434/api/tags").send().await else { return false };
    if !response.status().is_success() { return false; }
    let Ok(value) = response.json::<serde_json::Value>().await else { return false };
    value.get("models").and_then(|v| v.as_array()).map(|models| models.iter().any(|entry| entry.get("name").and_then(|v| v.as_str()) == Some(model))).unwrap_or(false)
}

#[tauri::command]
pub async fn pull_model(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    pull_state: State<'_, ModelPullState>,
    endpoint_id: String,
    model_name: String,
    job_id: Option<String>,
) -> Result<String, String> {
    // Check if a pull is already active
    {
        let active = pull_state.active.lock().map_err(|e| e.to_string())?;
        if let Some(ref pull) = *active {
            if !pull.handle.inner().is_finished() {
                return Err(format!(
                    "A pull is already active for endpoint '{}' (model: {})",
                    pull.endpoint_id, pull.model
                ));
            }
        }
    }

    // Look up endpoint URL
    let (url, backend_type) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT url, backend_type FROM local_endpoints WHERE id = ?1",
            rusqlite::params![endpoint_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|e| format!("Endpoint not found: {}", e))?
    };

    if backend_type != "ollama" || !matches!(url.trim_end_matches('/'), "http://localhost:11434" | "http://127.0.0.1:11434") {
        return Err("unsupported_endpoint".into());
    }
    if super::system_check::curated_model_size(&model_name).is_none() { return Err("model_not_approved".into()); }
    if !super::system_check::ollama_server_ready().await { return Err("server_not_ready".into()); }

    let pull_url = "http://127.0.0.1:11434/api/pull";
    let job_id = match job_id {
        Some(id) if uuid::Uuid::parse_str(&id).is_ok() => id,
        Some(_) => return Err("invalid_job_id".into()),
        None => uuid::Uuid::new_v4().to_string(),
    };
    let event_job_id = job_id.clone();
    let eid = endpoint_id.clone();
    let model = model_name.clone();
    let handle_app = app_handle.clone();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();

    let handle = tauri::async_runtime::spawn(async move {
        if start_rx.await.is_err() { return; }
        let emit_progress = |status: &str, percent: u64, completed: u64, total: u64, error: Option<&str>| {
            let _ = handle_app.emit(
                "model-pull-progress",
                json!({
                    "endpoint_id": &eid,
                    "jobId": &event_job_id,
                    "model": &model,
                    "status": status,
                    "percent": percent,
                    "completed_bytes": completed,
                    "total_bytes": total,
                    "error": error,
                }),
            );
        };

        emit_progress("downloading", 0, 0, 0, None);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3600))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build().expect("valid fixed loopback client");

        let response = match client
            .post(pull_url)
            .json(&json!({"name": &model, "stream": true}))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                emit_progress("error", 0, 0, 0, Some(&e.to_string()));
                return;
            }
        };

        if !response.status().is_success() {
            emit_progress(
                "error", 0, 0, 0,
                Some(&format!("Ollama returned status {}", response.status())),
            );
            return;
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut success = false;

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    emit_progress("error", 0, 0, 0, Some(&e.to_string()));
                    return;
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Ok(progress) = serde_json::from_str::<PullProgressLine>(&line) {
                    if let Some(ref err) = progress.error {
                        emit_progress("error", 0, 0, 0, Some(err));
                        return;
                    }

                    let status_str = progress.status.as_deref().unwrap_or("downloading");
                    if success { emit_progress("error", 0, 0, 0, Some("invalid_progress")); return; }
                    if status_str == "success" { success = true; continue; }
                    let completed = progress.completed.unwrap_or(0);
                    let total = progress.total.unwrap_or(0);
                    let percent = pull_percent(completed, total);

                    let mapped_status = if status_str.contains("verifying") || status_str.contains("writing") {
                        "verifying"
                    } else {
                        "downloading"
                    };

                    emit_progress(mapped_status, percent, completed, total, None);
                } else {
                    emit_progress("error", 0, 0, 0, Some("invalid_progress"));
                    return;
                }
            }
        }

        if !buffer.trim().is_empty() {
            match pull_stream_succeeded(std::iter::once(buffer.trim())) {
                Ok(true) if !success => success = true,
                _ => { emit_progress("error", 0, 0, 0, Some("invalid_progress")); return; }
            }
        }
        if !success { emit_progress("error", 0, 0, 0, Some("interrupted")); return; }
        if !exact_model_installed(&client, &model).await { emit_progress("error", 0, 0, 0, Some("model_not_found")); return; }
        emit_progress("complete", 100, 0, 0, None);
    });

    // Store the active pull handle
    {
        let mut active = pull_state.active.lock().map_err(|e| e.to_string())?;
        if active.as_ref().is_some_and(|pull| !pull.handle.inner().is_finished()) {
            handle.abort();
            return Err("pull_already_active".into());
        }
        *active = Some(ActivePull {
            job_id: job_id.clone(),
            endpoint_id,
            model: model_name,
            handle,
        });
    }
    let _ = start_tx.send(());

    Ok(job_id)
}

#[tauri::command]
pub async fn cancel_model_pull(
    app_handle: tauri::AppHandle,
    pull_state: State<'_, ModelPullState>,
    job_id: Option<String>,
) -> Result<(), String> {
    let mut active = pull_state.active.lock().map_err(|e| e.to_string())?;
    if active.as_ref().is_some_and(|pull| job_id.as_deref().is_some_and(|id| id != pull.job_id)) { return Err("job_mismatch".into()); }
    if let Some(pull) = active.take() {
        pull.handle.abort();
        let _ = app_handle.emit(
            "model-pull-progress",
            json!({
                "endpoint_id": &pull.endpoint_id,
                "jobId": &pull.job_id,
                "model": &pull.model,
                "status": "error",
                "percent": 0,
                "completed_bytes": 0,
                "total_bytes": 0,
                "error": "Pull cancelled by user",
            }),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_requires_explicit_success_terminal_line() {
        let progress = [r#"{"status":"downloading","completed":10,"total":100}"#];
        assert!(!pull_stream_succeeded(progress.into_iter()).unwrap());
    }

    #[test]
    fn pull_rejects_error_and_invalid_json() {
        assert!(pull_stream_succeeded([r#"{"error":"failed"}"#].into_iter()).is_err());
        assert!(pull_stream_succeeded(["not json"].into_iter()).is_err());
    }

    #[test]
    fn pull_accepts_only_success_terminal_line() {
        assert!(pull_stream_succeeded([r#"{"status":"success"}"#].into_iter()).unwrap());
    }

    #[test]
    fn test_pull_progress_line_parsing() {
        let line = r#"{"status":"downloading digestname","completed":1234567,"total":4700000000}"#;
        let parsed: PullProgressLine = serde_json::from_str(line).unwrap();
        assert_eq!(parsed.status.as_deref(), Some("downloading digestname"));
        assert_eq!(parsed.completed, Some(1234567));
        assert_eq!(parsed.total, Some(4700000000));
        assert!(parsed.error.is_none());
    }

    #[test]
    fn test_pull_progress_line_error() {
        let line = r#"{"error":"model not found"}"#;
        let parsed: PullProgressLine = serde_json::from_str(line).unwrap();
        assert_eq!(parsed.error.as_deref(), Some("model not found"));
    }

    #[test]
    fn test_pull_progress_line_verifying() {
        let line = r#"{"status":"verifying sha256 digest"}"#;
        let parsed: PullProgressLine = serde_json::from_str(line).unwrap();
        assert!(parsed.status.as_deref().unwrap().contains("verifying"));
    }

    #[test]
    fn test_pull_progress_line_success() {
        let line = r#"{"status":"success"}"#;
        let parsed: PullProgressLine = serde_json::from_str(line).unwrap();
        assert_eq!(parsed.status.as_deref(), Some("success"));
    }

    #[test]
    fn test_percent_calculation() {
        assert_eq!(pull_percent(2_350_000_000, 4_700_000_000), 50);
        // Multiply-then-divide: 1% of a multi-GB pull must not truncate to 0.
        assert_eq!(pull_percent(47_000_000, 4_700_000_000), 1);
        assert_eq!(pull_percent(4_700_000_000, 4_700_000_000), 100);
    }

    #[test]
    fn test_percent_zero_total() {
        // Ollama's first lines carry no total; dividing by it must not panic.
        assert_eq!(pull_percent(0, 0), 0);
        assert_eq!(pull_percent(1_234_567, 0), 0);
    }
}
