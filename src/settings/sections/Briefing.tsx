import { useCallback, useEffect, useState } from 'react';
import { getDailyBriefingSettings, setDailyBriefingSettings } from '../../lib/commands';
import type { DailyBriefingSettings } from '../../lib/types';
import { schedulerText } from '../../lib/cron-runs';
import { useUiLocale } from '../../i18n';
import SettingGroup from '../../ui/SettingGroup';
import SettingRow from '../../ui/SettingRow';
import Switch from '../../ui/Switch';
import TextField from '../../ui/TextField';
import Button from '../../ui/Button';
import { ROWS_BY_ID, rowText } from '../rows';
import { normalizeBriefingTime } from '../briefing';

type Change = Partial<Pick<DailyBriefingSettings, 'enabled' | 'time' | 'voice'>>;

/**
 * Settings > Daily briefing: on/off, time and read aloud, through the same
 * commands as the old Background block. Each control shows what the backend
 * reports after a change, never just what was clicked.
 */
export default function Briefing() {
  const { locale, t, text } = useUiLocale();
  const [briefing, setBriefing] = useState<DailyBriefingSettings | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [errors, setErrors] = useState<Record<string, string | null>>({});

  const load = useCallback(() => {
    setLoadError(false);
    getDailyBriefingSettings().then(setBriefing).catch(e => { console.error(e); setLoadError(true); });
  }, []);
  useEffect(load, [load]);

  const row = (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });
  const error = (id: string) => errors[id] && <span className="ui-secret-error" role="alert">{errors[id]}</span>;

  // Rejects on failure, so the time field keeps the draft for another try.
  async function save(id: string, change: Change) {
    if (!briefing || busy) throw new Error('not ready');
    const next = { enabled: briefing.enabled, time: briefing.time, voice: briefing.voice, ...change };
    setBusy(true);
    setErrors(e => ({ ...e, [id]: null }));
    try {
      setBriefing(await setDailyBriefingSettings(next.enabled, next.time, next.voice));
    } catch (e) {
      setErrors(all => ({ ...all, [id]: schedulerText(String(e), text) }));
      throw e;
    } finally {
      setBusy(false);
    }
  }

  async function commitTime(draft: string) {
    setErrors(e => ({ ...e, 'briefing-time': null }));
    const time = normalizeBriefingTime(draft);
    if (!time) {
      setErrors(e => ({ ...e, 'briefing-time': text('Enter a time as HH:MM, for example 08:00.', '請以 HH:MM 格式輸入時間，例如 08:00。') }));
      throw new Error('invalid briefing time');
    }
    if (time !== briefing?.time) await save('briefing-time', { time });
  }

  if (loadError) {
    return (
      <p className="ui-secret-error" role="alert">
        {text('Could not load the briefing settings.', '無法載入簡報設定。')} <Button kind="quiet" onClick={load}>{t('retry')}</Button>
      </p>
    );
  }
  if (!briefing) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  return (
    <SettingGroup title={text('Schedule', '時間安排')}>
      <SettingRow {...row('briefing-enabled')} control={
        <Switch checked={briefing.enabled} disabled={busy} label={row('briefing-enabled').title}
          onChange={enabled => { save('briefing-enabled', { enabled }).catch(() => {}); }} />
      }>
        {briefing.enabled && <div className="ui-hint" role="status">{t('dailyBriefingNext', { time: briefing.time })}</div>}
        <div className="ui-hint">{t('dailyBriefingHint')}</div>
        {error('briefing-enabled')}
      </SettingRow>

      <SettingRow {...row('briefing-time')} control={
        <div style={{ width: 120 }}>
          <TextField value={briefing.time} label={row('briefing-time').title} placeholder="08:00" maxLength={5} onCommit={commitTime} />
        </div>
      }>{error('briefing-time')}</SettingRow>

      <SettingRow {...row('briefing-aloud')} control={
        <Switch checked={briefing.voice} disabled={busy} label={row('briefing-aloud').title}
          onChange={voice => { save('briefing-aloud', { voice }).catch(() => {}); }} />
      }>
        {briefing.voice && !briefing.speech_enabled && <div className="ui-hint" role="status">{t('dailyBriefingSpeechOff')}</div>}
        <div className="ui-hint">{t('dailyBriefingVoiceHint')}</div>
        {error('briefing-aloud')}
      </SettingRow>
    </SettingGroup>
  );
}
