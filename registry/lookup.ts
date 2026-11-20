// How a model id is matched to a registry entry. The app's price lookup
// (src-tauri/src/ai/models.rs `find`) is reproduced exactly: among the
// entries that carry a price and are not managed, the longest id contained in
// the reported id wins, compared in lower case, and on a tie the later entry
// wins (Rust's `max_by_key` keeps the last maximum).

export interface Prices {
  input: number;
  output: number;
  cache_read: number | null;
  cache_write: number | null;
  tiers: { above_input_tokens: number; input: number; output: number; cache_read: number | null; cache_write: number | null }[];
}

/** The reasoning efforts OpenAI lists (schema.json $defs/reasoningEffort). */
export type ReasoningEffort = 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';

/** The thinking levels Google lists for the Gemini API (schema.json $defs/thinkingLevel). */
export type ThinkingLevel = 'minimal' | 'low' | 'medium' | 'high' | 'xhigh';

export interface RegistryModel {
  id: string;
  provider: string;
  kind: 'model' | 'family';
  status: 'active' | 'deprecated' | 'disabled';
  replacement: string | null;
  managed: boolean;
  prices: Prices | null;
  max_output: number | null;
  display_name: { en: string; 'zh-HK': string };
  capabilities: { tools: boolean; streaming: boolean; json: boolean; json_schema: boolean; vision: boolean; thinking: boolean; reasoning_effort: boolean };
  /** OpenAI only: this model's chat effort, in place of the provider's limits.chat_reasoning_effort. */
  chat_reasoning_effort?: ReasoningEffort;
  /** Gemini only: the thinking level Google uses for this model when a request names none. */
  default_thinking_level?: ThinkingLevel;
  bedrock?: { region: string; model_id: string; invoke: string; hold_text_with_tools: boolean };
  [key: string]: unknown;
}

/** The entries the app's price lookup searches: the price list in models.rs. */
export function pricedEntries(models: readonly RegistryModel[]): RegistryModel[] {
  return models.filter(model => !model.managed && model.prices !== null);
}

/** The price entry for a model id a cloud provider reports, as models.rs finds it. */
export function findPriced(models: readonly RegistryModel[], reportedId: string): RegistryModel | undefined {
  const wanted = reportedId.toLowerCase();
  let best: RegistryModel | undefined;
  for (const entry of pricedEntries(models)) {
    if (wanted.includes(entry.id) && (!best || entry.id.length >= best.id.length)) best = entry;
  }
  return best;
}

/**
 * The entry a configured id (a feature default, a preset, a replacement)
 * stands for. Managed ids name JARVIS AI's own models and match exactly.
 * Any other id matches an unmanaged entry exactly, or else as the price
 * lookup matches it.
 */
export function resolveModel(models: readonly RegistryModel[], id: string, managed: boolean): RegistryModel | undefined {
  const exact = models.find(model => model.managed === managed && model.id === id);
  if (exact || managed) return exact;
  return findPriced(models, id);
}

/** What one request costs in micro-US$, as models.rs `usd_cost_micro` computes it. */
export function costMicros(entry: RegistryModel, input: number, output: number, cacheRead: number, cacheWrite: number): number | null {
  if (!entry.prices) return null;
  const tier = [...entry.prices.tiers].reverse().find(t => input > t.above_input_tokens);
  const p = tier ?? entry.prices;
  const billable = Math.max(input - cacheRead - cacheWrite, 0);
  const usd = billable * p.input / 1e6 + output * p.output / 1e6 + cacheRead * (p.cache_read ?? 0) / 1e6 + cacheWrite * (p.cache_write ?? 0) / 1e6;
  return Math.round(usd * 1e6);
}
