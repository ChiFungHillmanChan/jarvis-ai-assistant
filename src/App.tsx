import { useState, useEffect, useRef, useCallback, lazy, Suspense } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { MessageSquare, Search, Minimize2, X, Command } from 'lucide-react';
import Sidebar from './components/Sidebar';
import OnboardingWizard from './pages/OnboardingWizard';
import UpdateToast from './components/UpdateToast';
import ErrorBoundary from './components/ErrorBoundary';
import { checkOnboardingStatus, stopSpeaking, listChatSessions } from './lib/commands';
import { useVoiceState } from './hooks/useVoiceState';
import ToastContainer, { showToast } from './components/Toast';
import WindowControls from './components/WindowControls';
import { ChatProvider, useChatContext } from './hooks/ChatContext';
import ModelPullToast from './components/ModelPullToast';
import type { ModelPullProgress } from './lib/types';
import { WorkspaceProvider, WorkspaceTabScope, useWorkspace } from './workspace/WorkspaceContext';
import WorkspaceTabs from './workspace/WorkspaceTabs';
import AssistantPanel from './workspace/AssistantPanel';
import AutomationOverview from './workspace/AutomationOverview';
import { useAssistantActivity } from './workspace/useAssistantActivity';
import { shouldFocusResult, viewName, displayTabTitle, type WorkspaceTab, type WorkspaceView } from './workspace/model';
import { resultIdentity, resultTitle, validResult } from './workspace/results';
import './styles/workspace.css';
import { useUiLocale } from './i18n';
const Settings = lazy(() => import('./pages/Settings'));
const CronDashboard = lazy(() => import('./pages/CronDashboard'));
const EmailPage = lazy(() => import('./pages/EmailPage'));
const CalendarPage = lazy(() => import('./pages/CalendarPage'));
const GitHubPage = lazy(() => import('./pages/GitHubPage'));
const NotionPage = lazy(() => import('./pages/NotionPage'));
const ConnectionsPage = lazy(() => import('./workspace/ConnectionsPage'));
const ResultWorkspace = lazy(() => import('./workspace/ResultWorkspace'));

function CommandPicker({ onClose }: { onClose: () => void }) {
  const { locale, text } = useUiLocale();
  const { state, dispatch, open, newTask } = useWorkspace();
  const [query, setQuery] = useState('');
  const [savedTasks, setSavedTasks] = useState<{ id: string; title: string }[]>([]);
  const [historyError, setHistoryError] = useState(false);
  useEffect(() => { void listChatSessions().then(setSavedTasks).catch(() => setHistoryError(true)); }, []);
  const [selected, setSelected] = useState(0);
  const dialog = useRef<HTMLDialogElement>(null);
  const restoreFocus = useRef(document.activeElement as HTMLElement | null);
  const choices = [
    { id: 'new', title: text('New task', '新增工作'), hint: '⌘N', action: () => newTask() },
    ...(['home', 'email', 'calendar', 'notion', 'github', 'cron', 'connections', 'settings'] as WorkspaceView[]).map(view => ({ id: view, title: viewName(view, locale), hint: text('Workspace', '工作區'), action: () => open(view) })),
    { id: 'legacy', title: text('Voice and earlier conversations', '語音及較早對話'), hint: text('History', '紀錄'), action: () => dispatch({ type: 'open' as const, view: 'chat', id: 'task:legacy', sessionId: 'legacy', title: 'Voice and earlier conversations' }) },
    ...savedTasks.filter(t => t.id !== 'legacy' && ![...state.tabs, ...state.history].some(tab => tab.sessionId === t.id && tab.view === 'chat')).map(t => ({ id: `saved:${t.id}`, title: t.title, hint: text('Saved task', '已儲存工作'), action: () => dispatch({ type: 'open' as const, view: 'chat', id: `task:${t.id}`, sessionId: t.id, title: t.title }) })),
    ...[...state.tabs, ...state.history].filter(t => t.view === 'chat' || t.view === 'result').map(t => ({ id: t.id, title: displayTabTitle(t, locale), hint: t.view === 'chat' ? text('Task', '工作') : text('Result', '結果'), action: () => dispatch({ type: 'open' as const, view: t.view, id: t.id, sessionId: t.sessionId, title: t.title }) })),
  ].filter(c => c.title.toLowerCase().includes(query.toLowerCase()));
  useEffect(() => { dialog.current?.showModal(); return () => restoreFocus.current?.focus(); }, []);
  function choose(index: number) { choices[index]?.action(); onClose(); }
  return <dialog className="command-picker" ref={dialog} aria-label={text("Search workspaces and tasks", "搜尋工作區與工作")} onCancel={onClose} onClick={e => { if (e.target === dialog.current) onClose(); }}>
    <div className="command-picker-inner"><div className="command-search"><Search size={19} /><input aria-label={text("Search workspaces and tasks", "搜尋工作區與工作")} placeholder={text("Find a workspace or task…", "搜尋工作區或工作…")} autoFocus value={query} onChange={e => { setQuery(e.target.value); setSelected(0); }} onKeyDown={e => { if (e.nativeEvent.isComposing) return; if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); setSelected(v => Math.max(0, Math.min(choices.length - 1, v + (e.key === 'ArrowDown' ? 1 : -1)))); } if (e.key === 'Enter') { e.preventDefault(); choose(selected); } }} /><button className="icon-button" aria-label={text("Close search", "關閉搜尋")} onClick={onClose}><X size={18} /></button></div>
    <div className="command-results">{historyError && <p className="workspace-error">{text("Saved task history could not be loaded.", "未能載入已儲存工作紀錄。")}</p>}{choices.map((choice, i) => <button key={choice.id} className={i === selected ? 'is-selected' : ''} onMouseMove={() => setSelected(i)} onClick={() => choose(i)}><span>{choice.title}</span><small>{choice.hint}</small></button>)}{!choices.length && <p className="workspace-empty">{text("No matching workspaces or tasks.", "找不到符合條件的工作區或工作。")}</p>}</div><footer>{text("↑ ↓ to move · Enter to open · Esc to close", "↑ ↓ 選擇 · Enter 開啟 · Esc 關閉")}</footer></div>
  </dialog>;
}

