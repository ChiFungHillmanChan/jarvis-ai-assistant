import { useEffect, useState, useSyncExternalStore } from 'react';
import { useUiLocale } from '../../../i18n';
import { useAiModels } from '../../../lib/ai-models';
import { isModelDownloaded } from '../../../lib/commands';
import Button from '../../../ui/Button';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import { clearAiError, lastAiError, subscribeAiErrors } from '../../ai-error-log';
import { useSettingsDialog } from '../../SettingsContext';
import { useRow } from './useRow';

/**
 * Settings > Advanced > Developer: the session's last AI settings error, and
 * where the speech model lives. The speech model is downloaded in one place,
 * Voice; this section links there instead of offering a second download.
 */
export default function Developer() {
  const { locale, text } = useUiLocale();
  const row = useRow();
  const { openSettings } = useSettingsDialog();
  const models = useAiModels();
  const lastError = useSyncExternalStore(subscribeAiErrors, lastAiError);
  const [downloaded, setDownloaded] = useState<boolean | null>(null);

  useEffect(() => { isModelDownloaded().then(setDownloaded).catch(() => setDownloaded(null)); }, []);

  const whisper = models?.defaults.whisper;
  const path = whisper ? `~/Library/Application Support/jarvis/models/${whisper}.bin` : null;
  const time = (at: number) => new Date(at).toLocaleTimeString(locale === 'zh-HK' ? 'zh-HK' : 'en-GB', { hour: '2-digit', minute: '2-digit' });

  return <>
    <SettingGroup title={text('Diagnostics', '診斷')}>
      <SettingRow {...row('ai-error-log')} control={lastError ? <Button kind="quiet" onClick={clearAiError}>{text('Clear', '清除')}</Button> : null}>
        {lastError
          ? <span className="ui-secret-error" role="status">{text(`At ${time(lastError.at)}: `, `${time(lastError.at)}：`)}{lastError.message}</span>
          : <span className="ui-hint">{text('No AI errors in this session. Errors from local endpoints and the provider order show here too.', '本次使用期間沒有 AI 錯誤。本機端點及供應商次序的錯誤亦會在此顯示。')}</span>}
      </SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Speech model', '語音模型')}>
      <SettingRow {...row('speech-model-redownload')} control={<Button kind="quiet" onClick={() => openSettings('voice')}>{text('Download it in Voice', '在語音中下載')}</Button>}>
        <span className="ui-hint">{text('If transcription keeps failing to load the model, the file may be damaged. "Download again" in Voice replaces it.', '如果語音辨識持續無法載入模型，檔案可能已損壞。在「語音」中按「重新下載」即可取代原檔。')}</span>
      </SettingRow>
      <SettingRow {...row('speech-model-path')} control={null}>
        {path ? <p className="ui-hint"><code>{path}</code></p> : <p className="ui-hint" role="status">{text('Loading…', '正在載入…')}</p>}
        {downloaded !== null && <p className="ui-hint">{downloaded ? text('The file is on this Mac.', '此 Mac 上已有此檔案。') : text('Not downloaded yet.', '尚未下載。')}</p>}
      </SettingRow>
    </SettingGroup>
  </>;
}
