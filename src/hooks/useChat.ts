import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { ChatMessage, ChatStep, ChatTokenPayload, ChatStatusPayload, ChatStatePayload } from '../lib/types';
import { sendMessage, getConversations } from '../lib/commands';
import { addStep, matchesRun, mergeHistory, roundBreak, spokenMessage } from '../workspace/chatState';
import { parseAiError, type AiProblem, type ConsentRequest, type Pick } from '../lib/ai-error';
// problem: the error card or consent card for the last request (P06).
// last: that request, so a pick on the card can run it again.
interface LastRequest { text: string; workload?: 'tools' | 'premium'; runId: string }
interface SessionState { historyLoading: boolean; messages: ChatMessage[]; loading: boolean; error: string | null; problem: AiProblem | null; last: LastRequest | null; currentStatus: string | null; currentDetail: string | null; streamingText: string }
interface Scoped { session_id?: string; run_id?: string }
const unanswered = (messages: ChatMessage[], text: string) => { const last = messages[messages.length - 1]; return last?.role === 'user' && last.content === text; };
const empty = (): SessionState => ({ historyLoading: true, messages: [], loading: false, error: null, problem: null, last: null, currentStatus: null, currentDetail: null, streamingText: '' });
// roundEnded: a status arrived after some text, so the model is on a new round
// (tool call, then more text). Rounds are separate replies and need a break
// between them, or "calendar." and "Tomorrow" run together.
interface Stream { sessionId: string; runId: string; text: string; held: string; buffer: string; steps: ChatStep[]; roundEnded: boolean }

