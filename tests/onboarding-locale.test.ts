import test from 'node:test';
import assert from 'node:assert/strict';
import { createLocaleStore } from '../src/i18n/locale-store.ts';
import { readStartupScreen } from '../src/lib/onboarding-flow.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test('first launch requires language choice; saved choice resumes the guide', async () => {
  const store = createLocaleStore('zh-HK', async () => '', async () => {});
  await store.restore();
  assert.equal(store.getSnapshot().hasSavedChoice, false, 'the system suggestion is not a user choice');
  assert.equal(await readStartupScreen(async () => false, async () => ''), 'welcome');
  assert.equal(await store.setLocale('en'), true);
  const reopened = createLocaleStore('zh-HK', async () => 'en', async () => {});
  await reopened.restore();
  assert.equal(reopened.getSnapshot().locale, 'en');
  assert.equal(reopened.getSnapshot().hasSavedChoice, true);
});

test('completing the intro avoids repeating it while mail setup remains incomplete', async () => {
  assert.equal(await readStartupScreen(async () => false, async () => 'true'), 'setup');
  assert.equal(await readStartupScreen(async () => true, async () => { throw Error('must not read intro for existing users'); }), 'workspace');
});

test('startup read failures are not treated as completed onboarding', async () => {
  await assert.rejects(readStartupScreen(async () => { throw Error('database unavailable'); }, async () => ''), /database unavailable/);
  await assert.rejects(readStartupScreen(async () => false, async () => { throw Error('database unavailable'); }), /database unavailable/);
});

test('a slow saved-language read cannot overwrite a new choice', async () => {
  const read = deferred<string>();
  const store = createLocaleStore('en', () => read.promise, async () => {});
  const restore = store.restore();
  await store.setLocale('zh-HK');
  read.resolve('en');
  await restore;
  assert.equal(store.getSnapshot().locale, 'zh-HK');
  assert.equal(store.getSnapshot().hasSavedChoice, true);
});

test('rapid language changes are persisted in selection order', async () => {
  const first = deferred<void>();
  let persisted = '';
  let writes = 0;
  const store = createLocaleStore('en', async () => '', async locale => {
    if (++writes === 1) await first.promise;
    persisted = locale;
  });
  const chinese = store.setLocale('zh-HK');
  const english = store.setLocale('en');
  await Promise.resolve();
  assert.equal(store.getSnapshot().locale, 'en');
  assert.equal(store.getSnapshot().saving, true);
  first.resolve();
  await Promise.all([chinese, english]);
  assert.equal(persisted, 'en', 'reopening must use the last selected language');
  assert.equal(store.getSnapshot().saving, false);
});

test('failed language writes do not confirm the choice and can be retried', async () => {
  let unavailable = true;
  const store = createLocaleStore('en', async () => '', async () => { if (unavailable) throw Error('database unavailable'); });
  assert.equal(await store.setLocale('zh-HK'), false);
  assert.equal(store.getSnapshot().hasSavedChoice, false);
  assert.equal(store.getSnapshot().saveError, true);
  unavailable = false;
  assert.equal(await store.setLocale('zh-HK'), true);
  assert.equal(store.getSnapshot().hasSavedChoice, true);
  assert.equal(store.getSnapshot().saveError, false);
});

test('failed locale restoration exposes a retryable error instead of guessing a saved language', async () => {
  let unavailable = true;
  const store = createLocaleStore('en', async () => { if (unavailable) throw Error('database unavailable'); return 'zh-HK'; }, async () => {});
  await store.restore();
  assert.equal(store.getSnapshot().loadError, true);
  assert.equal(store.getSnapshot().ready, false);
  unavailable = false;
  await store.restore();
  assert.equal(store.getSnapshot().ready, true);
  assert.equal(store.getSnapshot().locale, 'zh-HK');
});
