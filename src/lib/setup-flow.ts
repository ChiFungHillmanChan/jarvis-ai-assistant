import type { MailTriageStatus } from './mail';

export function canStartFirstAnalysis(readiness: string, selectedMessages: number) {
  return readiness === 'ready' && selectedMessages > 0;
}

/**
 * What AI sorting sends and to whom, for the consent asked during setup
 * (src-tauri/src/mail/triage.rs). TypeSafe Jev classifies only when the user
 * added a TypeSafe key; otherwise the AI chosen in setup does all of it.
 */
export function triageConsentText(triage: Pick<MailTriageStatus, 'typesafe_key' | 'chosen_ai'>, text: (en: string, zh: string) => string): string {
  const ai = triage.chosen_ai;
  if (triage.typesafe_key) return text(
    `TypeSafe Jev receives the sender, recipients, subject, the first 1,500 characters and your mailbox addresses to sort each email.${ai ? ` Only emails that need a summary or mention a date go to ${ai}, up to 6,000 characters.` : ''}`,
    `TypeSafe Jev 會收到寄件人、收件人、主旨、正文開頭 1,500 字及你的信箱地址，用來分類每封郵件。${ai ? `只有需要摘要或含日期的郵件，才會把最多 6,000 字傳送至 ${ai}。` : ''}`);
  if (ai === 'Ollama') return text(
    'The sender, recipients, subject and up to 6,000 characters of each email go to Ollama on this Mac, so the mail does not leave this Mac.',
    '每封郵件的寄件人、收件人、主旨及最多 6,000 字正文會交由此 Mac 上的 Ollama 處理，郵件不會離開此 Mac。');
  return text(
    `The sender, recipients, subject and up to 6,000 characters of each email are sent to ${ai}, the AI you chose.`,
    `每封郵件的寄件人、收件人、主旨及最多 6,000 字正文會傳送至你選擇的 ${ai}。`);
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
