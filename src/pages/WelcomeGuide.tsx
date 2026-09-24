import { useEffect, useRef, useState } from 'react';
import { useUiLocale, type UiLocale } from '../i18n';
import { markOnboardingComplete, setPreference } from '../lib/commands';
import WindowControls from '../components/WindowControls';
import '../styles/setup.css';

export default function WelcomeGuide({ onComplete }: { onComplete: (next: 'setup' | 'workspace') => void }) {
  const { locale, hasSavedChoice, setLocale, text } = useUiLocale();
  const [stage, setStage] = useState<'language' | 'guide'>(hasSavedChoice ? 'guide' : 'language');
  const [selected, setSelected] = useState<UiLocale>(locale);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const operation = useRef(false);

  useEffect(() => {
    if (stage !== 'language') return;
    const element = dialog.current;
    element?.showModal();
    return () => { element?.close(); };
  }, [stage]);

  async function confirmLanguage() {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError(false);
    if (await setLocale(selected)) setStage('guide');
    else setError(true);
    operation.current = false; setBusy(false);
  }
  async function finish(next: 'setup' | 'workspace') {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError(false);
    try {
      await setPreference('onboarding_intro_complete', 'true');
      if (next === 'workspace') await markOnboardingComplete();
      onComplete(next);
    } catch { setError(true); }
    finally { operation.current = false; setBusy(false); }
  }

  return <div className="setup-screen">
    <header className="setup-titlebar" data-tauri-drag-region><WindowControls /><span data-tauri-drag-region>JARVIS</span></header>
    {stage === 'language' ? <>
      <main className="setup-content"><h1>Welcome to JARVIS · 歡迎使用 JARVIS</h1></main>
      <dialog ref={dialog} className="welcome-dialog" aria-labelledby="welcome-language-title" aria-describedby="welcome-language-description" onCancel={event => event.preventDefault()}>
        <div className="setup-content">
          <h1 id="welcome-language-title">Choose your language<br /><span lang="zh-HK">選擇介面語言</span></h1>
          <p id="welcome-language-description">Your quick start guide will use this language.<br /><span lang="zh-HK">接下來的使用說明會以所選語言顯示。</span></p>
          <fieldset className="welcome-languages" disabled={busy}>
            <legend>Language / 語言</legend>
            <label className="setup-checkbox" lang="en"><input autoFocus type="radio" name="welcome-language" value="en" checked={selected === 'en'} onChange={() => setSelected('en')} />English</label>
            <label className="setup-checkbox" lang="zh-HK"><input type="radio" name="welcome-language" value="zh-HK" checked={selected === 'zh-HK'} onChange={() => setSelected('zh-HK')} />繁體中文（香港）</label>
          </fieldset>
          {error && <p className="setup-error" role="alert">Could not save your language. Please try again.<br />未能儲存語言選擇，請重試。</p>}
          <button className="setup-primary" disabled={busy} onClick={() => void confirmLanguage()}>{busy ? (selected === 'en' ? 'Saving…' : '正在儲存…') : (selected === 'en' ? 'Continue in English' : '以繁體中文繼續')}</button>
          <p className="setup-hint">You can change this in Settings later.<br /><span lang="zh-HK">稍後可在「設定」變更。助理回覆及語音辨識有獨立的語言設定。</span></p>
        </div>
      </dialog>
    </> : <main className="setup-content setup-guide">
      <span className="setup-hint">{text('Quick start', '快速入門')}</span>
      <h1>{text('Make JARVIS your workspace', '開始使用 JARVIS 工作區')}</h1>
      <p>{text('Keep your tools, tasks and results together. Start with one workflow and add connections when you need them.', '在同一個工作區使用工具、處理工作及查看結果。你可以先開始一項工作，再按需要連接服務。')}</p>
      <ol className="welcome-steps">
        <li><h2>{text('Open a tool from the sidebar', '從側邊欄開啟工具')}</h2><p>{text('Mail, Calendar, Notes and GitHub open in their own tabs. Use Connections to connect the services you need.', '電郵、日曆、筆記及 GitHub 會在各自的分頁開啟。你可前往「連接服務」，連接需要使用的服務。')}</p></li>
        <li><h2>{text('Start a task with the assistant', '交代工作給 AI 助理')}</h2><p>{text('Choose New task and describe what you need. The Assistant button opens it beside your tools so you can check results while you work. Configure an AI provider in Settings first.', '選擇「新增工作」，說明你需要的協助。按「AI 助理」可在工具旁開啟對話，方便一邊工作，一邊核對結果。首次使用前，請在「設定」配置 AI 供應商。')}</p></li>
        <li><h2>{text('Organize mail into useful groups', '按需要整理郵件群組')}</h2><p>{text('Mail setup helps you choose and test an AI model, connect Gmail and sync the first batch. Group related messages, add instructions, then analyze the group. Open cited messages to check the result; save useful memories only after reviewing them.', '電郵設定導引會協助你選擇及測試 AI 模型、連接 Gmail 並同步首批郵件。你可將相關郵件加入群組，填寫整理指示，再分析群組。請開啟引用的原信核對結果，並在確認後儲存有用的記憶。')}</p></li>
        <li><h2>{text('Return to your work', '隨時返回工作')}</h2><p>{text('Use the tabs to switch tools and tasks. Press ⌘K to find a workspace or saved task, and ⌘N to start a new task. Reopen mail setup with Set up at the top of the window.', '使用分頁切換工具及工作。按 ⌘K 搜尋工作區或已儲存的工作，按 ⌘N 開始新工作。需要時，可按視窗頂部的「設定導引」重新開啟電郵導引。')}</p></li>
      </ol>
      <div className="setup-current"><h2>{text('Understand what leaves your Mac', '了解哪些資料會傳送至雲端')}</h2><p>{text('Your workspace is stored on this Mac. Cloud AI sends the content needed for a request to your selected provider; local mail AI requires a compatible installed model. Mail, assistant and voice settings are separate. Gmail sync downloads up to 50 messages per batch and is not a full mailbox backup.', '工作區資料儲存在此 Mac。使用雲端 AI 時，處理請求所需的內容會傳送至所選供應商；本機電郵 AI 則需要已安裝的相容模型。電郵、AI 助理及語音分別設定。Gmail 每批最多同步 50 封郵件，並非完整信箱備份。')}</p></div>
      {error && <p className="setup-error" role="alert">{text('Could not save your progress. Please try again.', '未能儲存導引進度，請重試。')}</p>}
      <footer className="setup-footer"><button className="setup-primary" disabled={busy} onClick={() => void finish('setup')}>{text('Set up mail', '設定電郵')}</button><button disabled={busy} onClick={() => void finish('workspace')}>{text('Explore the workspace', '先瀏覽工作區')}</button><button disabled={busy} onClick={() => { setSelected(locale); setError(false); setStage('language'); }}>{text('Change language', '變更語言')}</button></footer>
      {busy && <p role="status">{text('Saving your progress…', '正在儲存導引進度…')}</p>}
    </main>}
  </div>;
}
