import aiConfig from '../../config/jarvis-ai.json';

export type Workload = 'chat' | 'mail_analysis' | 'mail_sort' | 'voice';
export const WORKLOADS: readonly Workload[] = ['chat', 'mail_analysis', 'mail_sort', 'voice'];

export interface CatalogEntry { region: string; tools: boolean; dsml: boolean }
export interface ModelPrice { inputMicrosPerMTok: number; outputMicrosPerMTok: number }

/** Money is integer micro-dollars (1_000_000 = US$1). */
export interface AiConfig {
  multiplierBps: number;
  includedMicros: number;
  defaultCapMicros: number;
  trialCreditMicros: number;
  globalPause: boolean;
  /** False: a Stripe subscription alone never gives JARVIS AI; only an admin grant does. */
  subscriptionAccess: boolean;
  /** Billed total across all users per Hong Kong day at which every request is paused, in real time. */
  globalDailyCapMicros: number;
  models: Record<Workload, { primary: string; fallback: string | null }>;
  prices: Record<string, ModelPrice>;
  pricesCheckedOn: string;
}

export const CATALOG: Record<string, CatalogEntry> = aiConfig.catalog;
export const DEFAULT_CONFIG: AiConfig = aiConfig.defaults;

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const positive = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) > 0;

function parsePrice(value: unknown): ModelPrice | null {
  if (!isObject(value) || !positive(value.inputMicrosPerMTok) || !positive(value.outputMicrosPerMTok)) return null;
  return { inputMicrosPerMTok: value.inputMicrosPerMTok, outputMicrosPerMTok: value.outputMicrosPerMTok };
}

/** Returns a fresh config holding only the known fields, or null when any value is unsafe. */
export function parseConfig(value: unknown): AiConfig | null {
  if (!isObject(value)) return null;
  const { multiplierBps, includedMicros, defaultCapMicros, trialCreditMicros, globalPause, pricesCheckedOn } = value;
  // A multiplier below 100% would sell below cost.
  if (!Number.isSafeInteger(multiplierBps) || (multiplierBps as number) < 10_000) return null;
  if (!positive(includedMicros) || !positive(defaultCapMicros) || !positive(trialCreditMicros)) return null;
  if (typeof globalPause !== 'boolean') return null;
  // A row written before this field existed keeps subscriptions off.
  const subscriptionAccess = value.subscriptionAccess === undefined ? false : value.subscriptionAccess;
  if (typeof subscriptionAccess !== 'boolean') return null;
  // A row written before this field existed gets the shipped ceiling, never no ceiling.
  const globalDailyCapMicros = value.globalDailyCapMicros === undefined ? DEFAULT_CONFIG.globalDailyCapMicros : value.globalDailyCapMicros;
  if (!positive(globalDailyCapMicros)) return null;
  if (typeof pricesCheckedOn !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(pricesCheckedOn)) return null;

  if (!isObject(value.models) || !isObject(value.prices)) return null;
  const prices: Record<string, ModelPrice> = {};
  for (const [id, raw] of Object.entries(value.prices)) {
    const price = parsePrice(raw);
    if (!price) return null;
    if (Object.hasOwn(CATALOG, id)) prices[id] = price; // own keys only: '__proto__' and 'constructor' are dropped
  }

  const models = {} as AiConfig['models'];
  for (const workload of WORKLOADS) {
    const entry = value.models[workload];
    if (!isObject(entry)) return null;
    const { primary, fallback } = entry;
    if (typeof primary !== 'string' || !Object.hasOwn(CATALOG, primary)) return null;
    if (fallback !== null && (typeof fallback !== 'string' || !Object.hasOwn(CATALOG, fallback) || fallback === primary)) return null;
    if (!Object.hasOwn(prices, primary) || (fallback !== null && !Object.hasOwn(prices, fallback))) return null;
    models[workload] = { primary, fallback };
  }

  return {
    multiplierBps: multiplierBps as number,
    includedMicros,
    defaultCapMicros,
    trialCreditMicros,
    globalPause,
    subscriptionAccess,
    globalDailyCapMicros,
    models,
    prices,
    pricesCheckedOn,
  };
}

/** A missing row means the shipped defaults. A row that does not validate pauses JARVIS AI rather than guessing. */
export function effectiveConfig(item: Record<string, unknown> | undefined): { config: AiConfig; invalid: boolean } {
  if (!item) return { config: DEFAULT_CONFIG, invalid: false };
  let parsed: AiConfig | null = null;
  if (typeof item.body === 'string') {
    try { parsed = parseConfig(JSON.parse(item.body)); } catch { parsed = null; }
  }
  return parsed
    ? { config: parsed, invalid: false }
    : { config: { ...DEFAULT_CONFIG, globalPause: true }, invalid: true };
}
