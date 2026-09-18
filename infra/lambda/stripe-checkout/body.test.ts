import test from 'node:test';
import assert from 'node:assert/strict';
import { readTier } from './body.ts';

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
