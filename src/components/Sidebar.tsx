import { Orbit, Mail, CalendarDays, NotebookPen, GitPullRequest, Clock3, Settings2, Plug, Plus, PanelLeftClose, Search, MessageSquare } from 'lucide-react';
import { useWorkspace } from '../workspace/WorkspaceContext';
import type { WorkspaceView } from '../workspace/model';
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
  const active = state.tabs.find(t => t.id === state.activeId)?.view;
  const nav = (view: WorkspaceView) => open(view);
  return <aside className="workspace-sidebar" aria-label="App navigation">
    <div className="sidebar-brand"><span className="brand-mark"><Orbit size={20} /></span><strong>JARVIS</strong><button className="icon-button" onClick={onCollapse} aria-label="Hide sidebar"><PanelLeftClose size={16} /></button></div>
    <button className="workspace-button sidebar-new" onClick={newTask}><Plus size={16} />New task<span className="shortcut">⌘N</span></button>
    <button className="sidebar-search" onClick={onSearch}><Search size={15} />Search and commands<span className="shortcut">⌘K</span></button>
    <nav>{destinations.map(({ view, label, Icon }) => <button key={view} className="sidebar-item" aria-current={active === view ? 'page' : undefined} onClick={() => nav(view)}><Icon size={18} /><span>{label}</span></button>)}</nav>
    <div className="sidebar-section-title">Recent tasks</div>
    <div className="sidebar-history">{[...state.tabs.slice().reverse(), ...state.history].filter(t => t.view === 'chat').slice(0, 8).map(t => <button key={t.id} className="sidebar-item" aria-current={state.activeId === t.id ? 'page' : undefined} onClick={() => dispatch({ type: 'open', view: t.view, id: t.id, sessionId: t.sessionId, title: t.title })}><MessageSquare size={15} /><span>{t.title}</span>{t.notice && <span className="status-dot" aria-label="New result" />}</button>)}{![...state.tabs, ...state.history].some(t => t.view === 'chat') && <p>Tasks you start will appear here.</p>}</div>
    <div className="sidebar-bottom"><button className="sidebar-item" aria-current={active === 'connections' ? 'page' : undefined} onClick={() => nav('connections')}><Plug size={18} /><span>Connections</span></button><button className="sidebar-item" aria-current={active === 'settings' ? 'page' : undefined} onClick={() => nav('settings')}><Settings2 size={18} /><span>Settings</span></button></div>
  </aside>;
}
