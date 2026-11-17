import test from 'node:test';
import assert from 'node:assert/strict';
import { createLocaleStore } from '../src/i18n/locale-store.ts';
import { keepsLocaleInUse, readStartupScreen } from '../src/lib/onboarding-flow.ts';

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

test('completing the intro avoids repeating it while quick setup remains incomplete', async () => {
  assert.equal(await readStartupScreen(async () => false, async () => 'true'), 'quick');
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

// R8: the backend reads the saved ui_locale for the messages it writes, and
// reads English when nothing is saved. Someone past the language step who
// never saved a choice gets the language on screen saved.
test('a user past the language step with nothing saved gets the language in use saved', async () => {
  const writes: string[] = [];
  const store = createLocaleStore('zh-HK', async () => '', async locale => { writes.push(locale); });
  await store.restore();
  assert.equal(store.getSnapshot().hasSavedChoice, false);
  assert.equal(await store.keepLocaleInUse(), true);
  assert.deepEqual(writes, ['zh-HK'], 'the Mac language shown on screen is the one saved');
  assert.equal(store.getSnapshot().hasSavedChoice, true);
  assert.equal(store.getSnapshot().locale, 'zh-HK');
  assert.equal(await store.keepLocaleInUse(), false, 'saved once');
  assert.deepEqual(writes, ['zh-HK']);
});

test('only someone past the welcome screen has the language in use saved', () => {
  assert.equal(keepsLocaleInUse('workspace'), true);
  assert.equal(keepsLocaleInUse('setup'), true);
  assert.equal(keepsLocaleInUse('quick'), true);
  assert.equal(keepsLocaleInUse('welcome'), false, 'first launch asks for the language');
  assert.equal(keepsLocaleInUse(null), false, 'not known yet');
});

test('a saved choice is never overwritten by the language in use', async () => {
  const writes: string[] = [];
  const store = createLocaleStore('zh-HK', async () => 'en', async locale => { writes.push(locale); });
  await store.restore();
  assert.equal(await store.keepLocaleInUse(), false);
  assert.deepEqual(writes, []);
  const unread = createLocaleStore('zh-HK', async () => { throw Error('database unavailable'); }, async locale => { writes.push(locale); });
  await unread.restore();
  assert.equal(await unread.keepLocaleInUse(), false, 'a failed read is not "nothing saved"');
  assert.deepEqual(writes, []);
});

test('a choice made while the language in use is being saved wins', async () => {
  const first = deferred<void>();
  let persisted = '';
  let writes = 0;
  const store = createLocaleStore('zh-HK', async () => '', async locale => {
    if (++writes === 1) await first.promise;
    persisted = locale;
  });
  await store.restore();
  const kept = store.keepLocaleInUse();
  await Promise.resolve();
  const chosen = store.setLocale('en');
  first.resolve();
  await Promise.all([kept, chosen]);
  assert.equal(persisted, 'en');
  assert.equal(store.getSnapshot().locale, 'en');
  assert.equal(store.getSnapshot().hasSavedChoice, true);
});

test('a failed save of the language in use changes nothing on screen', async () => {
  const store = createLocaleStore('zh-HK', async () => '', async () => { throw Error('database unavailable'); });
  await store.restore();
  assert.equal(await store.keepLocaleInUse(), false);
  assert.equal(store.getSnapshot().saveError, false, 'no error for a save the user did not ask for');
  assert.equal(store.getSnapshot().hasSavedChoice, false);
  assert.equal(store.getSnapshot().locale, 'zh-HK');
});
