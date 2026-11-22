import { useCallback, useEffect, useState } from 'react';
import { useUiLocale } from '../../i18n';
import { addLocalEndpoint, listEndpointModels, listLocalEndpoints, removeLocalEndpoint, testEndpointConnection } from '../../lib/commands';
import type { EndpointHealth, LocalEndpoint, LocalModel } from '../../lib/types';

/**
 * The local model endpoints (Advanced > Local AI) with the commands the old
 * Settings page used: add, test, list models and remove. Each failure is one
 * line in the interface language with the backend's reason after it.
 */
export function useLocalEndpoints() {
  const { text } = useUiLocale();
  const [endpoints, setEndpoints] = useState<LocalEndpoint[]>([]);
  const [models, setModels] = useState<Record<string, LocalModel[]>>({});
  const [health, setHealth] = useState<Record<string, EndpointHealth>>({});
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    try {
      setEndpoints(await listLocalEndpoints());
      setReady(true);
      setLoadError(false);
    } catch {
      setLoadError(true);
    }
  }, []);
  useEffect(() => { void load(); }, [load]);

  const added = useCallback((endpoint: LocalEndpoint) => setEndpoints(list => [...list.filter(e => e.id !== endpoint.id), endpoint]), []);
  const modelsLoaded = useCallback((id: string, list: LocalModel[]) => setModels(all => ({ ...all, [id]: list })), []);

  async function add(name: string, url: string) {
    setError('');
    try {
      const endpoint = await addLocalEndpoint(name.trim() || url.trim(), url.trim());
      added(endpoint);
      modelsLoaded(endpoint.id, await listEndpointModels(endpoint.id));
    } catch (e) {
      setError(text(`Could not add the endpoint: ${String(e)}`, `無法新增端點：${String(e)}`));
      throw e;
    }
  }

  async function test(id: string) {
    setError('');
    try {
      const result = await testEndpointConnection(id);
      setHealth(all => ({ ...all, [id]: result }));
      modelsLoaded(id, await listEndpointModels(id));
    } catch (e) {
      setError(text(`Could not test the endpoint: ${String(e)}`, `無法測試端點：${String(e)}`));
    }
  }

  /** Removes the endpoint; the caller takes its rows out of the provider order. */
  async function remove(id: string) {
    setError('');
    try {
      await removeLocalEndpoint(id);
      setEndpoints(list => list.filter(e => e.id !== id));
    } catch (e) {
      setError(text(`Could not remove the endpoint: ${String(e)}`, `無法移除端點：${String(e)}`));
      throw e;
    }
  }

  return { endpoints, models, health, ready, loadError, error, setError, load, added, modelsLoaded, add, test, remove };
}

export type LocalEndpoints = ReturnType<typeof useLocalEndpoints>;
