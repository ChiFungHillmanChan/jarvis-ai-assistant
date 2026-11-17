import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/**
 * The model list from the model registry (src-tauri/src/ai/registry, read
 * through ai_models), the only place model ids, defaults, prices, presets
 * and the services setup offers are kept. Never hardcode a model id in the
 * frontend.
 */
/** `whisper` is the on-device speech model; its file is `<id>.bin`. */
export interface AiModelDefaults { openai: string; claude: string; gemini_flash: string; gemini_pro: string; bedrock: string; whisper: string }
/** US$ per 1M tokens. */
export interface ModelPrice { input_per_million: number; output_per_million: number; cache_read_per_million: number; cache_write_per_million: number }
export interface CloudModel { provider: string; id: string; label: string; family: boolean; price: ModelPrice; max_output_tokens: number | null }
/** Set for the models offered in one-click setup and approved for mail. */
export interface LocalSetup { disk_reserve_gb: number; context_cap: number; think_off: boolean }
export interface LocalModel { id: string; label: string; param_size: string; download_gb: number; min_ram_gb: number; tool_capable: boolean; description: string; setup: LocalSetup | null }
/** Copy in both interface languages, as the registry holds it. */
export interface RegistryText { en: string; 'zh-HK': string }
/** Which registry the lists come from: shipped in the app, the last fetched copy kept on this Mac, or one fetched this launch. */
export interface RegistryInfo { version: number; issued_at: string; source: 'bundled' | 'cached' | 'remote' }
export interface PresetChoice { id: string; label: RegistryText }
/** A service setup offers; `openai_compat` is "another service". */
export interface SetupChoice { id: string; name: RegistryText; hint: RegistryText; key_page: string | null; model: 'optional' | 'required' }
export interface ServiceRegion { id: string; base_url: string; key_page: string }
export interface CompatibleService { id: string; name: string; regions: ServiceRegion[]; host_suffixes: string[] }
export interface AiModelCatalog {
  defaults: AiModelDefaults;
  cloud: CloudModel[];
  local: LocalModel[];
  registry: RegistryInfo;
  /** Whether this build checks for newer registries. */
  registry_updates: boolean;
  app_version: string;
  presets: PresetChoice[];
  default_preset: string;
  setup: SetupChoice[];
  compatible: CompatibleService[];
}

let cached: Promise<AiModelCatalog> | null = null;
const subscribers = new Set<() => void>();
let listening = false;

/** A newer registry was put in use (registry-updated): load the lists again. */
function listenForUpdates() {
  if (listening) return;
  listening = true;
  void listen('registry-updated', () => {
    cached = null;
    subscribers.forEach(reload => reload());
  }).catch(() => { listening = false; });
}

export function aiModels(): Promise<AiModelCatalog> {
  cached ??= invoke<AiModelCatalog>('ai_models').catch(error => { cached = null; throw error; });
  return cached;
}

/** The catalog, or null until it has loaded. Follows registry updates. */
export function useAiModels(): AiModelCatalog | null {
  const [catalog, setCatalog] = useState<AiModelCatalog | null>(null);
  useEffect(() => {
    let current = true;
    const load = () => { void aiModels().then(data => { if (current) setCatalog(data); }).catch(() => {}); };
    listenForUpdates();
    subscribers.add(load);
    load();
    return () => { current = false; subscribers.delete(load); };
  }, []);
  return catalog;
}
