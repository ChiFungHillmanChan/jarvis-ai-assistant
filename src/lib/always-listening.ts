import type { VoiceState, WakeWordStatus } from './types';

export interface ListeningApi {
  status: () => Promise<WakeWordStatus>;
  download: () => Promise<unknown>;
  enable: () => Promise<void>;
  disable: () => Promise<void>;
}

/**
 * Switch "Hey Jarvis" listening on or off. The wake phrase is heard by a speech
 * model on this Mac, so the first switch-on downloads it (about 148 MB). The
 * choice is saved by the backend and restored at every launch.
 */
export async function setAlwaysListening(on: boolean, api: ListeningApi): Promise<WakeWordStatus> {
  if (on) {
    if (!(await api.status()).model_downloaded) await api.download();
    await api.enable();
  } else {
    await api.disable();
  }
  return api.status();
}

/**
 * The switch stays still while the model downloads (a second download would
 * write the same file) and while push-to-talk owns the microphone.
 */
export function canSwitchListening(status: WakeWordStatus | null, busy: boolean, state: VoiceState): boolean {
  if (!status || busy) return false;
  if (typeof state === 'object') return !('ModelDownloading' in state);
  return state !== 'Listening' && state !== 'Processing' && state !== 'Speaking';
}

/** The voice state without its payload, so a download's progress is one kind. */
export function voiceStateKind(state: VoiceState): string {
  return typeof state === 'object' ? Object.keys(state)[0] : state;
}
