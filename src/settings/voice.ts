import type { CurrentAi } from '../lib/ai-choice';
import type { VoiceState } from '../lib/types';
import type { Text } from '../lib/bedrock-models';

// Settings > Voice: the pure parts, kept apart from the hook so node tests can
// import them without Tauri.

/**
 * The English voice is saved under both keys, `tts_voice` last. The router
 * (src-tauri/src/voice/tts_router.rs) reads `tts_voice` first, the speech
 * engine reads it (set_voice_setting also applies it in memory) and the shown
 * value comes from it, so if the first write fails all three keep the old voice.
 */
export const ENGLISH_VOICE_KEYS = ['tts_voice_en', 'tts_voice'] as const;

/** The cloud speech-to-text a spoken request uses (voice::cloud_speech), or null for the model on this Mac. */
export interface CloudSpeech { label: string; consent_needed: boolean }

/** The speech model's download progress, from the voice state the backend emits, or null when none is running. */
export function downloadPercent(state: VoiceState): number | null {
  return typeof state === 'object' && 'ModelDownloading' in state ? state.ModelDownloading : null;
}

/**
 * Mirrors VoiceEngine::is_available in src-tauri/src/voice/mod.rs: the cloud
 * speech-to-text for the chosen AI with its key, or the local Whisper model.
 * Without one of them push-to-talk errors out. Wording from the old page.
 */
export function voiceAvailabilityLine(
  { cloud, waits, local, chosen }: { cloud: string | null; waits: string | null; local: boolean; chosen: Pick<CurrentAi, 'provider' | 'label'> | null },
  text: Text,
): string {
  if (cloud && local) return text(`Speech-to-text ready -- cloud ${cloud}, with the local Whisper model as fallback.`, `語音辨識可用：雲端 ${cloud}，並以本機 Whisper 作後備。`);
  if (cloud) return text(`Speech-to-text ready -- cloud ${cloud}.`, `語音辨識可用：雲端 ${cloud}。`);
  if (local) return text('Speech-to-text ready -- local Whisper model.', '語音辨識可用：本機 Whisper 模型。');
  if (waits) return text(`Your voice goes to ${waits} only after you agree to its data terms in Settings → Your AI. Until then, voice input needs the speech model on this Mac (about 148 MB).`, `你的語音只會在你於「設定 → 你的 AI」同意 ${waits} 的資料條款後才傳送。在此之前，語音輸入需要此 Mac 上的語音模型（約 148 MB）。`);
  return chosen && chosen.provider !== 'gemini'
    ? text(`Voice input uses a speech model on this Mac, so your voice does not go to ${chosen.label}. Download it once (about 148 MB).`, `語音輸入使用此 Mac 上的語音模型，聲音不會傳送至 ${chosen.label}。請先下載一次（約 148 MB）。`)
    : text('Voice input needs a Gemini key (Settings → Your AI) or the speech model on this Mac (about 148 MB).', '語音輸入需要 Gemini 密鑰（設定 → 你的 AI），或此 Mac 上的語音模型（約 148 MB）。');
}

/** The wake-word status line, as the old page wrote it. */
export function wakeStatusLine(state: VoiceState, modelReady: boolean, text: Text): string {
  if (typeof state === 'object' && 'ModelDownloading' in state) return `${text('Downloading local model', '正在下載本機模型')}：${state.ModelDownloading}%`;
  if (typeof state === 'object' && 'Error' in state) return `${text('Error', '錯誤')}：${state.Error}`;
  if (state === 'WakeWordListening') return text('Wake word listening is active', '正在聆聽喚醒詞');
  if (state === 'WakeWordDetected') return text('Wake word detected', '偵測到喚醒詞');
  if (state === 'WakeWordProcessing') return text('Processing wake-word request', '正在處理喚醒詞指示');
  if (state === 'WakeWordSpeaking') return text('Speaking wake-word response', '正在朗讀回覆');
  if (modelReady) return text('Local speech model ready', '本機語音模型已準備好');
  return text('Model required before wake-word listening can start', '啟動喚醒詞聆聽前需要先下載模型');
}

/** Where wake-word audio goes, from the old page. */
export function wakePrivacyLine(cloud: string | null, waits: string | null, text: Text): string {
  if (cloud) return text(`Wake-word detection runs on this Mac. After it wakes, the full request goes to ${cloud}, the speech-to-text of the AI you chose.`, `喚醒詞偵測會在本機處理音訊。喚醒後，完整指示會傳送至 ${cloud}，即你選擇的 AI 的語音辨識。`);
  if (waits) return text(`Wake-word detection runs on this Mac, and so does the full request after it wakes, until you agree to ${waits}'s data terms.`, `喚醒詞偵測會在本機處理音訊；在你同意 ${waits} 的資料條款前，喚醒後的完整指示亦只在此 Mac 處理。`);
  return text('Wake-word detection runs on this Mac, and so does the full request after it wakes: the AI you chose has no cloud speech-to-text in JARVIS.', '喚醒詞偵測會在本機處理音訊，喚醒後的完整指示亦只在此 Mac 處理：你選擇的 AI 在 JARVIS 中沒有雲端語音辨識。');
}

