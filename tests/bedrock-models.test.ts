import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import {
  BUILT_IN_REGION_NOTE, OTHER_MODEL, groupLabel, groupedOptions, initialPicker, listStatus, pickerOptions, pickerReducer, selectValue,
  type BedrockModelList,
} from '../src/lib/bedrock-models.ts';
import { ACTIVITY_EVENTS, activityGate, userHasActed, watchUserActivity } from '../src/lib/user-activity.ts';

// Fix round 7: the Bedrock model picker. The options and the line under the
// select are pure functions; Settings.tsx is JSX, so its wiring is checked in
// the source (a real window shows the rest).

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;
const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

const awsList: BedrockModelList = {
  source: 'aws',
  region: 'ap-southeast-1',
  reason: null,
  missing_permissions: [],
  default_model: { id: 'global.anthropic.claude-sonnet-4-6', name: 'Claude Sonnet 4.6' },
  models: [
    { id: 'apac.amazon.nova-micro-v1:0', name: 'APAC Nova Micro', kind: 'profile' },
    { id: 'amazon.nova-micro-v1:0', name: 'Nova Micro', kind: 'model' },
  ],
};

const builtIn: BedrockModelList = {
  source: 'fallback',
  region: 'us-east-1',
  reason: 'not_allowed',
  missing_permissions: ['bedrock:ListInferenceProfiles', 'bedrock:ListFoundationModels'],
  default_model: { id: 'global.anthropic.claude-sonnet-4-6', name: 'Claude Sonnet 4.6' },
  models: [
    { id: 'global.anthropic.claude-sonnet-4-6', name: 'Claude Sonnet 4.6', kind: 'profile' },
    { id: 'us.amazon.nova-micro-v1:0', name: 'Amazon Nova Micro', kind: 'profile' },
  ],
};

// Breaks if the default stops being first and empty, the AWS profiles stop
// coming before the models, or "Other" stops being last.
test('the AWS list: the named default, then the profiles, then the models, then Other', () => {
  const options = pickerOptions(awsList, '', '', en);
  assert.deepEqual(options, [
    { value: '', group: 'default', label: 'Default: Claude Sonnet 4.6 (global.anthropic.claude-sonnet-4-6)' },
    { value: 'apac.amazon.nova-micro-v1:0', group: 'profiles', label: 'APAC Nova Micro (apac.amazon.nova-micro-v1:0)' },
    { value: 'amazon.nova-micro-v1:0', group: 'models', label: 'Nova Micro (amazon.nova-micro-v1:0)' },
    { value: OTHER_MODEL, group: 'other', label: 'Other (enter an ID)' },
  ]);
  assert.deepEqual(groupedOptions(options).map((run) => [run.group, groupLabel(run.group, en)]), [
    ['default', null], ['profiles', 'Inference profiles from AWS'], ['models', 'Models from AWS'], ['other', null],
  ]);
  assert.equal(pickerOptions(awsList, '', '', zh)[0].label, '預設：Claude Sonnet 4.6（global.anthropic.claude-sonnet-4-6）');
  assert.equal(pickerOptions(awsList, '', '', zh).at(-1)!.label, '其他（輸入 ID）');
});

// Breaks if the built-in list is labelled as coming from AWS.
test('the built-in list is one group of its own', () => {
  const runs = groupedOptions(pickerOptions(builtIn, '', '', en));
  assert.deepEqual(runs.map((run) => run.group), ['default', 'built-in', 'other']);
  assert.equal(groupLabel('built-in', en), 'Built-in list');
  assert.equal(groupLabel('built-in', zh), '內置清單');
  assert.deepEqual(runs[1].options.map((option) => option.value), ['global.anthropic.claude-sonnet-4-6', 'us.amazon.nova-micro-v1:0']);
});

// Breaks if a saved id the list does not hold is dropped or swapped for a
// listed one: the select would show another model than the one saved.
test('an id saved earlier that the list does not hold is the current choice', () => {
  const saved = 'eu.anthropic.fixture-saved-v1:0';
  const options = pickerOptions(awsList, saved, '', en);
  assert.deepEqual(options[1], { value: saved, group: 'saved', label: `Saved: ${saved}` });
  assert.equal(selectValue(initialPicker(saved), saved), saved);
  assert.ok(options.some((option) => option.value === selectValue(initialPicker(saved), saved)), 'the select has an option for it');
  assert.equal(pickerOptions(awsList, saved, '', zh)[1].label, `已儲存：${saved}`);
  // A listed saved id is shown as its listed option, once.
  const listed = pickerOptions(awsList, 'amazon.nova-micro-v1:0', '', en);
  assert.equal(listed.filter((option) => option.value === 'amazon.nova-micro-v1:0').length, 1);
  assert.ok(!listed.some((option) => option.group === 'saved'));
  // While the list loads, the saved id is still there.
  assert.deepEqual(pickerOptions(null, saved, 'global.anthropic.claude-sonnet-4-6', en).map((option) => option.group), ['default', 'saved', 'other']);
});

