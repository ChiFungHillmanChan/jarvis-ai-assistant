#!/usr/bin/env bash
# Finish a release whose bundle is already built and signed.
#
# scripts/release.sh builds, notarizes and publishes in one pass. This script is
# the second half only: staple, verify, upload, publish, tag. Use it when the
# build succeeded but notarization could not run -- typically because the
# notarytool keychain profile had to be re-created:
#
#   xcrun notarytool store-credentials jarvis-notarytool \
#     --apple-id <apple-id> --team-id 22K6G3HH9G --password <app-specific-password>
#
# Usage: ./scripts/finish-release.sh <version> [bucket-name]
set -euo pipefail

VERSION="${1:?usage: finish-release.sh <version> [bucket]}"
BUCKET="${2:-jarvis-releases}"
REGION="ap-east-1"
PROFILE="${AWS_PROFILE:-jarvis-release-bot}"
NOTARYTOOL_PROFILE="${NOTARYTOOL_PROFILE:-jarvis-notarytool}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${REPO_ROOT}"

DMG_PATH="src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/JARVIS_${VERSION}_aarch64.dmg"
APP_PATH="src-tauri/target/aarch64-apple-darwin/release/bundle/macos/JARVIS.app"
TARBALL_PATH="src-tauri/target/aarch64-apple-darwin/release/bundle/macos/JARVIS.app.tar.gz"
SIG_PATH="${TARBALL_PATH}.sig"

for f in "${DMG_PATH}" "${TARBALL_PATH}" "${SIG_PATH}"; do
  [ -f "${f}" ] || { echo "Missing ${f}. Run scripts/release.sh, or rebuild first."; exit 1; }
done

echo "==> 1/7 Verifying entitlements on the signed bundle"
codesign -d --entitlements :- "${APP_PATH}" 2>/dev/null | grep -q "com.apple.security.device.audio-input" \
  || { echo "ERROR: built app has no microphone entitlement"; exit 1; }

echo "==> 2/7 Submitting to Apple notarytool (5-15 min) using keychain profile ${NOTARYTOOL_PROFILE}"
xcrun notarytool submit "${DMG_PATH}" --keychain-profile "${NOTARYTOOL_PROFILE}" --wait

echo "==> 3/7 Stapling notarization ticket"
xcrun stapler staple "${DMG_PATH}"
xcrun stapler validate "${DMG_PATH}"

echo "==> 4/7 Uploading DMG + updater tarball + signature to s3://${BUCKET}"
DMG_NAME="JARVIS_${VERSION}_aarch64.dmg"
LATEST_DMG_NAME="JARVIS_latest_aarch64.dmg"
TARBALL_NAME="JARVIS_${VERSION}_aarch64.app.tar.gz"
aws s3 cp "${DMG_PATH}" "s3://${BUCKET}/${DMG_NAME}" --profile "${PROFILE}"
aws s3 cp "${DMG_PATH}" "s3://${BUCKET}/${LATEST_DMG_NAME}" --profile "${PROFILE}"
aws s3 cp "${TARBALL_PATH}" "s3://${BUCKET}/${TARBALL_NAME}" --profile "${PROFILE}"
aws s3 cp "${SIG_PATH}" "s3://${BUCKET}/${TARBALL_NAME}.sig" --profile "${PROFILE}"

echo "==> 5/7 Generating latest.json"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DMG_PUBLIC_URL="https://${BUCKET}.s3.${REGION}.amazonaws.com/${DMG_NAME}"
UPDATER_URL="https://${BUCKET}.s3.${REGION}.amazonaws.com/${TARBALL_NAME}"
NOTES="$(cat "${REPO_ROOT}/RELEASE_NOTES.md")"
# jq -n --arg, not a here-string: a trailing newline in the signature makes
# tauri-plugin-updater fail every update with InvalidByte.
SIG_CONTENT="$(cat "${SIG_PATH}")"
cat > /tmp/latest.json <<MANIFEST
{
  "version": "${VERSION}",
  "notes": $(jq -Rs . <<< "${NOTES}"),
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

echo "==> 6/7 Verifying the published artifacts are actually reachable"
RELEASE_OK=1
for url in "${DMG_PUBLIC_URL}" "${UPDATER_URL}"; do
  if curl -fsSI "${url}" >/dev/null 2>&1; then echo "  OK  ${url}";
  else echo "  FAIL ${url} is not publicly downloadable"; RELEASE_OK=0; fi
done
SIG_CHECK="$(jq -r 'if ((.platforms["darwin-aarch64"].signature? // "") | type == "string" and length > 0 and (test("[[:space:]]") | not)) then "ok" else "bad" end' /tmp/latest.json)"
[ "${SIG_CHECK}" = "ok" ] || { echo "  FAIL latest.json signature is empty or has whitespace"; RELEASE_OK=0; }
[ "${RELEASE_OK}" -eq 1 ] || { echo "RELEASE NOT SHIPPED. Fix the above, then re-run."; exit 1; }

echo "==> 7/7 Publishing latest.json to the website repo"
WEBSITE_MANIFEST="${REPO_ROOT}/../jarvis-official-website/public/latest.json"
if [ -d "$(dirname "${WEBSITE_MANIFEST}")" ]; then
  cp /tmp/latest.json "${WEBSITE_MANIFEST}"
  echo "Wrote ${WEBSITE_MANIFEST}"
else
  echo "WARNING: website repo not found. Copy /tmp/latest.json manually."
fi
aws s3 cp /tmp/latest.json "s3://${BUCKET}/latest.json" \
  --cache-control "max-age=60" --content-type "application/json" \
  --profile "${PROFILE}" || echo "(S3 manifest mirror skipped)"

git tag "v${VERSION}"
echo ""
echo "Released v${VERSION}. Remaining:"
echo "  git push origin v${VERSION}"
echo "  cd ../jarvis-official-website && git add public/latest.json && git commit -m \"release: 發佈 JARVIS v${VERSION} 更新資訊\" && git push"
