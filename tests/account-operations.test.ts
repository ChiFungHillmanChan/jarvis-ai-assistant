import test from 'node:test';
import assert from 'node:assert/strict';
import { beginAccountSessionChange, finishAccountSessionChange, createAccountOperationGuard } from '../src/lib/account.ts';

test('a deferred billing response cannot create a page or poll after sign-out and re-login', async () => {
  const guard = createAccountOperationGuard();
  let resolve!: (url: string) => void;
  const response = new Promise<string>(r => { resolve = r; });
  const current = guard.capture();
  const pages: string[] = [];
  let polls = 0;
  const completion = response.then(url => { if (current()) { pages.push(url); polls++; } });
  const logout = beginAccountSessionChange();
  finishAccountSessionChange(logout);
  const login = beginAccountSessionChange();
  finishAccountSessionChange(login);
  resolve('https://billing.stripe.com/p/session/old-account');
  await completion;
  assert.deepEqual(pages, []);
  assert.equal(polls, 0);
  assert.equal(guard.capture()(), true, 'the new account can start a new operation');
});

test('unmount invalidates a deferred page and a stale operation cannot clear newer busy state', async () => {
  const guard = createAccountOperationGuard();
  const old = guard.capture();
  guard.invalidate();
  const newer = guard.capture();
  assert.equal(old(), false);
  assert.equal(newer(), true);
  guard.invalidate();
  assert.equal(newer(), false);
});
