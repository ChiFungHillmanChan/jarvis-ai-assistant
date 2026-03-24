use crate::voice::{JarvisAppHandle, VoiceEngine, VoiceState};
use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tokio::io::AsyncWriteExt;

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";

pub fn model_path() -> PathBuf {
    let data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.join("jarvis").join("models").join("ggml-base.bin")
}

pub fn is_downloaded() -> bool {
    model_path().exists()
}

pub async fn download(app_handle: JarvisAppHandle) -> Result<PathBuf, String> {
    let path = model_path();
    if path.exists() {
        return Ok(path);
    }

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create model directory: {}", e))?;
    }

    let engine = app_handle.state::<Arc<VoiceEngine>>();
    let previous_state = engine.get_state();
    engine.set_state_and_emit(VoiceState::ModelDownloading(0));

    let temp_path = path.with_extension("bin.part");
    let result = async {
        let response = reqwest::Client::new()
            .get(MODEL_URL)
            .send()
            .await
            .map_err(|e| format!("Failed to start model download: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Model download failed with status {}",
                response.status()
            ));
        }

        let total_bytes = response.content_length();
        let mut downloaded_bytes = 0u64;
        let mut file = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|e| format!("Failed to create temporary model file: {}", e))?;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Model download stream failed: {}", e))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Failed to write model chunk: {}", e))?;
            downloaded_bytes += chunk.len() as u64;

            if let Some(total_bytes) = total_bytes {
                let progress = ((downloaded_bytes * 100) / total_bytes.max(1)).min(100) as u32;
                engine.set_state_and_emit(VoiceState::ModelDownloading(progress));
            }
        }

        file.flush()
            .await
            .map_err(|e| format!("Failed to flush model file: {}", e))?;
        tokio::fs::rename(&temp_path, &path)
            .await
            .map_err(|e| format!("Failed to finalize model file: {}", e))?;
        Ok(path.clone())
    }
    .await;

    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }

    match result {
        Ok(path) => {
            engine.set_state_and_emit(match previous_state {
                VoiceState::Disabled => VoiceState::Disabled,
                _ => VoiceState::Idle,
            });
            Ok(path)
        }
        Err(e) => {
            engine.set_state_and_emit(VoiceState::Error(e.clone()));
            Err(e)
        }
    }
}
