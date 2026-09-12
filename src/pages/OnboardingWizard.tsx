import { useEffect, useRef, useState } from 'react';
import { useUiLocale } from '../i18n';
import { getAwsCredentialsStatus, markOnboardingComplete, openUrl, setApiKey, setAwsCredentials } from '../lib/commands';
import * as mail from '../lib/mail';
import { canStartFirstAnalysis } from '../lib/setup-flow';
import { applyTestedMailSettings, getSetupStatus, setupError, testMailAiConnection, type SetupStatus } from '../lib/setup';
import LocalMailSetup from '../components/LocalMailSetup';
import WindowControls from '../components/WindowControls';
import '../styles/setup.css';

export default function OnboardingWizard({ onComplete }: { onComplete: () => void }) {
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
    void mail.mailListMessages({ accountId: accountId || undefined, offset: 0 }).then(page => { if (active) setMessages(page.messages.slice(0, 10)); }).catch(() => { if (active) setError(text('Could not load local mail. Return to the connection step and retry.', '未能載入本機郵件，請返回連接步驟再試。')); });
    return () => { active = false; };
  }, [step, accountId]);

  async function run(label: string, action: () => Promise<void>) {
    if (operation.current) return;
    operation.current = true; setBusy(label); setError(''); setNotice('');
    try { await action(); }
    catch (reason) { if (mounted.current) setError(setupError(reason instanceof Error ? reason.message : String(reason), text)); }
    finally { operation.current = false; if (mounted.current) setBusy(''); }
  }
  function changeSettings(next: mail.MailSettings) { setSettings(next); setAnalysis(null); setConsent(false); }
  const sameSettings = !!settings && JSON.stringify(settings) === JSON.stringify(workspace?.settings);
  const readiness = sameSettings && !credentialDirty ? status?.mail.state || 'not_configured' : 'saved_unverified';
  const savedModel = workspace?.settings.processing_mode === 'local' ? workspace.settings.local_model : workspace?.settings.processing_mode === 'openai' ? workspace.settings.openai_model : workspace?.settings.bedrock_model;
  function goStep(next: number) { setError(''); setNotice(''); setStep(next); }
  const canAnalyze = canStartFirstAnalysis(readiness, status?.gmail.connectedAccounts || 0, selected.length);

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
      }
      const tested = await testMailAiConnection(settings);
      if (tested.state !== 'ready') { setStatus(current => current ? { ...current, mail: tested } : current); throw Error(tested.errorCode || 'probe_failed'); }
      await applyTestedMailSettings(settings, previous);
      setCredentialDirty(false);
      try { await reload(); } catch { setError(text('Mail settings were saved, but this page could not refresh. Reopen setup to check.', '電郵設定已儲存，但畫面未能更新。請重開導引核對。')); }
      setNotice(text('The synthetic test passed. The selected model is saved for mail.', '合成測試通過，所選模型已儲存畀電郵工作區使用。'));
    });
  }
  async function syncAccount(id: string) {
    const result = await mail.mailSyncAccount(id, false);
    await reload();
    setNotice(text(`Saved ${result.saved} messages. ${result.warning ? 'Sync was partial; retry to finish.' : result.has_more ? 'More mail can be loaded in the workspace.' : 'This batch is complete.'}`, `已儲存 ${result.saved} 封郵件。${result.warning ? '今次同步只完成部分，請重試。' : result.has_more ? '仲有較舊郵件，可喺工作區繼續載入。' : '今批同步完成。'}`));
  }
  async function firstAnalysis() {
    if (!canAnalyze || !groupName.trim()) return;
    await run(text('Analyzing your group', '分析群組'), async () => {
      if (firstGroup.current === null) {
        const group = await mail.mailSaveGroup(groupName.trim(), instructions || text('Summarize confirmed dates, questions and next steps. Keep source citations and mark uncertainty.', '整理已確認日期、待覆問題同下一步。保留原信引用，未確認嘅資料要寫明。'));
        firstGroup.current = group.id;
      }
      await mail.mailSetGroupMembers(firstGroup.current, selected, true);
      setAnalysis(await mail.mailAnalyzeGroup(firstGroup.current));
    });
  }
  function finish() {
    void run(text('Opening mail', '開啟電郵'), async () => { await markOnboardingComplete(); onComplete(); });
  }

  return <div className="setup-screen">
    <header className="setup-titlebar" data-tauri-drag-region><WindowControls /><span data-tauri-drag-region>JARVIS</span></header>
    <main className="setup-content">
      <div className="setup-topline"><span>{text(`Step ${step} of 3`, `第 ${step}／3 步`)}</span><label>{text('Interface language', '介面語言')}<select disabled={!!busy || localBusy} value={locale} onChange={event => setLocale(event.target.value as 'en' | 'zh-HK')}><option value="zh-HK">繁體中文（香港）</option><option value="en">English</option></select></label></div>
      <h1>{step === 1 ? text('Set up AI for your mail', '設定電郵 AI') : step === 2 ? text('Connect your mail workspace', '連接電郵工作區') : text('Get your first useful result', '整理第一份有用結果')}</h1>
      <p>{text('Existing credentials and connected mailboxes are reused. Assistant, calendar and voice have separate optional setup.', '會沿用已儲存憑證同已連接信箱。AI 助理、日曆同語音可按需要另行設定。')}</p>
      {saveError && <p className="setup-error" role="alert">{saveError} <button onClick={retrySave}>{text('Retry saving language', '重試儲存語言')}</button></p>}
      {error && <p className="setup-error" role="alert">{error}</p>}
      {notice && <p className="setup-notice" role="status">{notice}</p>}
      {busy && <p role="status">{busy}…</p>}
      {loading ? <p role="status">{text('Reading existing settings…', '讀取緊現有設定…')}</p> : !workspace || !settings ? <button onClick={() => void run(text('Reloading', '重新載入'), reload)}>{text('Retry', '重試')}</button> : <fieldset className="setup-fields" disabled={!!busy}>
        {step === 1 && <>
          <div className="setup-current"><strong>{text('Current mail AI', '目前電郵 AI')}: </strong>{savedModel ? workspace?.settings.processing_mode : text('Not configured', '未設定')} / {savedModel || '—'}<p>{readiness === 'ready' ? text('Test passed in this session. Availability can change; normal requests still report errors.', '今次開啟 App 後測試通過。服務狀態可能改變，實際使用仍會回報錯誤。') : text('Saved settings are not proof of a working connection. Test before continuing.', '有儲存設定唔代表連接可用，請先測試。')}</p></div>
          <label>{text('Processing method', '處理方式')}<select disabled={localBusy} value={settings.processing_mode} onChange={event => changeSettings({ ...settings, processing_mode: event.target.value })}><option value="local">{text('Local AI — no API key', '本機 AI — 唔使 API key')}</option><option value="openai">{text('OpenAI — own account, eligible regions', 'OpenAI — 自備帳戶，限合資格地區')}</option><option value="bedrock">{text('AWS Bedrock — own company account', 'AWS Bedrock — 自備公司帳戶')}</option></select></label>
          {settings.processing_mode === 'local' ? <><LocalMailSetup onReady={reload} onBusyChange={setLocalBusy} /><details><summary>{text('Use another installed local model', '使用其他已安裝本機模型')}</summary><label>{text('Exact model name', '完整模型名稱')}<input disabled={localBusy} value={settings.local_model} onChange={event => changeSettings({ ...settings, local_model: event.target.value })} /></label><button type="button" disabled={localBusy} onClick={() => void testAndSave()}>{text('Test and use this model', '測試並使用呢個模型')}</button></details></> : <>
            {settings.processing_mode === 'openai' ? <>
              <p className="setup-hint">{text('Hong Kong is not listed for the OpenAI API. Use an eligible account and region, or choose local AI. You only need one provider for mail.', 'OpenAI API 支援清單未列香港。請用合資格帳戶同地區，或者揀本機 AI。電郵只需要設定一個供應商。')}</p>
              <label>OpenAI API key<input type="password" autoComplete="off" spellCheck={false} value={apiKey} onChange={event => { setKey(event.target.value); setCredentialDirty(true); }} placeholder={text('Leave blank to keep the saved key', '留空會沿用已儲存密鑰')} /></label>
              <button type="button" onClick={() => void openUrl('https://platform.openai.com/api-keys').catch(() => setError(text('Could not open the provider page.', '未能開啟供應商頁面。')))}>{text('Open official API key page', '開啟官方 API key 頁')}</button>
              <label>{text('Model', '模型')}<input required value={settings.openai_model} onChange={event => changeSettings({ ...settings, openai_model: event.target.value })} placeholder="gpt-5.6-luna" /></label>
            </> : <>
              <p>{text('Your company administrator provides these credentials. Model availability depends on the region and account. Start with a text model approved by your company.', '由公司管理員提供憑證；模型供應視乎地區同帳戶。先用公司批准嘅文字模型。')}</p>
              <label>Access Key ID<input autoComplete="off" value={accessKey} onChange={event => { setAccessKey(event.target.value); setCredentialDirty(true); }} placeholder={text('Leave both keys blank to reuse saved credentials', '兩個欄位都留空會沿用已儲存憑證')} /></label>
              <label>Secret Access Key<input type="password" autoComplete="off" value={secretKey} onChange={event => { setSecretKey(event.target.value); setCredentialDirty(true); }} /></label>
              <label>{text('Region for new credentials', '新憑證使用嘅地區')}<select value={region} onChange={event => setRegion(event.target.value)}><option value="us-east-1">us-east-1 — US</option><option value="ap-southeast-1">ap-southeast-1 — Singapore</option><option value="ap-northeast-1">ap-northeast-1 — Tokyo</option><option value="eu-west-2">eu-west-2 — London</option></select></label>
              <label>{text('Bedrock model ID', 'Bedrock 模型 ID')}<input required value={settings.bedrock_model} onChange={event => changeSettings({ ...settings, bedrock_model: event.target.value })} placeholder="amazon.nova-micro-v1:0" /></label>
            </>}
            <label className="setup-checkbox"><input type="checkbox" checked={consent} onChange={event => setConsent(event.target.checked)} />{text('The test sends synthetic text and may incur a small charge. Later mail analysis sends selected mail content, instructions and valid memories to this provider.', '測試會傳送合成文字，可能產生少量費用。之後分析郵件時，所選郵件內容、指示同有效記憶會傳送畀呢個供應商。')}</label>
            <button type="button" className="setup-primary" disabled={!consent} onClick={() => void testAndSave()}>{text('Test and save', '測試並儲存')}</button>
          </>}
        </>}
        {step === 2 && <>
          <div className="setup-current"><h2>{text('Gmail access for mail only', '只為電郵工作區授權 Gmail')}</h2><p>{text('JARVIS requests read-only mail access and stores readable copies on this Mac. This connection does not grant sending or calendar permissions. Disconnecting keeps local copies; deleting a local account removes them.', 'JARVIS 會要求唯讀郵件權限，將可讀副本儲存喺呢部 Mac。呢次連接唔會要求寄信或日曆權限。斷開連接會保留副本；刪除本機帳戶先會清除。')}</p></div>
          {workspace.accounts.length > 0 && <label>{text('Mailbox', '信箱')}<select value={accountId} onChange={event => setAccountId(event.target.value)}>{workspace.accounts.map(account => <option key={account.id} value={account.id}>{account.email} · {account.connected ? text('Connected', '已連接') : text('Disconnected', '已斷開')} · {account.message_count}</option>)}</select></label>}
          <div className="setup-actions"><button type="button" onClick={() => void run(text('Connecting Gmail', '連接 Gmail'), async () => { const account = await mail.mailConnectAccount(); setAccountId(account.id); await syncAccount(account.id); })}>{text('Connect Gmail and sync first batch', '連接 Gmail 並同步首批郵件')}</button><button type="button" disabled={!workspace.accounts.find(account => account.id === accountId)?.connected} onClick={() => void run(text('Syncing mail', '同步郵件'), () => syncAccount(accountId))}>{text('Sync selected mailbox', '同步所選信箱')}</button></div>
          <p className="setup-hint">{text('Up to 50 messages per sync, not a full backup. Company accounts may need administrator approval. If sign-in expires or is blocked, check the account policy before retrying.', '每次最多同步 50 封，唔係完整備份。公司帳戶可能要管理員批准；登入逾時或被阻擋時，請先核對帳戶政策。')}</p>
        </>}
        {step === 3 && <>
          <p>{text('Select 2–5 short messages for one customer or project. Grouping them does not change Gmail labels.', '揀同一客戶或項目嘅 2–5 封短信；加入群組唔會改 Gmail 標籤。')}</p>
          <div className="setup-mail-list">{messages.map(message => <label key={message.id} className="setup-checkbox"><input type="checkbox" checked={selected.includes(message.id)} disabled={firstGroup.current !== null || (!selected.includes(message.id) && selected.length >= 5)} onChange={event => setSelected(ids => event.target.checked ? [...ids, message.id] : ids.filter(id => id !== message.id))} /><span>{message.subject || text('(No subject)', '（冇主旨）')}<small>{message.sender}</small></span></label>)}{!messages.length && <p>{text('No local mail yet. Return to the previous step to sync, or open the workspace and connect later.', '未有本機郵件。可以返回上一步同步，或者先開工作區，之後再連接。')}</p>}</div>
          <label>{text('Group name', '群組名稱')}<input maxLength={100} value={groupName} disabled={firstGroup.current !== null} onChange={event => setGroupName(event.target.value)} placeholder={text('For example: Customer follow-up', '例如：客戶跟進')} /></label>
          <label>{text('Instructions for this group', '呢組嘅整理指示')}<textarea maxLength={8000} rows={3} value={instructions} disabled={firstGroup.current !== null} onChange={event => setInstructions(event.target.value)} placeholder={text('List confirmed dates, unanswered questions and next steps.', '列出已確認日期、未覆問題同下一步。')} /></label>
          <p className="setup-hint">{workspace.settings.processing_mode === 'local' ? text('Analysis uses the local model. If the group is too long, reduce its size in the mail workspace.', '分析用本機模型。群組太長時，可喺電郵工作區縮細範圍。') : text(`Analysis sends this group to ${workspace.settings.processing_mode === 'openai' ? 'OpenAI' : 'AWS Bedrock'} using your account.`, `分析會用你嘅帳戶將呢組內容傳送到 ${workspace.settings.processing_mode === 'openai' ? 'OpenAI' : 'AWS Bedrock'}。`)}</p>
          {readiness !== 'ready' && <p>{text('Return to step 1 to test the selected AI before analyzing.', '分析前，請返回第 1 步測試所選 AI。')}</p>}
          <button type="button" className="setup-primary" disabled={!canAnalyze || !groupName.trim()} onClick={() => void firstAnalysis()}>{text('Create group and analyze', '建立群組並分析')}</button>
          {analysis && <article className="setup-result"><h2>{text('Open a source to check the result', '開原信核對結果')}</h2><pre>{analysis.text}</pre><p>{text('Messages omitted', '未納入郵件')}: {analysis.omitted_messages}</p><div className="setup-actions">{analysis.source_ids.map(id => <button type="button" key={id} onClick={() => void run(text('Opening source', '開啟原信'), async () => { setSource(await mail.mailGetMessage(id)); })}>{text('Source', '原信')} #{id}</button>)}</div>{source && <section><h3>{source.subject}</h3><pre>{source.body || source.snippet}</pre></section>}</article>}
        </>}
        <footer className="setup-footer"><button type="button" disabled={step === 1 || localBusy} onClick={() => goStep(step - 1)}>{text('Back', '上一步')}</button>{step < 3 && <button type="button" disabled={localBusy} onClick={() => goStep(step + 1)}>{text('Continue', '繼續')}</button>}<button type="button" disabled={localBusy} onClick={finish}>{step === 3 ? text('Open mail workspace', '開啟電郵工作區') : text('Set up later', '稍後繼續設定')}</button></footer>
      </fieldset>}
    </main>
  </div>;
}
