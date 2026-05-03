#!/usr/bin/env bash
# Build, sign, notarize, and ship a JARVIS release to S3.
# Usage: ./scripts/release.sh <version> [bucket-name]
# Example: ./scripts/release.sh 0.1.0 jarvis-releases

set -euo pipefail

VERSION="${1:?usage: release.sh <version> [bucket]}"
BUCKET="${2:-jarvis-releases}"
REGION="ap-east-1"
PROFILE="${AWS_PROFILE:-jarvis-release-bot}"

# Required env (release-only secrets):
: "${APPLE_ID:?APPLE_ID env var required}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID env var required}"
: "${APPLE_APP_PASSWORD:?APPLE_APP_PASSWORD env var required}"
: "${TAURI_SIGNING_PRIVATE_KEY:?TAURI_SIGNING_PRIVATE_KEY env var required}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${REPO_ROOT}"

echo "==> 1/10 Bumping version to ${VERSION}"
node -e 'const fs=require("fs"); const p=JSON.parse(fs.readFileSync("package.json")); p.version=process.argv[1]; fs.writeFileSync("package.json", JSON.stringify(p,null,2)+"\n");' "${VERSION}"
node -e 'const fs=require("fs"); const f="src-tauri/tauri.conf.json"; const p=JSON.parse(fs.readFileSync(f)); p.version=process.argv[1]; fs.writeFileSync(f, JSON.stringify(p,null,2)+"\n");' "${VERSION}"
sed -i.bak -E "s/^(version = )\"[^\"]+\"$/\1\"${VERSION}\"/" src-tauri/Cargo.toml
rm -f src-tauri/Cargo.toml.bak

echo "==> 2/10 Building release"
npm run tauri build -- --target aarch64-apple-darwin

DMG_PATH="$(ls src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/JARVIS_${VERSION}_aarch64.dmg)"
APP_PATH="src-tauri/target/aarch64-apple-darwin/release/bundle/macos/JARVIS.app"
[ -f "${DMG_PATH}" ] || { echo "DMG not found at ${DMG_PATH}"; exit 1; }

echo "==> 3/10 Codesigning ${APP_PATH}"
SIGNING_IDENTITY="$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | awk -F\" '{print $2}')"
codesign --deep --force --options runtime --timestamp \
  --sign "${SIGNING_IDENTITY}" \
  "${APP_PATH}"
codesign --verify --verbose "${APP_PATH}"

echo "==> 4/10 Submitting to Apple notarytool (5-15 min)"
xcrun notarytool submit "${DMG_PATH}" \
  --apple-id "${APPLE_ID}" \
  --team-id "${APPLE_TEAM_ID}" \
  --password "${APPLE_APP_PASSWORD}" \
  --wait

echo "==> 5/10 Stapling notarization ticket"
xcrun stapler staple "${DMG_PATH}"
xcrun stapler validate "${DMG_PATH}"

echo "==> 6/10 Verifying Tauri updater signature"
# tauri.conf.json sets createUpdaterArtifacts: true, so `tauri build` (step 2) automatically
# runs `tauri signer sign --private-key-path <TAURI_SIGNING_PRIVATE_KEY>` and writes a .sig file
# next to the .dmg. The `tauri signer sign` CLI writes its output to <FILE>.sig; there is no
# stdout redirect required (confirmed via `tauri signer sign --help`: no --output flag exists).
SIG_PATH="${DMG_PATH}.sig"
if [ ! -f "${SIG_PATH}" ]; then
  echo "WARNING: ${SIG_PATH} was not produced by the build step."
  echo "Running explicit signer now. This can happen if TAURI_SIGNING_PRIVATE_KEY was not"
  echo "set in the environment when the build ran."
  npx tauri signer sign \
    --private-key-path "${TAURI_SIGNING_PRIVATE_KEY}" \
    "${DMG_PATH}"
  # The signer writes ${DMG_PATH}.sig automatically; no stdout redirect needed.
fi
[ -f "${SIG_PATH}" ] || { echo "ERROR: Updater signature file still missing: ${SIG_PATH}"; exit 1; }
echo "Updater signature present: ${SIG_PATH}"

echo "==> 7/10 Local install confirmation"
open "${DMG_PATH}"
echo ""
echo "Mount the DMG, drag JARVIS.app into Applications, launch it, and verify the app starts cleanly."
read -r -p "Did the local install start cleanly? [y/N] " ANSWER
if [[ "${ANSWER}" != "y" && "${ANSWER}" != "Y" ]]; then
  echo "Aborting before S3 upload. Local DMG remains at ${DMG_PATH}."
  exit 1
fi

echo "==> 8/10 Uploading DMG + signature to s3://${BUCKET}"
DMG_NAME="$(basename "${DMG_PATH}")"
aws s3 cp "${DMG_PATH}" "s3://${BUCKET}/${DMG_NAME}" --profile "${PROFILE}"
aws s3 cp "${SIG_PATH}" "s3://${BUCKET}/${DMG_NAME}.sig" --profile "${PROFILE}"

echo "==> 9/10 Generating latest.json"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DOWNLOAD_URL="https://${BUCKET}.s3.${REGION}.amazonaws.com/${DMG_NAME}"
NOTES_FILE="${REPO_ROOT}/RELEASE_NOTES.md"
NOTES="$([ -f "${NOTES_FILE}" ] && cat "${NOTES_FILE}" || echo "Routine update.")"
SIG_CONTENT="$(cat "${SIG_PATH}")"
cat > /tmp/latest.json <<MANIFEST
{
  "version": "${VERSION}",
  "notes": $(jq -Rs . <<< "${NOTES}"),
  "pub_date": "${PUB_DATE}",
  "platforms": {
    "darwin-aarch64": {
      "signature": $(jq -Rs . <<< "${SIG_CONTENT}"),
      "url": "${DOWNLOAD_URL}"
    }
  }
}
MANIFEST
jq . /tmp/latest.json > /dev/null

echo "==> 10/10 Uploading latest.json (cache-control 60s)"
aws s3 cp /tmp/latest.json "s3://${BUCKET}/latest.json" \
  --cache-control "max-age=60" \
  --content-type "application/json" \
  --profile "${PROFILE}"

echo ""
echo "Released v${VERSION}"
echo "Manifest: https://${BUCKET}.s3.${REGION}.amazonaws.com/latest.json"
echo "Download: ${DOWNLOAD_URL}"
