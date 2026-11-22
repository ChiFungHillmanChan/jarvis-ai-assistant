import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { commitFocusedField, initialSettingsDialog, isBackdropClick, isSettingsShortcut, settingsDialogReducer, type SettingsDialogState } from '../src/settings/settings-dialog-state.ts';

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
  assert.match(dialog, /onPointerDown=\{e => \{ pointerDown\.current = e\.target; \}\}/);
  assert.match(dialog, /isBackdropClick\(pointerDown\.current, e\.target, dialog\.current, dialog\.current\.getBoundingClientRect\(\), e\.clientX, e\.clientY\)/, 'a click on the backdrop closes');
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
  assert.match(css, /@media \(max-width: 1000px\)\s*\{[^}]*width:\s*94vw;[^}]*height:\s*90vh;/);
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
  assert.match(app, /id: 'settings', title: text\('Settings', '設定'\), hint: text\('Workspace', '工作區'\), action: \(\) => openSettings\(\)/);
});

test('every entry point opens the dialog', () => {
  assert.match(source('../src/components/SidebarAccount.tsx'), /openSettings\(/);
  const connections = source('../src/workspace/ConnectionsPage.tsx');
  assert.doesNotMatch(connections, /writeValue\("tool:settings"/);
  assert.match(connections, /openSettings\(section === 'google' \? 'connections' : 'advanced-cloud'\)/);
  // Final review M5: Bedrock credentials live in Advanced → Cloud providers, and the button names it.
  assert.match(connections, /const cloudLabel = sectionLabel\('advanced-cloud', text\);/);
  assert.match(connections, /onClick=\{\(\) => openSettings\("cloud"\)\}>\{text\(`Open Advanced → \$\{cloudLabel\}`, `開啟「進階 → \$\{cloudLabel\}」`\)\}/);
});

test('the old page and its bridge are gone; every section has its own view', () => {
  for (const gone of ['../src/pages/Settings.tsx', '../src/settings/LegacySection.tsx']) assert.ok(!existsSync(new URL(gone, import.meta.url)), gone);
  const dialog = source('../src/settings/SettingsDialog.tsx');
  assert.doesNotMatch(dialog, /LegacySection|pages\/Settings/);
  assert.match(dialog, /const SECTION_VIEWS: Record<SettingsSectionId, ComponentType> = \{/, 'a missing view is a type error');
  assert.match(dialog, /<ErrorBoundary key=\{section\}><View \/><\/ErrorBoundary>/);
  assert.doesNotMatch(source('../src/settings/sections.ts'), /legacySectionsFor/);
  assert.doesNotMatch(source('../src/App.tsx'), /pages\/Settings/);
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

// Fix round 1.
const dialogEl = { tag: 'dialog' };
const input = { tag: 'input' };
const rect = { left: 100, top: 100, right: 900, bottom: 700 };

test('a press and release on the backdrop closes', () => {
  assert.equal(isBackdropClick(dialogEl, dialogEl, dialogEl, rect, 50, 50), true);
  assert.equal(isBackdropClick(dialogEl, dialogEl, dialogEl, rect, 950, 400), true);
  assert.equal(isBackdropClick(dialogEl, dialogEl, dialogEl, rect, 400, 750), true);
});

test('a drag from inside released over the backdrop does not close', () => {
  // The click lands on the common ancestor, the dialog, outside its box.
  assert.equal(isBackdropClick(input, dialogEl, dialogEl, rect, 50, 50), false);
});

test('a press that starts on the backdrop and ends inside does not close', () => {
  assert.equal(isBackdropClick(dialogEl, input, dialogEl, rect, 400, 400), false);
});

test('a click on the dialog itself inside its box (border, padding) does not close', () => {
  assert.equal(isBackdropClick(dialogEl, dialogEl, dialogEl, rect, 100, 400), false);
  assert.equal(isBackdropClick(dialogEl, dialogEl, dialogEl, rect, 400, 700), false);
});

test('no recorded press or no dialog never closes', () => {
  assert.equal(isBackdropClick(null, dialogEl, dialogEl, rect, 50, 50), false);
  assert.equal(isBackdropClick(dialogEl, dialogEl, null, rect, 50, 50), false);
});

test('a native close updates the dialog state, and focus is restored once', () => {
  const dialog = source('../src/settings/SettingsDialog.tsx');
  assert.match(dialog, /el\.addEventListener\('close', onClose\)/);
  assert.match(dialog, /function onClose\(\) \{ closeSettings\(\); finishClose\(\); \}/);
  assert.match(dialog, /const target = restoreFocus\.current;\s*restoreFocus\.current = null;\s*target\?\.focus\(\);/, 'the ref is cleared before focusing, so a second run does nothing');
  assert.match(dialog, /\} else \{\s*if \(el\.open\) el\.close\(\);\s*finishClose\(\);\s*\}/, 'the close branch resets even when the element is already closed');
});

test('Your AI links to the mail analysis settings inside the dialog', () => {
  // The old page's "Mail analysis settings" opened the Mail tab (closing the
  // dialog first when bridged). Those settings now live in Mail and
  // follow-ups, so the link moves within the dialog and closes nothing.
  const ai = source('../src/settings/sections/YourAi.tsx');
  assert.match(ai, /<p id="ai-mail-link" className="settings-ai-footer">/);
  assert.match(ai, /onClick=\{\(\) => openSettings\('mail'\)\}>\{row\('ai-mail-link'\)\.title\}<\/Button>/);
  assert.match(ai, /text\('The AI you choose here answers you in chat and sorts your mail\. Mail analysis can use a different AI in its own settings\.', '你在此選擇的 AI 會在對話中回覆你，並整理你的郵件。郵件分析亦可在其設定中另選 AI。'\)/);
  assert.doesNotMatch(ai, /open\('email'\)|closeSettings/);
});

test('App documents that only one ConfirmDialog is mounted at a time', () => {
  const app = source('../src/App.tsx');
  assert.match(app, /Only one ConfirmDialog may be mounted at a time/);
});

// Final review I4: a half-typed field is saved before the content unmounts.
function focused(insideDialog: boolean) {
  const calls: string[] = [];
  return { calls, el: { closest: (selector: string) => (calls.push(`closest ${selector}`), insideDialog ? {} : null), blur: () => calls.push('blur') } };
}

test('a field focused inside the dialog is blurred, so its blur save starts', () => {
  const field = focused(true);
  commitFocusedField({ activeElement: field.el });
  assert.deepEqual(field.calls, ['closest .settings-dialog', 'blur']);
});

test('focus outside the dialog, or no focus, is left alone', () => {
  const outside = focused(false);
  commitFocusedField({ activeElement: outside.el });
  assert.deepEqual(outside.calls, ['closest .settings-dialog']);
  assert.doesNotThrow(() => commitFocusedField({ activeElement: null }));
  assert.doesNotThrow(() => commitFocusedField({ activeElement: {} }));
});

test('every close and section change blurs the focused field first', () => {
  const provider = source('../src/settings/SettingsContext.tsx');
  assert.match(provider, /const openSettings = useCallback\(\(section\?: SettingsSectionId\) => \{ commitFocusedField\(document\); dispatch\(\{ type: 'open', section \}\); \}, \[\]\);/);
  assert.match(provider, /const closeSettings = useCallback\(\(\) => \{ commitFocusedField\(document\); dispatch\(\{ type: 'close' \}\); \}, \[\]\);/);
  const dialog = source('../src/settings/SettingsDialog.tsx');
  // Esc (cancel), X and the backdrop all close through closeSettings; the nav goes through openSettings.
  assert.match(dialog, /function cancel\(e: Event\) \{[\s\S]*?closeSettings\(\);/);
  assert.match(dialog, /onClick=\{closeSettings\}><X size=\{18\} \/>/);
  assert.ok(/e\.clientX, e\.clientY\)\) closeSettings\(\);/.test(dialog), 'the backdrop closes through closeSettings');
  assert.match(dialog, /function go\(id: SettingsSectionId\) \{\s*setQuery\(''\);\s*openSettings\(id\);/);
  assert.doesNotMatch(dialog, /dispatch\(/, 'the dialog never changes state around the provider');
});

// Final review I3: rows stack on narrow content, and fixed control widths never overflow.
test('a setting row stacks when the dialog content is narrow', () => {
  const css = source('../src/settings/settings.css');
  assert.match(css, /\.settings-dialog-body \{ container: settings-body \/ inline-size;/);
  const narrow = css.match(/@container settings-body \(max-width: 560px\) \{([\s\S]*?)\n\}/)?.[1] ?? '';
  assert.match(narrow, /\.ui-setting-row \{ flex-wrap: wrap;/);
  assert.match(narrow, /\.ui-setting-text \{ flex-basis: 100%; \}/);
  assert.match(narrow, /\.ui-setting-control \{ flex-basis: 100%;/);
  assert.match(css, /\.settings-dialog-frame \{ container: settings-frame \/ inline-size;/);
  assert.match(css, /@container settings-frame \(max-width: 639px\)/);
  const ui = source('../src/ui/ui.css');
  assert.match(ui, /\.ui-setting-control \{ flex-shrink:0; max-width:100%; \}/);
  assert.match(ui, /\.ui-setting-control > \* \{ max-width:100%; \}/);
});
