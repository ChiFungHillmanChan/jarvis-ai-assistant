import { invoke } from '@tauri-apps/api/core';

/** The one AI JARVIS uses (setup step 1). See commands/ai_choice.rs. */
export type AiProvider = 'ollama' | 'openai' | 'gemini' | 'claude' | 'compatible';
export interface AiChoice { provider: AiProvider; apiKey?: string; baseUrl?: string; model?: string }
export interface CurrentAi { provider: AiProvider | 'bedrock' | string; label: string; model: string | null; base_url: string | null }

export const chooseAi = (choice: AiChoice) => invoke<CurrentAi>('choose_ai', { choice });
export const currentAi = () => invoke<CurrentAi | null>('current_ai');
export const listAiModels = (provider: 'ollama' | 'compatible', baseUrl?: string, apiKey?: string) =>
  invoke<string[]>('list_ai_models', { provider, baseUrl, apiKey });

/** OpenAI-compatible services offered by name, with their regional endpoints. */
export const COMPATIBLE_SERVICES = {
  kimi: {
    name: 'Kimi',
    regions: [
      { id: 'global', baseUrl: 'https://api.moonshot.ai/v1', keyPage: 'https://platform.moonshot.ai/console/api-keys' },
      { id: 'china', baseUrl: 'https://api.moonshot.cn/v1', keyPage: 'https://platform.moonshot.cn/console/api-keys' },
    ],
  },
  qwen: {
    name: 'Qwen',
    regions: [
      { id: 'global', baseUrl: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1', keyPage: 'https://www.alibabacloud.com/help/en/model-studio/get-api-key' },
      { id: 'china', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', keyPage: 'https://help.aliyun.com/zh/model-studio/get-api-key' },
    ],
  },
} as const;

/** Same names as compat::label in Rust. */
export function aiServiceLabel(baseUrl: string): string {
  let host = '';
  try { host = new URL(baseUrl).hostname.toLowerCase(); } catch { return baseUrl; }
  if (host.endsWith('moonshot.ai') || host.endsWith('moonshot.cn')) return 'Kimi';
  if (host.endsWith('aliyuncs.com')) return 'Qwen';
  if (host.endsWith('generativelanguage.googleapis.com')) return 'Gemini';
  return host;
}
