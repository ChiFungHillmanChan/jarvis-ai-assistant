JARVIS v0.1.3

Voice, sync, and tool-safety release.

- The microphone now works in the installed app: the shipped build carries the
  entitlement it was missing.
- Escape stops a spoken response mid-sentence.
- Cantonese replies are now spoken in a Cantonese voice.
- Calendar requests use your local time, not the server's.
- Your inbox is sorted by actual message time.
- Claude and OpenAI keys, when you provide them, are usable as fallbacks
  alongside Gemini.
- Sync and voice errors are now shown in the app instead of failing silently.
- Fixed scrolling and text selection being blocked in several panels.
- Safer tool behaviour: the assistant can no longer write inbox content into a
  shared calendar event, file an issue against a public repository, or turn
  injected text from an email into a standing memory.
