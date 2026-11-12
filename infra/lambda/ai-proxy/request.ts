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
  /** UTF-8 bytes of the raw body (the name predates the byte rule); input to estimateUsage. */
  requestChars: number;
}

const MAX_MESSAGES = 200;
const MAX_TOOLS = 16;
const TOOL_WORKLOADS: readonly Workload[] = ['chat', 'voice'];
const BLOCK_KEYS = ['text', 'toolUse', 'toolResult'];
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

// Bedrock also allows image, document and video tool-result content (some with s3Location); only text and JSON are accepted.
function validToolResultItem(item: unknown): boolean {
  if (!isObject(item)) return false;
  const keys = Object.keys(item);
  if (keys.length !== 1) return false;
  return keys[0] === 'text' ? typeof item.text === 'string' : keys[0] === 'json' && isObject(item.json);
}

function validBlock(block: unknown): boolean {
  if (!isObject(block)) return false;
  const keys = Object.keys(block);
  if (keys.length !== 1 || !BLOCK_KEYS.includes(keys[0])) return false;
  if (keys[0] !== 'toolResult') return true;
  const { toolResult } = block;
  return isObject(toolResult) && Array.isArray(toolResult.content) && toolResult.content.length > 0 && toolResult.content.every(validToolResultItem);
}

// Bedrock also allows cachePoint and system tools in the tool list (on some models); only toolSpec entries are accepted.
function validTool(tool: unknown): boolean {
  if (!isObject(tool)) return false;
  const keys = Object.keys(tool);
  return keys.length === 1 && keys[0] === 'toolSpec' && isObject(tool.toolSpec);
}

function hasToolBlock(messages: unknown[]): boolean {
  return messages.some(m => (m as { content: Record<string, unknown>[] }).content.some(b => 'toolUse' in b || 'toolResult' in b));
}

function validMessage(message: unknown): boolean {
  if (!isObject(message) || (message.role !== 'user' && message.role !== 'assistant')) return false;
  const { content } = message;
  if (!Array.isArray(content) || content.length === 0) return false;
  return content.every(validBlock);
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
  if (tools !== undefined) {
    if (!TOOL_WORKLOADS.includes(workload as Workload)) return null;
    if (!isObject(tools) || !Array.isArray(tools.tools) || tools.tools.length === 0 || tools.tools.length > MAX_TOOLS) return null;
    if (!tools.tools.every(validTool)) return null;
  }
  // Bedrock rejects tool blocks without a tool configuration, which would otherwise surface as a 502.
  if (tools === undefined && hasToolBlock(messages)) return null;

  const wanted = Number.isSafeInteger(maxTokens) && (maxTokens as number) > 0 ? (maxTokens as number) : DEFAULT_MAX_TOKENS;
  return {
    workload: workload as Workload,
    system: system === undefined ? null : system,
    messages,
    tools: tools === undefined ? null : (tools as Record<string, unknown>),
    maxTokens: Math.min(wanted, MAX_TOKENS),
    requestId,
    requestChars: Buffer.byteLength(raw, 'utf8'),
  };
}
