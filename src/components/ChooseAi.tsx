import { useEffect, useRef, useState } from 'react';
import { useUiLocale } from '../i18n';
import { chooseAi, currentAi, listAiModels, type AiProvider, type CurrentAi } from '../lib/ai-choice';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '../lib/commands';
import { errorLine, type ConsentRequest } from '../lib/ai-error';
import ConsentCard from './ConsentCard';
import { aiModels, useAiModels, type AiModelCatalog } from '../lib/ai-models';
import { localized, serviceFor, serviceLabel, setupOptions, type SetupOption } from '../lib/ai-catalog';
import LocalMailSetup from './LocalMailSetup';
import Select from '../ui/Select';
import '../styles/setup.css';

// The services, their hints, key pages and regions are the model
// registry's (ai_models); this file has the form for each.
type Option = SetupOption;
type Region = string;

export function optionFor(ai: CurrentAi | null, catalog: AiModelCatalog | null): Option | null {
  if (!ai) return null;
  if (ai.provider === 'compatible') {
    const service = ai.base_url ? serviceFor(catalog, ai.base_url)?.id : undefined;
    return service === 'kimi' || service === 'qwen' ? service : 'other';
  }
  return (['ollama', 'openai', 'gemini', 'claude'] as string[]).includes(ai.provider) ? ai.provider as Option : null;
}

type Text = (en: string, zh: string) => string;

/**
 * Whether Gmail and Calendar content may go to the AI in use: the consent
 * card it needs first (ai::consent::ai_consent_needed), or the plain line
 * when that content is never sent to it (ai::consent::ai_google_content_note).
 * Both are read again when the AI or a consent changes.
 */
export function useAiConsent(current: CurrentAi | null) {
  const [consentNeeded, setConsentNeeded] = useState<ConsentRequest | null>(null);
  const [googleNote, setGoogleNote] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const refresh = () => {
      void invoke<ConsentRequest | null>('ai_consent_needed').then(needed => { if (live) setConsentNeeded(needed); }).catch(() => {});
      void invoke<string | null>('ai_google_content_note').then(note => { if (live) setGoogleNote(note); }).catch(() => {});
    };
    refresh();
    const stop = listen('ai-consent-changed', refresh);
    return () => { live = false; void stop.then(unlisten => unlisten()).catch(() => {}); };
  }, [current]);
  return { consentNeeded, setConsentNeeded, googleNote };
}

/** What testing and then using `name` sends where, for the chosen option. */
export function testDisclosure(option: Option, name: string, text: Text): string {
  // Chinese text puts spaces around Latin names only ("向 Kimi 傳送", "向該服務傳送").
  const zhName = /^[\x00-\x7F]+$/.test(name) ? ` ${name} ` : name;
  return option === 'ollama'
    ? text('Testing sends a short message to Ollama on this Mac. Everything stays on this Mac.', '測試會向此 Mac 上的 Ollama 傳送一句簡短訊息，所有資料都留在此 Mac。')
    : option === 'kimi' || option === 'qwen' || option === 'other'
      ? text(`Testing sends one short message to ${name}. After that, your chats and voice requests are sent to ${name} with your key, and ${name}'s charges apply. JARVIS cannot confirm how ${name} uses data, so Gmail and Calendar content is not sent to it: mail features, reading your calendar and the daily briefing need another AI.`, `測試會向${zhName}傳送一句簡短訊息。之後你的對話及語音指令會以你的密鑰傳送至${zhName.trimEnd()}，並按${zhName}的收費計算。JARVIS 無法確認${zhName}如何使用資料，因此 Gmail 及日曆內容不會傳送至${zhName.trimEnd()}：郵件功能、讀取日曆及每日簡報須改用其他 AI。`)
      : option === 'gemini'
        ? text('Testing sends one short message to Gemini. After that, your chats and voice requests are sent to Gemini with your key, and Gemini\'s charges apply. Email and calendar content is sent only after you confirm on a card that your key has billing set up.', '測試會向 Gemini 傳送一句簡短訊息。之後你的對話及語音指令會以你的密鑰傳送至 Gemini，並按 Gemini 的收費計算。電郵及日曆內容只會在你按卡片確認密鑰已設定付費後才傳送。')
        : text(`Testing sends one short message to ${name}. After that, your chats, voice requests and the mail you ask JARVIS to analyse or sort are sent to ${name} with your key, and ${name}'s charges apply.`, `測試會向${zhName}傳送一句簡短訊息。之後你的對話、語音指令，以及你要求 JARVIS 分析或整理的郵件，會以你的密鑰傳送至${zhName.trimEnd()}，並按${zhName}的收費計算。`);
}

