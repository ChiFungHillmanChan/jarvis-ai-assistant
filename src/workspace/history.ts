/** Messages shown when a conversation opens. Each one is parsed as Markdown,
 * and a full history of a few hundred takes tens of milliseconds to mount. */
export const RECENT_MESSAGES = 40;

/** Index of the first message to render. `hidden` is null until the history
 * has loaded; after that it stays put, so new messages do not push the
 * shown ones off screen. */
export function historyStart(total: number, hidden: number | null): number {
  return Math.min(hidden ?? Math.max(0, total - RECENT_MESSAGES), total);
}
