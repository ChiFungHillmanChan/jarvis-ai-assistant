// Edits to the saved provider order (Advanced > Models and Local AI). Pure, so
// they are tested under node. Each returns a new chain with positions 0..n-1;
// the saved one is never changed in place.
import type { ProviderChainEntry } from '../lib/types';
import type { AiModelCatalog } from '../lib/ai-models';
import { serviceLabel } from '../lib/ai-catalog.ts';

type Chain = ProviderChainEntry[];
type Text = (en: string, zh: string) => string;

const renumber = (chain: Chain): Chain => chain.map((entry, position) => ({ ...entry, position }));

/** The entry at `index` swapped with its neighbour above (-1) or below (1); null at either end. */
export function moveEntry(chain: Chain, index: number, direction: -1 | 1): Chain | null {
  const other = index + direction;
  if (other < 0 || other >= chain.length) return null;
  const next = [...chain];
  [next[index], next[other]] = [next[other], next[index]];
  return renumber(next);
}

export function removeEntry(chain: Chain, index: number): Chain {
  return renumber(chain.filter((_, i) => i !== index));
}

export function toggleEntry(chain: Chain, index: number): Chain {
  return renumber(chain.map((entry, i) => (i === index ? { ...entry, enabled: !entry.enabled } : entry)));
}

/** A new, enabled entry at the end. */
export function addEntry(chain: Chain, entry: Omit<ProviderChainEntry, 'position' | 'enabled'>): Chain {
  return renumber([...chain, { ...entry, position: chain.length, enabled: true }]);
}

/** The order without the rows of a removed local endpoint. */
export function withoutEndpoint(chain: Chain, endpointId: string): Chain {
  return renumber(chain.filter(entry => entry.endpoint_id !== endpointId));
}

/** A Bedrock row's model; empty runs the default model. */
export function setEntryModel(chain: Chain, index: number, model: string): Chain {
  return renumber(chain.map((entry, i) => (i === index ? { ...entry, model_id: model || undefined } : entry)));
}

/** The preset the Default provider control shows: the saved one, or the registry default when it is not listed. */
export function activePreset(saved: string | null | undefined, catalog: Pick<AiModelCatalog, 'presets' | 'default_preset'>): string {
  return saved && catalog.presets.some(preset => preset.id === saved) ? saved : catalog.default_preset;
}

/** How a row of the order reads, as the old Settings page wrote it. */
export function chainLabel(entry: ProviderChainEntry, catalog: AiModelCatalog | null, text: Text): string {
  const model = entry.model_id || text('default', '預設');
  switch (entry.provider_type) {
    case 'local': return `${entry.model_id || text('unknown', '未知')} (${text('local', '本機')})`;
    case 'gemini': return `Gemini (${model})`;
    case 'claude': return `Claude (${model})`;
    case 'openai': return `${entry.base_url ? serviceLabel(catalog, entry.base_url) : 'OpenAI'} (${model})`;
    case 'bedrock': return `Bedrock (${model})`;
    default: return entry.provider_type;
  }
}
