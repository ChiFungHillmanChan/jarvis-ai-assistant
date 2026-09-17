import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useUiLocale } from '../i18n';
import { openUrl } from '../lib/commands';
import {
  cancelConnect, connectionStatus, connectionsChanged, disconnectService,
  githubBeginConnect, githubCompleteConnect, githubSetClientId, notionConnect,
  obsidianSelectVault, openObsidian,
  type ConnectionStatus, type GitHubDevicePrompt, type IntegrationService,
} from '../lib/connections';
import { cancelOperation, currentOperation, finishOperation, isCurrentOperation, startOperation, subscribeOperations, type ConnectionOperation } from '../lib/connection-operation';
import '../pages/settings-workspace.css';

const names = { github: 'GitHub', notion: 'Notion', obsidian: 'Obsidian' };

export default function IntegrationConnection({ service }: { service: IntegrationService }) {
  const { text } = useUiLocale();
  const name = names[service];
  const [status, setStatus] = useState<ConnectionStatus | null>(null);
  const activeOperation = useSyncExternalStore(subscribeOperations, () => currentOperation(service));
  const busy = activeOperation !== null;
  const ownOperation = useRef<ConnectionOperation | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [device, setDevice] = useState<GitHubDevicePrompt | null>(null);
  const [clientId, setClientId] = useState('');
  const mounted = useRef(false);
  const revision = useRef(0);


  const refresh = useCallback(async () => {
    if (currentOperation(service)) return;
    const current = ++revision.current;
    setChecking(true);
    try {
      const value = await connectionStatus(service);
      if (mounted.current && revision.current === current) { setStatus(value); }
    } catch {
      if (mounted.current && revision.current === current) {
        setStatus(null);
        setError(previous => previous ?? text('Could not check the connection. Open JARVIS on your Mac and try again.', '未能檢查連接。請在 Mac 上開啟 JARVIS 後重試。'));
      }
    } finally { if (mounted.current && revision.current === current) setChecking(false); }
  }, [service, text]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      revision.current++;
      const operation = ownOperation.current;
      if (operation && cancelOperation(operation) && service !== 'obsidian') {
        void cancelConnect(service).catch(() => {});
      }
    };
  }, [service]);
  useEffect(() => {
    void refresh();
    const update = () => { void refresh(); };
    window.addEventListener('jarvis:connections-changed', update);
    window.addEventListener('focus', update);
    return () => {
      revision.current++;
      window.removeEventListener('jarvis:connections-changed', update);
      window.removeEventListener('focus', update);
    };
  }, [refresh]);

  async function connect() {
    const operation = startOperation(service, 'connect');
    if (!operation) return;
    ownOperation.current = operation;
    revision.current++;
    setChecking(false); setError(null); setNotice(null); setDevice(null);
    try {
      let result: ConnectionStatus;
      if (service === 'github') {
        const prompt = await githubBeginConnect();
        if (!isCurrentOperation(operation)) {
          await cancelConnect('github');
          throw new Error(text('Authorization cancelled.', '授權已取消。'));
        }
        if (mounted.current) setDevice(prompt);
        // Only the fixed, official URI is opened; device_code stays in Rust.
        try { await openUrl('https://github.com/login/device'); }
        catch { if (mounted.current) setNotice(text('Use “Open authorization page” to continue.', '請按「開啟授權頁面」繼續。')); }
        result = await githubCompleteConnect(prompt.flow_id);
      } else if (service === 'notion') result = await notionConnect();
      else result = await obsidianSelectVault();
      if (mounted.current && isCurrentOperation(operation)) {
        setStatus(result);
        setNotice(result.message ?? (result.status === 'connected' ? text('Connection verified. JARVIS can now use this service.', '已驗證連接。JARVIS 現在可以使用此服務。') : null));
      }
    } catch (e) { if (mounted.current) setError(String(e)); }
    finally {
      finishOperation(operation);
      ownOperation.current = null;
      if (mounted.current) setDevice(null);
      connectionsChanged();
    }
  }

  async function cancel() {
    const operation = ownOperation.current;
    if (service === 'obsidian' || !operation || !cancelOperation(operation)) return;
    try { await cancelConnect(service); }
    catch (e) { setError(String(e)); }
  }
  async function disconnect() {
    const operation = startOperation(service, 'disconnect');
    if (!operation) return;
    ownOperation.current = operation;
    revision.current++; setChecking(false); setError(null); setNotice(null);
    try {
      await disconnectService(service);
      setNotice(service === 'obsidian'
        ? text('Local access removed. Your notes remain in Obsidian.', '已撤銷本機存取，Obsidian 筆記會保留。')
        : text('Access disabled on this Mac. You can also revoke the grant in the service’s account settings. Your original content remains in the service.', '已停用此 Mac 的存取權限。亦可在服務的帳戶設定撤銷授權；原始內容會保留在服務中。'));
    } catch (e) { setError(String(e)); }
    finally { finishOperation(operation); ownOperation.current = null; connectionsChanged(); }
  }
  async function openService() {
    try {
      if (service === 'obsidian') await openObsidian();
      else await openUrl(service === 'github' ? 'https://github.com' : 'https://www.notion.so');
    } catch (e) { setError(String(e)); }
  }
  async function saveClient() {
    setError(null);
    try { await githubSetClientId(clientId.trim()); setClientId(''); await refresh(); connectionsChanged(); }
    catch (e) { setError(String(e)); }
  }
  const missingClient = service === 'github' && status?.oauth_ready === false;
  return <section className="integration-connection" aria-label={`${name} ${text('connection', '連接')}`}>
    <div className="integration-connection__heading">
      <h3>{name}</h3>
      <span className={`connections-status${status?.status === 'connected' ? ' is-connected' : ''}`} role="status">
        {checking ? text('Checking…', '檢查中…') : status?.status === 'connected' ? text('Verified', '已驗證連接') : status?.status === 'configured' ? text('Saved · Unverified', '已儲存 · 尚未驗證') : status ? text('Not connected', '尚未連接') : text('Status unavailable', '未能取得狀態')}
      </span>
    </div>
    {status?.identity && <p className="integration-identity">{status.identity}</p>}
    <p className="settings-help">{service === 'github'
      ? text('Sign in on GitHub using a verification code. This requests profile access and broad read/write access to public and private repositories (repo). Review the permissions on GitHub before authorizing.', '使用驗證碼在 GitHub 登入。此流程會申請個人資料，以及公開和私人儲存庫的廣泛讀寫權限（repo）。請在 GitHub 確認權限後授權。')
      : service === 'notion'
        ? status?.method === 'token'
          ? text('Using an advanced Notion API token. Read and write access depends on the token and shared pages. “Connect Notion” switches to the read-only MCP connection.', '目前使用進階 Notion API 權杖，讀寫權限取決於權杖及已分享的頁面。按「連接 Notion」可切換至唯讀 MCP 連接。')
          : text('Sign in through Notion’s official authorization page. Notion MCP can access the same workspace content as your account. JARVIS only searches and reads pages through this connection; no API key is needed.', '透過 Notion 官方授權頁面登入。Notion MCP 可存取與此帳戶相同的工作區內容；JARVIS 透過此連接僅搜尋及讀取頁面，毋須 API key。')
        : status?.method === 'rest'
          ? text('Using the Local REST API plugin. Read and write access depends on the plugin and API key. Select a local vault to switch to read-only access without a key.', '目前使用 Local REST API 外掛，讀寫權限取決於外掛及 API key。選取本機資料庫可切換至毋須密鑰的唯讀存取。')
          : text('Select an existing Obsidian vault to allow read-only access to its Markdown notes. No plugin or API key is needed. Content used in a cloud AI conversation is sent to your selected AI provider.', '選取現有 Obsidian 資料庫，允許 JARVIS 唯讀存取其中的 Markdown 筆記。毋須外掛或 API key；在雲端 AI 對話中使用的筆記內容會傳送至所選 AI 供應商。')}</p>
    <div className="integration-actions">
      <button className="connections-primary" onClick={() => { void connect(); }} disabled={busy || missingClient}>
        {busy ? text('Connecting…', '處理中…') : service === 'obsidian' ? text('Select vault and allow access', '選取資料庫並允許存取') : `${text('Connect', '連接')} ${name}`}
      </button>
      <button onClick={() => { void openService(); }}>{text('Open', '開啟')} {name}</button>
      {busy && activeOperation === ownOperation.current && activeOperation?.kind === 'connect' && service !== 'obsidian' && <button onClick={() => { void cancel(); }}>{text('Cancel authorization', '取消授權')}</button>}
      {!busy && status && status.status !== 'not_connected' && <button onClick={() => { void disconnect(); }}>{text('Disconnect', '中斷連接')}</button>}
      <button disabled={busy || checking} onClick={() => { setError(null); void refresh(); }}>{text('Check connection', '檢查連接')}</button>
    </div>
    {device && <div className="integration-device" role="status">
      <p>{text('Enter this code on GitHub to authorize JARVIS:', '請在 GitHub 輸入此驗證碼以授權 JARVIS：')}</p>
      <code>{device.user_code}</code>
      <p>{text('Keep this window open until authorization completes.', '請保持此視窗開啟，直至授權完成。')}</p>
      <button onClick={() => { void openUrl('https://github.com/login/device').catch(e => setError(String(e))); }}>{text('Open authorization page', '開啟授權頁面')}</button>
    </div>}
    {missingClient && <details className="integration-setup" open>
      <summary>{text('GitHub authorization setup required', '尚需設定 GitHub 授權')}</summary>
      <p className="settings-help">{text('The JARVIS developer must register an OAuth App and enable Device Flow once. Enter its public Client ID here; never enter a Client Secret. Published builds can include the Client ID so customers skip this step.', 'JARVIS 開發者須先登記 OAuth App 並啟用 Device Flow。請在此輸入公開 Client ID，切勿輸入 Client Secret。正式發行版可預先加入 Client ID，使用者便毋須執行此步驟。')}</p>
      <div className="integration-actions"><input aria-label="GitHub OAuth Client ID" value={clientId} onChange={e => setClientId(e.target.value)} placeholder="Client ID" autoComplete="off" /><button disabled={!clientId.trim() || busy} onClick={() => { void saveClient(); }}>{text('Save Client ID', '儲存 Client ID')}</button><button onClick={() => { void openUrl('https://github.com/settings/applications/new').catch(e => setError(String(e))); }}>{text('Register GitHub application', '登記 GitHub 應用程式')}</button></div>
    </details>}
    {status?.message && status.message !== notice && !error && <p className="settings-help">{status.message}</p>}
    {notice && <p className="settings-help" role="status">{notice}</p>}
    {error && <p className="connections-error settings-help" role="alert">{error}</p>}
  </section>;
}
