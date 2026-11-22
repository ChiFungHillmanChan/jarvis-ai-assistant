import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { document } from './dom.ts';
import { StrictMode, createElement } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  BUILT_IN_REGION_NOTE, OTHER_MODEL, formPickerOptions, formRegionNote, groupLabel, groupedOptions, listStatus, modelChosen, otherIdHelp, selectValue, initialPicker,
  type BedrockListReason, type BedrockListState, type BedrockModelList,
} from '../src/lib/bedrock-models.ts';

// P02b: the Bedrock model picker in the Mail page's Analysis settings and in
// the setup guide's company Bedrock option. The options, the wording per
// place and the required rule are pure functions; the request rule is the
// hook's, rendered for real with the Tauri bridge replaced; the pages are
// JSX that imports CSS, so their wiring is checked in the source.

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;
const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const DEFAULT = 'global.anthropic.claude-sonnet-4-6';

const awsList: BedrockModelList = {
  source: 'aws',
  region: 'ap-southeast-1',
  reason: null,
  missing_permissions: [],
  default_model: { id: DEFAULT, name: 'Claude Sonnet 4.6' },
  models: [
    { id: 'apac.amazon.nova-micro-v1:0', name: 'APAC Nova Micro', kind: 'profile' },
    { id: 'amazon.nova-micro-v1:0', name: 'Nova Micro', kind: 'model' },
  ],
};

const builtIn: BedrockModelList = {
  source: 'fallback',
  region: 'us-east-1',
  reason: 'no_credentials',
  missing_permissions: [],
  default_model: { id: DEFAULT, name: 'Claude Sonnet 4.6' },
  models: [
    { id: DEFAULT, name: 'Claude Sonnet 4.6', kind: 'profile' },
    { id: 'us.amazon.nova-micro-v1:0', name: 'Amazon Nova Micro', kind: 'profile' },
  ],
};

// Requirement 5: there is no empty "use the default" choice in a mail form.
// A form that opens with no id shows "Choose a model", which cannot be
// picked, and nothing reads as the default. Breaks if formPickerOptions
// offers an empty value that can be chosen, or labels anything "Default".
test('a mail form has no "use the default" entry; with no id it asks for a choice', () => {
  for (const list of [awsList, builtIn, null]) {
    for (const chosen of ['', DEFAULT, 'amazon.nova-micro-v1:0']) {
      const options = formPickerOptions(list, chosen, '', DEFAULT, en);
      assert.ok(options.every((option) => option.value !== '' || option.disabled), `no empty choice that can be picked (${chosen || 'empty'})`);
      assert.ok(options.every((option) => !/^Default/.test(option.label)), 'nothing is labelled Default');
      assert.ok(formPickerOptions(list, chosen, '', DEFAULT, zh).every((option) => !option.label.startsWith('預設')), '沒有「預設」');
    }
  }
  const empty = formPickerOptions(builtIn, '', '', DEFAULT, en);
  assert.deepEqual(empty[0], { value: '', group: 'choose', label: 'Choose a model', disabled: true });
  assert.equal(formPickerOptions(builtIn, '', '', DEFAULT, zh)[0].label, '選擇模型');
  assert.equal(selectValue(initialPicker(''), ''), '', 'the select shows "Choose a model"');
  assert.equal(groupLabel('choose', en), null);
  // Once an id is chosen, "Choose a model" is gone.
  assert.ok(!formPickerOptions(builtIn, DEFAULT, '', DEFAULT, en).some((option) => option.group === 'choose'));
});

// Requirement 5: the app's default model is an ordinary choice, by name,
// and its id is what the form holds. Breaks if the default is left out when
// the list lacks it, offered twice when the list holds it, or offered with
// an empty value.
test('the default model is offered by name with its id', () => {
  // AWS's list does not hold it: it comes first, by name, valued with its id.
  const aws = formPickerOptions(awsList, '', '', DEFAULT, en);
  assert.deepEqual(aws.slice(1, 2), [{ value: DEFAULT, group: 'default', label: `Claude Sonnet 4.6 (${DEFAULT})` }]);
  assert.equal(formPickerOptions(awsList, '', '', DEFAULT, zh)[1].label, `Claude Sonnet 4.6（${DEFAULT}）`);
  // The built-in list holds it: it is offered once, in that list.
  const listed = formPickerOptions(builtIn, DEFAULT, '', DEFAULT, en);
  assert.equal(listed.filter((option) => option.value === DEFAULT).length, 1);
  assert.deepEqual(listed.find((option) => option.value === DEFAULT), { value: DEFAULT, group: 'built-in', label: `Claude Sonnet 4.6 (${DEFAULT})` });
  // Before the list arrives, the app's default id is offered.
  assert.deepEqual(formPickerOptions(null, '', '', DEFAULT, en).map((option) => [option.group, option.value]), [['choose', ''], ['default', DEFAULT], ['other', OTHER_MODEL]]);
  // The same groups and order as Settings after that: AWS's profiles, its models, Other last.
  assert.deepEqual(groupedOptions(aws).map((run) => run.group), ['choose', 'default', 'profiles', 'models', 'other']);
  assert.deepEqual(groupedOptions(listed).map((run) => run.group), ['built-in', 'other']);
});

