import { FormEvent, useEffect, useMemo, useState } from "react";
import * as mail from "../lib/mail";
import type { MailItem } from "../lib/mail";
import { MailItemRow } from "../components/mail/MailItems";
import "./email-workspace.css";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CalendarEventView, Task } from "../lib/types";
import { createEvent, getTasks, syncCalendar } from "../lib/commands";
import { useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";
import { useUiLocale } from "../i18n";
import { addDays, dayKey, dayStart, stepAnchor, toEntries, todaySummary, viewRange, type CalendarEntry, type CalendarView } from "../lib/calendar-layout";
import TimeGrid from "../components/calendar/TimeGrid";
import MonthGrid from "../components/calendar/MonthGrid";
import TodayPanel from "../components/calendar/TodayPanel";

// get_events matches on start date, so reach back far enough to catch
// multi-day events that began before the visible range.
const LOOKBACK_DAYS = 14;
const isAllDay = (event: CalendarEventView) => /^\d{4}-\d{2}-\d{2}$/.test(event.start_time);
type RangeFormat = Intl.DateTimeFormat & { formatRange(start: Date, end: Date): string };

export default function CalendarPage() {
  const { locale, text } = useUiLocale();
  const [now, setNow] = useState(() => new Date());
  const today = dayKey(now);
  const [view, setView] = useWorkspaceValue<CalendarView>("calendarView", "week");
  const [anchor, setAnchor] = useWorkspaceValue("calendarDate", today);
  // Right panel: null shows Today; otherwise an entry id ("event-12", "mail-4") or "new".
  const [selection, setSelection] = useWorkspaceValue<string | null>("calendarSelection", null);
  const [mailItems, setMailItems] = useState<MailItem[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [syncing, setSyncing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useWorkspaceValue("calendarDraftTitle", "");
  const [draftStart, setDraftStart] = useWorkspaceValue("calendarDraftStart", "");
  const [draftEnd, setDraftEnd] = useWorkspaceValue("calendarDraftEnd", "");
  const [draftNotes, setDraftNotes] = useWorkspaceValue("calendarDraftNotes", "");

  const range = useMemo(() => viewRange(view, anchor), [view, anchor]);
  const visible = useTauriCommand<CalendarEventView[]>("get_events", { dateFrom: addDays(range.from, -LOOKBACK_DAYS), dateTo: range.to });
  const todays = useTauriCommand<CalendarEventView[]>("get_events", { dateFrom: addDays(today, -LOOKBACK_DAYS), dateTo: today });

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 60_000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    let current = true;
    // Dated follow-ups found in email; local suggestions until added to Google.
    mail.mailListItems(undefined, false).then(items => { if (current) setMailItems(items.filter(item => item.due_date && !item.calendar_event_id)); }).catch(() => undefined);
    getTasks().then(items => { if (current) setTasks(items); }).catch(() => undefined);
    return () => { current = false; };
  }, [notice]);

  const entries = useMemo(() => toEntries(visible.data ?? [], mailItems), [visible.data, mailItems]);
  const summary = useMemo(() => todaySummary(todays.data ?? [], mailItems, tasks, now), [todays.data, mailItems, tasks, now]);
  const allEvents = [...(visible.data ?? []), ...(todays.data ?? [])];
  const selectedEvent = selection?.startsWith("event-") ? allEvents.find(event => `event-${event.id}` === selection) ?? null : null;
  const selectedMail = selection?.startsWith("mail-") ? mailItems.find(item => `mail-${item.id}` === selection) ?? null : null;
  const loadError = visible.error || todays.error;

  const refetch = () => { visible.refetch(); todays.refetch(); };
  const select = (entry: CalendarEntry) => setSelection(entry.id);
  const openDay = (day: string) => { setAnchor(day); setView("day"); };
  const move = (direction: 1 | -1) => setAnchor(stepAnchor(view, anchor, direction));

  const title = view === "month"
    ? dayStart(anchor).toLocaleDateString(locale, { month: "long", year: "numeric" })
    : view === "day"
      ? dayStart(anchor).toLocaleDateString(locale, { weekday: "long", day: "numeric", month: "long", year: "numeric" })
      : (new Intl.DateTimeFormat(locale, { day: "numeric", month: "short", year: "numeric" }) as RangeFormat).formatRange(dayStart(range.from), dayStart(range.to));
  const views: [CalendarView, string][] = [["day", text("Day", "日")], ["week", text("Week", "週")], ["month", text("Month", "月")]];

  async function sync() {
    setSyncing(true); setActionError(null); setNotice(null);
    try { await syncCalendar(range.from, range.to); setNotice(text("Calendar synced.", "日曆已同步。")); refetch(); } catch (error) { setActionError(String(error)); } finally { setSyncing(false); }
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const title = draftTitle.trim();
    const startValue = draftStart;
    const endValue = draftEnd;
    const description = draftNotes.trim();
    if (!title || !startValue || !endValue) return;
    const start = new Date(startValue); const end = new Date(endValue);
    if (end <= start) { setActionError(text("End time must be after start time.", "結束時間必須晚於開始時間。")); return; }
    setSaving(true); setActionError(null); setNotice(null);
    try {
      await createEvent(title, start.toISOString(), end.toISOString(), description || undefined);
      setNotice(text("Personal event created in Google Calendar. No invitations were sent.", "已在 Google Calendar 建立個人活動，沒有發出邀請。"));
      setDraftTitle(""); setDraftStart(""); setDraftEnd(""); setDraftNotes(""); refetch();
    } catch (error) { setActionError(String(error)); } finally { setSaving(false); }
  }

  const backToToday = <button className="tool-button tool-button--quiet" onClick={() => setSelection(null)}>{text("Back to today", "返回今日")}</button>;
  const panel = selectedMail ? <>
    <div className="tool-panel__heading"><h2>{text("From your email", "來自郵件")}</h2>{backToToday}</div>
    <div className="cal-panel__body">
      <p className="tool-help">{text("JARVIS found this in an email. It is only on this Mac until you add it to Google Calendar.", "JARVIS 從郵件找到此項目；加入 Google 日曆前，只保存在此 Mac。")}</p>
      <div className="mail-workspace mail-embedded"><ul className="mail-items"><MailItemRow item={selectedMail} text={text} locale={locale} onError={message => setActionError(message)} onChanged={next => { setMailItems(list => list.map(item => item.id === next.id ? next : item).filter(item => item.status === "open" && !item.calendar_event_id)); if (next.calendar_event_id) { setSelection(null); setNotice(text("Added to Google Calendar. Sync to see it here.", "已加入 Google 日曆；同步後會在此顯示。")); } }} /></ul></div>
    </div>
  </> : selectedEvent ? <>
    <div className="tool-panel__heading"><h2>{text("Event details", "活動詳情")}</h2>{backToToday}</div>
    <div className="cal-panel__body">
      <h3>{selectedEvent.summary}</h3>
      <dl className="tool-details"><dt>{text("When", "時間")}</dt><dd>{isAllDay(selectedEvent) ? text("All day", "全日") : `${new Date(selectedEvent.start_time).toLocaleString(locale)} – ${new Date(selectedEvent.end_time).toLocaleString(locale)}`}</dd><dt>{text("Status", "狀態")}</dt><dd>{selectedEvent.status === "confirmed" ? text("Confirmed", "已確認") : selectedEvent.status === "tentative" ? text("Tentative", "待確認") : selectedEvent.status === "cancelled" ? text("Cancelled", "已取消") : selectedEvent.status}</dd>{selectedEvent.account && <><dt>{text("Account", "帳戶")}</dt><dd>{text(`${selectedEvent.account} (read-only)`, `${selectedEvent.account}（唯讀）`)}</dd></>}{selectedEvent.location && <><dt>{text("Location", "地點")}</dt><dd>{selectedEvent.location}</dd></>}{selectedEvent.attendees && <><dt>{text("Attendees", "參加者")}</dt><dd>{selectedEvent.attendees}</dd></>}{selectedEvent.description && <><dt>{text("Description", "描述")}</dt><dd className="tool-prewrap">{selectedEvent.description}</dd></>}</dl>
    </div>
  </> : selection === "new" ? <>
    <div className="tool-panel__heading"><h2>{text("Create personal event", "新增個人活動")}</h2>{backToToday}</div>
    <div className="cal-panel__body">
      <p className="tool-help">{text("This adds an event to your calendar. Add guests later in Google Calendar if needed.", "此活動會加入你的日曆；如有需要，可稍後在 Google Calendar 加入受邀者。")}</p>
      <form className="tool-form" onSubmit={submit}><label>{text("Title", "標題")}<input name="title" required value={draftTitle} onChange={event => setDraftTitle(event.target.value)} /></label><div className="tool-form__row"><label>{text("Starts", "開始")}<input name="start" type="datetime-local" required value={draftStart} onChange={event => setDraftStart(event.target.value)} /></label><label>{text("Ends", "結束")}<input name="end" type="datetime-local" required value={draftEnd} onChange={event => setDraftEnd(event.target.value)} /></label></div><label>{text("Notes", "備註")}<textarea name="description" rows={4} value={draftNotes} onChange={event => setDraftNotes(event.target.value)} /></label><button className="tool-button" disabled={saving}>{saving ? text("Creating…", "建立中…") : text("Create personal event", "新增個人活動")}</button></form>
    </div>
  </> : <TodayPanel summary={summary} now={now} loading={todays.loading && !todays.data} locale={locale} text={text} onSelect={select} onSelectMail={id => setSelection(`mail-${id}`)} />;

  return <main className="tool-page calendar-tool-page">
    <header className="cal-toolbar">
      <div className="cal-toolbar__nav">
        <button className="tool-button" onClick={() => setAnchor(today)}>{text("Today", "今日")}</button>
        <button className="tool-button tool-button--quiet cal-toolbar__step" aria-label={view === "day" ? text("Previous day", "前一天") : view === "week" ? text("Previous week", "上一週") : text("Previous month", "上個月")} onClick={() => move(-1)}>‹</button>
        <button className="tool-button tool-button--quiet cal-toolbar__step" aria-label={view === "day" ? text("Next day", "後一天") : view === "week" ? text("Next week", "下一週") : text("Next month", "下個月")} onClick={() => move(1)}>›</button>
        <h1>{title}</h1>
      </div>
      <div className="tool-header-actions">
        <div className="cal-views" role="group" aria-label={text("Calendar view", "日曆檢視")}>{views.map(([value, label]) => <button key={value} className={view === value ? "is-active" : undefined} aria-pressed={view === value} onClick={() => setView(value)}>{label}</button>)}</div>
        <button className="tool-button" onClick={() => setSelection("new")}>{text("New event", "新增活動")}</button>
        <button className="tool-button" onClick={sync} disabled={syncing}>{syncing ? text("Syncing…", "同步中…") : text("Sync calendar", "同步日曆")}</button>
      </div>
    </header>
    {(loadError || actionError || notice) && <div className={loadError || actionError ? "tool-alert tool-alert--error" : "tool-alert"} role={loadError || actionError ? "alert" : "status"}>{actionError || loadError || notice}</div>}
    <div className="cal-layout">
      <section className="tool-panel cal-main" aria-busy={visible.loading}>
        {view === "month"
          ? <MonthGrid days={range.days} month={anchor.slice(0, 7)} entries={entries} now={now} selectedId={selection} locale={locale} text={text} onSelect={select} onOpenDay={openDay} />
          : <TimeGrid days={range.days} entries={entries} now={now} selectedId={selection} locale={locale} text={text} onSelect={select} onOpenDay={openDay} />}
      </section>
      <aside className="tool-panel cal-panel">{panel}</aside>
    </div>
  </main>;
}
