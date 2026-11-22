import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, type ReactNode } from 'react';
import type { SettingsSectionId } from './sections';
import { LocalSetupProvider } from './LocalSetupContext';
import { commitFocusedField, initialSettingsDialog, isSettingsShortcut, settingsDialogReducer } from './settings-dialog-state';

interface SettingsDialogValue {
  open: boolean;
  section: SettingsSectionId;
  openSettings(section?: SettingsSectionId): void;
  closeSettings(): void;
  setupOpen: boolean;
  openSetupGuide(): void;
  closeSetupGuide(): void;
}

const SettingsContext = createContext<SettingsDialogValue | null>(null);

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(settingsDialogReducer, initialSettingsDialog);
  // Every close and every section change, from any path (Esc, X, backdrop,
  // nav, a link inside a section), saves a half-typed field first.
  const openSettings = useCallback((section?: SettingsSectionId) => { commitFocusedField(document); dispatch({ type: 'open', section }); }, []);
  const closeSettings = useCallback(() => { commitFocusedField(document); dispatch({ type: 'close' }); }, []);
  const openSetupGuide = useCallback(() => dispatch({ type: 'open-setup' }), []);
  const closeSetupGuide = useCallback(() => dispatch({ type: 'close-setup' }), []);

  // Cmd+, from anywhere in the workspace. Not over another dialog or a
  // confirmation, which would leave that one under an inert backdrop.
  useEffect(() => {
    function key(e: KeyboardEvent) {
      if (!isSettingsShortcut(e) || e.repeat) return;
      if (document.querySelector('dialog[open], .confirm-backdrop')) return;
      e.preventDefault();
      dispatch({ type: 'open' });
    }
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, []);

  const value = useMemo<SettingsDialogValue>(() => ({
    open: state.open,
    section: state.section,
    setupOpen: state.setupOpen,
    openSettings,
    closeSettings,
    openSetupGuide,
    closeSetupGuide,
  }), [state, openSettings, closeSettings, openSetupGuide, closeSetupGuide]);

  return <SettingsContext.Provider value={value}><LocalSetupProvider>{children}</LocalSetupProvider></SettingsContext.Provider>;
}

export function useSettingsDialog(): SettingsDialogValue {
  const value = useContext(SettingsContext);
  if (!value) throw new Error('SettingsProvider is required');
  return value;
}
