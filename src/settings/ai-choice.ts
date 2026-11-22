// What the simple "Your AI" path offers. Pure, so it is tested under node.
// The model list is the registry catalog; nothing here names a model.
import type { AiModelCatalog, CloudModel } from '../lib/ai-models';
import type { ProviderChainEntry } from '../lib/types';

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
