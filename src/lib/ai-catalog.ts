import type { AiModelCatalog, CloudModel, CompatibleService, LocalModel, RegistryInfo, RegistryText, SetupChoice } from './ai-models';

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

function hostOf(baseUrl: string): string | null {
  try { return new URL(baseUrl).hostname.toLowerCase(); } catch { return null; }
}

/** Whether `host` is `suffix` or a name under it, as ai/compat.rs `on_host`: "evilmoonshot.ai" is not on "moonshot.ai". */
export function onHost(host: string, suffix: string): boolean {
  return host === suffix || host.endsWith(`.${suffix}`);
}

/** The compatible service a base URL is on, matched on its hosts as ai/compat.rs does. */
export function serviceFor(catalog: AiModelCatalog | null, baseUrl: string): CompatibleService | null {
  const host = hostOf(baseUrl);
  if (!catalog || !host) return null;
  return catalog.compatible.find(service => service.host_suffixes.some(suffix => onHost(host, suffix))) ?? null;
}

/** The name shown for a base URL: the service's, else its host (compat::label). */
export function serviceLabel(catalog: AiModelCatalog | null, baseUrl: string): string {
  return serviceFor(catalog, baseUrl)?.name ?? hostOf(baseUrl) ?? baseUrl;
}

/** The setup choices ChooseAi has a form for. */
export type SetupOption = 'ollama' | 'openai' | 'gemini' | 'claude' | 'kimi' | 'qwen' | 'other';
const OPTION_FOR: Record<string, SetupOption> = { ollama: 'ollama', openai: 'openai', gemini: 'gemini', claude: 'claude', kimi: 'kimi', qwen: 'qwen', openai_compat: 'other' };

/** The services setup offers, in the registry's order, for the ones this build has a form for. */
export function setupOptions(catalog: AiModelCatalog | null): (SetupChoice & { option: SetupOption })[] {
  return (catalog?.setup ?? []).flatMap(choice => OPTION_FOR[choice.id] ? [{ ...choice, option: OPTION_FOR[choice.id] }] : []);
}

/** Registry copy in the interface language. */
export function localized(copy: RegistryText, text: (en: string, zh: string) => string): string {
  return text(copy.en, copy['zh-HK']);
}

/** Where the model list in use came from, as Settings > About says it. */
export function registrySourceLabel(source: RegistryInfo['source'], text: (en: string, zh: string) => string): string {
  switch (source) {
    case 'bundled': return text('Built into this version of JARVIS', '此版本 JARVIS 內置');
    case 'cached': return text('An update downloaded earlier and kept on this Mac', '早前下載並保存在此 Mac 的更新');
    case 'remote': return text('An update downloaded since JARVIS opened', 'JARVIS 開啟後下載的更新');
  }
}
