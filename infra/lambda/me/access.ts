const WINDOW_MS = 24 * 3_600_000;

/** Requests recorded before this instant may be recorded again. */
export const accessCutoff = (now: Date): string => new Date(now.getTime() - WINDOW_MS).toISOString();

/** Decide whether an access request is written now or the stored one stands (one per 24 h). */
export function accessRequest(row: Record<string, unknown> | undefined, now: Date): { write: boolean; requestedAt: string } {
  const stored = row?.aiAccessRequestedAt;
  if (typeof stored === 'string' && Date.parse(stored) >= Date.parse(accessCutoff(now))) {
    return { write: false, requestedAt: stored };
  }
  return { write: true, requestedAt: now.toISOString() };
}