// Breaks if the empty choice stops naming the model it runs.
test('the default is the empty choice and names the model it runs', () => {
  assert.equal(pickerOptions(null, '', 'global.anthropic.claude-sonnet-4-6', en)[0].label, 'Default: global.anthropic.claude-sonnet-4-6');
  assert.equal(pickerOptions(null, '', '', en)[0].label, 'Default model');
  assert.equal(pickerOptions(null, '', '', zh)[0].label, '預設模型');
  assert.equal(selectValue(initialPicker(''), ''), '');
});

// Breaks if "Other" could be mistaken for a model id: the backend refuses
// any id with whitespace, so the value cannot collide with a saved id.
test('Other is last and its value is never a model id', () => {
  for (const list of [awsList, builtIn, null]) {
    const options = pickerOptions(list, 'x.saved', '', en);
    assert.equal(options.at(-1)!.value, OTHER_MODEL);
  }
  assert.match(OTHER_MODEL, /\s/);
});

// Requirement 4 and 9: which list, the region, why the built-in list, and
// the refused permissions in AWS's names. Never that the account may use a
// model. Breaks if a reason loses its words or the permissions their names.
test('the line under the picker says which list it shows and why', () => {
  assert.equal(listStatus({ status: 'ready', list: awsList }, en), 'List from your AWS account for ap-southeast-1. AWS can still refuse a listed model if your account has no access to it.');
  assert.match(listStatus({ status: 'ready', list: awsList }, zh), /^清單來自你的 AWS 帳戶（地區：ap-southeast-1）。/);
  assert.doesNotMatch(listStatus({ status: 'ready', list: awsList }, en), /you can use|available to you|your account can use/i);
  assert.equal(
    listStatus({ status: 'ready', list: builtIn }, en),
    'Built-in list: your AWS key is not allowed to list models. To load the list from your account, allow bedrock:ListInferenceProfiles and bedrock:ListFoundationModels in IAM. AWS may not offer every model in this list in your region.',
  );
  assert.equal(
    listStatus({ status: 'ready', list: builtIn }, zh),
    '內置清單：你的 AWS 密鑰無權列出模型。如要從帳戶載入清單，請在 IAM 允許 bedrock:ListInferenceProfiles 及 bedrock:ListFoundationModels。AWS 未必在你的地區提供此清單內的每個模型。',
  );
  const reasons = ['no_credentials', 'unreadable_credentials', 'credentials_refused', 'offline', 'timed_out', 'failed'] as const;
  const lines = new Set<string>();
  for (const reason of reasons) {
    const list = { ...builtIn, reason, missing_permissions: [] };
    const english = listStatus({ status: 'ready', list }, en);
    const chinese = listStatus({ status: 'ready', list }, zh);
    assert.match(english, /^Built-in list: /, reason);
    assert.match(chinese, /^內置清單：/, reason);
    lines.add(english);
  }
  assert.equal(lines.size, reasons.length, 'each reason has its own words');
  assert.match(listStatus({ status: 'ready', list: { ...builtIn, reason: 'no_credentials', missing_permissions: [] } }, en), /no AWS credentials are saved/);
  assert.match(listStatus({ status: 'ready', list: { ...builtIn, reason: 'offline', missing_permissions: [] } }, en), /could not connect to AWS/);
  assert.equal(listStatus({ status: 'idle' }, zh), '尚未載入模型清單。');
  assert.equal(listStatus({ status: 'loading' }, en), 'Loading the model list…');
  assert.match(listStatus({ status: 'error' }, en), /could not load the model list/);
});

// Point (d): the refusal goes when the user changes the choice or the text,
// not at the next save. Breaks if `choose` or `type` stops clearing `error`.
test('a refusal goes away when the choice or the text changes', () => {
  const refused = pickerReducer({ ...initialPicker('x'), other: true, draft: 'bad id' }, { type: 'saved', error: 'This is not a Bedrock model ID.' });
  assert.equal(refused.error, 'This is not a Bedrock model ID.');
  assert.equal(refused.other, true, 'a refused typed id keeps the field open');
  assert.equal(pickerReducer(refused, { type: 'type', draft: 'bad id2' }).error, null);
  assert.equal(pickerReducer(refused, { type: 'choose', value: 'amazon.nova-micro-v1:0', saved: 'x' }).error, null);
  assert.equal(pickerReducer(refused, { type: 'choose', value: OTHER_MODEL, saved: 'x' }).error, null);
  assert.equal(pickerReducer(refused, { type: 'synced', saved: 'x' }).error, refused.error, 'a reload of the chain does not hide it');
});

