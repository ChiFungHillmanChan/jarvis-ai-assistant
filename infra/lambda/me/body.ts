export interface MeBody {
  sub: string;
  email: string | null;
  tier: 'free' | 'pro' | 'team';
  subscriptionStatus: string | null;
}

const TIERS = new Set(['free', 'pro', 'team']);

/** A missing row is the free tier; a value the webhook would never write is also free. */
export function meBody(sub: string, email: string | null, item: Record<string, unknown> | undefined): MeBody {
  const stored = typeof item?.tier === 'string' ? item.tier : 'free';
  const status = typeof item?.subscriptionStatus === 'string' ? item.subscriptionStatus : null;
  return {
    sub,
    email,
    tier: (TIERS.has(stored) ? stored : 'free') as MeBody['tier'],
    subscriptionStatus: status,
  };
}
