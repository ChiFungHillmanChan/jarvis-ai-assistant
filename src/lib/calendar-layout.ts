import type { CalendarEventView, Task } from './types';
import type { MailItem } from './mail';

// Pure date and layout logic for the Calendar page. All day keys are local
// YYYY-MM-DD; week rows start on Sunday.

export type CalendarView = 'day' | 'week' | 'month';

export interface CalendarEntry {
  id: string;
  source: 'event' | 'mail';
  sourceId: number;
  title: string;
  location: string | null;
  allDay: boolean;
  /** Timed: the instant. All-day: local midnight of the first day. */
  start: Date;
  /** Timed: the instant. All-day: local midnight after the last day (exclusive, like Google). */
  end: Date;
  mailKind: MailItem['kind'] | null;
}

export interface TimedBlock { entry: CalendarEntry; day: string; startMin: number; endMin: number; column: number; columns: number }
export interface AllDayBar { entry: CalendarEntry; startCol: number; span: number; lane: number }
export interface TimeRange { start: Date; end: Date }
export interface DueItem { source: 'mail' | 'task'; id: number; title: string; time: string | null; overdueDays: number }
export interface TodaySummary { entries: CalendarEntry[]; current: CalendarEntry | null; next: CalendarEntry | null; free: TimeRange[]; due: DueItem[] }

export const WORKDAY_START_HOUR = 9;
export const WORKDAY_END_HOUR = 18;
const MIN_FREE_MINUTES = 30;
const DATE_ONLY = /^\d{4}-\d{2}-\d{2}$/;

export const dayKey = (value: Date) => `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, '0')}-${String(value.getDate()).padStart(2, '0')}`;
/** Local midnight of a day key. */
export const dayStart = (key: string) => { const [y, m, d] = key.split('-').map(Number); return new Date(y, m - 1, d); };
export const addDays = (key: string, days: number) => { const date = dayStart(key); date.setDate(date.getDate() + days); return dayKey(date); };
const daysBetween = (from: string, to: string) => Math.round((dayStart(to).getTime() - dayStart(from).getTime()) / 86_400_000);
const localAt = (key: string, time: string) => { const [h, m] = time.split(':').map(Number); const date = dayStart(key); date.setHours(h, m || 0); return date; };

export function viewRange(view: CalendarView, anchor: string): { from: string; to: string; days: string[] } {
  let from = anchor;
  let count = 1;
  if (view === 'week') { from = addDays(anchor, -dayStart(anchor).getDay()); count = 7; }
  if (view === 'month') { const first = `${anchor.slice(0, 7)}-01`; from = addDays(first, -dayStart(first).getDay()); count = 42; }
  const days = Array.from({ length: count }, (_, index) => addDays(from, index));
  return { from, to: days[days.length - 1], days };
}

export function stepAnchor(view: CalendarView, anchor: string, direction: 1 | -1): string {
  if (view === 'day') return addDays(anchor, direction);
  if (view === 'week') return addDays(anchor, 7 * direction);
  const [y, m, d] = anchor.split('-').map(Number);
  const lastDay = new Date(y, m - 1 + direction + 1, 0).getDate();
  return dayKey(new Date(y, m - 1 + direction, Math.min(d, lastDay)));
}

/** Calendar events plus dated email follow-ups that are not yet in Google. */
export function toEntries(events: CalendarEventView[], mailItems: MailItem[]): CalendarEntry[] {
  const fromEvents = events.map((event): CalendarEntry => {
    const allDay = DATE_ONLY.test(event.start_time);
    let start: Date, end: Date;
    if (allDay) {
      start = dayStart(event.start_time);
      end = DATE_ONLY.test(event.end_time) ? dayStart(event.end_time) : dayStart(addDays(event.start_time, 1));
      if (end <= start) end = dayStart(addDays(event.start_time, 1));
    } else {
      start = new Date(event.start_time);
      end = new Date(event.end_time);
      if (Number.isNaN(end.getTime()) || end < start) end = start;
    }
    return { id: `event-${event.id}`, source: 'event', sourceId: event.id, title: event.summary, location: event.location, allDay, start, end, mailKind: null };
  });
  const fromMail = mailItems.filter(item => item.due_date && !item.calendar_event_id && item.status === 'open').map((item): CalendarEntry => {
    const day = item.due_date as string;
    const timed = Boolean(item.due_time);
    const start = timed ? localAt(day, item.due_time as string) : dayStart(day);
    let end = timed && item.end_time ? localAt(day, item.end_time) : timed ? new Date(start.getTime() + 30 * 60_000) : dayStart(addDays(day, 1));
    if (end <= start) end = new Date(start.getTime() + 30 * 60_000);
    return { id: `mail-${item.id}`, source: 'mail', sourceId: item.id, title: item.title, location: item.location, allDay: !timed, start, end, mailKind: item.kind };
  });
  return [...fromEvents, ...fromMail];
}

/** Every local day the entry touches. An end exactly at midnight does not reach into that day. */
export function entryDays(entry: CalendarEntry): string[] {
  const first = dayKey(entry.start);
  const lastInstant = entry.end > entry.start ? new Date(entry.end.getTime() - 1) : entry.start;
  const count = daysBetween(first, dayKey(lastInstant));
  return Array.from({ length: count + 1 }, (_, index) => addDays(first, index));
}

