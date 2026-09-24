import test, { afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { accountSignOut, paymentsStartCheckout, paymentsOpenPortal, paymentsReopenUrl } from '../src/lib/commands.ts';
import { accountChanged, onAccountChanged, createAccountOperationGuard, onAccountSessionChange } from '../src/lib/account.ts';
import type { AccountStatus } from '../src/lib/types.ts';

(globalThis as unknown as { window: EventTarget }).window = new EventTarget();
afterEach(() => clearMocks());

test('checkout preserves existing subscription and browser failure for the account UI', async () => {
  mockIPC((command, args) => {
    assert.equal(command, 'payments_start_checkout');
    assert.deepEqual(args, { tier: 'pro', locale: 'zh-HK' });
    return { url: 'https://billing.stripe.com/p/session/test', session_id: 'bps_test', already_subscribed: true, browser_opened: false };
  });
  const page = await paymentsStartCheckout('pro', 'zh-HK');
  assert.equal(page.already_subscribed, true);
  assert.equal(page.browser_opened, false);
});

test('billing portal and reopen use the dedicated native commands', async () => {
  const calls: string[] = [];
  mockIPC((command, args) => {
    calls.push(command);
    if (command === 'payments_open_portal') assert.deepEqual(args, { locale: 'zh-HK' });
    if (command === 'payments_reopen_url') assert.deepEqual(args, { url: 'https://billing.stripe.com/p/session/test', sessionGeneration: 7 });
    return { url: 'https://billing.stripe.com/p/session/test', browser_opened: true, session_generation: 7 };
  });
  const portal = await paymentsOpenPortal('zh-HK');
  await paymentsReopenUrl(portal.url, portal.session_generation);
  assert.deepEqual(calls, ['payments_open_portal', 'payments_reopen_url']);
});

test('account notifications carry the exact unknown status without requiring a cached reread', () => {
  const unknown: AccountStatus = { available: true, signed_in: true, email: 'test@example.com', tier: null };
  const statuses: (AccountStatus | undefined)[] = [];
  const stop = onAccountChanged(status => statuses.push(status));
  accountChanged(unknown);
  stop();
  accountChanged();
  assert.deepEqual(statuses, [unknown]);
});

test('native sign-out invalidates pending billing before the deferred IPC finishes', async () => {
  let finish!: () => void;
  mockIPC(command => {
    assert.equal(command, 'account_sign_out');
    return new Promise<void>(resolve => { finish = resolve; });
  });
  const lifecycle: boolean[] = [];
  const stop = onAccountSessionChange(changing => lifecycle.push(changing));
  const current = createAccountOperationGuard().capture();
  const pending = accountSignOut();
  assert.equal(current(), false);
  assert.deepEqual(lifecycle, [true]);
  finish();
  await pending;
  assert.deepEqual(lifecycle, [true, false]);
  stop();
});
