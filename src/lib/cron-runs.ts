import type { CronRunView } from "./types";

type Text = (english: string, traditionalChinese: string) => string;

/** A job or run status as the user reads it. An unknown status is shown as stored. */
export function cronStatusLabel(status: string, text: Text): string {
  switch (status) {
    case "active": return text("Active", "運行中");
    case "paused": return text("Paused", "已暫停");
    case "running": return text("Running", "執行中");
    case "completed": return text("Completed", "已完成");
    case "failed": return text("Failed", "失敗");
    case "interrupted": return text("Interrupted", "已中斷");
    case "skipped_offline": return text("Waiting for the network", "等候網絡連線");
    case "skipped_offline_final": return text("Not run: no network", "沒有執行：沒有網絡連線");
    case "collapsed": return text("Missed", "已錯過");
    default: return status;
  }
}

/** Why a run started. Null for rows that are not runs, and for runs from before triggers were recorded. */
export function cronTriggerLabel(trigger: string | null, text: Text): string | null {
  switch (trigger) {
    case "scheduled": return text("On time", "準時執行");
    case "catch_up": return text("Caught up afterwards", "事後補跑");
    case "manual": return text("Started by hand", "手動執行");
    case "retry_interrupted": return text("Run again after an interruption", "中斷後重新執行");
    default: return null;
  }
}

/**
 * What a row that is not an ordinary run means, in the interface language.
 * Null for ordinary runs, whose own result or error text is shown instead.
 */
export function cronRunNote(run: Pick<CronRunView, "status" | "result">, text: Text): string | null {
  switch (run.status) {
    case "collapsed": {
      // The row stands for every older scheduled time that passed without a
      // run; `result` holds how many.
      let count = 0, capped = false;
      try { const detail = JSON.parse(run.result ?? ""); count = Number(detail.count) || 0; capped = detail.capped === true; } catch { /* an unreadable count is shown without a number */ }
      if (count <= 0) return text("Earlier scheduled times passed without a run. JARVIS runs only the latest missed one.", "較早的排程時間已過而沒有執行。JARVIS 只會補跑最近一次。");
      if (capped) return text(`At least ${count} earlier scheduled times passed without a run. JARVIS runs only the latest missed one.`, `最少有 ${count} 個較早的排程時間已過而沒有執行。JARVIS 只會補跑最近一次。`);
      return text(
        count === 1 ? "1 earlier scheduled time passed without a run. JARVIS runs only the latest missed one." : `${count} earlier scheduled times passed without a run. JARVIS runs only the latest missed one.`,
        `有 ${count} 個較早的排程時間已過而沒有執行。JARVIS 只會補跑最近一次。`,
      );
    }
    case "skipped_offline":
      return text("This Mac had no network connection. JARVIS runs this when the connection returns, or at the next scheduled time.", "當時此 Mac 沒有網絡連線。恢復連線後，或到下一個排程時間，JARVIS 會執行這項工作。");
    case "skipped_offline_final":
      return text("This Mac had no network connection until the next scheduled time, so this one was not run.", "直至下一個排程時間，此 Mac 仍沒有網絡連線，因此這一次沒有執行。");
    case "interrupted":
      return text("JARVIS was closed or the Mac went to sleep before this run finished.", "這次執行尚未完成，JARVIS 已關閉或 Mac 已進入睡眠。");
    default:
      return null;
  }
}
