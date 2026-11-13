JARVIS v0.5.12

This update makes automations dependable across restarts, sleep and quitting JARVIS. Nothing runs while your Mac is asleep; JARVIS catches up after it wakes. It also adds a Usage page and fixes several problems with Bedrock, Claude and Gemini.

New
- Open at login: JARVIS now opens when you log in to your Mac, so automations keep running after a restart. It turns on by itself once JARVIS is in your Applications folder. Turn it off in Settings > Background; once you turn it off, it stays off. "Delete all JARVIS data on this Mac" turns it off as well.
- Daily briefing: turn it on in Settings > Background and choose a time (08:30 unless you change it). JARVIS shows a notification and a briefing on the Automation page with today's meetings, tasks and unread mail, and adds an AI summary when the network is available. It is read aloud only when it runs on time, with the Mac awake and the screen unlocked. If the Mac was asleep or JARVIS was closed at that time, the briefing is shown once later the same day and is not read aloud. Once that day is over it is not shown.
- Keep awake on mains power (off unless you turn it on): while it is plugged in, the Mac does not go to sleep on its own. It does not help with the lid closed.
- The Automations page has a Run now button, and the run history says whether a run was on time, caught up afterwards or started by hand.
- Usage: Settings > Usage shows the AI usage recorded on this Mac. Choose the last 30 days, 90 days, 6 months, 1 year or 2 years, and group it by model, provider, feature or time. It shows requests, input, output and cache tokens, cost, the local model share and the most expensive requests. Gemini speech to text and MiniMax text to speech are recorded too, MiniMax speech in characters. Each request is kept for 30 days, then as daily totals for two years; the daily totals hold no prompts or mail. All of it stays on this Mac. Days are counted in UTC.
- Bedrock model ID: in Settings > Models, under Provider priority, the Bedrock row takes a model ID or an inference profile ID from the Bedrock console; leave it empty for the default model. JARVIS uses the model saved there and no longer switches to another model depending on the kind of request. Usage is recorded under the ID actually sent. The "+ Bedrock Claude" button is now "+ Bedrock".
- Mail analysis with the AI you chose, and AI sorting, can now use Bedrock instead of saying the feature is not supported. In these two features Bedrock's error messages follow the interface language, English or Chinese, and when the saved AWS credentials cannot be read JARVIS asks you to save them again in Settings. With Claude, AI sorting now asks Claude for a fixed-format answer.

Fixed
- Automations missed while the Mac was asleep or JARVIS was closed: after waking or reopening, only the latest missed run is caught up. Earlier ones are not run one by one, and the same scheduled run never runs twice.
- When the network is not back yet after waking, automations that need it (mail, calendar, Notion and GitHub sync) wait for it instead of failing, so "Scheduled jobs failed" no longer appears for that reason.
- If JARVIS is force-quit in the middle of a run, that run is done once more when JARVIS reopens.
- If the Mac goes to sleep in the middle of a run, the run is no longer counted as failed; JARVIS does it again after the Mac wakes.
- After a time zone change or a daylight saving change, automations follow the new local time without reopening JARVIS.
- After a long sleep, JARVIS refreshes an expired Google access token before syncing instead of using the old one.
- If the Mac was asleep when a meeting reminder was due and the meeting is still going on, JARVIS shows one "Meeting in progress" notice.
- Bedrock usage is recorded as AWS reports it for each request: input, output, cache read and cache write tokens. A reply AWS charged for but that has no content is recorded as well.
- When Gemini returns an empty answer, the chat says that it was Gemini and suggests another model. If some actions were already completed, it says they were kept and will not be repeated.
- Gemini replies can be as long as the chosen model allows, where JARVIS knows that model's limit; before, every Gemini reply was capped at 4,096 tokens. Tool descriptions sent to Gemini have the parts Gemini does not support removed first.
- The clean-up report in English uses singular and plural correctly.

Changed
- "Speak morning briefing on startup" is replaced by the daily briefing. If you had it on, the daily briefing is turned on for you at 08:30.
- Models that are not on the price list are no longer priced at a guessed US$3 / US$15 per million tokens. The Usage page and the mail sorting result show "Price unknown" instead; that usage is in none of the amounts and is not counted as free.
- Clean-up in Settings > Storage no longer deletes AI usage records.
- The first time JARVIS opens after the update, it corrects older usage records on this Mac once. Earlier versions worked out too low a cost for Bedrock requests that used the cache: records from before 29 October 2026 are corrected, so their cost may be higher than before; some later ones cannot be told apart from correct records and stay as they were. Usage of cloud models that are still not on the price list, recorded at the guessed price or at US$0, now reads "Price unknown". Local models always read US$0. Requests older than 30 days are combined into daily totals.
