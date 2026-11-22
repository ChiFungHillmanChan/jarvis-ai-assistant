/**
 * The last error from the AI settings in this session (adding, testing or
 * removing a local endpoint, saving the provider order), for the "Last AI
 * error" row in Advanced > Developer. It replaces the old page's llmError
 * line. Kept in memory only: nothing is written to disk.
 */
export interface AiErrorEntry { message: string; at: number }

let last: AiErrorEntry | null = null;
const listeners = new Set<() => void>();
const notify = () => listeners.forEach(listener => listener());

export function recordAiError(message: string, at = Date.now()): void {
  last = { message, at };
  notify();
}

export function clearAiError(): void {
  last = null;
  notify();
}

export function lastAiError(): AiErrorEntry | null {
  return last;
}

export function subscribeAiErrors(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
