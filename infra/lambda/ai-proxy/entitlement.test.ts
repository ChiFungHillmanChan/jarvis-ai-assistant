import test from 'node:test';
import assert from 'node:assert/strict';
import { loadTs } from '../../test-support/load-ts.cjs';

const { DEFAULT_CONFIG } = loadTs('lambda/ai-proxy/config.ts');
const { resolvePlan, activeGrant, hkMonth, denial, jarvisAiStatus } = loadTs('lambda/ai-proxy/entitlement.ts');

const now = new Date('2026-11-15T04:00:00Z');
const grant = (over = {}) => ({ grantId: 'g1', type: 'trial', creditMicros: 20_000_000, validFrom: '2026-11-01T00:00:00Z', validUntil: '2026-12-31T00:00:00Z', createdAt: '2026-11-01T00:00:00Z', ...over });
const NONE = { entitled: false, source: null, state: 'none', periodKey: null, periodEndsAt: null, includedMicros: 0, capMicros: 0, hardStop: true, meterToStripe: false };
const pro = { tier: 'pro', subscriptionStatus: 'active', periodStartsAt: '2026-11-03T00:00:00.000Z', periodEndsAt: '2026-12-03T00:00:00.000Z' };
// Subscriptions entitle only when the config turns them on; the shipped default is off.
const SUBS = { ...DEFAULT_CONFIG, subscriptionAccess: true };

test('no row and no grant is not entitled', () => assert.deepEqual(resolvePlan(undefined, [], DEFAULT_CONFIG, now), NONE));

test('active Pro uses the Stripe period with US$20 included, US$60 cap and billed overage', () =>
  assert.deepEqual(resolvePlan(pro, [], SUBS, now), { entitled: true, source: 'subscription', state: 'pro', periodKey: 's2026-11-03T00:00:00.000Z', periodEndsAt: '2026-12-03T00:00:00.000Z', includedMicros: 20_000_000, capMicros: 60_000_000, hardStop: false, meterToStripe: true }));

test('a Stripe trial stops hard at the trial credit and is never metered', () => {
  const p = resolvePlan({ ...pro, subscriptionStatus: 'trialing' }, [], SUBS, now);
  assert.deepEqual([p.state, p.capMicros, p.hardStop, p.meterToStripe], ['trial', 20_000_000, true, false]);
});

test('a subscriber without periodStartsAt falls back to the Hong Kong month', () => {
  const p = resolvePlan({ tier: 'pro', subscriptionStatus: 'active' }, [], SUBS, now);
  assert.deepEqual([p.periodKey, p.periodEndsAt], ['m2026-11', '2026-11-30T16:00:00.000Z']);
});

test('past_due, team and unknown tiers are not entitled', () => {
  for (const meta of [{ ...pro, subscriptionStatus: 'past_due' }, { ...pro, tier: 'team' }, { tier: 'platinum', subscriptionStatus: 'active' }])
    assert.deepEqual(resolvePlan(meta, [], SUBS, now), NONE);
});

test('an active grant gives its monthly credit as a hard stop', () => {
  assert.deepEqual(resolvePlan(undefined, [grant()], DEFAULT_CONFIG, now), { entitled: true, source: 'grant', state: 'grant_trial', periodKey: 'm2026-11', periodEndsAt: '2026-11-30T16:00:00.000Z', includedMicros: 20_000_000, capMicros: 20_000_000, hardStop: true, meterToStripe: false });
  assert.equal(resolvePlan(undefined, [grant({ type: 'complimentary_pro' })], DEFAULT_CONFIG, now).state, 'complimentary');
});

test('a grant wins over an active subscription', () => {
  const p = resolvePlan(pro, [grant()], SUBS, now);
  assert.deepEqual([p.source, p.periodKey, p.meterToStripe, p.hardStop], ['grant', 'm2026-11', false, true]);
});

test('aiCapMicros overrides the cap for Pro and grants; grants stay a hard stop', () => {
  assert.equal(resolvePlan({ ...pro, aiCapMicros: 5_000_000 }, [], SUBS, now).capMicros, 5_000_000);
  const g = resolvePlan({ aiCapMicros: 90_000_000 }, [grant()], DEFAULT_CONFIG, now);
  assert.deepEqual([g.capMicros, g.hardStop], [90_000_000, true]);
  for (const odd of [1.5, '5000000', -1, 0]) assert.equal(resolvePlan({ ...pro, aiCapMicros: odd }, [], SUBS, now).capMicros, 60_000_000);
});

