import { useEffect, useRef, useState } from 'react';
import { useUiLocale } from '../i18n';
import { getAwsCredentialsStatus, markOnboardingComplete, setApiKey, setAwsCredentials } from '../lib/commands';
import * as mail from '../lib/mail';
import { canStartFirstAnalysis, triageConsentText } from '../lib/setup-flow';
import { applyTestedMailSettings, getSetupStatus, setupError, testMailAiConnection, type SetupStatus } from '../lib/setup';
import ChooseAi from '../components/ChooseAi';
import Select from '../ui/Select';
import { parseAiError, problemText } from '../lib/ai-error';
import BedrockModelPicker from '../components/BedrockModelPicker';
import { useBedrockModelList } from '../hooks/useBedrockModelList';
import { modelChosen } from '../lib/bedrock-models';
import { useAiModels } from '../lib/ai-models';
import WindowControls from '../components/WindowControls';
import ReadableContent from '../components/content/ReadableContent';
import '../styles/setup.css';

export default function OnboardingWizard({ onComplete }: { onComplete: () => void }) {
  const models = useAiModels();
  const { locale, setLocale, text, saveError, retrySave } = useUiLocale();
  const [step, setStep] = useState(1);
  const [workspace, setWorkspace] = useState<mail.MailWorkspace | null>(null);
  const [settings, setSettings] = useState<mail.MailSettings | null>(null);
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [apiKey, setKey] = useState('');
  const [accessKey, setAccessKey] = useState('');
  const [secretKey, setSecretKey] = useState('');
  const [region, setRegion] = useState('us-east-1');
  const [consent, setConsent] = useState(false);
  const [accountId, setAccountId] = useState('');
  const [messages, setMessages] = useState<mail.MailMessage[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [groupName, setGroupName] = useState('');
  const [instructions, setInstructions] = useState('');
  const [analysis, setAnalysis] = useState<mail.MailAnalysis | null>(null);
  const [source, setSource] = useState<mail.MailMessage | null>(null);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [busy, setBusy] = useState('');
  const [localBusy, setLocalBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [credentialDirty, setCredentialDirty] = useState(false);
  const [triageDeclined, setTriageDeclined] = useState(false);
  // The Bedrock model list, asked for only while the Advanced Bedrock block
  // is open on step 1 (hooks/useBedrockModelList.ts). It reads the saved
  // credentials only, never the keys typed here.
  const [bedrockOpen, setBedrockOpen] = useState(false);
  const bedrockList = useBedrockModelList(step === 1 && bedrockOpen);
  const operation = useRef(false);
  const mounted = useRef(true);
  const firstGroup = useRef<number | null>(null);

  async function reload() {
    const [data, readiness] = await Promise.all([mail.mailWorkspace(), getSetupStatus()]);
    if (!mounted.current) return;
    setWorkspace(data); setSettings(data.settings); setStatus(readiness);
    setAccountId(current => data.accounts.some(account => account.id === current) ? current : data.accounts.find(account => account.connected)?.id || '');
  }
  useEffect(() => {
    mounted.current = true;
    void reload().catch(() => setError(text('Could not read setup status. Retry to avoid overwriting existing settings.', '未能讀取設定狀態。請重新載入，避免覆蓋現有設定。'))).finally(() => setLoading(false));
    void getAwsCredentialsStatus().then(value => { if (mounted.current && value.region) setRegion(value.region); }).catch(() => {});
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    if (step !== 3 || firstGroup.current !== null) return;
    let active = true;
    setMessages([]); setSelected([]);
    void mail.mailListMessages({ accountId: accountId || undefined, offset: 0 }).then(page => { if (active) setMessages(page.messages.slice(0, 10)); }).catch(() => { if (active) setError(text('Could not load local mail. Return to the connection step and retry.', '未能載入本機郵件，請返回連接步驟重試。')); });
    return () => { active = false; };
  }, [step, accountId]);

  async function run(label: string, action: () => Promise<void>) {
    if (operation.current) return;
    operation.current = true; setBusy(label); setError(''); setNotice('');
    try { await action(); }
    catch (reason) {
      const raw = reason instanceof Error ? reason.message : String(reason);
      const problem = parseAiError(raw);
      if (mounted.current) setError(problem ? problemText(problem, text) : setupError(raw, text));
    }
    finally { operation.current = false; if (mounted.current) setBusy(''); }
  }
  const connectingGmail = text('Connecting Gmail', '連接 Gmail');
  function changeSettings(next: mail.MailSettings) { setSettings(next); setAnalysis(null); setConsent(false); }
  const sameSettings = !!settings && JSON.stringify(settings) === JSON.stringify(workspace?.settings);
  const readiness = sameSettings && !credentialDirty ? status?.mail.state || 'not_configured' : 'saved_unverified';
  function goStep(next: number) { setError(''); setNotice(''); setBedrockOpen(false); setStep(next); }
  const canAnalyze = canStartFirstAnalysis(readiness, selected.length);

  async function testAndSave() {
    if (!settings) return;
    await run(text('Testing AI', '測試 AI'), async () => {
      const previous = workspace!.settings;
      if (settings.processing_mode === 'openai' && apiKey.trim()) {
        await setApiKey('openai', apiKey.trim()); setKey('');
      }
      if (settings.processing_mode === 'bedrock' && (accessKey.trim() || secretKey.trim())) {
        if (!accessKey.trim() || !secretKey.trim()) throw Error('credential_missing');
        await setAwsCredentials(accessKey.trim(), secretKey.trim(), region);
        setAccessKey(''); setSecretKey('');
        // Saved before the test, so the list is asked again whatever the test says.
        bedrockList.reload();
      }
      const tested = await testMailAiConnection(settings);
      if (tested.state !== 'ready') { setStatus(current => current ? { ...current, mail: tested } : current); throw Error(tested.errorCode || 'probe_failed'); }
      await applyTestedMailSettings(settings, previous);
      setCredentialDirty(false);
      try { await reload(); } catch { setError(text('Mail settings were saved, but this page could not refresh. Reopen setup to check.', '電郵設定已儲存，但畫面未能更新。請重新開啟導引以核對。')); }
      setNotice(text('The synthetic test passed. The selected model is saved for mail.', '合成測試通過，所選模型已儲存供電郵工作區使用。'));
    });
  }
  // The choice already pointed mail at the same AI; confirm the mail
  // pipeline (citations included) works with it before step 3.
  async function verifyMail() {
    await run(text('Checking mail analysis', '檢查電郵分析'), async () => {
      await reload();
      const tested = await testMailAiConnection();
      if (mounted.current) setStatus(current => current ? { ...current, mail: tested } : current);
      if (tested.state !== 'ready') throw Error(tested.errorCode || 'probe_failed');
    });
  }
  async function syncAccount(id: string) {
    const result = await mail.mailSyncAccount(id, false);
    await reload();
    setNotice(text(`Saved ${result.saved} messages. ${result.warning ? 'Sync was partial; retry to finish.' : result.has_more ? 'More mail can be loaded in the workspace.' : 'This batch is complete.'}`, `已儲存 ${result.saved} 封郵件。${result.warning ? '本次同步僅完成部分，請重試。' : result.has_more ? '仍有較舊的郵件，可在工作區繼續載入。' : '本批同步已完成。'}`));
  }
  async function firstAnalysis() {
    if (!canAnalyze || !groupName.trim()) return;
    await run(text('Analyzing your group', '分析群組'), async () => {
      if (firstGroup.current === null) {
        const group = await mail.mailSaveGroup(groupName.trim(), instructions || text('Summarize confirmed dates, questions and next steps. Keep source citations and mark uncertainty.', '以繁體書面語整理已確認的日期、待回覆問題及下一步。保留原信引用，並標示尚未確認的資料。'));
        firstGroup.current = group.id;
      }
      await mail.mailSetGroupMembers(firstGroup.current, selected, true);
      setAnalysis(await mail.mailAnalyzeGroup(firstGroup.current));
    });
  }
  function finish() {
    void run(text('Opening mail', '開啟電郵'), async () => {
      if (!await setLocale(locale)) return;
      await markOnboardingComplete();
      onComplete();
    });
  }

  return <div className="setup-screen">
    <header className="setup-titlebar" data-tauri-drag-region><WindowControls /><span data-tauri-drag-region>JARVIS</span></header>
    <main className="setup-content">
      <div className="setup-topline"><span>{text(`Step ${step} of 3`, `第 ${step}／3 步`)}</span><Select label={text('Interface language', '介面語言')} showLabel width={190} disabled={!!busy || localBusy} value={locale} onChange={setLocale} options={[{ value: 'zh-HK', label: '繁體中文（香港）' }, { value: 'en', label: 'English' }]} /></div>
      <h1>{step === 1 ? text('Choose your AI', '選擇你的 AI') : step === 2 ? text('Connect your mail workspace', '連接電郵工作區') : text('Get your first useful result', '取得第一份實用結果')}</h1>
      <p>{text('Existing credentials and connected mailboxes are reused. Calendar and voice have separate optional setup.', '系統會沿用已儲存的憑證及已連接的信箱。日曆及語音可按需要另行設定。')}</p>
      {saveError && <p className="setup-error" role="alert">{text(saveError, '未能儲存介面語言。關閉 JARVIS 前請重試。')} <button onClick={retrySave}>{text('Retry saving language', '重試儲存語言')}</button></p>}
      {error && <p className="setup-error" role="alert">{error}</p>}
      {notice && <p className="setup-notice" role="status">{notice}</p>}
      {busy && <p role="status">{busy}…</p>}
      {busy === connectingGmail && <button type="button" onClick={() => void mail.mailCancelConnect().catch(() => {})}>{text('Cancel Gmail sign-in', '取消 Gmail 登入')}</button>}
      {loading ? <p role="status">{text('Reading existing settings…', '正在讀取現有設定…')}</p> : !workspace || !settings ? <button onClick={() => void run(text('Reloading', '重新載入'), reload)}>{text('Retry', '重試')}</button> : <fieldset className="setup-fields" disabled={!!busy}>
        {step === 1 && <>
          <ChooseAi onChosen={() => void verifyMail()} onBusyChange={setLocalBusy} />
          {readiness === 'ready' && <p className="setup-notice">{text('Mail analysis passed its test with this AI. Press Continue to connect Gmail.', '電郵分析已用這個 AI 通過測試。按「繼續」連接 Gmail。')}</p>}
          <details onToggle={event => { const open = (event.target as HTMLDetailsElement).open; setBedrockOpen(open); if (open && settings.processing_mode !== 'bedrock') changeSettings({ ...settings, processing_mode: 'bedrock' }); }}>
            <summary>{text('Advanced: use a company AWS Bedrock account for mail', '進階：電郵改用公司 AWS Bedrock 帳戶')}</summary>
            <p>{text('Your company administrator provides these credentials. Model availability depends on the region and account. Start with a text model approved by your company.', '憑證由公司管理員提供；可用模型取決於地區及帳戶。請先使用公司批准的文字模型。')}</p>
            <label>Access Key ID<input autoComplete="off" value={accessKey} onChange={event => { setAccessKey(event.target.value); setCredentialDirty(true); }} placeholder={text('Leave both keys blank to reuse saved credentials', '兩個欄位都留空會沿用已儲存憑證')} /></label>
            <label>Secret Access Key<input type="password" autoComplete="off" value={secretKey} onChange={event => { setSecretKey(event.target.value); setCredentialDirty(true); }} /></label>
            <Select label={text('Region for new credentials', '新憑證使用的地區')} showLabel value={region} onChange={setRegion} options={[{ value: 'us-east-1', label: 'us-east-1 — US' }, { value: 'ap-southeast-1', label: 'ap-southeast-1 — Singapore' }, { value: 'ap-northeast-1', label: 'ap-northeast-1 — Tokyo' }, { value: 'eu-west-2', label: 'eu-west-2 — London' }]} />
            <BedrockModelPicker place="setup" value={settings.bedrock_model} stored={workspace.settings.bedrock_model} fallback={models?.defaults.bedrock ?? ''} listState={bedrockList.list} onReload={bedrockList.reload} onChange={model => changeSettings({ ...settings, processing_mode: 'bedrock', bedrock_model: model })} />
            <label className="setup-checkbox"><input type="checkbox" checked={consent} onChange={event => setConsent(event.target.checked)} />{text('The test sends synthetic text and may incur a small charge. Later mail analysis sends selected mail content, instructions and valid memories to this provider.', '測試會傳送合成文字，可能產生少量費用。之後分析郵件時，所選郵件內容、指示及有效記憶會傳送至此供應商。')}</label>
            <button type="button" className="setup-primary" disabled={!consent || settings.processing_mode !== 'bedrock' || !modelChosen(settings.bedrock_model)} onClick={() => void testAndSave()}>{text('Test and use Bedrock for mail', '測試並以 Bedrock 處理電郵')}</button>
          </details>
        </>}
        {step === 2 && <>
          <div className="setup-current"><h2>{text('Read-only Gmail and Calendar access', '唯讀授權 Gmail 及日曆')}</h2><p>{text('JARVIS requests read-only mail and calendar access and stores readable copies of the mail on this Mac. This connection cannot send mail or change events. Disconnecting keeps local copies; deleting a local account removes them.', 'JARVIS 會要求唯讀郵件及日曆權限，並將可讀郵件副本儲存於此 Mac。本次連接不能寄信，亦不能修改日曆活動。斷開連接會保留副本；刪除本機帳戶才會清除副本。')}</p></div>
          {workspace.accounts.length > 0 && <Select label={text('Mailbox', '信箱')} showLabel value={accountId} onChange={setAccountId} options={workspace.accounts.map(account => ({ value: account.id, label: `${account.email} · ${account.connected ? text('Connected', '已連接') : text('Disconnected', '已斷開')} · ${account.message_count}` }))} />}
          <div className="setup-actions"><button type="button" onClick={() => void run(connectingGmail, async () => { const account = await mail.mailConnectAccount(); setAccountId(account.id); await syncAccount(account.id); })}>{text('Connect Gmail and sync first batch', '連接 Gmail 並同步首批郵件')}</button><button type="button" disabled={!workspace.accounts.find(account => account.id === accountId)?.connected} onClick={() => void run(text('Syncing mail', '同步郵件'), () => syncAccount(accountId))}>{text('Sync selected mailbox', '同步所選信箱')}</button></div>
          <p className="setup-hint">{text('Up to 50 messages per sync, not a full backup. Company accounts may need administrator approval. If sign-in expires or is blocked, check the account policy before retrying.', '每次最多同步 50 封，並非完整備份。公司帳戶可能需要管理員批准；登入逾時或被封鎖時，請先核對帳戶政策。')}</p>
          {(workspace.triage.chosen_ai || workspace.triage.typesafe_key) && <div className="setup-current"><h2>{text('Sort new mail with AI', '以 AI 整理新郵件')}</h2>
            {workspace.triage.enabled ? <p>{text('AI sorting is on. You can turn it off in Mail, under Analysis settings.', '已開啟 AI 整理。可在電郵工作區的「分析設定」關閉。')}</p>
              : triageDeclined ? <p>{text('AI sorting stays off. You can turn it on later in Mail, under Analysis settings.', 'AI 整理維持關閉。之後可在電郵工作區的「分析設定」開啟。')}</p>
              : <><p>{text('JARVIS can read each new email once to sort it, write a summary and list its follow-ups and dates.', 'JARVIS 可讀取每封新郵件一次，用來分類、撰寫摘要，並列出待辦事項及日期。')} {triageConsentText(workspace.triage, text)}</p>
                <div className="setup-actions"><button type="button" className="setup-primary" onClick={() => void run(text('Turning on AI sorting', '開啟 AI 整理'), async () => { await mail.mailSetAutoTriage(true); await reload(); })}>{text('Turn on', '開啟')}</button><button type="button" onClick={() => setTriageDeclined(true)}>{text('Not now', '暫不開啟')}</button></div></>}
          </div>}
        </>}
        {step === 3 && <>
          <p>{text('Select 2–5 short messages for one customer or project. Grouping them does not change Gmail labels.', '選取同一客戶或項目的 2–5 封簡短郵件；加入群組不會變更 Gmail 標籤。')}</p>
          <div className="setup-mail-list">{messages.map(message => <label key={message.id} className="setup-checkbox"><input type="checkbox" checked={selected.includes(message.id)} disabled={firstGroup.current !== null || (!selected.includes(message.id) && selected.length >= 5)} onChange={event => setSelected(ids => event.target.checked ? [...ids, message.id] : ids.filter(id => id !== message.id))} /><span>{message.subject || text('(No subject)', '（無主旨）')}<small>{message.sender}</small></span></label>)}{!messages.length && <p>{text('No local mail yet. Return to the previous step to sync, or open the workspace and connect later.', '尚無本機郵件。可返回上一步同步，或先開啟工作區，稍後再連接。')}</p>}</div>
          <label>{text('Group name', '群組名稱')}<input maxLength={100} value={groupName} disabled={firstGroup.current !== null} onChange={event => setGroupName(event.target.value)} placeholder={text('For example: Customer follow-up', '例如：客戶跟進')} /></label>
          <label>{text('Instructions for this group', '此群組的整理指示')}<textarea maxLength={8000} rows={3} value={instructions} disabled={firstGroup.current !== null} onChange={event => setInstructions(event.target.value)} placeholder={text('List confirmed dates, unanswered questions and next steps.', '列出已確認的日期、待回覆問題及下一步。')} /></label>
          <p className="setup-hint">{workspace.settings.processing_mode === 'local' ? text('Analysis uses the local model. If the group is too long, reduce its size in the mail workspace.', '分析使用本機模型。群組內容過長時，可在電郵工作區縮小範圍。') : workspace.settings.processing_mode === 'assistant' ? text('Analysis sends this group to the AI you chose, using your key.', '分析會使用你的密鑰，將此群組內容傳送至你選擇的 AI。') : text(`Analysis sends this group to ${workspace.settings.processing_mode === 'openai' ? 'OpenAI' : 'AWS Bedrock'} using your account.`, `分析會使用你的帳戶，將此群組內容傳送至 ${workspace.settings.processing_mode === 'openai' ? 'OpenAI' : 'AWS Bedrock'}。`)}</p>
          {readiness !== 'ready' && <p>{text('Return to step 1 to test the selected AI before analyzing.', '分析前，請返回第 1 步測試所選 AI。')}</p>}
          <button type="button" className="setup-primary" disabled={!canAnalyze || !groupName.trim()} onClick={() => void firstAnalysis()}>{text('Create group and analyze', '建立群組並分析')}</button>
          {analysis && <article className="setup-result"><h2>{text('Open a source to check the result', '開啟原信以核對結果')}</h2><ReadableContent text={analysis.text} sourceIds={analysis.source_ids} onSource={id => void run(text('Opening source', '開啟原信'), async () => { setSource(await mail.mailGetMessage(id)); })} /><p>{text('Messages omitted', '未納入郵件')}: {analysis.omitted_messages}</p><div className="setup-actions">{analysis.source_ids.map(id => <button type="button" key={id} onClick={() => void run(text('Opening source', '開啟原信'), async () => { setSource(await mail.mailGetMessage(id)); })}>{text('Source', '原信')} #{id}</button>)}</div>{source && <section><h3>{source.subject}</h3><ReadableContent text={source.body || source.snippet} format="plain" /></section>}</article>}
        </>}
        <footer className="setup-footer"><button type="button" disabled={step === 1 || localBusy} onClick={() => goStep(step - 1)}>{text('Back', '上一步')}</button>{step < 3 && <button type="button" className={step === 1 && readiness === 'ready' ? 'setup-primary' : undefined} disabled={localBusy} onClick={() => goStep(step + 1)}>{text('Continue', '繼續')}</button>}<button type="button" disabled={localBusy} onClick={finish}>{step === 3 ? text('Open mail workspace', '開啟電郵工作區') : text('Set up later', '稍後繼續設定')}</button></footer>
      </fieldset>}
    </main>
  </div>;
}
