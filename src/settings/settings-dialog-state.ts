import type { SettingsSectionId } from './sections.ts';

// Whether the settings dialog is open, on which section, and whether the
// setup guide (OnboardingWizard) has replaced the workspace. One reducer so
// the rules below are tested without rendering.

export interface SettingsDialogState {
  open: boolean;
  section: SettingsSectionId;
  setupOpen: boolean;
}

export type SettingsDialogAction =
  | { type: 'open'; section?: SettingsSectionId }
  | { type: 'close' }
  | { type: 'open-setup' }
  | { type: 'close-setup' };

export const initialSettingsDialog: SettingsDialogState = { open: false, section: 'general', setupOpen: false };

export function settingsDialogReducer(state: SettingsDialogState, action: SettingsDialogAction): SettingsDialogState {
  switch (action.type) {
    // A new open with no section starts at General. While open, this is how
    // the nav moves between sections. The setup guide hides the workspace, so
    // settings never opens behind it.
    case 'open':
      if (state.setupOpen) return state;
      return { ...state, open: true, section: action.section ?? 'general' };
    case 'close':
      return state.open ? { ...state, open: false } : state;
    case 'open-setup':
      return { ...state, open: false, setupOpen: true };
    case 'close-setup':
      return state.setupOpen ? { ...state, setupOpen: false } : state;
  }
}

/** Cmd+, -- the standard macOS shortcut for an app's settings. */
export function isSettingsShortcut(e: { metaKey: boolean; key: string }): boolean {
  return e.metaKey && e.key === ',';
}
