import type { Text } from './mail-presentation';

/**
 * Settings -> Personalisation. Mirrors CHOICES in src-tauri/src/ai/persona.rs,
 * which validates every value again before it reaches the prompt. The first
 * value is the default.
 */
export const PERSONA_CHOICES = {
  assistant_tone: ['jarvis', 'professional', 'friendly'],
  assistant_length: ['balanced', 'brief', 'detailed'],
  assistant_chinese: ['written', 'cantonese'],
  assistant_search: ['thorough', 'quick'],
} as const;
export type PersonaKey = keyof typeof PERSONA_CHOICES;
export const MAX_ADDRESS_CHARS = 40;
export const MAX_INSTRUCTIONS_CHARS = 500;

export function personaValue(key: PersonaKey, saved: string | null): string {
  const allowed: readonly string[] = PERSONA_CHOICES[key];
  return saved !== null && allowed.includes(saved) ? saved : allowed[0];
}

const LABELS: Record<string, [string, string]> = {
  jarvis: ['JARVIS: calm, lightly witty', 'JARVIS：冷靜、帶點幽默'],
  professional: ['Professional and concise', '專業簡潔'],
  friendly: ['Relaxed and friendly', '輕鬆友善'],
  balanced: ['Balanced', '適中'],
  brief: ['Brief', '簡短'],
  detailed: ['Detailed', '詳細'],
  written: ['Written Chinese', '書面語'],
  cantonese: ['Natural Cantonese', '自然廣東話'],
  thorough: ['Thorough: tries other keywords when a search finds nothing', '徹底：找不到時會換關鍵字再找'],
  quick: ['Quick: one search per source', '快速：每個來源只找一次'],
};

export function personaLabel(value: string, text: Text): string {
  const label = LABELS[value];
  return label ? text(label[0], label[1]) : value;
}
