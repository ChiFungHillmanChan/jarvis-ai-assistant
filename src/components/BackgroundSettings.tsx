import { useEffect, useState } from "react";
import { getDailyBriefingSettings, getKeepAwake, getOpenAtLogin, setDailyBriefingSettings, setKeepAwake, setOpenAtLogin } from "../lib/commands";
import type { DailyBriefingSettings, KeepAwakeView } from "../lib/types";
import { useUiLocale } from "../i18n";
import { schedulerText } from "../lib/cron-runs";

/**
 * Settings > Background: open at login, keep awake on mains power, and the
 * scheduled daily briefing. Every control shows what the backend reports
 * after a change, never just what was clicked.
 */
export default function BackgroundSettings() {
  const { t, text } = useUiLocale();
  const [openAtLogin, setOpenAtLoginState] = useState<boolean | null>(null);
  const [keepAwake, setKeepAwakeState] = useState<KeepAwakeView | null>(null);
  const [briefing, setBriefing] = useState<DailyBriefingSettings | null>(null);
  const [time, setTime] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    Promise.all([getOpenAtLogin(), getKeepAwake(), getDailyBriefingSettings()])
      .then(([login, awake, brief]) => { if (current) { setOpenAtLoginState(login); setKeepAwakeState(awake); setBriefing(brief); setTime(brief.time); } })
      .catch(e => { if (current) setError(String(e)); });
    // "In effect now" follows the power cable, so it is re-read while the page is open.
    const timer = setInterval(() => { if (!document.hidden) void getKeepAwake().then(view => { if (current) setKeepAwakeState(view); }).catch(() => {}); }, 5000);
    return () => { current = false; clearInterval(timer); };
  }, []);

  async function change<T>(action: () => Promise<T>, apply: (value: T) => void) {
    if (busy) return;
    setBusy(true); setError(null);
    try { apply(await action()); } catch (e) { setError(schedulerText(String(e), text)); } finally { setBusy(false); }
  }
  const saveBriefing = (next: { enabled?: boolean; time?: string; voice?: boolean }) => {
    if (!briefing) return;
    const merged = { enabled: briefing.enabled, time: briefing.time, voice: briefing.voice, ...next };
    void change(() => setDailyBriefingSettings(merged.enabled, merged.time, merged.voice), saved => { setBriefing(saved); setTime(saved.time); });
  };
  // The time field reports every keystroke; it is saved once the field is left, and only if it holds a whole time.
  const saveTime = () => { if (briefing && /^\d{2}:\d{2}$/.test(time) && time !== briefing.time) saveBriefing({ time }); else if (briefing) setTime(briefing.time); };

  return <>
    {error && <p role="alert" style={styles.error}>{error}</p>}
    <div className="settings-panel">
      <h2 className="settings-heading">{t("openAtLoginTitle")}</h2>
      <label style={styles.option}>
        <input type="checkbox" checked={openAtLogin ?? false} disabled={openAtLogin === null || busy} style={styles.checkbox}
          onChange={e => { const enabled = e.target.checked; void change(() => setOpenAtLogin(enabled), setOpenAtLoginState); }} />
        <span style={styles.optionLabel}>{t("openAtLoginLabel")}</span>
      </label>
      <p style={styles.hint}>{t("openAtLoginHint")}</p>
    </div>
    <div className="settings-panel">
      <h2 className="settings-heading">{t("keepAwakeTitle")}</h2>
      <label style={styles.option}>
        <input type="checkbox" checked={keepAwake?.enabled ?? false} disabled={!keepAwake || busy} style={styles.checkbox}
          onChange={e => { const enabled = e.target.checked; void change(() => setKeepAwake(enabled), setKeepAwakeState); }} />
        <span style={styles.optionLabel}>{t("keepAwakeLabel")}</span>
      </label>
      {keepAwake?.enabled && <p role="status" style={styles.hint}>{keepAwake.active ? t("keepAwakeActive") : t("keepAwakeOnBattery")}</p>}
      <p style={styles.hint}>{t("keepAwakeHint")}</p>
    </div>
    <div className="settings-panel">
      <h2 className="settings-heading">{t("dailyBriefingTitle")}</h2>
      <label style={styles.option}>
        <input type="checkbox" checked={briefing?.enabled ?? false} disabled={!briefing || busy} style={styles.checkbox}
          onChange={e => saveBriefing({ enabled: e.target.checked })} />
        <span style={styles.optionLabel}>{t("dailyBriefingLabel")}</span>
      </label>
      <label style={styles.option}>
        <span style={styles.optionLabel}>{t("dailyBriefingTime")}</span>
        <input type="time" value={time} disabled={!briefing || busy} style={styles.time}
          onChange={e => setTime(e.target.value)} onBlur={saveTime} onKeyDown={e => { if (e.key === "Enter") saveTime(); }} />
      </label>
      {briefing?.enabled && <p role="status" style={styles.hint}>{t("dailyBriefingNext", { time: briefing.time })}</p>}
      <p style={styles.hint}>{t("dailyBriefingHint")}</p>
      <label style={styles.option}>
        <input type="checkbox" checked={briefing?.voice ?? true} disabled={!briefing || busy} style={styles.checkbox}
          onChange={e => saveBriefing({ voice: e.target.checked })} />
        <span style={styles.optionLabel}>{t("dailyBriefingVoice")}</span>
      </label>
      {briefing?.voice && !briefing.speech_enabled
        ? <p role="status" style={styles.hint}>{t("dailyBriefingSpeechOff")}</p>
        : <p style={styles.hint}>{t("dailyBriefingNeedsSpeech")}</p>}
      <p style={styles.hint}>{t("dailyBriefingVoiceHint")}</p>
    </div>
  </>;
}

const styles: Record<string, React.CSSProperties> = {
  option: { display: "flex", alignItems: "center", gap: 10, marginBottom: 12, cursor: "pointer", lineHeight: 1.5 },
  checkbox: { accentColor: "var(--cyan)", width: 16, height: 16, flexShrink: 0 },
  optionLabel: { color: "var(--text-primary)", fontSize: 13 },
  hint: { color: "var(--text-secondary)", fontSize: 13, marginBottom: 12, lineHeight: 1.6, maxWidth: "72ch" },
  time: { background: "var(--bg-primary)", border: "1px solid var(--border-primary)", borderRadius: 6, padding: "7px 10px", color: "var(--text-primary)", fontSize: 13, fontFamily: "inherit" },
  error: { color: "var(--accent-urgent)", fontSize: 13, marginBottom: 12, lineHeight: 1.5 },
};
