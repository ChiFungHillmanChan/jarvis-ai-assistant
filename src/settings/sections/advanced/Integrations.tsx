import { useEffect, useState } from 'react';
import { useUiLocale } from '../../../i18n';
import { getPreference, openUrl, setPreference } from '../../../lib/commands';
import { connectionsChanged, type IntegrationService } from '../../../lib/connections';
import { TOKEN_METHOD, useIntegrationConnection } from '../../../hooks/useIntegrationConnection';
import Button from '../../../ui/Button';
import SecretField from '../../../ui/SecretField';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import TextField from '../../../ui/TextField';
import { useRow } from './useRow';

type Connection = ReturnType<typeof useIntegrationConnection>;
type Copy = { en: string; zh: string };

/** One advanced credential per service. `stops` is what its Remove ends. */
const TOKENS: { service: IntegrationService; row: string; name: string; kind: Copy; placeholder: string; stops: Copy }[] = [
  { service: 'notion', row: 'notion-token', name: 'Notion', kind: { en: 'token', zh: '權杖' }, placeholder: 'ntn_…', stops: { en: 'JARVIS can no longer read your Notion pages until you add a token again or connect Notion.', zh: 'JARVIS 將無法讀取你的 Notion 頁面，直至你再次加入權杖或連接 Notion。' } },
  { service: 'github', row: 'github-token', name: 'GitHub', kind: { en: 'token', zh: '權杖' }, placeholder: 'ghp_…', stops: { en: 'JARVIS can no longer read your repositories or file issues until you add a token again or connect GitHub.', zh: 'JARVIS 將無法讀取你的儲存庫或建立 Issue，直至你再次加入權杖或連接 GitHub。' } },
  { service: 'obsidian', row: 'obsidian-key', name: 'Obsidian', kind: { en: 'API key', zh: 'API 密鑰' }, placeholder: 'Local REST API', stops: { en: 'JARVIS can no longer read your Obsidian notes until you add a key again or select a vault.', zh: 'JARVIS 將無法讀取你的 Obsidian 筆記，直至你再次加入密鑰或選取資料庫。' } },
];

/** A connection's notice and error, at its row. */
function Messages({ connection }: { connection: Connection }) {
  return <>
    {connection.notice && <p className="ui-hint" role="status">{connection.notice}</p>}
    {connection.error && <p className="ui-secret-error" role="alert">{connection.error}</p>}
  </>;
}

/**
 * Settings > Advanced > Integrations: the GitHub OAuth Client ID, the Notion
 * and GitHub tokens, the Obsidian REST API key and endpoint. Saves and removes
 * go through the same connection hook Connections uses; a saved token is
 * never shown, and each Remove asks first and says what stops working.
 */