// Requirement 6: an id saved earlier that the list does not hold is the
// current choice, and stays offered after the user picks something else,
// until the form is saved. Breaks if the saved id is dropped or swapped.
test('a saved id outside the list is kept as the current choice', () => {
  const saved = 'eu.anthropic.fixture-saved-v1:0';
  const options = formPickerOptions(awsList, saved, saved, DEFAULT, en);
  assert.deepEqual(options.find((option) => option.value === saved), { value: saved, group: 'saved', label: `Saved: ${saved}` });
  assert.equal(formPickerOptions(awsList, saved, saved, DEFAULT, zh).find((option) => option.value === saved)!.label, `已儲存：${saved}`);
  assert.equal(selectValue(initialPicker(saved), saved), saved, 'the select shows it');
  assert.ok(!options.some((option) => option.group === 'choose'));
  // The user picks a listed model: the saved id is still there to go back to.
  const picked = formPickerOptions(awsList, 'amazon.nova-micro-v1:0', saved, DEFAULT, en);
  assert.ok(picked.some((option) => option.value === saved && option.group === 'saved'));
  // While "Other" holds the id, no extra option follows the typing.
  assert.equal(formPickerOptions(awsList, null, saved, DEFAULT, en).filter((option) => option.group === 'saved').length, 1);
  // An id picked from a list that was loaded again since, and not saved, stays offered.
  const earlier = formPickerOptions(awsList, 'us.amazon.nova-micro-v1:0', saved, DEFAULT, en);
  assert.deepEqual(earlier.find((option) => option.value === 'us.amazon.nova-micro-v1:0'), { value: 'us.amazon.nova-micro-v1:0', group: 'saved', label: 'Chosen: us.amazon.nova-micro-v1:0' });
  assert.equal(formPickerOptions(awsList, 'us.amazon.nova-micro-v1:0', saved, DEFAULT, zh).find((option) => option.value === 'us.amazon.nova-micro-v1:0')!.label, '已選擇：us.amazon.nova-micro-v1:0');
});

// Requirement 5: the forms' own check stops a save with no model, also when
// "Other" is chosen and its field is empty or only spaces (the picker hands
// the form the trimmed text). Breaks if modelChosen accepts blank text.
test('an empty choice is refused', () => {
  assert.equal(modelChosen(''), false);
  assert.equal(modelChosen('   '), false);
  assert.equal(modelChosen(DEFAULT), true);
});

const ALL_REASONS: BedrockListReason[] = ['no_credentials', 'unreadable_credentials', 'credentials_refused', 'not_allowed', 'offline', 'timed_out', 'failed'];
const CREDENTIAL_REASONS: BedrockListReason[] = ['no_credentials', 'unreadable_credentials', 'credentials_refused'];
const ready = (reason: BedrockListReason, list: BedrockModelList = builtIn): BedrockListState => ({ status: 'ready', list: { ...list, reason, missing_permissions: reason === 'not_allowed' ? ['bedrock:ListFoundationModels'] : [] } });

