import { useCallback, useEffect, useState } from "react";
import { check, type Update, type DownloadEvent } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useUiLocale } from "../i18n";

type Phase = "idle" | "downloading" | "error";

// How often a long-running session re-checks for a new release.
const RECHECK_MS = 6 * 60 * 60 * 1000;

/**
 * Persistent, always-visible update affordance for the title bar.
 * Renders nothing until an update exists; then shows a button anyone can click
 * to download, install, and relaunch into the new version.
 */
export default function UpdateButton() {
  const { text } = useUiLocale();
  const [update, setUpdate] = useState<Update | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState(0);
  const [message, setMessage] = useState<string | null>(null);

  const runCheck = useCallback(async () => {
    try {
      const found = await check();
      // Don't clobber a handle we're already installing.
      setUpdate((prev) => (prev && phase === "downloading" ? prev : found));
    } catch (e) {
      // Updater errors (S3 down, offline, no manifest) must never disrupt the app.
      console.warn("update check failed", e);
    }
  }, [phase]);

  useEffect(() => {
    runCheck();
    const id = setInterval(runCheck, RECHECK_MS);
    return () => clearInterval(id);
    // runCheck is stable enough for this once-on-mount + interval usage.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const install = useCallback(async () => {
    if (!update || phase === "downloading") return;
    setPhase("downloading");
    setProgress(0);
    setMessage(null);
    try {
      let downloaded = 0;
      let total = 0;
      await update.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total > 0) setProgress(Math.round((downloaded / total) * 100));
        } else if (event.event === "Finished") {
          setProgress(100);
        }
      });
      await relaunch();
    } catch (e) {
      console.error("update install failed", e);
      setMessage(String(e));
      setPhase("error");
    }
  }, [update, phase]);

  if (!update) return null;

  const label =
    phase === "downloading"
      ? `${text("Updating", "更新中")} ${progress}%`
      : phase === "error"
        ? text("Retry update", "重試更新")
        : `${text("Update to", "更新至")} v${update.version}`;

  return (
    <div style={styles.wrap}>
      <button
        onClick={install}
        disabled={phase === "downloading"}
        title={phase === "error" && message ? message : update.body?.trim() || `${text("Install JARVIS", "安裝 JARVIS")} v${update.version}`}
        style={{ ...styles.btn, ...(phase === "error" ? styles.errBtn : {}) }}
      >
        <span
          className={phase === "downloading" ? undefined : "animate-glow"}
          style={{ ...styles.dot, ...(phase === "error" ? styles.errDot : {}) }}
        />
        {label}
      </button>
      {phase === "error" && message && <span style={styles.errMsg}>{message}</span>}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  wrap: {
    display: "flex",
    flexDirection: "column",
    alignItems: "flex-end",
    gap: 2,
  },
  errMsg: {
    maxWidth: 260,
    color: "rgba(255,180,180,0.8)",
    fontFamily: "var(--font-mono)",
    fontSize: 9,
    lineHeight: 1.3,
    textAlign: "right",
    overflowWrap: "anywhere",
  },
  btn: {
    display: "flex",
    alignItems: "center",
    gap: 6,
    background: "rgba(0,180,255,0.14)",
    border: "1px solid rgba(0,180,255,0.5)",
    borderRadius: 20,
    padding: "4px 12px",
    color: "rgba(0,180,255,0.95)",
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    letterSpacing: 0.5,
    cursor: "pointer",
    whiteSpace: "nowrap",
  },
  errBtn: {
    background: "rgba(255,100,100,0.12)",
    border: "1px solid rgba(255,100,100,0.5)",
    color: "rgba(255,180,180,0.95)",
  },
  dot: {
    width: 7,
    height: 7,
    borderRadius: "50%",
    background: "rgba(0,180,255,0.9)",
    boxShadow: "0 0 6px rgba(0,180,255,0.7)",
    flexShrink: 0,
  },
  errDot: {
    background: "rgba(255,120,120,0.9)",
    boxShadow: "0 0 6px rgba(255,120,120,0.7)",
  },
};
