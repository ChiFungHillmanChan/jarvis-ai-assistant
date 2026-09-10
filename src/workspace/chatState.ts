import type { ChatMessage } from '../lib/types';
export interface EventScope { session_id?: string; run_id?: string }
export function matchesRun(scope: EventScope, run: { sessionId: string; runId: string } | null): boolean {
  return !!run && scope.session_id === run.sessionId && scope.run_id === run.runId;
}
// A voice event can arrive during the initial DB read. Retain it without
// duplicating an entry already included in that read.
export function mergeHistory(saved: ChatMessage[], pending: ChatMessage[]): ChatMessage[] {
  return [...saved, ...pending.filter(m => !saved.some(s => s.role === m.role && s.content === m.content))];
}
