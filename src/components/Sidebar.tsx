import { Orbit, Mail, CalendarDays, NotebookPen, GitPullRequest, Clock3, Settings2, Plug, Plus, PanelLeftClose, Search, MessageSquare } from 'lucide-react';
import { useWorkspace } from '../workspace/WorkspaceContext';
import { displayTabTitle, viewName, type WorkspaceView } from '../workspace/model';
import { useUiLocale } from '../i18n';
const destinations = [
  { view: 'home', label: 'Automation', Icon: Orbit },
  { view: 'email', label: 'Mail', Icon: Mail },
  { view: 'calendar', label: 'Calendar', Icon: CalendarDays },
  { view: 'notion', label: 'Notes', Icon: NotebookPen },
  { view: 'github', label: 'GitHub', Icon: GitPullRequest },
  { view: 'cron', label: 'Automations', Icon: Clock3 },
] as const;
export default function Sidebar({ onSearch, onCollapse }: { onSearch: () => void; onCollapse: () => void }) {
  const { state, open, newTask, dispatch } = useWorkspace();
  const { locale, t } = useUiLocale();
  const active = state.tabs.find(t => t.id === state.activeId)?.view;
  const nav = (view: WorkspaceView) => open(view);
  return <aside className="workspace-sidebar" aria-label={t("appNavigation")}>
    <div className="sidebar-brand"><span className="brand-mark"><Orbit size={20} /></span><strong>JARVIS</strong><button className="icon-button" onClick={onCollapse} aria-label={t("hideSidebar")}><PanelLeftClose size={16} /></button></div>
    <button className="workspace-button sidebar-new" onClick={newTask}><Plus size={16} />{t("newTask")}<span className="shortcut">⌘N</span></button>
    <button className="sidebar-search" onClick={onSearch}><Search size={15} />{t("searchCommands")}<span className="shortcut">⌘K</span></button>
    <nav>{destinations.map(({ view, Icon }) => <button key={view} className="sidebar-item" aria-current={active === view ? 'page' : undefined} onClick={() => nav(view)}><Icon size={18} /><span>{viewName(view, locale)}</span></button>)}</nav>
    <div className="sidebar-section-title">{t("recentTasks")}</div>
    <div className="sidebar-history">{[...state.tabs.slice().reverse(), ...state.history].filter(t => t.view === 'chat').slice(0, 8).map(tab => <button key={tab.id} className="sidebar-item" aria-current={state.activeId === tab.id ? 'page' : undefined} onClick={() => dispatch({ type: 'open', view: tab.view, id: tab.id, sessionId: tab.sessionId, title: tab.title })}><MessageSquare size={15} /><span>{displayTabTitle(tab, locale)}</span>{tab.notice && <span className="status-dot" aria-label={t("newResult")} />}</button>)}{![...state.tabs, ...state.history].some(t => t.view === 'chat') && <p>{t("tasksAppear")}</p>}</div>
    <div className="sidebar-bottom"><button className="sidebar-item" aria-current={active === 'connections' ? 'page' : undefined} onClick={() => nav('connections')}><Plug size={18} /><span>{t("navConnections")}</span></button><button className="sidebar-item" aria-current={active === 'settings' ? 'page' : undefined} onClick={() => nav('settings')}><Settings2 size={18} /><span>{t("navSettings")}</span></button></div>
  </aside>;
}
