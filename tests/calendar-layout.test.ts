import test from 'node:test';
import assert from 'node:assert/strict';
import { viewRange, stepAnchor, toEntries, entryDays, layoutTimed, layoutAllDay, freeBlocks, todaySummary } from '../src/lib/calendar-layout.ts';
import type { CalendarEventView, Task } from '../src/lib/types.ts';
import type { MailItem } from '../src/lib/mail.ts';

// Local wall-clock instants, so every assertion holds in any TZ the runner uses.
const at = (d: number, h: number, m = 0, month = 8) => new Date(2026, month, d, h, m);
const iso = (d: number, h: number, m = 0, month = 8) => at(d, h, m, month).toISOString();

let nextId = 1;
const event = (start_time: string, end_time: string, summary = 'Event'): CalendarEventView =>
  ({ id: nextId++, google_id: `g${nextId}`, summary, description: null, location: null, start_time, end_time, attendees: '', status: 'confirmed', account: null });
const mailItem = (over: Partial<MailItem>): MailItem => ({
  id: nextId++, message_id: 1, kind: 'deadline', title: 'Mail item', due_date: '2026-09-26', due_time: null, end_time: null,
  location: null, evidence: '', evidence_verified: true, status: 'open', remind_at: null, reminded_at: null,
  calendar_event_id: null, subject: 's', sender: 'x', ...over,
});
const task = (deadline: string | null, status = 'pending', title = 'Task'): Task =>
  ({ id: nextId++, title, description: null, deadline, priority: 1, status, source: 'manual', created_at: null });

test('week range runs Sunday to Saturday around the anchor', () => {
  const range = viewRange('week', '2026-09-26');
  assert.equal(range.from, '2026-09-20');
  assert.equal(range.to, '2026-09-26');
  assert.equal(range.days.length, 7);
});

test('day range is the anchor alone', () => {
  assert.deepEqual(viewRange('day', '2026-09-26').days, ['2026-09-26']);
});

test('month range is six full weeks starting on the Sunday on or before the 1st', () => {
  const range = viewRange('month', '2026-10-15');
  assert.equal(range.from, '2026-09-27');
  assert.equal(range.to, '2026-11-07');
  assert.equal(range.days.length, 42);
  assert.equal(range.days[0], '2026-09-27');
});

test('stepping moves by a day, a week or a month, clamping to short months', () => {
  assert.equal(stepAnchor('day', '2026-09-30', 1), '2026-10-01');
  assert.equal(stepAnchor('week', '2026-09-26', -1), '2026-09-19');
  assert.equal(stepAnchor('month', '2026-01-31', 1), '2026-02-28');
  assert.equal(stepAnchor('month', '2026-01-15', -1), '2025-12-15');
});

test('an all-day event uses Google\'s exclusive end date', () => {
  const [single] = toEntries([event('2026-09-25', '2026-09-26')], []);
  assert.equal(single.allDay, true);
  assert.deepEqual(entryDays(single), ['2026-09-25']);
  const [multi] = toEntries([event('2026-09-24', '2026-09-27')], []);
  assert.deepEqual(entryDays(multi), ['2026-09-24', '2026-09-25', '2026-09-26']);
});

test('a timed event crossing midnight touches both days, and one ending at midnight does not', () => {
  const [late] = toEntries([event(iso(25, 22), iso(26, 1))], []);
  assert.deepEqual(entryDays(late), ['2026-09-25', '2026-09-26']);
  const [toMidnight] = toEntries([event(iso(25, 22), iso(26, 0))], []);
  assert.deepEqual(entryDays(toMidnight), ['2026-09-25']);
});

test('a timed block is clipped to each day it crosses', () => {
  const entries = toEntries([event(iso(25, 22), iso(26, 1))], []);
  const blocks = layoutTimed(entries, ['2026-09-25', '2026-09-26']);
  assert.deepEqual(blocks.map(b => [b.day, b.startMin, b.endMin]), [['2026-09-25', 22 * 60, 24 * 60], ['2026-09-26', 0, 60]]);
});