// Requirement 9: each place has its own true "no credentials" line, in both
// languages. Settings' says to save them below; the Mail dialog has no such
// panel (they are in Settings > Advanced > Cloud providers); the setup guide takes the keys
// above and saves them with its test button, which needs a model, so it
// says to choose one first (review M-3). Breaks if a place falls back to
// another place's words.
test('each place says where to save AWS credentials, in both languages', () => {
  assert.equal(
    listStatus(ready('no_credentials'), en, 'settings'),
    `Built-in list: no AWS credentials are saved. Save them under AWS Bedrock below to load the list from your AWS account. ${BUILT_IN_REGION_NOTE[0]}`,
  );
  assert.equal(listStatus(ready('no_credentials'), en), listStatus(ready('no_credentials'), en, 'settings'), 'Settings is the default place');
  assert.equal(
    listStatus(ready('no_credentials'), en, 'mail'),
    `Built-in list: no AWS credentials are saved. Save them in Settings → Advanced → Cloud providers to load the list from your AWS account. ${BUILT_IN_REGION_NOTE[0]}`,
  );
  assert.equal(
    listStatus(ready('no_credentials'), zh, 'mail'),
    `內置清單：尚未儲存 AWS 憑證。請在「設定 → 進階 → 雲端服務」儲存憑證，即可從你的 AWS 帳戶載入清單。${BUILT_IN_REGION_NOTE[1]}`,
  );
  assert.equal(
    listStatus(ready('no_credentials'), en, 'setup'),
    `Built-in list: no AWS credentials are saved. Choose a model first; the keys entered above are saved when you press “Test and use Bedrock for mail”, and JARVIS then asks your AWS account for the list. ${BUILT_IN_REGION_NOTE[0]}`,
  );
  assert.equal(
    listStatus(ready('no_credentials'), zh, 'setup'),
    `內置清單：尚未儲存 AWS 憑證。請先選擇模型；上方輸入的密鑰會在按「測試並以 Bedrock 處理電郵」時儲存，之後 JARVIS 會向你的 AWS 帳戶查詢清單。${BUILT_IN_REGION_NOTE[1]}`,
  );
  // The other two lines about saved credentials follow the same rule.
  for (const reason of CREDENTIAL_REASONS) {
    const mail = [listStatus(ready(reason), en, 'mail'), listStatus(ready(reason), zh, 'mail')];
    const setup = [listStatus(ready(reason), en, 'setup'), listStatus(ready(reason), zh, 'setup')];
    assert.doesNotMatch(mail[0], /below/, `${reason}: nothing is below in the Mail dialog`);
    assert.doesNotMatch(mail[1], /下方/, reason);
    assert.match(mail[0], /Settings → Advanced → Cloud providers/, reason);
    assert.match(mail[1], /「設定 → 進階 → 雲端服務」/, reason);
    assert.doesNotMatch(setup[0], /below|Settings/, `${reason}: the keys are in the setup guide's own block`);
    assert.doesNotMatch(setup[1], /下方|設定的/, reason);
    assert.match(setup[0], /above.*“Test and use Bedrock for mail”/, reason);
    assert.match(setup[1], /上方.*「測試並以 Bedrock 處理電郵」/, reason);
    for (const line of [...mail, ...setup]) assert.match(line, /^(Built-in list: |內置清單：)/, reason);
  }
});

// Requirement 9: every other line the picker brings is the same in each
// place and true there; every "Built-in list:" line keeps its last sentence
// about regions; the AWS line keeps the sentence that AWS can still refuse a
// listed model, and never says the account can use one. Breaks if a place
// drops the region sentence or the refusal sentence.
test('every other line is the same in each place, with the region and refusal sentences', () => {
  for (const place of ['settings', 'mail', 'setup'] as const) {
    for (const reason of ALL_REASONS) {
      assert.ok(listStatus(ready(reason), en, place).endsWith(`. ${BUILT_IN_REGION_NOTE[0]}`), `${place} ${reason}`);
      assert.ok(listStatus(ready(reason), zh, place).endsWith(`。${BUILT_IN_REGION_NOTE[1]}`), `${place} ${reason}`);
      if (!CREDENTIAL_REASONS.includes(reason)) {
        assert.equal(listStatus(ready(reason), en, place), listStatus(ready(reason), en, 'settings'), `${place} ${reason}`);
        assert.equal(listStatus(ready(reason), zh, place), listStatus(ready(reason), zh, 'settings'), `${place} ${reason}`);
      }
    }
    for (const state of [{ status: 'idle' }, { status: 'loading' }, { status: 'error' }] as BedrockListState[]) {
      assert.equal(listStatus(state, en, place), listStatus(state, en, 'settings'));
    }
    const aws = [listStatus({ status: 'ready', list: awsList }, en, place), listStatus({ status: 'ready', list: awsList }, zh, place)];
    assert.ok(aws[0].endsWith('AWS can still refuse a listed model if your account has no access to it.'), place);
    assert.ok(aws[1].endsWith('如帳戶未獲某個模型的存取權限，即使該模型在清單中，AWS 仍可能拒絕。'), place);
    assert.match(aws[0], /ap-southeast-1/);
    assert.doesNotMatch(aws[0], /you can use|available to you|your account can use/i);
  }
  // The setup guide also takes new keys: its AWS line says the list is from the saved ones.
  assert.match(listStatus({ status: 'ready', list: awsList }, en, 'setup'), /^List from the AWS account of your saved credentials, for ap-southeast-1\./);
  assert.match(listStatus({ status: 'ready', list: awsList }, zh, 'setup'), /^清單來自已儲存憑證所屬的 AWS 帳戶（地區：ap-southeast-1）。/);
  assert.equal(listStatus({ status: 'ready', list: awsList }, en, 'mail'), listStatus({ status: 'ready', list: awsList }, en, 'settings'));
});

