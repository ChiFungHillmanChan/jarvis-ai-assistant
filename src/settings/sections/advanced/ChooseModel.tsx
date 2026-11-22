import { useRef, useState } from 'react';
import { useUiLocale } from '../../../i18n';
import type { AiModelCatalog } from '../../../lib/ai-models';
import { chooseAi, listAiModels, type AiChoice } from '../../../lib/ai-choice';
import { formatRate, serviceLabel } from '../../../lib/ai-catalog';
import { errorLine } from '../../../lib/ai-error';
import { confirmAction } from '../../../components/ConfirmDialog';
import { chosenNotice, testDisclosure } from '../../../components/ChooseAi';
import Button from '../../../ui/Button';
import Select from '../../../ui/Select';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import { needsChainConfirm } from '../../ai-choice';
import type { ChainEditor } from '../../hooks/useChainEditor';
import { useRow } from './useRow';

type Cloud = 'gemini' | 'claude' | 'openai';
const CLOUDS: { value: Cloud; label: string }[] = [{ value: 'gemini', label: 'Google Gemini' }, { value: 'claude', label: 'Anthropic Claude' }, { value: 'openai', label: 'OpenAI' }];

/**
 * Advanced > Models: any model the registry lists (the expensive ones the main
 * path leaves out included), and an OpenAI-format service at its own address.
 * Both save through choose_ai, as Your AI does: it tests the choice, then
 * replaces the provider order, so an order of more than one entry asks first.
 */
