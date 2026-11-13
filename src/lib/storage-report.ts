import type { Text } from './mail-presentation';
import type { SweepReport } from './types';

/**
 * What "Clean up now" did (run_storage_sweep, src-tauri/src/commands/storage.rs).
 * AI usage is not removed: requests older than the per-request window are
 * combined into daily totals, which are kept, so they are reported apart from
 * what was removed.
 */
export function sweepSummary(report: SweepReport, text: Text): string {
  const removed = report.mailBodiesCleared + report.conversationsDeleted + report.emailsDeleted + report.briefingsDeleted;
  const combined = report.usageRowsRolledUp;
  const parts: string[] = [];
  if (removed > 0) {
    const items = [
      count(report.mailBodiesCleared, 'mail body', 'mail bodies'),
      count(report.conversationsDeleted, 'chat turn', 'chat turns'),
      count(report.emailsDeleted, 'cached email', 'cached emails'),
      count(report.briefingsDeleted, 'briefing', 'briefings'),
    ].join(', ');
    parts.push(text(
      `Removed ${count(removed, 'item', 'items')}: ${items}.`,
      `已清理 ${removed} 項：郵件內文 ${report.mailBodiesCleared}、對話記錄 ${report.conversationsDeleted}、快取郵件 ${report.emailsDeleted}、簡報 ${report.briefingsDeleted}。`,
    ));
  }
  if (combined > 0) {
    parts.push(text(
      combined === 1 ? 'Combined 1 AI usage record into the daily usage totals.' : `Combined ${combined} AI usage records into the daily usage totals.`,
      `已將 ${combined} 項 AI 用量記錄合併為每日用量總數。`,
    ));
  }
  return parts.length > 0 ? parts.join(' ') : text('Nothing was old enough to remove.', '沒有超過保留期限的資料。');
}

/** "1 mail body", "2 mail bodies", "0 mail bodies". */
function count(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}
