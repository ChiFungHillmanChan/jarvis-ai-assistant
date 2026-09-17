#!/bin/bash
# Kept only as a manual recovery path. The app downloads this model itself:
# Settings -> Voice -> "Download offline model" streams it with a progress bar
# and verifies the SHA-256 below before putting it in place. Use this script
# only when the in-app download cannot run.
#
# The previous contents of this file told users to set OPENAI_API_KEY in a .env
# for speech-to-text. That has not been true since the local whisper fallback
# shipped, and a packaged .app has no .env to set.
set -euo pipefail

MODEL_DIR="${HOME}/Library/Application Support/jarvis/models"
MODEL_PATH="${MODEL_DIR}/ggml-base.bin"
MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
# Must match MODEL_SHA256 in src-tauri/src/voice/model_manager.rs.
MODEL_SHA256="60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"

mkdir -p "${MODEL_DIR}"
echo "Downloading ggml-base.bin (148 MB) to ${MODEL_PATH}"
curl -fL --progress-bar "${MODEL_URL}" -o "${MODEL_PATH}.part"

ACTUAL="$(shasum -a 256 "${MODEL_PATH}.part" | cut -d' ' -f1)"
if [ "${ACTUAL}" != "${MODEL_SHA256}" ]; then
  rm -f "${MODEL_PATH}.part"
  echo "Checksum mismatch: expected ${MODEL_SHA256}, got ${ACTUAL}. Download discarded."
  exit 1
fi

mv "${MODEL_PATH}.part" "${MODEL_PATH}"
echo "Verified and installed. Restart JARVIS to pick it up."
