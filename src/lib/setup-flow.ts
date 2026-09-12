export function canStartFirstAnalysis(readiness: string, selectedMessages: number) {
  return readiness === 'ready' && selectedMessages > 0;
}

/** Subscribe before starting; only this operation's terminal event may complete it. */
export function followJob<T extends { jobId: string }>(options: {
  jobId: string;
  signal: AbortSignal;
  subscribe: (receive: (event: T) => void) => Promise<() => void>;
  start: () => Promise<unknown>;
  cancel: (jobId: string) => Promise<unknown>;
  terminal: (event: T) => boolean;
  progress: (event: T) => void;
}): Promise<T> {
  return new Promise((resolve, reject) => {
    let done = false;
    let started = false;
    let unsubscribe: (() => void) | undefined;
    const finish = (error?: unknown, result?: T) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      options.signal.removeEventListener('abort', abort);
      unsubscribe?.();
      if (error) reject(error); else resolve(result!);
    };
    const cancel = () => { if (started) void options.cancel(options.jobId).catch(() => {}); };
    const abort = () => { cancel(); finish(new Error('cancelled')); };
    const timer = setTimeout(() => { cancel(); finish(new Error('job_timeout')); }, 66 * 60 * 1000);
    options.signal.addEventListener('abort', abort, { once: true });
    if (options.signal.aborted) { abort(); return; }
    void (async () => {
      try {
        unsubscribe = await options.subscribe(event => {
          if (done || event.jobId !== options.jobId) return;
          options.progress(event);
          if (options.terminal(event)) finish(undefined, event);
        });
        if (done) { unsubscribe(); return; }
        started = true;
        await options.start();
        if (options.signal.aborted) cancel();
      } catch (error) { finish(error); }
    })();
  });
}
