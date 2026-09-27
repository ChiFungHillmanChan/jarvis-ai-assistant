import test from 'node:test';
import assert from 'node:assert/strict';
import { formatPlanDate, gateScreen, planLine } from '../src/lib/paywall.ts';
import type { AccountStatus } from '../src/lib/types.ts';

const free: AccountStatus = {
  available: true, signed_in: true, email: 'a@example.com', tier: 'free',
  subscription_status: null, trial_ends_at: null, period_ends_at: null,
  cancel_at_period_end: false, trial_days: 28, plan_checked: 'live',
};

test('the gate waits for the first status and lets free and dev builds straight through', () => {
  assert.equal(gateScreen(null, null), 'checking');
  assert.equal(gateScreen({ ...free, available: false, signed_in: false, tier: null, plan_checked: 'none' }, null), 'open');
});

test('a signed-out user is asked to sign in, even mid-payment', () => {
  assert.equal(gateScreen({ ...free, signed_in: false, tier: null, plan_checked: 'none' }, null), 'sign-in');
  assert.equal(gateScreen({ ...free, signed_in: false, tier: null, plan_checked: 'none' }, 'waiting'), 'sign-in');
});

test('a paid plan opens the app, whether confirmed now or saved while offline', () => {
  assert.equal(gateScreen({ ...free, tier: 'pro' }, null), 'open');
  assert.equal(gateScreen({ ...free, tier: 'team' }, null), 'open');
  assert.equal(gateScreen({ ...free, tier: 'pro', plan_checked: 'saved' }, null), 'open');
});

test('a payment confirmed after the two-minute wait still opens the app', () => {
  assert.equal(gateScreen({ ...free, tier: 'pro' }, 'unconfirmed'), 'open');
});

test('a free account sees the offer, then the payment wait, then the unconfirmed screen', () => {
  assert.equal(gateScreen(free, null), 'offer');
  assert.equal(gateScreen(free, 'waiting'), 'waiting');
  assert.equal(gateScreen(free, 'unconfirmed'), 'unconfirmed');
});

test('an unreachable server with nothing saved shows the offline screen, never the offer', () => {
  assert.equal(gateScreen({ ...free, tier: null, plan_checked: 'none' }, null), 'offline');
});

test('an unexpected tier string is never treated as paid', () => {
  assert.equal(gateScreen({ ...free, tier: 'PRO' }, null), 'offer');
});

test('Settings shows the trial end while trialing, then the renewal or end date', () => {
  assert.equal(planLine(free), null);
  const trialing = { ...free, tier: 'pro', subscription_status: 'trialing', trial_ends_at: '2026-10-25T00:00:00.000Z', period_ends_at: '2026-10-25T00:00:00.000Z' };
  assert.deepEqual(planLine(trialing), { key: 'planTrialEnds', date: '2026-10-25T00:00:00.000Z' });
  const active = { ...free, tier: 'pro', subscription_status: 'active', period_ends_at: '2026-11-25T00:00:00.000Z' };
  assert.deepEqual(planLine(active), { key: 'planRenews', date: '2026-11-25T00:00:00.000Z' });
  assert.deepEqual(planLine({ ...active, cancel_at_period_end: true }), { key: 'planEnds', date: '2026-11-25T00:00:00.000Z' });
  assert.equal(planLine({ ...active, period_ends_at: null }), null);
});

test('plan dates follow the interface language, and a malformed date is shown as given', () => {
  assert.match(formatPlanDate('2026-10-25T00:00:00.000Z', 'en'), /2026/);
  assert.match(formatPlanDate('2026-10-25T00:00:00.000Z', 'zh-HK'), /2026/);
  assert.equal(formatPlanDate('not a date', 'en'), 'not a date');
});
