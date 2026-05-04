// TODO: B9 wires this into tts.rs::speak via TtsRouter dispatch. For now,
// callers can invoke voice::cosyvoice::speak directly.

use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::mpsc;

/// Resolves the path to the speech-swift CosyVoice binary.
/// Check order:
///   1. $JARVIS_COSYVOICE_BIN env var (absolute path)
///   2. `cosyvoice` on PATH (via `which`)
///
/// Note: the speech-swift project (https://github.com/soniqo/speech-swift) names its CLI
/// product `AudioCLI` in its Package.swift. The user installs it themselves via
/// `swift build -c release` and places or symlinks the output binary somewhere on PATH
/// (or sets JARVIS_COSYVOICE_BIN to its location). Homebrew packaging is a Phase 2 task.
pub fn resolve_binary() -> Option<std::path::PathBuf> {
    if let Ok(env_path) = std::env::var("JARVIS_COSYVOICE_BIN") {
        let p = std::path::PathBuf::from(env_path);
        if p.exists() {
            return Some(p);
        }
    }
    // PATH lookup — try both the generic `cosyvoice` alias and the speech-swift `AudioCLI` binary name
    for candidate in &["cosyvoice", "AudioCLI"] {
        if let Ok(out) = std::process::Command::new("which").arg(candidate).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    let p = std::path::PathBuf::from(s);
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

/// Streams WAV/PCM audio chunks from speech-swift's AudioCLI binary.
/// Returns Err if the binary is not installed; the caller is responsible for
/// surfacing a user-facing prompt (e.g., via the Settings UI binary-not-found warning).
///
/// CLI argument shape is a best-effort guess based on the speech-swift README as of
/// 2026-05-04. Verify against an actual installation before wiring into production:
///
///   AudioCLI tts --model-path <path> --language <lang> --output - --format wav --text <text>
///
/// If speech-swift's CLI surface differs (e.g., positional args, different flag names),
/// update the `args` slice below. The contract with the caller is: subprocess writes
/// WAV bytes to stdout; we forward them as Vec<u8> chunks on the returned channel.
pub async fn speak(
    text: &str,
    language: &str,
    model_dir: &Path,
) -> Result<mpsc::Receiver<Vec<u8>>, String> {
    let bin = resolve_binary().ok_or_else(|| {
        "CosyVoice binary not found. Set JARVIS_COSYVOICE_BIN or install speech-swift and \
         symlink/alias the AudioCLI binary as `cosyvoice` on PATH."
            .to_string()
    })?;

    // CosyVoice3 upstream language tags per the Alibaba paper (arxiv 2505.17589).
    // "cantonese" is the likely correct tag for speech-swift; "yue" is the BCP-47 code.
    // Verify with: AudioCLI tts --help | grep language
    let lang_arg = match language {
        "yue" => "cantonese",
        "zh" | "zh-CN" | "zh-TW" => "chinese",
        _ => "english",
    };

    let mut child = Command::new(&bin)
        .args([
            "tts",
            "--model-path",
            model_dir.to_str().unwrap(),
            "--language",
            lang_arg,
            "--output",
            "-", // write WAV to stdout
            "--format",
            "wav",
            "--text",
            text,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn cosyvoice: {e}"))?;

    let stdout = child.stdout.take().ok_or("no stdout from cosyvoice binary")?;
    let (tx, rx) = mpsc::channel(64);

    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut buf = vec![0u8; 4096];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        // Wait for child to avoid zombie processes; ignore exit status here.
        let _ = child.wait().await;
    });

    Ok(rx)
}
