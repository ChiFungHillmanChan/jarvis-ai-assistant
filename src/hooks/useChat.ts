import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { ChatMessage, ChatStep, ChatTokenPayload, ChatStatusPayload, ChatStatePayload } from '../lib/types';
import { sendMessage, getConversations } from '../lib/commands';
import { addStep, matchesRun, mergeHistory, roundBreak, spokenMessage } from '../workspace/chatState';
interface SessionState { historyLoading: boolean; messages: ChatMessage[]; loading: boolean; error: string | null; currentStatus: string | null; currentDetail: string | null; streamingText: string }
interface Scoped { session_id?: string; run_id?: string }
const empty = (): SessionState => ({ historyLoading: true, messages: [], loading: false, error: null, currentStatus: null, currentDetail: null, streamingText: '' });
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
    const voiceListener = listen<{ role: string; content: string; id?: number | null; has_recording?: boolean } & Scoped>('chat-new-message', ({ payload }) => {
      if (payload.session_id && payload.session_id !== 'legacy') return;
      update('legacy', s => ({ ...s, messages: [...s.messages, spokenMessage(payload, new Date().toISOString())] }));
    });
    return () => { if (frame.current !== null) cancelAnimationFrame(frame.current); [tokenListener, statusListener, stateListener, voiceListener].forEach(p => { void p.then(fn => fn()).catch(() => {}); }); };
  }, [update]);
  const send = useCallback(async (text: string, workload?: 'tools' | 'premium') => {
    const id = selected.current;
    if (!text.trim()) return;
    if (stream.current) { update(id, s => ({ ...s, error: 'Another assistant task is running. You can keep working here and send this request when it finishes.' })); return; }
    const run: Stream = { sessionId: id, runId: crypto.randomUUID(), text: '', held: '', buffer: '', steps: [], roundEnded: false };
    stream.current = run; setRunningSessionId(id);
    update(id, s => ({ ...s, loading: true }));
    try {
      await hydrate(id);
      update(id, s => ({ ...s, error: null, streamingText: '', currentStatus: 'Understanding your request…', messages: [...s.messages, { id: null, role: 'user', content: text, created_at: new Date().toISOString() }] }));
      const response = await sendMessage(text, workload, id, run.runId);
      update(id, s => ({ ...s, messages: [...s.messages, run.steps.length ? { ...response, steps: run.steps } : response], streamingText: '' }));
    } catch (error) {
      const partial = run.text + run.buffer || run.held;
      update(id, s => ({ ...s, error: String(error), streamingText: partial ? `${partial}\n\n[Incomplete response]` : '' }));
    } finally {
      if (frame.current !== null) cancelAnimationFrame(frame.current);
      frame.current = null;
      stream.current = null; setRunningSessionId(null);
      update(id, s => ({ ...s, loading: false, currentStatus: null, currentDetail: null }));
    }
  }, [update, hydrate]);
  const acceptsRun = useCallback((payload: Scoped) => matchesRun(payload, stream.current), []);
  // After the database rows are deleted, drop the cached copy too; a view left
  // on the deleted conversation moves to a fresh, empty one.
  const forget = useCallback((id: string) => {
    historyLoads.current.delete(id);
    setSessions(prev => { const next = { ...prev }; delete next[id]; return next; });
    if (selected.current === id) setSessionId(crypto.randomUUID());
  }, []);
  const current = sessions[sessionId] ?? empty();
  return { ...current, reloadHistory: () => hydrate(sessionId).catch(() => {}), acceptsRun, sessionId, selectSession: setSessionId, forget, sessions, runningSessionId, busy: runningSessionId !== null, send, aiState: runningSessionId ? 'thinking' as const : 'idle' as const };
}
