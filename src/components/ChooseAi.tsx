import { useEffect, useRef, useState } from 'react';
import { useUiLocale } from '../i18n';
import { chooseAi, currentAi, listAiModels, type AiProvider, type CurrentAi } from '../lib/ai-choice';
import { getOptionalPreference, openUrl, setPreference } from '../lib/commands';
import { aiModels, useAiModels, type AiModelCatalog } from '../lib/ai-models';
import { localized, serviceFor, serviceLabel, setupOptions, type SetupOption } from '../lib/ai-catalog';
import LocalMailSetup from './LocalMailSetup';
import '../styles/setup.css';

// The services, their hints, key pages and regions are the model
// registry's (ai_models); this file has the form for each.
type Option = SetupOption;
type Region = string;

/** Same key as ai::mail_policy in Rust. */
const GEMINI_BILLING_CONFIRMED = 'gemini_billing_confirmed';

function optionFor(ai: CurrentAi | null, catalog: AiModelCatalog | null): Option | null {
  if (!ai) return null;
  if (ai.provider === 'compatible') {
    const service = ai.base_url ? serviceFor(catalog, ai.base_url)?.id : undefined;
    return service === 'kimi' || service === 'qwen' ? service : 'other';
  }
  return (['ollama', 'openai', 'gemini', 'claude'] as string[]).includes(ai.provider) ? ai.provider as Option : null;
}

/**
 * Setup step 1 and Settings: the user picks the one AI JARVIS uses. The
 * choice is tested with a short request before it is saved; nothing else is
 * used behind the user's back.
 */
