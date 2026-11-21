JARVIS v0.5.13

This update sends each request only to the AI you chose. JARVIS no longer switches to another model on its own; when something fails, it tells you why and lets you pick a model to try again. Gemini's data terms become one consent card: email, calendar content and voice recordings go to Gemini only after you agree. The model list is updated to current models, and OpenAI, Claude and Gemini requests are faster or cheaper.

New
- Consent card: if you have not confirmed billing for Google Gemini, JARVIS shows the same card wherever you are: in the chat, on the Mail page, in setup, or as a notice for a job running in the background. Once you confirm, email, calendar content and voice recordings go to Gemini, and every feature works with it. You can withdraw it at any time in Settings > Your AI. If you had already ticked the Gemini billing confirmation, it carries over after the update.
- New models: Gemini 4 Hydrogen, Gemini 4 Neon, Gemini 4 Carbon, Gemini 4 Argon, Gemini 3.8 Flash, Claude Fable 5.2, and OpenAI's GPT-6.2 Astra, GPT-6.2 Sol, GPT-6.1 Terra, GPT-6.1 Sol and GPT-6 Astra.
- Settings > About shows the version, source and issue date of the model list JARVIS uses.

Changed
- No more silent model switching: each request goes only to the AI you chose. On a rate limit, a server error or a dropped connection, it is sent again once to the same model. If that still fails, an error card shows the model, the reason (with the service's own explanation when it refuses a request) and the other models you have keys for. Picking one runs that request once and leaves your settings as they are. A request that already carried out an action, such as creating an event or a draft, is never sent again. If you set up several AIs and relied on JARVIS moving to the next one, you will now see this card instead.
- Kimi, Qwen, Ollama Cloud and custom addresses: chat works as before. JARVIS cannot confirm how these services use data, so Gmail and Calendar content is not sent to them: mail features, reading your calendar and the daily briefing need another AI. A request that needs this content shows the reason and lists the models that may receive it.
- Speech-to-text follows your chosen AI: Settings shows which service transcribes your voice. If you have not confirmed Gemini billing, your voice is transcribed on your Mac, or goes to Gemini only after you agree. If cloud transcription fails, JARVIS uses only the Whisper model on your Mac, never another cloud service.
- Gemini's default model is now Gemini 4 Hydrogen (if you chose a specific model, nothing changes). Gemini 4 Hydrogen costs twice as much as Gemini 3.8 Flash, so sorting mail with it costs more; to keep the cost down, choose Gemini 3.8 Flash in Settings > Models.
- Think harder appears only when your model can use it, and not for Gemini models that already think at the highest level by default. While it is on, the line under the message box names the model that answers. If a Think harder request fails, you can send it again without Think harder.
- With your own AWS Bedrock account, replies now appear and are spoken as they arrive.
- OpenAI chat and general requests now go through OpenAI's Responses API, with OpenAI told not to store responses. Kimi, Qwen and other OpenAI-compatible services are unchanged.

Fixed
- Sorting mail with Gemini uses the lowest thinking level: in a test sorting 120 emails with Gemini 3.7 Flash, the time fell from about 22 minutes to about 6, the cost by about 70%, and the categories stayed nearly the same.
- When you use Claude directly, or OpenAI's GPT-5.6 and later models, the fixed system prompt and tool list use prompt caching, so later requests in the same conversation cost less.
- When a model's request for an action is cut off or incomplete, JARVIS does not carry it out and tells you why.
- When a model returns nothing, JARVIS shows the error card and no longer shows a blank answer.
- When Test and use this AI fails in Settings > Your AI or in the setup guide, it shows a sentence and the service's own reason instead of raw data. The key messages of Choose a model and Load models follow the interface language.
- Model prices and model IDs are corrected to the providers' current figures; models that still work but are being retired are marked.
- Local log files no longer contain the content of tool results, GitHub, due-date reminders, speech or automations.
