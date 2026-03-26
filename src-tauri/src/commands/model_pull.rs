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

#[derive(Deserialize)]
struct PullProgressLine {
    status: Option<String>,
    completed: Option<u64>,
    total: Option<u64>,
    error: Option<String>,
}

#[tauri::command]
pub async fn pull_model(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    pull_state: State<'_, ModelPullState>,
    endpoint_id: String,
    model_name: String,
) -> Result<(), String> {
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
    let url = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT url FROM local_endpoints WHERE id = ?1",
            rusqlite::params![endpoint_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|e| format!("Endpoint not found: {}", e))?
    };

    let pull_url = format!("{}/api/pull", url.trim_end_matches('/'));
    let eid = endpoint_id.clone();
    let model = model_name.clone();
    let handle_app = app_handle.clone();

    let handle = tauri::async_runtime::spawn(async move {
        let emit_progress = |status: &str, percent: u64, completed: u64, total: u64, error: Option<&str>| {
            let _ = handle_app.emit(
                "model-pull-progress",
                json!({
                    "endpoint_id": &eid,
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
            .build()
            .unwrap_or_default();

        let response = match client
            .post(&pull_url)
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
                    let completed = progress.completed.unwrap_or(0);
                    let total = progress.total.unwrap_or(0);
                    let percent = if total > 0 { (completed * 100) / total } else { 0 };

                    let mapped_status = if status_str.contains("verifying") || status_str.contains("writing") {
                        "verifying"
                    } else {
                        "downloading"
                    };

                    emit_progress(mapped_status, percent, completed, total, None);
                }
            }
        }

        emit_progress("complete", 100, 0, 0, None);
    });

    // Store the active pull handle
    {
        let mut active = pull_state.active.lock().map_err(|e| e.to_string())?;
        *active = Some(ActivePull {
            endpoint_id,
            model: model_name,
            handle,
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn cancel_model_pull(
    app_handle: tauri::AppHandle,
    pull_state: State<'_, ModelPullState>,
) -> Result<(), String> {
    let mut active = pull_state.active.lock().map_err(|e| e.to_string())?;
    if let Some(pull) = active.take() {
        pull.handle.abort();
        let _ = app_handle.emit(
            "model-pull-progress",
            json!({
                "endpoint_id": &pull.endpoint_id,
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
        let completed: u64 = 2_350_000_000;
        let total: u64 = 4_700_000_000;
        let percent = if total > 0 { (completed * 100) / total } else { 0 };
        assert_eq!(percent, 50);
    }

    #[test]
    fn test_percent_zero_total() {
        let total: u64 = 0;
        let completed: u64 = 0;
        let percent = if total > 0 { (completed * 100) / total } else { 0 };
        assert_eq!(percent, 0);
    }
}
