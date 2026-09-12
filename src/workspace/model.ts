import { translate, type UiLocale } from '../i18n/catalog.ts';
export const views = ['home', 'chat', 'email', 'calendar', 'github', 'notion', 'cron', 'connections', 'settings', 'result'] as const;
export type WorkspaceView = typeof views[number];
export const viewNames: Record<WorkspaceView, string> = { home: 'Automation', chat: 'Assistant', email: 'Mail', calendar: 'Calendar', github: 'GitHub', notion: 'Notes', cron: 'Automations', connections: 'Connections', settings: 'Settings', result: 'Result' };
export interface WorkspaceTab { id: string; view: WorkspaceView; title: string; sessionId?: string; pinned?: boolean; notice?: boolean }
export interface WorkspaceState { tabs: WorkspaceTab[]; activeId: string; history: WorkspaceTab[]; previousId?: string }
export type WorkspaceAction =
  | { type: 'open'; view: WorkspaceView; id?: string; title?: string; sessionId?: string; background?: boolean }
  | { type: 'activate' | 'close' | 'pin'; id: string }
  | { type: 'rename'; id: string; title: string }
  | { type: 'notice'; id: string }
  | { type: 'restore'; state: WorkspaceState };
export function initialWorkspace(): WorkspaceState { return { tabs: [{ id: 'automation', view: 'home', title: 'Automation', pinned: true }], activeId: 'automation', history: [] }; }
export function workspaceReducer(state: WorkspaceState, action: WorkspaceAction): WorkspaceState {
  if (action.type === 'restore') return action.state;
  if (action.type === 'open') {
    const id = action.id ?? (action.view === 'home' ? 'automation' : `tool:${action.view}`);
    const existing = state.tabs.find(t => t.id === id);
    const old = existing ?? state.history.find(t => t.id === id);
    const tab: WorkspaceTab = old && action.view === 'result' ? { ...old, sessionId: action.sessionId, title: action.title ?? old.title } : old ?? { id, view: action.view, title: action.title ?? viewNames[action.view], sessionId: action.sessionId };
    const tabs = existing ? state.tabs : [...state.tabs, tab];
    return { ...state, tabs: tabs.map(t => t.id === id ? { ...tab, notice: action.background && id !== state.activeId } : t), activeId: action.background ? state.activeId : id, previousId: action.background || id === state.activeId ? state.previousId : state.activeId, history: state.history.filter(t => t.id !== id) };
  }
  const tab = state.tabs.find(t => t.id === action.id);
  if (action.type === 'rename') {
    const title = action.title.trim().slice(0, 64);
    if (!title) return state;
    return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, title } : t), history: state.history.map(t => t.id === action.id ? { ...t, title } : t) };
  }
  if (!tab) return state;
  if (action.type === 'activate') return { ...state, activeId: tab.id, previousId: tab.id === state.activeId ? state.previousId : state.activeId, tabs: state.tabs.map(t => t.id === tab.id ? { ...t, notice: false } : t) };
  if (action.type === 'notice') return { ...state, tabs: state.tabs.map(t => t.id === tab.id ? { ...t, notice: t.id !== state.activeId } : t) };
  if (action.type === 'pin') return { ...state, tabs: state.tabs.map(t => t.id === tab.id && t.view !== 'home' ? { ...t, pinned: !t.pinned } : t) };
  if (tab.view === 'home') return state;
  const tabs = state.tabs.filter(t => t.id !== tab.id);
  const next = tabs.find(t => t.id === state.previousId) ?? tabs[Math.max(0, state.tabs.indexOf(tab) - 1)] ?? tabs[0];
  return { ...state, tabs, activeId: state.activeId === tab.id ? next.id : state.activeId, history: [tab, ...state.history.filter(t => t.id !== tab.id)].slice(0, 20) };
}
function validTab(value: unknown): value is WorkspaceTab {
  if (!value || typeof value !== 'object') return false;
  const t = value as Partial<WorkspaceTab>;
  return typeof t.id === 'string' && t.id.length > 0 && t.id.length <= 100 && typeof t.title === 'string' && t.title.length <= 64 && views.includes(t.view as WorkspaceView) && (t.view !== 'chat' || (typeof t.sessionId === 'string' && t.sessionId.length > 0 && t.sessionId.length <= 100));
}
export function restoreWorkspace(raw: string | undefined): WorkspaceState {
  if (!raw) return initialWorkspace();
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed.tabs)) return initialWorkspace();
    const tabs = initialWorkspace().tabs;
    for (const t of parsed.tabs.filter(validTab).slice(0, 30)) {
      if (t.view === 'home' || tabs.some(x => x.id === t.id)) continue;
      tabs.push({ id: t.id, view: t.view, title: t.title, sessionId: t.sessionId, pinned: !!t.pinned });
    }
    const history: WorkspaceTab[] = [];
    for (const t of (Array.isArray(parsed.history) ? parsed.history : []).filter(validTab).slice(0, 20)) {
      if (t.view === 'home' || [...tabs, ...history].some(x => x.id === t.id)) continue;
      history.push({ id: t.id, view: t.view, title: t.title, sessionId: t.sessionId });
    }
    return { tabs, history, activeId: tabs.some(t => t.id === parsed.activeId) ? parsed.activeId : 'automation' };
  } catch { return initialWorkspace(); }
}
export function shouldFocusResult(activeId: string, originId: string, activeView: WorkspaceView): boolean { return activeId === originId && activeView === 'chat'; }

const viewKeys = { home: 'navAutomation', chat: 'navAssistant', email: 'navMail', calendar: 'navCalendar', github: 'navGitHub', notion: 'navNotes', cron: 'navAutomations', connections: 'navConnections', settings: 'navSettings', result: 'navResult' } as const;
export function viewName(view: WorkspaceView, locale: UiLocale) { return translate(locale, viewKeys[view]); }
export function displayTabTitle(tab: WorkspaceTab, locale: UiLocale) { return tab.title === viewNames[tab.view] || (tab.view === 'chat' && tab.title === 'New task') ? (tab.view === 'chat' && tab.title === 'New task' ? (locale === 'zh-HK' ? '新工作' : 'New task') : viewName(tab.view, locale)) : tab.title; }
