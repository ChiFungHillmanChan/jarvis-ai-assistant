JARVIS v0.5.0

This update lets you use one AI of your choice, and adds AI email sorting, calendar views and background sync.

Choose your AI
- First-time setup now asks which AI you use: a local Ollama model, OpenAI, Google Gemini, Anthropic Claude, Kimi, Qwen or another OpenAI-compatible service. JARVIS tests it with one short message and saves it only if it answers. The AI assistant, email analysis and automatic sorting use only this AI.
- Kimi and Qwen offer international or mainland China accounts; the model list comes directly from the service.
- Existing settings stay as they are. You can choose or change the AI in Settings → Models, under Your AI.
- Once you choose an AI, voice input uses cloud recognition only when Gemini is your choice; otherwise it uses a speech model on this Mac (about 148 MB, downloaded in Settings → Voice).

Email
- New AI sorting, off by default: it categorises messages, marks the ones that need action, writes summaries and finds follow-ups and dates. Without a TypeSafe key your chosen AI does this; adding a TypeSafe key lowers the cost of classification.
- When a sender later corrects a date, the new date replaces the old one, and a repeated follow-up is kept only once.
- While JARVIS is open, new email syncs in the background about every 5 minutes, and email missed earlier is recovered.
- The AI assistant can view and update email follow-ups, and view email categories and to-dos.
- The message list shows the sender's name and address separately and marks how many attachments a message has; message details list attachment names and sizes. Attachments stay in Gmail.

Reminders, calendar and more
- Automation now has a Follow-ups list with overdue tasks and tasks due today or within three days, which you can mark as done directly.
- New reminders appear as macOS notifications, and the same reminder is not notified twice.
- The calendar adds day, week and month views and a Today summary.
- Notion sync no longer fails because of your workspace plan, and GitHub connects directly without creating your own app.
- JARVIS remembers the window size and position.
- Fixed interface issues on first launch: the language screen could not be moved, and text overflowed its box.

For Apple Silicon Macs. The app requires macOS 12 or later; the local Ollama setup requires macOS 14 or later. The voice model (148 MB), the offline speech synthesis model (1.26 GB) and Ollama models (2.7 to 6.6 GB) are downloaded only when you turn on the related feature. Cloud AI depends on your account and region being eligible, and API charges are paid by your provider account; JARVIS does not offer hosted AI without a key.
