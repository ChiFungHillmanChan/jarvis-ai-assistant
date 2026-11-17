import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useUiLocale } from '../i18n';
import { openUrl } from '../lib/commands';
import { consentCopy, type ConsentRequest } from '../lib/ai-error';

/**
 * The one consent card (P06), wherever email and calendar content waits for
 * the user's agreement to an AI provider's data terms: inline in the chat,
 * on the Email page, in setup and Settings, and as the notice for a job in
 * the background. One click agrees (give_ai_consent); it is stored for this
 * provider and address, and can be withdrawn in Settings → Your AI.
 */
export default function ConsentCard({ request, onAgreed, onClose }: { request: ConsentRequest; onAgreed?: () => void; onClose?: () => void }) {
  const { text, locale } = useUiLocale();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const copy = consentCopy(request, locale);

  async function agree() {
    if (busy) return;
    setBusy(true); setError('');
    try {
      await invoke('give_ai_consent', { provider: request.provider, baseUrl: request.base_url || null });
      onAgreed?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return <div className="ai-card" role="group" aria-label={text('Data terms', '資料條款')}>
    <p>{copy.blocked}</p>
    {copy.billingPage && <button type="button" className="text-button" onClick={() => void openUrl(copy.billingPage!).catch(() => setError(text('Could not open the page.', '未能開啟頁面。')))}>{text('How to set up Gemini billing', '如何設定 Gemini 付費')}</button>}
    {error && <p className="setup-error" role="alert">{error}</p>}
    <div className="tool-approval-actions">
      <button type="button" className="update-toast-action" disabled={busy} onClick={() => void agree()}>{copy.confirm}</button>
      {onClose && <button type="button" className="tool-approval-decline" disabled={busy} onClick={onClose}>{text('Not now', '稍後')}</button>}
    </div>
    <small>{text('You can withdraw this in Settings → Your AI.', '可在「設定 → 你的 AI」撤回。')}</small>
  </div>;
}
