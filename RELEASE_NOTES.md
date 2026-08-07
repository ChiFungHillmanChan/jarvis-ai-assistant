JARVIS v0.1.2

Security and reliability release.

- Stronger protection against malicious email content: the assistant can no
  longer be tricked into sending data out. Mail is always drafted for your
  review, never sent automatically, and the same restrictions now apply to
  local models.
- Your GitHub, Notion, and Obsidian tokens moved from the local database into
  the macOS Keychain.
- Smarter model routing: routine tasks now run on faster, cheaper models
  automatically; complex work still gets the most capable model.
- Fixed onboarding losing API keys, the app addressing you by the wrong name,
  and archived email reappearing in lists.
- Startup is more resilient in the first minute after launch.
- Download and automatic updates now work end to end.
