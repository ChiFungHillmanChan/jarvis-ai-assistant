import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, type ReactNode } from 'react';
import type { SettingsSectionId } from './sections';
import { initialSettingsDialog, isSettingsShortcut, settingsDialogReducer } from './settings-dialog-state';

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
  const openSettings = useCallback((section?: SettingsSectionId) => dispatch({ type: 'open', section }), []);
  const closeSettings = useCallback(() => dispatch({ type: 'close' }), []);
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

  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettingsDialog(): SettingsDialogValue {
  const value = useContext(SettingsContext);
  if (!value) throw new Error('SettingsProvider is required');
  return value;
}
