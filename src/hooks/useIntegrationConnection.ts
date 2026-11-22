import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useUiLocale } from '../i18n';
import { openUrl } from '../lib/commands';
import {
  cancelConnect, connectionStatuses, connectionsChanged, disconnectService,
  githubBeginConnect, githubCompleteConnect, githubSetClientId, notionConnect,
  obsidianSelectVault, openObsidian,
  type ConnectionStatus, type GitHubDevicePrompt, type IntegrationService,
} from '../lib/connections';
import { cancelOperation, currentOperation, finishOperation, isCurrentOperation, startOperation, subscribeOperations, type ConnectionOperation } from '../lib/connection-operation';

/**
 * One Notion, GitHub or Obsidian connection: its status and the connect,
 * cancel and disconnect flows. Shared by the full IntegrationConnection panel
 * and the one-line row in Settings > Connections, so both run the same flow.
 * Moved here unchanged from IntegrationConnection.tsx.
 */
export function useIntegrationConnection(service: IntegrationService) {
  const { text } = useUiLocale();
  // The last checked status shows at once; checking again waits on the service.
  const [status, setStatus] = useState<ConnectionStatus | null>(() => connectionStatuses.peek(service));
  const statusRef = useRef(status);
  const show = (value: ConnectionStatus | null) => { statusRef.current = value; setStatus(value); };
  const activeOperation = useSyncExternalStore(subscribeOperations, () => currentOperation(service));
  const busy = activeOperation !== null;
  const ownOperation = useRef<ConnectionOperation | null>(null);
  const [checking, setChecking] = useState(() => status === null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [device, setDevice] = useState<GitHubDevicePrompt | null>(null);
  const mounted = useRef(false);
  const revision = useRef(0);

  // `force` is the user asking; otherwise a recent status is reused, and a
  // known status stays on screen while it is checked again.
  const refresh = useCallback(async (force = false) => {
    if (currentOperation(service)) return;
    const current = ++revision.current;
    setChecking(force || statusRef.current === null);
    try {
      const value = await connectionStatuses.load(service, force);
      if (mounted.current && revision.current === current) { show(value); }
    } catch {
      if (mounted.current && revision.current === current) {
        show(null);
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
        show(result);
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
  /** Resolves to whether the Client ID was saved. */
  async function saveClient(clientId: string): Promise<boolean> {
    setError(null);
    try { await githubSetClientId(clientId.trim()); connectionsChanged(); return true; }
    catch (e) { setError(String(e)); return false; }
  }

  const missingClient = service === 'github' && status?.oauth_ready === false;
  // Once verified, Connect is hidden until the service needs reconnecting.
  // An advanced token or REST connection keeps it: it switches to the official one.
  const switchable = (service === 'notion' && status?.method === 'token') || (service === 'obsidian' && status?.method === 'rest');
  const needsConnect = status ? status.status !== 'connected' || switchable : !checking;
  const showConnect = activeOperation?.kind === 'connect' || (!busy && needsConnect);
  /** This screen started the connect that is running, so it may cancel it. */
  const ownConnect = busy && activeOperation === ownOperation.current && activeOperation?.kind === 'connect';

  return {
    status, checking, busy, error, setError, notice, device,
    refresh, connect, cancel, disconnect, openService, saveClient,
    missingClient, switchable, showConnect, ownConnect,
  };
}

/** The confirm a Disconnect asks first: what is removed, what stays, and that it can be connected again. */
export function disconnectConfirm(service: IntegrationService, text: (en: string, zh: string) => string): { message: string; label: string } {
  const label = text('Disconnect', '中斷連接');
  switch (service) {
    case 'notion': return { label, message: text(
      'Disconnect Notion? JARVIS deletes its saved Notion sign-in on this Mac and stops reading your pages. Your pages stay in Notion. You can connect again later.',
      '是否中斷 Notion 連接？JARVIS 會刪除此 Mac 上已儲存的 Notion 授權，並停止讀取你的頁面。頁面會保留在 Notion，之後可以重新連接。') };
    case 'github': return { label, message: text(
      'Disconnect GitHub? JARVIS deletes its saved GitHub sign-in and token on this Mac. Your repositories and issues stay on GitHub. You can connect again later.',
      '是否中斷 GitHub 連接？JARVIS 會刪除此 Mac 上已儲存的 GitHub 授權及權杖。儲存庫及 Issue 會保留在 GitHub，之後可以重新連接。') };
    case 'obsidian': return { label, message: text(
      'Disconnect Obsidian? JARVIS forgets the vault it may read and deletes its saved API key on this Mac. Your notes stay in Obsidian. You can connect again later.',
      '是否中斷 Obsidian 連接？JARVIS 會移除可讀取的資料庫，並刪除此 Mac 上已儲存的 API 金鑰。筆記會保留在 Obsidian，之後可以重新連接。') };
  }
}
