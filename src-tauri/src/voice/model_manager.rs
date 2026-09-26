use crate::voice::{JarvisAppHandle, VoiceEngine, VoiceState};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Manager;
use tokio::io::AsyncWriteExt;

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";

/// Git LFS object id of `ggml-base.bin`, read from the pointer file at
/// `huggingface.co/ggerganov/whisper.cpp/raw/main/ggml-base.bin` and matching
/// the `X-Linked-ETag` the CDN serves. A length check alone accepts any body of
/// the right size, and this file is fed straight into whisper.cpp.
const MODEL_SHA256: &str = "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe";

pub fn model_path() -> PathBuf {
    let data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.join("jarvis").join("models").join("ggml-base.bin")
}

/// A floor for `ggml-base.bin`, which is roughly 148 MB. This is a floor and
/// not a checksum -- a file this large can still be the wrong bytes -- but it
/// stops a zero-byte or grossly truncated leftover from being reported as a
/// usable model and then failing to load forever.
const MIN_MODEL_BYTES: u64 = 100_000_000;

pub fn is_downloaded() -> bool {
    model_file_is_usable(std::fs::metadata(model_path()).ok().map(|m| m.len()))
}

/// `None` means the file is absent.
fn model_file_is_usable(len: Option<u64>) -> bool {
    matches!(len, Some(len) if len >= MIN_MODEL_BYTES)
}

/// `force` re-downloads over an existing file. It is the only way out when the
/// model on disk is present but unusable, so it has to reach the UI.
pub async fn download(app_handle: JarvisAppHandle, force: bool) -> Result<PathBuf, String> {
    download_model(app_handle, force).await
}

/// A file that exists but is too short is not a download worth keeping, so the
/// skip is on `is_downloaded`, not on mere existence. `force` bypasses the skip
/// altogether -- a file that passes the length floor can still be the wrong
/// bytes, and re-downloading is the only recovery.
fn should_skip_download(force: bool, downloaded: bool) -> bool {
    !force && downloaded
}

/// One download at a time. Two would write the same `.part` file: the second
/// truncates it under the first, which then renames a file with a hole in it
/// (its checksum is taken over the stream, not the file).
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

struct DownloadGuard;

impl DownloadGuard {
    fn acquire() -> Option<Self> {
        DOWNLOADING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| DownloadGuard)
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        DOWNLOADING.store(false, Ordering::SeqCst);
    }
}

async fn download_model(app_handle: JarvisAppHandle, force: bool) -> Result<PathBuf, String> {
    let _only_download = DownloadGuard::acquire().ok_or("The speech model is already downloading.")?;
    let path = model_path();
    if should_skip_download(force, is_downloaded()) {
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
        let mut digest = Sha256::new();
        let mut file = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|e| format!("Failed to create temporary model file: {}", e))?;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Model download stream failed: {}", e))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Failed to write model chunk: {}", e))?;
            digest.update(&chunk);
            downloaded_bytes += chunk.len() as u64;

            if let Some(total_bytes) = total_bytes {
                let progress = ((downloaded_bytes * 100) / total_bytes.max(1)).min(100) as u32;
                engine.set_state_and_emit(VoiceState::ModelDownloading(progress));
            }
        }

        verify_download_size(downloaded_bytes, total_bytes)?;
        verify_checksum(&hex_digest(digest.finalize().as_slice()))?;

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
            // The cached whisper context, if any, points at the old file.
            engine.invalidate_local_transcriber();
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

/// Reject a truncated download before it is renamed into place.
///
/// A short file is renamed over the model path and then fails to load forever,
/// with no in-app way to re-download it. Length is the cheap half of the check;
/// `verify_checksum` is the half that catches a full-length wrong body.
fn verify_download_size(downloaded: u64, expected: Option<u64>) -> Result<(), String> {
    match expected {
        Some(expected) if downloaded != expected => Err(format!(
            "Model download incomplete: expected {} bytes, received {}",
            expected, downloaded
        )),
        _ => Ok(()),
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Refuse anything that is not the exact model we pinned. A mirror, a proxy
/// error page padded to the right length, or a tampered CDN object all land
/// here rather than inside whisper.cpp.
fn verify_checksum(actual: &str) -> Result<(), String> {
    if actual.eq_ignore_ascii_case(MODEL_SHA256) {
        return Ok(());
    }
    Err(format!(
        "Model checksum mismatch: expected {MODEL_SHA256}, got {actual}. The download was discarded."
    ))
}

#[cfg(test)]
mod download_guard_tests {
    use super::DownloadGuard;

    #[test]
    fn a_second_download_is_refused_while_one_runs() {
        let first = DownloadGuard::acquire().expect("the first download starts");
        assert!(DownloadGuard::acquire().is_none(), "two downloads would write the same .part file");
        drop(first);
        assert!(DownloadGuard::acquire().is_some(), "a finished or failed download frees the slot");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        hex_digest, model_file_is_usable, should_skip_download, verify_checksum,
        verify_download_size, MIN_MODEL_BYTES, MODEL_SHA256,
    };
    use sha2::{Digest, Sha256};

    #[test]
    fn a_full_length_wrong_body_is_still_rejected() {
        // The exact failure the length check cannot see: right size, wrong bytes.
        assert!(verify_download_size(147_951_465, Some(147_951_465)).is_ok());
        let wrong = hex_digest(Sha256::digest(b"not the model").as_slice());
        assert!(verify_checksum(&wrong).is_err());
    }

    #[test]
    fn the_pinned_digest_is_accepted_in_either_case() {
        assert!(verify_checksum(MODEL_SHA256).is_ok());
        assert!(verify_checksum(&MODEL_SHA256.to_uppercase()).is_ok());
    }

    #[test]
    fn hex_digest_is_lowercase_and_full_width() {
        let digest = hex_digest(Sha256::digest(b"").as_slice());
        assert_eq!(digest.len(), 64);
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn force_re_downloads_a_model_that_is_already_on_disk() {
        // Without force, a model that passes the length floor is left alone.
        assert!(should_skip_download(false, true));
        assert!(!should_skip_download(false, false));
        // With force, an existing file is replaced -- this is the recovery path
        // for a full-length but corrupt model.
        assert!(!should_skip_download(true, true));
        assert!(!should_skip_download(true, false));
    }

    #[test]
    fn a_present_but_truncated_model_is_not_downloaded() {
        assert!(!model_file_is_usable(None), "absent file");
        assert!(!model_file_is_usable(Some(0)), "zero-byte leftover");
        assert!(!model_file_is_usable(Some(4_096)), "truncated leftover");
        assert!(!model_file_is_usable(Some(MIN_MODEL_BYTES - 1)));
        assert!(model_file_is_usable(Some(147_951_465)), "full ggml-base.bin");
    }

    #[test]
    fn truncated_download_is_rejected() {
        assert!(verify_download_size(147_000_000, Some(147_951_465)).is_err());
    }

    #[test]
    fn complete_download_is_accepted() {
        assert!(verify_download_size(147_951_465, Some(147_951_465)).is_ok());
    }

    #[test]
    fn unknown_content_length_cannot_be_checked() {
        assert!(verify_download_size(147_951_465, None).is_ok());
    }
}