function AppShell() {
  const { text } = useUiLocale();
  const [setupOpen, setSetupOpen] = useState(false);
  const { state, ready, saveError, dispatch, writeValue, readValue } = useWorkspace();
  const chat = useChatContext();
  const activity = useAssistantActivity();
  const [sidebar, setSidebar] = useState(true);
  const [search, setSearch] = useState(false);
  const [assistant, setAssistant] = useState(false);
  const [wallpaper, setWallpaper] = useState(false);
  const [raised, setRaised] = useState(false);
  const [progress, setProgress] = useState<ModelPullProgress | null>(null);
  const voice = useVoiceState();
  const activeTab = state.tabs.find(t => t.id === state.activeId) || state.tabs[0];
  const stateRef = useRef(state); stateRef.current = state;
  const seenResults = useRef(new Set<string>());
  const focusedRuns = useRef(new Set<string>());
  const listening = voice.state === 'Listening' || voice.state === 'WakeWordListening';
  useEffect(() => { if ((activeTab.view === 'chat' || activeTab.view === 'result') && activeTab.sessionId) chat.selectSession(activeTab.sessionId); }, [activeTab.id, activeTab.view, activeTab.sessionId, chat.selectSession]);
  useEffect(() => { if (voice.error) showToast(voice.error, 'error'); }, [voice.error]);
  const toggleVoice = useCallback(() => {
    if (voice.state === 'Listening') void voice.stopVoice();
    else if (!chat.busy && (voice.state === 'Idle' || voice.state === 'WakeWordListening' || typeof voice.state === 'object' && 'Error' in voice.state)) { chat.selectSession('legacy'); dispatch({ type: 'open', view: 'chat', id: 'task:legacy', sessionId: 'legacy', title: 'Voice and earlier conversations', background: stateRef.current.tabs.find(t => t.id === stateRef.current.activeId)?.view !== 'chat' }); setAssistant(true); void voice.startVoice(); }
  }, [voice.state, voice.startVoice, voice.stopVoice, chat.busy, chat.selectSession, dispatch]);
  useEffect(() => {
    const listener = listen<unknown>('chat-tool-result', ({ payload }) => {
      if (!validResult(payload) || !chat.acceptsRun(payload)) return;
      const key = `${payload.run_id}:${payload.tool_call_id}`;
      if (seenResults.current.has(key)) return;
      seenResults.current.add(key);
      const current = stateRef.current;
      const originId = `task:${payload.session_id}`;
      const currentView = current.tabs.find(t => t.id === current.activeId)?.view || 'home';
      const focus = !focusedRuns.current.has(payload.run_id) && shouldFocusResult(current.activeId, originId, currentView);
      if (focus) focusedRuns.current.add(payload.run_id);
      const matching = [...current.tabs, ...current.history].find(tab => {
        if (tab.view !== 'result') return false;
        const previous = readValue(tab.id, 'toolResult', null);
        return validResult(previous) && resultIdentity(previous) === resultIdentity(payload);
      });
      const id = matching?.id || `result:${crypto.randomUUID()}`;
      writeValue(id, 'toolResult', payload);
      dispatch({ type: 'open', id, view: 'result', title: resultTitle(payload), sessionId: payload.session_id, background: !focus });
      dispatch({ type: 'notice', id: originId });
      if (focus) setAssistant(true);
    });
    return () => { void listener.then(fn => fn()).catch(() => {}); };
  }, [chat.acceptsRun, dispatch, writeValue, readValue]);
  useEffect(() => {
    void invoke<boolean>('get_wallpaper_status').then(setWallpaper).catch(() => {});
    void invoke<boolean>('is_wallpaper_raised').then(setRaised).catch(() => {});
    let timer: ReturnType<typeof setTimeout>;
    const listeners = [
      listen<boolean>('wallpaper-status', e => setWallpaper(e.payload)),
      listen<boolean>('wallpaper-raised', e => setRaised(e.payload)),
      listen<ModelPullProgress>('model-pull-progress', ({ payload }) => { clearTimeout(timer); setProgress(payload); if (payload.status === 'complete' || payload.status === 'error') timer = setTimeout(() => setProgress(null), 3000); }),
    ];
    return () => { clearTimeout(timer); listeners.forEach(p => { void p.then(fn => fn()).catch(() => {}); }); };
  }, []);
  useEffect(() => {
    function key(e: KeyboardEvent) {
      if (document.querySelector('dialog[open]')) return;
      const command = e.metaKey || e.ctrlKey;
      if (e.key === 'Escape') { if (activity.state === 'speaking') void stopSpeaking(); else if (assistant) setAssistant(false); else if (wallpaper && raised) void invoke('lower_wallpaper'); return; }
      if (command && e.key.toLowerCase() === 'k') { e.preventDefault(); setSearch(true); }
      else if (command && e.key.toLowerCase() === 'n') { e.preventDefault(); const sessionId = crypto.randomUUID(); dispatch({ type: 'open', view: 'chat', id: `task:${sessionId}`, sessionId, title: 'New task' }); }
      else if (command && e.key.toLowerCase() === 'w') { e.preventDefault(); dispatch({ type: 'close', id: state.activeId }); }
      else if (command && /^[1-9]$/.test(e.key)) { e.preventDefault(); const tab = state.tabs[Number(e.key) - 1]; if (tab) dispatch({ type: 'activate', id: tab.id }); }
      else if (e.ctrlKey && e.key === 'Tab') { e.preventDefault(); const i = state.tabs.findIndex(t => t.id === state.activeId); const next = (i + (e.shiftKey ? -1 : 1) + state.tabs.length) % state.tabs.length; dispatch({ type: 'activate', id: state.tabs[next].id }); }
      else if (command && e.shiftKey && e.key.toLowerCase() === 'j') { e.preventDefault(); toggleVoice(); }
    }
    window.addEventListener('keydown', key); return () => window.removeEventListener('keydown', key);
  }, [state, dispatch, toggleVoice, activity.state, assistant, wallpaper, raised]);
  function render(tab: WorkspaceTab) {
    switch (tab.view) {
      case 'home': return <AutomationOverview activity={activity} onVoice={toggleVoice} listening={listening} />;
      case 'chat': return chat.sessionId === tab.sessionId ? <AssistantPanel full onVoice={toggleVoice} /> : <div className="workspace-loading">{text("Opening task…", "正在開啟工作…")}</div>;
      case 'email': return <EmailPage onSetup={() => setSetupOpen(true)} />;
      case 'calendar': return <CalendarPage />;
      case 'github': return <GitHubPage />;
      case 'notion': return <NotionPage />;
      case 'cron': return <CronDashboard />;
      case 'settings': return <Settings />;
      case 'connections': return <ConnectionsPage />;
      case 'result': return <ResultWorkspace />;
    }
  }
  if (setupOpen) return <OnboardingWizard onComplete={() => { setSetupOpen(false); dispatch({ type: 'open', view: 'email' }); }} />;
  if (!ready) return <div className="workspace-loading" role="status">{text("Restoring your workspace…", "正在還原工作區…")}</div>;
  return <div className="workspace-app">
    <header className="workspace-titlebar" data-tauri-drag-region><WindowControls /><span data-tauri-drag-region>{text("JARVIS Workspace", "JARVIS 工作區")}</span><div><button className="workspace-button" onClick={() => setSetupOpen(true)}>{text('Set up', '設定導引')}</button>{wallpaper && raised && <button className="icon-button" aria-label={text("Return to desktop", "返回桌面")} onClick={() => void invoke('lower_wallpaper')}><Minimize2 size={15} /></button>}<button className="icon-button" aria-label={text("Search workspaces", "搜尋工作區")} onClick={() => setSearch(true)}><Search size={16} /></button><button className="workspace-button assistant-toggle" aria-pressed={assistant || activeTab.view === 'chat'} disabled={activeTab.view === 'chat'} onClick={() => setAssistant(v => !v)}><MessageSquare size={15} />{text('Assistant', 'AI 助理')}</button></div></header>
    <div className="workspace-body">{sidebar && <Sidebar onSearch={() => setSearch(true)} onCollapse={() => setSidebar(false)} />}<main className="workspace-main"><WorkspaceTabs sidebarVisible={sidebar} onShowSidebar={() => setSidebar(true)} />{saveError && <p className="workspace-error" role="alert">{saveError}</p>}<div className="workspace-content"><div className="workspace-pages">{state.tabs.map(tab => {
      const active = tab.id === state.activeId;
      // The sphere and task composer exist only in their active view. Tool work stays mounted.
      if ((tab.view === 'home' || tab.view === 'chat') && !active) return null;
      return <section className={`workspace-page view-${tab.view}`} key={tab.id} id={`page-${tab.id}`} role="tabpanel" aria-labelledby={`tab-${tab.id}`} hidden={!active}><WorkspaceTabScope id={tab.id}><ErrorBoundary><Suspense fallback={<div className="workspace-loading">{text("Opening workspace…", "正在開啟工作區…")}</div>}>{render(tab)}</Suspense></ErrorBoundary></WorkspaceTabScope></section>;
    })}</div>{assistant && activeTab.view !== 'chat' && <div className="workspace-assistant-dock"><WorkspaceTabScope key={chat.sessionId} id={`task:${chat.sessionId}`}><AssistantPanel onClose={() => setAssistant(false)} onVoice={toggleVoice} /></WorkspaceTabScope></div>}</div><footer className="workspace-statusbar"><span className={`status-dot ${chat.busy || listening ? 'is-working' : ''}`} /><span>{listening ? text('Listening · Press ⌘⇧J to finish', '聆聽中 · 按 ⌘⇧J 完成') : chat.busy ? text('Assistant working · You can keep using your tools', 'AI 助理處理中 · 可繼續使用其他工具') : text('Ready', '就緒')}</span>{chat.busy && <button className="text-button" onClick={() => dispatch({ type: 'open', view: 'chat', id: `task:${chat.runningSessionId}`, sessionId: chat.runningSessionId!, title: 'Assistant' })}>{text("View task", "查看工作")}</button>}<span className="grow" /><button className="text-button" onClick={() => setSearch(true)}><Command size={12} />{text("K Search", "K 搜尋")}</button></footer></main></div>
    {search && <CommandPicker onClose={() => setSearch(false)} />}<ToastContainer /><ModelPullToast progress={progress} visible={progress !== null} /><UpdateToast />
  </div>;
}
function AppGate() {
  const { text } = useUiLocale();
  const { open } = useWorkspace();
  const [onboarding, setOnboarding] = useState<boolean | null>(null);
  useEffect(() => { void checkOnboardingStatus().then(setOnboarding).catch(() => setOnboarding(true)); }, []);
  if (onboarding === null) return <div className="workspace-loading" role="status">{text("Opening JARVIS…", "正在開啟 JARVIS…")}</div>;
  if (!onboarding) return <OnboardingWizard onComplete={() => { setOnboarding(true); open('email'); }} />;
  return <AppShell />;
}

export default function App() { return <WorkspaceProvider><ChatProvider><AppGate /></ChatProvider></WorkspaceProvider>; }
