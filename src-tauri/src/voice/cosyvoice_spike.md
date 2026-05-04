# CosyVoice 2 runtime spike

**Date:** 2026-05-04
**Decision:** speech-swift (Option B)
**Rationale:** mlx-audio does not include CosyVoice in its current model catalogue (v0.4.3); speech-swift ships a fully-implemented `CosyVoiceTTS` Swift SPM package with streaming, voice cloning, and Cantonese (9 languages via CosyVoice3), and is actively maintained as of today.

## Option A — mlx-audio (Python, MLX backend)

- Repo: https://github.com/Blaizzy/mlx-audio
- Install path: pip / uvx / uv (`pip install mlx-audio`)
- PyPI status: confirmed — latest version 0.4.3, 21 releases published
- Python version requirement: 3.10+ (verified: system has Python 3.13.7, uv available)
- Bundled deps weight (estimate): MLX (~50MB) + torch transitive (~500MB) + model (~600MB) = ~1.1GB total first-time install
- Tauri integration shape: subprocess via `tokio::process::Command` invoking `python -m mlx_audio.tts.generate ...` — no FFI needed
- Apple Silicon support: native (MLX is Apple-only)
- Streaming support: supported via `--stream` flag on the CLI generate command; also via Python async generator API
- **CosyVoice model status: NOT PRESENT** — as of v0.4.3, the `mlx_audio/tts/models/` directory contains 37 model families (Kokoro, Qwen3, Spark, Dia, Chatterbox, etc.) but no CosyVoice. The model was either never ported or was removed. mlx-audio README does not list CosyVoice in its supported model table.
- Pros: well-documented, well-known (6932 stars), CLI-friendly, no FFI needed, easy interactive testing
- Cons: does not support CosyVoice at all — cannot be used for this task without a custom port; heavy Python dependency chain; bundling a venv into a Tauri DMG is non-trivial

## Option B — speech-swift (Swift CLI / SPM package)

- Repo: https://github.com/soniqo/speech-swift (verified active — pushed 2026-05-04, same day as this spike)
- Stars: 688 / Forks: 85 / Open issues: 3 / License: Apache-2.0
- Language: Swift (native Apple Silicon, MLX backend)
- Build: `swift build -c release` or add as Swift Package Manager dependency
- CosyVoice model: `CosyVoiceTTS` is a first-class SPM product with a full Swift implementation — `Sources/CosyVoiceTTS/` contains: `CosyVoiceTTS.swift`, `LLM.swift`, `FlowMatching.swift`, `HiFiGAN.swift`, `DiT.swift`, `Configuration.swift`, `DialogueParser.swift`, `DialogueSynthesizer.swift`, `WeightLoading.swift`. Three-stage pipeline: Qwen2.5-0.5B LLM generates speech tokens → DiT flow matching → HiFi-GAN vocoder at 24kHz.
- Model: CosyVoice3-0.5B-MLX-4bit (HuggingFace: `aufklarer/CosyVoice3-0.5B-MLX-4bit`), caches to `~/Library/Caches/qwen3-speech/`
- Bundled deps weight: Swift stdlib (already on macOS) + model ~500–700MB total (0.5B 4-bit quantised)
- Languages: 9 languages — CosyVoice3 upstream supports ZH, EN, JA, KO, plus Cantonese (yue) is listed in the original Alibaba CosyVoice3 paper (arxiv 2505.17589). The `language:` parameter is a free string passed to the tokenizer.
- Tauri integration shape: two options — (1) subprocess via `tokio::process::Command` invoking the compiled `AudioCLI` Swift binary, or (2) direct FFI via Swift package linked into the Tauri build. Option 1 (subprocess) is lower risk and matches the existing MiniMax B6 pattern.
- Apple Silicon support: native MLX backend, CoreML option for some models
- Streaming support: confirmed — `synthesizeStream(text:language:)` returns `AsyncThrowingStream<AudioChunk, Error>`; the CLI runner can forward chunks to stdout for the Rust subprocess to consume
- Pros: CosyVoice is fully implemented and tested; Swift binary is self-contained (no Python runtime needed); smaller attack surface in the DMG; repo updated today; streaming built-in; voice cloning and emotion tags available
- Cons: 688 stars vs 6932 for mlx-audio (smaller community); SPM dependency adds to Rust/Swift build complexity; subprocess CLI integration requires building the AudioCLI binary as part of the Tauri release bundle

## Decision

**Pick:** B — speech-swift

**Why:**
- mlx-audio v0.4.3 does not ship CosyVoice; using it would require a custom port, which is out of scope for B8
- speech-swift has a production-quality `CosyVoiceTTS` Swift SPM package with streaming, actively maintained (last push: 2026-05-04), and uses the CosyVoice3-0.5B-MLX-4bit quantised model that fits in ~500MB
- No Python runtime dependency simplifies Tauri DMG packaging significantly

**Risk:** speech-swift is a smaller project (688 stars) and the Cantonese language tag for CosyVoice3 has not been verified by live inference in this spike.

**Mitigation:** B8 implementer should test Cantonese output with `language: "cantonese"` or `"yue"` against the CosyVoice3 upstream language list before wiring it into production. If Cantonese is not supported, fallback is Mandarin ZH (which is intelligible to Cantonese speakers for a Jarvis assistant context).

## What B8 should do

- Use `tokio::process::Command` to invoke the compiled `speech-swift` `AudioCLI` binary
- Add `CosyVoiceTTS` as an SPM dependency in the Tauri Swift build or compile it as a standalone CLI binary bundled in the Tauri `.app/Contents/Resources/`
- Download `aufklarer/CosyVoice3-0.5B-MLX-4bit` from HuggingFace on first "Enable offline voice" toggle; implement in `cosyvoice_download.rs`
- Stream stdout `AudioChunk` bytes into the existing audio playback path (matches MiniMax B6 pattern)
- Tauri integration entry point: `src-tauri/src/voice/cosyvoice.rs` — wrap the subprocess call, handle model-not-downloaded error with a user-facing Settings prompt

## Skipped (for B8 to do live)

- Real CosyVoice3 model download from HuggingFace
- Cantonese sample generation timing (RTF measurement)
- Swift binary bundling logistics and code signing for Tauri DMG
- Confirmation of exact Cantonese language tag (`"cantonese"` vs `"yue"` vs `"zh-yue"`)

If this paper spike turns out wrong (e.g., speech-swift CosyVoiceTTS does not compile or Cantonese is absent from the CosyVoice3 language set), B8 implementer should pause and re-spike against Qwen3-TTS in mlx-audio (which does support ZH/EN/JA/KO) as a fallback Cantonese-capable model.

## Appendix — Raw findings

| Attribute | mlx-audio (Option A) | speech-swift (Option B) |
|-----------|---------------------|------------------------|
| GitHub stars | 6932 | 688 |
| Last push | 2026-05-03 | 2026-05-04 |
| License | MIT | Apache-2.0 |
| PyPI/SPM | PyPI confirmed (v0.4.3) | SPM package |
| CosyVoice support | None (not in model catalogue) | Full — `CosyVoiceTTS` SPM product |
| Streaming | Yes (--stream flag) | Yes (AsyncThrowingStream) |
| Python required | Yes | No |
| Apple Silicon | Native (MLX) | Native (MLX + CoreML) |
| Model size | N/A (no CosyVoice) | ~500MB (0.5B 4-bit) |
