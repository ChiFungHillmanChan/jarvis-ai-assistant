// Playing a kept voice recording back (src/workspace/VoiceReplay.tsx). The
// sequencing lives here so pressing twice, leaving mid-way and failures can be
// tested without a browser.

export type ReplayState = 'idle' | 'playing' | 'failed';

/** The part of Web Audio a replay needs. */
export interface ReplayAudio {
  decode(wav: ArrayBuffer): Promise<unknown>;
  play(buffer: unknown, onEnded: () => void): void;
  close(): void;
}

export function createReplay(load: () => Promise<ArrayBuffer>, newAudio: () => ReplayAudio, onChange: (state: ReplayState) => void) {
  // Every start and stop takes a new number, so a step that resumes after an
  // await can tell it has been stopped or replaced.
  let run = 0;
  let busy = false;
  let audio: ReplayAudio | null = null;
  const stop = (state: ReplayState = 'idle') => {
    run++; busy = false;
    audio?.close(); audio = null;
    onChange(state);
  };
  async function toggle() {
    if (busy) { stop(); return; }
    const mine = ++run;
    busy = true;
    onChange('idle');
    try {
      const wav = await load();
      if (run !== mine) return;
      const current = audio = newAudio();
      const buffer = await current.decode(wav);
      if (run !== mine) return;
      current.play(buffer, () => { if (run === mine) stop(); });
      onChange('playing');
    } catch (e) {
      console.warn('voice recording playback failed', e);
      if (run === mine) stop('failed');
    }
  }
  return { toggle, stop: () => stop() };
}
