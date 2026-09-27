/** Stripe's largest `trial_period_days`. */
export const MAX_TRIAL_DAYS = 730;
export const DEFAULT_TRIAL_DAYS = 28;

/**
 * The trial length from TRIAL_DAYS. Unset means the default; anything that is
 * not a whole number from 1 to MAX_TRIAL_DAYS is null, so a caller refuses it
 * instead of guessing. Shared by the checkout and /me Lambdas.
 */
export function readTrialDays(raw: string | undefined): number | null {
  const value = raw === undefined || raw.trim() === '' ? String(DEFAULT_TRIAL_DAYS) : raw.trim();
  if (!/^\d+$/.test(value)) return null;
  const days = Number(value);
  return days >= 1 && days <= MAX_TRIAL_DAYS ? days : null;
}
