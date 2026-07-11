JARVIS v0.1.1

Reliability and safety update.

- Fixed a crash when reading or searching Chinese/Cantonese email, notes, and
  saved memory. The app no longer freezes on non-Latin text.
- Faster email search, with timeouts so it can't hang.
- More reliable background jobs: pausing or deleting a scheduled task now takes
  effect immediately, and stalled network calls no longer stick.
- Security hardening: system commands are now allow-listed, and your Google
  sign-in token is stored in the macOS Keychain instead of the local database.
- Assistant memory no longer stores duplicate facts.
- New: a persistent update button in the title bar so you can install future
  updates in one click.