// Breaks if Other stops showing the saved id to edit, or a saved typed id
// leaves the ID field open.
test('Other opens the ID field on the saved id; a save closes it', () => {
  const opened = pickerReducer(initialPicker('eu.saved-v1:0'), { type: 'choose', value: OTHER_MODEL, saved: 'eu.saved-v1:0' });
  assert.deepEqual([opened.other, opened.draft, selectValue(opened, 'eu.saved-v1:0')], [true, 'eu.saved-v1:0', OTHER_MODEL]);
  const saving = pickerReducer(opened, { type: 'saving', value: 'eu.typed-v1:0' });
  const saved = pickerReducer(saving, { type: 'saved', error: null });
  assert.deepEqual([saved.other, saved.pending], [false, null]);
  const picking = pickerReducer(initialPicker(''), { type: 'saving', value: 'amazon.nova-micro-v1:0' });
  assert.equal(selectValue(picking, ''), 'amazon.nova-micro-v1:0', 'the choice shows while it saves');
});

// Round 8, R7-4: every built-in list line ends with the same sentence, in
// both languages, and the AWS line does not carry it. Breaks if listStatus
// stops appending BUILT_IN_REGION_NOTE, or a reason returns early.
test('every built-in list line says AWS may not offer each listed model in the region', () => {
  assert.deepEqual([...BUILT_IN_REGION_NOTE], ['AWS may not offer every model in this list in your region.', 'AWS 未必在你的地區提供此清單內的每個模型。']);
  const reasons = ['no_credentials', 'unreadable_credentials', 'credentials_refused', 'not_allowed', 'offline', 'timed_out', 'failed'] as const;
  for (const reason of reasons) {
    const list = { ...builtIn, reason };
    assert.ok(listStatus({ status: 'ready', list }, en).endsWith('. AWS may not offer every model in this list in your region.'), reason);
    assert.ok(listStatus({ status: 'ready', list }, zh).endsWith('。AWS 未必在你的地區提供此清單內的每個模型。'), reason);
  }
  assert.doesNotMatch(listStatus({ status: 'ready', list: awsList }, en), /may not offer/);
  assert.doesNotMatch(listStatus({ status: 'ready', list: awsList }, zh), /未必/);
});

// Requirement 8: the request waits for someone to use the window. Round 8,
// R7-1: a press that arrives as a click alone (VoiceOver, Voice Control,
// Switch Control) opens the gate. Breaks if a type leaves ACTIVITY_EVENTS or
// the gate stops listening in the capture phase.
test('user activity is false until a click, a pointer press or a key press', () => {
  assert.deepEqual([...ACTIVITY_EVENTS].sort(), ['click', 'keydown', 'pointerdown']);
  for (const type of ACTIVITY_EVENTS) {
    const listeners: Record<string, () => void> = {};
    const captured: boolean[] = [];
    const gate = activityGate();
    gate.watch({ addEventListener: (event, listener, capture) => { listeners[event] = listener; captured.push(capture); } });
    assert.deepEqual(Object.keys(listeners).sort(), ['click', 'keydown', 'pointerdown']);
    assert.ok(captured.every(Boolean), 'capture phase');
    assert.equal(gate.hasActed(), false, 'closed before any event');
    let told = 0;
    gate.subscribe(() => { told += 1; });
    listeners[type]();
    assert.equal(gate.hasActed(), true, `${type} alone opens the gate`);
    listeners[type]();
    assert.equal(told, 1, 'subscribers hear it once');
  }
  // The app's own gate, started by main.tsx before the app renders.
  const appListeners: Record<string, () => void> = {};
  watchUserActivity({ addEventListener: (event, listener) => { appListeners[event] = listener; } });
  assert.equal(userHasActed(), false);
  appListeners.click();
  assert.equal(userHasActed(), true, 'a click alone opens the app gate');
  assert.match(source('../src/main.tsx'), /^watchUserActivity\(\);$/m);
});

