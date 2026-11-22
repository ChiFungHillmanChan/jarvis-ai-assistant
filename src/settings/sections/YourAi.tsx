import { useRef, useState, type KeyboardEvent } from 'react';
import { useUiLocale } from '../../i18n';
import { useAiModels } from '../../lib/ai-models';
import { chooseAi, type CurrentAi } from '../../lib/ai-choice';
import { localized } from '../../lib/ai-catalog';
import { confirmAction } from '../../components/ConfirmDialog';
import ConsentCard from '../../components/ConsentCard';
import { chosenNotice, useAiConsent } from '../../components/ChooseAi';
import { useLocalSetup } from '../../components/LocalMailSetup';
import Button from '../../ui/Button';
import { nextIndex } from '../../ui/keys';
import { geminiEffectiveModel, jarvisAiAvailable, localUnavailable, mainPathModels, needsChainConfirm, sortCostPer1000 } from '../ai-choice';
import { useProviderChain } from '../hooks/useProviderChain';
import { useSettingsDialog } from '../SettingsContext';
import GeminiCard from '../ai/GeminiCard';
import LocalCard from '../ai/LocalCard';
import OtherServiceCard from '../ai/OtherServiceCard';
import { CANCELLED, type TestAndUse } from '../ai/save';
import { useRow } from './advanced/useRow';

type Card = 'jarvis' | 'gemini' | 'local' | 'other';
interface Face { id: Card; name: string; blurb: string; cost: string; disabled?: boolean }

/** The card that shows the AI in use. */
function cardFor(current: CurrentAi | null): Card | null {
  switch (current?.provider) {
    case 'jarvis_ai': return 'jarvis';
    case 'gemini': return 'gemini';
    case 'ollama': return 'local';
    case 'openai': case 'claude': case 'compatible': return 'other';
    default: return null;
  }
}

/**
 * Settings > Your AI: pick the one AI JARVIS uses. Every card saves through
 * `testAndUse`, which asks before replacing a provider order of more than one
 * entry, then lets choose_ai test the choice and save it. A save keeps
 * running if the dialog closes.
 */