/** OpenAI does not list Hong Kong as a supported API region. */
export function openAiRegionNote(text: Text): string {
  return text('Hong Kong is not on the OpenAI API supported list. Use an eligible account, or choose another AI.', 'OpenAI API 支援地區清單未列出香港。請使用合資格帳戶，或選擇其他 AI。');
}

/** The line shown once a choice answered and was saved. */
export function chosenNotice(chosen: CurrentAi, text: Text): string {
  return text(`${chosen.label} answered. JARVIS now uses only ${chosen.label}.`, `${chosen.label} 已回覆。JARVIS 現在只會使用 ${chosen.label}。`)
    + (chosen.provider === 'gemini' ? '' : text(' Voice input uses a speech model on this Mac; download it in Settings → Speech.', '語音輸入會使用此 Mac 上的語音模型，可在「設定 → 語音」下載。'));
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
  const { consentNeeded, setConsentNeeded, googleNote } = useAiConsent(current);
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
    // A failed test comes as the error card's data (ai/failure.rs); this
    // panel has no card, so it shows the card's sentence.
    try { await action(); } catch (e) { if (mounted.current) setError(errorLine(e, text)); } finally { if (mounted.current) setBusy(''); }
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
      setNotice(chosenNotice(chosen, text));
      onChosen?.(chosen);
    });
  }

  const options: [Option, string, string][] = offered.map(choice => [choice.option, localized(choice.name, text), localized(choice.hint, text)]);
  const ready = !!provider && consent && (!needsModel || !!model) && (option !== 'other' || !!otherUrl.trim());

  function consentGiven() {
    setConsentNeeded(null);
    // Setup checks mail again once the user agrees.
    if (current) onChosen?.(current);
  }

  return <div className="setup-local choose-ai">
    <p className="setup-hint">{current
      ? text(`Now using: ${current.label}${current.model ? ` · ${current.model}` : ['openai', 'gemini', 'claude'].includes(current.provider) ? ' · latest model' : ''}. Choose again to switch; only one AI is used at a time.`, `目前使用：${current.label}${current.model ? `・${current.model}` : ['openai', 'gemini', 'claude'].includes(current.provider) ? '・最新模型' : ''}。重新選擇即可更換；每次只會使用一個 AI。`)
      : text('Choose the AI you already use. Only this AI is used for the assistant, voice replies and mail. You can change it later in Settings.', '請選擇你已在使用的 AI。AI 助理、語音回覆及電郵都只會使用這一個 AI，之後可在設定中更換。')}</p>
    {current && consentNeeded && <ConsentCard request={consentNeeded} onAgreed={consentGiven} />}
    {current && googleNote && <p className="setup-hint" role="note">{googleNote}</p>}
    <fieldset className="choose-ai-options" disabled={!!busy}>
      <legend>{text('Your AI', '你的 AI')}</legend>
      {options.map(([id, label, hint]) => <label key={id} className="setup-checkbox"><input type="radio" name="choose-ai" checked={option === id} onChange={() => pick(id)} /><span>{label}<small>{hint}</small></span></label>)}
    </fieldset>
    {option && <div className="choose-ai-details">
      {named && named.regions.length > 1 && <Select label={text('Account region', '帳戶地區')} showLabel disabled={!!busy} value={service?.id ?? ''} onChange={v => { setRegion(v); setModels(null); setModel(''); }}
        options={named.regions.map(r => ({ value: r.id, label: r.id === 'global' ? text('International', '國際') : r.id === 'china' ? text('Mainland China', '中國內地') : r.id }))} />}
      {option === 'other' && <label>{text('API base URL (ends in /v1 for most services)', 'API 基本網址（大多以 /v1 結尾）')}<input value={otherUrl} disabled={!!busy} spellCheck={false} placeholder="https://api.deepseek.com/v1" onChange={e => { setOtherUrl(e.target.value); setModels(null); setModel(''); }} /></label>}
      {option === 'openai' && <p className="setup-hint">{openAiRegionNote(text)}</p>}
      {option !== 'ollama' && <>
        <label>{text(`${name} API key`, `${name} API 密鑰`)}<input type="password" autoComplete="off" spellCheck={false} disabled={!!busy} value={apiKey} onChange={e => setApiKey(e.target.value)} placeholder={optionFor(current, catalog) === option ? text('Saved. Leave blank to keep it', '已儲存，留空即沿用') : text(`Paste your ${name} API key`, `貼上你的${zhName.trimEnd()} API 密鑰`)} /></label>
        {keyPage && <button type="button" disabled={!!busy} onClick={() => void openUrl(keyPage).catch(() => setError(text('Could not open the page.', '未能開啟頁面。')))}>{text('Get a key', '取得密鑰')}</button>}
      </>}
      {optionalModel && provider && <>
        <button type="button" disabled={!!busy} onClick={() => void loadModels(provider)}>{text('Choose a model', '選擇模型')}</button>
        {models && models.length > 0 && <Select label={text('Model', '模型')} showLabel disabled={!!busy} value={model} onChange={setModel}
          options={[{ value: '', label: text(`Latest (recommended)${latest ? `: ${latest}` : ''}, updates with JARVIS`, `最新模型（建議）${latest ? `：${latest}` : ''}，隨 JARVIS 更新`) }, ...models.map(m => ({ value: m, label: m }))]} />}
        {!models?.length && <p className="setup-hint">{text('Uses the latest recommended model unless you choose one.', '未選擇模型時，會使用最新的建議模型。')}</p>}
      </>}
      {needsModel && <>
        {option !== 'ollama' && <button type="button" disabled={!!busy || (option === 'other' && !otherUrl.trim())} onClick={() => void loadModels('compatible')}>{text('Load models', '載入模型')}</button>}
        {models && models.length > 0 && <Select label={text('Model', '模型')} showLabel disabled={!!busy} value={model} onChange={setModel}
          options={[{ value: '', label: text('Choose a model', '選擇模型') }, ...models.map(m => ({ value: m, label: m }))]} />}
        {option === 'ollama' && models && models.length === 0 && <>
          <p className="setup-hint">{text('Ollama is not running, or has no model yet. Prepare a model below, or open Ollama and load the models you already have.', 'Ollama 未開啟或尚未安裝模型。請在下方準備模型，或開啟 Ollama 後載入已安裝的模型。')}</p>
          <LocalMailSetup onReady={() => loadModels('ollama')} onBusyChange={b => setBusy(b ? text('Installing', '安裝中') : '')} />
          <button type="button" disabled={!!busy} onClick={() => void loadModels('ollama')}>{text('Load installed models', '載入已安裝的模型')}</button>
        </>}
      </>}
      <label className="setup-checkbox"><input type="checkbox" checked={consent} disabled={!!busy} onChange={e => setConsent(e.target.checked)} />{testDisclosure(option, name, text)}</label>
      <button type="button" className="setup-primary" disabled={!ready || !!busy} onClick={testAndUse}>{text('Test and use this AI', '測試並使用這個 AI')}</button>
    </div>}
    {busy && <p role="status">{busy}…</p>}
    {error && <p className="setup-error" role="alert">{error}</p>}
    {notice && <p className="setup-notice" role="status">{notice}</p>}
  </div>;
}