export default function ChooseAi({ onChosen, onBusyChange }: { onChosen?: (ai: CurrentAi) => void; onBusyChange?: (busy: boolean) => void }) {
  const { text } = useUiLocale();
  const [current, setCurrent] = useState<CurrentAi | null>(null);
  const [option, setOption] = useState<Option | null>(null);
  const [region, setRegion] = useState<Region>('global');
  const [apiKey, setApiKey] = useState('');
  const [otherUrl, setOtherUrl] = useState('');
  const [models, setModels] = useState<string[] | null>(null);
  const [model, setModel] = useState('');
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [geminiBilled, setGeminiBilled] = useState(false);
  const mounted = useRef(true);
  const catalog = useAiModels();

  useEffect(() => {
    mounted.current = true;
    void Promise.all([currentAi(), aiModels().catch(() => null)]).then(([ai, models]) => {
      if (!mounted.current) return;
      setCurrent(ai); setOption(optionFor(ai, models));
      if (ai?.model) { setModel(ai.model); setModels([ai.model]); }
      // Show the saved service, not an empty form.
      const saved = ai?.base_url;
      if (saved) {
        const known = (models?.compatible ?? []).flatMap(service => service.regions).find(r => r.base_url === saved);
        if (known) setRegion(known.id); else setOtherUrl(saved);
      }
    }).catch(() => {});
    void getOptionalPreference(GEMINI_BILLING_CONFIRMED).then(value => { if (mounted.current) setGeminiBilled(value === 'true'); }).catch(() => {});
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => { onBusyChange?.(!!busy); }, [busy]);

  const offered = setupOptions(catalog);
  const chosen = offered.find(choice => choice.option === option);
  const named = option === 'kimi' || option === 'qwen' ? catalog?.compatible.find(s => s.id === option) ?? null : null;
  const service = named?.regions.find(r => r.id === region) ?? named?.regions[0] ?? null;
  const baseUrl = service ? service.base_url : option === 'other' ? otherUrl.trim() : '';
  const needsModel = chosen?.model === 'required';
  // OpenAI, Gemini and Claude may pin a model; left blank, JARVIS follows the
  // latest model it ships with.
  const optionalModel = chosen?.model === 'optional';
  const latest = !catalog ? '' : option === 'openai' ? catalog.defaults.openai : option === 'claude' ? catalog.defaults.claude : option === 'gemini' ? catalog.defaults.gemini_flash : '';
  const name = option === 'ollama' ? 'Ollama' : option === 'openai' ? 'OpenAI' : option === 'gemini' ? 'Gemini' : option === 'claude' ? 'Claude'
    : named ? named.name : /^https?:\/\/[^/\s]+/.test(otherUrl.trim()) ? serviceLabel(catalog, otherUrl.trim()) : text('this service', '該服務');
  // Chinese text puts spaces around Latin names only ("向 Kimi 傳送", "向該服務傳送").
  const zhName = /^[\x00-\x7F]+$/.test(name) ? ` ${name} ` : name;
  const keyPage = service?.key_page ?? chosen?.key_page ?? undefined;
  const provider: AiProvider | null = !option ? null : option === 'kimi' || option === 'qwen' || option === 'other' ? 'compatible' : option;

  function pick(next: Option) {
    setOption(next); setModels(null); setModel(''); setApiKey(''); setConsent(false); setError(''); setNotice('');
    if (next === 'ollama') void loadModels('ollama');
  }
  async function run(label: string, action: () => Promise<void>) {
    if (busy) return;
    setBusy(label); setError(''); setNotice('');
    try { await action(); } catch (e) { if (mounted.current) setError(String(e)); } finally { if (mounted.current) setBusy(''); }
  }
  function loadModels(kind: AiProvider) {
    return run(text('Loading models', '載入模型'), async () => {
      const list = await listAiModels(kind, kind === 'compatible' ? baseUrl : undefined, apiKey.trim() || undefined).catch(error => {
        // Ollama not running or not installed: show the installer below.
        if (kind === 'ollama' && mounted.current) setModels([]);
        throw error;
      });
      if (!mounted.current) return;
      setModels(list);
      setModel(previous => list.includes(previous) ? previous : !optionalModel && list.length === 1 ? list[0] : '');
    });
  }
  function testAndUse() {
    if (!provider) return;
    void run(text('Testing', '測試中'), async () => {
      const chosen = await chooseAi({ provider, apiKey: apiKey.trim() || undefined, baseUrl: provider === 'compatible' ? baseUrl : undefined, model: needsModel || optionalModel ? model || undefined : undefined });
      if (!mounted.current) return;
      setCurrent(chosen); setApiKey(''); setConsent(false);
      setNotice(text(`${chosen.label} answered. JARVIS now uses only ${chosen.label}.`, `${chosen.label} 已回覆。JARVIS 現在只會使用 ${chosen.label}。`)
        + (chosen.provider === 'gemini' ? '' : text(' Voice input uses a speech model on this Mac; download it in Settings → Speech.', '語音輸入會使用此 Mac 上的語音模型，可在「設定 → 語音」下載。')));
      onChosen?.(chosen);
    });
  }

  const options: [Option, string, string][] = offered.map(choice => [choice.option, localized(choice.name, text), localized(choice.hint, text)]);
  const ready = !!provider && consent && (!needsModel || !!model) && (option !== 'other' || !!otherUrl.trim());

  async function confirmGeminiBilling(checked: boolean) {
    try {
      await setPreference(GEMINI_BILLING_CONFIRMED, checked ? 'true' : 'false');
      if (!mounted.current) return;
      setGeminiBilled(checked);
      // Setup re-checks mail with Gemini once billing is confirmed.
      if (checked && current) onChosen?.(current);
    } catch { if (mounted.current) setError(text('Could not save this setting. Try again.', '未能儲存此設定，請重試。')); }
  }

  return <div className="setup-local choose-ai">
    <p className="setup-hint">{current
      ? text(`Now using: ${current.label}${current.model ? ` · ${current.model}` : ['openai', 'gemini', 'claude'].includes(current.provider) ? ' · latest model' : ''}. Choose again to switch; only one AI is used at a time.`, `目前使用：${current.label}${current.model ? `・${current.model}` : ['openai', 'gemini', 'claude'].includes(current.provider) ? '・最新模型' : ''}。重新選擇即可更換；每次只會使用一個 AI。`)
      : text('Choose the AI you already use. Only this AI is used for the assistant, voice replies and mail. You can change it later in Settings.', '請選擇你已在使用的 AI。AI 助理、語音回覆及電郵都只會使用這一個 AI，之後可在設定中更換。')}</p>
    {current?.provider === 'gemini' && <label className="setup-checkbox"><input type="checkbox" checked={geminiBilled} disabled={!!busy} onChange={e => void confirmGeminiBilling(e.target.checked)} />{text('My Gemini API key has billing enabled. Google does not use paid-tier content to train its models, so JARVIS sends email to Gemini only after you confirm this.', '我的 Gemini API 金鑰已啟用付費。Google 不會以付費方案的內容訓練模型，因此 JARVIS 只會在你確認後才把電郵傳送給 Gemini。')}</label>}
    {current?.provider === 'compatible' && <p className="setup-hint">{text(`Email is not sent to ${current.label}. Google's rules for Gmail data allow only AI services that do not train on it. Chat and voice use ${current.label}; for email analysis and sorting, choose OpenAI, Claude, a local Ollama model, or Gemini with billing enabled.`, `電郵內容不會傳送至 ${current.label}。Google 對 Gmail 資料的規定只容許不會以此訓練模型的 AI 處理電郵。對話及語音會使用 ${current.label}；如需電郵分析及自動整理，請改選 OpenAI、Claude、本機 Ollama 模型，或已啟用付費的 Gemini。`)}</p>}
    <fieldset className="choose-ai-options" disabled={!!busy}>
      <legend>{text('Your AI', '你的 AI')}</legend>
      {options.map(([id, label, hint]) => <label key={id} className="setup-checkbox"><input type="radio" name="choose-ai" checked={option === id} onChange={() => pick(id)} /><span>{label}<small>{hint}</small></span></label>)}
    </fieldset>
    {option && <div className="choose-ai-details">
      {named && named.regions.length > 1 && <label>{text('Account region', '帳戶地區')}<select disabled={!!busy} value={service?.id ?? ''} onChange={e => { setRegion(e.target.value); setModels(null); setModel(''); }}>
        {named.regions.map(r => <option key={r.id} value={r.id}>{r.id === 'global' ? text('International', '國際') : r.id === 'china' ? text('Mainland China', '中國內地') : r.id}</option>)}
      </select></label>}
      {option === 'other' && <label>{text('API base URL (ends in /v1 for most services)', 'API 基本網址（大多以 /v1 結尾）')}<input value={otherUrl} disabled={!!busy} spellCheck={false} placeholder="https://api.deepseek.com/v1" onChange={e => { setOtherUrl(e.target.value); setModels(null); setModel(''); }} /></label>}
      {option === 'openai' && <p className="setup-hint">{text('Hong Kong is not on the OpenAI API supported list. Use an eligible account, or choose another AI.', 'OpenAI API 支援地區清單未列出香港。請使用合資格帳戶，或選擇其他 AI。')}</p>}
      {option !== 'ollama' && <>
        <label>{text(`${name} API key`, `${name} API 密鑰`)}<input type="password" autoComplete="off" spellCheck={false} disabled={!!busy} value={apiKey} onChange={e => setApiKey(e.target.value)} placeholder={optionFor(current, catalog) === option ? text('Saved. Leave blank to keep it', '已儲存，留空即沿用') : text(`Paste your ${name} API key`, `貼上你的${zhName.trimEnd()} API 密鑰`)} /></label>
        {keyPage && <button type="button" disabled={!!busy} onClick={() => void openUrl(keyPage).catch(() => setError(text('Could not open the page.', '未能開啟頁面。')))}>{text('Get a key', '取得密鑰')}</button>}
      </>}
      {optionalModel && provider && <>
        <button type="button" disabled={!!busy} onClick={() => void loadModels(provider)}>{text('Choose a model', '選擇模型')}</button>
        {models && models.length > 0 && <label>{text('Model', '模型')}<select disabled={!!busy} value={model} onChange={e => setModel(e.target.value)}>
          <option value="">{text(`Latest (recommended)${latest ? `: ${latest}` : ''}, updates with JARVIS`, `最新模型（建議）${latest ? `：${latest}` : ''}，隨 JARVIS 更新`)}</option>{models.map(m => <option key={m} value={m}>{m}</option>)}
        </select></label>}
        {!models?.length && <p className="setup-hint">{text('Uses the latest recommended model unless you choose one.', '未選擇模型時，會使用最新的建議模型。')}</p>}
      </>}
      {needsModel && <>
        {option !== 'ollama' && <button type="button" disabled={!!busy || (option === 'other' && !otherUrl.trim())} onClick={() => void loadModels('compatible')}>{text('Load models', '載入模型')}</button>}
        {models && models.length > 0 && <label>{text('Model', '模型')}<select disabled={!!busy} value={model} onChange={e => setModel(e.target.value)}>
          <option value="">{text('Choose a model', '選擇模型')}</option>{models.map(m => <option key={m} value={m}>{m}</option>)}
        </select></label>}
        {option === 'ollama' && models && models.length === 0 && <>
          <p className="setup-hint">{text('Ollama is not running, or has no model yet. Prepare a model below, or open Ollama and load the models you already have.', 'Ollama 未開啟或尚未安裝模型。請在下方準備模型，或開啟 Ollama 後載入已安裝的模型。')}</p>
          <LocalMailSetup onReady={() => loadModels('ollama')} onBusyChange={b => setBusy(b ? text('Installing', '安裝中') : '')} />
          <button type="button" disabled={!!busy} onClick={() => void loadModels('ollama')}>{text('Load installed models', '載入已安裝的模型')}</button>
        </>}
      </>}
      <label className="setup-checkbox"><input type="checkbox" checked={consent} disabled={!!busy} onChange={e => setConsent(e.target.checked)} />{option === 'ollama'
        ? text('Testing sends a short message to Ollama on this Mac. Everything stays on this Mac.', '測試會向此 Mac 上的 Ollama 傳送一句簡短訊息，所有資料都留在此 Mac。')
        : option === 'kimi' || option === 'qwen' || option === 'other'
          ? text(`Testing sends one short message to ${name}. After that, your chats and voice requests are sent to ${name} with your key, and ${name}'s charges apply. Email is not sent to ${name}.`, `測試會向${zhName}傳送一句簡短訊息。之後你的對話及語音指令會以你的密鑰傳送至${zhName.trimEnd()}，並按${zhName}的收費計算。電郵內容不會傳送至${zhName.trimEnd()}。`)
          : option === 'gemini'
            ? text('Testing sends one short message to Gemini. After that, your chats and voice requests are sent to Gemini with your key, and Gemini\'s charges apply. The mail you ask JARVIS to analyse or sort is sent only after you confirm your key has billing enabled.', '測試會向 Gemini 傳送一句簡短訊息。之後你的對話及語音指令會以你的密鑰傳送至 Gemini，並按 Gemini 的收費計算。你要求 JARVIS 分析或整理的郵件，只會在你確認金鑰已啟用付費後才傳送。')
            : text(`Testing sends one short message to ${name}. After that, your chats, voice requests and the mail you ask JARVIS to analyse or sort are sent to ${name} with your key, and ${name}'s charges apply.`, `測試會向${zhName}傳送一句簡短訊息。之後你的對話、語音指令，以及你要求 JARVIS 分析或整理的郵件，會以你的密鑰傳送至${zhName.trimEnd()}，並按${zhName}的收費計算。`)}</label>
      <button type="button" className="setup-primary" disabled={!ready || !!busy} onClick={testAndUse}>{text('Test and use this AI', '測試並使用這個 AI')}</button>
    </div>}
    {busy && <p role="status">{busy}…</p>}
    {error && <p className="setup-error" role="alert">{error}</p>}
    {notice && <p className="setup-notice" role="status">{notice}</p>}
  </div>;
}
