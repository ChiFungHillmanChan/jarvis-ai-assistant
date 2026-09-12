import test from 'node:test';
import assert from 'node:assert/strict';
import { catalog, interpolate, normalizeUiLocale, suggestedUiLocale } from '../src/i18n/catalog.ts';
import { displayTabTitle, initialWorkspace, restoreWorkspace, workspaceReducer } from '../src/workspace/model.ts';

test('English and Hong Kong catalogs have matching keys and variables', () => {
  assert.deepEqual(Object.keys(catalog.en).sort(), Object.keys(catalog['zh-HK']).sort());
  for (const key of Object.keys(catalog.en) as (keyof typeof catalog.en)[]) {
    const variables = (value: string) => [...value.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();
    assert.deepEqual(variables(catalog.en[key]), variables(catalog['zh-HK'][key]), key);
  }
});

test('interpolation treats source text literally', () => {
  assert.equal(interpolate('Close {title}', { title: '<script>{unsafe}</script>' }), 'Close <script>{unsafe}</script>');
});

test('Hong Kong locale is suggested without overwriting a saved choice', () => {
  assert.equal(suggestedUiLocale(['zh-HK', 'en-GB']), 'zh-HK');
  assert.equal(suggestedUiLocale(['en-GB', 'zh-HK']), 'en');
  assert.equal(normalizeUiLocale('en', 'zh-HK'), 'en');
  assert.equal(normalizeUiLocale('unexpected', 'zh-HK'), 'zh-HK');
});

test('default tab titles follow locale while custom titles survive restoration', () => {
  let state = workspaceReducer(initialWorkspace(), { type: 'open', view: 'email' });
  const mail = state.tabs.find(tab => tab.view === 'email')!;
  assert.equal(displayTabTitle(mail, 'zh-HK'), catalog['zh-HK'].navMail);
  state = workspaceReducer(state, { type: 'rename', id: mail.id, title: 'My mail' });
  const restored = restoreWorkspace(JSON.stringify(state));
  assert.equal(displayTabTitle(restored.tabs.find(tab => tab.view === 'email')!, 'zh-HK'), 'My mail');
  assert.equal(displayTabTitle(restored.tabs[0], 'zh-HK'), catalog['zh-HK'].navAutomation);
});
