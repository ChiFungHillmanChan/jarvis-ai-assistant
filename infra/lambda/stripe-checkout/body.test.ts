import test from 'node:test';
import assert from 'node:assert/strict';
import { readCode, readTier } from './body.ts';

test('the tier is read from a JSON body', () => {
  assert.equal(readTier('{"tier":"pro"}', false), 'pro');
  assert.equal(readTier(Buffer.from('{"tier":"team"}').toString('base64'), true), 'team');
});

test('identity fields in the body are ignored, and bad input yields null', () => {
  assert.equal(readTier('{"tier":"pro","user_id":"someone-else","email":"x@y.z"}', false), 'pro');
  assert.equal(readTier('{"tier":"free"}', false), null);
  assert.equal(readTier('not json', false), null);
  assert.equal(readTier(undefined, false), null);
});

test('a code is trimmed; a missing or blank code means a normal checkout', () => {
  assert.equal(readCode('{"tier":"pro","code":"  PILOT100 "}', false), 'PILOT100');
  assert.equal(readCode(Buffer.from('{"code":"abc"}').toString('base64'), true), 'abc');
  assert.equal(readCode('{"tier":"pro"}', false), null);
  assert.equal(readCode('{"tier":"pro","code":"   "}', false), null);
  assert.equal(readCode('{"tier":"pro","code":null}', false), null);
});

test('a code that cannot be one is refused without a Stripe call', () => {
  assert.equal(readCode(JSON.stringify({ tier: 'pro', code: 'x'.repeat(65) }), false), false);
  assert.equal(readCode(JSON.stringify({ tier: 'pro', code: 'x'.repeat(64) }), false), 'x'.repeat(64));
  assert.equal(readCode('{"tier":"pro","code":42}', false), false);
});
