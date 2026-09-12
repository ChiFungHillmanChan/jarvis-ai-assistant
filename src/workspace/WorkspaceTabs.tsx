import { Plus, X, Pin, PanelLeftOpen } from 'lucide-react';
import { useWorkspace } from './WorkspaceContext';
import { displayTabTitle } from './model';
import { useUiLocale } from '../i18n';
export default function WorkspaceTabs({ sidebarVisible, onShowSidebar }: { sidebarVisible: boolean; onShowSidebar: () => void }) {
  const { state, dispatch, newTask } = useWorkspace();
  const { locale, t } = useUiLocale();
  return <div className="workspace-tabbar">
    {!sidebarVisible && <button className="icon-button" onClick={onShowSidebar} aria-label={t("showSidebar")}><PanelLeftOpen size={17} /></button>}
    <div className="workspace-tabs" role="tablist" aria-label={t("openWork")} onKeyDown={e => {
      if (!(e.target instanceof HTMLElement) || e.target.getAttribute('role') !== 'tab') return;
      const i = state.tabs.findIndex(t => t.id === state.activeId);
      const next = e.key === 'ArrowRight' ? (i + 1) % state.tabs.length : e.key === 'ArrowLeft' ? (i - 1 + state.tabs.length) % state.tabs.length : e.key === 'Home' ? 0 : e.key === 'End' ? state.tabs.length - 1 : -1;
      if (next < 0) return;
      e.preventDefault(); const id = state.tabs[next].id; dispatch({ type: 'activate', id });
      requestAnimationFrame(() => document.getElementById(`tab-${id}`)?.focus());
    }}>{state.tabs.map(tab => <div className={`workspace-tab ${tab.id === state.activeId ? 'is-active' : ''}`} key={tab.id}>
      <button id={`tab-${tab.id}`} role="tab" aria-selected={tab.id === state.activeId} aria-controls={`page-${tab.id}`} tabIndex={tab.id === state.activeId ? 0 : -1} onClick={() => dispatch({ type: 'activate', id: tab.id })} onDoubleClick={() => dispatch({ type: 'pin', id: tab.id })} title={`${displayTabTitle(tab, locale)}${tab.pinned ? ` (${t('pinned')})` : ` — ${t('pinHint')}`}`}>{tab.pinned && <Pin size={12} />}<span>{displayTabTitle(tab, locale)}</span>{tab.notice && <span className="status-dot" aria-label={t("newResult")} />}</button>
      {tab.view !== 'home' && <button className="tab-close" aria-label={t("closeTab", { title: displayTabTitle(tab, locale) })} onClick={() => dispatch({ type: 'close', id: tab.id })}><X size={13} /></button>}
    </div>)}</div><button className="icon-button" aria-label={t("newTaskTab")} onClick={newTask}><Plus size={17} /></button>
  </div>;
}