// Requirement 9: the help under "Other" is true where it is shown. Every place
// says, in Settings' words, that a us., eu. or apac. profile is sent for the
// saved region (option A); none says the id is sent as typed without that. A
// mail form cannot be left empty, so only Settings says to leave it empty for
// the default; the setup guide adds that its button saves new keys and their
// region first. Breaks if a form says it may be left empty, drops the region
// sentence or the button clause, or Settings' text changes.
test('the help under Other is true in each place', () => {
  assert.equal(
    otherIdHelp('settings', DEFAULT, en),
    `A model ID or an inference profile ID from the Bedrock console. A us., eu. or apac. profile is sent for the region of your saved AWS credentials; other IDs are sent as typed. Leave it empty for ${DEFAULT}. Usage is recorded against the ID that is sent.`,
  );
  assert.equal(otherIdHelp('settings', '', zh), '可輸入 Bedrock 主控台的模型 ID 或推論設定檔 ID。us.、eu. 或 apac. 開頭的設定檔會按已儲存 AWS 憑證的地區發送，其他 ID 按輸入內容發送。留空即使用 預設模型。用量按實際發送的 ID 記錄。');
  for (const place of ['mail', 'setup'] as const) {
    assert.doesNotMatch(otherIdHelp(place, DEFAULT, en), /empty|default/i, place);
    assert.doesNotMatch(otherIdHelp(place, DEFAULT, zh), /留空|預設/, place);
    assert.ok(otherIdHelp(place, DEFAULT, en).includes('A us., eu. or apac. profile is sent for the region of your saved AWS credentials; other IDs are sent as typed.'), place);
    assert.ok(otherIdHelp(place, DEFAULT, zh).includes('us.、eu. 或 apac. 開頭的設定檔會按已儲存 AWS 憑證的地區發送，其他 ID 按輸入內容發送。'), place);
  }
  assert.equal(otherIdHelp('mail', DEFAULT, en), 'A model ID or an inference profile ID from the Bedrock console. A us., eu. or apac. profile is sent for the region of your saved AWS credentials; other IDs are sent as typed. Usage is recorded against the ID that is sent.');
  assert.equal(otherIdHelp('mail', DEFAULT, zh), '可輸入 Bedrock 主控台的模型 ID 或推論設定檔 ID。us.、eu. 或 apac. 開頭的設定檔會按已儲存 AWS 憑證的地區發送，其他 ID 按輸入內容發送。用量按實際發送的 ID 記錄。');
  assert.equal(otherIdHelp('setup', DEFAULT, en), 'A model ID or an inference profile ID from the Bedrock console. A us., eu. or apac. profile is sent for the region of your saved AWS credentials; other IDs are sent as typed. When you enter new keys, “Test and use Bedrock for mail” saves them with their region before it tests. Usage is recorded against the ID that is sent.');
  assert.equal(otherIdHelp('setup', DEFAULT, zh), '可輸入 Bedrock 主控台的模型 ID 或推論設定檔 ID。us.、eu. 或 apac. 開頭的設定檔會按已儲存 AWS 憑證的地區發送，其他 ID 按輸入內容發送。如輸入新的密鑰，「測試並以 Bedrock 處理電郵」會先儲存密鑰及其地區，然後才測試。用量按實際發送的 ID 記錄。');
});

