import test from 'node:test';
import assert from 'node:assert/strict';
import { cronRunNote, cronStatusLabel, cronTriggerLabel } from '../src/lib/cron-runs.ts';

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

test('every status the scheduler writes has a label in both languages', () => {
  for (const status of ['active', 'paused', 'running', 'completed', 'failed', 'interrupted', 'skipped_offline', 'skipped_offline_final', 'collapsed']) {
    assert.notEqual(cronStatusLabel(status, en), status, status);
    assert.notEqual(cronStatusLabel(status, zh), status, status);
  }
  assert.equal(cronStatusLabel('something_new', zh), 'something_new', 'an unknown status is shown as stored');
});

test('every trigger has a label, and a row without one has none', () => {
  for (const trigger of ['scheduled', 'catch_up', 'manual', 'retry_interrupted']) {
    assert.ok(cronTriggerLabel(trigger, en), trigger);
    assert.ok(cronTriggerLabel(trigger, zh), trigger);
  }
  assert.equal(cronTriggerLabel(null, en), null);
});

test('a collapsed row says how many scheduled times were missed', () => {
  const row = (count: number, capped = false) => ({ status: 'collapsed', result: JSON.stringify({ count, capped, from: '2026-11-12T09:00:00+08:00', to: '2026-11-12T10:00:00+08:00' }) });
  assert.match(cronRunNote(row(1), en)!, /^1 earlier scheduled time passed/);
  assert.match(cronRunNote(row(12), en)!, /^12 earlier scheduled times passed/);
  assert.match(cronRunNote(row(12), zh)!, /^有 12 個較早的排程時間/);
  assert.match(cronRunNote(row(5000, true), en)!, /^At least 5000 /);
  assert.match(cronRunNote(row(5000, true), zh)!, /^最少有 5000 個/);
  assert.match(cronRunNote({ status: 'collapsed', result: 'not json' }, en)!, /^Earlier scheduled times passed/);
});

test('an offline wait is not described as a failure, and an ordinary run has no note', () => {
  assert.match(cronRunNote({ status: 'skipped_offline', result: null }, en)!, /runs this when the connection returns/);
  assert.match(cronRunNote({ status: 'skipped_offline_final', result: null }, en)!, /was not run/);
  assert.ok(cronRunNote({ status: 'interrupted', result: null }, zh));
  assert.equal(cronRunNote({ status: 'completed', result: 'Synced 3 emails' }, en), null);
  assert.equal(cronRunNote({ status: 'failed', result: null }, en), null);
});
