#!/bin/sh
# Cargo runner for macOS dev builds (see .cargo/config.toml).
#
# The Keychain decides who may read an item by two things: the trusted-app
# list ("Always Allow") and the item's partition list. Debug builds are
# ad-hoc signed with a hash-derived identifier, and even an Apple Development
# signature gets a per-build "cdhash:" partition, so every rebuild was a new
# app and the password prompt came back. Only a Developer ID (or App Store)
# signature gets the stable "teamid:" partition.
#
# So sign the dev binary exactly like the release app: Developer ID and the
# identifier com.jarvis.app. Keychain then treats dev and release JARVIS as
# the same app, and one "Always Allow" per item sticks across rebuilds. Test
# binaries are left alone; without the certificate (CI, another Mac) the
# binary runs unsigned as before.
bin="$1"
case "$bin" in
  */debug/jarvis)
    # Pick by hash: the same Developer ID can appear twice (two keychains),
    # which makes signing by name ambiguous.
    identity=${JARVIS_DEV_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | awk '/Developer ID Application/ {print $2; exit}')}
    if [ -n "$identity" ]; then
      codesign --force --sign "$identity" --identifier com.jarvis.app "$bin" >/dev/null 2>&1 \
        || echo "dev-run: codesign failed; Keychain will ask again after each rebuild" >&2
    fi
    ;;
esac
exec "$@"
