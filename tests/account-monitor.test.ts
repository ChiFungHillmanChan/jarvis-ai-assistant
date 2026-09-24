import test from 'node:test';
import assert from 'node:assert/strict';
import { createAccountMonitor, PAYMENT_WAIT_MS, PAYMENT_POLL_MS } from '../src/lib/account-monitor.ts';
import type { AccountStatus } from '../src/lib/types.ts';

const free: AccountStatus = { available: true, signed_in: true, email: 'test@example.com', tier: 'free' };
const pro = { ...free, tier: 'pro' };

function harness(fetchStatus: (force: boolean) => Promise<AccountStatus>) {
  let now = 0;
  let next = 0;
  const timers = new Map<number, { at: number; run: () => void }>();
  const statuses: AccountStatus[] = [];
  const payments: (string | null)[] = [];
  const errors: unknown[] = [];
  const monitor = createAccountMonitor({
    fetchStatus, onStatus: s => statuses.push(s), onPayment: s => payments.push(s), onError: e => errors.push(e),
    now: () => now,
    schedule: (run, delay) => { const id = ++next; timers.set(id, { at: now + delay, run }); return () => timers.delete(id); },
  });
  const advance = async (ms: number) => {
    now += ms;
    for (const [id, timer] of [...timers]) if (timer.at <= now) { timers.delete(id); timer.run(); }
    await new Promise(resolve => setImmediate(resolve));
  };
  return { monitor, statuses, payments, errors, advance, timers };
}

test('checkout polls the account until the server confirms a paid tier, then stops', async () => {
  const forces: boolean[] = [];
  const h = harness(async force => { forces.push(force); return forces.length === 1 ? free : pro; });
  h.monitor.waitForPayment();
  await h.advance(0);
  assert.deepEqual(h.payments, ['waiting']);
  await h.advance(PAYMENT_POLL_MS);
  assert.deepEqual(h.payments, ['waiting', 'confirmed']);
  assert.deepEqual(forces, [true, true]);
  assert.equal(h.timers.size, 0);
});

test('a resident account monitor bypasses a recently read Free plan on browser return', async () => {
  const forces: boolean[] = [];
  const h = harness(async force => { forces.push(force); return force ? pro : free; });
  await h.monitor.refresh();
  await Promise.all([h.monitor.refresh(true), h.monitor.refresh(true)]);
  assert.deepEqual(forces, [false, true], 'focus and visibility share one forced read');
  assert.equal(h.statuses.at(-1)?.tier, 'pro');
});

test('offline and timeout never report payment confirmed or keep polling', async () => {
  let requests = 0;
  const h = harness(async () => { requests++; return { ...free, tier: null }; });
  h.monitor.waitForPayment();
  await h.advance(0);
  await h.advance(PAYMENT_WAIT_MS);
  assert.equal(h.payments.at(-1), 'unconfirmed');
  assert.equal(h.payments.includes('confirmed'), false);
  assert.equal(h.timers.size, 0);
  assert.equal(requests, 1, 'a queued poll must not make another request after the deadline');
});

test('focus and timer refreshes share one in-flight request; late results after timeout are not payment confirmation', async () => {
  let resolve!: (status: AccountStatus) => void;
  let requests = 0;
  const h = harness(() => { requests++; return new Promise(r => { resolve = r; }); });
  h.monitor.waitForPayment();
  void h.monitor.refresh(true);
  assert.equal(requests, 1);
  await h.advance(PAYMENT_WAIT_MS);
  resolve(pro);
  await h.advance(0);
  assert.equal(h.payments.at(-1), 'unconfirmed');
  assert.equal(h.timers.size, 0);
});

test('sign-out cancels payment checks and ignores an older account response', async () => {
  let resolve!: (status: AccountStatus) => void;
  const h = harness(() => new Promise(r => { resolve = r; }));
  h.monitor.waitForPayment();
  h.monitor.cancelPayment();
  resolve(pro);
  await h.advance(0);
  assert.equal(h.statuses.length, 0);
  assert.equal(h.payments.includes('confirmed'), false);
  assert.equal(h.timers.size, 0);
});

test('unmount cancels timers and ignores a pending response', async () => {
  let resolve!: (status: AccountStatus) => void;
  const h = harness(() => new Promise(r => { resolve = r; }));
  h.monitor.waitForPayment();
  h.monitor.dispose();
  resolve(pro);
  await h.advance(0);
  assert.equal(h.statuses.length, 0);
  assert.equal(h.payments.includes('confirmed'), false);
  assert.equal(h.timers.size, 0);
});

test('IPC failure remains an error and a signed-out response cannot confirm payment', async () => {
  let attempts = 0;
  const h = harness(async () => { if (++attempts === 1) throw new Error('offline'); return { ...free, signed_in: false, tier: null }; });
  h.monitor.waitForPayment();
  await h.advance(0);
  assert.equal(h.errors.length, 1);
  await h.advance(PAYMENT_POLL_MS);
  assert.equal(h.payments.at(-1), 'unconfirmed');
  assert.equal(h.timers.size, 0);
});
