import { useState } from 'react';
import { useUiLocale } from '../../i18n';
import type { AiModelCatalog, CloudModel } from '../../lib/ai-models';
import type { CurrentAi } from '../../lib/ai-choice';
import { TestConsent } from '../../components/ChooseAi';
import Button from '../../ui/Button';
import SecretField from '../../ui/SecretField';
import Select from '../../ui/Select';
import { costRatio, geminiDefault, geminiModelNote, geminiModelToSave, sortCostPer1000 } from '../ai-choice';
import GuideSteps from './GuideSteps';
import { TEST_AND_SAVE, guideFor } from './providerGuides';
import { saveErrorLine, type TestAndUse } from './save';

interface GeminiCardProps {
  catalog: AiModelCatalog;
  /** The main-path Gemini models, cheapest first (mainPathModels). */
  models: CloudModel[];
  current: CurrentAi | null;
  busy: boolean;
  onTestAndUse: TestAndUse;
}

/** Google Gemini: the 4 key steps, the key field, and the model it uses. */
export default function GeminiCard({ catalog, models, current, busy, onTestAndUse }: GeminiCardProps) {
  const { text } = useUiLocale();
  const recommended = geminiDefault(catalog);
  const saved = current?.provider === 'gemini' ? current.model : null;
  // Existing users keep their saved model; everyone else starts on the mail sort default.
  const [model, setModel] = useState(saved ?? recommended?.id ?? models[0]?.id ?? '');
  const [error, setError] = useState('');
  // The test sends a message, so it waits for the tick (as ChooseAi does).
  const [consent, setConsent] = useState(false);
  const chosen = catalog.cloud.find(m => m.provider === 'gemini' && m.id === model) ?? null;
  const cost = sortCostPer1000(chosen, catalog);
  const hint = (m: CloudModel) => m.id === recommended?.id ? text('Recommended', '推薦')
    : recommended ? text(`${costRatio(m, recommended)}x cost`, `${costRatio(m, recommended)} 倍成本`) : '';
  const options = models.map(m => ({ value: m.id, label: [m.label, hint(m)].filter(Boolean).join(' · ') }));
  // A saved model outside the main path (picked in Advanced) stays listed.
  if (saved && !models.some(m => m.id === saved)) options.unshift({ value: saved, label: catalog.cloud.find(m => m.id === saved)?.label ?? saved });
  const toSave = geminiModelToSave(model, saved, recommended?.id);
  const choice = (apiKey?: string) => ({ provider: 'gemini' as const, apiKey, model: toSave.model });
  const saveLabel = busy ? text('Testing…', '測試中…') : text(TEST_AND_SAVE.en, TEST_AND_SAVE.zh);
  const test = async (apiKey?: string) => { await onTestAndUse(choice(apiKey)); setConsent(false); };

  return <div className="settings-ai-detail">
    <h3>{text('Connect Google Gemini in 4 steps', '以 4 個步驟連接 Google Gemini')}</h3>
    <p className="ui-hint">{text('Takes about 3 minutes. You need a Google account.', '約需 3 分鐘，需要 Google 帳戶。')}</p>
    <GuideSteps steps={guideFor('gemini', catalog.setup, catalog.compatible).steps} last={
      <div id="ai-test-use">
        <TestConsent id="ai-test-consent" option="gemini" name="Gemini" consent={consent} disabled={busy} onChange={setConsent} />
        <div id="ai-key">
          <SecretField saved={false} label={text('Gemini API key', 'Gemini API 密鑰')} saveLabel={saveLabel} saveDisabled={!consent}
            placeholder={current?.provider === 'gemini' ? text('Saved in Keychain. Paste a new key to replace it', '已儲存在鑰匙圈。貼上新密鑰即可更換') : text('Paste your Gemini API key', '貼上你的 Gemini API 密鑰')}
            errorFor={e => saveErrorLine(e, text)}
            onSave={key => { setError(''); return test(key); }} />
        </div>
      </div>
    } />
    <div id="ai-model" className="settings-ai-model">
      <Select label={text('Model', '模型')} showLabel value={model} options={options} disabled={busy} width={320} onChange={setModel} />
      <p id="ai-model-use" className="ui-hint">{geminiModelNote(catalog, toSave.model, text)}</p>
      <p className="ui-hint">{text('The recommended model handles mail well and costs little.', '推薦的模型能妥善整理郵件，費用低。')}</p>
      {cost && <p className="settings-ai-cost">{text(`About US$${cost} to sort 1,000 emails`, `整理 1,000 封郵件約 US$${cost}`)}</p>}
      {current?.provider === 'gemini' && toSave.changed && (
        <Button kind="secondary" busy={busy} disabled={!consent} onClick={() => { setError(''); return test(); }} onError={e => setError(saveErrorLine(e, text))}>
          {busy ? text('Testing…', '測試中…') : text('Use this model with the saved key', '以已儲存的密鑰使用此模型')}
        </Button>
      )}
      {error && <p className="ui-secret-error" role="alert">{error}</p>}
    </div>
  </div>;
}