// R7-1: nothing in the app clicks by itself, so the click listener cannot
// open the gate at launch. Breaks if code under src/ calls .click() or
// dispatches a click event.
test('no code under src/ clicks or dispatches a click by itself', () => {
  const files = readdirSync(new URL('../src/', import.meta.url), { recursive: true })
    .map(String)
    .filter((file) => /\.(ts|tsx)$/.test(file));
  assert.ok(files.length > 50, `found ${files.length} source files`);
  for (const file of files) {
    const text = readFileSync(new URL(`../src/${file}`, import.meta.url), 'utf8');
    assert.doesNotMatch(text, /\.click\(\s*\)/, `${file} calls .click()`);
    assert.doesNotMatch(text, /new (Mouse|Pointer)Event\(|new Event\(\s*['"]click['"]/, `${file} builds a click event`);
  }
});

/** The text between two markers, so a check reads one block, not the file. */
function between(text: string, start: string, end: string): string {
  const from = text.indexOf(start);
  assert.ok(from >= 0, `missing: ${start}`);
  const to = text.indexOf(end, from + start.length);
  assert.ok(to > from, `missing after ${start}: ${end}`);
  return text.slice(from, to);
}

// Requirement 8 in Settings: the list is asked for only from this effect,
// only with a Bedrock row on screen in the active tab's Models section and
// after someone used the window, once per opening. Breaks if a condition is
// dropped or another caller appears.
test('Settings asks for the list only with a Bedrock row on screen, once per opening', () => {
  const settings = source('../src/pages/Settings.tsx');
  assert.equal(settings.split('listBedrockModels()').length, 2, 'one call');
  const effect = between(settings, 'const tabActive = useIsActiveTab();', '}, [bedrockOnScreen, userActed, bedrockAsk]);');
  assert.match(effect, /const bedrockOnScreen = \(embedded \? shows\("ai"\) : tabActive && section === "ai"\) && chain\.some\(\(entry\) => entry\.provider_type === "bedrock"\);/);
  assert.match(effect, /if \(!bedrockOnScreen\) \{\s*bedrockAsked\.current = null;\s*bedrockRequest\.current \+= 1;\s*setBedrockList\(\{ status: "idle" \}\);\s*return;\s*\}/, 'leaving the section forgets the list');
  assert.match(effect, /if \(!userActed \|\| bedrockAsked\.current === bedrockAsk\) return;\s*bedrockAsked\.current = bedrockAsk;/, 'once per opening');
  assert.match(effect, /listBedrockModels\(\)\s*\.then\(\(list\) => \{ if \(bedrockRequest\.current === request\) setBedrockList/);
  for (const file of ['../src/App.tsx', '../src/main.tsx', '../src/lib/commands.ts']) {
    assert.doesNotMatch(source(file).replace('export async function listBedrockModels', ''), /listBedrockModels\(/, `${file} does not ask`);
  }
  assert.match(source('../src/lib/commands.ts'), /return await invoke<BedrockModelList>\("list_bedrock_models"\);/);
  // Saving or removing AWS credentials, and the button, load it again.
  assert.equal(settings.split('setBedrockAsk((n) => n + 1)').length - 1, 3);
});

// The select has a label and is described by the status line; the refusal
// is the picker's own alert. Breaks if the select loses its label or the
// handlers stop dispatching the changes that clear the refusal.
test('the picker is labelled, described by its status line, and clears its refusal on change', () => {
  const field = source('../src/components/BedrockModelPicker.tsx');
  assert.match(field, /<Select label=\{text\("Bedrock model", "Bedrock 模型"\)\} showLabel value=\{selectValue\(state, saved\)\} disabled=\{busy\} describedBy=\{statusId\} onChange=\{choose\}/);
  assert.match(field, /<p id=\{statusId\} role="status" className=\{LOOK\[place\]\.help\}>\{listStatus\(listState, text, place, credentialsIn\)\}<\/p>/);
  assert.match(field, /settings: \{ field: "settings-bedrock-model", help: "settings-help" \}/, 'Settings keeps its classes');
  assert.match(field, /dispatch\(\{ type: "choose", value, saved \}\);\s*if \(value !== OTHER_MODEL && value !== saved\) void save\(value\);/);
  assert.match(field, /onChange=\{\(e\) => type\(e\.target\.value\)\}/);
  assert.match(field, /function type\(draft: string\) \{\s*dispatch\(\{ type: "type", draft \}\);/);
  assert.match(field, /dispatch\(\{ type: "saved", error: await onSave\(value\) \}\);/);
  assert.match(field, /\{state\.error && <p role="alert" className="settings-bedrock-model-error">\{state\.error\}<\/p>\}/);
  assert.match(field, /\{state\.other && \(<>\s*<label>\s*<span>\{text\("Bedrock model ID", "Bedrock 模型 ID"\)\}<\/span>/, 'Other shows the ID field');
});
