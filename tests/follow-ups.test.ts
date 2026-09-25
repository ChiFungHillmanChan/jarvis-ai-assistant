import test from 'node:test';
import assert from 'node:assert/strict';
import { followUps, localDate } from '../src/lib/follow-ups.ts';

const task = (id: number, deadline: string | null, status = 'pending') =>
  ({ id, title: `Task ${id}`, description: null, deadline, priority: 1, status, source: 'manual', created_at: null });

test('follow-ups are open tasks due within three local days, overdue first', () => {
  const tasks = [
    task(1, '2026-09-27'),
    task(2, '2026-09-20'),
    task(3, '2026-09-24'),
    task(4, '2026-09-28'),
    task(5, null),
    task(6, '2026-09-23', 'completed'),
    task(7, '2026-09-24'),
  ];
  assert.deepEqual(followUps(tasks, '2026-09-24').map(t => t.id), [2, 3, 7, 1]);
});

test('the three-day window crosses a month boundary', () => {
  assert.deepEqual(followUps([task(1, '2026-10-03'), task(2, '2026-10-04')], '2026-09-30').map(t => t.id), [1]);
});

test('local dates use the local calendar day, not UTC', () => {
  assert.equal(localDate(new Date(2026, 0, 5, 0, 30)), '2026-01-05');
});
