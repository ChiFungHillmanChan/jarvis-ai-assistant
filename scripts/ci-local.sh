#!/usr/bin/env bash
# The same checks as .github/workflows/ci.yml, run on this Mac. Use it before
# merging or releasing when GitHub Actions minutes are not available.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> npm ci"
npm ci
echo "==> Type-check and build the frontend"
npm run build
echo "==> Frontend workspace tests"
npm run test:frontend
echo "==> Rust tests"
npm test
echo "==> Clippy (advisory)"
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets || echo "Clippy reported issues (advisory, not a failure)"
echo ""
echo "LOCAL CI PASSED"
