import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { X } from 'lucide-react';
import { useUiLocale } from '../i18n';

/**
 * Shown when the database key could not be read from or saved to the Keychain,
 * so this launch opened the database without encryption (src-tauri/src/db.rs).
 * Mail sync is paused until a launch that can encrypt it.
 */
export default function UnencryptedNotice() {
  const { text } = useUiLocale();
  const [shown, setShown] = useState(false);

  useEffect(() => {
    let mounted = true;
    invoke<boolean>('database_unencrypted').then(unencrypted => { if (mounted) setShown(unencrypted); }).catch(() => {});
    return () => { mounted = false; };
  }, []);

  if (!shown) return null;
  return <div className="update-toast is-error unencrypted-notice" role="alert">
    <span className="update-toast-dot" aria-hidden="true" />
    <div className="update-toast-body">
      <strong>{text('Your JARVIS data is not encrypted', '你的 JARVIS 資料未加密')}</strong>
      <span className="tool-approval-detail">{text(
        'JARVIS could not use the Keychain, so the database on this Mac is not encrypted and mail sync is paused. Restart JARVIS and choose Allow when macOS asks for Keychain access.',
        'JARVIS 未能使用鑰匙圈，此 Mac 上的資料庫因此未加密，郵件同步已暫停。請重新啟動 JARVIS，並在 macOS 要求存取鑰匙圈時選擇「允許」。',
      )}</span>
    </div>
    <button type="button" className="update-toast-close" onClick={() => setShown(false)} aria-label={text('Dismiss', '關閉')}><X size={14} /></button>
  </div>;
}
