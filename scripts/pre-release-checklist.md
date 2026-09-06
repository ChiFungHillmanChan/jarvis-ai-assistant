# Pre-Release Manual Smoke Checklist

Run before every `./scripts/release.sh` invocation, against the built and notarized
`.app` / DMG that step is about to ship -- not `npm run tauri dev`. ~15 minutes.

If any item fails, block the release, fix the bug, then restart from item 1.

- [ ] 1. **Test suite green.** `npm test` (`cargo test --manifest-path
      src-tauri/Cargo.toml`) exits 0. `release.sh` runs this itself as step 0/10 and
      aborts on failure, so a red suite blocks the DMG either way -- run it here first
      to fail fast instead of after the rest of this list.
- [ ] 2. **Gemini liveness, with a real key.** `GEMINI_API_KEY=<real key> cargo test
      --manifest-path src-tauri/Cargo.toml shipped_gemini_model_ids_are_still_served --
      --ignored --nocapture` passes. Model ids can be delisted without warning; this is
      the only check that calls the live API before the pinned ids reach a customer.
- [ ] 3. **Build with the `option_env!` secrets exported.** Confirm `GOOGLE_CLIENT_ID`,
      `GOOGLE_CLIENT_SECRET` and `JARVIS_PAYMENT_API` are set in the shell (or in `.env`
      at the repo root, which `release.sh` sources) before it runs `tauri build`. These
      are compiled in via `option_env!`; a packaged `.app` has no way to pick them up
      afterward.
- [ ] 4. **`strings` confirms the secrets and model id actually compiled in.**
      ```
      APP=src-tauri/target/aarch64-apple-darwin/release/bundle/macos/JARVIS.app
      strings "$APP/Contents/MacOS/jarvis" | grep -c apps.googleusercontent.com
      strings "$APP/Contents/MacOS/jarvis" | grep -c gemini-3.7-flash
      ```
      Both must return at least 1. A silent `option_env!(None)` build ships Google
      sign-in and Gemini calls that fail for every user, with no build-time error.
- [ ] 5. **Entitlements.** `codesign -d --entitlements :- "$APP"` includes
      `com.apple.security.device.audio-input`. `release.sh` step 3/10 checks this too;
      verify it again here on the exact bundle you are about to hand a second user.
- [ ] 6. **Notarization.** `spctl -a -vv -t exec "$APP"` reports `accepted` and
      `source=Notarized Developer ID`. Gatekeeper blocks anything else on a clean Mac
      with no prior override.
- [ ] 7. **Second macOS user account walkthrough.** Log into a *different* macOS user
      account on the same machine (System Settings -> Users & Groups). This is the only
      way to see what a stranger sees, with no leftover Keychain entries or
      `~/Library/Application Support/jarvis` state carried over from your own account.
      - [ ] 7a. Mount the DMG, drag `JARVIS.app` into `/Applications`, launch it.
      - [ ] 7b. Onboarding: paste a Gemini API key only (no other provider key). Confirm
            it completes without requiring anything else.
      - [ ] 7c. Connect an allow-listed Google account (unverified-app Testing status
            caps this at 100 users total -- use a test account already added in the
            Cloud Console).
      - [ ] 7d. Chat: "list 3 unread emails" -> Gmail results render.
      - [ ] 7e. Chat: "schedule a follow-up tomorrow at 10am Hong Kong time" -> open
            Google Calendar in a browser and confirm the event lands at 10:00 HKT the
            next day, not shifted to the machine's or the server's own offset.
      - [ ] 7f. Press Cmd+Shift+J -> the macOS microphone permission prompt appears
            (first use on this account) and, once granted, the voice indicator goes to
            listening.
      - [ ] 7g. Trigger a spoken response, then press Escape mid-sentence -> speech
            stops within one sentence boundary.
      - [ ] 7h. Quit from the tray icon.
      - [ ] 7i. Relaunch -> the Gemini key and the Google connection are both still
            present; onboarding does not re-trigger.
- [ ] 8. **In-app update path.** Install the *previous* shipped version (0.1.2) fresh on
      a clean machine or account, launch it, and confirm it offers and completes an
      in-app update to the version you are about to release. This is the only check
      that exercises `latest.json` and the updater signature the way a real user's
      already-installed app will.

All 8 must pass before running `release.sh`.

## Notes

- For BYOK (item 7b), use a low-quota dev key. The onboarding wizard masks the input;
  the key itself goes to the macOS Keychain (`set_api_key` -> `KeychainStore`), never to
  SQLite or a file, so there is nothing to redact from disk afterward -- only from
  whatever terminal history you pasted it into.
- For Google OAuth (item 7c), the callback opens `http://127.0.0.1:<port>` in the
  system browser. If the second account's default browser blocks the redirect, switch
  to Safari or Chrome temporarily.
