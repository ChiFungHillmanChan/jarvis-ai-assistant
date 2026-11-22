import { useState, type ReactNode } from 'react';
import { useUiLocale } from '../../i18n';
import { openUrl } from '../../lib/commands';
import Button from '../../ui/Button';
import type { ProviderGuide } from './providerGuides';

/** A provider's numbered key steps (providerGuides.ts). `last` is shown under the final step: the key field. */
export default function GuideSteps({ steps, last }: { steps: ProviderGuide['steps']; last?: ReactNode }) {
  const { text } = useUiLocale();
  const [failed, setFailed] = useState(false);
  return <>
    <ol className="settings-ai-steps">
      {steps.map((step, index) => (
        <li key={index}>
          <span className="settings-ai-step-number" aria-hidden="true">{index + 1}</span>
          <div className="settings-ai-step-body">
            <span>{text(step.en, step.zh)}</span>
            {step.link && <Button kind="quiet" onClick={() => openUrl(step.link!).then(() => setFailed(false))} onError={() => setFailed(true)}>
              {index === 0 ? text('Get a key', '取得密鑰') : text('Open the guide', '開啟說明')}
            </Button>}
            {index === steps.length - 1 && last}
          </div>
        </li>
      ))}
    </ol>
    {failed && <p className="ui-secret-error" role="alert">{text('Could not open the page.', '未能開啟頁面。')}</p>}
  </>;
}
