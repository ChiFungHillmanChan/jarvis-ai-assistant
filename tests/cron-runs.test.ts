import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { cronRunNote, cronStatusLabel, cronTriggerLabel, schedulerSentences, schedulerText } from '../src/lib/cron-runs.ts';

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

test('a run started by hand that ended offline does not promise another try', () => {
  const manual = { status: 'skipped_offline_final', result: null, trigger: 'manual' };
  assert.match(cronRunNote(manual, en)!, /not tried again on its own/);
  assert.match(cronRunNote(manual, zh)!, /不會自動重試/);
  assert.doesNotMatch(cronRunNote(manual, en)!, /next scheduled time/);
  // A scheduled time that stayed offline keeps its own wording.
  assert.match(cronRunNote({ status: 'skipped_offline_final', result: null, trigger: null }, en)!, /until the next scheduled time/);
});

test('the fixed sentences of the scheduler are shown in the interface language', () => {
  assert.equal(schedulerText('This automation is already running', zh), '此自動排程正在執行中。');
  assert.equal(schedulerText('This automation is already running', en), 'This automation is already running');
  assert.equal(schedulerText('Briefing shown; AI summary added', zh), '已顯示簡報，並已加入 AI 摘要。');
  assert.equal(schedulerText('Already shown today', zh), '今天已顯示過簡報。');
  assert.equal(schedulerText('Not shown: this briefing was for an earlier day', zh), '沒有顯示：這次簡報屬於較早的日子。');
  // A sentence with a detail of its own keeps the detail.
  assert.equal(
    schedulerText('Briefing shown from what is saved on this Mac; the AI summary failed: No AI provider is set up', zh),
    '已根據此 Mac 上儲存的資料顯示簡報；未能取得 AI 摘要：No AI provider is set up',
  );
  assert.equal(schedulerText("Briefing time must be HH:MM between 00:00 and 23:59, not '25:00'", zh), "簡報時間必須是 00:00 至 23:59 之間的時間（HH:MM），不能是 '25:00'");
  // Anything else, a job's own output for one, is shown as it came.
  assert.equal(schedulerText('Synced 3 emails', zh), 'Synced 3 emails');
  assert.equal(schedulerText('', zh), '');
});

test('every sentence the mapping knows is one the backend still writes', () => {
  const backend = ['scheduler/engine.rs', 'scheduler/scheduled_briefing.rs', 'commands/cron.rs']
    .map(file => readFileSync(new URL(`../src-tauri/src/${file}`, import.meta.url), 'utf8'))
    .join('\n');
  for (const sentence of schedulerSentences) assert.ok(backend.includes(sentence), sentence);
});