// Founder decision (option A) and requirement 10: mail sends a us., eu. or
// apac. profile for the saved region, as chat does (mail/analysis.rs
// analyze_with_endpoints). Such an id can be chosen in a mail form from either list,
// so the forms say so under the picker, in words true there: in the setup
// guide new keys and their region are saved by the test button before it
// tests. While "Other" is chosen the help under the ID field says it instead
// (test above), so it is shown once. Breaks if the line goes, is shown in
// Settings (which has the sentence under "Other" already), stays shown next
// to the Other help, or loses its words.
test('the mail forms say a us., eu. or apac. profile is sent for the saved region', () => {
  assert.equal(formRegionNote('mail', en), 'A us., eu. or apac. profile is sent for the region of your saved AWS credentials.');
  assert.equal(formRegionNote('mail', zh), 'us.、eu. 或 apac. 開頭的設定檔會按已儲存 AWS 憑證的地區發送。');
  assert.equal(formRegionNote('setup', en), 'A us., eu. or apac. profile is sent for the region of your saved AWS credentials. When you enter new keys, “Test and use Bedrock for mail” saves them with their region before it tests.');
  assert.equal(formRegionNote('setup', zh), 'us.、eu. 或 apac. 開頭的設定檔會按已儲存 AWS 憑證的地區發送。如輸入新的密鑰，「測試並以 Bedrock 處理電郵」會先儲存密鑰及其地區，然後才測試。');
  // The same claim Settings makes under "Other".
  assert.ok(otherIdHelp('settings', DEFAULT, en).includes(formRegionNote('mail', en).slice(0, -1)));
  assert.ok(otherIdHelp('settings', DEFAULT, zh).includes('us.、eu. 或 apac. 開頭的設定檔會按已儲存 AWS 憑證的地區發送'));
  const picker = source('../src/components/BedrockModelPicker.tsx');
  assert.match(picker, /\{props\.place !== "settings" && !state\.other && <p className=\{LOOK\[place\]\.help\}>\{formRegionNote\(props\.place, text\)\}<\/p>\}/);
  assert.match(picker, /\{state\.other && <p className=\{LOOK\[place\]\.help\}>\{otherIdHelp\(place, fallback, text\)\}<\/p>\}/, 'under Other the help says it');
});

// ---------------------------------------------------------------------------
// Requirement 7: when JARVIS asks AWS, rendered for real.

// Each request the stand-in receives: answer it with asked[i], or make the
// command itself fail with refused[i].
const asked: Array<(value: BedrockModelList) => void> = [];
const refused: Array<(error: Error) => void> = [];
const calls: Array<Record<string, unknown> | undefined> = [];
mockIPC((command, args) => {
  assert.equal(command, 'list_bedrock_models', 'the hook sends nothing else');
  calls.push(args as Record<string, unknown> | undefined);
  return new Promise((resolve, reject) => { asked.push(resolve); refused.push(reject); });
});
const { watchUserActivity } = await import('../src/lib/user-activity.ts');
const activity: Record<string, () => void> = {};
watchUserActivity({ addEventListener: (event, listener) => { activity[event] = listener; } });
const { useBedrockModelList } = await import('../src/hooks/useBedrockModelList.ts');

const settle = () => new Promise((resolve) => setTimeout(resolve, 20));

function mount() {
  const seen: BedrockListState[] = [];
  let reload = () => {};
  function Probe({ onScreen }: { onScreen: boolean }) {
    const result = useBedrockModelList(onScreen);
    seen.push(result.list);
    reload = result.reload;
    return null;
  }
  const root = createRoot(document.createElement('div') as unknown as HTMLElement);
  return {
    // The app renders in StrictMode, which runs each effect twice on mount.
    render: (onScreen: boolean) => flushSync(() => root.render(createElement(StrictMode, null, createElement(Probe, { onScreen })))),
    reload: () => flushSync(() => reload()),
    last: () => seen[seen.length - 1],
    unmount: () => flushSync(() => root.unmount()),
  };
}

// Not asked while the picker is closed, nor while it is open before anyone
// has clicked or pressed a key; asked once when someone does; once per
// opening after that. Breaks if the hook stops checking onScreen or the
// gate.
test('the mail forms ask only while the picker is open, after a key or click, once per opening', async () => {
  const probe = mount();
  probe.render(false);
  await settle();
  assert.equal(asked.length, 0, 'closed: nothing asked');
  probe.render(true);
  await settle();
  assert.equal(asked.length, 0, 'open, but nobody has used the window: nothing asked');
  assert.deepEqual(probe.last(), { status: 'idle' });

  flushSync(() => activity.keydown());
  await settle();
  assert.equal(asked.length, 1, 'a key press opens the gate: asked once');
  assert.deepEqual(probe.last(), { status: 'loading' });
  assert.deepEqual(calls[0] ?? {}, {}, 'the request carries no keys, typed or saved');
  asked[0](builtIn);
  await settle();
  assert.deepEqual(probe.last(), { status: 'ready', list: builtIn });

  probe.render(true);
  flushSync(() => activity.click());
  await settle();
  assert.equal(asked.length, 1, 'the same opening does not ask again');

  probe.render(false);
  await settle();
  assert.deepEqual(probe.last(), { status: 'idle' }, 'closing forgets the list');
  assert.equal(asked.length, 1, 'closing asks nothing');

  probe.render(true);
  await settle();
  assert.equal(asked.length, 2, 'opened again: asked once more');
  probe.render(true);
  await settle();
  assert.equal(asked.length, 2);
  probe.unmount();
});

