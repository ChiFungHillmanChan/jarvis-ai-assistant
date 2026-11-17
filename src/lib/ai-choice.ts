import { invoke } from '@tauri-apps/api/core';

/** The one AI JARVIS uses (setup step 1). See commands/ai_choice.rs. */
export type AiProvider = 'ollama' | 'openai' | 'gemini' | 'claude' | 'compatible';
export interface AiChoice { provider: AiProvider; apiKey?: string; baseUrl?: string; model?: string }
export interface CurrentAi { provider: AiProvider | 'bedrock' | string; label: string; model: string | null; base_url: string | null }

export const chooseAi = (choice: AiChoice) => invoke<CurrentAi>('choose_ai', { choice });
export const currentAi = () => invoke<CurrentAi | null>('current_ai');
export const listAiModels = (provider: AiProvider, baseUrl?: string, apiKey?: string) =>
  invoke<string[]>('list_ai_models', { provider, baseUrl, apiKey });
