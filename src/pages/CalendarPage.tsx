import { FormEvent, useEffect, useMemo, useState } from "react";
import * as mail from "../lib/mail";
import type { MailItem } from "../lib/mail";
import { MailItemRow } from "../components/mail/MailItems";
import { itemKindLabel } from "../lib/mail-presentation";
import "./email-workspace.css";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CalendarEventView } from "../lib/types";
import { createEvent, syncCalendar } from "../lib/commands";
import { useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";
import { useUiLocale } from "../i18n";

const dayKey = (value: Date) => `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}`;
const eventDay = (value: string) => /^\d{4}-\d{2}-\d{2}$/.test(value) ? value : dayKey(new Date(value));
const isAllDay = (event: CalendarEventView) => /^\d{4}-\d{2}-\d{2}$/.test(event.start_time);
const formatTime = (value: string, locale: string) => new Date(value).toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });

export default function CalendarPage() {
  const { locale, text } = useUiLocale();
  const [selectedDateKey, setSelectedDateKey] = useWorkspaceValue("calendarDate", dayKey(new Date()));
  const selectedDate = new Date(`${selectedDateKey}T12:00:00`);
  const setSelectedDate = (date: Date) => setSelectedDateKey(dayKey(date));
  const [selectedId, setSelectedId] = useWorkspaceValue<number | null>("calendarEventId", null);
  const [selectedMailItem, setSelectedMailItem] = useState<number | null>(null);
  const [mailItems, setMailItems] = useState<MailItem[]>([]);
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
  useEffect(() => {
    let current = true;
    // Dated follow-ups found in email; local suggestions until added to Google.
    mail.mailListItems(undefined, false).then(items => { if (current) setMailItems(items.filter(item => item.due_date && !item.calendar_event_id)); }).catch(() => undefined);
    return () => { current = false; };
  }, [weekStartKey, notice]);
  const visibleEvents = (data ?? []).filter((event) => eventDay(event.start_time) === dayKey(selectedDate));
  const visibleMail = mailItems.filter(item => item.due_date === dayKey(selectedDate));
  const busyDays = new Set([...(data ?? []).map(event => eventDay(event.start_time)), ...mailItems.map(item => item.due_date as string)]);
  const mailSelected = mailItems.find(item => item.id === selectedMailItem) ?? null;
  const moveWeek = (days: number) => { const next = new Date(selectedDate); next.setDate(selectedDate.getDate() + days); setSelectedDate(next); setSelectedId(null); setSelectedMailItem(null); };
  const selected = (data ?? []).find((event) => event.id === selectedId) ?? null;
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || text("Local time", "本機時間");

  async function sync() {
    setSyncing(true); setActionError(null); setNotice(null);
    try { await syncCalendar(weekStartKey, weekEndKey); setNotice(text("Calendar synced.", "日曆已同步。")); refetch(); } catch (error) { setActionError(String(error)); } finally { setSyncing(false); }
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

  return <main className="tool-page calendar-tool-page">
    <header className="tool-page__header"><div><p className="tool-page__eyebrow">{text("Calendar", "日曆")}</p><h1>{text("Your schedule", "你的日程")}</h1><p>{locale === "zh-HK" ? `時間以 ${timezone} 顯示。可能需要重新整理才能顯示完整的同步活動。` : `Times shown in ${timezone}. Synced events may be incomplete until you refresh.`}</p></div><div className="tool-header-actions"><button className="tool-button" onClick={() => setSelectedId(null)}>{text("New event", "新增活動")}</button><button className="tool-button" onClick={sync} disabled={syncing}>{syncing ? text("Syncing…", "同步中…") : text("Sync calendar", "同步日曆")}</button></div></header>
    {(loadError || actionError || notice) && <div className={loadError || actionError ? "tool-alert tool-alert--error" : "tool-alert"} role={loadError || actionError ? "alert" : "status"}>{actionError || loadError || notice}</div>}
    <nav className="calendar-week" aria-label={text("Choose a day", "選擇日期")}><span className="calendar-week__nav"><button className="tool-button tool-button--quiet" aria-label={text("Previous week", "上一週")} onClick={() => moveWeek(-7)}>‹</button><button className="tool-button tool-button--quiet" onClick={() => { setSelectedDate(new Date()); setSelectedMailItem(null); }}>{text("Today", "今日")}</button><button className="tool-button tool-button--quiet" aria-label={text("Next week", "下一週")} onClick={() => moveWeek(7)}>›</button></span>{days.map(day => <button key={dayKey(day)} className={`calendar-day${dayKey(day) === dayKey(selectedDate) ? " is-selected" : ""}`} onClick={() => { setSelectedDate(day); setSelectedId(null); setSelectedMailItem(null); }}><span>{day.toLocaleDateString(locale, { weekday: "short" })}</span><strong>{day.getDate()}</strong>{busyDays.has(dayKey(day)) && <i className="calendar-day__dot" aria-label={text("Has items", "有日程")} />}</button>)}</nav>
    <div className="tool-split"><section className="tool-panel tool-list-pane" aria-busy={loading}><div className="tool-panel__heading"><h2>{selectedDate.toLocaleDateString(locale, { weekday: "long", month: "long", day: "numeric" })}</h2><span>{locale === "zh-HK" ? `${visibleEvents.length + visibleMail.length} 項` : `${visibleEvents.length + visibleMail.length} items`}</span></div>
      {visibleMail.map(item => <button key={`mail-${item.id}`} className={`tool-list-row calendar-event calendar-event--mail${selectedMailItem === item.id ? " is-selected" : ""}`} onClick={() => { setSelectedMailItem(item.id); setSelectedId(null); }}><span className="calendar-event__time">{item.due_time ? `${item.due_time}${item.end_time ? `–${item.end_time}` : ""}` : text("All day", "全日")}</span><span><strong>{item.title}</strong><small>{text(`From email: ${itemKindLabel(item.kind, text)}`, `來自郵件：${itemKindLabel(item.kind, text)}`)}</small></span></button>)}
      {loading && !data ? <p className="tool-empty">{text("Loading schedule…", "正在載入日程…")}</p> : visibleEvents.length === 0 ? (visibleMail.length ? null : <p className="tool-empty">{text("Nothing on this day.", "此日期沒有日程。")}</p>) : visibleEvents.map(item => <button key={item.id} className={`tool-list-row calendar-event${selectedId === item.id ? " is-selected" : ""}`} onClick={() => { setSelectedId(item.id); setSelectedMailItem(null); }}><span className="calendar-event__time">{isAllDay(item) ? text("All day", "全日") : `${formatTime(item.start_time, locale)}–${formatTime(item.end_time, locale)}`}</span><span><strong>{item.summary}</strong><small>{item.location || item.status}</small></span></button>)}
    </section><aside className="tool-panel tool-detail-pane">{mailSelected ? <><div className="tool-panel__heading"><h2>{text("From your email", "來自郵件")}</h2></div><p className="tool-help">{text("JARVIS found this in an email. It is only on this Mac until you add it to Google Calendar.", "JARVIS 從郵件找到此項目；加入 Google 日曆前，只保存在此 Mac。")}</p><div className="mail-workspace mail-embedded"><ul className="mail-items"><MailItemRow item={mailSelected} text={text} locale={locale} onError={message => setActionError(message)} onChanged={next => { setMailItems(list => list.map(item => item.id === next.id ? next : item).filter(item => item.status === "open" && !item.calendar_event_id)); if (next.calendar_event_id) { setSelectedMailItem(null); setNotice(text("Added to Google Calendar. Sync to see it here.", "已加入 Google 日曆；同步後會在此顯示。")); } }} /></ul></div></> : selected ? <><div className="tool-panel__heading"><h2>{text("Event details", "活動詳情")}</h2></div><h3>{selected.summary}</h3><dl className="tool-details"><dt>{text("When", "時間")}</dt><dd>{isAllDay(selected) ? text("All day", "全日") : `${new Date(selected.start_time).toLocaleString(locale)} – ${new Date(selected.end_time).toLocaleString(locale)}`}</dd><dt>{text("Status", "狀態")}</dt><dd>{selected.status === "confirmed" ? text("Confirmed", "已確認") : selected.status === "tentative" ? text("Tentative", "待確認") : selected.status === "cancelled" ? text("Cancelled", "已取消") : selected.status}</dd>{selected.location && <><dt>{text("Location", "地點")}</dt><dd>{selected.location}</dd></>}{selected.attendees && <><dt>{text("Attendees", "參加者")}</dt><dd>{selected.attendees}</dd></>}{selected.description && <><dt>{text("Description", "描述")}</dt><dd className="tool-prewrap">{selected.description}</dd></>}</dl></> : <><div className="tool-panel__heading"><h2>{text("Create personal event", "新增個人活動")}</h2></div><p className="tool-help">{text("This adds an event to your calendar. Add guests later in Google Calendar if needed.", "此活動會加入你的日曆；如有需要，可稍後在 Google Calendar 加入受邀者。")}</p><form className="tool-form" onSubmit={submit}><label>{text("Title", "標題")}<input name="title" required value={draftTitle} onChange={event => setDraftTitle(event.target.value)} /></label><div className="tool-form__row"><label>{text("Starts", "開始")}<input name="start" type="datetime-local" required value={draftStart} onChange={event => setDraftStart(event.target.value)} /></label><label>{text("Ends", "結束")}<input name="end" type="datetime-local" required value={draftEnd} onChange={event => setDraftEnd(event.target.value)} /></label></div><label>{text("Notes", "備註")}<textarea name="description" rows={4} value={draftNotes} onChange={event => setDraftNotes(event.target.value)} /></label><button className="tool-button" disabled={saving}>{saving ? text("Creating…", "建立中…") : text("Create personal event", "新增個人活動")}</button></form></>}</aside></div>
  </main>;
}
