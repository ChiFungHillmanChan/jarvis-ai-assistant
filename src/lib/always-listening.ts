import type { WakeWordStatus } from './types';

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
