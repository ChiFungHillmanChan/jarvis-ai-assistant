import test from 'node:test';
import assert from 'node:assert/strict';
import { historyStart, RECENT_MESSAGES } from '../src/workspace/history.ts';

test('a long conversation opens on its most recent messages', () => {
  assert.equal(RECENT_MESSAGES, 40);
  assert.equal(historyStart(200, null), 160);
  assert.equal(historyStart(12, null), 0);
});

test('new messages do not push the shown history off screen once it is fixed', () => {
  const hidden = historyStart(200, null);
  assert.equal(historyStart(202, hidden), 160);
  assert.equal(historyStart(202, 0), 0, 'showing everything stays that way');
});

test('a shorter reloaded history never starts past its end', () => {
  assert.equal(historyStart(100, 160), 100);
});