export default function YourAi() {
  const { text } = useUiLocale();
  const { openSettings } = useSettingsDialog();
  const row = useRow();
  const catalog = useAiModels();
  const { chain, current, ready, error, reload } = useProviderChain();
  const { consentNeeded, setConsentNeeded, googleNote } = useAiConsent(current);
  // Downloading does not change mail; Test and save applies the choice, as the other cards do.
  const local = useLocalSetup({ applyToMail: false, onReady: () => local.refresh() });
  const [picked, setPicked] = useState<Card | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState('');
  const running = useRef(false);
  const cards = useRef<(HTMLButtonElement | null)[]>([]);

  const testAndUse: TestAndUse = async choice => {
    if (running.current) throw CANCELLED;
    running.current = true;
    setNotice('');
    try {
      if (needsChainConfirm(chain) && !(await confirmAction(text('This replaces your provider order in Advanced. Continue?', '這會取代你在進階設定中的供應商次序。要繼續嗎？'), text('Replace', '取代')))) throw CANCELLED;
      setBusy(true);
      const chosen = await chooseAi(choice);
      setNotice(chosenNotice(chosen, text));
      void reload();
    } finally {
      running.current = false;
      setBusy(false);
    }
  };

  if (error) {
    return <p className="ui-secret-error" role="alert">
      {text('Could not load your AI settings.', '無法載入 AI 設定。')} <Button kind="quiet" onClick={reload}>{text('Retry', '重試')}</Button>
    </p>;
  }
  if (!ready || !catalog) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  const jarvis = jarvisAiAvailable(catalog);
  const jarvisChoice = catalog.setup.find(choice => choice.id === 'jarvis_ai');
  const unavailable = local.probe ? localUnavailable(local.probe) : null;
  const sortCost = sortCostPer1000(geminiEffectiveModel(catalog, current?.provider === 'gemini' ? current.model : null), catalog);
  const faces: Face[] = [
    ...(jarvis && jarvisChoice ? [{ id: 'jarvis' as const, name: localized(jarvisChoice.name, text), blurb: localized(jarvisChoice.hint, text), cost: '' }] : []),
    { id: 'gemini', name: 'Google Gemini', blurb: text('Best for most people. Fast, and handles English and Chinese mail well.', '適合大多數人。速度快，能妥善處理中英文郵件。'),
      cost: sortCost ? text(`About US$${sortCost} to sort 1,000 emails`, `整理 1,000 封郵件約 US$${sortCost}`) : '' },
    { id: 'local', name: text('On this Mac', '在此 Mac 上'), blurb: text('Free and private. Nothing leaves your Mac, but replies are slower.', '免費且私密。資料不會離開你的 Mac，但回覆較慢。'),
      cost: unavailable ? text('Needs a newer Mac', '需要較新的 Mac') : text('Free', '免費'), disabled: !!unavailable },
    { id: 'other', name: text('Other services', '其他服務'), blurb: text('OpenAI, Claude, Kimi or Qwen, if you already pay for one.', '如你已付費使用 OpenAI、Claude、Kimi 或 Qwen，可選用其中一個。'), cost: text('Varies by service', '視乎服務而定') },
  ];
  const recommended: Card = jarvis ? 'jarvis' : 'gemini';
  const saved = cardFor(current);
  const card = picked ?? (saved && faces.some(f => f.id === saved) ? saved : recommended);

  function onKeyDown(e: KeyboardEvent, index: number) {
    const next = nextIndex(index, e.key, faces.length);
    if (next === null) return;
    e.preventDefault();
    setPicked(faces[next].id);
    cards.current[next]?.focus();
  }

  return <>
    <p className="settings-ai-intro">{text('JARVIS needs an AI to read your mail and answer you. Pick one. You can change it any time.', 'JARVIS 需要一個 AI 來閱讀你的郵件並回覆你。請選擇一個，之後可隨時更換。')}</p>
    {current && <p id="ai-current" className="ui-hint" role="status">{text(`Now using ${current.label}${current.model ? ` · ${current.model}` : ''}. Only one AI is used at a time.`, `目前使用 ${current.label}${current.model ? `・${current.model}` : ''}。每次只會使用一個 AI。`)}</p>}
    {current && consentNeeded && <div id="ai-consent"><ConsentCard request={consentNeeded} onAgreed={() => setConsentNeeded(null)} /></div>}
    {current && googleNote && <p id="ai-google-note" className="ui-hint" role="note">{googleNote}</p>}

    <div id="ai-choice" className="settings-ai-cards" role="radiogroup" aria-label={text('Your AI', '你的 AI')}>
      {faces.map((face, i) => (
        <button key={face.id} ref={el => { cards.current[i] = el; }} type="button" role="radio" className="settings-ai-card"
          aria-checked={card === face.id} tabIndex={card === face.id ? 0 : -1} disabled={face.disabled || busy}
          onClick={() => setPicked(face.id)} onKeyDown={e => onKeyDown(e, i)}>
          <span className="settings-ai-card-name">{face.name}{recommended === face.id && <span className="ui-setting-badge">{text('Recommended', '推薦')}</span>}</span>
          <span className="settings-ai-card-blurb">{face.blurb}</span>
          {face.cost && <span className="settings-ai-card-cost">{face.cost}</span>}
        </button>
      ))}
    </div>

    {card === 'jarvis' && jarvisChoice && <div className="settings-ai-detail"><p className="ui-hint">{localized(jarvisChoice.hint, text)}</p></div>}
    {card === 'gemini' && <GeminiCard catalog={catalog} models={mainPathModels(catalog.cloud, 'gemini')} current={current} busy={busy} onTestAndUse={testAndUse} />}
    {card === 'local' && <LocalCard setup={local} unavailable={unavailable} current={current} busy={busy} onTestAndUse={testAndUse} />}
    {card === 'other' && <OtherServiceCard catalog={catalog} current={current} busy={busy} onTestAndUse={testAndUse} />}

    {notice && <p className="settings-ai-notice" role="status">{notice}</p>}
    <p id="ai-mail-link" className="settings-ai-footer">
      {text('The AI you choose here answers you in chat and sorts your mail. Mail analysis can use a different AI in its own settings.', '你在此選擇的 AI 會在對話中回覆你，並整理你的郵件。郵件分析亦可在其設定中另選 AI。')}{' '}
      <Button kind="quiet" onClick={() => openSettings('mail')}>{row('ai-mail-link').title}</Button>
    </p>
    <p className="settings-ai-footer">
      {text('Bedrock, custom addresses, provider order and every model are in Advanced.', 'Bedrock、自訂位址、供應商次序及所有模型都在進階設定中。')}{' '}
      <Button kind="quiet" onClick={() => openSettings('advanced-models')}>{text('Open Advanced → Models', '開啟「進階 → 模型」')}</Button>
    </p>
  </>;
}
