import test from 'node:test';
import assert from 'node:assert/strict';
import { formatOption, groupOptions } from '../src/ui/options.ts';
import { nextIndex } from '../src/ui/keys.ts';
import { runAction } from '../src/ui/actions.ts';

test('formatOption joins the label and the hint, or returns the label alone', () => {
  assert.equal(formatOption({ value: 'a', label: 'Gemini 3.8 Flash', hint: 'lowest cost' }), 'Gemini 3.8 Flash · lowest cost');
  assert.equal(formatOption({ value: 'a', label: 'Gemini 3.8 Flash' }), 'Gemini 3.8 Flash');
});

test('groupOptions makes one run per consecutive group and keeps the order', () => {
  const opts = [
    { value: '1', label: '1', group: 'A' },
    { value: '2', label: '2', group: 'A' },
    { value: '3', label: '3' },
    { value: '4', label: '4', group: 'B' },
  ];
  const runs = groupOptions(opts);
  assert.deepEqual(runs.map(r => r.group), ['A', null, 'B']);
  assert.deepEqual(runs.map(r => r.items.map(i => i.value)), [['1', '2'], ['3'], ['4']]);
});

test('groupOptions on an ungrouped list returns one run with a null group', () => {
  const runs = groupOptions([{ value: '1', label: '1' }, { value: '2', label: '2' }]);
  assert.equal(runs.length, 1);
  assert.equal(runs[0].group, null);
  assert.equal(runs[0].items.length, 2);
});

test('nextIndex wraps on the arrow keys and jumps on Home and End', () => {
  assert.equal(nextIndex(0, 'ArrowRight', 3), 1);
  assert.equal(nextIndex(2, 'ArrowRight', 3), 0);
  assert.equal(nextIndex(0, 'ArrowLeft', 3), 2);
  assert.equal(nextIndex(0, 'ArrowDown', 3), 1);
  assert.equal(nextIndex(2, 'ArrowDown', 3), 0);
  assert.equal(nextIndex(0, 'ArrowUp', 3), 2);
  assert.equal(nextIndex(1, 'Home', 3), 0);
  assert.equal(nextIndex(1, 'End', 3), 2);
  assert.equal(nextIndex(1, 'a', 3), null);
});

test('nextIndex returns null when there is nothing to move to', () => {
  assert.equal(nextIndex(0, 'ArrowRight', 0), null);
});

test('a danger action that is declined does not run', async () => {
  let calls = 0;
  const ran = await runAction('danger', () => { calls++; }, { message: 'x' }, async () => false);
  assert.equal(ran, false);
  assert.equal(calls, 0);
});

test('a danger action that is confirmed runs once and gets the message and label', async () => {
  let calls = 0;
  let asked: [string, string | undefined] | null = null;
  const ran = await runAction('danger', () => { calls++; }, { message: 'x', label: 'Delete' }, async (m, l) => { asked = [m, l]; return true; });
  assert.equal(ran, true);
  assert.equal(calls, 1);
  assert.deepEqual(asked, ['x', 'Delete']);
});

test('a primary action runs without asking', async () => {
  let calls = 0;
  let asks = 0;
  const ran = await runAction('primary', () => { calls++; }, undefined, async () => { asks++; return true; });
  assert.equal(ran, true);
  assert.equal(calls, 1);
  assert.equal(asks, 0);
});

test('a danger action with no confirm message throws', async () => {
  await assert.rejects(
    runAction('danger', () => {}, undefined, async () => true),
    { message: 'danger actions need a confirm message' },
  );
});

test('runAction awaits an async action', async () => {
  let done = false;
  await runAction('secondary', async () => { await Promise.resolve(); done = true; }, undefined, async () => true);
  assert.equal(done, true);
});

// Component source checks: tests cannot import .tsx, so these read the files as text.
import { readFileSync } from 'node:fs';
const src = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

