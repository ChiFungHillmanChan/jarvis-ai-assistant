import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { check, type Update, type DownloadEvent } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Download, X } from "lucide-react";
import { useUiLocale } from "../i18n";
import { initialToastState, isToastVisible, updateToastReducer } from "../lib/update";

// How often a long-running session re-checks for a new release. A dismissal
// lasts until the next one of these, never longer.
const RECHECK_MS = 6 * 60 * 60 * 1000;

/**
 * Bottom-left notification offering the new version.
 *
 * Renders nothing until a check finds an update; then offers a download that
 * installs and relaunches into the new version. All of the visibility rules
 * live in lib/update.ts so they can be tested without rendering.
 */
export default function UpdateToast() {
  const { t, text } = useUiLocale();
  const [state, dispatch] = useReducer(updateToastReducer, initialToastState);
  const [update, setUpdate] = useState<Update | null>(null);
  // Read inside the check so a poll landing mid-install cannot swap the handle
  // out from under downloadAndInstall.
  const phaseRef = useRef(state.phase);
  phaseRef.current = state.phase;

  const runCheck = useCallback(async () => {
    try {
      const found = await check();
      if (phaseRef.current === "downloading") return;
      setUpdate(found);
      dispatch({ type: "checked", version: found?.version ?? null });
    } catch (e) {
      // Updater errors (S3 down, offline, no manifest) must never disrupt the app.
      console.warn("update check failed", e);
    }
  }, []);

  useEffect(() => {
    void runCheck();
    const id = setInterval(() => void runCheck(), RECHECK_MS);
    return () => clearInterval(id);
  }, [runCheck]);

  const install = useCallback(async () => {
    if (!update || phaseRef.current === "downloading") return;
    dispatch({ type: "download-started" });
    try {
      let downloaded = 0;
      let total = 0;
      await update.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total > 0) dispatch({ type: "progress", percent: Math.round((downloaded / total) * 100) });
        } else if (event.event === "Finished") {
          dispatch({ type: "progress", percent: 100 });
        }
      });
      await relaunch();
    } catch (e) {
      console.error("update install failed", e);
      dispatch({ type: "failed", message: String(e) });
    }
  }, [update]);

  if (!isToastVisible(state)) return null;

  const busy = state.phase === "downloading";
  const action =
    state.phase === "downloading"
      ? t("updating", { percent: state.progress })
      : state.phase === "error"
        ? t("retry")
        : t("downloadLatest");

  return (
    <div className={`update-toast${state.phase === "error" ? " is-error" : ""}`} role="status" aria-live="polite">
      <span className={busy ? "update-toast-dot" : "update-toast-dot animate-glow"} aria-hidden="true" />
      <div className="update-toast-body">
        <strong>{state.phase === "error" ? t("updateFailed") : t("updateAvailable", { version: state.version ?? "" })}</strong>
        <button type="button" className="update-toast-action" onClick={() => void install()} disabled={busy}>
          <Download size={13} aria-hidden="true" />{action}
        </button>
        {state.phase === "error" && state.message && <span className="update-toast-error">{state.message}</span>}
      </div>
      {!busy && (
        <button
          type="button"
          className="update-toast-close"
          aria-label={t("dismissUpdate")}
          title={text("Dismiss until the next check", "暫時關閉，下次檢查再提示")}
          onClick={() => dispatch({ type: "dismissed" })}
        >
          <X size={13} />
        </button>
      )}
    </div>
  );
}
