import test, { afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { getOptionalPreference, getPreference, checkOnboardingStatus } from '../src/lib/commands.ts';
import { createLocaleStore } from '../src/i18n/locale-store.ts';
import { readStartupScreen } from '../src/lib/onboarding-flow.ts';

(globalThis as unknown as { window: typeof globalThis }).window = globalThis;
afterEach(() => clearMocks());

test('a new native settings map without locale or intro keys opens the language guide', async () => {
  mockIPC(command => {
    if (command === 'get_settings') return { values: {} };
    if (command === 'check_onboarding_status') return false;
    if (command === 'get_user_preference') throw Error('db read: Query returned no rows');
    throw Error(`Unexpected command: ${command}`);
  });
  const store = createLocaleStore('en', () => getOptionalPreference('ui_locale'), async () => {});
  await store.restore();
  assert.equal(store.getSnapshot().ready, true);
  assert.equal(store.getSnapshot().loadError, false);
  assert.equal(store.getSnapshot().hasSavedChoice, false);
  assert.equal(await readStartupScreen(checkOnboardingStatus, () => getOptionalPreference('onboarding_intro_complete')), 'welcome');
});

test('saved native settings restore the selected language and skip the completed intro', async () => {
  mockIPC(command => {
    if (command === 'get_settings') return { values: { ui_locale: 'zh-HK', onboarding_intro_complete: 'true' } };
    if (command === 'check_onboarding_status') return false;
    throw Error(`Unexpected command: ${command}`);
  });
  const store = createLocaleStore('en', () => getOptionalPreference('ui_locale'), async () => {});
  await store.restore();
  assert.equal(store.getSnapshot().locale, 'zh-HK');
  assert.equal(store.getSnapshot().hasSavedChoice, true);
  assert.equal(await readStartupScreen(checkOnboardingStatus, () => getOptionalPreference('onboarding_intro_complete')), 'setup');
});

test('a native settings read failure remains an error rather than becoming a first launch', async () => {
  mockIPC(command => {
    if (command === 'get_settings') throw Error('database unavailable');
    if (command === 'check_onboarding_status') return false;
    throw Error(`Unexpected command: ${command}`);
  });
  const store = createLocaleStore('en', () => getOptionalPreference('ui_locale'), async () => {});
  await store.restore();
  assert.equal(store.getSnapshot().ready, false);
  assert.equal(store.getSnapshot().loadError, true);
  await assert.rejects(readStartupScreen(checkOnboardingStatus, () => getOptionalPreference('onboarding_intro_complete')), /database unavailable/);
});

test('the existing required preference reader still rejects missing native preferences', async () => {
  mockIPC(command => {
    if (command === 'get_user_preference') throw Error('db read: Query returned no rows');
    throw Error(`Unexpected command: ${command}`);
  });
  await assert.rejects(getPreference('missing'), /Query returned no rows/);
});