test('overlapping events share the width and a separate event keeps it all', () => {
  const entries = toEntries([
    event(iso(26, 9), iso(26, 10), 'A'),
    event(iso(26, 9, 30), iso(26, 11), 'B'),
    event(iso(26, 10, 30), iso(26, 11, 30), 'C'),
    event(iso(26, 14), iso(26, 15), 'D'),
  ], []);
  const byTitle = Object.fromEntries(layoutTimed(entries, ['2026-09-26']).map(b => [b.entry.title, [b.column, b.columns]]));
  assert.deepEqual(byTitle.A, [0, 2]);
  assert.deepEqual(byTitle.B, [1, 2]);
  assert.deepEqual(byTitle.C, [0, 2]);
  assert.deepEqual(byTitle.D, [0, 1]);
});

test('email follow-ups become entries: timed ones in the grid, undated or linked ones not at all', () => {
  const entries = toEntries([], [
    mailItem({ kind: 'event', title: 'Interview', due_time: '15:00', end_time: '15:45' }),
    mailItem({ title: 'Reply by today' }),
    mailItem({ title: 'No date', due_date: null }),
    mailItem({ title: 'Already in Google', calendar_event_id: 'abc' }),
  ]);
  assert.deepEqual(entries.map(e => [e.title, e.allDay]), [['Interview', false], ['Reply by today', true]]);
  assert.equal(entries[0].end.getTime() - entries[0].start.getTime(), 45 * 60_000);
});

test('all-day bars span their visible columns and stack into lanes', () => {
  const entries = toEntries([
    event('2026-09-18', '2026-09-22', 'Trip'),   // starts before the week
    event('2026-09-21', '2026-09-22', 'Mon'),
    event('2026-09-25', '2026-09-26', 'Fri'),
  ], []);
  const bars = layoutAllDay(entries, viewRange('week', '2026-09-26').days);
  const byTitle = Object.fromEntries(bars.map(b => [b.entry.title, [b.startCol, b.span, b.lane]]));
  assert.deepEqual(byTitle.Trip, [0, 2, 0]);
  assert.deepEqual(byTitle.Mon, [1, 1, 1]);
  assert.deepEqual(byTitle.Fri, [5, 1, 0]);
});

test('free blocks are the gaps of at least 30 minutes, ignoring short ones', () => {
  const busy = [{ start: at(26, 10), end: at(26, 11) }, { start: at(26, 11, 20), end: at(26, 14) }, { start: at(26, 13), end: at(26, 15) }];
  const blocks = freeBlocks(busy, at(26, 9), at(26, 18));
  assert.deepEqual(blocks.map(b => [b.start.getHours(), b.end.getHours()]), [[9, 10], [15, 18]]);
});

test('today summary: the event on now, the next one, free time from now, and what is due', () => {
  const now = at(26, 11);
  const events = [
    event('2026-09-26', '2026-09-27', 'Holiday'),
    event(iso(26, 10, 30), iso(26, 11, 30), 'Standup'),
    event(iso(26, 14), iso(26, 15), 'Evoke check-in'),
    event(iso(25, 9), iso(25, 10), 'Yesterday'),
  ];
  const mail = [
    mailItem({ title: 'Reply HKSTP' }),
    mailItem({ title: 'Old deadline', due_date: '2026-09-24' }),
    mailItem({ kind: 'event', title: 'Past train', due_date: '2026-09-25', due_time: '12:15' }),
    mailItem({ title: 'Next week', due_date: '2026-10-02' }),
  ];
  const tasks = [task('2026-09-25', 'pending', 'SCR filing'), task('2026-09-26', 'completed', 'Done'), task('2026-09-27', 'pending', 'Tomorrow'), task(null)];
  const summary = todaySummary(events, mail, tasks, now);

  // Dated email follow-ups that are not events belong in "due", not in the agenda.
  assert.deepEqual(summary.entries.map(e => e.title), ['Holiday', 'Standup', 'Evoke check-in']);
  assert.equal(summary.current?.title, 'Standup');
  assert.equal(summary.next?.title, 'Evoke check-in');
  assert.deepEqual(summary.free.map(b => [b.start.getHours() * 60 + b.start.getMinutes(), b.end.getHours()]), [[11 * 60 + 30, 14], [15 * 60, 18]]);
  assert.deepEqual(summary.due.map(d => [d.title, d.overdueDays]), [['Old deadline', 2], ['SCR filing', 1], ['Reply HKSTP', 0]]);
});

test('free time is empty once the working day is over', () => {
  assert.deepEqual(todaySummary([], [], [], at(26, 19)).free, []);
});
