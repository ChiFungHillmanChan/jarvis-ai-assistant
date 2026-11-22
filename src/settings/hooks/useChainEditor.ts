import { useCallback, useState } from 'react';
import { updateProviderChain } from '../../lib/commands';
import type { ProviderChainEntry } from '../../lib/types';
import { useProviderChain } from './useProviderChain';
import { recordAiError } from '../ai-error-log';

/**
 * The saved provider order and one way to change it: `save` writes the new
 * order with update_provider_chain, then reads back what was saved. While a
 * save runs, `busy` is true so a second edit cannot start from a stale order.
 * The write is not tied to the section staying open.
 */
export function useChainEditor() {
  const { chain, current, ready, error, reload } = useProviderChain();
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  /** Resolves to the refusal, or null once saved. `quiet` leaves the shared error line alone (a field shows its own). */
  const save = useCallback(async (next: ProviderChainEntry[] | null, quiet = false): Promise<string | null> => {
    if (!next) return null;
    setBusy(true);
    if (!quiet) setSaveError(null);
    try {
      await updateProviderChain(next);
      return null;
    } catch (e) {
      recordAiError(String(e));
      if (!quiet) setSaveError(String(e));
      return String(e);
    } finally {
      await reload();
      setBusy(false);
    }
  }, [reload]);

  return { chain, current, ready, loadError: error, reload, save, saveError, busy };
}

export type ChainEditor = ReturnType<typeof useChainEditor>;
