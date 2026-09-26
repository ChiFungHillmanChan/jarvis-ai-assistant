import test from 'node:test';
import assert from 'node:assert/strict';
import { createStatusCache, STATUS_TTL_MS } from '../src/lib/connection-status.ts';

function harness() {
  let now = 0;
  const calls: string[] = [];
  const pending: ((value: string) => void)[] = [];
  const cache = createStatusCache<string, string>(key => {
    calls.push(key);
    return new Promise(resolve => pending.push(resolve));
  }, () => now);
  return { cache, calls, pending, advance: (ms: number) => { now += ms; } };
}

test('a checked status is shown again at once instead of being re-checked', async () => {
  const h = harness();
  const first = h.cache.load('notion');
  h.pending.shift()!('connected');
  assert.equal(await first, 'connected');
  assert.equal(h.cache.peek('notion'), 'connected');
  assert.equal(await h.cache.load('notion'), 'connected');
  assert.deepEqual(h.calls, ['notion']);
});

test('pages opened together share one check', async () => {
  const h = harness();
  const a = h.cache.load('github');
  const b = h.cache.load('github');
  h.pending.shift()!('connected');
  assert.deepEqual(await Promise.all([a, b]), ['connected', 'connected']);
  assert.deepEqual(h.calls, ['github']);
});

test('a stale status is re-checked, and a forced check skips the cache', async () => {
  const h = harness();
  const first = h.cache.load('github');
  h.pending.shift()!('connected');
  await first;
  h.advance(STATUS_TTL_MS);
  const stale = h.cache.load('github');
  h.pending.shift()!('not_connected');
  assert.equal(await stale, 'not_connected');
  const forced = h.cache.load('github', true);
  h.pending.shift()!('connected');
  assert.equal(await forced, 'connected');
  assert.deepEqual(h.calls, ['github', 'github', 'github']);
});

test('a check that started before a change never overwrites the newer status', async () => {
  const h = harness();
  const old = h.cache.load('notion');
  h.cache.invalidate();
  const fresh = h.cache.load('notion');
  h.pending[1]('connected');
  assert.equal(await fresh, 'connected');
  h.pending[0]('not_connected');
  await old;
  assert.equal(h.cache.peek('notion'), 'connected');
});

test('a failed check keeps nothing and the next load tries again', async () => {
  const h = harness();
  const cache = createStatusCache<string, string>(async key => { h.calls.push(key); throw new Error('offline'); }, () => 0);
  await assert.rejects(cache.load('obsidian'));
  assert.equal(cache.peek('obsidian'), null);
  await assert.rejects(cache.load('obsidian'));
  assert.deepEqual(h.calls, ['obsidian', 'obsidian']);
});
