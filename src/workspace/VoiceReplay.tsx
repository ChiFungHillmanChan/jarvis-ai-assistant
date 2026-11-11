import { useEffect, useMemo, useState } from 'react';
import { Play, Square } from 'lucide-react';
import { getVoiceRecording } from '../lib/commands';
import { createReplay, type ReplayAudio, type ReplayState } from '../lib/voice-replay';
import { useUiLocale } from '../i18n';

// The WAV is decoded with Web Audio rather than handed to an <audio> element:
// the app's content security policy has no media-src, and it stays that way.
function webAudio(): ReplayAudio {
  const context = new AudioContext();
  return {
    decode: wav => context.decodeAudioData(wav),
    play: (buffer, onEnded) => {
      const source = context.createBufferSource();
      source.buffer = buffer as AudioBuffer;
      source.connect(context.destination);
      source.onended = onEnded;
      source.start();
    },
    close: () => { void context.close().catch(() => {}); },
  };
}

/** Plays back the kept recording of one voice request. */
export default function VoiceReplay({ id }: { id: number }) {
  const { text } = useUiLocale();
  const [state, setState] = useState<ReplayState>('idle');
  const replay = useMemo(() => createReplay(() => getVoiceRecording(id), webAudio, setState), [id]);
  useEffect(() => replay.stop, [replay]);
  const playing = state === 'playing';
  const label = state === 'failed' ? text('Could not play', '未能播放') : playing ? text('Stop', '停止') : text('Play recording', '播放錄音');
  return <button type="button" className={`voice-replay${playing ? ' is-playing' : ''}`} onClick={() => void replay.toggle()}>{playing ? <Square size={10} aria-hidden="true" /> : <Play size={10} aria-hidden="true" />}{label}</button>;
}