test('Select is a native select with optgroup, a label and a described-by hint', () => {
  const s = src('src/ui/Select.tsx');
  assert.match(s, /<select/);
  assert.match(s, /optgroup/);
  assert.match(s, /groupOptions\(/);
  assert.match(s, /aria-describedby/);
  assert.match(s, /aria-label/);
  assert.match(s, /required=\{required\}/, 'a required select stays required');
});

test('Switch is a role=switch button with aria-checked', () => {
  const s = src('src/ui/Switch.tsx');
  assert.match(s, /role="switch"/);
  assert.match(s, /aria-checked/);
});

test('Segmented is a radiogroup that moves focus with nextIndex', () => {
  const s = src('src/ui/Segmented.tsx');
  assert.match(s, /role="radiogroup"/);
  assert.match(s, /role="radio"/);
  assert.match(s, /aria-checked/);
  assert.match(s, /tabIndex/);
  assert.match(s, /nextIndex\(/);
});

test('Button runs its action through runAction with the confirm dialog', () => {
  const s = src('src/ui/Button.tsx');
  assert.match(s, /runAction\(/);
  assert.match(s, /confirmAction/);
});

test('a danger Button requires confirm in its type', () => {
  const s = src('src/ui/Button.tsx');
  assert.match(s, /kind: 'danger';\s*confirm: \{/);
});

test('TextField commits on blur and Enter, SecretField never takes the secret value', () => {
  const t = src('src/ui/TextField.tsx');
  assert.match(t, /onBlur/);
  assert.match(t, /'Enter'/);
  const s = src('src/ui/SecretField.tsx');
  assert.match(s, /saved: boolean/);
  assert.match(s, /type="password"/);
  assert.match(s, /已儲存在鑰匙圈/);
});

test('Switch is 46px by 26px with a 20px thumb that travels 20px', () => {
  const css = src('src/ui/ui.css');
  assert.match(css, /\.ui-switch \{[^}]*width:46px/);
  assert.match(css, /\.ui-switch \{[^}]*height:26px/);
  assert.match(css, /\.ui-switch::after \{[^}]*width:20px; height:20px/);
  assert.match(css, /translateX\(20px\)/);
});

test('dark control background is the board value', () => {
  assert.match(src('src/styles/tokens.css'), /--ui-control-bg: #131925;/);
});

test('TextField ignores Enter while an IME composes', () => {
  const t = src('src/ui/TextField.tsx');
  assert.match(t, /isComposing/);
  assert.match(t, /229/);
  assert.match(t, /committed/);
});

test('Button catches a rejecting action and guards re-entry', () => {
  const b = src('src/ui/Button.tsx');
  assert.match(b, /catch/);
  assert.match(b, /onError/);
  assert.match(b, /running\.current/);
});

test('SecretField props carry no secret value and Remove has an error path', () => {
  const s = src('src/ui/SecretField.tsx');
  const props = s.match(/interface SecretFieldProps \{[^}]*\}/)?.[0] ?? '';
  assert.ok(props.length > 0);
  assert.doesNotMatch(props, /\b(value|secret)\b/);
  assert.match(s, /async function remove\(\)[\s\S]*catch/);
});

test('SettingRow and SettingGroup exist', () => {
  assert.match(src('src/ui/SettingRow.tsx'), /export default function SettingRow/);
  assert.match(src('src/ui/SettingGroup.tsx'), /export default function SettingGroup/);
});

test('tokens define the ui variables for dark, light and system-light', () => {
  const css = src('src/styles/tokens.css');
  for (const v of ['--ui-control-bg', '--ui-control-border', '--ui-control-border-hover', '--ui-focus', '--ui-focus-glow', '--ui-danger-bg', '--ui-danger-border', '--ui-danger-text', '--ui-good', '--ui-good-bg', '--ui-warn']) {
    assert.equal(css.split(`${v}:`).length - 1, 3, `${v} should be defined for dark, light and system`);
  }
  assert.match(css, /:root\[data-appearance="light"\]/);
  assert.match(css, /prefers-color-scheme: light[\s\S]*data-appearance="system"/);
});

test('main.tsx imports tokens.css after App, which pulls in workspace.css', () => {
  const m = src('src/main.tsx');
  assert.match(m, /import "\.\/styles\/tokens\.css"/);
  assert.ok(m.indexOf('./App') < m.indexOf('./styles/tokens.css'));
  assert.match(src('src/App.tsx'), /styles\/workspace\.css/);
});
