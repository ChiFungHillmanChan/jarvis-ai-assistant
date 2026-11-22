import { useEffect, useState } from 'react';
import { currentAi } from '../../lib/ai-choice';
import { getDailyBriefingSettings, googleAccounts } from '../../lib/commands';
import { mailWorkspace } from '../../lib/mail';
import { useUiLocale } from '../../i18n';
import SettingGroup from '../../ui/SettingGroup';
import SettingRow from '../../ui/SettingRow';
import Button from '../../ui/Button';
import { ROWS_BY_ID, rowText } from '../rows';
import { useSettingsDialog } from '../SettingsContext';
import type { SettingsSectionId } from '../sections';
import { setupChecklist, type ChecklistId, type ChecklistInput } from '../setup-checklist';

// Where each unfinished item is done.
const OPENS: Record<ChecklistId, SettingsSectionId> = { ai: 'ai', google: 'connections', triage: 'mail', briefing: 'briefing' };

type Readings = { input: ChecklistInput; failed: Set<ChecklistId> };

/**
 * Reads the same things the app already uses: "AI ready" is current_ai, as in
 * QuickSetup; "Google connected" is the main account's state; mail sorting is
 * the triage switch in the Mail workspace; the briefing time counts only
 * while the daily briefing is on. A reading that fails is shown as unknown,
 * never as not done.
 */
async function readChecklist(): Promise<Readings> {
  const [ai, google, mail, briefing] = await Promise.allSettled([currentAi(), googleAccounts(), mailWorkspace(), getDailyBriefingSettings()]);
  const failed = new Set<ChecklistId>();
  const value = <T,>(id: ChecklistId, r: PromiseSettledResult<T>) => { if (r.status === 'rejected') failed.add(id); return r.status === 'fulfilled' ? r.value : null; };
  const brief = value('briefing', briefing);
  return {
    failed,
    input: {
      aiReady: !!value('ai', ai),
      googleConnected: value('google', google)?.main.state === 'connected',
      autoTriage: value('triage', mail)?.triage.enabled ?? false,
      briefingTime: brief?.enabled ? brief.time : null,
    },
  };
}

/** Settings > Setup guide: what is set up so far, and the first-run guide again. */
export default function SetupGuide() {
  const { locale, text } = useUiLocale();
  const { openSettings, openSetupGuide } = useSettingsDialog();
  const [readings, setReadings] = useState<Readings | null>(null);

  useEffect(() => {
    let live = true;
    void readChecklist().then(r => { if (live) setReadings(r); });
    return () => { live = false; };
  }, []);

  const row = (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });
  const labels: Record<ChecklistId, { title: string; action: string }> = {
    ai: { title: text('AI ready', 'AI 已就緒'), action: text('Choose your AI', '選擇 AI') },
    google: { title: text('Google connected', 'Google 已連接'), action: text('Connect Google', '連接 Google') },
    triage: { title: text('Mail sorting on', '郵件整理已開啟'), action: text('Turn on in Mail', '在郵件設定中開啟') },
    briefing: { title: text('Briefing time set', '已設定簡報時間'), action: text('Set a time', '設定時間') },
  };
  const items = readings ? setupChecklist(readings.input) : [];

  return <>
    <SettingGroup title={text('Your setup', '你的設定')}>
      <SettingRow {...row('setup-checklist')} control={null}>
        {!readings && <span className="ui-hint" role="status">{text('Checking…', '正在檢查…')}</span>}
        {readings && <ul className="settings-checklist">
          {items.map(item => {
            const unknown = readings.failed.has(item.id);
            const state = unknown ? text('Could not check', '未能檢查') : item.done ? text('Done', '已完成') : text('Not yet', '尚未完成');
            return (
              <li key={item.id}>
                <span className={`settings-chip${item.done && !unknown ? ' is-on' : ''}`}>{state}</span>
                <span className="settings-checklist-title">
                  {labels[item.id].title}
                  {item.id === 'briefing' && item.done && <span className="ui-hint"> · {readings.input.briefingTime}</span>}
                </span>
                {(!item.done || unknown) && <Button kind="secondary" onClick={() => openSettings(OPENS[item.id])}>{labels[item.id].action}</Button>}
              </li>
            );
          })}
        </ul>}
      </SettingRow>
      <SettingRow {...row('setup-open')} control={<Button kind="primary" onClick={() => openSetupGuide()}>{text('Open setup guide', '開啟設定導引')}</Button>} />
    </SettingGroup>
  </>;
}
