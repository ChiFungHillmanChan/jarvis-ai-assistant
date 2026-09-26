/** How long a checked connection status is shown without asking the service again. */
export const STATUS_TTL_MS = 5 * 60_000;

/**
 * Connection statuses kept across page switches. Each check asks the service
 * over the network, so switching between Settings pages used to wait on a new
 * check every time and showed the Connect button meanwhile.
 */
export function createStatusCache<K, V>(fetch: (key: K) => Promise<V>, now: () => number = Date.now) {
  const values = new Map<K, { value: V; at: number }>();
  const requests = new Map<K, { epoch: number; promise: Promise<V> }>();
  // A check that started before a change must not overwrite a newer status.
  const epochs = new Map<K, number>();
  const bump = (key: K) => { const next = (epochs.get(key) ?? 0) + 1; epochs.set(key, next); return next; };

  return {
    peek(key: K): V | null { return values.get(key)?.value ?? null; },
    load(key: K, force = false): Promise<V> {
      const cached = values.get(key);
      if (!force && cached && now() - cached.at < STATUS_TTL_MS) return Promise.resolve(cached.value);
      const pending = requests.get(key);
      if (!force && pending) return pending.promise;
      const epoch = bump(key);
      const promise = fetch(key).then(value => {
        if (epochs.get(key) === epoch) values.set(key, { value, at: now() });
        return value;
      }).finally(() => { if (requests.get(key)?.epoch === epoch) requests.delete(key); });
      requests.set(key, { epoch, promise });
      return promise;
    },
    invalidate() {
      for (const key of new Set([...values.keys(), ...requests.keys()])) bump(key);
      values.clear();
      requests.clear();
    },
  };
}
