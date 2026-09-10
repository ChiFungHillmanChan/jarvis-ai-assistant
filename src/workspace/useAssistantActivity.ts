import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { AiState, AssistantPhase } from '../lib/types';
export function useAssistantActivity() {
  const [state, setState] = useState<AiState>('idle');
  const [phase, setPhase] = useState<AssistantPhase>('idle');
  const [status, setStatus] = useState('');
  const [tool, setTool] = useState<string | null>(null);
  const tts = useRef(0), mic = useRef(0);
  useEffect(() => {
    const listeners = [
      listen<{ state: AiState }>('chat-state', ({ payload }) => { setState(payload.state); if (payload.state === 'idle') { setPhase('idle'); setStatus(''); } }),
      listen<{ phase?: AssistantPhase; status: string }>('chat-status', ({ payload }) => { setStatus(payload.status); if (payload.phase) setPhase(payload.phase); }),
      listen<{ tool_name: string }>('chat-tool-call', ({ payload }) => setTool(payload.tool_name)),
      listen<{ amplitude: number }>('tts-amplitude', ({ payload }) => { tts.current = payload.amplitude; }),
      listen<{ amplitude: number }>('mic-amplitude', ({ payload }) => { mic.current = payload.amplitude; }),
    ];
    return () => { listeners.forEach(p => { void p.then(fn => fn()).catch(() => {}); }); };
  }, []);
  return { state, phase, status, tool, clearTool: () => setTool(null), tts, mic };
}
export type AssistantActivity = ReturnType<typeof useAssistantActivity>;