/** A macOS voice as list_tts_voices returns it (src/lib/commands.ts TtsVoice). */
export interface VoiceEntry { name: string; locale: string }

/**
 * Voices hidden from the lists: macOS sound effects and novelty voices, the
 * 1980s voices, and the eight voices Apple ships once per language (Eddy and
 * the rest), which sound robotic and filled the list with repeats. A voice
 * already saved still shows (voiceChoices keeps it).
 */
export const HIDDEN_VOICES = new Set([
  'Albert', 'Bad News', 'Bahh', 'Bells', 'Boing', 'Bubbles', 'Cellos', 'Fred', 'Good News', 'Jester', 'Junior', 'Kathy',
  'Organ', 'Ralph', 'Superstar', 'Trinoids', 'Whisper', 'Wobble', 'Zarvox',
  'Eddy', 'Flo', 'Grandma', 'Grandpa', 'Reed', 'Rocko', 'Sandy', 'Shelley',
]);

const ACCENTS: Record<string, [string, string]> = {
  en_US: ['American English', '美式英語'], en_GB: ['British English', '英式英語'], en_AU: ['Australian English', '澳洲英語'],
  en_IE: ['Irish English', '愛爾蘭英語'], en_ZA: ['South African English', '南非英語'], en_IN: ['Indian English', '印度英語'],
  en_SC: ['Scottish English', '蘇格蘭英語'], en_NZ: ['New Zealand English', '紐西蘭英語'], zh_HK: ['Cantonese', '廣東話'],
};

/** Apple does not say which voices are female or male; these are the ones the lists show by default. */
const GENDERS: Record<string, 'f' | 'm'> = {
  Samantha: 'f', Daniel: 'm', Karen: 'f', Moira: 'f', Tessa: 'f', Rishi: 'm', Aman: 'm', Tara: 'f', Sinji: 'f',
};

/** The name saved and passed to `say -v`: "Daniel (English (UK))" becomes "Daniel"; "Ava (Premium)" stays whole. */
export function voiceValue(name: string): string {
  return name.replace(/ \([^()]*\([^()]*\)\)$/, '');
}

/**
 * The options for one voice list: English voices for 'en', Hong Kong voices
 * for 'yue', each once, hidden voices left out, labelled with accent and,
 * where known, female or male. The saved voice is kept at the top when it is
 * not in the list, so a choice made before this never disappears.
 */
export function voiceChoices(voices: VoiceEntry[], language: 'en' | 'yue', saved: string, text: Text): { value: string; label: string }[] {
  const wanted = language === 'en' ? /^en[_-]/ : /^(zh[_-]HK|yue)/;
  const seen = new Set<string>();
  const options: { value: string; label: string }[] = [];
  for (const voice of voices) {
    const value = voiceValue(voice.name);
    if (!wanted.test(voice.locale) || HIDDEN_VOICES.has(value) || seen.has(value)) continue;
    seen.add(value);
    const accent = ACCENTS[voice.locale.replace('-', '_')];
    const gender = GENDERS[value] === 'f' ? text('female', '女聲') : GENDERS[value] === 'm' ? text('male', '男聲') : '';
    const parts = [accent ? text(accent[0], accent[1]) : voice.locale, gender].filter(Boolean);
    options.push({ value, label: text(`${value} (${parts.join(', ')})`, `${value}（${parts.join('，')}）`) });
  }
  options.sort((a, b) => a.value.localeCompare(b.value));
  if (saved && !seen.has(saved)) options.unshift({ value: saved, label: saved });
  return options;
}

/** Speaking speed in words per minute for `say -r`; 200 is what JARVIS has always used. */
export const SPEECH_RATES = { slow: 160, normal: 200, fast: 240 } as const;
export type SpeechRate = keyof typeof SPEECH_RATES;

/** The speed option nearest a saved rate, so a rate saved some other way still shows one option selected. */
export function speechRateChoice(rate: number): SpeechRate {
  let best: SpeechRate = 'normal';
  for (const key of Object.keys(SPEECH_RATES) as SpeechRate[]) {
    if (Math.abs(SPEECH_RATES[key] - rate) < Math.abs(SPEECH_RATES[best] - rate)) best = key;
  }
  return best;
}
