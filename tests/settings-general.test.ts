import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { nameToShow } from '../src/lib/user-name.ts';
import { followsUi, replyLanguageAfterUiChange, replyLanguageForUi, replyLanguageOnLocaleChange } from '../src/settings/language.ts';
import { ROWS } from '../src/settings/rows.ts';

// Task 6: the one-name helper, the reply-language rule and the General
// section. The JSX cannot be imported under node, so its wiring is checked in
// the source.

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

test('nameToShow prefers the trimmed assistant_address', () => {
  assert.equal(nameToShow({ assistant_address: '  Boss  ', display_name: 'Hillman', user_name: 'Hillman Chan' }), 'Boss');
});

test('nameToShow falls back to display_name, then user_name', () => {
  assert.equal(nameToShow({ assistant_address: '', display_name: ' Hillman ', user_name: 'Hillman Chan' }), 'Hillman');
  assert.equal(nameToShow({ assistant_address: null, display_name: null, user_name: ' Hillman Chan ' }), 'Hillman Chan');
  assert.equal(nameToShow({ user_name: 'Hillman Chan' }), 'Hillman Chan');
});

test('nameToShow skips whitespace-only values and gives null when all are empty', () => {
  assert.equal(nameToShow({ assistant_address: '   ', display_name: '\t', user_name: 'Sam' }), 'Sam');
  assert.equal(nameToShow({ assistant_address: ' ', display_name: '', user_name: '  ' }), null);
  assert.equal(nameToShow({}), null);
});

test('the reply language for each interface language', () => {
  assert.equal(replyLanguageForUi('en'), 'en');
  assert.equal(replyLanguageForUi('zh-HK'), 'yue');
});

test('followsUi reads the saved flag when there is one', () => {
  assert.equal(followsUi({ language_follows_ui: 'true' }, 'en'), true);
  assert.equal(followsUi({ language_follows_ui: 'false' }, 'en'), false);
  assert.equal(followsUi({ language_follows_ui: 'false', language: 'en' }, 'en'), false);
});

test('with the flag missing, followsUi is true only when the saved language matches the UI', () => {
  assert.equal(followsUi({ language: 'yue' }, 'zh-HK'), true);
  assert.equal(followsUi({ language: 'en' }, 'zh-HK'), false);
  assert.equal(followsUi({ language: 'bilingual' }, 'zh-HK'), false);
  assert.equal(followsUi({ language: 'en' }, 'en'), true);
  assert.equal(followsUi({}, 'zh-HK'), true);
  assert.equal(followsUi({ language: null, language_follows_ui: null }, 'en'), true);
});

test('changing the UI language moves a following reply language and leaves a chosen one', () => {
  assert.equal(replyLanguageAfterUiChange('zh-HK', { language: 'en', language_follows_ui: 'true' }), 'yue');
  assert.equal(replyLanguageAfterUiChange('en', { language: 'yue', language_follows_ui: 'true' }), 'en');
  assert.equal(replyLanguageAfterUiChange('zh-HK', { language: 'en' }), null, 'an old deliberate choice is kept');
  assert.equal(replyLanguageAfterUiChange('zh-HK', { language: 'en', language_follows_ui: 'false' }), null);
  assert.equal(replyLanguageAfterUiChange('zh-HK', {}), 'yue');
});

// Fix round 1: with the flag missing, whether the reply language follows is
// judged against the interface language in use BEFORE the change (spec 6.3).
test('a missing flag is judged against the current UI, so seeded en replies follow an en user to zh-HK', () => {
  assert.equal(replyLanguageOnLocaleChange('en', 'zh-HK', { language: 'en' }), 'yue');
  assert.equal(replyLanguageOnLocaleChange('zh-HK', 'en', { language: 'en' }), null);
  assert.equal(replyLanguageOnLocaleChange('zh-HK', 'en', { language: 'yue' }), 'en');
  assert.equal(replyLanguageOnLocaleChange('en', 'zh-HK', { language: 'bilingual' }), null);
  assert.equal(replyLanguageOnLocaleChange('en', 'zh-HK', {}), 'yue');
});

test('a saved flag wins over the current UI', () => {
  assert.equal(replyLanguageOnLocaleChange('en', 'zh-HK', { language: 'en', language_follows_ui: 'false' }), null);
  assert.equal(replyLanguageOnLocaleChange('zh-HK', 'en', { language: 'en', language_follows_ui: 'true' }), 'en');
});

