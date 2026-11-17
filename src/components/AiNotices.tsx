import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useUiLocale } from '../i18n';
import ConsentCard from './ConsentCard';
import { addNotice, type ConsentRequest } from '../lib/ai-error';

interface NeedsModel { feature: string; message: string; model: string }
interface NeedsConsent { consent: ConsentRequest; feature: string }

const consentKey = (n: NeedsConsent) => `${n.consent.provider}|${n.consent.base_url}`;

/**
 * What a job in the background could not do (P06). Background jobs (mail
 * sorting, sync triage, the briefing) never prompt in the middle of their
 * work: they record what they need, and the backend raises each notice
 * once per launch. A model that cannot run a job shows why; held-back
 * email shows the consent card.
 */
export default function AiNotices() {
  const { text } = useUiLocale();
  const [models, setModels] = useState<NeedsModel[]>([]);
  const [consents, setConsents] = useState<NeedsConsent[]>([]);

  useEffect(() => {
    const stops = [
      listen<NeedsModel>('ai-needs-model', ({ payload }) => setModels(previous => addNotice(previous, payload, n => n.message))),
      listen<NeedsConsent>('ai-consent-needed', ({ payload }) => setConsents(previous => addNotice(previous, payload, consentKey))),
      // Agreed elsewhere (the chat card, Settings): the notice has done its job.
      listen('ai-consent-changed', () => setConsents([])),
    ];
    return () => stops.forEach(stop => void stop.then(unlisten => unlisten()).catch(() => {}));
  }, []);

  const model = models[0];
  const consent = consents[0];
  if (!model && !consent) return null;
  return <div className="update-toast ai-notice" role="status">
    <span className="update-toast-dot" />
    <div className="update-toast-body">
      {consent
        ? <><strong>{text('Held back until you agree', '待你同意後才處理')}</strong><ConsentCard request={consent.consent} onAgreed={() => setConsents(previous => previous.slice(1))} onClose={() => setConsents(previous => previous.slice(1))} /></>
        : <><strong>{text('A job could not run on your AI', '一項背景工作未能使用你的 AI 執行')}</strong><span className="tool-approval-detail">{model.message}</span>
          <div className="tool-approval-actions"><button type="button" className="tool-approval-decline" onClick={() => setModels(previous => previous.slice(1))}>{text('Close', '關閉')}</button></div></>}
    </div>
  </div>;
}
