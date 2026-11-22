// Pure helpers for the home views: what each card is built from and the few
// lines of copy that depend on counts. Tested under node (tests/home-views.test.ts).
import type { Text } from '../../lib/bedrock-models.ts';
import type { MailItem, MailTriageStatus } from '../../lib/mail.ts';
import type { CalendarEventView, CronJobView, Task } from '../../lib/types.ts';
import { followUps } from '../../lib/follow-ups.ts';
import { defaultRemindAt } from '../../lib/mail-presentation.ts';
import type { FocusItem } from './home-logic.ts';

/** The mail view's reminder format: local "YYYY-MM-DDTHH:MM" (mail/store.rs update_item). */
function localMinute(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** An item the user asked to be reminded about later is off the home page until then. */
export function isSnoozed(remindAt: string | null, now: Date): boolean {
  return !!remindAt && remindAt.slice(0, 16) > localMinute(now);
}

/** "Remind me tomorrow": 09:00 tomorrow, as the mail view's own default for an undated item. */
export function remindTomorrow(now: Date): string {
  return defaultRemindAt(null, null, now);
}

const day = (value: string | null) => value ? value.slice(0, 10) : null;

/** The open replies, in the order mailListItems gives them, without snoozed ones. */
export function openReplies(items: MailItem[], now: Date): MailItem[] {
  return items.filter(i => i.kind === 'reply' && i.status === 'open' && !isSnoozed(i.remind_at, now));
}

/** Follow-ups for the Overview column: overdue first, then due today. */
export function followUpsDue(tasks: Task[], today: string): Task[] {
  return followUps(tasks, today).filter(t => day(t.deadline)! <= today);
}

/** The three groups focusItems chooses from, with dates normalised to YYYY-MM-DD. */
export function focusInputs(tasks: Task[], items: MailItem[], today: string, now: Date): { followUps: FocusItem[]; replies: FocusItem[]; tasks: FocusItem[] } {
  const open = items.filter(i => i.status === 'open' && !isSnoozed(i.remind_at, now));
  const fromMail = (i: MailItem, kind: FocusItem['kind']): FocusItem => ({ kind, id: i.id, title: i.title, due: day(i.due_date), overdue: false });
  return {
    followUps: followUps(tasks, today).filter(t => t.id !== null).map(t => ({ kind: 'follow-up', id: t.id!, title: t.title, due: day(t.deadline), overdue: false })),
    replies: open.filter(i => i.kind === 'reply').map(i => fromMail(i, 'reply')),
    tasks: open.filter(i => i.kind === 'task' || i.kind === 'deadline').map(i => fromMail(i, 'task')),
  };
}

/**
 * Sorted mail that does not need attention: every triaged message has a
 * category, and attention is a subset of them. Across all synced mail, not
 * only today's. Null when there is nothing to count.
 */
export function filedCount(triage: MailTriageStatus | null): number | null {
  if (!triage) return null;
  const sorted = triage.categories.reduce((sum, [, n]) => sum + n, 0);
  const filed = sorted - triage.attention;
  return filed > 0 ? filed : null;
}

export function canWaitLine(n: number, text: Text): string {
  return text(n === 1 ? '1 sorted email does not need your attention' : `${n} sorted emails do not need your attention`, `${n} 封已分類的郵件不需要你處理`);
}

/** The footer: up to two active routines by name, then "+N". Null when none is active. */
export function runningSummary(jobs: CronJobView[], text: Text): string | null {
  const active = jobs.filter(j => j.status === 'active');
  if (!active.length) return null;
  const more = active.length > 2 ? ` +${active.length - 2}` : '';
  const names = active.slice(0, 2).map(j => j.name);
  return text(`Running for you: ${names.join(', ')}${more}`, `正在為你執行：${names.join('、')}${more}`);
}

const timed = (e: CalendarEventView) => e.start_time.includes('T');

/** The first timed event today that has not started yet. */
export function nextEventId(events: CalendarEventView[], now: Date): number | null {
  const next = events.filter(e => timed(e) && new Date(e.start_time) > now)
    .sort((a, b) => new Date(a.start_time).getTime() - new Date(b.start_time).getTime())[0];
  return next ? next.id : null;
}

/** All-day events first, then by start time. */
export function sortEvents(events: CalendarEventView[]): CalendarEventView[] {
  return [...events].sort((a, b) => Number(timed(a)) - Number(timed(b)) || new Date(a.start_time).getTime() - new Date(b.start_time).getTime());
}

export function eventTime(e: CalendarEventView, locale: string, text: Text): string {
  if (!timed(e)) return text('All day', '全日');
  const d = new Date(e.start_time);
  return isNaN(d.getTime()) ? '' : d.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit', hour12: false });
}

/** What "Draft a reply" asks JARVIS in chat. JARVIS drafts in Gmail; it never sends. */
export function draftReplyPrompt(item: MailItem, text: Text): string {
  const subject = item.subject || text('(no subject)', '（無主旨）');
  return text(
    `Draft a reply to the email from ${item.sender} with the subject "${subject}". Save it as a Gmail draft for me to review.`,
    `請為 ${item.sender} 寄來、主旨為「${subject}」的郵件草擬回覆，並儲存為 Gmail 草稿讓我檢查。`,
  );
}
