// What the simple "Your AI" path offers. Pure, so it is tested under node.
// The model list is the registry catalog; nothing here names a model.
import type { AiModelCatalog, CloudModel } from '../lib/ai-models';
import type { ProviderChainEntry } from '../lib/types';
import type { LocalSetupProbe } from '../lib/setup';

/** US$ per 1M input tokens. Models above this stay selectable in Advanced > Models. */
export const MAIN_PATH_MAX_INPUT_USD = 1.5;

/** Cheapest first; equal prices are ordered by output price, then id, so the result never shifts. */
function byPrice(a: CloudModel, b: CloudModel): number {
  return a.price.input_per_million - b.price.input_per_million
    || a.price.output_per_million - b.price.output_per_million
    || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}

/** The active, pickable models of one provider within the main-path price, cheapest first. */
export function mainPathModels(cloud: CloudModel[], provider: string): CloudModel[] {
  return cloud
    .filter(model => model.provider === provider && model.status === 'active' && !model.family && model.price.input_per_million <= MAIN_PATH_MAX_INPUT_USD)
    .sort(byPrice);
}

/** The lowest-priced active model of a provider, or null when it lists none. */
export function cheapestFor(cloud: CloudModel[], provider: string): CloudModel | null {
  return cloud
    .filter(model => model.provider === provider && model.status === 'active' && !model.family)
    .sort(byPrice)[0] ?? null;
}

/** Choosing a main AI replaces the provider order, so more than one entry asks first. */
export function needsChainConfirm(chain: ProviderChainEntry[]): boolean {
  return chain.length > 1;
}

/** The JARVIS AI card shows only once the registry offers the service in setup. */
export function jarvisAiAvailable(catalog: AiModelCatalog): boolean {
  return catalog.setup.some(choice => choice.id === 'jarvis_ai');
}

/**
 * Measured cost of sorting one email with Gemini's mail sort default: the
 * live 120-email test of 22 Nov 2026 (mail/triage.rs
 * live_flash_lite_vs_flash_on_the_measured_set), US$0.00099 per email. It
 * holds only for the model measured, so it carries that model's price.
 */
export const GEMINI_SORT_MEASURED = { usdPerEmail: 0.00099, inputPerMillion: 0.25, outputPerMillion: 1.5 };

/** The model Gemini sorts mail with when none is picked (registry features.mail_sort.gemini). */
export function geminiDefault(catalog: AiModelCatalog | null): CloudModel | null {
  if (!catalog) return null;
  return catalog.cloud.find(model => model.provider === 'gemini' && model.id === catalog.defaults.gemini_mail_sort) ?? null;
}

/** "1.00" for "About US$1.00 to sort 1,000 emails", rounded to 10 cents; null unless `model` is the measured one. */
export function sortCostPer1000(model: CloudModel | null, catalog: AiModelCatalog | null): string | null {
  const measured = model && model.id === geminiDefault(catalog)?.id
    && model.price.input_per_million === GEMINI_SORT_MEASURED.inputPerMillion
    && model.price.output_per_million === GEMINI_SORT_MEASURED.outputPerMillion;
  return measured ? (Math.round(GEMINI_SORT_MEASURED.usdPerEmail * 1000 * 10) / 10).toFixed(2) : null;
}

/** How many times the default's input price a model costs, to one decimal. */
export function costRatio(model: CloudModel, base: CloudModel): number {
  return Math.round(model.price.input_per_million / base.price.input_per_million * 10) / 10;
}

/**
 * Why Ollama cannot run here, as the local setup checks it
 * (commands/local_setup.rs): not macOS, older than macOS 14, or too little
 * memory for any approved model. Null when it can. Codes match setupError.
 */
export function localUnavailable(probe: LocalSetupProbe): 'unsupported_os' | 'unsupported_os_version' | 'insufficient_ram' | null {
  if (probe.os !== 'macos') return 'unsupported_os';
  const major = Number(probe.macosVersion?.split('.')[0]);
  if (!(major >= 14)) return 'unsupported_os_version';
  if (!probe.recommendedTag) return 'insufficient_ram';
  return null;
}