// The selected conversation is a view; an in-flight run always belongs to its origin.
export function useChat() {
  const [sessionId, setSessionId] = useState('legacy');
  const selected = useRef(sessionId); selected.current = sessionId;
  const [sessions, setSessions] = useState<Record<string, SessionState>>({});
  const historyLoads = useRef(new Map<string, Promise<void>>());
  const [runningSessionId, setRunningSessionId] = useState<string | null>(null);
  const stream = useRef<Stream | null>(null);
  const frame = useRef<number | null>(null);
  const update = useCallback((id: string, fn: (s: SessionState) => SessionState) => {
    setSessions(prev => ({ ...prev, [id]: fn(prev[id] ?? empty()) }));
  }, []);
  const hydrate = useCallback((id: string) => {
    const current = historyLoads.current.get(id);
    if (current) return current;
    update(id, s => ({ ...s, historyLoading: true }));
    const request = getConversations(id).then(messages => {
      update(id, s => ({ ...s, historyLoading: false, error: null, messages: mergeHistory(messages, s.messages) }));
    }).catch(e => {
      historyLoads.current.delete(id);
      update(id, s => ({ ...s, historyLoading: false, error: String(e) }));
      throw e;
    });
    historyLoads.current.set(id, request);
    return request;
  }, [update]);
  useEffect(() => { void hydrate(sessionId).catch(() => {}); }, [sessionId, hydrate]);
  useEffect(() => {
    const matches = (payload: Scoped) => matchesRun(payload, stream.current);
    const flush = () => {
      const run = stream.current;
      frame.current = null;
      if (!run || !run.buffer) return;
      run.text += run.buffer; run.buffer = '';
      update(run.sessionId, s => ({ ...s, streamingText: run.text }));
    };
    const tokenListener = listen<ChatTokenPayload & Scoped>('chat-token', ({ payload }) => {
      if (!matches(payload)) return;
      const run = stream.current!;
      if (payload.discard) {
        run.held = run.text + run.buffer; run.text = ''; run.buffer = '';
        if (frame.current !== null) cancelAnimationFrame(frame.current);
        frame.current = null;
        update(run.sessionId, s => ({ ...s, streamingText: '' }));
        return;
      }
      // The reply text is complete. The run can stay open while the voice reply
      // reads it aloud, and "Composing response..." must not stay up meanwhile.
      if (payload.done) { flush(); update(run.sessionId, s => ({ ...s, currentStatus: null, currentDetail: null })); return; }
      if (run.roundEnded && payload.token.trim()) {
        run.roundEnded = false;
        run.buffer += roundBreak(run.text + run.buffer, payload.token);
      }
      run.held = ''; run.buffer += payload.token;
      if (frame.current === null) frame.current = requestAnimationFrame(flush);
    });
    const statusListener = listen<ChatStatusPayload & Scoped>('chat-status', ({ payload }) => {
      if (!matches(payload)) return;
      const run = stream.current!;
      run.steps = addStep(run.steps, payload);
      if ((run.text + run.buffer).trim()) run.roundEnded = true;
      update(payload.session_id!, s => ({ ...s, currentStatus: payload.status, currentDetail: payload.detail ?? null }));
    });
    const stateListener = listen<ChatStatePayload & Scoped>('chat-state', ({ payload }) => {
      if (matches(payload) && payload.state === 'idle') update(payload.session_id!, s => ({ ...s, currentStatus: null, currentDetail: null }));
    });
    // Email or calendar content this run left out until the user agrees
    // (ai/consent.rs): the card goes up in the conversation.
    const consentListener = listen<{ consent: ConsentRequest; feature: string } & Scoped>('chat-consent', ({ payload }) => {
      if (!matches(payload)) return;
      update(payload.session_id!, s => ({ ...s, problem: { kind: 'consent', consent: payload.consent } }));
    });
    const voiceListener = listen<{ role: string; content: string; id?: number | null; has_recording?: boolean } & Scoped>('chat-new-message', ({ payload }) => {
      if (payload.session_id && payload.session_id !== 'legacy') return;
      update('legacy', s => ({ ...s, messages: [...s.messages, spokenMessage(payload, new Date().toISOString())] }));
    });
    return () => { if (frame.current !== null) cancelAnimationFrame(frame.current); [tokenListener, statusListener, stateListener, consentListener, voiceListener].forEach(p => { void p.then(fn => fn()).catch(() => {}); }); };
  }, [update]);
  // retry: the request on the card, sent again (on `pick`, or on the saved
  // choice when null). A message with no answer yet is not added twice; one
  // that was answered (email was held back) is a new turn, as the backend
  // saves it (save_retried_turn).
  const send = useCallback(async (text: string, workload?: 'tools' | 'premium', retry?: { pick: Pick | null; retryOf: string }) => {
    const id = selected.current;
    if (!text.trim()) return;
    if (stream.current) { update(id, s => ({ ...s, error: 'Another assistant task is running. You can keep working here and send this request when it finishes.' })); return; }
    const run: Stream = { sessionId: id, runId: crypto.randomUUID(), text: '', held: '', buffer: '', steps: [], roundEnded: false };
    stream.current = run; setRunningSessionId(id);
    update(id, s => ({ ...s, loading: true }));
    try {
      await hydrate(id);
      update(id, s => ({ ...s, error: null, problem: null, last: { text, workload, runId: run.runId }, streamingText: '', currentStatus: 'Understanding your request…', messages: retry && unanswered(s.messages, text) ? s.messages : [...s.messages, { id: null, role: 'user', content: text, created_at: new Date().toISOString() }] }));
      const response = await sendMessage(text, workload, id, run.runId, retry?.pick ?? undefined, retry?.retryOf);
      update(id, s => ({ ...s, messages: [...s.messages, run.steps.length ? { ...response, steps: run.steps } : response], streamingText: '' }));
    } catch (error) {
      const partial = run.text + run.buffer || run.held;
      const problem = parseAiError(error);
      update(id, s => ({ ...s, error: problem ? null : String(error), problem: problem ?? s.problem, streamingText: partial ? `${partial}\n\n[Incomplete response]` : '' }));
    } finally {
      if (frame.current !== null) cancelAnimationFrame(frame.current);
      frame.current = null;
      stream.current = null; setRunningSessionId(null);
      update(id, s => ({ ...s, loading: false, currentStatus: null, currentDetail: null }));
    }
  }, [update, hydrate]);
  const acceptsRun = useCallback((payload: Scoped) => matchesRun(payload, stream.current), []);
  // The card's pick (or, after the consent card, the saved choice): the
  // last request once more. The backend refuses it if an action already ran.
  const retry = useCallback((pick: Pick | null) => {
    const last = sessions[selected.current]?.last;
    if (!last) return Promise.resolve();
    return send(last.text, last.workload, { pick, retryOf: last.runId });
  }, [sessions, send]);
  const dismissProblem = useCallback(() => update(selected.current, s => ({ ...s, problem: null })), [update]);
  // After the database rows are deleted, drop the cached copy too; a view left
  // on the deleted conversation moves to a fresh, empty one.
  const forget = useCallback((id: string) => {
    historyLoads.current.delete(id);
    setSessions(prev => { const next = { ...prev }; delete next[id]; return next; });
    if (selected.current === id) setSessionId(crypto.randomUUID());
  }, []);
  const current = sessions[sessionId] ?? empty();
  return { ...current, reloadHistory: () => hydrate(sessionId).catch(() => {}), acceptsRun, retry, dismissProblem, sessionId, selectSession: setSessionId, forget, sessions, runningSessionId, busy: runningSessionId !== null, send, aiState: runningSessionId ? 'thinking' as const : 'idle' as const };
}
