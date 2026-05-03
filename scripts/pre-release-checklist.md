# Pre-Release Manual Smoke Checklist

Run before every `./scripts/release.sh` invocation. ~10 minutes.

If any item fails, block the release, fix the bug, then restart from item 1.

- [ ] 1. `rm -rf ~/Library/Application\ Support/jarvis`
- [ ] 2. `npm run tauri dev` → onboarding wizard mounts at Step 1
- [ ] 3. Step 1: pull a small model (e.g. Qwen 0.5B), confirm progress bar advances
- [ ] 4. Step 2: paste an OpenAI API key (use a dev key, redact before sharing logs)
- [ ] 5. Step 3: complete Google OAuth → callback returns successfully
- [ ] 6. Step 4: click Start → land on Dashboard without console errors
- [ ] 7. Chat: "list 3 unread emails" → confirm Gmail action runs and renders results
- [ ] 8. Chat: "show today's events" → confirm Calendar action runs
- [ ] 9. Open LaunchPage in sidebar → submit dummy feedback → verify SQLite write:
      ```
      sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db \
        "SELECT id, category, rating, length(message) FROM beta_feedback ORDER BY id DESC LIMIT 1"
      ```
- [ ] 10. Quit and relaunch app → onboarding does NOT re-trigger; lands directly on Dashboard
- [ ] 11. Settings → confirm OpenAI key from step 4 is still present
- [ ] 12. Walk every feature marked `pass` in [trial-audit-results.md](../docs/superpowers/specs/2026-05-04-trial-audit-results.md). 5 minutes per feature, single happy-path action.

All 12 must pass before running `release.sh`.

## Notes

- The audit doc at item 12 is the source of truth for which features ship in this release.
  If a `pass` feature regresses, either patch it before release or downgrade it to `hide` in
  the audit (with a follow-up commit) — don't ship a broken `pass`.
- For BYOK (item 4), use a low-quota dev key. The wizard masks the input but the key is
  stored in SQLite afterward — anyone with disk access to your test machine can read it.
- For Google OAuth (item 5), the callback opens `http://127.0.0.1:<port>` in the system
  browser. If your default browser blocks the redirect, switch to Safari or Chrome temporarily.
