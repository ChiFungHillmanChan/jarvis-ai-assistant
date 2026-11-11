import { useEffect, useRef, useState } from 'react';
import { Play, Square } from 'lucide-react';
import { getVoiceRecording } from '../lib/commands';
import { useUiLocale } from '../i18n';

/**
 * Plays back the kept recording of one voice request.
 *
 * The WAV is decoded with Web Audio rather than handed to an <audio> element:
 * the app's content security policy has no media-src, and it stays that way.
 */
export default function VoiceReplay({ id }: { id: number }) {
  const { text } = useUiLocale();
  const [playing, setPlaying] = useState(false);
  const [failed, setFailed] = useState(false);
  const context = useRef<AudioContext | null>(null);
  const starting = useRef(false);
  const stop = () => {
    const current = context.current;
    context.current = null;
    if (current) void current.close().catch(() => {});
    setPlaying(false);
  };
  useEffect(() => stop, []);
  async function toggle() {
    if (context.current) { stop(); return; }
    if (starting.current) return;
    starting.current = true;
    setFailed(false);
    try {
      const wav = await getVoiceRecording(id);
      const audio = new AudioContext();
      context.current = audio;
      const source = audio.createBufferSource();
      source.buffer = await audio.decodeAudioData(wav);
      source.connect(audio.destination);
      source.onended = () => { if (context.current === audio) stop(); };
      source.start();
      setPlaying(true);
    } catch (e) {
      console.warn('voice recording playback failed', e);
      stop();
      setFailed(true);
    } finally {
      starting.current = false;
    }
  }
  const label = failed ? text('Could not play', '未能播放') : playing ? text('Stop', '停止') : text('Play recording', '播放錄音');
  return <button type="button" className={`voice-replay${playing ? ' is-playing' : ''}`} onClick={() => void toggle()}>{playing ? <Square size={10} aria-hidden="true" /> : <Play size={10} aria-hidden="true" />}{label}</button>;
}
