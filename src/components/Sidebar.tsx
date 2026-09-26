import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Orbit, Mail, CalendarDays, NotebookPen, GitPullRequest, Clock3, Plus, PanelLeftClose, Search, MessageSquare, Trash2 } from 'lucide-react';
import { useWorkspace } from '../workspace/WorkspaceContext';
import { displayTabTitle, recentChats, viewName, type SavedChat, type WorkspaceView } from '../workspace/model';
import { useChatContext } from '../hooks/ChatContext';
import { clearConversations, listChatSessions } from '../lib/commands';
import { showToast } from './Toast';
import { useUiLocale } from '../i18n';
import SidebarAccount from './SidebarAccount';
const destinations = [
  { view: 'home', label: 'Automation', Icon: Orbit },
  { view: 'email', label: 'Mail', Icon: Mail },
  { view: 'calendar', label: 'Calendar', Icon: CalendarDays },
  { view: 'notion', label: 'Notes', Icon: NotebookPen },
  { view: 'github', label: 'GitHub', Icon: GitPullRequest },
  { view: 'cron', label: 'Automations', Icon: Clock3 },
] as const;
export default function Sidebar({ onSearch, onCollapse }: { onSearch: () => void; onCollapse: () => void }) {
  const { state, open, newTask, dispatch, forgetSession } = useWorkspace();
  const chat = useChatContext();
  const { locale, t } = useUiLocale();
  const [saved, setSaved] = useState<SavedChat[] | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const active = state.tabs.find(t => t.id === state.activeId)?.view;
  const nav = (view: WorkspaceView) => open(view);
  const reload = useCallback(() => { void listChatSessions().then(setSaved).catch(() => {}); }, []);
  // A typed run starting or finishing, and each voice turn, can add a conversation.
  useEffect(reload, [reload, chat.runningSessionId]);
  useEffect(() => { const off = listen('chat-new-message', reload); return () => { void off.then(fn => fn()).catch(() => {}); }; }, [reload]);
  async function remove(sessionId: string) {
    setDeleting(true);
    try {
      await clearConversations(sessionId);
      forgetSession(sessionId);
      chat.forget(sessionId);
      setSaved(list => list && list.filter(s => s.id !== sessionId));
      showToast(t('conversationDeleted'), 'success');
    } catch (e) { showToast(String(e), 'error'); }
    finally { setDeleting(false); setConfirming(null); }
  }
  const chats = recentChats(state, saved).slice(0, 30);
  return <aside className="workspace-sidebar" aria-label={t("appNavigation")}>
    <div className="sidebar-brand"><span className="brand-mark"><Orbit size={20} /></span><strong>JARVIS</strong><button className="icon-button" onClick={onCollapse} aria-label={t("hideSidebar")}><PanelLeftClose size={16} /></button></div>
    <button className="workspace-button sidebar-new" onClick={newTask}><Plus size={16} />{t("newTask")}<span className="shortcut">⌘N</span></button>
    <button className="sidebar-search" onClick={onSearch}><Search size={15} />{t("searchCommands")}<span className="shortcut">⌘K</span></button>
    <nav>{destinations.map(({ view, Icon }) => <button key={view} className="sidebar-item" aria-current={active === view ? 'page' : undefined} onClick={() => nav(view)}><Icon size={18} /><span>{viewName(view, locale)}</span></button>)}</nav>
    <div className="sidebar-section-title">{t("recentTasks")}</div>
    <div className="sidebar-history">{chats.map(tab => confirming === tab.sessionId
      ? <div key={tab.id} className="sidebar-confirm" role="group" aria-label={t('deleteConversation')}><strong>{t('deleteConversationConfirm')}</strong><span>{displayTabTitle(tab, locale)}</span><small>{t('deleteConversationHint')}</small><div><button className="text-button is-danger" disabled={deleting} onClick={() => void remove(tab.sessionId!)}>{t('deleteConfirm')}</button><button className="text-button" disabled={deleting} autoFocus onClick={() => setConfirming(null)}>{t('deleteCancel')}</button></div></div>
      : <div key={tab.id} className="sidebar-chat"><button className="sidebar-item" aria-current={state.activeId === tab.id ? 'page' : undefined} onClick={() => dispatch({ type: 'open', view: tab.view, id: tab.id, sessionId: tab.sessionId, title: tab.title })}><MessageSquare size={15} /><span>{displayTabTitle(tab, locale)}</span>{tab.notice && <span className="status-dot" aria-label={t("newResult")} />}</button><button className="icon-button sidebar-delete" aria-label={`${t('deleteConversation')}: ${displayTabTitle(tab, locale)}`} title={chat.busy ? t('deleteWhileBusy') : t('deleteConversation')} disabled={chat.busy} onClick={() => setConfirming(tab.sessionId!)}><Trash2 size={14} /></button></div>)}{!chats.length && <p>{t("tasksAppear")}</p>}</div>
    <div className="sidebar-bottom"><SidebarAccount active={active} onNavigate={nav} /></div>
  </aside>;
}
