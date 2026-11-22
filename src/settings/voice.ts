import type { CurrentAi } from '../lib/ai-choice';
import type { VoiceState } from '../lib/types';
import type { Text } from '../lib/bedrock-models';

// Settings > Voice: the pure parts, kept apart from the hook so node tests can
// import them without Tauri.

/**
 * The English voice is saved under both keys: the router that speaks acks and
 * the briefing reads `tts_voice_en` (src-tauri/src/voice/tts_router.rs), and
 * the speech engine reads `tts_voice`, which set_voice_setting also applies
 * in memory.
 */
export const ENGLISH_VOICE_KEYS = ['tts_voice', 'tts_voice_en'] as const;

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
