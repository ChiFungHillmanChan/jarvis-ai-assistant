import type { AiConfig } from './config';

export interface Grant {
  grantId: string;
  type: 'trial' | 'complimentary_pro';
  creditMicros: number;
  validFrom: string;
  validUntil: string;
  createdAt: string;
  revokedAt?: string;
}

export type AiState = 'trial' | 'pro' | 'grant_trial' | 'complimentary' | 'suspended' | 'none';

export interface Plan {
  entitled: boolean;
  source: 'subscription' | 'grant' | null;
  state: AiState;
  periodKey: string | null;
  periodEndsAt: string | null;
  includedMicros: number;
  capMicros: number;
  /** True: stop at the cap. False: usage past includedMicros is billed, up to the cap. */
  hardStop: boolean;
  meterToStripe: boolean;
}

export type Denial = 'suspended' | 'not_entitled' | 'paused' | 'cap_reached' | 'trial_credit_used';

const HK_OFFSET_MS = 8 * 3_600_000;

const NONE: Plan = { entitled: false, source: null, state: 'none', periodKey: null, periodEndsAt: null, includedMicros: 0, capMicros: 0, hardStop: true, meterToStripe: false };

/** The Hong Kong calendar month containing now (UTC+8, no DST). */
export function hkMonth(now: Date): { key: string; endsAt: string } {
  const hk = new Date(now.getTime() + HK_OFFSET_MS);
  const year = hk.getUTCFullYear();
  const month = hk.getUTCMonth();
  return {
    key: `m${year}-${String(month + 1).padStart(2, '0')}`,
    endsAt: new Date(Date.UTC(year, month + 1, 1) - HK_OFFSET_MS).toISOString(),
  };
}

/** Newest grant that is not revoked and covers now; validUntil is exclusive. A grant without a valid credit is ignored (fail closed). */
export function activeGrant(grants: Grant[], now: Date): Grant | null {
  const t = now.getTime();
  let best: Grant | null = null;
  for (const g of grants) {
    if (g.revokedAt || !Number.isSafeInteger(g.creditMicros) || !(g.creditMicros > 0) || !(Date.parse(g.validFrom) <= t && t < Date.parse(g.validUntil))) continue;
    if (!best || Date.parse(g.createdAt) > Date.parse(best.createdAt)) best = g;
  }
  return best;
}

export function resolvePlan(meta: Record<string, unknown> | undefined, grants: Grant[], config: AiConfig, now: Date): Plan {
  if (meta?.aiSuspended === true) return { ...NONE, state: 'suspended' };
  const override = Number.isSafeInteger(meta?.aiCapMicros) && (meta!.aiCapMicros as number) > 0 ? (meta!.aiCapMicros as number) : null;

  const grant = activeGrant(grants, now);
  if (grant) {
    const month = hkMonth(now);
    return {
      entitled: true, source: 'grant', state: grant.type === 'trial' ? 'grant_trial' : 'complimentary',
      periodKey: month.key, periodEndsAt: month.endsAt,
      includedMicros: grant.creditMicros, capMicros: override ?? grant.creditMicros, hardStop: true, meterToStripe: false,
    };
  }

  // Until subscriptionAccess is on, access is approved by hand: a subscription is treated as no subscription.
  const status = meta?.subscriptionStatus;
  if (config.subscriptionAccess && meta?.tier === 'pro' && (status === 'active' || status === 'trialing')) {
    const { periodStartsAt, periodEndsAt } = meta;
    const month = hkMonth(now);
    const stripePeriod = typeof periodStartsAt === 'string' && typeof periodEndsAt === 'string';
    const periodKey = stripePeriod ? `s${periodStartsAt}` : month.key;
    const periodEnd = stripePeriod ? periodEndsAt : month.endsAt;
    if (status === 'trialing') {
      return {
        entitled: true, source: 'subscription', state: 'trial', periodKey, periodEndsAt: periodEnd,
        includedMicros: config.trialCreditMicros, capMicros: override ?? config.trialCreditMicros, hardStop: true, meterToStripe: false,
      };
    }
    return {
      entitled: true, source: 'subscription', state: 'pro', periodKey, periodEndsAt: periodEnd,
      includedMicros: config.includedMicros, capMicros: override ?? config.defaultCapMicros, hardStop: false, meterToStripe: true,
    };
  }

  return { ...NONE };
}

/** Why a request must be refused, in priority order, or null when it may proceed. */
export function denial(plan: Plan, usedMicros: number, paused: boolean): Denial | null {
  if (plan.state === 'suspended') return 'suspended';
  if (!plan.entitled) return 'not_entitled';
  if (paused) return 'paused';
  if (usedMicros >= plan.capMicros) return plan.state === 'trial' ? 'trial_credit_used' : 'cap_reached';
  return null;
}

export interface JarvisAiStatus {
  entitled: boolean;
  source: Plan['source'];
  state: AiState;
  periodKey: string | null;
  periodEndsAt: string | null;
  usedMicros: number;
  includedMicros: number;
  capMicros: number;
  hardStop: boolean;
  paused: boolean;
  accessRequestedAt: string | null;
}

export function jarvisAiStatus(plan: Plan, usedMicros: number, paused: boolean, meta: Record<string, unknown> | undefined): JarvisAiStatus {
  return {
    entitled: plan.entitled,
    source: plan.source,
    state: plan.state,
    periodKey: plan.periodKey,
    periodEndsAt: plan.periodEndsAt,
    usedMicros: plan.entitled ? usedMicros : 0,
    includedMicros: plan.includedMicros,
    capMicros: plan.capMicros,
    hardStop: plan.hardStop,
    paused,
    accessRequestedAt: typeof meta?.aiAccessRequestedAt === 'string' ? meta.aiAccessRequestedAt : null,
  };
}
