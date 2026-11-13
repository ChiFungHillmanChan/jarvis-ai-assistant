import type { AiModelCatalog, CloudModel, LocalModel } from './ai-models';

/** The local models one-click mail setup offers, in catalog order. */
export function mailSetupModels(catalog: AiModelCatalog | null): LocalModel[] {
  return (catalog?.local ?? []).filter(model => model.setup !== null);
}

/**
 * The catalog entry that prices a model id, matched as ai/models.rs does:
 * the longest catalog id contained in it. null when the model has no price.
 */
export function cloudModelFor(catalog: AiModelCatalog | null, id: string): CloudModel | null {
  const wanted = id.trim().toLowerCase();
  if (!catalog || !wanted) return null;
  return catalog.cloud.filter(model => wanted.includes(model.id)).sort((a, b) => b.id.length - a.id.length)[0] ?? null;
}

/** A per-1M-token rate as written in copy: 0.1 -> "0.10", 0.042 -> "0.042". */
export function formatRate(usd: number): string {
  const cents = usd * 100;
  return Math.abs(cents - Math.round(cents)) < 1e-9 ? usd.toFixed(2) : String(usd);
}