// An opening asks once, StrictMode's second effect run included. The
// button, and the setup guide after it saved AWS credentials, ask again once
// each. A reply that arrives after the picker closed is not shown. Breaks if
// the hook stops remembering that this opening has asked, reload stops
// asking, or the request counter stops guarding a late reply. It stands
// alone (review M-5.3): it opens the gate itself and counts only its own
// requests.
test('loading the list again asks once more; a late reply after closing is dropped', async () => {
  flushSync(() => activity.keydown());
  const before = asked.length;
  const probe = mount();
  probe.render(true);
  await settle();
  assert.equal(asked.length, before + 1, 'a new opening asks once');
  asked[before](builtIn);
  await settle();

  probe.reload();
  await settle();
  assert.equal(asked.length, before + 2, 'reload asks once');
  asked[before + 1](awsList);
  await settle();
  assert.deepEqual(probe.last(), { status: 'ready', list: awsList });

  probe.reload();
  await settle();
  assert.equal(asked.length, before + 3);
  probe.render(false);
  asked[before + 2](builtIn);
  await settle();
  assert.deepEqual(probe.last(), { status: 'idle' }, 'the reply for a closed picker is not shown');

  probe.reload();
  await settle();
  assert.equal(asked.length, before + 3, 'nothing is asked while closed, whatever asks');
  probe.render(true);
  await settle();
  assert.equal(asked.length, before + 4, 'reopened: asked once');
  probe.unmount();
});

// Review M-5.5: the command itself failing gives the error state, whose
// line says to load the list again; the button stays enabled, reads "Load
// the list again", and asks once more. Breaks if the hook stops catching the
// failure, or the button is disabled in the error state.
test('a failed request shows the error state, and loading the list again asks once more', async () => {
  flushSync(() => activity.keydown());
  const before = asked.length;
  const probe = mount();
  probe.render(true);
  await settle();
  assert.equal(asked.length, before + 1, 'asked once');
  refused[before](new Error('stand-in failure'));
  await settle();
  assert.deepEqual(probe.last(), { status: 'error' });
  assert.equal(listStatus({ status: 'error' }, en, 'mail'), 'JARVIS could not load the model list. Load it again to retry.');
  assert.equal(listStatus({ status: 'error' }, zh, 'setup'), 'JARVIS 未能載入模型清單，可重新載入再試。');

  probe.reload();
  await settle();
  assert.equal(asked.length, before + 2, '"Load the list again" asks once more');
  assert.deepEqual(probe.last(), { status: 'loading' });
  asked[before + 1](builtIn);
  await settle();
  assert.deepEqual(probe.last(), { status: 'ready', list: builtIn });
  probe.unmount();

  const picker = source('../src/components/BedrockModelPicker.tsx');
  assert.match(picker, /<button type="button" disabled=\{listState\.status === "loading" \|\| busy\} onClick=\{onReload\}/, 'enabled in the error state');
  assert.match(picker, /\{listState\.status === "idle" \? text\("Load the list", "載入清單"\) : text\("Load the list again", "重新載入清單"\)\}/);
});

// ---------------------------------------------------------------------------
// The pages' wiring, read in the source.

/** The text between two markers, so a check reads one block, not the file. */
function between(text: string, start: string, end: string): string {
  const from = text.indexOf(start);
  assert.ok(from >= 0, `missing: ${start}`);
  const to = text.indexOf(end, from + start.length);
  assert.ok(to > from, `missing after ${start}: ${end}`);
  return text.slice(from, to);
}