test('while subscriptionAccess is off a subscription never entitles', () => {
  for (const meta of [pro, { ...pro, subscriptionStatus: 'trialing' }, { tier: 'pro', subscriptionStatus: 'active' }, { ...pro, aiCapMicros: 5_000_000 }])
    assert.deepEqual(resolvePlan(meta, [], DEFAULT_CONFIG, now), NONE, JSON.stringify(meta));
  assert.equal(denial(resolvePlan(pro, [], DEFAULT_CONFIG, now), 0, false), 'not_entitled');
  // /me reports the same state as a user with no subscription, so the access request stays available.
  const requested = { ...pro, aiAccessRequestedAt: '2026-11-14T00:00:00.000Z' };
  const s = jarvisAiStatus(resolvePlan(requested, [], DEFAULT_CONFIG, now), 5_000_000, false, requested);
  assert.deepEqual(s, { entitled: false, source: null, state: 'none', periodKey: null, periodEndsAt: null, usedMicros: 0, includedMicros: 0, capMicros: 0, hardStop: true, paused: false, accessRequestedAt: '2026-11-14T00:00:00.000Z' });
});

test('while subscriptionAccess is off a grant still entitles and suspension still wins', () => {
  assert.deepEqual(resolvePlan(pro, [grant()], DEFAULT_CONFIG, now), resolvePlan(undefined, [grant()], SUBS, now));
  assert.equal(resolvePlan(pro, [grant()], DEFAULT_CONFIG, now).state, 'grant_trial');
  assert.deepEqual(resolvePlan({ ...pro, aiSuspended: true }, [grant()], DEFAULT_CONFIG, now), { ...NONE, state: 'suspended' });
  assert.deepEqual(resolvePlan({ ...pro, aiSuspended: true }, [], SUBS, now), { ...NONE, state: 'suspended' });
});

test('a suspended user is never entitled, even with a grant', () =>
  assert.deepEqual(resolvePlan({ aiSuspended: true }, [grant()], DEFAULT_CONFIG, now), { ...NONE, state: 'suspended' }));

test('activeGrant ignores revoked, expired and future grants and picks the newest', () => {
  assert.equal(activeGrant([grant({ revokedAt: '2026-11-02T00:00:00Z' })], now), null);
  assert.equal(activeGrant([grant({ validUntil: '2026-11-15T04:00:00Z' })], now), null); // validUntil is exclusive
  assert.equal(activeGrant([grant({ validFrom: '2026-11-16T00:00:00Z' })], now), null);
  assert.equal(activeGrant([grant(), grant({ grantId: 'g2', createdAt: '2026-11-10T00:00:00Z' })], now).grantId, 'g2');
});

test('a grant without a valid creditMicros is not active, so the user is not entitled', () => {
  const { creditMicros, ...missing } = grant();
  for (const g of [missing, grant({ creditMicros: 0 }), grant({ creditMicros: 1.5 }), grant({ creditMicros: '20000000' })]) {
    assert.equal(activeGrant([g], now), null, JSON.stringify(g));
    assert.deepEqual(resolvePlan(undefined, [g], DEFAULT_CONFIG, now), NONE);
  }
});

test('hkMonth uses Hong Kong time at the month boundary', () => {
  assert.deepEqual(hkMonth(new Date('2026-10-31T15:59:59Z')), { key: 'm2026-10', endsAt: '2026-10-31T16:00:00.000Z' });
  assert.deepEqual(hkMonth(new Date('2026-10-31T16:00:00Z')), { key: 'm2026-11', endsAt: '2026-11-30T16:00:00.000Z' });
  assert.deepEqual(hkMonth(new Date('2026-12-31T16:30:00Z')), { key: 'm2027-01', endsAt: '2027-01-31T16:00:00.000Z' });
});

test('denial codes and their order', () => {
  const g = resolvePlan(undefined, [grant()], DEFAULT_CONFIG, now);
  assert.equal(denial(resolvePlan({ aiSuspended: true }, [grant()], DEFAULT_CONFIG, now), 0, true), 'suspended');
  assert.equal(denial(NONE, 0, true), 'not_entitled');
  assert.equal(denial(g, 0, true), 'paused');
  assert.equal(denial(g, 19_999_999, false), null);
  assert.equal(denial(g, 20_000_000, false), 'cap_reached');
  assert.equal(denial(resolvePlan({ ...pro, subscriptionStatus: 'trialing' }, [], SUBS, now), 20_000_000, false), 'trial_credit_used');
  assert.equal(denial(resolvePlan(pro, [], SUBS, now), 60_000_000, false), 'cap_reached');
});

test('jarvisAiStatus reports usage, pause and the pending access request', () => {
  const s = jarvisAiStatus(resolvePlan(undefined, [grant()], DEFAULT_CONFIG, now), 1_234, true, { aiAccessRequestedAt: '2026-11-14T00:00:00.000Z' });
  assert.deepEqual([s.usedMicros, s.paused, s.accessRequestedAt, s.state], [1_234, true, '2026-11-14T00:00:00.000Z', 'grant_trial']);
  assert.equal(jarvisAiStatus(NONE, 999, false, undefined).usedMicros, 0);
});
