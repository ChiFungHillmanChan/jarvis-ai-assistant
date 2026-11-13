import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface UseTauriCommandResult<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  refetch: () => void;
}

/** What the hook holds, and the command and arguments it holds it for. */
interface Slot<T> {
  key: string;
  data: T | null;
  error: string | null;
  loading: boolean;
}

// Module-level stale-while-revalidate cache
const ipcCache = new Map<string, { data: unknown; timestamp: number }>();
const CACHE_TTL = 30_000; // 30 seconds

/** What arguments start with before their own reply: their cached data, if any. */
function fromCache<T>(key: string): Slot<T> {
  const entry = ipcCache.get(key);
  return { key, data: entry ? (entry.data as T) : null, error: null, loading: !entry };
}

export function useTauriCommand<T>(command: string, args?: Record<string, unknown>): UseTauriCommandResult<T> {
  const argsKey = JSON.stringify(args);
  const cacheKey = `${command}:${argsKey}`;

  const [slot, setSlot] = useState<Slot<T>>(() => fromCache<T>(cacheKey));
  const [trigger, setTrigger] = useState(0);

  useEffect(() => {
    // Cleared when the arguments change or the component goes: a reply that
    // arrives after that is for arguments no longer shown, and is only cached.
    let current = true;
    const entry = ipcCache.get(cacheKey);

    // Serve cached data immediately to avoid loading flash
    if (entry) {
      setSlot({ key: cacheKey, data: entry.data as T, error: null, loading: false });
      // Skip fetch if cache is still fresh
      if (Date.now() - entry.timestamp < CACHE_TTL) return;
    } else {
      // A refetch keeps what these arguments showed; other arguments' data is not kept.
      setSlot((prev) => ({ key: cacheKey, data: prev.key === cacheKey ? prev.data : null, error: null, loading: true }));
    }

    // Fetch in background (stale-while-revalidate)
    invoke<T>(command, args)
      .then((result) => {
        ipcCache.set(cacheKey, { data: result, timestamp: Date.now() });
        if (current) setSlot({ key: cacheKey, data: result, error: null, loading: false });
      })
      .catch((e) => {
        if (current) setSlot((prev) => ({ key: cacheKey, data: prev.key === cacheKey ? prev.data : null, error: String(e), loading: false }));
      });
    return () => {
      current = false;
    };
  }, [command, argsKey, trigger]);

  function refetch() {
    ipcCache.delete(cacheKey);
    setTrigger((prev) => prev + 1);
  }
  // Until the effect has run for new arguments, the slot still holds the old
  // ones' data: show only what the cache has for the new ones, or nothing.
  const shown = slot.key === cacheKey ? slot : fromCache<T>(cacheKey);
  return { data: shown.data, error: shown.error, loading: shown.loading, refetch };
}
