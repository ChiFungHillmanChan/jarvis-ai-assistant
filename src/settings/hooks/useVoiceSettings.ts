import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { disableWakeWord, enableWakeWord, getPreference, getVoiceSettings, getWakeWordStatus, listTtsVoices, setPreference, setVoiceSetting, type TtsVoice } from '../../lib/commands';
import { currentAi, type CurrentAi } from '../../lib/ai-choice';
import { voiceStateKind } from '../../lib/always-listening';
import { useVoiceState } from '../../hooks/useVoiceState';
import type { VoiceSettings, WakeWordStatus } from '../../lib/types';
import type { CloudSpeech } from '../voice';

/**
 * Settings > Voice: the saved voice settings, the speech-to-text language,
 * the voices on this Mac, wake-word status and the cloud speech service, with
 * the commands the old Settings page used. The Cantonese voice is read
 * through get_voice_settings, the same command family that saves it.
 */
export function useVoiceSettings() {
  const { state } = useVoiceState();
  const [settings, setSettings] = useState<VoiceSettings | null>(null);
  const [stt, setStt] = useState('auto');
  const [voices, setVoices] = useState<TtsVoice[]>([]);
  const [wake, setWake] = useState<WakeWordStatus | null>(null);
  const [wakeBusy, setWakeBusy] = useState(false);
  const [speech, setSpeech] = useState<CloudSpeech | null>(null);
  const [chosen, setChosen] = useState<CurrentAi | null>(null);
  const [loadError, setLoadError] = useState(false);

  const load = useCallback(async () => {
    setLoadError(false);
    try {
      // V14 seeds stt_language; a missing row reads as the default.
      const [saved, language] = await Promise.all([getVoiceSettings(), getPreference('stt_language').catch(() => '')]);
      setSettings(saved);
      setStt(language || 'auto');
    } catch (e) {
      console.error(e);
      setLoadError(true);
    }
  }, []);

  useEffect(() => {
    void load();
    listTtsVoices().then(setVoices).catch(() => setVoices([]));
    currentAi().then(setChosen).catch(() => {});
  }, [load]);

  useEffect(() => {
    const refresh = () => void invoke<CloudSpeech | null>('cloud_speech').then(setSpeech).catch(() => setSpeech(null));
    refresh();
    const stop = listen('ai-consent-changed', refresh);
    return () => { void stop.then(unlisten => unlisten()).catch(() => {}); };
  }, []);

  const refreshWake = useCallback(() => getWakeWordStatus().then(setWake).catch(() => {}), []);
  // A download or the home switch can change listening while this is open:
  // re-read whenever the voice state changes kind.
  const kind = voiceStateKind(state);
  useEffect(() => { void refreshWake(); }, [kind, refreshWake]);

  /** Saves voice keys in order. The shown value moves at once and goes back to what it was if a write fails. */
  async function saveVoice<K extends keyof VoiceSettings>(field: K, value: VoiceSettings[K], writes: [key: string, value: string][]) {
    const before = settings?.[field];
    setSettings(s => s ? { ...s, [field]: value } : s);
    try {
      for (const [key, v] of writes) await setVoiceSetting(key, v);
    } catch (e) {
      setSettings(s => s && before !== undefined ? { ...s, [field]: before } : s);
      throw e;
    }
  }

  async function saveStt(next: string) {
    const before = stt;
    setStt(next);
    try { await setPreference('stt_language', next); }
    catch (e) { setStt(before); throw e; }
  }

  async function toggleWake(on: boolean) {
    setWakeBusy(true);
    try {
      if (on) await enableWakeWord(); else await disableWakeWord();
    } finally {
      await refreshWake();
      setWakeBusy(false);
    }
  }

  const cloud = speech && !speech.consent_needed ? speech.label : null;
  const waits = speech?.consent_needed ? speech.label : null;
  return { state, settings, stt, voices, wake, wakeBusy, cloud, waits, chosen, loadError, load, refreshWake, saveVoice, saveStt, toggleWake };
}
