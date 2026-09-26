JARVIS v0.5.1

This update strengthens privacy and security, and prepares JARVIS for Google's security assessment.

Data protection
- The database on this Mac is now encrypted (AES-256), with its key kept in macOS Keychain. Your existing data is converted automatically the first time you open JARVIS; there is nothing to do.
- The data folder and database files can only be opened by your account.
- Removing or disconnecting a mailbox in the email workspace also revokes JARVIS's access at Google. If the mailbox is the same Google account as the AI assistant's, access is kept so Calendar and the assistant keep working.

Email and AI
- Email content is sent only to AI services that do not train on it: OpenAI, Claude, AWS Bedrock, a local Ollama model, or Gemini once you confirm billing is enabled. If you use Gemini, confirm in Settings → Models that your API key has billing enabled.
- With Kimi, Qwen or another compatible service, chat and voice work as before, but email analysis, automatic sorting and the daily briefing do not send email to that service.

AI assistant
- Before saving a memory, archiving an email, deleting a calendar event or changing a follow-up, JARVIS shows a confirmation card and acts only after you choose Allow.
- Custom AI service addresses must use https, except for services on this Mac.
- New Google sign-ins ask only for access to calendar events, not full calendar access.

Other
- Updated every component with a known security issue.
- Logs no longer contain voice transcripts or tool content.

For Apple Silicon Macs; the app requires macOS 12 or later.
