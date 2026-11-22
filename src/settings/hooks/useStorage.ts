import { useCallback, useEffect, useState } from 'react';
import { deleteAllData, getStorageUsage, purgeStorage, runStorageSweep, setRetentionPolicy } from '../../lib/commands';
import type { EraseLeftovers } from '../../lib/commands';
import type { StorageScope, StorageUsage } from '../../lib/types';
import type { Text } from '../../lib/bedrock-models';
import { sweepSummary } from '../../lib/storage-report';

/// Decimal units, matching what Finder reports. The same formatter as the old
/// Storage page (src/pages/Settings.tsx), which is deleted with it.
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 MB';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(Math.floor(Math.log10(bytes) / 3), units.length - 1);
  const value = bytes / 1000 ** index;
  return `${value >= 100 || index === 0 ? Math.round(value) : value.toFixed(1)} ${units[index]}`;
}

/**
 * Settings > Privacy and data: the storage report and every action on it, with
 * the same commands the old Storage page used. Each action resolves to a line
 * to show at its row and rejects on failure, so the row can say so. One action
 * runs at a time; `busy` holds the row id of the one running.
 */
export function useStorage(text: Text) {
  const [storage, setStorage] = useState<StorageUsage | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try { setStorage(await getStorageUsage()); setLoadError(false); }
    catch { setLoadError(true); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  async function run<T>(id: string, action: () => Promise<T>): Promise<T> {
    if (busy) throw new Error(text('Another storage action is still running.', '另一項儲存操作仍在進行。'));
    setBusy(id);
    try { return await action(); }
    finally { await refresh(); setBusy(null); }
  }

  const sweep = () => run('cleanup-now', async () => sweepSummary(await runStorageSweep(), text));
  const purge = (id: string, scope: StorageScope) => run(id, async () => {
    const freed = await purgeStorage(scope);
    return scope === 'conversations'
      ? text(`Deleted ${freed} chat turn(s).`, `已刪除 ${freed} 項對話記錄。`)
      : text(`Freed ${formatBytes(freed)}.`, `已釋放 ${formatBytes(freed)}。`);
  });
  const setRetention = (id: string, days: number, enabled: boolean) => run(id, async () => {
    await setRetentionPolicy(days, enabled);
    return enabled
      ? text(`Retention set to ${days} days.`, `保留期限已設為 ${days} 日。`)
      : text('Retention switched off. JARVIS will keep everything.', '已關閉自動清理，JARVIS 會保留所有資料。');
  });
  /** Resolves only when something was left behind: when nothing was, JARVIS restarts. */
  const deleteAll = (): Promise<EraseLeftovers> => run('delete-everything', () => deleteAllData());

  return { storage, loadError, busy, refresh, sweep, purge, setRetention, deleteAll };
}
