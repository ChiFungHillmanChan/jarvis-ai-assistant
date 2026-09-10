import { FormEvent, useMemo, useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CalendarEventView } from "../lib/types";
import { createEvent, syncCalendar } from "../lib/commands";
import { useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";

const dayKey = (value: Date) => `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}`;
const eventDay = (value: string) => /^\d{4}-\d{2}-\d{2}$/.test(value) ? value : dayKey(new Date(value));
const isAllDay = (event: CalendarEventView) => /^\d{4}-\d{2}-\d{2}$/.test(event.start_time);
const formatTime = (value: string) => new Date(value).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

export default function CalendarPage() {
  const [selectedDateKey, setSelectedDateKey] = useWorkspaceValue("calendarDate", dayKey(new Date()));
  const selectedDate = new Date(`${selectedDateKey}T12:00:00`);
  const setSelectedDate = (date: Date) => setSelectedDateKey(dayKey(date));
  const [selectedId, setSelectedId] = useWorkspaceValue<number | null>("calendarEventId", null);
  const [syncing, setSyncing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useWorkspaceValue("calendarDraftTitle", "");
  const [draftStart, setDraftStart] = useWorkspaceValue("calendarDraftStart", "");
  const [draftEnd, setDraftEnd] = useWorkspaceValue("calendarDraftEnd", "");
  const [draftNotes, setDraftNotes] = useWorkspaceValue("calendarDraftNotes", "");
  const weekStart = new Date(selectedDate);
  weekStart.setDate(selectedDate.getDate() - selectedDate.getDay());
  const weekEnd = new Date(weekStart);
  weekEnd.setDate(weekStart.getDate() + 6);
  const weekStartKey = dayKey(weekStart);
  const weekEndKey = dayKey(weekEnd);
  const { data, error: loadError, loading, refetch } = useTauriCommand<CalendarEventView[]>("get_events", { dateFrom: weekStartKey, dateTo: weekEndKey });
  const days = useMemo(() => Array.from({ length: 7 }, (_, index) => {
    const date = new Date(`${weekStartKey}T12:00:00`);
    date.setDate(date.getDate() + index);
    return date;
  }), [weekStartKey]);
  const visibleEvents = (data ?? []).filter((event) => eventDay(event.start_time) === dayKey(selectedDate));
  const selected = (data ?? []).find((event) => event.id === selectedId) ?? null;
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || "Local time";

  async function sync() {
    setSyncing(true); setActionError(null); setNotice(null);
    try { setNotice(await syncCalendar()); refetch(); } catch (error) { setActionError(String(error)); } finally { setSyncing(false); }
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const title = draftTitle.trim();
    const startValue = draftStart;
    const endValue = draftEnd;
    const description = draftNotes.trim();
    if (!title || !startValue || !endValue) return;
    const start = new Date(startValue); const end = new Date(endValue);
    if (end <= start) { setActionError("End time must be after start time."); return; }
    setSaving(true); setActionError(null); setNotice(null);
    try {
      await createEvent(title, start.toISOString(), end.toISOString(), description || undefined);
      setNotice("Personal event created in Google Calendar. No invitations were sent.");
      setDraftTitle(""); setDraftStart(""); setDraftEnd(""); setDraftNotes(""); refetch();
    } catch (error) { setActionError(String(error)); } finally { setSaving(false); }
  }

  return <main className="tool-page calendar-tool-page">
    <header className="tool-page__header"><div><p className="tool-page__eyebrow">Calendar</p><h1>Your schedule</h1><p>Times shown in {timezone}. Synced events may be incomplete until you refresh.</p></div><div className="tool-header-actions"><button className="tool-button" onClick={() => setSelectedId(null)}>New event</button><button className="tool-button" onClick={sync} disabled={syncing}>{syncing ? "Syncing…" : "Sync calendar"}</button></div></header>
    {(loadError || actionError || notice) && <div className={loadError || actionError ? "tool-alert tool-alert--error" : "tool-alert"} role="status">{actionError || loadError || notice}</div>}
    <nav className="calendar-week" aria-label="Choose a day"><button className="tool-button tool-button--quiet" onClick={() => setSelectedDate(new Date())}>Today</button>{days.map(day => <button key={dayKey(day)} className={`calendar-day${dayKey(day) === dayKey(selectedDate) ? " is-selected" : ""}`} onClick={() => { setSelectedDate(day); setSelectedId(null); }}><span>{day.toLocaleDateString([], { weekday: "short" })}</span><strong>{day.getDate()}</strong></button>)}</nav>
    <div className="tool-split"><section className="tool-panel tool-list-pane" aria-busy={loading}><div className="tool-panel__heading"><h2>{selectedDate.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric" })}</h2><span>{visibleEvents.length} events</span></div>
      {loading && !data ? <p className="tool-empty">Loading schedule…</p> : visibleEvents.length === 0 ? <p className="tool-empty">No synced events for this day.</p> : visibleEvents.map(item => <button key={item.id} className={`tool-list-row calendar-event${selectedId === item.id ? " is-selected" : ""}`} onClick={() => setSelectedId(item.id)}><span className="calendar-event__time">{isAllDay(item) ? "All day" : `${formatTime(item.start_time)}–${formatTime(item.end_time)}`}</span><span><strong>{item.summary}</strong><small>{item.location || item.status}</small></span></button>)}
    </section><aside className="tool-panel tool-detail-pane">{selected ? <><div className="tool-panel__heading"><h2>Event details</h2></div><h3>{selected.summary}</h3><dl className="tool-details"><dt>When</dt><dd>{isAllDay(selected) ? "All day" : `${new Date(selected.start_time).toLocaleString()} – ${new Date(selected.end_time).toLocaleString()}`}</dd><dt>Status</dt><dd>{selected.status}</dd>{selected.location && <><dt>Location</dt><dd>{selected.location}</dd></>}{selected.attendees && <><dt>Attendees</dt><dd>{selected.attendees}</dd></>}{selected.description && <><dt>Description</dt><dd className="tool-prewrap">{selected.description}</dd></>}</dl></> : <><div className="tool-panel__heading"><h2>Create personal event</h2></div><p className="tool-help">This adds an event to your calendar. Add guests later in Google Calendar if needed.</p><form className="tool-form" onSubmit={submit}><label>Title<input name="title" required value={draftTitle} onChange={event => setDraftTitle(event.target.value)} /></label><div className="tool-form__row"><label>Starts<input name="start" type="datetime-local" required value={draftStart} onChange={event => setDraftStart(event.target.value)} /></label><label>Ends<input name="end" type="datetime-local" required value={draftEnd} onChange={event => setDraftEnd(event.target.value)} /></label></div><label>Notes<textarea name="description" rows={4} value={draftNotes} onChange={event => setDraftNotes(event.target.value)} /></label><button className="tool-button" disabled={saving}>{saving ? "Creating…" : "Create personal event"}</button></form></>}</aside></div>
  </main>;
}
