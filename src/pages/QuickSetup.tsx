import { useEffect, useState } from 'react';
import { useUiLocale } from '../i18n';
import ChooseAi from '../components/ChooseAi';
import GoogleConnectSteps from '../components/GoogleConnectSteps';
import WindowControls from '../components/WindowControls';
import { currentAi } from '../lib/ai-choice';
import { googleAccounts, googleCancelConnect, googleConnect, markOnboardingComplete, type GoogleAccounts } from '../lib/commands';
import '../styles/setup.css';

/**
 * First run in three steps: choose the AI, connect Google Calendar, then ask
 * about the day. Mail setup stays one click away for people who want it.
 */
export default function QuickSetup({ onDone, onMailSetup }: { onDone: () => void; onMailSetup: () => void }) {
  const { text } = useUiLocale();
  const [step, setStep] = useState(1);
  const [aiReady, setAiReady] = useState(false);
  const [aiBusy, setAiBusy] = useState(false);
  const [google, setGoogle] = useState<GoogleAccounts['main'] | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [finishing, setFinishing] = useState(false);
  const [error, setError] = useState('');

  const refreshGoogle = () => googleAccounts().then(accounts => setGoogle(accounts.main)).catch(() => setGoogle(null));
  useEffect(() => {
    void currentAi().then(ai => setAiReady(!!ai)).catch(() => {});
    void refreshGoogle();
  }, []);

  const connected = google?.state === 'connected';
  const calendarReady = connected && google.calendar;
  const busy = aiBusy || connecting || finishing;

  async function connect() {
    setConnecting(true); setError('');
    try { await googleConnect(false); }
    catch (e) { setError(String(e)); }
    finally { await refreshGoogle(); setConnecting(false); }
  }
  async function finish() {
    setFinishing(true); setError('');
    try { await markOnboardingComplete(); onDone(); }
    catch { setError(text('Could not save your progress. Please try again.', '未能儲存導引進度，請重試。')); setFinishing(false); }
  }
  const go = (next: number) => { setError(''); setStep(next); };

  return <div className="setup-screen">
    <header className="setup-titlebar" data-tauri-drag-region><WindowControls /><span data-tauri-drag-region>JARVIS</span></header>
    <main className="setup-content">
      <div className="setup-topline"><span>{text(`Step ${step} of 3`, `第 ${step}／3 步`)}</span></div>
      {step === 1 && <>
        <h1>{text('Choose your AI', '選擇你的 AI')}</h1>
        <p>{text('JARVIS uses the AI you already have. Paste its key, tick the box and choose Test and use this AI.', 'JARVIS 會使用你已有的 AI。請貼上密鑰，剔選方格，再按「測試並使用這個 AI」。')}</p>
        <ChooseAi onChosen={() => setAiReady(true)} onBusyChange={setAiBusy} />
        {aiReady && <p className="setup-notice">{text('Your AI is ready. Choose Continue.', '你的 AI 已準備好，請按「繼續」。')}</p>}
      </>}
      {step === 2 && <>
        <h1>{text('Connect Google Calendar', '連接 Google 日曆')}</h1>
        <p>{text('JARVIS reads your events to answer questions about your day, and adds, moves or cancels events only when you ask and approve. It does not ask for Gmail here.', 'JARVIS 會讀取你的活動，以回答有關你日程的問題；只會在你要求並批准後新增、更改或取消活動。這一步不會要求 Gmail 權限。')}</p>
        {calendarReady
          ? <p className="setup-notice">{text(`Connected${google.email ? ` as ${google.email}` : ''}. Your calendar is syncing.`, `已連接${google.email ? `（${google.email}）` : ''}，日曆正在同步。`)}</p>
          : <>
            <div className="setup-current"><GoogleConnectSteps style={{ margin: 0 }} /></div>
            {connected && <p className="setup-error" role="alert">{text('Calendar access was not allowed, so JARVIS cannot see your calendar. Choose Allow Calendar and tick the calendar box on Google\'s page.', '未授權日曆，JARVIS 無法讀取你的日曆。請按「允許日曆」，並在 Google 頁面剔選日曆方格。')}</p>}
            <div className="setup-actions">
              <button type="button" className="setup-primary" disabled={busy} onClick={() => void connect()}>{connecting ? text('Waiting for Google…', '正在等候 Google…') : connected ? text('Allow Calendar', '允許日曆') : text('Connect Google', '連接 Google')}</button>
              {connecting && <button type="button" onClick={() => void googleCancelConnect().catch(() => {})}>{text('Cancel', '取消')}</button>}
            </div>
            {connecting && <p className="setup-hint">{text('Finish in your browser. JARVIS comes back to the front when Google is done.', '請在瀏覽器完成。Google 完成後，JARVIS 會自動回到前景。')}</p>}
          </>}
      </>}
      {step === 3 && <>
        <h1>{text('Ask about your day', '問問你的日程')}</h1>
        <p>{text('Choose Assistant at the top right of the workspace and try:', '在工作區右上角按「AI 助理」，試試輸入：')}</p>
        <ul className="welcome-steps">
          <li>{text('What\'s on my calendar tomorrow?', '我明天有甚麼安排？')}</li>
          <li>{text('Create a meeting with Alex tomorrow at 3pm', '明天下午三時和 Alex 開會')}</li>
          <li>{text('Move it to 4pm', '改到下午四時')}</li>
        </ul>
        <p className="setup-hint">{text('JARVIS asks before it changes anything on your calendar. Mail can be added later in Settings.', 'JARVIS 更改日曆前一定會先問你。電郵功能之後可在「設定」加入。')}</p>
      </>}
      {error && <p className="setup-error" role="alert">{error}</p>}
      <footer className="setup-footer">
        <button type="button" disabled={step === 1 || busy} onClick={() => go(step - 1)}>{text('Back', '上一步')}</button>
        {step === 1 && <button type="button" className={aiReady ? 'setup-primary' : undefined} disabled={busy} onClick={() => go(2)}>{aiReady ? text('Continue', '繼續') : text('Skip for now', '暫時略過')}</button>}
        {step === 2 && <button type="button" className={calendarReady ? 'setup-primary' : undefined} disabled={busy} onClick={() => go(3)}>{calendarReady ? text('Continue', '繼續') : text('Skip for now', '暫時略過')}</button>}
        {step === 3 && <button type="button" className="setup-primary" disabled={busy} onClick={() => void finish()}>{text('Open my calendar', '開啟我的日曆')}</button>}
        <button type="button" disabled={busy} onClick={onMailSetup}>{text('Set up Mail instead', '改為設定電郵')}</button>
      </footer>
    </main>
  </div>;
}
