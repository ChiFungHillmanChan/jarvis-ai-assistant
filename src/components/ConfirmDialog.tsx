import { useEffect, useRef, useState } from 'react';
import { useUiLocale, type MessageKey } from '../i18n';

interface ConfirmRequest {
  message: string;
  confirmLabel?: string;
  resolve: (confirmed: boolean) => void;
}

let openConfirm: ((request: ConfirmRequest) => void) | null = null;

/**
 * In-app replacement for window.confirm. In the macOS app window.confirm always
 * returns false without showing anything: wry implements no WKUIDelegate
 * JavaScript confirm panel, so every confirm-then-act button silently did nothing.
 */
export function confirmAction(message: string, confirmLabel?: string): Promise<boolean> {
  return new Promise(resolve => {
    if (!openConfirm) { resolve(false); return; }
    openConfirm({ message, confirmLabel, resolve });
  });
}

/**
 * Spec 6.3: sign out asks first, wherever it is offered (Settings > General's
 * account panel and the sidebar account menu), with one question. Sign out
 * ends the account session (the refresh token and cached plan in the
 * Keychain) and opens the browser logout page; nothing else on this Mac is
 * removed, which is what the question says.
 */
export function confirmSignOut(t: (key: MessageKey) => string): Promise<boolean> {
  return confirmAction(t('signOutConfirm'), t('signOut'));
}

export default function ConfirmDialog() {
  const { text } = useUiLocale();
  const [request, setRequest] = useState<ConfirmRequest | null>(null);
  const current = useRef<ConfirmRequest | null>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);

  function answer(confirmed: boolean) {
    current.current?.resolve(confirmed);
    current.current = null;
    setRequest(null);
  }

  useEffect(() => {
    openConfirm = next => {
      current.current?.resolve(false);
      current.current = next;
      setRequest(next);
    };
    return () => { openConfirm = null; current.current?.resolve(false); current.current = null; };
  }, []);

  useEffect(() => {
    if (!request) return;
    cancelButton.current?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') answer(false); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [request]);

  if (!request) return null;

  return <div className="confirm-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) answer(false); }}>
    <div className="confirm-dialog" role="alertdialog" aria-modal="true" aria-describedby="confirm-dialog-message">
      <p id="confirm-dialog-message">{request.message}</p>
      <div className="tool-approval-actions">
        <button type="button" className="update-toast-action" onClick={() => answer(true)}>{request.confirmLabel ?? text('Continue', '繼續')}</button>
        <button type="button" ref={cancelButton} className="tool-approval-decline" onClick={() => answer(false)}>{text('Cancel', '取消')}</button>
      </div>
    </div>
  </div>;
}
