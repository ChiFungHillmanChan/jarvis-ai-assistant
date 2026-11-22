import { useCallback, useEffect, useRef, useState } from 'react';
import { googleAccounts, googleCancelConnect, googleConnect, googleConnectGmail, googleDisconnect, type GoogleAccounts } from '../../lib/commands';
import { GOOGLE_ALREADY_CONNECTED, GOOGLE_CALENDAR_NOT_ALLOWED } from '../../lib/types';
import { gmailConnectErrorKey } from '../../lib/google-gmail';
import type { Text } from '../../lib/bedrock-models';
import type { MessageKey } from '../../i18n/catalog';

type Busy = 'connect' | 'switch' | 'gmail' | 'disconnect' | null;

/**
 * Settings > Connections: the main Google account, with the same commands the
 * old Google page used. The state is always read back from the backend after
 * an action, so Disconnect and Switch settle on the truth. The confirm step for
 * Switch and Disconnect is the danger Button's, not this hook's. Each action
 * rejects on failure so its row can show the error; a cancelled sign-in is a
 * notice, not a failure.
 */
export function useGoogleConnection(text: Text, t: (key: MessageKey) => string) {
  const [google, setGoogle] = useState<GoogleAccounts | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState<Busy>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const cancelled = useRef(false);

  const refresh = useCallback(async () => {
    try { setGoogle(await googleAccounts()); setLoadError(false); }
    catch { setGoogle(null); setLoadError(true); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  async function run(kind: Exclude<Busy, null>, action: () => Promise<string | null>) {
    setBusy(kind); setNotice(null); cancelled.current = false;
    try {
      setNotice(await action());
    } catch (e) {
      // Cancelling a sign-in the user abandoned is not a failure.
      if (cancelled.current) { setNotice(text('Google sign-in cancelled.', '已取消 Google 登入。')); return; }
      throw e;
    } finally {
      await refresh();
      setBusy(null);
    }
  }

  /** `force` switches account: it revokes the current one first (Switch account). */
  const connect = (force: boolean) => run(force ? 'switch' : 'connect', async () => {
    const result = await googleConnect(force);
    if (result === GOOGLE_ALREADY_CONNECTED) return text('That account is already linked. Use Switch account to change it.', '此帳戶已連接。請使用「切換帳戶」更換。');
    return result === GOOGLE_CALENDAR_NOT_ALLOWED ? null : result;
  });
  // Gmail is opt-in on the same account; the backend keeps the session if another account is chosen.
  const connectGmail = () => run('gmail', async () => {
    try { await googleConnectGmail(); }
    catch (e) {
      const key = gmailConnectErrorKey(e);
      if (key && !cancelled.current) throw new Error(t(key));
      throw e;
    }
    return t('googleGmailConnected');
  });
  const disconnect = () => run('disconnect', googleDisconnect);
  // Closing the browser tab tells JARVIS nothing; this stops the wait.
  const cancel = () => { cancelled.current = true; void googleCancelConnect().catch(() => {}); };

  return { google, loadError, busy, notice, refresh, connect, connectGmail, disconnect, cancel };
}
