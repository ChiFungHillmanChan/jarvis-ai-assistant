// How JARVIS updates itself, and when the update toast is on screen.
//
// An update downloads in the background as soon as a check finds one. The toast
// appears only when it is ready, and its one action restarts into the new
// version. The toast is the only update affordance in the app, so these rules
// decide whether a user can update at all.

/** How often a long-running session checks again when no update was found. */
export const RECHECK_MS = 6 * 60 * 60 * 1000;
/** A failed check or download tries again after this long. */
export const RETRY_MS = 15 * 60 * 1000;
/** Restart reports failure if JARVIS is still running this long after relaunch. */
export const RESTART_GRACE_MS = 15 * 1000;

/** The updater handle for one found version. */
export interface PendingUpdate {
  version: string;
  download(): Promise<void>;
  install(): Promise<void>;
  close(): Promise<void>;
}

// idle: nothing to show (no update yet, or still downloading in the background).
// ready: downloaded; Restart installs and relaunches.
// restarting: installing and relaunching.
// install-failed: the install was refused; Restart tries again.
// restart-failed: installed, but JARVIS did not restart; reopening finishes it.
export type UpdatePhase = 'idle' | 'ready' | 'restarting' | 'install-failed' | 'restart-failed';

export interface UpdateState {
  version: string | null;
  phase: UpdatePhase;
  dismissed: boolean;
  message: string | null;
}

export function isToastVisible(state: UpdateState): boolean {
  if (state.phase === 'idle') return false;
  // Hiding a restart in progress would leave the user with no sign it is happening.
  if (state.phase === 'restarting') return true;
  return !state.dismissed;
}

/** One per app session: a second runner would download and install again. */
export function createUpdateRunner(options: {
  check: () => Promise<PendingUpdate | null>;
  relaunch: () => Promise<void>;
  schedule?: (run: () => void, delay: number) => () => void;
}) {
  const schedule = options.schedule ?? ((run, delay) => {
    const timer = setTimeout(run, delay);
    return () => clearTimeout(timer);
  });
  let state: UpdateState = { version: null, phase: 'idle', dismissed: false, message: null };
  let pending: PendingUpdate | null = null;
  let started = false;
  const listeners = new Set<() => void>();

  function set(next: Partial<UpdateState>) {
    state = { ...state, ...next };
    listeners.forEach(listener => listener());
  }

  async function run() {
    let found: PendingUpdate | null;
    try {
      found = await options.check();
    } catch (error) {
      // Updater errors (S3 down, offline, no manifest) must never disrupt the app.
      console.warn('update check failed', error);
      schedule(() => void run(), RETRY_MS);
      return;
    }
    if (!found) {
      schedule(() => void run(), RECHECK_MS);
      return;
    }
    try {
      await found.download();
    } catch (error) {
      console.warn('update download failed', error);
      void found.close().catch(() => {});
      schedule(() => void run(), RETRY_MS);
      return;
    }
    pending = found;
    set({ version: found.version, phase: 'ready' });
  }

  return {
    getState: () => state,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    start() {
      if (started) return;
      started = true;
      void run();
    },
    async restart() {
      if (!pending || state.phase === 'restarting') return;
      set({ phase: 'restarting', message: null });
      try {
        await pending.install();
      } catch (error) {
        console.error('update install failed', error);
        set({ phase: 'install-failed', message: String(error), dismissed: false });
        return;
      }
      try {
        await options.relaunch();
      } catch (error) {
        console.error('relaunch after update failed', error);
        set({ phase: 'restart-failed', dismissed: false });
        return;
      }
      // A successful relaunch ends this process before the timer fires.
      schedule(() => set({ phase: 'restart-failed', dismissed: false }), RESTART_GRACE_MS);
    },
    dismiss() {
      if (state.phase !== 'restarting') set({ dismissed: true });
    },
  };
}

export type UpdateRunner = ReturnType<typeof createUpdateRunner>;
