JARVIS v0.5.7

This update fixes several privacy and security issues, improves in-app updates and makes the interface smoother.

Privacy and security
- When JARVIS has read content written by someone else, such as an email or a web page, while answering a request, it now also asks you to click before it opens a file or an app.
- Ollama cloud models (names ending in :cloud or -cloud) send content to ollama.com, so they no longer count as a model running on this Mac: they are left out of the local model list and are not sent your mail or calendar content.
- After you switch to an AI that may not receive your email content, chats, memories and tasks saved before the switch are no longer sent to that AI. They return when you switch back to an AI that may receive email content.
- If JARVIS cannot get its database key from the macOS Keychain, the database is stored unencrypted. JARVIS now shows a notice and pauses mail and calendar sync until you restart it and allow Keychain access.
- Updated the libraries used for encrypted network connections.

Updates
- A new version now downloads in the background and asks you to restart only once it is ready. A download that times out or fails is retried, and the notice no longer stays on "Updating".

Fixes
- "Remember" is no longer refused because of ordinary wording.
- Local speech recognition set to Cantonese no longer produces garbled English.
- Mail group analysis handled by the AI you chose is no longer labelled "Local Ollama".
- Step 2 of setup asks whether to sort new mail with AI.

Performance
- Tables and links stay visible while a reply streams in; a long chat opens with the latest 40 messages first.
- Mail search, the calendar month view, Settings and the 3D scene on Home are smoother.

For Apple Silicon Macs; the app requires macOS 12 or later.
