import { useEffect, useRef, useState } from 'react';
import { ArrowUp, X, Mic, Plus, Maximize2, VolumeX } from 'lucide-react';
import { useChatContext } from '../hooks/ChatContext';
import { useWorkspace, useWorkspaceValue } from './WorkspaceContext';
import MessageRenderer from '../components/chat/MessageRenderer';
import { stopSpeaking } from '../lib/commands';
import { useUiLocale } from '../i18n';

export default function AssistantPanel({ full = false, onClose, onVoice }: { full?: boolean; onClose?: () => void; onVoice?: () => void }) {
  const { text } = useUiLocale();
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
    <header className="assistant-header"><span className={`assistant-orb ${chat.loading ? 'is-working' : ''}`} aria-hidden="true" /><div><strong>JARVIS</strong><small>{chat.loading ? text('Working on this task', '處理緊呢項工作') : text('Your assistant', '你嘅助手')}</small></div><div className="assistant-header-actions"><button className="icon-button" onClick={() => { const id = newTask(); chat.selectSession(id); }} aria-label={text('Start another task', '開始另一項工作')}><Plus size={17} /></button>{!full && <button className="icon-button" aria-label={text('Open conversation tab', '開啟完整對話分頁')} onClick={() => dispatch({ type: 'open', view: 'chat', id: `task:${chat.sessionId}`, sessionId: chat.sessionId, title: 'Assistant' })}><Maximize2 size={16} /></button>}{onClose && <button className="icon-button" onClick={onClose} aria-label={text('Close assistant', '收起助手')}><X size={17} /></button>}</div></header>
    <div className="assistant-messages" ref={messages} onScroll={() => { const el = messages.current; if (el) follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 70; }}>
      {chat.historyLoading && <p className="workspace-empty">{text('Loading conversation…', '載入對話緊…')}</p>}
      {!chat.historyLoading && !chat.messages.length && <div className="assistant-welcome"><h1>{text('What would you like to get done?', '你想處理咩工作？')}</h1><p>{text('Give JARVIS an instruction. Tool results open in their own workspace, and your conversation stays here.', '交代工作俾 JARVIS。工具結果會喺獨立工作區開啟，對話會留喺呢度。')}</p><div className="assistant-prompts"><button onClick={() => setInput('What is on my calendar today?')}>{text('Review my calendar', '查看我嘅日曆')}</button><button onClick={() => setInput('Show the email waiting for a reply in my connected Google account.')}>{text('Find email to follow up', '搵待跟進郵件')}</button></div><small>{text('Uses the accounts connected to Assistant. Mail workspace accounts are managed separately.', '使用已連接到助手嘅帳戶。郵件工作區帳戶另行管理。')}</small></div>}
      {chat.messages.map((message, index) => <article key={`${message.id ?? 'local'}-${index}`} className={`assistant-message ${message.role}`}><div className="message-author">{message.role === 'user' ? text('You', '你') : 'JARVIS'}</div><MessageRenderer content={message.content} /></article>)}
      {chat.currentStatus && <div className="assistant-progress" role="status"><span className="status-dot" />{chat.currentStatus}</div>}
      {chat.streamingText && <article className="assistant-message assistant"><MessageRenderer content={chat.streamingText} /></article>}
      {chat.error && <p className="workspace-error" role="alert">{chat.error}<button className="text-button" onClick={() => void chat.reloadHistory()}>{text('Reload history', '重新載入紀錄')}</button></p>}<div ref={end} />
    </div>
    <form className="assistant-composer" onSubmit={e => { e.preventDefault(); submit(); }}><textarea aria-label={text('Message JARVIS', '輸入俾 JARVIS 嘅指示')} rows={2} placeholder={text('Ask JARVIS…', '問 JARVIS…')} value={input} onChange={e => setInput(e.target.value)} onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); submit(); } }} /><div className="assistant-compose-actions"><button type="button" className={`reasoning-toggle ${premium ? 'is-active' : ''}`} aria-pressed={premium} onClick={() => setPremium(v => !v)}>{text('Think harder', '深入思考')}</button><span className="grow" /><button type="button" className="icon-button" aria-label={text('Stop speaking', '停止朗讀')} onClick={() => void stopSpeaking()}><VolumeX size={16} /></button>{onVoice && <button type="button" className="icon-button" aria-label={text('Talk to JARVIS', '開始語音輸入')} disabled={chat.busy} onClick={onVoice}><Mic size={17} /></button>}<button className="workspace-button primary send-button" type="submit" aria-label={text('Send message', '送出指示')} disabled={!input.trim() || chat.busy || chat.historyLoading}><ArrowUp size={18} /></button></div>{chat.busy && !chat.loading && <small>{text('Another task is running. Your draft is kept.', '另一項工作執行緊；你嘅草稿已保留。')}</small>}</form>
  </section>;
}
