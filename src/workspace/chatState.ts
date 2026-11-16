import type { ChatMessage, ChatStep } from '../lib/types';
export interface EventScope { session_id?: string; run_id?: string }
export function matchesRun(scope: EventScope, run: { sessionId: string; runId: string } | null): boolean {
  return !!run && scope.session_id === run.sessionId && scope.run_id === run.runId;
}
// A voice event can arrive during the initial DB read. Retain it without
// duplicating an entry already included in that read.
export function mergeHistory(saved: ChatMessage[], pending: ChatMessage[]): ChatMessage[] {
  return [...saved, ...pending.filter(m => !saved.some(s => s.role === m.role && s.content === m.content))];
}

/** The id to fetch a message's recording by, when it is a voice request whose recording was kept. */
export function replayableId(message: ChatMessage): number | null {
  return message.role === 'user' && message.has_recording && message.id != null ? message.id : null;
}

/** A message announced by the voice flow (`chat-new-message`), as the chat shows it. */
export function spokenMessage(payload: { role: string; content: string; id?: number | null; has_recording?: boolean }, now: string): ChatMessage {
  return { id: payload.id ?? null, role: payload.role as ChatMessage['role'], content: payload.content, created_at: now, has_recording: payload.has_recording ?? false };
}

/** The steps a run took, for the "what I did" list under its reply: tool runs only, each once. */
export function addStep(steps: ChatStep[], payload: { status: string; detail?: string | null }): ChatStep[] {
  if (!payload.status.startsWith('Running: ')) return steps;
  const step = { status: payload.status, detail: payload.detail ?? null };
  return steps.some(s => s.status === step.status && s.detail === step.detail) ? steps : [...steps, step];
}
// A model round that follows a tool call is a separate reply. Streamed straight
// on, "calendar." and "Tomorrow" ran together; it starts a new paragraph.
export function roundBreak(sofar: string, token: string): string {
  return token.trim() && sofar.trim() && !/\s$/.test(sofar) ? '\n\n' : '';
}
