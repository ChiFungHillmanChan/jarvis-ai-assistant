import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface UseTauriCommandResult<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  refetch: () => void;
}

// Module-level stale-while-revalidate cache
const ipcCache = new Map<string, { data: unknown; timestamp: number }>();
const CACHE_TTL = 30_000; // 30 seconds

export function useTauriCommand<T>(command: string, args?: Record<string, unknown>): UseTauriCommandResult<T> {
  const argsKey = JSON.stringify(args);
  const cacheKey = `${command}:${argsKey}`;

  const [data, setData] = useState<T | null>(() => {
    const entry = ipcCache.get(cacheKey);
    return entry ? (entry.data as T) : null;
  });
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(() => !ipcCache.has(cacheKey));
  const [trigger, setTrigger] = useState(0);

  useEffect(() => {
    const entry = ipcCache.get(cacheKey);

    // Serve cached data immediately to avoid loading flash
    if (entry) {
      setData(entry.data as T);
      setLoading(false);
      // Skip fetch if cache is still fresh
      if (Date.now() - entry.timestamp < CACHE_TTL) return;
    } else {
      setLoading(true);
    }

    // Fetch in background (stale-while-revalidate)
    setError(null);
    invoke<T>(command, args)
      .then((result) => {
        ipcCache.set(cacheKey, { data: result, timestamp: Date.now() });
        setData(result);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [command, argsKey, trigger]);

  function refetch() {
    ipcCache.delete(cacheKey);
    setTrigger((prev) => prev + 1);
  }
  return { data, error, loading, refetch };
}
