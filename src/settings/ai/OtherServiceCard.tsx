import { useRef, useState } from 'react';
import { useUiLocale } from '../../i18n';
import type { AiModelCatalog } from '../../lib/ai-models';
import { listAiModels, type CurrentAi } from '../../lib/ai-choice';
import { localized, setupOptions } from '../../lib/ai-catalog';
import { optionFor, testDisclosure } from '../../components/ChooseAi';
import Button from '../../ui/Button';
import SecretField from '../../ui/SecretField';
import Select from '../../ui/Select';
import { cheapestFor } from '../ai-choice';
import GuideSteps from './GuideSteps';
import { PROVIDER_GUIDES, TEST_AND_SAVE, guideFor, type ProviderGuide } from './providerGuides';
import { saveErrorLine, type TestAndUse } from './save';

type Service = Exclude<ProviderGuide['id'], 'gemini'>;

/**
 * OpenAI, Claude, Kimi or Qwen: that service's key steps and its key field.
 * OpenAI and Claude use the service's lowest-priced model. Kimi and Qwen
 * list no priced model and choose_ai needs one, so their key first loads the
 * models the service lists, as setup does.
 */
export default function OtherServiceCard({ catalog, current, busy, onTestAndUse }: { catalog: AiModelCatalog; current: CurrentAi | null; busy: boolean; onTestAndUse: TestAndUse }) {
  const { text } = useUiLocale();
  const offered = setupOptions(catalog);
  const services = PROVIDER_GUIDES.flatMap(g => {
    const choice = g.id !== 'gemini' ? offered.find(o => o.option === g.id) : undefined;
    return choice ? [{ value: g.id as Service, label: localized(choice.name, text) }] : [];
  });
  const saved = optionFor(current, catalog);
  const [service, setService] = useState<Service>(services.find(s => s.value === saved)?.value ?? services[0]?.value ?? 'openai');
  const [region, setRegion] = useState('global');
  const [models, setModels] = useState<string[] | null>(null);
  const [model, setModel] = useState('');
  const [error, setError] = useState('');
  // A Kimi or Qwen key, held only until it is tested, as setup holds it.
  const pending = useRef<string | undefined>(undefined);

  const named = service === 'kimi' || service === 'qwen' ? catalog.compatible.find(s => s.id === service) ?? null : null;
  const place = named?.regions.find(r => r.id === region) ?? named?.regions[0] ?? null;
  const guide = guideFor(service, catalog.setup, catalog.compatible);
  const steps = place ? guide.steps.map((step, i) => (i === 0 ? { ...step, link: place.key_page } : step)) : guide.steps;
  const cheapest = named ? null : cheapestFor(catalog.cloud, service);
  const name = named?.name ?? services.find(s => s.value === service)?.label ?? service;
  const isSaved = saved === service;
  const saveLabel = busy ? text('Testing…', '測試中…') : text(TEST_AND_SAVE.en, TEST_AND_SAVE.zh);
  const fail = (e: unknown) => setError(saveErrorLine(e, text));

  function pick(next: Service) { setService(next); setRegion('global'); setModels(null); setModel(''); setError(''); pending.current = undefined; }
  async function loadModels(apiKey?: string) {
    setError('');
    const list = await listAiModels('compatible', place?.base_url, apiKey);
    pending.current = apiKey;
    setModels(list); setModel(list.length === 1 ? list[0] : '');
  }
  const save = (apiKey?: string) => onTestAndUse(named
    ? { provider: 'compatible', apiKey, baseUrl: place?.base_url, model }
    : { provider: service as 'openai' | 'claude', apiKey, model: cheapest?.id });

  return <div className="settings-ai-detail">
    <h3>{text('Use another AI service', '使用其他 AI 服務')}</h3>
    <div className="settings-ai-model">
      <Select label={text('Service', '服務')} showLabel value={service} options={services} disabled={busy} width={260} onChange={pick} />
      {named && named.regions.length > 1 && <Select label={text('Account region', '帳戶地區')} showLabel value={place?.id ?? ''} disabled={busy} width={260}
        onChange={v => { setRegion(v); setModels(null); setModel(''); pending.current = undefined; }}
        options={named.regions.map(r => ({ value: r.id, label: r.id === 'global' ? text('International', '國際') : r.id === 'china' ? text('Mainland China', '中國內地') : r.id }))} />}
    </div>
    <GuideSteps steps={steps} last={
      <SecretField key={`${service}-${place?.id ?? ''}`} saved={false} label={text(`${name} API key`, `${name} API 密鑰`)}
        saveLabel={named ? text('Load models', '載入模型') : saveLabel}
        placeholder={isSaved ? text('Saved in Keychain. Paste a new key to replace it', '已儲存在鑰匙圈。貼上新密鑰即可更換') : text(`Paste your ${name} API key`, `貼上你的 ${name} API 密鑰`)}
        errorFor={e => saveErrorLine(e, text)}
        onSave={key => { setError(''); return named ? loadModels(key) : save(key); }} />
    } />
    {cheapest && <p className="ui-hint">{text(`JARVIS uses ${cheapest.label}, the low-cost model of ${name}.`, `JARVIS 會使用 ${name} 的低成本模型 ${cheapest.label}。`)}</p>}
    {named && <div className="settings-ai-model">
      {isSaved && !models && <Button kind="quiet" onClick={() => loadModels()} onError={fail}>{text('Load models with the saved key', '以已儲存的密鑰載入模型')}</Button>}
      {models && <>
        <Select label={text('Model', '模型')} showLabel value={model} disabled={busy} width={320} onChange={setModel}
          options={[{ value: '', label: text('Choose a model', '選擇模型') }, ...models.map(m => ({ value: m, label: m }))]} />
        <Button kind="primary" busy={busy} disabled={!model} onClick={() => save(pending.current)} onError={fail}>{saveLabel}</Button>
      </>}
    </div>}
    <p className="ui-hint">{testDisclosure(service, name, text)}</p>
    {error && <p className="ui-secret-error" role="alert">{error}</p>}
  </div>;
}
