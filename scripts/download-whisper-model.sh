#!/bin/bash
echo "JARVIS uses OpenAI Whisper API for speech-to-text."
echo "Make sure OPENAI_API_KEY is set in your .env file."
echo ""
echo "For future local Whisper support, download the model:"
echo "  mkdir -p ~/Library/Application\\ Support/jarvis/models"
echo "  curl -L https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin -o ~/Library/Application\\ Support/jarvis/models/ggml-base.bin"
