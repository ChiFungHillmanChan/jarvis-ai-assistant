import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { initialSettingsDialog, isSettingsShortcut, settingsDialogReducer, type SettingsDialogState } from '../src/settings/settings-dialog-state.ts';

// Task 5: the settings dialog shell. The open/close/section logic is a pure
// reducer tested here; the JSX files cannot be imported under node, so their
// wiring is checked in the source.

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const reduce = (state: SettingsDialogState, ...actions: Parameters<typeof settingsDialogReducer>[1][]) =>
  actions.reduce(settingsDialogReducer, state);

test('the dialog starts closed on General with the setup guide closed', () => {
  assert.deepEqual(initialSettingsDialog, { open: false, section: 'general', setupOpen: false });
});

test('opening with no section lands on General, with a section on that section', () => {
  assert.deepEqual(reduce(initialSettingsDialog, { type: 'open' }), { open: true, section: 'general', setupOpen: false });
  assert.deepEqual(reduce(initialSettingsDialog, { type: 'open', section: 'connections' }), { open: true, section: 'connections', setupOpen: false });
});

test('a new open with no section starts at General again, not the last section', () => {
  const state = reduce(initialSettingsDialog, { type: 'open', section: 'usage' }, { type: 'close' }, { type: 'open' });
  assert.equal(state.section, 'general');
});

test('opening a section while open moves to it and keeps the dialog open', () => {
  const state = reduce(initialSettingsDialog, { type: 'open' }, { type: 'open', section: 'advanced-models' });
  assert.deepEqual(state, { open: true, section: 'advanced-models', setupOpen: false });
});

test('closing keeps the section and is a no-op when already closed', () => {
  const open = reduce(initialSettingsDialog, { type: 'open', section: 'voice' });
  assert.deepEqual(settingsDialogReducer(open, { type: 'close' }), { open: false, section: 'voice', setupOpen: false });
  assert.equal(settingsDialogReducer(initialSettingsDialog, { type: 'close' }), initialSettingsDialog);
});

test('opening the setup guide closes the dialog; settings cannot open over it', () => {
  const open = reduce(initialSettingsDialog, { type: 'open', section: 'setup' });
  const setup = settingsDialogReducer(open, { type: 'open-setup' });
  assert.equal(setup.open, false);
  assert.equal(setup.setupOpen, true);
  assert.equal(settingsDialogReducer(setup, { type: 'open' }), setup, 'Cmd+, during the setup guide does nothing');
  assert.equal(settingsDialogReducer(setup, { type: 'close-setup' }).setupOpen, false);
  assert.equal(settingsDialogReducer(initialSettingsDialog, { type: 'close-setup' }), initialSettingsDialog);
});

test('Cmd+, is the shortcut; Ctrl+, and a plain comma are not', () => {
  assert.equal(isSettingsShortcut({ metaKey: true, key: ',' }), true);
  assert.equal(isSettingsShortcut({ metaKey: false, key: ',' }), false);
  assert.equal(isSettingsShortcut({ metaKey: true, key: '.' }), false);
});

test('the dialog uses showModal, the native cancel event and a labelled close button', () => {
  const dialog = source('../src/settings/SettingsDialog.tsx');
  assert.match(dialog, /showModal\(/);
  assert.match(dialog, /'cancel'/);
  assert.match(dialog, /aria-label=\{text\('Close settings', '關閉設定'\)\}/);
  assert.match(dialog, /aria-current=/);
  assert.match(dialog, /<Select\b/, 'the narrow nav is the shared Select');
  assert.match(dialog, /searchRows\(/);
  assert.match(dialog, /e\.target === dialog\.current/, 'a click on the backdrop closes');
});

test('Cmd+, is registered once, in the provider', () => {
  const files = ['App.tsx', 'settings/SettingsDialog.tsx', 'settings/SettingsContext.tsx', 'components/SidebarAccount.tsx'];
  const owners = files.filter(file => source(`../src/${file}`).includes('isSettingsShortcut('));
  assert.deepEqual(owners, ['settings/SettingsContext.tsx']);
});

test('the dialog size and backdrop match the spec', () => {
  const css = source('../src/settings/settings.css');
  assert.match(css, /width:\s*min\(78vw,\s*1120px\)/);
  assert.match(css, /height:\s*min\(80vh,\s*780px\)/);
  assert.match(css, /@media \(max-width: 900px\)\s*\{[^}]*width:\s*94vw;[^}]*height:\s*90vh;/);
  assert.match(css, /::backdrop\s*\{[^}]*rgba\(0,\s*0,\s*0,\s*\.45\)[^}]*backdrop-filter:\s*blur\(2px\)/);
});

test('the titlebar Set up button is gone and the setup guide state lives in the provider', () => {
  const app = source('../src/App.tsx');
  assert.doesNotMatch(app, /設定導引/);
  assert.doesNotMatch(app, /useState\(false\);\s*const openSetup/);
  assert.match(app, /useSettingsDialog\(\)/);
  assert.match(app, /<SettingsProvider>/);
  assert.match(app, /<SettingsDialog \/>/);
  assert.match(app, /closeSetupGuide\(\); dispatch\(\{ type: 'open', view: 'email' \}\);/, 'completing setup still opens Mail');
  assert.match(app, /id: 'settings', title: viewName\('settings', locale\), hint: text\('Workspace', '工作區'\), action: \(\) => openSettings\(\)/);
});

test('every entry point opens the dialog', () => {
  assert.match(source('../src/components/SidebarAccount.tsx'), /openSettings\(/);
  const connections = source('../src/workspace/ConnectionsPage.tsx');
  assert.doesNotMatch(connections, /writeValue\("tool:settings"/);
  assert.match(connections, /openSettings\(section === 'google' \? 'connections' : 'ai'\)/);
});

test('the bridge renders the old sections a new section replaces', () => {
  const bridge = source('../src/settings/LegacySection.tsx');
  assert.match(bridge, /<Settings embedded=\{\{ sections \}\} \/>/);
  assert.match(bridge, /legacySectionsFor\(id\)/);
  assert.match(bridge, /This section moves here soon\./);
  assert.match(bridge, /此部分即將移到這裏。/);
  const settings = source('../src/pages/Settings.tsx');
  assert.match(settings, /export default function Settings\(\{ embedded \}: \{ embedded\?: \{ sections: string\[\] \} \} = \{\}\)/);
  assert.match(settings, /const shows = \(id: string\) => embedded \? embedded\.sections\.includes\(id\) : section === id;/);
  assert.doesNotMatch(settings, /\{section === "/, 'every render condition goes through shows()');
});

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? files(path) : [path];
  });
}

test('closing never cancels a pending save', () => {
  assert.doesNotMatch(source('../src/settings/SettingsDialog.tsx'), /AbortController/);
  const offenders = files('src/settings').filter(file => readFileSync(file, 'utf8').includes('abort('));
  assert.deepEqual(offenders, []);
});