// Requirement 1: one component and one hook serve the forms; nothing else
// asks for the list. Breaks if a page calls listBedrockModels itself or
// keeps its own copy of the picker.
test('the forms use the shared picker and hook, and only they ask', () => {
  const email = source('../src/pages/EmailPage.tsx');
  const wizard = source('../src/pages/OnboardingWizard.tsx');
  for (const page of [email, wizard]) {
    assert.doesNotMatch(page, /listBedrockModels/, 'the page does not ask by itself');
    assert.doesNotMatch(page, /Bedrock model ID", "Bedrock 模型 ID"\)\}<input|Bedrock model ID', 'Bedrock 模型 ID'\)\}<input/, 'the typed field is gone');
    assert.doesNotMatch(page, /pickerReducer|formPickerOptions/, 'no copy of the picker');
  }
  const hook = source('../src/hooks/useBedrockModelList.ts');
  assert.equal(hook.split('listBedrockModels()').length, 2, 'one call, with no arguments');
  assert.match(hook, /if \(!onScreen\) \{\s*asked\.current = null;\s*request\.current \+= 1;\s*setList\(\{ status: "idle" \}\);\s*return;\s*\}/);
  assert.match(hook, /if \(!userActed \|\| asked\.current === ask\) return;\s*asked\.current = ask;/);
  assert.match(hook, /const userActed = useUserHasActed\(\);/);
  const picker = source('../src/components/BedrockModelPicker.tsx');
  assert.doesNotMatch(picker, /listBedrockModels|invoke\(/, 'the picker shows a list; it does not ask');
});

// Review M-5.4: who may ask AWS for the list, anywhere under src/. The
// commands module defines the call; only Settings > Models and the forms'
// hook make it; no other file names the command. Breaks if any other file
// imports or calls listBedrockModels, or invokes list_bedrock_models itself.
test('only Settings and the hook call listBedrockModels, anywhere under src/', () => {
  const root = new URL('../src/', import.meta.url);
  const files = readdirSync(root, { recursive: true }).map(String).filter((file) => /\.(ts|tsx|js|jsx)$/.test(file));
  const read = (file: string) => readFileSync(new URL(file, root), 'utf8');
  assert.ok(files.length > 50, 'the walk reaches the source tree');
  assert.deepEqual(files.filter((file) => /\blistBedrockModels\b/.test(read(file))).sort(), ['hooks/useBedrockModelList.ts', 'lib/commands.ts', 'pages/Settings.tsx']);
  // dev/workspacePreview.ts answers the command with sample data in the browser preview; it is never in the app.
  assert.deepEqual(files.filter((file) => /["'`]list_bedrock_models["'`]/.test(read(file))).sort(), ['dev/workspacePreview.ts', 'lib/commands.ts']);
});

// Review M-5.1: a form gets the form options (no empty "Default" entry, a
// choice is required), Settings gets its own. Breaks if the forms are given
// Settings' options or the other way round.
test('a form gets the form options, Settings gets its own', () => {
  const picker = source('../src/components/BedrockModelPicker.tsx');
  assert.match(picker, /const options = props\.place === "settings"\s*\? pickerOptions\(list, props\.saved, fallback, text\)\s*: formPickerOptions\(list, state\.other \? null : props\.value, props\.stored, fallback, text\);/);
});

// Requirements 4, 5 and 7, moved with the Mail dialog (Task 13): the mail
// Bedrock picker is in Settings > Advanced > Models, and Settings > Mail and
// follow-ups keeps the consent box and the Save rule. Breaks if Mail gets its
// own picker or list request, Save no longer needs a model with Bedrock, or
// Save stops writing through mailSaveSettings.
test('the Mail settings: the picker lives in Advanced > Models; Save needs a model with Bedrock', () => {
  const mailSection = source('../src/settings/sections/Mail.tsx');
  const hook = source('../src/settings/hooks/useMailSettings.ts');
  const models = source('../src/settings/sections/advanced/MailModels.tsx');
  assert.match(models, /<BedrockModelPicker place="mail" /);
  assert.doesNotMatch(mailSection + hook, /BedrockModelPicker|useBedrockModelList/, 'no second picker in Mail');
  assert.match(hook, /\(i\.processingMode !== 'bedrock' \|\| modelChosen\(i\.bedrockModel\)\)/);
  // The consent box and the save path are as before.
  assert.match(mailSection, /text\("I understand that analysis sends the group's emails, instructions and active memories to AWS Bedrock\."/);
  assert.match(hook, /await mailSaveSettings\(next\);/);
});

// Requirements 4, 5, 7 and 8 in the setup guide. Breaks if the list is
// asked for with the block closed or off step 1, typed keys are saved before
// the button, the list is not asked again after the button saved keys, or
// the button runs with no model.
test('the setup guide: asked only while the block is open; again after the button saved keys', () => {
  const wizard = source('../src/pages/OnboardingWizard.tsx');
  assert.match(wizard, /const \[bedrockOpen, setBedrockOpen\] = useState\(false\);\s*const bedrockList = useBedrockModelList\(step === 1 && bedrockOpen\);/);
  assert.match(wizard, /<details onToggle=\{event => \{ const open = \(event\.target as HTMLDetailsElement\)\.open; setBedrockOpen\(open\);/);
  assert.match(wizard, /function goStep\(next: number\) \{ setError\(''\); setNotice\(''\); setBedrockOpen\(false\); setStep\(next\); \}/);
  assert.equal(wizard.split('setStep(').length, 2, 'goStep is the only way off step 1');
  const save = between(wizard, 'async function testAndSave() {', 'async function verifyMail() {');
  assert.match(save, /await setAwsCredentials\(accessKey\.trim\(\), secretKey\.trim\(\), region\);\s*setAccessKey\(''\); setSecretKey\(''\);\s*(\/\/[^\n]*\n\s*)?bedrockList\.reload\(\);\s*\}\s*const tested = await testMailAiConnection\(settings\);/, 'asked again once the keys are saved, before the test runs');
  assert.equal(wizard.split('setAwsCredentials(').length, 2, 'keys are saved in one place only: the test button');
  assert.match(wizard, /<BedrockModelPicker place="setup" value=\{settings\.bedrock_model\} stored=\{workspace\.settings\.bedrock_model\} fallback=\{models\?\.defaults\.bedrock \?\? ''\} listState=\{bedrockList\.list\} onReload=\{bedrockList\.reload\} onChange=\{model => changeSettings\(\{ \.\.\.settings, processing_mode: 'bedrock', bedrock_model: model \}\)\} \/>/);
  assert.match(wizard, /disabled=\{!consent \|\| settings\.processing_mode !== 'bedrock' \|\| !modelChosen\(settings\.bedrock_model\)\} onClick=\{\(\) => void testAndSave\(\)\}/);
});

// Requirements 4, 5 and 12 in the shared component: in a form a choice goes
// into the form, nothing is saved by the picker, a model is required, and
// the reload button does not submit the Mail form. Breaks if form mode gets
// a save of its own, loses required, or the button loses type="button".
test('the picker in a form: changes the form only, requires a model, and its button does not submit', () => {
  const picker = source('../src/components/BedrockModelPicker.tsx');
  assert.match(picker, /if \(props\.place !== "settings"\) \{\s*props\.onChange\(value\);\s*return;\s*\}/, 'a choice goes into the form');
  assert.match(picker, /function type\(draft: string\) \{\s*dispatch\(\{ type: "type", draft \}\);\s*if \(props\.place !== "settings"\) props\.onChange\(draft\.trim\(\)\);/, 'typed text goes into the form, trimmed');
  assert.match(picker, /describedBy=\{statusId\} onChange=\{choose\} required=\{form\}/, 'the select is required in a form');
  assert.match(picker, /disabled=\{busy\} required=\{form\}\s*onChange=\{\(e\) => type\(e\.target\.value\)\}/, 'the ID field is required in a form');
  assert.match(picker, /\{!form && <button disabled=\{!changed \|\| busy\}/, '"Save model" only in Settings');
  assert.match(picker, /onKeyDown=\{\(e\) => \{ if \(!form && e\.key === "Enter"/, 'Enter saves only in Settings');
  assert.match(picker, /<button type="button" disabled=\{listState\.status === "loading" \|\| busy\} onClick=\{onReload\}/);
  assert.match(picker, /placeholder=\{form \? undefined : fallback\}/, 'an empty ID field in a form shows no id');
  assert.match(picker, /options=\{options\.map\(\(option\) => \(\{ value: option\.value, label: option\.label, disabled: option\.disabled, group: groupLabel\(option\.group, text\) \?\? undefined \}\)\)\}/, 'each option keeps its label, disabled state and group heading');
  assert.match(picker, /<p id=\{statusId\} role="status" className=\{LOOK\[place\]\.help\}>/, 'the status line is announced as in Settings');
  assert.match(picker, /mail: \{ field: "mail-bedrock-model", help: "mail-hint" \}/);
  assert.match(picker, /setup: \{ field: "setup-bedrock-model", help: "setup-hint" \}/);
});

// Requirement 2: Settings > Models renders the same picker with the same
// props as merged, and keeps its own request effect. Breaks if Settings'
// row passes anything else or loses the immediate save.
test('Settings > Models is unchanged: same props, same save, same request effect', () => {
  const settings = source('../src/pages/Settings.tsx');
  assert.match(settings, /<BedrockModelPicker place="settings" saved=\{entry\.model_id \?\? ""\} fallback=\{models\?\.defaults\.bedrock \?\? ""\} listState=\{bedrockList\} onReload=\{\(\) => setBedrockAsk\(\(n\) => n \+ 1\)\} onSave=\{\(model\) => handleSetBedrockModel\(i, model\)\} styles=\{\{ select: styles\.select, button: styles\.saveBtn, input: styles\.tokenInput \}\} \/>/);
  assert.doesNotMatch(settings, /function BedrockModelField\(/, 'no second copy of the picker');
  assert.doesNotMatch(settings, /useBedrockModelList/, 'Settings keeps its own effect, as merged');
  const picker = source('../src/components/BedrockModelPicker.tsx');
  assert.match(picker, /pickerOptions\(list, props\.saved, fallback, text\)/, 'Settings keeps its options, the default entry included');
  assert.match(picker, /const \{ onSave \} = props;\s*dispatch\(\{ type: "saving", value \}\);/);
  assert.match(picker, /otherIdHelp\(place, fallback, text\)/);
});
