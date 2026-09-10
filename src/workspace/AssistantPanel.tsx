import { useEffect, useRef, useState } from 'react';
import { ArrowUp, X, Mic, Plus, Maximize2, VolumeX } from 'lucide-react';
import { useChatContext } from '../hooks/ChatContext';
import { useWorkspace, useWorkspaceValue } from './WorkspaceContext';
import MessageRenderer from '../components/chat/MessageRenderer';
import { stopSpeaking } from '../lib/commands';

export default function AssistantPanel({ full = false, onClose, onVoice }: { full?: boolean; onClose?: () => void; onVoice?: () => void }) {
  const chat = useChatContext();
  const { state, dispatch, newTask } = useWorkspace();
  const [input, setInput] = useWorkspaceValue('assistantDraft', '');
  const [premium, setPremium] = useState(false);
  const end = useRef<HTMLDivElement>(null);
  const messages = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  useEffect(() => { if (follow.current) end.current?.scrollIntoView({ block: 'nearest', behavior: 'auto' }); }, [chat.messages, chat.streamingText, chat.sessionId]);
  function submit() {
    if (!input.trim() || chat.busy || chat.historyLoading) return;
    const text = input.trim();
    const tabId = `task:${chat.sessionId}`;
    const tab = state.tabs.find(t => t.id === tabId);
    if (tab?.title === 'New task') dispatch({ type: 'rename', id: tabId, title: text.slice(0, 50) });
    void chat.send(text, premium ? 'premium' : 'tools'); setInput('');
  }
  return <section className={`assistant-panel ${full ? 'assistant-full' : ''}`} aria-label="JARVIS assistant">
    <header className="assistant-header"><span className={`assistant-orb ${chat.loading ? 'is-working' : ''}`} aria-hidden="true" /><div><strong>JARVIS</strong><small>{chat.loading ? 'Working on this task' : 'Your assistant'}</small></div><div className="assistant-header-actions"><button className="icon-button" onClick={() => { const id = newTask(); chat.selectSession(id); }} aria-label="Start another task"><Plus size={17} /></button>{!full && <button className="icon-button" aria-label="Open conversation tab" onClick={() => dispatch({ type: 'open', view: 'chat', id: `task:${chat.sessionId}`, sessionId: chat.sessionId, title: 'Assistant' })}><Maximize2 size={16} /></button>}{onClose && <button className="icon-button" onClick={onClose} aria-label="Close assistant"><X size={17} /></button>}</div></header>
    <div className="assistant-messages" ref={messages} onScroll={() => { const el = messages.current; if (el) follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 70; }}>
      {chat.historyLoading && <p className="workspace-empty">Loading conversation…</p>}
      {!chat.historyLoading && !chat.messages.length && <div className="assistant-welcome"><h1>What would you like to get done?</h1><p>Give JARVIS an instruction. Tool results open in their own workspace, and your conversation stays here.</p><div className="assistant-prompts"><button onClick={() => setInput('What is on my calendar today?')}>Review my calendar</button><button onClick={() => setInput('Show the email waiting for a reply in my connected Google account.')}>Find email to follow up</button></div><small>Uses the accounts connected to Assistant. Mail workspace accounts are managed separately.</small></div>}
      {chat.messages.map((message, index) => <article key={`${message.id ?? 'local'}-${index}`} className={`assistant-message ${message.role}`}><div className="message-author">{message.role === 'user' ? 'You' : 'JARVIS'}</div><MessageRenderer content={message.content} /></article>)}
      {chat.currentStatus && <div className="assistant-progress" role="status"><span className="status-dot" />{chat.currentStatus}</div>}
      {chat.streamingText && <article className="assistant-message assistant"><MessageRenderer content={chat.streamingText} /></article>}
      {chat.error && <p className="workspace-error" role="alert">{chat.error}<button className="text-button" onClick={() => void chat.reloadHistory()}>Reload history</button></p>}<div ref={end} />
    </div>
    <form className="assistant-composer" onSubmit={e => { e.preventDefault(); submit(); }}><textarea aria-label="Message JARVIS" rows={2} placeholder="Ask JARVIS…" value={input} onChange={e => setInput(e.target.value)} onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); submit(); } }} /><div className="assistant-compose-actions"><button type="button" className={`reasoning-toggle ${premium ? 'is-active' : ''}`} aria-pressed={premium} onClick={() => setPremium(v => !v)}>Think harder</button><span className="grow" /><button type="button" className="icon-button" aria-label="Stop speaking" onClick={() => void stopSpeaking()}><VolumeX size={16} /></button>{onVoice && <button type="button" className="icon-button" aria-label="Talk to JARVIS" disabled={chat.busy} onClick={onVoice}><Mic size={17} /></button>}<button className="workspace-button primary send-button" type="submit" aria-label="Send message" disabled={!input.trim() || chat.busy || chat.historyLoading}><ArrowUp size={18} /></button></div>{chat.busy && !chat.loading && <small>Another task is running. Your draft is kept.</small>}</form>
  </section>;
}
