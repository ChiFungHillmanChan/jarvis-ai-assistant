JARVIS v0.5.5

This update fixes AI mail sorting and lets you choose the model for your AI.

Mail
- Fixed: with OpenAI as your AI and no TypeSafe key, "Sort with AI" sorted nothing, and background sorting failed the same way. Your chosen AI now decides the category and writes the summary; TypeSafe is optional.
- If AI sorting is on but no AI is set up, the Mail page shows "Set up your AI" instead of a button that cannot work.
- Quoted earlier emails are shown in sections by sender, without ">" marks.
- Sorting with a local model shows the tokens used (local models are free).

Your AI
- Choose the model for OpenAI, Gemini or Claude. The list comes straight from the provider, so new models appear as soon as they launch.
- Without a choice, "Latest (recommended)" is used and updates with JARVIS. The recommended Claude model is now Claude Sonnet 5.5, and Claude Haiku 5.5 is available.

Costs
- Fixed: mail sorted or analysed with a local Ollama model was counted as paid in the cost panel. Local models now show as free with their tokens; earlier records are not changed.

For Apple Silicon Macs; the app requires macOS 12 or later.
