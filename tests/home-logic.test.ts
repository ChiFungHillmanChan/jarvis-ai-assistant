import test from 'node:test';
import assert from 'node:assert/strict';
import { briefingSummary, focusItems, homeState, homeView } from '../src/workspace/home/home-logic.ts';
import type { FocusItem } from '../src/workspace/home/home-logic.ts';
import type { DailyBrief } from '../src/lib/types.ts';

const en = (a: string, _b: string) => a;
const zh = (_a: string, b: string) => b;
const COLLOQUIAL = /[嘅咗唔係冇啲喺嚟]/;
const TODAY = '2026-11-21';

const item = (kind: FocusItem['kind'], id: number, due: string | null): FocusItem =>
  ({ kind, id, title: `${kind} ${id}`, due, overdue: false });

test('homeView defaults to focus; only "overview" selects overview', () => {
  assert.equal(homeView(null), 'focus');
  assert.equal(homeView(undefined), 'focus');
  assert.equal(homeView(''), 'focus');
  assert.equal(homeView('nonsense'), 'focus');
  assert.equal(homeView('focus'), 'focus');
  assert.equal(homeView('overview'), 'overview');
});

test('homeState puts Google before AI', () => {
  assert.equal(homeState({ googleConnected: false, aiReady: false }), 'needs-google');
  assert.equal(homeState({ googleConnected: false, aiReady: true }), 'needs-google');
  assert.equal(homeState({ googleConnected: true, aiReady: false }), 'needs-ai');
  assert.equal(homeState({ googleConnected: true, aiReady: true }), 'ready');
});

test('focusItems: overdue follow-ups, then replies, then tasks, max 3', () => {
  const out = focusItems({
    followUps: [item('follow-up', 1, '2026-11-18'), item('follow-up', 2, '2026-11-20')],
    replies: [item('reply', 10, null), item('reply', 11, null)],
    tasks: [item('task', 20, TODAY)],
  }, TODAY);
  assert.deepEqual(out.map(i => `${i.kind}:${i.id}`), ['follow-up:1', 'follow-up:2', 'reply:10']);
});

test('focusItems: empty input gives an empty list, never padded', () => {
  assert.deepEqual(focusItems({ followUps: [], replies: [], tasks: [] }, TODAY), []);
  const one = focusItems({ followUps: [], replies: [item('reply', 1, null)], tasks: [] }, TODAY);
  assert.equal(one.length, 1);
});

test('focusItems: overdue is computed from due and today, not the input flag', () => {
  const out = focusItems({
    followUps: [
      { ...item('follow-up', 1, '2026-11-22'), overdue: true },  // due tomorrow, wrongly flagged
      { ...item('follow-up', 2, TODAY), overdue: true },         // due today is not overdue
      { ...item('follow-up', 3, '2026-11-20'), overdue: false }, // overdue, wrongly unflagged
      item('follow-up', 4, null),
    ],
    replies: [], tasks: [],
  }, TODAY);
  assert.deepEqual(out.map(i => i.id), [3]);
  assert.equal(out[0].overdue, true);
});

test('focusItems: a follow-up due tomorrow is not included', () => {
  const out = focusItems({ followUps: [item('follow-up', 1, '2026-11-22')], replies: [], tasks: [] }, TODAY);
  assert.deepEqual(out, []);
});

test('focusItems: tasks are those due today or overdue; later or undated are left out', () => {
  const out = focusItems({
    followUps: [], replies: [],
    tasks: [item('task', 1, '2026-11-22'), item('task', 2, null), item('task', 3, TODAY), item('task', 4, '2026-11-01')],
  }, TODAY);
  assert.deepEqual(out.map(i => i.id), [3, 4]);
  assert.deepEqual(out.map(i => i.overdue), [false, true]);
});

test('focusItems: de-duplicates by kind and id, keeps input order within a group', () => {
  const out = focusItems({
    followUps: [item('follow-up', 5, '2026-11-01'), item('follow-up', 5, '2026-11-01'), item('follow-up', 3, '2026-11-02')],
    replies: [item('reply', 5, null), item('reply', 5, null)],
    tasks: [],
  }, TODAY);
  assert.deepEqual(out.map(i => `${i.kind}:${i.id}`), ['follow-up:5', 'follow-up:3', 'reply:5']);
});

const brief = (local: string, date = TODAY): DailyBrief =>
  ({ date, shown_at: `${date} 08:00:00`, trigger: 'scheduled', local, summary: null, summary_pending: false, summary_error: null });

const EN_LOCAL = ['Saturday 21 November', 'Meetings: 2 today', '  09:00　Standup', 'Tasks: 1 overdue, 2 due today', 'Mail: 3 unread'].join('\n');
const ZH_LOCAL = ['11 月 21 日（星期六）', '會議：今天有 2 個', '  09:00　晨會', '工作：1 項已逾期，2 項今天到期', '電郵：3 封未讀'].join('\n');

test('briefingSummary is null with no brief or a brief for another day', () => {
  assert.equal(briefingSummary(null, en, TODAY), null);
  assert.equal(briefingSummary(brief(EN_LOCAL, '2026-11-20'), en, TODAY), null);
});

test('briefingSummary uses only the counts the brief has (English)', () => {
  assert.equal(briefingSummary(brief(EN_LOCAL), en, TODAY), "Today's briefing: 2 meetings, 3 tasks due, 3 unread emails");
});

test('briefingSummary reads a Chinese brief and speaks the UI language', () => {
  const line = briefingSummary(brief(ZH_LOCAL), zh, TODAY)!;
  assert.equal(line, '今日簡報：2 個會議，3 項工作到期，3 封未讀郵件');
  assert.equal(COLLOQUIAL.test(line), false);
  assert.equal(briefingSummary(brief(ZH_LOCAL), en, TODAY), "Today's briefing: 2 meetings, 3 tasks due, 3 unread emails");
});

test('briefingSummary leaves out zero or unsynced parts and handles singulars', () => {
  const some = ['Wed', 'Meetings: 1 today', 'Tasks: nothing overdue or due today', 'Mail: nothing unread'].join('\n');
  assert.equal(briefingSummary(brief(some), en, TODAY), "Today's briefing: 1 meeting");
  const single = ['Wed', 'Meetings: none today', 'Tasks: 0 overdue, 1 due today', 'Mail: 1 unread'].join('\n');
  assert.equal(briefingSummary(brief(single), en, TODAY), "Today's briefing: 1 task due, 1 unread email");
  const unsynced = ['Wed', 'Meetings: the calendar has not been synced yet', 'Tasks: nothing overdue or due today', 'Mail: not synced yet'].join('\n');
  assert.equal(briefingSummary(brief(unsynced), en, TODAY), "Today's briefing is ready");
  assert.equal(briefingSummary(brief(unsynced), zh, TODAY), '今日簡報已備妥');
  assert.equal(briefingSummary(brief(''), en, TODAY), "Today's briefing is ready");
});

test('all zh strings avoid colloquial Cantonese', () => {
  for (const local of [ZH_LOCAL, '', '會議：今天沒有會議\n工作：沒有已逾期或今天到期的工作\n電郵：沒有未讀郵件']) {
    assert.equal(COLLOQUIAL.test(briefingSummary(brief(local), zh, TODAY)!), false);
  }
});
