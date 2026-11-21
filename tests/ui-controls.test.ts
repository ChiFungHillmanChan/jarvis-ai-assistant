import test from 'node:test';
import assert from 'node:assert/strict';
import { formatOption, groupOptions } from '../src/ui/options.ts';
import { nextIndex } from '../src/ui/keys.ts';
import { runAction } from '../src/ui/actions.ts';

test('formatOption joins the label and the hint, or returns the label alone', () => {
  assert.equal(formatOption({ value: 'a', label: 'Gemini 3.8 Flash', hint: 'lowest cost' }), 'Gemini 3.8 Flash · lowest cost');
  assert.equal(formatOption({ value: 'a', label: 'Gemini 3.8 Flash' }), 'Gemini 3.8 Flash');
});

test('groupOptions makes one run per consecutive group and keeps the order', () => {
  const opts = [
    { value: '1', label: '1', group: 'A' },
    { value: '2', label: '2', group: 'A' },
    { value: '3', label: '3' },
    { value: '4', label: '4', group: 'B' },
  ];
  const runs = groupOptions(opts);
  assert.deepEqual(runs.map(r => r.group), ['A', null, 'B']);
  assert.deepEqual(runs.map(r => r.items.map(i => i.value)), [['1', '2'], ['3'], ['4']]);
});

test('groupOptions on an ungrouped list returns one run with a null group', () => {
  const runs = groupOptions([{ value: '1', label: '1' }, { value: '2', label: '2' }]);
  assert.equal(runs.length, 1);
  assert.equal(runs[0].group, null);
  assert.equal(runs[0].items.length, 2);
});

test('nextIndex wraps on the arrow keys and jumps on Home and End', () => {
  assert.equal(nextIndex(0, 'ArrowRight', 3), 1);
  assert.equal(nextIndex(2, 'ArrowRight', 3), 0);
  assert.equal(nextIndex(0, 'ArrowLeft', 3), 2);
  assert.equal(nextIndex(0, 'ArrowDown', 3), 1);
  assert.equal(nextIndex(2, 'ArrowDown', 3), 0);
  assert.equal(nextIndex(0, 'ArrowUp', 3), 2);
  assert.equal(nextIndex(1, 'Home', 3), 0);
  assert.equal(nextIndex(1, 'End', 3), 2);
  assert.equal(nextIndex(1, 'a', 3), null);
});

test('nextIndex returns null when there is nothing to move to', () => {
  assert.equal(nextIndex(0, 'ArrowRight', 0), null);
});

test('a danger action that is declined does not run', async () => {
  let calls = 0;
  const ran = await runAction('danger', () => { calls++; }, { message: 'x' }, async () => false);
  assert.equal(ran, false);
  assert.equal(calls, 0);
});

test('a danger action that is confirmed runs once and gets the message and label', async () => {
  let calls = 0;
  let asked: [string, string | undefined] | null = null;
  const ran = await runAction('danger', () => { calls++; }, { message: 'x', label: 'Delete' }, async (m, l) => { asked = [m, l]; return true; });
  assert.equal(ran, true);
  assert.equal(calls, 1);
  assert.deepEqual(asked, ['x', 'Delete']);
});

test('a primary action runs without asking', async () => {
  let calls = 0;
  let asks = 0;
  const ran = await runAction('primary', () => { calls++; }, undefined, async () => { asks++; return true; });
  assert.equal(ran, true);
  assert.equal(calls, 1);
  assert.equal(asks, 0);
});

test('a danger action with no confirm message throws', async () => {
  await assert.rejects(
    runAction('danger', () => {}, undefined, async () => true),
    { message: 'danger actions need a confirm message' },
  );
});

test('runAction awaits an async action', async () => {
  let done = false;
  await runAction('secondary', async () => { await Promise.resolve(); done = true; }, undefined, async () => true);
  assert.equal(done, true);
});
