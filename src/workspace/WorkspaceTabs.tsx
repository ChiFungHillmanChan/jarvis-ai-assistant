import { Plus, X, Pin, PanelLeftOpen } from 'lucide-react';
import { useWorkspace } from './WorkspaceContext';
export default function WorkspaceTabs({ sidebarVisible, onShowSidebar }: { sidebarVisible: boolean; onShowSidebar: () => void }) {
  const { state, dispatch, newTask } = useWorkspace();
  return <div className="workspace-tabbar">
    {!sidebarVisible && <button className="icon-button" onClick={onShowSidebar} aria-label="Show sidebar"><PanelLeftOpen size={17} /></button>}
    <div className="workspace-tabs" role="tablist" aria-label="Open work" onKeyDown={e => {
      if (!(e.target instanceof HTMLElement) || e.target.getAttribute('role') !== 'tab') return;
      const i = state.tabs.findIndex(t => t.id === state.activeId);
      const next = e.key === 'ArrowRight' ? (i + 1) % state.tabs.length : e.key === 'ArrowLeft' ? (i - 1 + state.tabs.length) % state.tabs.length : e.key === 'Home' ? 0 : e.key === 'End' ? state.tabs.length - 1 : -1;
      if (next < 0) return;
      e.preventDefault(); const id = state.tabs[next].id; dispatch({ type: 'activate', id });
      requestAnimationFrame(() => document.getElementById(`tab-${id}`)?.focus());
    }}>{state.tabs.map(tab => <div className={`workspace-tab ${tab.id === state.activeId ? 'is-active' : ''}`} key={tab.id}>
      <button id={`tab-${tab.id}`} role="tab" aria-selected={tab.id === state.activeId} aria-controls={`page-${tab.id}`} tabIndex={tab.id === state.activeId ? 0 : -1} onClick={() => dispatch({ type: 'activate', id: tab.id })} onDoubleClick={() => dispatch({ type: 'pin', id: tab.id })} title={`${tab.title}${tab.pinned ? ' (pinned)' : ' — double-click to pin'}`}>{tab.pinned && <Pin size={12} />}<span>{tab.title}</span>{tab.notice && <span className="status-dot" aria-label="New result" />}</button>
      {tab.view !== 'home' && <button className="tab-close" aria-label={`Close ${tab.title}`} onClick={() => dispatch({ type: 'close', id: tab.id })}><X size={13} /></button>}
    </div>)}</div><button className="icon-button" aria-label="New task tab" onClick={newTask}><Plus size={17} /></button>
  </div>;
}
