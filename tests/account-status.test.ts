import test from 'node:test';
import assert from 'node:assert/strict';
import { accountLabel, accountSectionVisible, tierKey } from '../src/lib/account.ts';

test('known tiers map to their labels', () => {
  assert.equal(tierKey('free'), 'tierFree');
  assert.equal(tierKey('pro'), 'tierPro');
  assert.equal(tierKey('team'), 'tierTeam');
});

test('an unknown or missing tier never reads as a paid plan', () => {
  for (const value of [null, undefined, '', 'enterprise', 'PRO']) {
    assert.equal(tierKey(value), 'tierUnknown', `for ${JSON.stringify(value)}`);
  }
});

test('the typed display name wins, then the account email, then the fallback', () => {
  assert.equal(accountLabel('Hillman', 'h@example.com', 'Settings').label, 'Hillman');
  const byEmail = accountLabel('  ', 'h@example.com', 'Settings');
  assert.equal(byEmail.label, 'h@example.com');
  assert.equal(byEmail.initial, 'H');
  assert.equal(byEmail.isPlaceholder, false);
  const fallback = accountLabel(null, null, 'Settings');
  assert.equal(fallback.label, 'Settings');
  assert.equal(fallback.isPlaceholder, true);
});

test('the Settings account section only shows once accounts are known to be available', () => {
  assert.equal(accountSectionVisible({ available: true, signed_in: false, email: null, tier: null }), true);
  assert.equal(accountSectionVisible({ available: false, signed_in: false, email: null, tier: null }), false);
  assert.equal(accountSectionVisible(null), false);
  assert.equal(accountSectionVisible(undefined), false);
});
