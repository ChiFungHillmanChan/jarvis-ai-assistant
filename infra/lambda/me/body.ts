export interface MeBody {
  sub: string;
  email: string | null;
  tier: 'free' | 'pro' | 'team';
  subscriptionStatus: string | null;
  trialEndsAt: string | null;
  periodEndsAt: string | null;
  cancelAtPeriodEnd: boolean;
  /** Trial days a checkout would offer; 0 once the account has had a subscription. Display only -- checkout decides. */
  trialDays: number;
}

const TIERS = new Set(['free', 'pro', 'team']);
const text = (value: unknown) => typeof value === 'string' ? value : null;

/** A missing row is the free tier; a value the webhook would never write is also free. */
export function meBody(sub: string, email: string | null, item: Record<string, unknown> | undefined, trialDays = 0): MeBody {
  const stored = typeof item?.tier === 'string' ? item.tier : 'free';
  return {
    sub,
    email,
    tier: (TIERS.has(stored) ? stored : 'free') as MeBody['tier'],
    subscriptionStatus: text(item?.subscriptionStatus),
    trialEndsAt: text(item?.trialEndsAt),
    periodEndsAt: text(item?.periodEndsAt),
    cancelAtPeriodEnd: item?.cancelAtPeriodEnd === true,
    trialDays: typeof item?.stripeSubscriptionId === 'string' ? 0 : trialDays,
  };
}

const APP_VERSION = /^\d+\.\d+\.\d+([-+][0-9A-Za-z.-]{1,20})?$/;
const MAILBOX_COUNT = /^\d{1,2}$/;

/** Client-reported labels for the touch write. Anything that does not match is dropped, so no free text reaches the table. */
export function clientInfo(headers: Record<string, string | undefined>): { appVersion: string | null; mailboxCount: number | null } {
  const version = headers['x-jarvis-app-version'];
  const count = headers['x-jarvis-mailbox-count'];
  const mailboxCount = count !== undefined && MAILBOX_COUNT.test(count) ? Number(count) : null;
  return {
    appVersion: version !== undefined && APP_VERSION.test(version) ? version : null,
    mailboxCount: mailboxCount !== null && mailboxCount <= 50 ? mailboxCount : null,
  };
}
