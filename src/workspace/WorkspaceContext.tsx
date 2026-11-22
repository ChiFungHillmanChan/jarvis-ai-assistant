import { createContext, useCallback, useContext, useEffect, useReducer, useRef, useState, useSyncExternalStore, type Dispatch, type ReactNode, type SetStateAction } from 'react';
import { getSettings, updateSetting } from '../lib/commands';
import { initialWorkspace, pruneWorkspacePages, restoreWorkspace, workspaceReducer, viewNames, type WorkspaceAction, type WorkspacePages, type WorkspaceState, type WorkspaceView } from './model';

interface WorkspaceContextValue {
  state: WorkspaceState;
  dispatch: Dispatch<WorkspaceAction>;
  ready: boolean;
  saveError: string | null;
  open: (view: WorkspaceView) => void;
  newTask: () => string;
  forgetSession: (sessionId: string) => void;
  readValue: <T>(tab: string, key: string, initial: T) => T;
  writeValue: (tab: string, key: string, value: unknown) => void;
  subscribe: (callback: () => void) => () => void;
  appearance: string;
  /** Shows the theme at once; on a failed save puts the previous one back, sets saveError and rejects. */
  setAppearance: (value: string) => Promise<void>;
}
const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);
const TabContext = createContext('automation');
export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(workspaceReducer, undefined, initialWorkspace);
  const [ready, setReady] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [appearance, setAppearanceValue] = useState('dark');
  const pages = useRef<WorkspacePages>({});
  const subscribers = useRef(new Set<() => void>());
  const restored = useRef(false);
  const stateRef = useRef(state);
  stateRef.current = state;
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const saveChain = useRef(Promise.resolve());
  const scheduleSave = useCallback(() => {
    if (!restored.current) return;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      const snapshot = stateRef.current;
      // A tool result writes its payload before dispatching open. Prune only
      // when this deferred save sees the current tabs and restorable history.
      pages.current = pruneWorkspacePages(snapshot, pages.current);
      const layout = JSON.stringify(snapshot);
      const pageState = JSON.stringify(pages.current);
      saveChain.current = saveChain.current.then(async () => {
        await updateSetting('workspace_layout_v1', layout);
        await updateSetting('workspace_pages_v1', pageState);
        setSaveError(null);
      }).catch(() => setSaveError('Workspace changes could not be saved. Keep this window open and try again.'));
    }, 600);
  }, []);
  useEffect(() => {
    let cancelled = false;
    getSettings().then(settings => {
      if (cancelled) return;
      const layout = restoreWorkspace(settings.values.workspace_layout_v1);
      dispatch({ type: 'restore', state: layout });
      try { const saved = JSON.parse(settings.values.workspace_pages_v1 || '{}'); if (saved && typeof saved === 'object' && !Array.isArray(saved)) pages.current = pruneWorkspacePages(layout, saved); } catch { /* Recover from an invalid saved layout with a fresh workspace. */ }
      const mode = settings.values.workspace_appearance;
      if (mode === 'light' || mode === 'dark' || mode === 'system') setAppearanceValue(mode);
      restored.current = true;
    }).catch(() => setSaveError('Could not restore the previous workspace. New work will still be kept in this window.')).finally(() => { if (!cancelled) setReady(true); });
    return () => { cancelled = true; };
  }, []);
  useEffect(() => { if (ready) scheduleSave(); }, [state, ready, scheduleSave]);
  useEffect(() => { document.documentElement.dataset.appearance = appearance; }, [appearance]);
  const readValue = useCallback(<T,>(tab: string, key: string, initial: T): T => {
    const value = pages.current[tab]?.[key];
    return value === undefined ? initial : value as T;
  }, []);
  const writeValue = useCallback((tab: string, key: string, value: unknown) => {
    if (Object.is(pages.current[tab]?.[key], value)) return;
    pages.current[tab] = { ...pages.current[tab], [key]: value };
    subscribers.current.forEach(fn => fn());
    scheduleSave();
  }, [scheduleSave]);
  const subscribe = useCallback((callback: () => void) => {
    subscribers.current.add(callback);
    return () => { subscribers.current.delete(callback); };
  }, []);
  const open = useCallback((view: WorkspaceView) => dispatch({ type: 'open', view }), []);
  const newTask = useCallback(() => {
    const sessionId = crypto.randomUUID();
    dispatch({ type: 'open', view: 'chat', id: `task:${sessionId}`, sessionId, title: 'New task' });
    return sessionId;
  }, []);
  const forgetSession = useCallback((sessionId: string) => {
    // An unsent draft is kept past pruning on purpose, so remove it explicitly.
    delete pages.current[`task:${sessionId}`];
    subscribers.current.forEach(fn => fn());
    dispatch({ type: 'forget', sessionId });
  }, []);
  const appearanceRef = useRef(appearance);
  appearanceRef.current = appearance;
  const setAppearance = useCallback(async (value: string) => {
    const previous = appearanceRef.current;
    setAppearanceValue(value);
    try {
      await updateSetting('workspace_appearance', value);
    } catch (e) {
      setAppearanceValue(previous);
      setSaveError('Could not save appearance.');
      throw e;
    }
  }, []);
  return <WorkspaceContext.Provider value={{ state, dispatch, ready, saveError, open, newTask, forgetSession, readValue, writeValue, subscribe, appearance, setAppearance }}>{children}</WorkspaceContext.Provider>;
}
export function useWorkspace() { const value = useContext(WorkspaceContext); if (!value) throw new Error('WorkspaceProvider is required'); return value; }
export function WorkspaceTabScope({ id, children }: { id: string; children: ReactNode }) { return <TabContext.Provider value={id}>{children}</TabContext.Provider>; }
/** Whether the tab this component renders in is the one on screen. */
export function useIsActiveTab(): boolean { const { state } = useWorkspace(); const tab = useContext(TabContext); return state.activeId === tab; }
export function useWorkspaceValue<T>(key: string, initial: T): [T, Dispatch<SetStateAction<T>>] {
  const { readValue, writeValue, subscribe } = useWorkspace();
  const tab = useContext(TabContext);
  const fallback = useRef(initial);
  const snapshot = useCallback(() => readValue(tab, key, fallback.current), [readValue, tab, key]);
  const value = useSyncExternalStore(subscribe, snapshot);
  const update = useCallback<Dispatch<SetStateAction<T>>>((next) => {
    const current = readValue(tab, key, fallback.current);
    const result = typeof next === 'function' ? (next as (value: T) => T)(current) : next;
    writeValue(tab, key, result);
  }, [tab, key, writeValue, readValue]);
  return [value, update];
}
export function workspaceLabel(view: WorkspaceView) { return viewNames[view]; }
