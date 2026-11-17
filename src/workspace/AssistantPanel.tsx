import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ArrowUp, X, Mic, Plus, Maximize2, VolumeX } from 'lucide-react';
import { useChatContext } from '../hooks/ChatContext';
import { useWorkspace, useWorkspaceValue } from './WorkspaceContext';
import MessageRenderer from '../components/chat/MessageRenderer';
import { stopSpeaking } from '../lib/commands';
import { useUiLocale } from '../i18n';
import { chatStatusText } from '../lib/chat-status';
import { chatErrorText } from '../lib/chat-error';
import { historyStart } from './history';
import { replayableId } from './chatState';
import VoiceReplay from './VoiceReplay';
import ConsentCard from '../components/ConsentCard';
import ModelErrorCard from '../components/ModelErrorCard';

export default function AssistantPanel({ full = false, onClose, onVoice }: { full?: boolean; onClose?: () => void; onVoice?: () => void }) {
  const { text } = useUiLocale();
  const chat = useChatContext();
  const { state, dispatch, newTask } = useWorkspace();
  const [input, setInput] = useWorkspaceValue('assistantDraft', '');
  const [premium, setPremium] = useState(false);
  // "Think harder" is shown only when the model the next turn goes to can
  // think harder (ai::capability::think_harder_offered, from the registry).
  const [thinkHarder, setThinkHarder] = useState(false);
  const [agreed, setAgreed] = useState(false);
  // Asked again after each turn, when the registry changes, and when the
  // composer gets focus (the AI may have been changed in Settings).
  const live = useRef(true);
  const refreshThinkHarder = useCallback(() => void invoke<boolean>('think_harder_offered').then(offered => { if (live.current) { setThinkHarder(offered); if (!offered) setPremium(false); } }).catch(() => {}), []);
  useEffect(() => {
    live.current = true;
    refreshThinkHarder();
    const stop = listen('registry-updated', refreshThinkHarder);
    return () => { live.current = false; void stop.then(unlisten => unlisten()).catch(() => {}); };
  }, [chat.busy, refreshThinkHarder]);
  useEffect(() => setAgreed(false), [chat.problem]);
  const end = useRef<HTMLDivElement>(null);
  const messages = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  // Older messages stay hidden until asked for; the count is fixed once the history loads.
  const [hidden, setHidden] = useState<number | null>(null);
  const start = historyStart(chat.messages.length, hidden);
  useEffect(() => { if (hidden === null && !chat.historyLoading) setHidden(start); }, [hidden, chat.historyLoading, start]);
  useEffect(() => { if (follow.current) end.current?.scrollIntoView({ block: 'nearest', behavior: 'auto' }); }, [chat.messages, chat.streamingText, chat.sessionId]);
  function submit() {
    if (!input.trim() || chat.busy || chat.historyLoading) return;
    const text = input.trim();
    const tabId = `task:${chat.sessionId}`;
    const tab = state.tabs.find(t => t.id === tabId);
    if (tab?.title === 'New task') dispatch({ type: 'rename', id: tabId, title: text.slice(0, 50) });
    void chat.send(text, premium && thinkHarder ? 'premium' : 'tools'); setInput('');
  }
  return <section className={`assistant-panel ${full ? 'assistant-full' : ''}`} aria-label="JARVIS assistant">
    <header className="assistant-header"><span className={`assistant-orb ${chat.loading ? 'is-working' : ''}`} aria-hidden="true" /><div><strong>JARVIS</strong><small>{chat.loading ? text('Working on this task', '正在處理此工作') : text('Your assistant', '你的助理')}</small></div><div className="assistant-header-actions"><button className="icon-button" onClick={() => { const id = newTask(); chat.selectSession(id); }} aria-label={text('Start another task', '開始另一項工作')}><Plus size={17} /></button>{!full && <button className="icon-button" aria-label={text('Open conversation tab', '開啟完整對話分頁')} onClick={() => dispatch({ type: 'open', view: 'chat', id: `task:${chat.sessionId}`, sessionId: chat.sessionId, title: 'Assistant' })}><Maximize2 size={16} /></button>}{onClose && <button className="icon-button" onClick={onClose} aria-label={text('Close assistant', '收起助理')}><X size={17} /></button>}</div></header>
    <div className="assistant-messages" ref={messages} onScroll={() => { const el = messages.current; if (el) follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 70; }}>
      {chat.historyLoading && <p className="workspace-empty">{text('Loading conversation…', '正在載入對話…')}</p>}
      {!chat.historyLoading && !chat.messages.length && <div className="assistant-welcome"><h1>{text('What would you like to get done?', '需要處理甚麼工作？')}</h1><p>{text('Give JARVIS an instruction. Tool results open in their own workspace, and your conversation stays here.', '向 JARVIS 提供工作指示。工具結果會在獨立工作區開啟，對話則保留於此。')}</p><div className="assistant-prompts"><button onClick={() => setInput('What is on my calendar today?')}>{text('Review my calendar', '查看我的日曆')}</button><button onClick={() => setInput('Show the email waiting for a reply in my connected Google account.')}>{text('Find email to follow up', '尋找待跟進郵件')}</button></div><small>{text('Uses the accounts connected to Assistant. Mail workspace accounts are managed separately.', '使用已連接至助理的帳戶。郵件工作區帳戶需另行管理。')}</small></div>}
      {start > 0 && <button type="button" className="text-button assistant-earlier" onClick={() => setHidden(0)}>{text(`Show ${start} earlier message${start === 1 ? '' : 's'}`, `顯示較早的 ${start} 則訊息`)}</button>}
      {chat.messages.slice(start).map((message, i) => <article key={`${message.id ?? 'local'}-${start + i}`} className={`assistant-message ${message.role}`}><div className="message-author">{message.role === 'user' ? text('You', '你') : 'JARVIS'}{replayableId(message) !== null && <VoiceReplay id={replayableId(message)!} />}</div><MessageRenderer content={message.content} />{message.steps && <details className="assistant-steps"><summary>{text(`What I did · ${message.steps.length}`, `處理過程 · ${message.steps.length} 步`)}</summary><ol>{message.steps.map((step, i) => <li key={i}>{chatStatusText(step.status, text, step.detail)}</li>)}</ol></details>}</article>)}
      {chat.currentStatus && <div className="assistant-progress" role="status"><span className="status-dot" />{chatStatusText(chat.currentStatus, text, chat.currentDetail)}</div>}
      {chat.streamingText && <article className="assistant-message assistant"><MessageRenderer content={chat.streamingText} /></article>}
      {chat.problem?.kind === 'failure' && !chat.busy && <ModelErrorCard failure={chat.problem.failure} onRetry={pick => chat.retry(pick)} />}
      {chat.problem?.kind === 'consent' && (agreed
        ? <div className="ai-card" role="status"><p>{text('Agreed. Send the request again to include your email and calendar.', '已同意。再次傳送這個請求，即可納入電郵及日曆內容。')}</p><div className="tool-approval-actions"><button type="button" className="update-toast-action" disabled={chat.busy} onClick={() => void chat.retry(null)}>{text('Send again', '再次傳送')}</button><button type="button" className="tool-approval-decline" onClick={chat.dismissProblem}>{text('Close', '關閉')}</button></div></div>
        : <ConsentCard request={chat.problem.consent} onAgreed={() => setAgreed(true)} onClose={chat.dismissProblem} />)}
      {chat.error && (() => { const shown = chatErrorText(chat.error, text); return <div className="workspace-error" role="alert"><span className="error-text">{shown.message}</span><button className="text-button" onClick={() => void chat.reloadHistory()}>{text('Reload history', '重新載入紀錄')}</button>{shown.detail && <details><summary>{text('Technical details', '技術資料')}</summary>{shown.detail}</details>}</div>; })()}<div ref={end} />
    </div>
    <form className="assistant-composer" onSubmit={e => { e.preventDefault(); submit(); }}><textarea aria-label={text('Message JARVIS', '輸入給 JARVIS 的指示')} rows={2} placeholder={text('Ask JARVIS…', '向 JARVIS 提問…')} value={input} onFocus={refreshThinkHarder} onChange={e => setInput(e.target.value)} onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); submit(); } }} /><div className="assistant-compose-actions">{thinkHarder && <button type="button" className={`reasoning-toggle ${premium ? 'is-active' : ''}`} aria-pressed={premium} onClick={() => setPremium(v => !v)}>{text('Think harder', '深入思考')}</button>}<span className="grow" /><button type="button" className="icon-button" aria-label={text('Stop speaking', '停止朗讀')} onClick={() => void stopSpeaking()}><VolumeX size={16} /></button>{onVoice && <button type="button" className="icon-button" aria-label={text('Talk to JARVIS', '開始語音輸入')} disabled={chat.busy} onClick={onVoice}><Mic size={17} /></button>}<button className="workspace-button primary send-button" type="submit" aria-label={text('Send message', '送出指示')} disabled={!input.trim() || chat.busy || chat.historyLoading}><ArrowUp size={18} /></button></div>{chat.busy && !chat.loading && <small>{text('Another task is running. Your draft is kept.', '另一項工作正在執行；草稿已保留。')}</small>}</form>
  </section>;
}
