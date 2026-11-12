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
export function cronRunNote(run: Pick<CronRunView, "status" | "result"> & { trigger?: string | null }, text: Text): string | null {
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
      // A run started by hand belongs to no scheduled time, so nothing runs it again.
      if (run.trigger === "manual") return text("This Mac had no network connection, so this run could not finish. It is not tried again on its own: run it again when the connection is back.", "當時此 Mac 沒有網絡連線，這次手動執行未能完成，亦不會自動重試。恢復連線後請再執行一次。");
      return text("This Mac had no network connection until the next scheduled time, so this one was not run.", "直至下一個排程時間，此 Mac 仍沒有網絡連線，因此這一次沒有執行。");
    case "interrupted":
      return text("JARVIS was closed or the Mac went to sleep before this run finished.", "這次執行尚未完成，JARVIS 已關閉或 Mac 已進入睡眠。");
    default:
      return null;
  }
}

/**
 * The fixed sentences the scheduler and the daily briefing write in English:
 * run results, run errors, and the errors of "Run now" and the briefing time.
 * They are stored and sent as written, and put into the interface language
 * here, so a run from last week reads in the language the interface is in now.
 */
const SCHEDULER_TEXT: Record<string, string> = {
  "Interrupted: the Mac went to sleep during this run": "已中斷：執行期間 Mac 進入了睡眠。",
  "Interrupted again on the retry; this occurrence is not retried a second time": "重新執行時再次中斷；這一次不會再重試。",
  "This automation is already running": "此自動排程正在執行中。",
  "Automation not found": "找不到此自動排程。",
  "The scheduler is still starting. Try again in a moment.": "排程功能仍在啟動，請稍後再試。",
  "Already shown today": "今天已顯示過簡報。",
  "Briefing shown; AI summary added": "已顯示簡報，並已加入 AI 摘要。",
  "Briefing shown from what is saved on this Mac; the AI summary follows when the network returns": "已根據此 Mac 上儲存的資料顯示簡報；恢復網絡連線後會加入 AI 摘要。",
  "Briefing shown from what is saved on this Mac; the Mac went to sleep before the AI summary arrived, so it is added afterwards": "已根據此 Mac 上儲存的資料顯示簡報；AI 摘要未送達，Mac 已進入睡眠，稍後會補上。",
  "Briefing shown; it was replaced or removed before the AI summary arrived": "已顯示簡報；AI 摘要送達前，該簡報已被取代或移除。",
};

/** Sentences that end with a detail of their own (the reason, the value that was refused), which is kept as it came. */
const SCHEDULER_TEXT_WITH_DETAIL: Record<string, string> = {
  "Briefing shown from what is saved on this Mac; the AI summary failed: ": "已根據此 Mac 上儲存的資料顯示簡報；未能取得 AI 摘要：",
  "Briefing time must be HH:MM between 00:00 and 23:59, not ": "簡報時間必須是 00:00 至 23:59 之間的時間（HH:MM），不能是 ",
};

/** Text from the scheduler in the interface language. Anything it does not know is shown as it came. */
export function schedulerText(message: string, text: Text): string {
  const whole = SCHEDULER_TEXT[message];
  if (whole) return text(message, whole);
  for (const [start, chinese] of Object.entries(SCHEDULER_TEXT_WITH_DETAIL)) {
    if (message.startsWith(start)) return text(message, chinese + message.slice(start.length));
  }
  return message;
}

/** Every sentence `schedulerText` knows, as the backend writes it. For the test that holds the two sides together. */
export const schedulerSentences = [...Object.keys(SCHEDULER_TEXT), ...Object.keys(SCHEDULER_TEXT_WITH_DETAIL)];
