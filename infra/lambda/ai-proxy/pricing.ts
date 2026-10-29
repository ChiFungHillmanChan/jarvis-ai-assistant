import type { ModelPrice } from './config';

export interface Usage { inputTokens: number; outputTokens: number }

/** Provider cost in integer micro-dollars, rounded up. Prices are micro-dollars per million tokens. */
export function costMicros(usage: Usage, price: ModelPrice): number {
  return Math.ceil((usage.inputTokens * price.inputMicrosPerMTok + usage.outputTokens * price.outputMicrosPerMTok) / 1e6);
}

/** What the user is charged: cost times the multiplier in basis points (15000 = 1.5x), rounded up. */
export function billedMicros(cost: number, multiplierBps: number): number {
  return Math.ceil((cost * multiplierBps) / 10_000);
}

/** Token estimate used only when the provider reports no usage: two characters per token, rounded up. */
export function estimateUsage(requestChars: number, streamedChars: number): Usage {
  return { inputTokens: Math.ceil(requestChars / 2), outputTokens: Math.ceil(streamedChars / 2) };
}
