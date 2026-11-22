import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { ROWS } from '../src/settings/rows.ts';
import { ENGLISH_VOICE_KEYS, HIDDEN_VOICES, SPEECH_RATES, downloadPercent, speechRateChoice, voiceAvailabilityLine, voiceChoices, voiceValue, wakePrivacyLine, wakeStatusLine } from '../src/settings/voice.ts';
import { normalizeBriefingTime } from '../src/settings/briefing.ts';

// Task 14: Settings > Daily briefing and Voice. The helpers are pure and
// imported; the JSX cannot be imported under node, so its wiring is checked
// in the source.

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const VOICE = '../src/settings/sections/Voice.tsx';
const BRIEFING = '../src/settings/sections/Briefing.tsx';
const HOOK = '../src/settings/hooks/useVoiceSettings.ts';
const PURE = '../src/settings/voice.ts';
const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

test('the dialog shows Daily briefing and Voice through their own components', () => {
  const dialog = source('../src/settings/SettingsDialog.tsx');
  assert.match(dialog, /import Briefing from '\.\/sections\/Briefing'/);
  assert.match(dialog, /import Voice from '\.\/sections\/Voice'/);
  assert.match(dialog, /\n {2}briefing: Briefing,/);
  assert.match(dialog, /\n {2}voice: Voice,/);
});

for (const [section, file, expected] of [
  ['briefing', BRIEFING, ['briefing-enabled', 'briefing-time', 'briefing-aloud']],
  ['voice', VOICE, ['stt-language', 'voice-input-status', 'speech-model', 'tts-enabled', 'tts-voice', 'tts-voice-yue', 'tts-rate', 'keep-recordings', 'wake-word', 'wake-status', 'wake-privacy']],
] as const) {
  test(`${section} renders its rows, in order, by row id`, () => {
    const code = source(file);
    const ids = ROWS.filter(r => r.section === section).map(r => r.id);
    assert.deepEqual(ids, expected);
    let last = -1;
    for (const id of ids) {
      const at = code.indexOf(`row('${id}')`);
      assert.ok(at > last, `row ${id} is missing or out of order`);
      last = at;
    }
  });
}

