import { useState } from 'react';
import { confirmAction } from '../../components/ConfirmDialog';
import { downloadModel } from '../../lib/commands';
import { canSwitchListening } from '../../lib/always-listening';
import { useUiLocale } from '../../i18n';
import { runAction } from '../../ui/actions';
import SettingGroup from '../../ui/SettingGroup';
import SettingRow from '../../ui/SettingRow';
import Select from '../../ui/Select';
import Switch from '../../ui/Switch';
import Button from '../../ui/Button';
import { ROWS_BY_ID, rowText } from '../rows';
import { useVoiceSettings } from '../hooks/useVoiceSettings';
import { ENGLISH_VOICE_KEYS, downloadPercent, voiceAvailabilityLine, wakePrivacyLine, wakeStatusLine } from '../voice';

type Failure = { message: string } | null;

/**
 * Settings > Voice: speech-to-text language, voice input and its speech
 * model, spoken replies and their voices, kept recordings and the wake word.
 * The reply language is in General. Every control saves on change and goes
 * back, with an inline error, if the save fails.
 */
export default function Voice() {
  const { locale, t, text } = useUiLocale();
  const v = useVoiceSettings();
  const [failed, setFailed] = useState<Record<string, Failure>>({});
  const [downloading, setDownloading] = useState(false);

  const row = (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });
  const fail = (id: string, e: unknown) => { console.error(e); setFailed(f => ({ ...f, [id]: { message: e instanceof Error ? e.message : String(e) } })); };
  // Runs one row's change; a failure is shown at that row.
  const attempt = (id: string, change: () => Promise<unknown>) => {
    setFailed(f => ({ ...f, [id]: null }));
    change().catch(e => fail(id, e));
  };
  const saveFailure = (id: string) => failed[id] && <span className="ui-secret-error" role="alert">{text('Could not save. Try again.', '無法儲存，請再試一次。')}</span>;
  const actionFailure = (id: string) => failed[id] && <span className="ui-secret-error" role="alert">{text('That did not work: ', '操作未能完成：')}{failed[id]!.message}</span>;

  if (v.loadError) {
    return (
      <p className="ui-secret-error" role="alert">
        {text('Could not load voice settings.', '無法載入語音設定。')} <Button kind="quiet" onClick={v.load}>{t('retry')}</Button>
      </p>
    );
  }
  const settings = v.settings;
  if (!settings) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  // The download runs in the backend and reports progress through the voice
  // state, so closing and reopening Settings shows it again.
  const percent = downloadPercent(v.state);
  const modelReady = Boolean(v.wake?.model_downloaded);
  async function download(force: boolean) {
    setFailed(f => ({ ...f, 'speech-model': null }));
    setDownloading(true);
    try {
      await downloadModel(force);
      await v.refreshWake();
    } finally {
      setDownloading(false);
    }
  }

  const voiceOptions = (saved: string) => (v.voices.includes(saved) ? v.voices : [saved, ...v.voices]).map(name => ({ value: name, label: name }));
  const noVoices = v.voices.length === 0 ? text('No voices found on this Mac.', '在此 Mac 上找不到可用的聲音。') : undefined;

  const changeKeep = (next: boolean) => attempt('keep-recordings', () => runAction(next ? 'secondary' : 'danger',
    () => v.saveVoice('keep_recordings', next, [['keep_voice_recordings', String(next)]]),
    {
      message: text(
        'Turning this off deletes every recording already kept on this Mac, and it cannot be undone. New voice requests are not kept either. The chat text stays.',
        '關閉後，此 Mac 上已保留的所有錄音會即時刪除，且無法復原；之後的語音請求亦不會保留錄音。對話文字會保留。',
      ),
      label: text('Turn off and delete', '關閉並刪除'),
    },
    confirmAction));

  const wakeCanEnable = Boolean(v.wake?.model_downloaded || v.wake?.enabled);

  return <>
    <SettingGroup title={text('Voice input', '語音輸入')}>
      <SettingRow {...row('stt-language')} control={
        <Select value={v.stt} label={row('stt-language').title} width={200} onChange={next => attempt('stt-language', () => v.saveStt(next))}
          options={[{ value: 'auto', label: text('Auto-detect', '自動偵測') }, { value: 'en', label: 'English' }, { value: 'yue', label: '廣東話' }]} />
      }>{saveFailure('stt-language')}</SettingRow>

      <SettingRow {...row('voice-input-status')} control={null}>
        <div className="ui-hint" role="status">{voiceAvailabilityLine({ cloud: v.cloud, waits: v.waits, local: modelReady, chosen: v.chosen }, text)}</div>
      </SettingRow>

      <SettingRow {...row('speech-model')} control={
        <Button kind="secondary" busy={downloading} disabled={percent !== null} onClick={() => download(modelReady)} onError={e => fail('speech-model', e)}>
          {percent !== null ? `${text('Downloading', '正在下載')} ${percent}%`
            : modelReady ? text('Download again', '重新下載')
            : text('Download the speech model (about 148 MB)', '下載語音模型（約 148 MB）')}
        </Button>
      }>
        {percent !== null && <progress max={100} value={percent} aria-label={row('speech-model').title} style={{ width: '100%' }} />}
        {modelReady && percent === null && <div className="ui-hint" role="status">{text('Model ready. Download again replaces the file if transcription keeps failing to load it.', '模型已準備好。如語音辨識持續無法載入模型，「重新下載」會取代原檔。')}</div>}
        {actionFailure('speech-model')}
      </SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Spoken replies and recordings', '語音朗讀與錄音')}>
      <SettingRow {...row('tts-enabled')} control={
        <Switch checked={settings.tts_enabled} label={row('tts-enabled').title}
          onChange={next => attempt('tts-enabled', () => v.saveVoice('tts_enabled', next, [['tts_enabled', String(next)]]))} />
      }>{saveFailure('tts-enabled')}</SettingRow>

      <SettingRow {...row('keep-recordings')} control={
        <Switch checked={settings.keep_recordings} label={row('keep-recordings').title} onChange={changeKeep} />
      }>
        <div className="ui-hint">{text('Lets you play a voice request back from the chat. The kept copy is stored only in the encrypted database on this Mac and is deleted with its message. Transcription is unchanged: when it uses a cloud service, the audio is still sent there to be transcribed.', '可在對話中重播語音請求。保存的錄音只存於此 Mac 的加密資料庫，並隨訊息一併刪除。語音轉錄方式不變：使用雲端辨識時，語音仍會傳送至該服務轉錄。')}</div>
        {saveFailure('keep-recordings')}
      </SettingRow>

      <SettingRow {...row('tts-voice')} control={
        <Select value={settings.tts_voice} options={voiceOptions(settings.tts_voice)} label={row('tts-voice').title} width={200}
          disabled={!!noVoices} disabledReason={noVoices}
          onChange={next => attempt('tts-voice', () => v.saveVoice('tts_voice', next, ENGLISH_VOICE_KEYS.map(key => [key, next] as [string, string])))} />
      }>{saveFailure('tts-voice')}</SettingRow>

      <SettingRow {...row('tts-voice-yue')} control={
        <Select value={settings.tts_voice_yue} options={voiceOptions(settings.tts_voice_yue)} label={row('tts-voice-yue').title} width={200}
          disabled={!!noVoices} disabledReason={noVoices}
          onChange={next => attempt('tts-voice-yue', () => v.saveVoice('tts_voice_yue', next, [['tts_voice_yue', next]]))} />
      }>{saveFailure('tts-voice-yue')}</SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Wake word', '喚醒詞')}>
      <SettingRow {...row('wake-word')} control={
        <Switch checked={v.wake?.running ?? false} label={row('wake-word').title}
          disabled={!wakeCanEnable || !canSwitchListening(v.wake, v.wakeBusy, v.state)}
          onChange={next => attempt('wake-word', () => v.toggleWake(next))} />
      }>
        {!wakeCanEnable && <div className="ui-hint">{text('Download the speech model above first.', '請先在上方下載語音模型。')}</div>}
        {actionFailure('wake-word')}
      </SettingRow>

      <SettingRow {...row('wake-status')} control={null}>
        <div className="ui-hint" role="status">{wakeStatusLine(v.state, modelReady, text)}</div>
      </SettingRow>

      <SettingRow {...row('wake-privacy')} control={null}>
        <div className="ui-hint">{wakePrivacyLine(v.cloud, v.waits, text)}</div>
      </SettingRow>
    </SettingGroup>
  </>;
}
