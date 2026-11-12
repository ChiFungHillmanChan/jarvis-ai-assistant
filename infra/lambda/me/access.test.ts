import test from 'node:test';
import assert from 'node:assert/strict';
import { accessRequest } from './access.ts';

const now = new Date('2026-11-15T04:00:00.000Z');

test('a first request is recorded', () => assert.deepEqual(accessRequest(undefined, now), { write: true, requestedAt: '2026-11-15T04:00:00.000Z' }));

test('a repeat within 24 hours returns the stored time without writing', () =>
  assert.deepEqual(accessRequest({ aiAccessRequestedAt: '2026-11-14T05:00:00.000Z' }, now), { write: false, requestedAt: '2026-11-14T05:00:00.000Z' }));

test('after 24 hours, or with a malformed stored value, it is recorded again', () => {
  assert.equal(accessRequest({ aiAccessRequestedAt: '2026-11-14T03:59:59.000Z' }, now).write, true);
  assert.equal(accessRequest({ aiAccessRequestedAt: 12345 }, now).write, true);
});
