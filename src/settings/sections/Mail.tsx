import { useUiLocale } from '../../i18n';
import { sortedByText } from '../../lib/mail-presentation';
import Button from '../../ui/Button';
import Select from '../../ui/Select';
import SettingGroup from '../../ui/SettingGroup';
import SettingRow from '../../ui/SettingRow';
import Switch from '../../ui/Switch';
import { useSettingsDialog } from '../SettingsContext';
import { ROWS_BY_ID, rowText } from '../rows';
import { useMailSettings } from '../hooks/useMailSettings';

/**
 * Settings > Mail and follow-ups, moved from the Mail page's Analysis
 * settings dialog. "Sort new mail with AI" saves on change once its consent
 * is ticked; the analysis form (processing mode and its consent) keeps one
 * Save, the one exception to save-as-you-change (spec 6.3). The mail models
 * are chosen only in Advanced > Models, and the TypeSafe key only in
 * Advanced > Cloud providers.
 */
export default function Mail() {
  const { locale, text } = useUiLocale();
  const { openSettings } = useSettingsDialog();
  const m = useMailSettings(text);
  const row = (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });

  if (m.loadError) {
    return <p className="ui-secret-error" role="alert">{text('Could not load the mail settings.', '無法載入郵件設定。')} <Button kind="quiet" onClick={m.reload}>{text('Retry', '重試')}</Button></p>;
  }
  // A change made before the saved values arrive could be overwritten by them.
  if (!m.ready || !m.triage) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  const chosenAi = m.triage.chosen_ai;
  const typesafe = m.triage.typesafe_key;
  const sortingConsentLabel = typesafe
    ? text(`I understand that email text is sent to TypeSafe and, for some emails, ${chosenAi ?? 'my AI'}, as described above.`, `我明白郵件內容會按上述方式傳送至 TypeSafe，部分郵件亦會傳送至${chosenAi ? ` ${chosenAi}` : '我選擇的 AI'}。`)
    : text(`I understand that email text is sent to ${chosenAi ?? 'my AI'}, as described above.`, `我明白郵件內容會按上述方式傳送至${chosenAi ? ` ${chosenAi}` : '我選擇的 AI'}。`);
  const analysisConsentLabel = m.mode === 'openai'
    ? text("I understand that analysis sends the group's emails, instructions and active memories to OpenAI.", '我明白分析時，群組郵件、指示及有效記憶會傳送至 OpenAI。')
    : m.mode === 'bedrock'
    ? text("I understand that analysis sends the group's emails, instructions and active memories to AWS Bedrock.", '我明白分析時，群組郵件、指示及有效記憶會傳送至 AWS Bedrock。')
    : text("I understand that analysis sends the group's emails, instructions and active memories to that AI (unless it is Ollama on this Mac).", '我明白分析時，群組郵件、指示及有效記憶會傳送至該 AI（如為此 Mac 的 Ollama 則不會離開此 Mac）。');
  const modeHint = m.mode === 'assistant'
    ? text('Uses the AI chosen in Settings → Your AI, with its saved key.', '使用在「設定 → 你的 AI」選擇的 AI 及其已儲存的密鑰。')
    : m.mode === 'local'
    ? text('Install and start Ollama in Settings → Advanced → Local AI first.', '請先在「設定 → 進階 → 本機 AI」安裝並啟動 Ollama。')
    : m.mode === 'openai'
    ? text('Uses the OpenAI key saved in Settings → Advanced → Cloud providers. Your OpenAI account pays.', '使用在「設定 → 進階 → 雲端服務」儲存的 OpenAI 密鑰，費用由你的 OpenAI 帳戶支付。')
    : text('Add your AWS credentials and region in Settings → Advanced → Cloud providers first. Your AWS account pays.', '請先在「設定 → 進階 → 雲端服務」填入 AWS 憑證及地區；費用由你的 AWS 帳戶支付。');

  return <>
    <SettingGroup title={text('AI sorting', 'AI 自動整理')}>
      <SettingRow {...row('mail-auto-sort')} badge={text('Recommended', '推薦')} control={
        <Switch checked={m.sorting || m.pending} disabled={m.sortBusy} label={row('mail-auto-sort').title} onChange={m.toggleSorting} />
      }>
        {m.pending && <div className="ui-hint" role="status">{text('Not on yet: tick "Email text is sent to the AI" below to turn sorting on.', '尚未開啟：請勾選下方的「郵件文字會傳送給 AI」以開啟整理。')}</div>}
        {m.sortError && <div className="ui-secret-error" role="alert">{m.sortError}</div>}
      </SettingRow>

      <SettingRow {...row('mail-triage-consent')} control={
        <input type="checkbox" aria-label={sortingConsentLabel} checked={m.sortingConsent} disabled={m.sorting || m.sortBusy} onChange={e => m.tickSortingConsent(e.target.checked)} />
      }>
        <div className="ui-hint">{typesafe
          ? text(`Each new email is read once. TypeSafe Jev gets the sender, recipients, subject, the first 1,500 characters and your mailbox addresses to decide its category and whether it needs you. Only emails that need a summary or mention a date go to ${chosenAi ?? 'your AI'} (up to 6,000 characters). Nothing is sent from your Gmail or added to your calendar without your click.`, `每封新郵件只讀取一次：TypeSafe Jev 會收到寄件人、收件人、主旨、正文開頭 1,500 字及你的信箱地址，用來判斷分類及是否需要你處理；只有需要摘要或含日期的郵件，才會把最多 6,000 字傳送至${chosenAi ? ` ${chosenAi}` : '你選擇的 AI'}。未經你按下按鈕，不會寄出郵件或加入日曆。`)
          : text(`Each new email is read once. The sender, recipients, subject and up to 6,000 characters go to ${chosenAi ?? 'the AI you choose'}, which decides the category, writes a summary and finds dates. Nothing is sent from your Gmail or added to your calendar without your click.`, `每封新郵件只讀取一次：寄件人、收件人、主旨及最多 6,000 字正文會傳送至${chosenAi ? ` ${chosenAi}` : '你選擇的 AI'}，用來判斷分類、撰寫摘要及找出日期。未經你按下按鈕，不會寄出郵件或加入日曆。`)}</div>
        <div className="ui-hint">{sortingConsentLabel}</div>
      </SettingRow>

      <SettingRow {...row('mail-ai-status')} control={null}>
        {chosenAi
          ? <div className="ui-hint" role="status">{sortedByText(m.triage, text)}</div>
          : <div className="ui-hint" role="status">{text('No AI chosen yet: choose one in Settings → Your AI.', '尚未選擇 AI：請到「設定 → 你的 AI」選擇。')} <Button kind="quiet" onClick={() => openSettings('ai')}>{text('Go to Your AI', '前往「你的 AI」')}</Button></div>}
        {!typesafe && <div className="ui-hint">{text(`With a TypeSafe key, Jev sorts every email first and only the ones that need a summary go to ${chosenAi ?? 'your AI'}.`, `加入 TypeSafe 密鑰後，由 Jev 先分類每封郵件，只有需要摘要的郵件才交給${chosenAi ? ` ${chosenAi}` : '你選擇的 AI'}。`)} <Button kind="quiet" onClick={() => openSettings('advanced-cloud')}>{text('Add a key in Advanced → Cloud providers', '在進階 → 雲端服務中加入密鑰')}</Button></div>}
      </SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Group analysis', '群組分析')}>
      <SettingRow {...row('mail-processing')} control={
        <Select value={m.mode} label={row('mail-processing').title} width={240} onChange={v => m.setMode(v)}
          options={[{ value: 'assistant', label: text('Same AI as the assistant', '與 AI 助理相同的 AI') }, { value: 'local', label: text('Ollama on this Mac', '此 Mac 的 Ollama') }, { value: 'openai', label: 'OpenAI' }, { value: 'bedrock', label: text('Your AWS Bedrock account', '自備 AWS Bedrock 帳戶') }]} />
      }>
        <div className="ui-hint">{modeHint}</div>
        <Button kind="quiet" onClick={() => openSettings('advanced-models')}>{text('Choose the mail model in Advanced → Models', '在進階 → 模型中選擇郵件模型')}</Button>
      </SettingRow>

      <SettingRow {...row('mail-analysis-consent')} control={m.mode === 'local' ? null :
        <input type="checkbox" aria-label={analysisConsentLabel} checked={m.analysisConsent} onChange={e => m.setAnalysisConsent(e.target.checked)} />
      }>
        <span className="ui-hint">{m.mode === 'local' ? text('Uses Ollama on this Mac only, with no cloud fallback.', '只使用此 Mac 上的 Ollama，不會自動改用雲端。') : analysisConsentLabel}</span>
      </SettingRow>

      <div className="settings-row-actions settings-row-actions-start">
        <Button kind="primary" disabled={!m.savable} busy={m.saving} onClick={m.save}>{m.saving ? text('Saving…', '正在儲存…') : text('Save', '儲存')}</Button>
        {!m.savable && m.blockedReason && <span className="ui-hint">{m.blockedReason}</span>}
        {m.saved && <span className="ui-hint" role="status">{text('Saved.', '已儲存。')}</span>}
        {m.saveError && <span className="ui-secret-error" role="alert">{m.saveError}</span>}
      </div>
    </SettingGroup>
  </>;
}
