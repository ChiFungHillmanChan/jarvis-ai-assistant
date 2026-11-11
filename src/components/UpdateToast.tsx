import { useEffect, useSyncExternalStore } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { RotateCw, X } from "lucide-react";
import { useUiLocale } from "../i18n";
import { createUpdateRunner, isToastVisible, type UpdateRunner } from "../lib/update";

// Without these the updater has no timeout at all: a download that stalls
// (sleep, a dropped Wi-Fi) never fails, so it never retries either.
const CHECK_TIMEOUT_MS = 30 * 1000;
const DOWNLOAD_TIMEOUT_MS = 10 * 60 * 1000;

// PaywallGate and the workspace each render this toast; both share one runner
// so an update is only ever downloaded and installed once.
let runner: UpdateRunner | null = null;
function sharedRunner(): UpdateRunner {
  runner ??= createUpdateRunner({
    async check() {
      const found = await check({ timeout: CHECK_TIMEOUT_MS });
      return found && {
        version: found.version,
        download: () => found.download(undefined, { timeout: DOWNLOAD_TIMEOUT_MS }),
        install: () => found.install(),
        close: () => found.close(),
      };
    },
    relaunch,
  });
  return runner;
}

/**
 * Bottom-left notification for a downloaded update.
 *
 * Renders nothing while the update downloads in the background; then offers a
 * restart that installs it and relaunches into the new version. All of the
 * rules live in lib/update.ts so they can be tested without rendering.
 */
export default function UpdateToast() {
  const { t } = useUiLocale();
  const updates = sharedRunner();
  const state = useSyncExternalStore(updates.subscribe, updates.getState);

  useEffect(() => {
    // `tauri dev` runs src-tauri/target/debug/jarvis, outside any .app. The
    // updater installs into the executable's folder, so it would replace the
    // build folder with the release and delete the dev binary.
    if (!import.meta.env.DEV) updates.start();
  }, [updates]);

  if (!isToastVisible(state)) return null;

  const busy = state.phase === "restarting";
  const failed = state.phase === "install-failed";
  const heading =
    failed ? t("updateFailed")
      : state.phase === "restart-failed" ? t("updateReopen")
        : t("updateReady", { version: state.version ?? "" });

  return (
    <div className={`update-toast${failed ? " is-error" : ""}`} role="status" aria-live="polite">
      <span className={busy ? "update-toast-dot" : "update-toast-dot animate-glow"} aria-hidden="true" />
      <div className="update-toast-body">
        <strong>{heading}</strong>
        {state.phase !== "restart-failed" && (
          <button type="button" className="update-toast-action" onClick={() => void updates.restart()} disabled={busy}>
            <RotateCw size={13} aria-hidden="true" />{busy ? t("restartingUpdate") : t("restartToUpdate")}
          </button>
        )}
        {failed && state.message && <span className="update-toast-error">{state.message}</span>}
      </div>
      {!busy && (
        <button
          type="button"
          className="update-toast-close"
          aria-label={t("dismissUpdate")}
          title={t("dismissUpdate")}
          onClick={() => updates.dismiss()}
        >
          <X size={13} />
        </button>
      )}
    </div>
  );
}
