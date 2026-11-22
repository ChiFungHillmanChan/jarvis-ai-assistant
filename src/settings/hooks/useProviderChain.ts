import { useCallback, useEffect, useState } from 'react';
import { getProviderChain } from '../../lib/commands';
import { currentAi, type CurrentAi } from '../../lib/ai-choice';
import type { ProviderChainEntry } from '../../lib/types';

/**
 * The saved provider chain (Advanced > Models) and the one AI it describes
 * (current_ai). `ready` stays false until both have loaded, so nothing is
 * saved against a chain that has not been read.
 */
export function useProviderChain() {
  const [chain, setChain] = useState<ProviderChainEntry[]>([]);
  const [current, setCurrent] = useState<CurrentAi | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const [saved, ai] = await Promise.all([getProviderChain(), currentAi()]);
      setChain(saved);
      setCurrent(ai);
      setReady(true);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => { void reload(); }, [reload]);

  return { chain, current, ready, error, reload };
}
