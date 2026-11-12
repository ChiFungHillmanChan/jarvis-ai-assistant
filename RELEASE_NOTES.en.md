JARVIS v0.5.12

This update makes automations dependable across restarts, sleep and quitting JARVIS. Nothing runs while your Mac is asleep; JARVIS catches up after it wakes.

New
- Open at login: JARVIS now opens when you log in to your Mac, so automations keep running after a restart. It turns on by itself once JARVIS is in your Applications folder. Turn it off in Settings > Background; once you turn it off, it stays off.
- Daily briefing: turn it on in Settings > Background and choose a time (08:30 unless you change it). JARVIS shows a notification and a briefing on the Automation page with today's meetings, tasks and unread mail, and adds an AI summary when the network is available. It is read aloud only when it runs on time, with the Mac awake and the screen unlocked. If the Mac was asleep or JARVIS was closed at that time, the briefing is shown once afterwards and is not read aloud.
- Keep awake on mains power (off unless you turn it on): while it is plugged in, the Mac does not go to sleep on its own. It does not help with the lid closed.
- The Automations page has a Run now button, and the run history says whether a run was on time, caught up afterwards or started by hand.

Fixed
- Automations missed while the Mac was asleep or JARVIS was closed: after waking or reopening, only the latest missed run is caught up. Earlier ones are not run one by one, and the same scheduled run never runs twice.
- When the network is not back yet after waking, automations that need it (mail, calendar, Notion and GitHub sync) wait for it instead of failing, so "Scheduled jobs failed" no longer appears for that reason.
- If JARVIS is force-quit in the middle of a run, that run is done once more when JARVIS reopens.
- After a time zone change or a daylight saving change, automations follow the new local time without reopening JARVIS.
- After a long sleep, JARVIS refreshes an expired Google access token before syncing instead of using the old one.
- If the Mac was asleep when a meeting reminder was due and the meeting is still going on, JARVIS shows one "Meeting in progress" notice.

Changed
- "Speak morning briefing on startup" is replaced by the daily briefing. If you had it on, the daily briefing is turned on for you at 08:30.
