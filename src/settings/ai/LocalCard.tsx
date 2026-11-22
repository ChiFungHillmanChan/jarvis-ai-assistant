import { useState } from 'react';
import { useUiLocale } from '../../i18n';
import { openUrl } from '../../lib/commands';
import { setupError } from '../../lib/setup';
import type { CurrentAi } from '../../lib/ai-choice';
import { testDisclosure } from '../../components/ChooseAi';
import type { useLocalSetup } from '../../components/LocalMailSetup';
import Button from '../../ui/Button';
import Select from '../../ui/Select';
import { useSettingsDialog } from '../SettingsContext';
import { TEST_AND_SAVE } from './providerGuides';
import { saveErrorLine, type TestAndUse } from './save';

interface LocalCardProps {
  /** The local setup flow (LocalMailSetup's useLocalSetup), shared with the card face. */
  setup: ReturnType<typeof useLocalSetup>;
  /** Why Ollama cannot run on this Mac (localUnavailable), or null. */
  unavailable: string | null;
  current: CurrentAi | null;
  busy: boolean;
  onTestAndUse: TestAndUse;
}

/** On this Mac: install Ollama, download the model that suits this Mac, then use a model Ollama has. */
export default function LocalCard({ setup, unavailable, current, busy, onTestAndUse }: LocalCardProps) {
  const { text } = useUiLocale();
  const { openSettings } = useSettingsDialog();
  const { probe } = setup;
  const installed = probe?.installedModels ?? [];
  const [model, setModel] = useState(current?.provider === 'ollama' && current.model ? current.model : '');
  const [error, setError] = useState('');
  const chosen = installed.includes(model) ? model : installed.includes(setup.model) ? setup.model : installed[0] ?? '';

  if (unavailable) {
    return <div className="settings-ai-detail">
      <h3>{text('Needs a newer Mac', '需要較新的 Mac')}</h3>
      <p className="ui-hint" role="note">{setupError(unavailable, text)}</p>
    </div>;
  }

  return <div className="settings-ai-detail">
    <h3>{text('Run the AI on this Mac', '在此 Mac 上執行 AI')}</h3>
    <p className="ui-hint">{text('Free, and nothing leaves your Mac. Replies are slower than Gemini.', '免費，所有資料都留在此 Mac，但回覆比 Gemini 慢。')}</p>
    <ol className="settings-ai-steps">
      <li>
        <span className="settings-ai-step-number" aria-hidden="true">1</span>
        <div className="settings-ai-step-body">
          <span>{text('Install the free Ollama app.', '安裝免費的 Ollama 應用程式。')}</span>
          {probe?.ollamaInstalled || probe?.ollamaRunning
            ? <span className="settings-chip is-on">{text('Installed', '已安裝')}</span>
            : <Button kind="secondary" disabled={!probe} onClick={() => openUrl(probe!.downloadUrl)} onError={() => setError(text('Could not open the official download page.', '未能開啟官方下載頁。'))}>{text('Download Ollama', '下載 Ollama')}</Button>}
        </div>
      </li>
      <li>
        <span className="settings-ai-step-number" aria-hidden="true">2</span>
        <div className="settings-ai-step-body">
          <span>{text('Let JARVIS download the model that suits your Mac.', '讓 JARVIS 下載適合你的 Mac 的模型。')}</span>
          {setup.model && <span className="settings-chip">{setup.model}</span>}
          <Button kind="secondary" busy={setup.busy} disabled={busy || setup.checking || !probe || !setup.model} onClick={() => setup.prepare()}>{text('Download model', '下載模型')}</Button>
          {setup.busy && <Button kind="quiet" disabled={setup.stage === 'applying'} onClick={setup.cancel}>{text('Cancel', '取消')}</Button>}
          {!setup.busy && <Button kind="quiet" disabled={setup.checking} onClick={setup.refresh}>{text('Check again', '重新檢查')}</Button>}
        </div>
      </li>
      {installed.length > 0 && <li>
        <span className="settings-ai-step-number" aria-hidden="true">3</span>
        <div className="settings-ai-step-body">
          <Select label={text('Model on this Mac', '此 Mac 上的模型')} showLabel value={chosen} disabled={busy} width={260} onChange={setModel}
            options={installed.map(m => ({ value: m, label: m }))} />
          <Button kind="primary" busy={busy} disabled={!chosen || setup.busy} onClick={() => { setError(''); return onTestAndUse({ provider: 'ollama', model: chosen }); }}
            onError={e => setError(saveErrorLine(e, text))}>{busy ? text('Testing…', '測試中…') : text(TEST_AND_SAVE.en, TEST_AND_SAVE.zh)}</Button>
        </div>
      </li>}
    </ol>
    {setup.busy && <div role="status" aria-live="polite"><p className="ui-hint">{setup.stageLabel}</p>
      {setup.bytes && <progress max={setup.bytes.total} value={Math.min(setup.bytes.done, setup.bytes.total)} />}</div>}
    {setup.ready && <p className="ui-hint" role="status">{text(`The model is downloaded. Choose it above and press "${TEST_AND_SAVE.en}": JARVIS tests it, then uses it for chat and mail.`, `模型已下載。請在上方選擇該模型並按「${TEST_AND_SAVE.zh}」：JARVIS 會先測試，然後用於聊天及郵件。`)}</p>}
    <p className="ui-hint">{testDisclosure('ollama', 'Ollama', text)}</p>
    <p className="ui-hint">{text('Other local servers, and a local model for mail only, are in Advanced.', '其他本機伺服器，以及只用於郵件的本機模型，都在進階設定中。')}{' '}
      <Button kind="quiet" onClick={() => openSettings('advanced-local')}>{text('Open Advanced → Local AI', '開啟「進階 → 本機 AI」')}</Button></p>
    {(setup.error || error) && <p className="ui-secret-error" role="alert">{setup.error || error}</p>}
  </div>;
}
