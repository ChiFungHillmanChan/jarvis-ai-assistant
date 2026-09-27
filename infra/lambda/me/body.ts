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
