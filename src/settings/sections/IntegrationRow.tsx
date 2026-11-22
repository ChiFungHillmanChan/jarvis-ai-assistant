import { useUiLocale } from '../../i18n';
import { openUrl } from '../../lib/commands';
import type { IntegrationService } from '../../lib/connections';
import { disconnectConfirm, useIntegrationConnection } from '../../hooks/useIntegrationConnection';
import SettingRow from '../../ui/SettingRow';
import Button from '../../ui/Button';
import { useSettingsDialog } from '../SettingsContext';

const names = { github: 'GitHub', notion: 'Notion', obsidian: 'Obsidian' };

/**
 * Settings > Connections: one Notion, GitHub or Obsidian row with its status
 * and one action, using the same connect and disconnect flow as the full
 * IntegrationConnection panel. Tokens, keys and addresses are never shown
 * here: when GitHub still needs its OAuth Client ID, the action opens
 * Advanced > Integrations instead.
 */
export default function IntegrationRow({ id, title, description, service }: { id: string; title: string; description?: string; service: IntegrationService }) {
  const { text } = useUiLocale();
  const { openSettings } = useSettingsDialog();
  const connection = useIntegrationConnection(service);
  const name = names[service];
  const status = connection.checking ? text('Checking…', '檢查中…')
    : connection.status?.status === 'connected' ? text('Connected', '已連接')
      : connection.status?.status === 'configured' ? text('Saved · Unverified', '已儲存 · 尚未驗證')
        : connection.status ? text('Not connected', '尚未連接') : text('Status unavailable', '未能取得狀態');
  const connectLabel = connection.busy ? text('Connecting…', '處理中…')
    : service === 'obsidian' ? text('Select vault', '選取資料庫')
      : connection.status?.status === 'configured' && !connection.switchable ? text('Reconnect', '重新連接') : text('Connect', '連接');
  const canDisconnect = !connection.busy && !!connection.status && connection.status.status !== 'not_connected';

  const control = (
    <div className="settings-row-actions">
      <span className={`settings-chip${connection.status?.status === 'connected' ? ' is-on' : ''}`} role="status">{status}</span>
      {connection.missingClient
        ? <Button kind="secondary" onClick={() => openSettings('advanced-integrations')}>{text('Set up in Advanced', '在進階設定中設定')}</Button>
        : connection.showConnect && <Button kind="secondary" busy={connection.busy} onClick={() => connection.connect()}>{connectLabel}</Button>}
      {connection.ownConnect && service !== 'obsidian' && <Button kind="quiet" onClick={() => connection.cancel()}>{text('Cancel', '取消')}</Button>}
      {canDisconnect && <Button kind="danger" confirm={disconnectConfirm(service, text)} onClick={() => connection.disconnect()}>{text('Disconnect', '中斷連接')}</Button>}
    </div>
  );

  return (
    <SettingRow id={id} title={title} description={description} control={control}>
      {connection.status?.identity && <span className="ui-hint">{connection.status.identity}</span>}
      {connection.device && <div className="integration-device" role="status">
        <p>{text(`Enter this code on GitHub to authorize JARVIS:`, '請在 GitHub 輸入此驗證碼以授權 JARVIS：')}</p>
        <code>{connection.device.user_code}</code>
        <Button kind="secondary" onClick={() => openUrl('https://github.com/login/device')} onError={e => connection.setError(String(e))}>{text('Open authorization page', '開啟授權頁面')}</Button>
      </div>}
      {connection.notice && <p className="ui-hint" role="status">{connection.notice}</p>}
      {connection.error && <p className="ui-secret-error" role="alert">{connection.error}</p>}
      {connection.missingClient && <p className="ui-hint">{text(`${name} needs its OAuth Client ID first. It is entered in Advanced, under Integrations.`, `${name} 須先設定 OAuth Client ID，可在「進階」的「整合」中輸入。`)}</p>}
    </SettingRow>
  );
}
