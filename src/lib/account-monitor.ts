import type { AccountStatus } from './types';

export const PAYMENT_WAIT_MS = 120_000;
export const PAYMENT_POLL_MS = 5_000;
export type PaymentState = 'waiting' | 'confirmed' | 'unconfirmed' | null;

/** Bounded payment checks. Only /me can confirm the account's paid tier. */
export function createAccountMonitor(options: {
  fetchStatus: (force: boolean) => Promise<AccountStatus>;
  onStatus: (status: AccountStatus) => void;
  onPayment: (state: PaymentState) => void;
  onError: (error: unknown) => void;
  now?: () => number;
  schedule?: (run: () => void, delay: number) => () => void;
}) {
  const now = options.now ?? Date.now;
  const schedule = options.schedule ?? ((run, delay) => {
    const timer = setTimeout(run, delay);
    return () => clearTimeout(timer);
  });
  let disposed = false;
  let generation = 0;
  let waiting = false;
  let deadline = 0;
  let request: Promise<void> | null = null;
  let cancelPoll: (() => void) | undefined;
  let cancelDeadline: (() => void) | undefined;

  function finish(state: PaymentState) {
    waiting = false;
    cancelPoll?.();
    cancelDeadline?.();
    cancelPoll = cancelDeadline = undefined;
    if (!disposed) options.onPayment(state);
  }

  function refresh(force = false): Promise<void> {
    if (disposed) return Promise.resolve();
    if (request) return request;
    const current = generation;
    request = (async () => {
      try {
        const status = await options.fetchStatus(force);
        if (disposed || current !== generation) return;
        options.onStatus(status);
        if (waiting && (now() >= deadline || !status.signed_in)) finish('unconfirmed');
        else if (waiting && (status.tier === 'pro' || status.tier === 'team')) finish('confirmed');
      } catch (error) {
        if (!disposed && current === generation) options.onError(error);
      } finally {
        if (current === generation) {
          request = null;
          if (waiting && !disposed) {
            cancelPoll?.();
            cancelPoll = schedule(() => {
              if (!waiting || disposed) return;
              if (now() >= deadline) finish('unconfirmed');
              else void refresh(true);
            }, PAYMENT_POLL_MS);
          }
        }
      }
    })();
    return request;
  }

  return {
    refresh,
    waitForPayment() {
      if (disposed) return;
      finish('waiting');
      waiting = true;
      deadline = now() + PAYMENT_WAIT_MS;
      cancelDeadline = schedule(() => finish('unconfirmed'), PAYMENT_WAIT_MS);
      void refresh(true);
    },
    cancelPayment() {
      generation++;
      request = null;
      finish(null);
    },
    dispose() {
      disposed = true;
      generation++;
      request = null;
      finish(null);
    },
  };
}
