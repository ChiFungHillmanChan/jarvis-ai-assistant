// When the update toast is on screen, and what it says.
//
// The toast is the only update affordance in the workspace, so these rules
// decide whether a user can update at all. Two of them matter most: a dismissal
// lasts only until the next check, and a download in progress can never be
// hidden.

export type UpdatePhase = 'idle' | 'downloading' | 'error';

export interface ToastState {
  version: string | null;
  phase: UpdatePhase;
  dismissed: boolean;
  progress: number;
  message: string | null;
}

export type ToastEvent =
  | { type: 'checked'; version: string | null }
  | { type: 'dismissed' }
  | { type: 'download-started' }
  | { type: 'progress'; percent: number }
  | { type: 'failed'; message: string };

export const initialToastState: ToastState = {
  version: null,
  phase: 'idle',
  dismissed: false,
  progress: 0,
  message: null,
};

export function updateToastReducer(state: ToastState, event: ToastEvent): ToastState {
  switch (event.type) {
    case 'checked':
      // A check that lands mid-download must not relabel the toast: the handle
      // being installed is for the version already showing.
      if (state.phase === 'downloading') return state;
      return { ...state, version: event.version, dismissed: false, phase: 'idle', message: null };
    case 'dismissed':
      return { ...state, dismissed: true };
    case 'download-started':
      return { ...state, phase: 'downloading', progress: 0, message: null };
    case 'progress':
      return { ...state, progress: event.percent };
    case 'failed':
      return { ...state, phase: 'error', message: event.message };
  }
}

export function isToastVisible(state: ToastState): boolean {
  if (!state.version) return false;
  // Progress and failures stay up regardless: hiding a running install would
  // leave the user with no sign it is happening, and no way to retry.
  if (state.phase === 'downloading' || state.phase === 'error') return true;
  return !state.dismissed;
}