test('Voice has one speech-model download call, with Download again in both languages', () => {
  const code = source(VOICE);
  assert.equal(code.split('downloadModel(').length - 1, 1, 'one call of the download command');
  assert.match(code, /text\('Download again', '重新下載'\)/);
  assert.match(code, /downloadPercent\(/, 'progress comes from the voice state, so it survives closing the dialog');
});

test('the Cantonese voice is read through get_voice_settings, never through a preference read', () => {
  const hook = source(HOOK);
  const code = source(VOICE);
  assert.match(hook, /getVoiceSettings\(\)/);
  assert.match(code, /settings\.tts_voice_yue/);
  for (const file of [hook, code]) assert.doesNotMatch(file, /Preference\('tts_voice_yue'\)/);
  assert.match(code, /'tts_voice_yue'/, 'saved with set_voice_setting, as the old page did');
  assert.match(source('../src/lib/types.ts'), /tts_voice_yue: string;/);
});

test('the English voice is written to tts_voice_en first and tts_voice last', () => {
  // The router reads tts_voice first, and the engine and the shown value follow
  // tts_voice: a failed first write leaves all three on the old voice.
  assert.deepEqual([...ENGLISH_VOICE_KEYS], ['tts_voice_en', 'tts_voice']);
  assert.match(source(VOICE), /ENGLISH_VOICE_KEYS/);
});

test('turning Keep recordings off asks first in both languages; turning it on does not', () => {
  const code = source(VOICE);
  assert.match(code, /runAction\(next \? 'secondary' : 'danger'/);
  assert.match(code, /confirmAction/);
  assert.match(code, /keep_voice_recordings/);
  assert.match(code, /deletes every recording already kept/);
  assert.match(code, /已保留的所有錄音/);
});

test('a failed voice save puts the shown value back', () => {
  const hook = source(HOOK);
  const save = hook.slice(hook.indexOf('async function saveVoice'));
  const caught = save.indexOf('catch');
  assert.ok(caught > 0 && save.indexOf('before', caught) > caught, 'restores the value from before the change');
});

test('Voice has no reply-language row: General owns it', () => {
  for (const file of [source(VOICE), source(HOOK), source(PURE)]) {
    assert.doesNotMatch(file, /language_follows_ui/);
    assert.doesNotMatch(file, /'language'/);
  }
  assert.match(source(HOOK), /'stt_language'/);
});

test('download progress is read from the voice state', () => {
  assert.equal(downloadPercent({ ModelDownloading: 42 }), 42);
  assert.equal(downloadPercent('Idle'), null);
  assert.equal(downloadPercent({ Error: 'x' }), null);
});

test('the voice input line names the cloud speech, the local model, or what is missing', () => {
  assert.match(voiceAvailabilityLine({ cloud: 'Gemini', waits: null, local: true, chosen: null }, en), /cloud Gemini, with the local Whisper model as fallback/);
  assert.match(voiceAvailabilityLine({ cloud: null, waits: null, local: true, chosen: null }, en), /local Whisper model/);
  assert.match(voiceAvailabilityLine({ cloud: null, waits: 'Gemini', local: false, chosen: null }, en), /agree to its data terms/);
  assert.match(voiceAvailabilityLine({ cloud: null, waits: null, local: false, chosen: { provider: 'openai', label: 'OpenAI' } }, en), /does not go to OpenAI/);
  assert.match(voiceAvailabilityLine({ cloud: null, waits: null, local: false, chosen: null }, zh), /語音模型/);
});

test('the wake status line and privacy text keep the old page wording', () => {
  assert.match(wakeStatusLine({ ModelDownloading: 12 }, false, en), /12%/);
  assert.equal(wakeStatusLine('WakeWordListening', true, en), 'Wake word listening is active');
  assert.equal(wakeStatusLine('Idle', true, en), 'Local speech model ready');
  assert.equal(wakeStatusLine('Idle', false, en), 'Model required before wake-word listening can start');
  assert.match(wakePrivacyLine('Gemini', null, en), /full request goes to Gemini/);
  assert.match(wakePrivacyLine(null, 'Gemini', en), /until you agree to Gemini's data terms/);
  assert.match(wakePrivacyLine(null, null, en), /has no cloud speech-to-text/);
});

test('a briefing time is saved as HH:MM, and anything else is refused', () => {
  assert.equal(normalizeBriefingTime('08:00'), '08:00');
  assert.equal(normalizeBriefingTime(' 8:05 '), '08:05');
  assert.equal(normalizeBriefingTime('23:59'), '23:59');
  for (const bad of ['', '24:00', '7:60', '0800', 'eight', '08:00:00']) assert.equal(normalizeBriefingTime(bad), null, bad);
});

test('Briefing saves through the daily briefing command and shows what the backend returns', () => {
  const code = source(BRIEFING);
  assert.match(code, /getDailyBriefingSettings\(\)/);
  assert.match(code, /setBriefing\(await setDailyBriefingSettings\(/);
  assert.match(code, /normalizeBriefingTime\(/);
});

test('General shows only the startup part; the briefing is only in its own section', () => {
  assert.match(source('../src/settings/sections/General.tsx'), /<BackgroundSettings \/>/);
  const background = source('../src/components/BackgroundSettings.tsx');
  assert.doesNotMatch(background, /DailyBriefing|dailyBriefing/, 'the briefing half went with the old page');
  assert.match(background, /getOpenAtLogin\(\), getKeepAwake\(\)/);
  assert.match(source(BRIEFING), /getDailyBriefingSettings\(\)/);
});

test('Briefing says so when a change is still saving, and shows a normalised time as saved', () => {
  const code = source(BRIEFING);
  assert.match(code, /Still saving the last change\. Try again in a moment\./);
  assert.match(code, /仍在儲存上一項更改，請稍後再試。/);
  assert.doesNotMatch(code, /throw new Error\('not ready'\)/);
  // "8:30" when 08:30 is saved: nothing to save, but the field shows 08:30.
  assert.match(code, /key=\{timeShown\}/);
  assert.match(code, /setTimeShown\(/);
});

test('ChooseAi points at Settings → Voice', () => {
  const code = source('../src/components/ChooseAi.tsx');
  assert.ok(code.includes('Settings → Voice'));
  assert.ok(!code.includes('Settings → Speech'));
});

test('the preview answers every voice command these sections call', () => {
  const preview = source('../src/dev/workspacePreview.ts');
  for (const command of ['get_voice_settings', 'set_voice_setting', 'list_tts_voices', 'preview_tts_voice', 'cloud_speech', 'download_model', 'get_wake_word_status', 'current_ai']) {
    assert.ok(preview.includes(`case '${command}'`), command);
  }
  assert.doesNotMatch(preview, /case 'get_voice_settings': return null/);
});

test('Briefing and Voice use the shared controls and written Chinese', () => {
  const files = [source(VOICE), source(BRIEFING), source(HOOK), source(PURE)];
  for (const code of files) assert.doesNotMatch(code, /<select|<input/);
  const chinese = files.flatMap(code => [...code.matchAll(/text\(\s*[`'][^`']*[`'],\s*[`']([^`']*)[`']\s*\)/g)].map(m => m[1])).join('');
  assert.ok(chinese.length > 0);
  assert.doesNotMatch(chinese, /[嘅咗唔係冇啲喺嚟]/);
});

test('each new file stays small', () => {
  for (const file of [VOICE, BRIEFING, HOOK, PURE]) assert.ok(source(file).split('\n').length <= 250, file);
});

// Voice lists: say -v ? names a voice once per language and the old list kept
// only the first word, so "Eddy" showed 14 times and "Bad News" as "Bad".
const MAC_VOICES = [
  { name: 'Bad News', locale: 'en_US' }, { name: 'Daniel (English (UK))', locale: 'en_GB' },
  { name: 'Eddy (English (UK))', locale: 'en_GB' }, { name: 'Eddy (English (US))', locale: 'en_US' }, { name: 'Eddy (Chinese (Taiwan))', locale: 'zh_TW' },
  { name: 'Karen', locale: 'en_AU' }, { name: 'Samantha (English (US))', locale: 'en_US' }, { name: 'Samantha (English (US))', locale: 'en_US' },
  { name: 'Sinji (Chinese (Hong Kong))', locale: 'zh_HK' }, { name: 'Tingting', locale: 'zh_CN' }, { name: 'Ava (Premium)', locale: 'en_US' },
  { name: 'Anna', locale: 'de_DE' },
];

test('a voice is saved by the name say accepts: the language suffix goes, other brackets stay', () => {
  assert.equal(voiceValue('Daniel (English (UK))'), 'Daniel');
  assert.equal(voiceValue('Sinji (Chinese (Hong Kong))'), 'Sinji');
  assert.equal(voiceValue('Ava (Premium)'), 'Ava (Premium)');
  assert.equal(voiceValue('Bad News'), 'Bad News');
});

test('the English list shows English voices once, without novelty or per-language voices, with accent and gender', () => {
  const en = voiceChoices(MAC_VOICES, 'en', 'Samantha', zh);
  assert.deepEqual(en.map(o => o.value), ['Ava (Premium)', 'Daniel', 'Karen', 'Samantha']);
  assert.equal(en.find(o => o.value === 'Samantha')!.label, 'Samantha（美式英語，女聲）');
  assert.equal(en.find(o => o.value === 'Daniel')!.label, 'Daniel（英式英語，男聲）');
  assert.equal(voiceChoices(MAC_VOICES, 'en', 'Samantha', en => en).find(o => o.value === 'Karen')!.label, 'Karen (Australian English, female)');
  assert.ok(HIDDEN_VOICES.has('Eddy') && HIDDEN_VOICES.has('Bad News'));
});

test('the Cantonese list shows only Hong Kong voices', () => {
  assert.deepEqual(voiceChoices(MAC_VOICES, 'yue', 'Sinji', zh).map(o => o.label), ['Sinji（廣東話，女聲）']);
});

test('a saved voice that the list hides or lacks stays first, so it never disappears', () => {
  assert.deepEqual(voiceChoices(MAC_VOICES, 'en', 'Eddy', zh)[0], { value: 'Eddy', label: 'Eddy' });
  assert.deepEqual(voiceChoices([], 'yue', 'Sinji', zh), [{ value: 'Sinji', label: 'Sinji' }]);
});

test('speaking speed: three choices, 200 is normal, any saved rate maps to the nearest', () => {
  assert.deepEqual(SPEECH_RATES, { slow: 160, normal: 200, fast: 240 });
  assert.equal(speechRateChoice(200), 'normal');
  assert.equal(speechRateChoice(150), 'slow');
  assert.equal(speechRateChoice(300), 'fast');
  assert.equal(speechRateChoice(185), 'normal');
});

test('Voice: speech on/off sits above the voices, the voice controls grey out when it is off, and each voice has Play sample', () => {
  const code = source(VOICE);
  assert.ok(code.indexOf("row('tts-enabled')") < code.indexOf("row('tts-voice')"));
  assert.match(code, /disabled=\{ttsOff \|\| !!noVoices\}/);
  assert.match(code, /disabled=\{ttsOff\}/);
  assert.equal(code.split('previewTtsVoice(').length - 1, 1);
  assert.match(code, /text\('Play sample', '試聽'\)/);
  assert.match(code, /\['tts_rate', String\(SPEECH_RATES\[next\]\)\]/);
});
