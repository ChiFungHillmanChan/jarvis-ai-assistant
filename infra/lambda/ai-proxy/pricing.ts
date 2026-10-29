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

/**
 * Token estimate used only when the provider reports no usage: three UTF-8 bytes per token, rounded up.
 * Bytes, not characters, so Chinese (three bytes a character) is not undercounted. Callers pass byte lengths.
 */
export function estimateUsage(requestBytes: number, streamedBytes: number): Usage {
  return { inputTokens: Math.ceil(requestBytes / 3), outputTokens: Math.ceil(streamedBytes / 3) };
}
