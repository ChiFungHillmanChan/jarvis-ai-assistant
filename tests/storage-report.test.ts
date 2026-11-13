import test from 'node:test';
import assert from 'node:assert/strict';
import { sweepSummary } from '../src/lib/storage-report.ts';
import type { SweepReport } from '../src/lib/types.ts';

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

function report(change: Partial<SweepReport>): SweepReport {
  return { mailBodiesCleared: 0, conversationsDeleted: 0, emailsDeleted: 0, usageRowsRolledUp: 0, briefingsDeleted: 0, ...change };
}

// The clean-up deletes no AI usage: what it combined into the daily totals
// is not counted as removed.
test('usage combined into the daily totals is not reported as removed', () => {
  assert.equal(sweepSummary(report({}), en), 'Nothing was old enough to remove.');
  assert.equal(sweepSummary(report({ usageRowsRolledUp: 12 }), en), 'Combined 12 AI usage records into the daily usage totals.');
  assert.equal(sweepSummary(report({ usageRowsRolledUp: 1 }), en), 'Combined 1 AI usage record into the daily usage totals.');
  assert.equal(sweepSummary(report({ usageRowsRolledUp: 12 }), zh), '已將 12 項 AI 用量記錄合併為每日用量總數。');
  assert.equal(
    sweepSummary(report({ mailBodiesCleared: 2, conversationsDeleted: 3, emailsDeleted: 1, briefingsDeleted: 4, usageRowsRolledUp: 9 }), en),
    'Removed 10 item(s): 2 mail bodies, 3 chat turns, 1 cached emails, 4 briefings. Combined 9 AI usage records into the daily usage totals.',
  );
  assert.equal(
    sweepSummary(report({ mailBodiesCleared: 2, conversationsDeleted: 3, emailsDeleted: 1, briefingsDeleted: 4 }), zh),
    '已清理 10 項：郵件內文 2、對話記錄 3、快取郵件 1、簡報 4。',
  );
});
