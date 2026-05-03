import { useEffect, useState } from "react";
import { check } from "@tauri-apps/plugin-updater";

interface UpdateInfo {
  version: string;
  notes: string | undefined;
}

export default function UpdatePrompt() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    check()
      .then((update) => {
        if (cancelled || !update) return;
        setInfo({ version: update.version, notes: update.body });
      })
      .catch((e) => {
        // Silent: never block startup on updater errors (S3 down, no manifest, network off, etc.)
        console.warn("update check failed", e);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function install() {
    setError(null);
    setInstalling(true);
    try {
      const update = await check();
      if (!update) return;
      await update.downloadAndInstall();
      // The plugin relaunches the app automatically when install finishes.
    } catch (e) {
      setError(String(e));
      setInstalling(false);
    }
  }

  if (!info) return null;

  return (
    <div style={styles.overlay}>
      <div style={styles.modal}>
        <div style={styles.eyebrow}>UPDATE AVAILABLE</div>
        <h3 style={styles.title}>Version {info.version}</h3>
        {info.notes && <pre style={styles.notes}>{info.notes}</pre>}
        {error && <div style={styles.error}>{error}</div>}
        <div style={styles.row}>
          <button style={styles.later} onClick={() => setInfo(null)} disabled={installing}>Later</button>
          <button style={styles.install} onClick={install} disabled={installing}>
            {installing ? "Installing..." : "Install"}
          </button>
        </div>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  overlay: { position: "fixed", inset: 0, background: "rgba(6,10,20,0.85)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 1000 },
  modal: { background: "rgba(6,10,20,0.95)", border: "1px solid rgba(0,180,255,0.3)", borderRadius: 10, padding: 24, maxWidth: 480, width: "90%" },
  eyebrow: { fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: 4, color: "rgba(0,180,255,0.5)" },
  title: { color: "rgba(0,180,255,0.9)", fontSize: 20, margin: "8px 0 16px" },
  notes: { color: "rgba(0,180,255,0.7)", fontSize: 12, fontFamily: "var(--font-mono)", whiteSpace: "pre-wrap", marginBottom: 16 },
  row: { display: "flex", gap: 8, justifyContent: "flex-end" },
  later: { background: "transparent", border: "1px solid rgba(0,180,255,0.2)", color: "rgba(0,180,255,0.6)", borderRadius: 6, padding: "8px 14px", cursor: "pointer", fontFamily: "var(--font-mono)" },
  install: { background: "rgba(0,180,255,0.18)", border: "1px solid rgba(0,180,255,0.5)", color: "rgba(0,180,255,0.95)", borderRadius: 6, padding: "8px 14px", cursor: "pointer", fontFamily: "var(--font-mono)" },
  error: { color: "rgba(255,100,100,0.9)", fontSize: 12, marginBottom: 12 },
};
