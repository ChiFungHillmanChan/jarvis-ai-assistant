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

# This script ends by committing the version bump and tagging the release
# (see step after the reachability gate below). A dirty tree at that point
# would either mix unrelated changes into the release commit or make the
# commit itself fail confusingly, so refuse to start.
[ -z "$(git status --porcelain)" ] || { echo "commit or stash first"; exit 1; }

# Source .env so option_env! macros (GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET,
# JARVIS_PAYMENT_API and public Cognito settings) get baked into the release binary at compile time. dotenvy
# at runtime is enough for `tauri dev`, but a packaged DMG has no .env to read —
# these values must be in the env when `tauri build` runs. `set -a` exports every
# variable the file defines, which is what option_env! reads.
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
# JARVIS_RELEASE_FREE=1 ships a free build: no account sign-in and no checkout.
# The dev .env sets a sandbox JARVIS_PAYMENT_API, so clear all three rather than
# trust what the file left behind; option_env! then bakes none of them in.
if [ "${JARVIS_RELEASE_FREE:-}" = "1" ]; then
  unset JARVIS_PAYMENT_API JARVIS_COGNITO_DOMAIN JARVIS_COGNITO_CLIENT_ID
  echo "==> Free build: account sign-in and checkout are left out"
else
  # A payment endpoint alone leaves Account hidden in downloaded builds. Stop
  # before version changes or uploads if the complete public configuration is absent.
  export JARVIS_PAYMENT_API JARVIS_COGNITO_DOMAIN JARVIS_COGNITO_CLIENT_ID
  node scripts/release-billing-config.mjs
fi

echo "==> 0/10 Running the test suite (same command CI runs)"
# release.sh builds from the local working tree, so CI green on the last push
# says nothing about what is about to be signed and notarized. Run the suite
# here, before anything is built, uploaded or published. `npm test` is
# `cargo test --manifest-path src-tauri/Cargo.toml` (package.json) — the same
# command .github/workflows/ci.yml runs. Explicit `if !` rather than relying on
# `set -e`: this must abort, and it must say why.
if ! npm test; then
  echo ""
  echo "TESTS FAILED — release aborted. Nothing was built, signed, uploaded or published."
  echo "Fix the suite (npm test) and re-run this script."
  exit 1
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

echo "==> 3/10 Verifying entitlements on the signed bundle"
codesign -d --entitlements :- "${APP_PATH}" 2>/dev/null | grep -q "com.apple.security.device.audio-input" || { echo "ERROR: built app has no microphone entitlement"; exit 1; }

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
# English notes for the English download page. Used only when the first line
# names this version, so last release's text is never published as this one's;
# without them the English page falls back to the Chinese notes.
NOTES_EN_FILE="${REPO_ROOT}/RELEASE_NOTES.en.md"
NOTES_EN=""
if [ -f "${NOTES_EN_FILE}" ] && head -n 1 "${NOTES_EN_FILE}" | grep -qF "v${VERSION}"; then
  NOTES_EN="$(cat "${NOTES_EN_FILE}")"
else
  echo "NOTE: RELEASE_NOTES.en.md is missing or not for v${VERSION}; the English download page will show the Chinese notes."
fi
# $(cat ...) already strips trailing newlines. Emit the JSON string with
# `jq -n --arg` — a here-string (`jq -Rs . <<< "$SIG_CONTENT"`) would put the
# newline back, and tauri-plugin-updater base64-decodes the signature without
# trimming, so a single "\n" makes every update fail with InvalidByte.
SIG_CONTENT="$(cat "${SIG_PATH}")"
cat > /tmp/latest.json <<MANIFEST
{
  "version": "${VERSION}",
  "notes": $(jq -Rs . <<< "${NOTES}"),
  "notes_en": $(jq -n --arg notes "${NOTES_EN}" '$notes'),
  "pub_date": "${PUB_DATE}",
  "dmg_url": "${DMG_PUBLIC_URL}",
  "platforms": {
    "darwin-aarch64": {
      "signature": $(jq -n --arg sig "${SIG_CONTENT}" '$sig'),
      "url": "${UPDATER_URL}"
    }
  }
}
MANIFEST
jq . /tmp/latest.json > /dev/null

