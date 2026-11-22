import { useState } from 'react';
import { Mic } from 'lucide-react';
import Button from '../../ui/Button';
import { useChatContext } from '../../hooks/ChatContext';
import { useUiLocale } from '../../i18n';
import { chatStatusText } from '../../lib/chat-status';
import type { VoiceState } from '../../lib/types';
import type { AssistantActivity } from '../useAssistantActivity';

/**
 * The home page's way in to chat: what is typed becomes a new task. The mic
 * is the same push-to-talk as before (onVoice, also Cmd+Shift+J).
 */
export default function AskBar({ onAsk, onVoice, listening, voiceState, activity, placeholder }: {
  onAsk(prompt: string): void; onVoice(): void; listening: boolean; voiceState: VoiceState; activity: AssistantActivity; placeholder: string;
}) {
  const { text } = useUiLocale();
  const chat = useChatContext();
  const [value, setValue] = useState('');
  const working = listening || activity.state !== 'idle';
  const downloading = typeof voiceState === 'object' && 'ModelDownloading' in voiceState ? voiceState.ModelDownloading : null;
  const status = listening ? text('Listening · Press ⌘⇧J to finish', '聆聽中 · 按 ⌘⇧J 完成')
    : downloading !== null ? text(`Downloading speech model ${downloading}%`, `正在下載語音模型 ${downloading}%`)
    : activity.status ? chatStatusText(activity.status, text) : '';

  function submit() {
    const prompt = value.trim();
    if (!prompt) return;
    onAsk(prompt);
    setValue('');
  }

  return <div className="home-ask-wrap">
    <form className="home-ask" onSubmit={e => { e.preventDefault(); submit(); }}>
      <span className={`home-orb${working ? ' is-working' : ''}`} aria-hidden="true" />
      <input className="home-ask-input" type="text" value={value} placeholder={placeholder} aria-label={text('Ask JARVIS', '向 JARVIS 提問')}
        onChange={e => setValue(e.target.value)}
        onKeyDown={e => { if (e.key === 'Enter' && (e.nativeEvent.isComposing || e.keyCode === 229)) e.preventDefault(); }} />
      <button type="button" className="home-mic" onClick={onVoice} disabled={chat.busy} aria-pressed={listening}
        aria-label={listening ? text('Finish listening', '結束語音輸入') : text('Speak', '語音輸入')}
        title={listening ? text('Finish listening', '結束語音輸入') : text('Speak (⌘⇧J)', '語音輸入（⌘⇧J）')}>
        <Mic size={18} />
      </button>
      <Button kind="primary" type="submit" onClick={() => {}} disabled={!value.trim()}>{text('Ask', '提問')}</Button>
    </form>
    {status && <p className="home-ask-status" role="status">{status}</p>}
  </div>;
}