export default function ChooseModel({ editor, catalog, onChosen }: { editor: ChainEditor; catalog: AiModelCatalog; onChosen: () => void }) {
  const { text } = useUiLocale();
  const row = useRow();
  const saved = editor.current;
  const [provider, setProvider] = useState<Cloud>(CLOUDS.some(c => c.value === saved?.provider) ? saved!.provider as Cloud : 'gemini');
  const [model, setModel] = useState(CLOUDS.some(c => c.value === saved?.provider) ? saved?.model ?? '' : '');
  const [baseUrl, setBaseUrl] = useState(saved?.provider === 'compatible' ? saved.base_url ?? '' : '');
  const [apiKey, setApiKey] = useState('');
  const [listed, setListed] = useState<string[] | null>(null);
  const [listedModel, setListedModel] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ place: 'url' | 'model'; line: string } | null>(null);
  const [notice, setNotice] = useState('');
  const running = useRef(false);

  async function run(place: 'url' | 'model', choice: AiChoice) {
    if (running.current) return;
    running.current = true;
    setError(null); setNotice('');
    try {
      if (needsChainConfirm(editor.chain) && !(await confirmAction(text('This replaces your provider order in Advanced. Continue?', '這會取代你在進階設定中的供應商次序。要繼續嗎？'), text('Replace', '取代')))) return;
      setBusy(true);
      const chosen = await chooseAi(choice);
      setNotice(chosenNotice(chosen, text));
      if (place === 'url') setApiKey('');
      void editor.reload();
      onChosen();
    } catch (e) {
      setError({ place, line: errorLine(e, text) });
    } finally {
      running.current = false;
      setBusy(false);
    }
  }

  async function loadModels() {
    setError(null);
    try {
      const list = await listAiModels('compatible', baseUrl.trim(), apiKey.trim() || undefined);
      setListed(list);
      setListedModel(list.length === 1 ? list[0] : '');
    } catch (e) {
      setError({ place: 'url', line: errorLine(e, text) });
    }
  }

  const url = baseUrl.trim();
  const serviceName = /^https?:\/\/[^/\s]+/.test(url) ? serviceLabel(catalog, url) : text('this service', '該服務');
  const name = CLOUDS.find(c => c.value === provider)!.label;
  const latest = provider === 'openai' ? catalog.defaults.openai : provider === 'claude' ? catalog.defaults.claude : catalog.defaults.gemini_flash;
  const latestLabel = catalog.cloud.find(m => m.id === latest)?.label ?? latest;
  const models = catalog.cloud.filter(m => m.provider === provider && m.status === 'active' && !m.family);
  const modelOptions = [
    { value: '', label: text(`Latest (recommended)${latestLabel ? `: ${latestLabel}` : ''}`, `最新（建議）${latestLabel ? `：${latestLabel}` : ''}`) },
    ...models.map(m => ({ value: m.id, label: m.label, hint: text(`US$${formatRate(m.price.input_per_million)} / $${formatRate(m.price.output_per_million)} per 1M tokens`, `每百萬 token US$${formatRate(m.price.input_per_million)} / $${formatRate(m.price.output_per_million)}`) })),
    // A saved model the registry no longer offers stays the current choice.
    ...(model && !models.some(m => m.id === model) ? [{ value: model, label: model }] : []),
  ];
  const failure = (place: 'url' | 'model') => error?.place === place && error.line && <p className="ui-secret-error" role="alert">{error.line}</p>;

  return (
    <SettingGroup title={text('Choose the AI', '選擇 AI')}>
      <SettingRow {...row('ai-base-url')} control={null}>
        <div className="settings-advanced-form">
          <input className="ui-input" aria-label={row('ai-base-url').title} value={baseUrl} disabled={busy} spellCheck={false} placeholder="https://api.example.com/v1"
            onChange={e => { setBaseUrl(e.target.value); setListed(null); setListedModel(''); }} />
          <input className="ui-input" type="password" autoComplete="off" spellCheck={false} aria-label={text(`${serviceName} API key`, `${serviceName} API 密鑰`)} value={apiKey} disabled={busy}
            placeholder={saved?.provider === 'compatible' && saved.base_url === url ? text('Saved. Leave blank to keep it', '已儲存，留空即沿用') : text('API key', 'API 密鑰')}
            onChange={e => setApiKey(e.target.value)} />
          <span className="settings-row-actions settings-row-actions-start">
            <Button kind="secondary" disabled={busy || !url} onClick={loadModels}>{text('Load models', '載入模型')}</Button>
            {listed && listed.length > 0 && <>
              <Select label={text('Model', '模型')} value={listedModel} disabled={busy} width={260} onChange={setListedModel}
                options={[{ value: '', label: text('Choose a model', '選擇模型') }, ...listed.map(m => ({ value: m, label: m }))]} />
              <Button kind="secondary" busy={busy} disabled={!listedModel} onClick={() => run('url', { provider: 'compatible', apiKey: apiKey.trim() || undefined, baseUrl: url, model: listedModel })}>
                {text('Test and save', '測試並儲存')}
              </Button>
            </>}
          </span>
          {listed && listed.length === 0 && <p className="ui-hint" role="status">{text('The service listed no models.', '該服務沒有列出任何模型。')}</p>}
          <p className="ui-hint">{testDisclosure('other', serviceName, text)}</p>
        </div>
        {failure('url')}
      </SettingRow>

      <SettingRow {...row('ai-model-choose')} control={null}>
        <div className="settings-advanced-form">
          <span className="settings-row-actions settings-row-actions-start">
            <Select label={text('Service', '服務')} value={provider} options={CLOUDS} disabled={busy} width={200} onChange={v => { setProvider(v); setModel(''); }} />
            <Select label={text('Model', '模型')} value={model} options={modelOptions} disabled={busy} width={340} onChange={setModel} />
            <Button kind="secondary" busy={busy} onClick={() => run('model', { provider, model: model || undefined })}>{text('Test and use', '測試並使用')}</Button>
          </span>
          <p className="ui-hint">{text(`Uses the ${name} key saved in Cloud providers. Every listed model is offered here, including the expensive ones Your AI leaves out.`, `使用在「雲端服務」中儲存的 ${name} 密鑰。這裡列出所有模型，包括「你的 AI」未列出的高價模型。`)}</p>
          <p className="ui-hint">{testDisclosure(provider, name, text)}</p>
        </div>
        {failure('model')}
      </SettingRow>
      {notice && <p className="settings-ai-notice" role="status">{notice}</p>}
    </SettingGroup>
  );
}
