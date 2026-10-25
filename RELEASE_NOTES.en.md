JARVIS v0.5.6

This update gives your mail and calendar more protection and adds "Delete all JARVIS data on this Mac".

Privacy and security
- When JARVIS has read content written by someone else, such as an email or a web page, while answering a request, it asks you to click before it changes a calendar event, creates a Notion page, opens a GitHub issue or deletes a memory. Instructions hidden in an email cannot trigger these on their own.
- Before a link from an AI reply or a page opens, JARVIS shows the full address and site name and opens it only after you confirm.
- Google Calendar event titles and locations follow the same rule as Gmail content: an AI that may not receive your email content is told only how many meetings you have today.

Storage
- New: Settings > Storage > Delete all JARVIS data on this Mac. It revokes JARVIS's Google access, then deletes the copies of your mail and calendar, chat history, memory, settings, the models JARVIS downloaded, backups, and every API key and sign-in saved in the Keychain, and restarts JARVIS as a fresh install. You type DELETE to confirm; it cannot be undone.
- Nothing in Gmail, Google Calendar, Notion or GitHub is deleted, and your JARVIS account stays open. Ollama and its models are not touched.

For Apple Silicon Macs; the app requires macOS 12 or later.