test('the pure helpers import no React and no .tsx', () => {
  for (const path of ['../src/lib/user-name.ts', '../src/settings/language.ts']) {
    const code = source(path);
    assert.doesNotMatch(code, /from ['"]react['"]/, path);
    assert.doesNotMatch(code, /\.tsx['"]/, path);
  }
});

test('the dialog shows General through its own component', () => {
  const dialog = source('../src/settings/SettingsDialog.tsx');
  assert.match(dialog, /import General from '\.\/sections\/General'/);
  assert.match(dialog, /general: General/);
});

test('General renders every General row, in order, by its row id', () => {
  const general = source('../src/settings/sections/General.tsx');
  const ids = ROWS.filter(r => r.section === 'general').map(r => r.id);
  assert.ok(ids.length >= 13);
  let last = -1;
  // A row is placed by row('<id>') (title, description and DOM id) or, for an
  // anchor around a reused component, by id="<id>".
  const place = (id: string) => [general.indexOf(`row('${id}')`), general.indexOf(`id="${id}"`)].filter(i => i !== -1).sort((a, b) => a - b)[0] ?? -1;
  for (const id of ids) {
    const at = place(id);
    assert.ok(at !== -1, `row ${id} is missing from General.tsx`);
    assert.ok(at > last, `row ${id} is out of order in General.tsx`);
    last = at;
  }
});

test('General has no Display name row and no raw select', () => {
  const general = source('../src/settings/sections/General.tsx');
  assert.doesNotMatch(general, /display_name/);
  assert.doesNotMatch(general, /DISPLAY_NAME_KEY/);
  assert.doesNotMatch(general, /<select/);
});

test('General keeps the reply-language flag and applies the UI rule', () => {
  const general = source('../src/settings/sections/General.tsx');
  assert.match(general, /'language_follows_ui', 'false'/);
  assert.match(general, /'language_follows_ui', 'true'/);
  assert.match(general, /replyLanguageOnLocaleChange\(locale, next, values\)/);
  assert.match(general, /accountSectionVisible\(/);
  assert.match(general, /<BackgroundSettings \/>/);
});

test('a failed save is shown in both languages', () => {
  const general = source('../src/settings/sections/General.tsx');
  assert.match(general, /Could not save\. Try again\./);
  assert.match(general, /無法儲存，請再試一次。/);
});

test('General zh-HK copy is written Chinese, never colloquial', () => {
  const general = source('../src/settings/sections/General.tsx');
  const zh = [...general.matchAll(/text\('[^']*', '([^']*)'\)/g)].map(m => m[1]).join('');
  assert.ok(zh.length > 0);
  assert.doesNotMatch(zh, /[嘅咗唔係冇啲喺嚟]/);
});

test('the sidebar shows nameToShow from the three saved names', () => {
  const sidebar = source('../src/components/SidebarAccount.tsx');
  assert.match(sidebar, /nameToShow\(/);
  assert.doesNotMatch(sidebar, /DISPLAY_NAME_KEY/);
});

test('a direct reply-language change writes the flag first, then the language', () => {
  const general = source('../src/settings/sections/General.tsx');
  assert.match(general, /save\('reply-language', \['language_follows_ui', 'false'\], \['language', v\]\)/);
});

test('General shows a loading line, not the controls, until the values are loaded', () => {
  const general = source('../src/settings/sections/General.tsx');
  assert.match(general, /if \(!ready\)/);
  assert.match(general, /text\('Loading settings…', '正在載入設定…'\)/);
});

test('a failed theme save shows at the theme row and the theme rolls back', () => {
  const general = source('../src/settings/sections/General.tsx');
  assert.match(general, /setAppearance\(v\)[\s\S]{0,120}theme: true/);
  const workspace = source('../src/workspace/WorkspaceContext.tsx');
  assert.match(workspace, /setAppearance: \(value: string\) => Promise<void>/);
  assert.match(workspace, /setAppearanceValue\(previous\)/);
});

test('the one reply-language select records a direct choice; Voice has no second one', () => {
  // The old page also had a reply-language select under Voice. The dialog has
  // one, in General, and it records the direct choice before the language.
  const general = source('../src/settings/sections/General.tsx');
  assert.match(general, /save\('reply-language', \['language_follows_ui', 'false'\], \['language', v\]\)/);
  const voice = source('../src/settings/sections/Voice.tsx') + source('../src/settings/hooks/useVoiceSettings.ts');
  assert.doesNotMatch(voice, /['"]language['"]\s*[,\]]/, 'Voice does not write the reply language');
});
