import { useEffect, useState } from "react";
import { getKeepAwake, getOpenAtLogin, setKeepAwake, setOpenAtLogin } from "../lib/commands";
import type { KeepAwakeView } from "../lib/types";
import { useUiLocale } from "../i18n";
import { schedulerText } from "../lib/cron-runs";

/**
 * Settings > General: open at login and keep awake on mains power. Every
 * control shows what the backend reports after a change, never just what was
 * clicked. The daily briefing has its own section (Settings > Daily briefing).
 */
export default function BackgroundSettings() {
  const { t, text } = useUiLocale();
  const [openAtLogin, setOpenAtLoginState] = useState<boolean | null>(null);
  const [keepAwake, setKeepAwakeState] = useState<KeepAwakeView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    Promise.all([getOpenAtLogin(), getKeepAwake()])
      .then(([login, awake]) => { if (current) { setOpenAtLoginState(login); setKeepAwakeState(awake); } })
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
  </>;
}

const styles: Record<string, React.CSSProperties> = {
  option: { display: "flex", alignItems: "center", gap: 10, marginBottom: 12, cursor: "pointer", lineHeight: 1.5 },
  checkbox: { accentColor: "var(--cyan)", width: 16, height: 16, flexShrink: 0 },
  optionLabel: { color: "var(--text-primary)", fontSize: 13 },
  hint: { color: "var(--text-secondary)", fontSize: 13, marginBottom: 12, lineHeight: 1.6, maxWidth: "72ch" },
  error: { color: "var(--accent-urgent)", fontSize: 13, marginBottom: 12, lineHeight: 1.5 },
};
