import { useState } from 'react';
import AiConsents from '../../components/AiConsents';
import type { EraseLeftovers } from '../../lib/commands';
import { useUiLocale } from '../../i18n';
import SettingGroup from '../../ui/SettingGroup';
import SettingRow from '../../ui/SettingRow';
import Select from '../../ui/Select';
import Switch from '../../ui/Switch';
import Button from '../../ui/Button';
import { ROWS_BY_ID, rowText } from '../rows';
import { formatBytes, useStorage } from '../hooks/useStorage';

type Result = { ok?: string; error?: string } | undefined;
const WINDOWS = ['30', '90', '180', '365'] as const;

/**
 * Settings > Privacy and data: the data terms agreed to, what JARVIS keeps on
 * this Mac, automatic clean-up, each purge as its own row, and Delete
 * everything. Every action that removes something is a danger Button, so it
 * always asks first; Delete everything also keeps its type-DELETE step.
 * "Keep recordings" lives in Voice.
 */
export default function Privacy() {
  const { locale, text } = useUiLocale();
  const store = useStorage(text);
  const s = store.storage;
  const [results, setResults] = useState<Record<string, Result>>({});
  const [erase, setErase] = useState('');
  const [left, setLeft] = useState<EraseLeftovers | null>(null);

  const row = (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });
  const set = (id: string, result: Result) => setResults(r => ({ ...r, [id]: result }));
  const fail = (id: string) => (e: unknown) => set(id, { error: e instanceof Error ? e.message : String(e) });
  // Runs one row's action and shows its outcome at that row; a failure reaches the Button's onError.
  const act = (id: string, action: () => Promise<string>) => async () => { set(id, undefined); set(id, { ok: await action() }); };
  const result = (id: string) => {
    const r = results[id];
    if (r?.error) return <span className="ui-secret-error" role="alert">{text('That did not work: ', '操作未能完成：')}{r.error}</span>;
    return r?.ok ? <span className="ui-hint" role="status">{r.ok}</span> : null;
  };
  const blocked = !s || !!store.busy;
  const amount = (bytes: number | undefined) => bytes ? formatBytes(bytes) : text('Empty', '沒有資料');
  const days = String(s?.retentionDays ?? 90) as (typeof WINDOWS)[number];
  const dayLabel = (d: string) => d === '365' ? text('1 year', '1 年') : text(`${d} days`, `${d} 日`);
  const purgeLabel = text('Delete', '刪除');

  return <>
    <SettingGroup title={text('AI services', 'AI 服務')}>
      <SettingRow {...row('ai-consents')} control={null}><AiConsents bare /></SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Storage', '儲存空間')}>
      <SettingRow {...row('storage-usage')} control={s && <strong className="settings-storage-total">{formatBytes(s.totalBytes)}</strong>}>
        {!s && !store.loadError && <span className="ui-hint" role="status">{text('Reading…', '正在讀取…')}</span>}
        {store.loadError && <span className="ui-secret-error" role="alert">{text('Could not read local storage usage.', '未能讀取本機儲存用量。')} <Button kind="quiet" onClick={store.refresh}>{text('Retry', '重試')}</Button></span>}
        {s && <>
          <dl className="settings-storage-list">
            <dt>{text('Database', '資料庫')}</dt><dd>{formatBytes(s.databaseBytes + s.walBytes)}</dd>
            <dt>{text('Voice and AI models', '語音及 AI 模型')}</dt><dd>{formatBytes(s.modelsBytes)}</dd>
            <dt>{text('Web cache', '網頁快取')}</dt><dd>{formatBytes(s.cacheBytes)}</dd>
            <dt>{text('Old backups', '舊備份')}</dt><dd>{formatBytes(s.backupsBytes)}</dd>
          </dl>
          <p className="ui-hint">{text(`${s.mailMessageCount} mail message(s) holding ${formatBytes(s.mailBodyBytes)} of message text, and ${s.conversationCount} chat turn(s). The total excludes the App itself and models managed by other apps, such as Ollama.`, `已儲存 ${s.mailMessageCount} 封郵件，內文共 ${formatBytes(s.mailBodyBytes)}；對話記錄 ${s.conversationCount} 項。總計不包括 App 本身或由 Ollama 等其他應用程式管理的模型。`)}</p>
          <p className="ui-hint settings-storage-path">{s.dataDir}</p>
        </>}
      </SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Automatic clean-up', '自動清理')}>
      <SettingRow {...row('retention-enabled')} control={
        <Switch checked={s?.retentionEnabled ?? true} disabled={blocked} label={row('retention-enabled').title}
          onChange={next => { set('retention-enabled', undefined); store.setRetention('retention-enabled', s?.retentionDays ?? 90, next).then(ok => set('retention-enabled', { ok }), fail('retention-enabled')); }} />
      }>
        <span className="ui-hint">{text('Ungrouped mail keeps its subject, sender and preview; only its text is removed. Grouped mail is kept. AI usage records are combined into daily totals after 30 days and kept for two years.', '未分組郵件只移除內文，保留主旨、寄件人及預覽；已分組的郵件會保留。AI 用量記錄會在 30 日後合併為每日用量總數，保留兩年。')}</span>
        {result('retention-enabled')}
      </SettingRow>
      <SettingRow {...row('retention-window')} control={
        <Select value={days} label={row('retention-window').title} width={160} disabled={blocked || !s?.retentionEnabled}
          options={WINDOWS.map(d => ({ value: d, label: dayLabel(d) }))}
          onChange={v => { set('retention-window', undefined); store.setRetention('retention-window', Number(v), true).then(ok => set('retention-window', { ok }), fail('retention-window')); }} />
      }>{result('retention-window')}</SettingRow>
      <SettingRow {...row('cleanup-now')} control={
        <Button kind="danger" confirm={{ message: text(`Clean up now? JARVIS removes chat history, cached emails and briefings older than ${dayLabel(days)}, and the text of ungrouped mail of that age. This cannot be undone.`, `是否立即清理？JARVIS 會移除超過${dayLabel(days)}的對話記錄、快取郵件及簡報，以及同樣期限的未分組郵件內文。此操作無法復原。`), label: text('Clean up', '清理') }}
          disabled={blocked || !s?.retentionEnabled} busy={store.busy === 'cleanup-now'} onClick={act('cleanup-now', () => store.sweep())} onError={fail('cleanup-now')}>{text('Clean up now', '立即清理')}</Button>
      }>{result('cleanup-now')}</SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Free up space', '釋放空間')}>
      <SettingRow {...row('purge-cache')} control={
        <Button kind="danger" confirm={{ message: text('Clear the web cache? It is rebuilt automatically when needed.', '是否清除網頁快取？需要時會自動重建。'), label: purgeLabel }}
          disabled={blocked || !s?.cacheBytes} busy={store.busy === 'purge-cache'} onClick={act('purge-cache', () => store.purge('purge-cache', 'cache'))} onError={fail('purge-cache')}>{amount(s?.cacheBytes)}</Button>
      }>{result('purge-cache')}</SettingRow>
      <SettingRow {...row('purge-backups')} control={
        <Button kind="danger" confirm={{ message: text('Delete old backups? Earlier backup copies are removed permanently, so you can no longer restore data or an older version from them. This cannot be undone.', '是否刪除舊備份？先前的備份副本會永久刪除，之後無法再用這些備份還原資料或舊版本。此操作無法復原。'), label: purgeLabel }}
          disabled={blocked || !s?.backupsBytes} busy={store.busy === 'purge-backups'} onClick={act('purge-backups', () => store.purge('purge-backups', 'backups'))} onError={fail('purge-backups')}>{amount(s?.backupsBytes)}</Button>
      }>{result('purge-backups')}</SettingRow>
      <SettingRow {...row('purge-models')} description={text('Offline speech and voice models. Download them again to use those features.', '離線語音及聲音模型。刪除後須重新下載才能使用相關功能。')} control={
        <Button kind="danger" confirm={{ message: text('Delete downloaded models? Offline speech and voice features stop working until you download the models again.', '是否刪除已下載的模型？重新下載模型前，離線語音功能將無法使用。'), label: purgeLabel }}
          disabled={blocked || !s?.modelsBytes} busy={store.busy === 'purge-models'} onClick={act('purge-models', () => store.purge('purge-models', 'models'))} onError={fail('purge-models')}>{amount(s?.modelsBytes)}</Button>
      }>{result('purge-models')}</SettingRow>
      <SettingRow {...row('purge-mail-bodies')} description={text('Keeps subjects and previews. Grouped mail is kept in full. Nothing in Gmail is deleted.', '保留主旨及預覽；已分組的郵件會完整保留。不會刪除 Gmail 中的郵件。')} control={
        <Button kind="danger" confirm={{ message: text('Drop stored mail text? The text of ungrouped mail is removed from this Mac; subjects, previews and grouped mail are kept, and Gmail is not touched. The text cannot be restored here, but it can sync again from Gmail.', '是否移除已儲存的郵件內文？此 Mac 上未分組郵件的內文會被移除；主旨、預覽及已分組的郵件會保留，Gmail 不受影響。內文無法在此復原，但可從 Gmail 重新同步。'), label: purgeLabel }}
          disabled={blocked || !s?.mailBodyBytes} busy={store.busy === 'purge-mail-bodies'} onClick={act('purge-mail-bodies', () => store.purge('purge-mail-bodies', 'mailBodies'))} onError={fail('purge-mail-bodies')}>{amount(s?.mailBodyBytes)}</Button>
      }>{result('purge-mail-bodies')}</SettingRow>
      <SettingRow {...row('purge-conversations')} control={
        <Button kind="danger" confirm={{ message: text('Delete all chat history? Every conversation is removed from this Mac. This cannot be undone.', '是否刪除所有對話記錄？所有對話會從此 Mac 刪除。此操作無法復原。'), label: purgeLabel }}
          disabled={blocked || !s?.conversationCount} busy={store.busy === 'purge-conversations'} onClick={act('purge-conversations', () => store.purge('purge-conversations', 'conversations'))} onError={fail('purge-conversations')}>
          {s?.conversationCount ? text(`${s.conversationCount} turns`, `${s.conversationCount} 項`) : text('Empty', '沒有資料')}
        </Button>
      }>{result('purge-conversations')}</SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Delete all JARVIS data on this Mac', '刪除此 Mac 上的所有 JARVIS 資料')}>
      <SettingRow {...row('delete-everything')} control={null}>
        <p className="ui-hint">{text("Revokes JARVIS's access to Google, then deletes everything JARVIS keeps on this Mac: copies of your mail and calendar, chat history, memory, settings, downloaded models, backups, and every API key and sign-in JARVIS saved in the Keychain. Open at login is turned off. JARVIS then restarts as a fresh install. Ollama and its models are not touched. Nothing in Gmail, Google Calendar, Notion or GitHub is deleted, and your JARVIS account stays open. This cannot be undone.", '先撤銷 JARVIS 的 Google 存取權限，然後刪除 JARVIS 在此 Mac 上保存的所有資料：郵件及日曆副本、對話記錄、記憶、設定、已下載的模型、備份，以及 JARVIS 存放在鑰匙圈的所有 API 密鑰及登入資料。「登入時開啟」亦會關閉。完成後 JARVIS 會以全新安裝狀態重新啟動。Ollama 及其中的模型不受影響。Gmail、Google 日曆、Notion 及 GitHub 中的內容不會刪除，你的 JARVIS 帳戶亦會保留。此操作無法復原。')}</p>
        <p className="ui-hint">{text('If this Mac is offline, Google cannot be told. Remove JARVIS afterwards at myaccount.google.com/permissions.', '如此 Mac 未有連線，便無法通知 Google；請之後到 myaccount.google.com/permissions 移除 JARVIS。')}</p>
        <div className="settings-row-actions settings-erase">
          <input className="ui-input" aria-label={text('Type DELETE to confirm', '請輸入 DELETE 以確認')} placeholder={text('Type DELETE to confirm', '請輸入 DELETE 以確認')}
            value={erase} autoComplete="off" spellCheck={false} disabled={!!store.busy} onChange={e => setErase(e.target.value)} />
          <Button kind="danger" confirm={{ message: text('Delete all JARVIS data on this Mac and restart? This cannot be undone.', '是否刪除此 Mac 上的所有 JARVIS 資料並重新啟動？此操作無法復原。'), label: text('Delete everything', '全部刪除') }}
            disabled={erase !== 'DELETE' || !!store.busy} busy={store.busy === 'delete-everything'}
            onClick={async () => { set('delete-everything', undefined); setLeft(null); setLeft(await store.deleteAll()); }} onError={fail('delete-everything')}>
            {store.busy === 'delete-everything' ? text('Deleting…', '正在刪除…') : text('Delete everything and restart', '刪除所有資料並重新啟動')}
          </Button>
        </div>
        {result('delete-everything')}
        {left && left.keychain.length > 0 && <div className="ui-secret-error" role="alert">
          {text('Your data was deleted, but these Keychain items could not be removed. Delete them in Keychain Access, then quit and reopen JARVIS:', '資料已刪除，但以下鑰匙圈項目未能移除。請在「鑰匙圈存取」中刪除，然後結束並重新開啟 JARVIS：')}
          <ul>{left.keychain.map(item => <li key={item}><code>{item}</code></li>)}</ul>
        </div>}
        {left?.openAtLogin && <p className="ui-secret-error" role="alert">{text('Your data was deleted, but JARVIS could not turn off Open at login, so this Mac will still open JARVIS when you log in. Turn JARVIS off in the login items settings under System Settings > General, then quit and reopen JARVIS.', '資料已刪除，但 JARVIS 未能關閉「登入時開啟」，因此你登入此 Mac 時仍會開啟 JARVIS。請在「系統設定」>「一般」的登入項目設定中關閉 JARVIS，然後結束並重新開啟 JARVIS。')}</p>}
      </SettingRow>
    </SettingGroup>
  </>;
}