export const entriesOn = (entries: CalendarEntry[], day: string) => entries.filter(entry => entryDays(entry).includes(day));

/** Timed entries clipped to each visible day, with side-by-side columns for overlaps. */
export function layoutTimed(entries: CalendarEntry[], days: string[]): TimedBlock[] {
  const blocks: TimedBlock[] = [];
  for (const day of days) {
    const midnight = dayStart(day).getTime();
    const nextMidnight = dayStart(addDays(day, 1)).getTime();
    const segments = entries.filter(entry => !entry.allDay && entryDays(entry).includes(day)).map(entry => ({
      entry, day,
      startMin: Math.round((Math.max(entry.start.getTime(), midnight) - midnight) / 60_000),
      endMin: Math.round((Math.min(entry.end.getTime(), nextMidnight) - midnight) / 60_000),
    })).sort((a, b) => a.startMin - b.startMin || b.endMin - a.endMin);
    // Clusters of transitively overlapping blocks share one column count.
    let cluster: TimedBlock[] = [];
    let clusterEnd = -1;
    const columnEnds: number[] = [];
    const close = () => { for (const block of cluster) block.columns = columnEnds.length; blocks.push(...cluster); cluster = []; columnEnds.length = 0; };
    for (const segment of segments) {
      if (cluster.length && segment.startMin >= clusterEnd) close();
      let column = columnEnds.findIndex(end => end <= segment.startMin);
      if (column === -1) { column = columnEnds.length; columnEnds.push(0); }
      columnEnds[column] = Math.max(segment.endMin, segment.startMin + 1);
      cluster.push({ ...segment, column, columns: 1 });
      clusterEnd = Math.max(clusterEnd, columnEnds[column]);
    }
    if (cluster.length) close();
  }
  return blocks;
}

/** All-day entries as bars across the visible columns, stacked into lanes. */
export function layoutAllDay(entries: CalendarEntry[], days: string[]): AllDayBar[] {
  const bars = entries.filter(entry => entry.allDay).flatMap(entry => {
    const covered = entryDays(entry).map(day => days.indexOf(day)).filter(index => index !== -1);
    return covered.length ? [{ entry, startCol: covered[0], span: covered[covered.length - 1] - covered[0] + 1, lane: 0 }] : [];
  }).sort((a, b) => a.startCol - b.startCol || b.span - a.span);
  const laneEnds: number[] = [];
  for (const bar of bars) {
    let lane = laneEnds.findIndex(end => end < bar.startCol);
    if (lane === -1) { lane = laneEnds.length; laneEnds.push(0); }
    laneEnds[lane] = bar.startCol + bar.span - 1;
    bar.lane = lane;
  }
  return bars;
}

export function freeBlocks(busy: TimeRange[], windowStart: Date, windowEnd: Date, minMinutes = MIN_FREE_MINUTES): TimeRange[] {
  const free: TimeRange[] = [];
  let cursor = windowStart.getTime();
  const sorted = [...busy].sort((a, b) => a.start.getTime() - b.start.getTime());
  const push = (until: number) => { if (until - cursor >= minMinutes * 60_000) free.push({ start: new Date(cursor), end: new Date(until) }); };
  for (const range of sorted) {
    const start = Math.min(range.start.getTime(), windowEnd.getTime());
    if (start > cursor) push(start);
    cursor = Math.max(cursor, range.end.getTime());
    if (cursor >= windowEnd.getTime()) return free;
  }
  push(windowEnd.getTime());
  return free;
}

const isAgendaEntry = (entry: CalendarEntry) => entry.source === 'event' || entry.mailKind === 'event';

/** Today's agenda, what is on now and next, free working time left, and what is due or overdue. */
export function todaySummary(events: CalendarEventView[], mailItems: MailItem[], tasks: Task[], now: Date): TodaySummary {
  const today = dayKey(now);
  const entries = entriesOn(toEntries(events, mailItems), today).filter(isAgendaEntry)
    .sort((a, b) => Number(b.allDay) - Number(a.allDay) || a.start.getTime() - b.start.getTime());
  const timed = entries.filter(entry => !entry.allDay);
  const current = timed.find(entry => entry.start <= now && now < entry.end) ?? null;
  const next = timed.find(entry => entry.start > now) ?? null;
  const workStart = localAt(today, `${WORKDAY_START_HOUR}:00`);
  const workEnd = localAt(today, `${WORKDAY_END_HOUR}:00`);
  const windowStart = now > workStart ? now : workStart;
  const free = windowStart < workEnd ? freeBlocks(timed, windowStart, workEnd) : [];
  const due: DueItem[] = [
    ...mailItems.filter(item => item.status === 'open' && item.kind !== 'event' && item.due_date && item.due_date <= today)
      .map(item => ({ source: 'mail' as const, id: item.id, title: item.title, time: item.due_time, overdueDays: daysBetween(item.due_date as string, today) })),
    ...tasks.filter(item => item.status !== 'completed' && item.deadline && item.deadline <= today)
      .map(item => ({ source: 'task' as const, id: item.id ?? 0, title: item.title, time: null, overdueDays: daysBetween(item.deadline as string, today) })),
  ].sort((a, b) => b.overdueDays - a.overdueDays);
  return { entries, current, next, free, due };
}
