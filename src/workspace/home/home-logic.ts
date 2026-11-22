import type { Text } from '../../lib/bedrock-models.ts';
import type { DailyBrief } from '../../lib/types.ts';

export type HomeView = 'overview' | 'focus';

/** New users and anyone with no saved choice land on Focus. */
export function homeView(saved: string | null | undefined): HomeView {
  return saved === 'overview' ? 'overview' : 'focus';
}

export type HomeState = 'needs-google' | 'needs-ai' | 'ready';

/** Google comes first: without it there is nothing to look at, whichever AI is set up. */
export function homeState(i: { googleConnected: boolean; aiReady: boolean }): HomeState {
  if (!i.googleConnected) return 'needs-google';
  return i.aiReady ? 'ready' : 'needs-ai';
}

export interface FocusItem { kind: 'follow-up' | 'reply' | 'task'; id: number; title: string; due: string | null; overdue: boolean }

const FOCUS_MAX = 3;

/**
 * Up to three items that need the user today: overdue follow-ups, then replies
 * (already filtered to reply-needed by the caller, kept in the order given),
 * then tasks due today or overdue. Input order is kept within a group, items
 * are de-duplicated by (kind, id), and the list is never padded.
 *
 * Overdue is computed here from `due < today` (YYYY-MM-DD strings, `today` in
 * local date form). The `overdue` flag on the input is ignored, and the items
 * returned carry the recomputed flag. A follow-up due tomorrow, or with no due
 * date, is therefore not an overdue follow-up.
 */
export function focusItems(i: { followUps: FocusItem[]; replies: FocusItem[]; tasks: FocusItem[] }, today: string): FocusItem[] {
  const isOverdue = (item: FocusItem) => item.due !== null && item.due < today;
  const withFlag = (item: FocusItem): FocusItem => ({ ...item, overdue: isOverdue(item) });
  const ordered = [
    ...i.followUps.filter(isOverdue),
    ...i.replies,
    ...i.tasks.filter(item => item.due === today || isOverdue(item)),
  ];
  const seen = new Set<string>();
  const out: FocusItem[] = [];
  for (const item of ordered) {
    const key = `${item.kind}:${item.id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(withFlag(item));
    if (out.length === FOCUS_MAX) break;
  }
  return out;
}

// A DailyBrief carries no structured counts, only the text the backend wrote
// (`local`, in the UI language of the day it was made: scheduled_briefing.rs,
// LocalBrief::text). The counts are read back from those lines, in either
// language, so the summary line follows the language the user sees now. A line
// that does not match (unsynced, "none today") contributes nothing.
const MEETINGS = [/^Meetings: (\d+) today/m, /^會議：今天有 (\d+) 個/m];
const TASKS = [/^Tasks: (\d+) overdue, (\d+) due today/m, /^工作：(\d+) 項已逾期，(\d+) 項今天到期/m];
const UNREAD = [/^Mail: (\d+) unread/m, /^電郵：(\d+) 封未讀/m];

function numbers(local: string, patterns: RegExp[]): number[] | null {
  for (const pattern of patterns) {
    const match = pattern.exec(local);
    if (match) return match.slice(1).map(Number);
  }
  return null;
}

/**
 * One line for the top of the home page, or null when there is no brief for
 * `today` (a local YYYY-MM-DD). The brief has no count of emails needing a
 * reply, only unread mail, so that is what the line says. Zero counts are left
 * out; with none left, a plain "ready" line.
 */
export function briefingSummary(brief: DailyBrief | null, text: Text, today: string): string | null {
  if (!brief || brief.date !== today) return null;
  const meetings = numbers(brief.local, MEETINGS)?.[0] ?? 0;
  const tasks = (numbers(brief.local, TASKS) ?? [0, 0]).reduce((a, b) => a + b, 0);
  const unread = numbers(brief.local, UNREAD)?.[0] ?? 0;
  const parts: string[] = [];
  if (meetings > 0) parts.push(text(`${meetings} ${meetings === 1 ? 'meeting' : 'meetings'}`, `${meetings} 個會議`));
  if (tasks > 0) parts.push(text(`${tasks} ${tasks === 1 ? 'task' : 'tasks'} due`, `${tasks} 項工作到期`));
  if (unread > 0) parts.push(text(`${unread} unread ${unread === 1 ? 'email' : 'emails'}`, `${unread} 封未讀郵件`));
  if (!parts.length) return text("Today's briefing is ready", '今日簡報已備妥');
  return text(`Today's briefing: ${parts.join(', ')}`, `今日簡報：${parts.join('，')}`);
}
