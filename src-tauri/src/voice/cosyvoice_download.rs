use std::path::PathBuf;
use tokio::sync::mpsc;

pub fn model_dir() -> PathBuf {
    dirs::data_dir().unwrap().join("jarvis/models/cosyvoice2")
}

/// Total download for the file list below, so the UI can warn before spending
/// a gigabyte of someone's disk. Sums the blob sizes the Hugging Face API
/// reports for this repo.
pub const TOTAL_DOWNLOAD_BYTES: u64 = 1_255_690_000;

#[derive(serde::Serialize, Clone, Debug)]
pub enum DownloadProgress {
    Started { total_files: usize },
    File { index: usize, name: String, bytes_done: u64, bytes_total: Option<u64> },
    Complete,
    Failed { error: String },
}

/// Returns path to model dir if already present, otherwise streams progress and downloads.
/// Uses the 4-bit quantised model from `aufklarer/CosyVoice3-0.5B-MLX-4bit` per B7 spike.
pub async fn ensure_downloaded(
    progress: mpsc::Sender<DownloadProgress>,
) -> Result<PathBuf, String> {
    let dir = model_dir();
    let marker = dir.join(".complete");
    if marker.exists() {
        return Ok(dir);
    }
    tokio::fs::create_dir_all(&dir).await.map_err(|e| format!("mkdir: {e}"))?;

    // File list for the 4-bit quantised CosyVoice3 model, read from the repo's
    // own file listing rather than guessed. The B7 spike's placeholder list
    // (model.safetensors, config.yaml, tokenizer.json, spk_embeddings.safetensors)
    // named four files that do not exist in this repo, so every download 404'd.
    let files = [
        "config.json",
        "flow.safetensors",
        "hifigan.safetensors",
        "llm.safetensors",
        "merges.txt",
        "speech_tokenizer.safetensors",
        "tokenizer_config.json",
        "vocab.json",
        "weight_shapes.json",
    ];
    let _ = progress.send(DownloadProgress::Started { total_files: files.len() }).await;

    let base = "https://huggingface.co/aufklarer/CosyVoice3-0.5B-MLX-4bit/resolve/main/";
    for (i, name) in files.iter().enumerate() {
        let url = format!("{base}{name}");
        let dest = dir.join(name);
        match download_file(&url, &dest, &progress, i, name).await {
            Ok(_) => {}
            Err(e) => {
                let _ = progress.send(DownloadProgress::Failed { error: e.clone() }).await;
                return Err(e);
            }
        }
    }
    tokio::fs::write(&marker, b"ok").await.map_err(|e| format!("marker: {e}"))?;
    let _ = progress.send(DownloadProgress::Complete).await;
    Ok(dir)
}

async fn download_file(
    url: &str,
    dest: &std::path::Path,
    progress: &mpsc::Sender<DownloadProgress>,
    index: usize,
    name: &str,
) -> Result<(), String> {
    use futures_util::StreamExt;
    let resp = reqwest::get(url).await.map_err(|e| format!("get {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("http {} for {url}", resp.status()));
    }
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("create {}: {e}", dest.display()))?;
    let mut bytes_done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream: {e}"))?;
        bytes_done += chunk.len() as u64;
        use tokio::io::AsyncWriteExt;
        file.write_all(&chunk).await.map_err(|e| format!("write: {e}"))?;
        let _ = progress
            .send(DownloadProgress::File {
                index,
                name: name.to_string(),
                bytes_done,
                bytes_total: total,
            })
            .await;
    }
    Ok(())
}