echo "==> 10/10 Publishing latest.json"
# The in-app updater and the website /download page both read the manifest from
# the public website (Vercel-served, publicly readable), NOT from the private S3
# bucket. Write it into the website repo's public/ so a commit + push deploys the
# new release. This is the "update on jarvis-automation" step.
WEBSITE_MANIFEST="${REPO_ROOT}/../jarvis-official-website/public/latest.json"
if [ -d "$(dirname "${WEBSITE_MANIFEST}")" ]; then
  cp /tmp/latest.json "${WEBSITE_MANIFEST}"
  echo "Wrote ${WEBSITE_MANIFEST}"
  echo ""
  echo "ACTION REQUIRED — commit + push the website to publish this release:"
  echo "  git -C \"${REPO_ROOT}/../jarvis-official-website\" add public/latest.json"
  echo "  git -C \"${REPO_ROOT}/../jarvis-official-website\" commit -m \"release: JARVIS v${VERSION}\""
  echo "  git -C \"${REPO_ROOT}/../jarvis-official-website\" push"
  echo "  (Vercel auto-deploys; manifest goes live at https://jarvis-automation.com/latest.json)"
else
  echo "WARNING: website repo not found at ${WEBSITE_MANIFEST}."
  echo "Copy /tmp/latest.json to jarvis-official-website/public/latest.json manually, then commit + push."
fi

# Optional S3 mirror of the manifest (harmless; not authoritative).
aws s3 cp /tmp/latest.json "s3://${BUCKET}/latest.json" \
  --cache-control "max-age=60" --content-type "application/json" \
  --profile "${PROFILE}" || echo "(S3 manifest mirror skipped — the website manifest is authoritative)"

echo ""
echo "==> Verifying the published artifacts are actually reachable"
# A release that reports success while the download 403s is worse than a failed
# release: the website keeps offering a button that cannot work. Verified broken
# on 2026-07-26, which is why this gate exists. Check the same URLs a stranger
# and the in-app updater would fetch.
RELEASE_OK=1
for url in "${DMG_PUBLIC_URL}" "${UPDATER_URL}"; do
  if curl -fsSI "${url}" >/dev/null 2>&1; then
    echo "  OK  ${url}"
  else
    echo "  FAIL ${url} is not publicly downloadable"
    RELEASE_OK=0
  fi
done

# The signature must be present AND free of whitespace: tauri-plugin-updater
# feeds it straight into a base64 decode with no trim, so a trailing newline is
# just as fatal as an empty value while looking fine to the eye. Checked inside
# jq so the shell cannot silently strip the very newline being tested for.
SIG_CHECK="$(jq -r 'if ((.platforms["darwin-aarch64"].signature? // "") | type == "string" and length > 0 and (test("[[:space:]]") | not)) then "ok" else "bad" end' /tmp/latest.json)"
if [ "${SIG_CHECK}" != "ok" ]; then
  echo "  FAIL latest.json signature is empty or contains whitespace; the in-app updater cannot verify this release"
  RELEASE_OK=0
fi

if [ "${RELEASE_OK}" -ne 1 ]; then
  echo ""
  echo "RELEASE NOT SHIPPED. Users cannot download or update from this build."
  echo "Most likely cause: S3 Block Public Access is enabled on '${BUCKET}', which"
  echo "overrides the bucket policy in infra/releases-bucket-policy.json. Check with:"
  echo "  aws s3api get-public-access-block --bucket ${BUCKET} --profile ${PROFILE}"
  echo "Fix that (or front the bucket with CloudFront), then re-run this script."
  exit 1
fi

echo ""
echo "==> Committing release artifacts and tagging v${VERSION}"
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock RELEASE_NOTES.md
git commit -m "release: v${VERSION}"
git tag "v${VERSION}"

echo ""
echo "Released v${VERSION}"
echo "Manifest (live after website deploy): https://jarvis-automation.com/latest.json"
echo "Download (DMG):  ${DMG_PUBLIC_URL}"
echo "Updater payload: ${UPDATER_URL}"
echo ""
echo "Push this repo:"
echo "  git push origin main"
echo "  git push origin v${VERSION}"
echo "Push the website repo:"
echo "  git -C \"${REPO_ROOT}/../jarvis-official-website\" push"
