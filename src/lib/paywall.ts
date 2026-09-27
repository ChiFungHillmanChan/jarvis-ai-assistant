// What the paywall gate shows, and the dated line under the plan in Settings.
// Pure functions of the account status, so they are tested without a window.

import type { AccountStatus } from './types';
import type { PaymentState } from './account-monitor';
import type { UiLocale } from '../i18n/catalog';

export type GateScreen = 'checking' | 'sign-in' | 'offer' | 'waiting' | 'unconfirmed' | 'offline' | 'open';

/** Which full-screen step a billing build shows; 'open' lets the app through. */
export function gateScreen(status: AccountStatus | null, payment: PaymentState): GateScreen {
  if (!status) return 'checking';
  // Free and dev builds carry no account settings and never show the gate.
  if (!status.available) return 'open';
  if (!status.signed_in) return 'sign-in';
  if (status.tier === 'pro' || status.tier === 'team') return 'open';
  if (payment === 'waiting') return 'waiting';
  if (payment === 'unconfirmed') return 'unconfirmed';
  // No tier and nothing saved: the server could not be reached.
  if (status.plan_checked === 'none') return 'offline';
  return 'offer';
}

export interface PlanLine { key: 'planTrialEnds' | 'planRenews' | 'planEnds'; date: string }

/** The one dated line under the plan name in Settings, or null when there is none. */
export function planLine(status: AccountStatus): PlanLine | null {
  if (status.tier !== 'pro' && status.tier !== 'team') return null;
  if (status.subscription_status === 'trialing' && status.trial_ends_at) return { key: 'planTrialEnds', date: status.trial_ends_at };
  if (status.period_ends_at) return { key: status.cancel_at_period_end ? 'planEnds' : 'planRenews', date: status.period_ends_at };
  return null;
}

export function formatPlanDate(iso: string, locale: UiLocale): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleDateString(locale === 'zh-HK' ? 'zh-HK' : 'en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
}
