#!/usr/bin/env bash
# Build, sign, notarize, and ship a JARVIS release to S3.
# Usage: ./scripts/release.sh <version> [bucket-name]
# Example: ./scripts/release.sh 0.1.0 jarvis-releases

set -euo pipefail

VERSION="${1:?usage: release.sh <version> [bucket]}"
BUCKET="${2:-jarvis-releases}"
REGION="ap-east-1"
PROFILE="${AWS_PROFILE:-jarvis-release-bot}"

# Required env / Keychain entries:
# - TAURI_SIGNING_PRIVATE_KEY: path to the ed25519 private key for the Tauri updater
#   (defaults to ~/.tauri/jarvis.key in this project's setup; set in ~/.zshrc).
# - Apple notarization credentials are stored in the macOS Keychain under the
#   profile name in NOTARYTOOL_PROFILE (default: jarvis-notarytool). Create with:
#     xcrun notarytool store-credentials jarvis-notarytool \
#       --apple-id <id> --team-id <team> --password <app-specific-password>
NOTARYTOOL_PROFILE="${NOTARYTOOL_PROFILE:-jarvis-notarytool}"
: "${TAURI_SIGNING_PRIVATE_KEY:?TAURI_SIGNING_PRIVATE_KEY env var required}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${REPO_ROOT}"

# Source .env so option_env! macros (GOOGLE_CLIENT_ID etc.) get baked into the
# release binary at compile time. dotenvy at runtime is enough for `tauri dev`,
# but a packaged DMG has no .env to read — these values must be in the env when
# `tauri build` runs.
if [ -f "${REPO_ROOT}/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "${REPO_ROOT}/.env"
  set +a
fi
if [ -z "${GOOGLE_CLIENT_ID:-}" ]; then
  echo "WARNING: GOOGLE_CLIENT_ID not set — the shipped DMG will have no Google sign-in."
  read -r -p "Continue anyway? [y/N] " GO_ANYWAY
  if [[ "${GO_ANYWAY}" != "y" && "${GO_ANYWAY}" != "Y" ]]; then exit 1; fi
fi

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

echo "==> 4/10 Submitting to Apple notarytool (5-15 min) using keychain profile ${NOTARYTOOL_PROFILE}"
xcrun notarytool submit "${DMG_PATH}" \
  --keychain-profile "${NOTARYTOOL_PROFILE}" \
  --wait

echo "==> 5/10 Stapling notarization ticket"
xcrun stapler staple "${DMG_PATH}"
xcrun stapler validate "${DMG_PATH}"

echo "==> 6/10 Verifying Tauri updater artifact + signature"
# Tauri's updater payload is a .app.tar.gz tarball, NOT the DMG. With
# createUpdaterArtifacts: true, `tauri build` produces:
#   bundle/macos/JARVIS.app.tar.gz       <- updater downloads this
#   bundle/macos/JARVIS.app.tar.gz.sig   <- ed25519 signature over the tarball
#   bundle/dmg/JARVIS_<ver>_aarch64.dmg  <- first-time install (website /download)
# The DMG is for first install via the website. The updater fetches the tarball
# referenced in latest.json. Both must ship to S3.
APP_BUNDLE_DIR="$(dirname "${APP_PATH}")"
TARBALL_PATH="${APP_BUNDLE_DIR}/JARVIS.app.tar.gz"
SIG_PATH="${TARBALL_PATH}.sig"
[ -f "${TARBALL_PATH}" ] || { echo "ERROR: Updater tarball missing: ${TARBALL_PATH}"; exit 1; }
[ -f "${SIG_PATH}" ] || { echo "ERROR: Updater signature missing: ${SIG_PATH} (createUpdaterArtifacts must be true and TAURI_SIGNING_PRIVATE_KEY must be set during build)"; exit 1; }
echo "Updater tarball: ${TARBALL_PATH}"
echo "Updater signature: ${SIG_PATH}"

echo "==> 7/10 Local install confirmation"
open "${DMG_PATH}"
echo ""
echo "Mount the DMG, drag JARVIS.app into Applications, launch it, and verify the app starts cleanly."
read -r -p "Did the local install start cleanly? [y/N] " ANSWER
if [[ "${ANSWER}" != "y" && "${ANSWER}" != "Y" ]]; then
  echo "Aborting before S3 upload. Local DMG remains at ${DMG_PATH}."
  exit 1
fi

echo "==> 8/10 Uploading DMG + updater tarball + signature to s3://${BUCKET}"
DMG_NAME="$(basename "${DMG_PATH}")"
# Stable alias used by the website's /download page and as a fallback URL when
# latest.json is unreachable. Always points to the most recently released DMG.
LATEST_DMG_NAME="JARVIS_latest_aarch64.dmg"
# Use a versioned tarball name so old + new tarballs can coexist while users update.
TARBALL_NAME="JARVIS_${VERSION}_aarch64.app.tar.gz"
aws s3 cp "${DMG_PATH}" "s3://${BUCKET}/${DMG_NAME}" --profile "${PROFILE}"
aws s3 cp "${DMG_PATH}" "s3://${BUCKET}/${LATEST_DMG_NAME}" --profile "${PROFILE}"
aws s3 cp "${TARBALL_PATH}" "s3://${BUCKET}/${TARBALL_NAME}" --profile "${PROFILE}"
aws s3 cp "${SIG_PATH}" "s3://${BUCKET}/${TARBALL_NAME}.sig" --profile "${PROFILE}"

echo "==> 9/10 Generating latest.json"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
# DMG_PUBLIC_URL is shown by the website /download page for first-time install.
DMG_PUBLIC_URL="https://${BUCKET}.s3.${REGION}.amazonaws.com/${DMG_NAME}"
# UPDATER_URL is what the in-app updater downloads (must be the .app.tar.gz, not the DMG).
UPDATER_URL="https://${BUCKET}.s3.${REGION}.amazonaws.com/${TARBALL_NAME}"
NOTES_FILE="${REPO_ROOT}/RELEASE_NOTES.md"
NOTES="$([ -f "${NOTES_FILE}" ] && cat "${NOTES_FILE}" || echo "Routine update.")"
SIG_CONTENT="$(cat "${SIG_PATH}")"
cat > /tmp/latest.json <<MANIFEST
{
  "version": "${VERSION}",
  "notes": $(jq -Rs . <<< "${NOTES}"),
  "pub_date": "${PUB_DATE}",
  "dmg_url": "${DMG_PUBLIC_URL}",
  "platforms": {
    "darwin-aarch64": {
      "signature": $(jq -Rs . <<< "${SIG_CONTENT}"),
      "url": "${UPDATER_URL}"
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
echo "Manifest:        https://${BUCKET}.s3.${REGION}.amazonaws.com/latest.json"
echo "Download (DMG):  ${DMG_PUBLIC_URL}"
echo "Updater payload: ${UPDATER_URL}"
