import { WORKLOADS } from './config';
import type { Workload } from './config';

export const DEFAULT_MAX_TOKENS = 2048;
export const MAX_TOKENS = 8192;
export const MAX_BODY_BYTES = 524288;

export interface ConverseRequest {
  workload: Workload;
  system: string | null;
  messages: unknown[];
  tools: Record<string, unknown> | null;
  maxTokens: number;
  requestId: string;
  requestChars: number;
}

const MAX_MESSAGES = 200;
const BLOCK_KEYS = ['text', 'toolUse', 'toolResult'];
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

function validMessage(message: unknown): boolean {
  if (!isObject(message) || (message.role !== 'user' && message.role !== 'assistant')) return false;
  const { content } = message;
  if (!Array.isArray(content) || content.length === 0) return false;
  return content.every((block) => {
    if (!isObject(block)) return false;
    const keys = Object.keys(block);
    return keys.length === 1 && BLOCK_KEYS.includes(keys[0]);
  });
}

/** Returns the validated request, or null for anything outside the contract. Never logs or keeps content. */
export function parseRequest(raw: string): ConverseRequest | null {
  if (Buffer.byteLength(raw, 'utf8') > MAX_BODY_BYTES) return null;
  let body: unknown;
  try { body = JSON.parse(raw); } catch { return null; }
  if (!isObject(body)) return null;

  const { workload, system, messages, tools, maxTokens, requestId } = body;
  if (typeof workload !== 'string' || !WORKLOADS.includes(workload as Workload)) return null;
  if (typeof requestId !== 'string' || !UUID.test(requestId)) return null;
  if (!Array.isArray(messages) || messages.length < 1 || messages.length > MAX_MESSAGES || !messages.every(validMessage)) return null;
  if (system !== undefined && typeof system !== 'string') return null;
  if (tools !== undefined && (!isObject(tools) || !Array.isArray(tools.tools) || tools.tools.length === 0)) return null;

  const wanted = Number.isSafeInteger(maxTokens) && (maxTokens as number) > 0 ? (maxTokens as number) : DEFAULT_MAX_TOKENS;
  return {
    workload: workload as Workload,
    system: system === undefined ? null : system,
    messages,
    tools: tools === undefined ? null : (tools as Record<string, unknown>),
    maxTokens: Math.min(wanted, MAX_TOKENS),
    requestId,
    requestChars: raw.length,
  };
}
