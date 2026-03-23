import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface UseTauriCommandResult<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  refetch: () => void;
}

export function useTauriCommand<T>(command: string, args?: Record<string, unknown>): UseTauriCommandResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [trigger, setTrigger] = useState(0);
  const argsKey = JSON.stringify(args);

  useEffect(() => {
    setLoading(true);
    setError(null);
    invoke<T>(command, args)
      .then(setData)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [command, argsKey, trigger]);

  function refetch() { setTrigger((prev) => prev + 1); }
  return { data, error, loading, refetch };
}
