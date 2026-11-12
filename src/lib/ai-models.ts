import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

/**
 * The model list from src-tauri/src/ai/models.rs, the only place model ids,
 * defaults and prices are kept. Never hardcode a model id in the frontend.
 */
export interface AiModelDefaults { openai: string; claude: string; gemini_flash: string; gemini_pro: string; bedrock: string }
/** US$ per 1M tokens. */
export interface ModelPrice { input_per_million: number; output_per_million: number; cache_read_per_million: number; cache_write_per_million: number }
export interface CloudModel { provider: string; id: string; label: string; family: boolean; price: ModelPrice; max_output_tokens: number | null }
/** Set for the models offered in one-click setup and approved for mail. */
export interface LocalSetup { disk_reserve_gb: number; context_cap: number; think_off: boolean }
export interface LocalModel { id: string; label: string; param_size: string; download_gb: number; min_ram_gb: number; tool_capable: boolean; description: string; setup: LocalSetup | null }
export interface AiModelCatalog { defaults: AiModelDefaults; cloud: CloudModel[]; local: LocalModel[] }

let cached: Promise<AiModelCatalog> | null = null;

export function aiModels(): Promise<AiModelCatalog> {
  cached ??= invoke<AiModelCatalog>('ai_models').catch(error => { cached = null; throw error; });
  return cached;
}

/** The catalog, or null until it has loaded. */
export function useAiModels(): AiModelCatalog | null {
  const [catalog, setCatalog] = useState<AiModelCatalog | null>(null);
  useEffect(() => {
    let current = true;
    void aiModels().then(data => { if (current) setCatalog(data); }).catch(() => {});
    return () => { current = false; };
  }, []);
  return catalog;
}
