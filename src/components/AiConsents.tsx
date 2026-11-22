import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useUiLocale } from '../i18n';
import { consentStatus, type GivenConsent } from '../lib/ai-error';
import Button from '../ui/Button';

/**
 * Settings > Privacy and data: the data terms the user agreed to on the consent
 * card (ai::consent::list_ai_consents), and Withdraw for each. Withdrawing
 * holds email and calendar content back from that AI again. `bare`: the
 * Settings > Privacy and data row, which has its own title, so only the list
 * shows, and an empty list says so instead of hiding the row.
 */
export default function AiConsents({ bare }: { bare?: boolean } = {}) {
  const { text } = useUiLocale();
  const [given, setGiven] = useState<GivenConsent[] | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    let live = true;
    const refresh = () => void invoke<GivenConsent[]>('list_ai_consents').then(list => { if (live) setGiven(list); }).catch(e => { if (live) setError(String(e)); });
    refresh();
    const stop = listen('ai-consent-changed', refresh);
    return () => { live = false; void stop.then(unlisten => unlisten()).catch(() => {}); };
  }, []);

  async function withdraw(consent: GivenConsent) {
    setError('');
    try { await invoke('revoke_ai_consent', { provider: consent.provider, baseUrl: consent.base_url || null }); }
    catch (e) { setError(String(e)); }
  }

  const list = <>
    {given?.map(consent => <div key={`${consent.provider}|${consent.base_url}`} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, marginTop: 8 }}>
      <span><strong>{consent.label}</strong> <small>{consentStatus(consent, text)}</small></span>
      <Button kind="danger" confirm={{ message: text('Withdraw this agreement? JARVIS stops sending email and calendar content to this AI until you agree again on its consent card. Content already sent is not recalled.', '是否撤回此項同意？JARVIS 會停止向此 AI 傳送電郵及日曆內容，直至你再次在同意卡上按同意。已傳送的內容不會收回。'), label: text('Withdraw', '撤回') }} onClick={() => withdraw(consent)}>{text('Withdraw', '撤回')}</Button>
    </div>)}
    {error && <p className="setup-error" role="alert">{error}</p>}
  </>;
  if (bare) {
    if (given?.length === 0 && !error) return <p className="ui-hint">{text('You have not agreed to send email or calendar content to any AI service.', '你尚未同意向任何 AI 服務傳送電郵或日曆內容。')}</p>;
    return list;
  }
  if (!given?.length && !error) return null;
  return <div className="settings-panel">
    <h2 className="settings-heading">{text('Data terms you agreed to', '已同意的資料條款')}</h2>
    <p className="settings-help">{text('Email and calendar content goes to these AI services because you agreed on the consent card. Withdraw to hold it back again.', '由於你已在同意卡上按同意，電郵及日曆內容會傳送至以下 AI 服務。撤回後會再次暫停傳送。')}</p>
    {list}
  </div>;
}
