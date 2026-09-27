import test from 'node:test';
import assert from 'node:assert/strict';
import { meBody } from './body.ts';

test('a user with no row is on the free tier', () => {
  assert.deepEqual(meBody('sub-1', 'a@b.com', undefined), {
    sub: 'sub-1', email: 'a@b.com', tier: 'free', subscriptionStatus: null,
    trialEndsAt: null, periodEndsAt: null, cancelAtPeriodEnd: false, trialDays: 0,
  });
});

test('a paid row is reported as stored', () => {
  assert.deepEqual(meBody('sub-1', 'a@b.com', { tier: 'pro', subscriptionStatus: 'active', stripeSubscriptionId: 'sub_1' }, 28), {
    sub: 'sub-1', email: 'a@b.com', tier: 'pro', subscriptionStatus: 'active',
    trialEndsAt: null, periodEndsAt: null, cancelAtPeriodEnd: false, trialDays: 0,
  });
});

test('an unrecognised stored tier is reported as free, never passed through', () => {
  assert.equal(meBody('s', null, { tier: 'platinum' }).tier, 'free');
});

test('a first-time account is offered the configured trial; an account that has subscribed is not', () => {
  assert.equal(meBody('s', null, undefined, 28).trialDays, 28);
  assert.equal(meBody('s', null, { tier: 'free', stripeCustomerId: 'cus_1' }, 28).trialDays, 28);
  assert.equal(meBody('s', null, { tier: 'free', stripeSubscriptionId: 'sub_old' }, 28).trialDays, 0);
});

test('trial and renewal dates come from the row; anything malformed reads as absent', () => {
  const body = meBody('s', null, { tier: 'pro', subscriptionStatus: 'trialing', trialEndsAt: '2026-10-25T00:00:00.000Z', periodEndsAt: '2026-10-25T00:00:00.000Z', cancelAtPeriodEnd: true, stripeSubscriptionId: 'sub_1' }, 28);
  assert.equal(body.trialEndsAt, '2026-10-25T00:00:00.000Z');
  assert.equal(body.periodEndsAt, '2026-10-25T00:00:00.000Z');
  assert.equal(body.cancelAtPeriodEnd, true);
  const odd = meBody('s', null, { tier: 'pro', trialEndsAt: 1792800000, cancelAtPeriodEnd: 'yes' });
  assert.equal(odd.trialEndsAt, null);
  assert.equal(odd.cancelAtPeriodEnd, false);
});
