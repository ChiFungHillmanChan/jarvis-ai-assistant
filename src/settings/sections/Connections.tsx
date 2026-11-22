import { useState } from 'react';
import OtherGoogleAccounts from '../../components/OtherGoogleAccounts';
import GoogleConnectSteps from '../../components/GoogleConnectSteps';
import { offersConnectGmail } from '../../lib/google-gmail';
import { useUiLocale } from '../../i18n';
import SettingGroup from '../../ui/SettingGroup';
import SettingRow from '../../ui/SettingRow';
import Button from '../../ui/Button';
import { ROWS_BY_ID, rowText } from '../rows';
import { useSettingsDialog } from '../SettingsContext';
import { useGoogleConnection } from '../hooks/useGoogleConnection';
import IntegrationRow from './IntegrationRow';

/**
 * Settings > Connections: the main Google account (status, Gmail and Calendar,
 * switch, disconnect), other Google accounts, and one row each for Notion,
 * Obsidian and GitHub. Tokens and addresses live in Advanced > Integrations.
 */
export default function Connections() {
  const { locale, t, text } = useUiLocale();
  const { openSettings } = useSettingsDialog();
  const google = useGoogleConnection(text, t);
  // The rows whose last action failed, with the error, by row id.
  const [failed, setFailed] = useState<Record<string, string | null>>({});

  const row = (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });
  const fail = (id: string) => (e: unknown) => setFailed(f => ({ ...f, [id]: e instanceof Error ? e.message : String(e) }));
  const clear = (id: string) => setFailed(f => ({ ...f, [id]: null }));
  const failure = (id: string) => failed[id] && <span className="ui-secret-error" role="alert">{text('That did not work: ', '操作未能完成：')}{failed[id]}</span>;
  const run = (id: string, action: () => Promise<void>) => () => { clear(id); return action(); };

  const main = google.google?.main ?? null;
  const linked = !!main && main.state !== 'not_connected';
  const connected = main?.state === 'connected';
  const working = google.busy !== null;
  const chip = (on: boolean, label: string) => <span className={`settings-chip${on ? ' is-on' : ''}`}>{label}</span>;
  const signingIn = google.busy === 'connect' || google.busy === 'switch' || google.busy === 'gmail';
  const cancel = signingIn && <Button kind="quiet" onClick={google.cancel}>{text('Cancel', '取消')}</Button>;
  const who = main?.name && main.email ? `${main.name}（${main.email}）` : main?.name ?? main?.email ?? null;

  const accountStatus = google.google === null
    ? (google.loadError ? text('Google connection status is unavailable.', '未能取得 Google 連接狀態。') : text('Checking Google connection…', '正在檢查 Google 連接…'))
    : main?.state === 'reconnect' ? text('Google no longer accepts this connection. Reconnect to keep using mail tools, Calendar and briefings.', 'Google 已不接受此連接。請重新連接，以繼續使用郵件工具、日曆及簡報。')
      : connected ? (who ? `${text('Connected as', '已連接帳戶：')} ${who}` : text('Connected', '已連接'))
        : text('Not connected', '尚未連接');

  const accountAction = google.google === null
    ? (google.loadError && <Button kind="secondary" onClick={google.refresh}>{text('Retry', '重試')}</Button>)
    : main?.state === 'not_connected'
      ? <Button kind="primary" busy={google.busy === 'connect'} onClick={run('google-account', () => google.connect(false))} onError={fail('google-account')}>{google.busy === 'connect' ? text('Connecting…', '正在連接…') : text('Connect Google', '連接 Google')}</Button>
      : main?.state === 'reconnect' && <Button kind="secondary" busy={google.busy === 'connect'} disabled={working} onClick={run('google-account', () => google.connect(false))} onError={fail('google-account')}>{google.busy === 'connect' ? text('Connecting…', '正在連接…') : text('Reconnect', '重新連接')}</Button>;

  return <>
    <SettingGroup title={text('Google', 'Google')}>
      <SettingRow {...row('google-account')} control={<div className="settings-row-actions">{accountAction}{google.busy !== 'gmail' && cancel}</div>}>
        <span className={main?.state === 'reconnect' ? 'ui-secret-error' : 'ui-hint'}>{accountStatus}</span>
        {main?.state === 'not_connected' && <GoogleConnectSteps className="ui-hint" />}
        {signingIn && <p className="ui-hint">{text('Finish signing in in your browser. If you closed the Google tab, choose Cancel.', '請在瀏覽器完成登入。如已關閉 Google 登入分頁，請按「取消」。')}</p>}
        {google.notice && <p className="ui-hint" role="status">{google.notice}</p>}
        {failure('google-account')}
      </SettingRow>

      {connected && <>
        <SettingRow {...row('google-gmail')} control={<div className="settings-row-actions">
          {chip(main.gmail, main.gmail ? text('On', '已開啟') : text('Off', '未開啟'))}
          {offersConnectGmail(main) && <Button kind="secondary" busy={google.busy === 'gmail'} disabled={working} onClick={run('google-gmail', google.connectGmail)} onError={fail('google-gmail')}>{google.busy === 'gmail' ? t('googleGmailConnecting') : t('googleGmailConnect')}</Button>}
          {google.busy === 'gmail' && cancel}
        </div>}>
          <span className="ui-hint">{main.gmail ? t('googleGmailOn') : t('googleGmailOff')}</span>
          {offersConnectGmail(main) && <p className="ui-hint">{t('googleGmailHint')}</p>}
          {failure('google-gmail')}
        </SettingRow>

        <SettingRow {...row('google-calendar')} control={<div className="settings-row-actions">
          {chip(main.calendar, main.calendar ? text('Allowed', '已允許') : text('Not allowed', '未允許'))}
          {!main.calendar && <Button kind="secondary" disabled={working} onClick={run('google-calendar', () => google.connect(false))} onError={fail('google-calendar')}>{text('Allow Calendar', '允許日曆')}</Button>}
        </div>}>
          {!main.calendar && <span className="ui-secret-error" role="alert">{text('Calendar access was not allowed, so JARVIS cannot see your calendar. Choose Allow Calendar, then tick "View and edit events on all your calendars" on Google\'s page.', '未授權日曆，JARVIS 無法讀取你的日曆。請按「允許日曆」，並在 Google 頁面剔選日曆權限（View and edit events on all your calendars）。')}</span>}
          {failure('google-calendar')}
        </SettingRow>
      </>}

      {linked && <>
        <SettingRow {...row('google-switch')} control={
          <Button kind="danger" confirm={{ message: text('Switch the main Google account? This revokes this Google access (unless Mail still uses the same account) and deletes its Assistant mail cache, Calendar events and briefings on this Mac. This cannot be undone, but the new account syncs again. Mail workspace copies and other accounts are kept.', '是否切換主 Google 帳戶？此操作會撤銷目前的 Google 連接（郵件工作區仍使用同一帳戶時除外），並刪除此 Mac 上該帳戶的助手郵件快取、日曆活動及簡報。刪除後無法復原，但新帳戶會重新同步。郵件工作區副本及其他帳戶會保留。'), label: text('Switch account', '切換帳戶') }}
            busy={google.busy === 'switch'} disabled={working} onClick={run('google-switch', () => google.connect(true))} onError={fail('google-switch')}>
            {google.busy === 'switch' ? text('Opening…', '正在開啟…') : text('Switch account', '切換帳戶')}
          </Button>
        }>{failure('google-switch')}</SettingRow>

        <SettingRow {...row('google-disconnect')} description={text("Removes this account's Assistant mail cache, Calendar events and briefings. Local copies in the Mail workspace and other accounts are kept.", '會刪除此帳戶的助手郵件快取、日曆活動及簡報；郵件工作區的本機副本及其他帳戶會保留。')} control={
          <Button kind="danger" confirm={{ message: text('Disconnect the main Google account? This revokes this Google access (unless Mail still uses the same account) and deletes its Assistant mail cache, Calendar events and briefings on this Mac. This cannot be undone. Mail workspace copies and other accounts are kept. You can reconnect later.', '是否中斷主 Google 帳戶連接？此操作會撤銷 Google 連接（郵件工作區仍使用同一帳戶時除外），並刪除此 Mac 上該帳戶的助手郵件快取、日曆活動及簡報，刪除後無法復原。郵件工作區副本及其他帳戶會保留，之後可以重新連接。'), label: text('Disconnect', '中斷連接') }}
            busy={google.busy === 'disconnect'} disabled={working} onClick={run('google-disconnect', () => google.disconnect())} onError={fail('google-disconnect')}>
            {google.busy === 'disconnect' ? text('Disconnecting…', '正在中斷連接…') : text('Disconnect', '中斷連接')}
          </Button>
        }>{failure('google-disconnect')}</SettingRow>
      </>}

      {/* OtherGoogleAccounts holds the list and its own Add button; these ids let search land here. */}
      <SettingRow {...row('google-other-accounts')} control={null}>
        <span id="google-add-account" />
        {google.google && <OtherGoogleAccounts accounts={google.google.others} canAdd={connected} onChanged={google.refresh} />}
      </SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Notes and work tools', '筆記及工作工具')}>
      <IntegrationRow {...row('notion')} service="notion" />
      <IntegrationRow {...row('obsidian')} service="obsidian" />
      <IntegrationRow {...row('github')} service="github" />
      <p className="ui-hint">
        {text('Tokens, API keys and addresses for these tools are in Advanced.', '這些工具的權杖、API 密鑰及地址位於「進階」。')}{' '}
        <Button kind="quiet" onClick={() => openSettings('advanced-integrations')}>{text('Go to Advanced → Integrations', '前往「進階 → 整合」')}</Button>
      </p>
    </SettingGroup>
  </>;
}
