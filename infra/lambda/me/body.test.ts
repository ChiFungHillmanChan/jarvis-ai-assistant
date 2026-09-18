import test from 'node:test';
import assert from 'node:assert/strict';
import { meBody } from './body.ts';

test('a user with no row is on the free tier', () => {
  assert.deepEqual(meBody('sub-1', 'a@b.com', undefined), {
    sub: 'sub-1', email: 'a@b.com', tier: 'free', subscriptionStatus: null,
  });
});

test('a paid row is reported as stored', () => {
  assert.deepEqual(meBody('sub-1', 'a@b.com', { tier: 'pro', subscriptionStatus: 'active' }), {
    sub: 'sub-1', email: 'a@b.com', tier: 'pro', subscriptionStatus: 'active',
  });
});

test('an unrecognised stored tier is reported as free, never passed through', () => {
  assert.equal(meBody('s', null, { tier: 'platinum' }).tier, 'free');
});