export default function Integrations() {
  const { text } = useUiLocale();
  const row = useRow();
  // One hook per service, shared by its rows, so a save's status read is reported once.
  const connections: Record<IntegrationService, Connection> = {
    notion: useIntegrationConnection('notion'),
    github: useIntegrationConnection('github'),
    obsidian: useIntegrationConnection('obsidian'),
  };
  // Saved or removed here while the status could not be read again.
  const [assumed, setAssumed] = useState<Partial<Record<IntegrationService, boolean>>>({});
  const [endpoint, setEndpoint] = useState('');
  const [endpointError, setEndpointError] = useState('');
  const github = connections.github;
  // GitHub's Client ID and token share one connection; its messages show at the row last used.
  const [githubRow, setGithubRow] = useState<'client' | 'token'>('token');

  // An absent row rejects, which means the plugin default is in use.
  useEffect(() => { getPreference('obsidian_base_url').then(v => setEndpoint(v ?? '')).catch(() => {}); }, []);

  async function saveClient(value: string) {
    if (!value.trim()) return;
    setGithubRow('client');
    if (!(await github.saveClient(value))) throw new Error('not saved');
  }
  async function saveEndpoint(value: string) {
    setEndpointError('');
    try {
      await setPreference('obsidian_base_url', value.trim());
      setEndpoint(value.trim());
      connectionsChanged();
    } catch (e) {
      setEndpointError(text(`Could not save the endpoint: ${String(e)}`, `無法儲存端點：${String(e)}`));
      throw e;
    }
  }

  const tokenSaved = (service: IntegrationService) => {
    const status = connections[service].status;
    return status ? status.method === TOKEN_METHOD[service] : assumed[service] ?? false;
  };

  return <>
    <p className="ui-hint">{text('For developers. Connections has the usual way to connect these tools; a token or key here replaces that connection.', '供開發者使用。一般連接方式位於「連接」；在此加入權杖或密鑰會取代該連接。')}</p>

    <SettingGroup title="GitHub">
      <SettingRow {...row('github-client-id')} control={
        <div style={{ width: 340 }}>
          <TextField value="" label={row('github-client-id').title} placeholder="Client ID" onCommit={saveClient} />
        </div>
      }>
        <span className="ui-hint">{github.status?.oauth_ready
          ? text('A Client ID is saved. Enter a new one to replace it.', '已儲存 Client ID。輸入新的 Client ID 即可更換。')
          : text('Not set. Signing in with GitHub needs it. Enter the public Client ID only, never a Client Secret.', '尚未設定。使用 GitHub 登入時需要此項。只輸入公開的 Client ID，切勿輸入 Client Secret。')}</span>
        {githubRow === 'client' && <Messages connection={github} />}
        <span className="settings-row-actions settings-row-actions-start">
          <Button kind="quiet" onClick={() => openUrl('https://github.com/settings/applications/new')} onError={e => { setGithubRow('client'); github.setError(String(e)); }}>{text('Register GitHub application', '登記 GitHub 應用程式')}</Button>
        </span>
      </SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Tokens and keys', '權杖及密鑰')}>
      <p className="ui-hint">{text('Kept in the macOS Keychain and never shown again. To change one, choose Replace and paste the new one.', '保存在 macOS 鑰匙圈中，儲存後不會再顯示。如需更換，請按「更換」並貼上新的內容。')}</p>
      {TOKENS.map(k => {
        const connection = connections[k.service];
        const other = connection.status?.status === 'connected' && !tokenSaved(k.service);
        return (
          <SettingRow key={k.service} {...row(k.row)} control={
            <div style={{ width: 340 }}>
              <SecretField saved={tokenSaved(k.service)} label={row(k.row).title} placeholder={k.placeholder}
                errorFor={e => text(`Could not save: ${String(e)}`, `無法儲存：${String(e)}`)}
                onSave={async v => { setGithubRow('token'); await connection.saveToken(v); setAssumed(a => ({ ...a, [k.service]: true })); }}
                onRemove={async () => { setGithubRow('token'); if (await connection.disconnect()) setAssumed(a => ({ ...a, [k.service]: false })); }}
                removeConfirm={text(`Remove the saved ${k.name} ${k.kind.en}? ${k.stops.en} This cannot be undone.`, `要移除已儲存的 ${k.name} ${k.kind.zh}嗎？${k.stops.zh}此操作無法復原。`)} />
            </div>
          }>
            {other && <span className="ui-hint">{text(`${k.name} is connected the usual way. Saving here switches it to this ${k.kind.en}.`, `${k.name} 已透過一般方式連接。在此儲存會改用此${k.kind.zh}。`)}</span>}
            {(k.service !== 'github' || githubRow === 'token') && <Messages connection={connection} />}
          </SettingRow>
        );
      })}
    </SettingGroup>

    <SettingGroup title="Obsidian">
      <SettingRow {...row('obsidian-endpoint')} control={
        <div style={{ width: 340 }}>
          <TextField value={endpoint} label={row('obsidian-endpoint').title} placeholder="https://127.0.0.1:27124" onCommit={saveEndpoint} />
        </div>
      }>
        <span className="ui-hint">{text('Leave blank for the plugin default (HTTPS on port 27124). Change this only if you configured a different local endpoint.', '留空會使用外掛預設的 HTTPS 27124 連接埠。僅在已設定其他本機端點時更改。')}</span>
        {endpointError && <span className="ui-secret-error" role="alert">{endpointError}</span>}
      </SettingRow>
    </SettingGroup>
  </>;
}
