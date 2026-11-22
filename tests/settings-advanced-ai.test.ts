import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { ROWS } from '../src/settings/rows.ts';
import type { SettingsSectionId } from '../src/settings/sections.ts';
import type { ProviderChainEntry } from '../src/lib/types.ts';
import { activePreset, addEntry, moveEntry, removeEntry, setEntryModel, toggleEntry, withoutEndpoint } from '../src/settings/advanced-chain.ts';
import { mergeMailSetting } from '../src/settings/mail-models.ts';
import { BUILT_IN_REGION_NOTE, listStatus, type BedrockListState } from '../src/lib/bedrock-models.ts';
import type { MailSettings } from '../src/lib/mail.ts';

// Task 11: Advanced > Models, Cloud providers and Local AI. The chain edits
// are pure and imported; the JSX cannot be imported under node, so its wiring
// is checked in the source.

const root = new URL('..', import.meta.url).pathname;
const source = (path: string) => readFileSync(join(root, path), 'utf8');
const ADV = 'src/settings/sections/advanced/';
const MODELS_FILES = ['ProviderOrder.tsx', 'Models.tsx', 'ChooseModel.tsx', 'MailModels.tsx'].map(f => ADV + f);
const CLOUD_FILES = ['CloudProviders.tsx'].map(f => ADV + f);
const LOCAL_FILES = ['LocalEngines.tsx', 'LocalEndpoints.tsx', 'LocalAi.tsx'].map(f => ADV + f);
const ALL = [...MODELS_FILES, ...CLOUD_FILES, ...LOCAL_FILES, ADV + 'useRow.ts', 'src/settings/hooks/useChainEditor.ts', 'src/settings/hooks/useLocalEndpoints.ts', 'src/settings/advanced-chain.ts'];

const entry = (provider_type: ProviderChainEntry['provider_type'], extra: Partial<ProviderChainEntry> = {}): ProviderChainEntry =>
  ({ position: 0, provider_type, enabled: true, ...extra });
const positions = (chain: ProviderChainEntry[]) => chain.map(e => e.position);

test('moving an entry swaps it with its neighbour and renumbers the order', () => {
  const chain = [entry('gemini'), entry('claude', { position: 1 }), entry('openai', { position: 2 })];
  const up = moveEntry(chain, 1, -1)!;
  assert.deepEqual(up.map(e => e.provider_type), ['claude', 'gemini', 'openai']);
  assert.deepEqual(positions(up), [0, 1, 2]);
  assert.equal(moveEntry(chain, 0, -1), null);
  assert.equal(moveEntry(chain, 2, 1), null);
  assert.equal(chain[0].provider_type, 'gemini', 'the saved chain is not changed in place');
});

test('removing, turning off and adding entries keep positions in order', () => {
  const chain = [entry('gemini'), entry('claude', { position: 1 }), entry('bedrock', { position: 2 })];
  const removed = removeEntry(chain, 1);
  assert.deepEqual(removed.map(e => e.provider_type), ['gemini', 'bedrock']);
  assert.deepEqual(positions(removed), [0, 1]);
  assert.equal(toggleEntry(chain, 2)[2].enabled, false);
  assert.equal(toggleEntry(toggleEntry(chain, 2), 2)[2].enabled, true);
  const added = addEntry(chain, { provider_type: 'local', endpoint_id: 'ep', model_id: 'm' });
  assert.equal(added.length, 4);
  assert.deepEqual(added[3], { position: 3, provider_type: 'local', endpoint_id: 'ep', model_id: 'm', enabled: true });
});

test('a removed endpoint leaves the order with its rows, and a Bedrock model is set on its row', () => {
  const chain = [entry('local', { endpoint_id: 'a', model_id: 'x' }), entry('gemini', { position: 1 }), entry('local', { position: 2, endpoint_id: 'b', model_id: 'y' })];
  const left = withoutEndpoint(chain, 'a');
  assert.deepEqual(left.map(e => e.endpoint_id ?? e.provider_type), ['gemini', 'b']);
  assert.deepEqual(positions(left), [0, 1]);
  const bedrock = [entry('bedrock')];
  assert.equal(setEntryModel(bedrock, 0, 'some-id')[0].model_id, 'some-id');
  assert.equal(setEntryModel(bedrock, 0, '')[0].model_id, undefined, 'empty runs the default model');
});

test('the default provider shows the saved preset, or the registry default when the saved one is gone', () => {
  const catalog = { presets: [{ id: 'one', label: { en: 'One', 'zh-HK': 'One' } }, { id: 'two', label: { en: 'Two', 'zh-HK': 'Two' } }], default_preset: 'one' };
  assert.equal(activePreset('two', catalog), 'two');
  assert.equal(activePreset('retired', catalog), 'one');
  assert.equal(activePreset(null, catalog), 'one');
});

test('the dialog shows the three Advanced sections through their own components, and no second nav is built', () => {
  const dialog = source('src/settings/SettingsDialog.tsx');
  for (const [id, name] of [['advanced-models', 'Models'], ['advanced-cloud', 'CloudProviders'], ['advanced-local', 'LocalAi']]) {
    assert.match(dialog, new RegExp(`import ${name} from '\\./sections/advanced/${name}'`));
    assert.match(dialog, new RegExp(`'${id}': ${name}`));
  }
  // The dialog's own nav already lists the Advanced sub-sections.
  assert.match(dialog, /ADVANCED_SECTIONS\.map\(id => <li key=\{id\}>\{navButton\(id\)\}<\/li>\)/);
  assert.throws(() => statSync(join(root, ADV, 'AdvancedNav.tsx')));
});

function assertRowsInOrder(files: string[], section: SettingsSectionId) {
  const code = files.map(source).join('\n');
  const ids = ROWS.filter(r => r.section === section).map(r => r.id);
  assert.ok(ids.length > 0, section);
  let last = -1;
  // A row is placed by row('<id>'), or by a `row: '<id>'` entry of a list the file renders with row(k.row).
  const listed = code.includes('{...row(k.row)}');
  const place = (id: string) => [code.indexOf(`row('${id}')`), listed ? code.indexOf(`row: '${id}'`) : -1].filter(i => i !== -1).sort((a, b) => a - b)[0] ?? -1;
  for (const id of ids) {
    const at = place(id);
    assert.ok(at !== -1, `row ${id} is missing from ${section}`);
    assert.ok(at > last, `row ${id} is out of order in ${section}`);
    last = at;
  }
}

test('each section renders its rows, in order, by row id', () => {
  assertRowsInOrder(MODELS_FILES, 'advanced-models');
  assertRowsInOrder(CLOUD_FILES, 'advanced-cloud');
  assertRowsInOrder(LOCAL_FILES, 'advanced-local');
  // The parts render in the order their rows are listed.
  const models = source(ADV + 'Models.tsx');
  const order = ['<ProviderOrder', "row('default-provider')", '<ChooseModel', '<MailModels'].map(s => models.indexOf(s));
  assert.ok(order.every((at, i) => at !== -1 && (i === 0 || at > order[i - 1])), `Models parts in order: ${order}`);
  const local = source(ADV + 'LocalAi.tsx');
  const parts = ['<LocalEngines', '<LocalEndpoints', "row('local-mail-setup')"].map(s => local.indexOf(s));
  assert.ok(parts.every((at, i) => at !== -1 && (i === 0 || at > parts[i - 1])), `Local AI parts in order: ${parts}`);
  // Title, description and DOM id come from rows.ts through one helper.
  assert.match(source(ADV + 'useRow.ts'), /\(\{ id, \.\.\.rowText\(ROWS_BY_ID\[id\], locale\) \}\)/);
});

test('the Advanced files have no raw button or select, name no model id and use written Chinese', () => {
  for (const file of ALL) {
    const code = source(file);
    assert.doesNotMatch(code, /<button/, file);
    assert.doesNotMatch(code, /<select/, file);
    assert.doesNotMatch(code, /gemini-\d|gpt-\d|claude-|qwen\d|llama\d/, file);
    assert.doesNotMatch(code, /[嘅咗唔係冇啲喺嚟]/, file);
  }
});

/** True when the call at `index` sits inside the opening tag or body of a <Button kind="danger">. */
function insideDanger(code: string, index: number): boolean {
  const open = code.lastIndexOf('<Button', index);
  if (open === -1) return false;
  const between = code.slice(open, index);
  return between.includes('kind="danger"') && !between.includes('</Button>');
}

test('every remove in the three sections is a danger Button, or a SecretField Remove with its own confirm', () => {
  const removes: [string, RegExp][] = [
    [ADV + 'ProviderOrder.tsx', /removeEntry\(/g],
    [ADV + 'CloudProviders.tsx', /clearAwsCredentials\(/g],
    [ADV + 'LocalEndpoints.tsx', /\.remove\(/g],
  ];
  for (const [file, pattern] of removes) {
    const code = source(file);
    const calls = [...code.matchAll(pattern)];
    assert.ok(calls.length > 0, `${file} removes something`);
    for (const call of calls) assert.ok(insideDanger(code, call.index!), `${call[0]} in ${file} is not inside a danger Button`);
  }
  const cloud = source(ADV + 'CloudProviders.tsx');
  const removals = [...cloud.matchAll(/onRemove=\{/g)];
  assert.ok(removals.length >= 1, 'the keys can be removed');
  assert.equal([...cloud.matchAll(/removeConfirm=\{/g)].length, removals.length, 'each key Remove says what is lost');
});

test('every danger confirm in the three sections says what is lost and whether it can be undone, in en and zh-HK', () => {
  const code = ALL.map(source).join('\n');
  const dangers = [...code.matchAll(/<Button kind="danger"([\s\S]*?)>/g)];
  assert.ok(dangers.length >= 3, `danger buttons found: ${dangers.length}`);
  for (const [, attrs] of dangers) {
    const m = attrs.match(/confirm=\{\{ message: text\(`?'?([^'`]+)'?`?, `?'?([^'`]+)'?`?\)/);
    assert.ok(m, `a confirm with en and zh: ${attrs.slice(0, 80)}`);
    assert.match(m![1], /cannot be undone|again/i, m![1]);
    assert.match(m![2], /無法復原|再次|重新/, m![2]);
  }
  const keys = source(ADV + 'CloudProviders.tsx');
  for (const [, en, zh] of keys.matchAll(/removeConfirm=\{text\(`([^`]+)`, `([^`]+)`\)\}/g)) {
    assert.match(en, /cannot be undone/i);
    assert.match(zh, /無法復原/);
  }
});

test('SecretField takes its own Remove confirm, and keeps the old one when none is given', () => {
  const field = source('src/ui/SecretField.tsx');
  assert.match(field, /removeConfirm\?: string/);
  assert.match(field, /message: removeConfirm \?\? text\('Remove this key from Keychain\?', '要從鑰匙圈移除此密鑰嗎？'\)/);
});

test('every key, TypeSafe included, has one field and a Remove through set_api_key', () => {
  const commands = source('src/lib/commands.ts');
  assert.match(commands, /export async function removeApiKey\(provider: string\)[\s\S]*?invoke\("set_api_key", \{ provider, value: "", remove: true \}\)/);
  const cloud = source(ADV + 'CloudProviders.tsx');
  for (const id of ['gemini', 'claude', 'openai', 'typesafe']) assert.match(cloud, new RegExp(`'${id}'`));
  assert.match(cloud, /removeApiKey\(/);
  // TypeSafe's saved state comes from the mail workspace, which never returns the key.
  assert.match(cloud, /triage\.typesafe_key/);
  assert.doesNotMatch(cloud, /getPreference\('typesafe_api_key'\)/);
});

test('AWS uses the shared regions, and its Remove says when it fails', () => {
  const cloud = source(ADV + 'CloudProviders.tsx');
  assert.match(cloud, /AWS_REGIONS/);
  const at = cloud.indexOf('clearAwsCredentials(');
  const open = cloud.lastIndexOf('<Button', at);
  assert.match(cloud.slice(open, at), /onError=\{/);
  assert.match(cloud, /text\('Could not remove the AWS key\. Try again\.', '無法移除 AWS 密鑰，請再試一次。'\)/);
});

test('the default provider is kept, saved with update_setting so the router follows it', () => {
  const models = source(ADV + 'Models.tsx');
  assert.match(models, /updateSetting\('ai_provider', /);
  assert.match(models, /activePreset\(/);
});

test('choosing a model in Advanced asks before replacing a provider order, as Your AI does', () => {
  const choose = source(ADV + 'ChooseModel.tsx');
  const confirm = choose.indexOf('needsChainConfirm(');
  const call = choose.indexOf('chooseAi(');
  assert.ok(confirm > 0 && call > confirm, 'needsChainConfirm( comes before chooseAi(');
  assert.match(choose, /confirmAction\(text\('This replaces your provider order in Advanced\. Continue\?', '這會取代你在進階設定中的供應商次序。要繼續嗎？'\)/);
  // Every model the registry lists, expensive ones included, by provider.
  assert.match(choose, /catalog\.cloud/);
  assert.doesNotMatch(choose, /mainPathModels/);
});

test('the mail model fields change only the model, never the processing mode', () => {
  const mail = source(ADV + 'MailModels.tsx');
  assert.doesNotMatch(mail, /processing_mode:/);
  assert.doesNotMatch(mail, /\.\.\.settings, \[key\]/, 'never saves over the settings read when the page opened');
});

// Review fix 1: choose_ai on the same page rewrites processing_mode, so a
// model saved from the settings read at load would put the old mode back.
test('a mail model is merged over freshly read mail settings, so a newer processing mode stays', () => {
  const stale = { processing_mode: 'openai', local_model: 'a', openai_model: 'b', bedrock_model: 'c' } as unknown as MailSettings;
  const fresh = { ...stale, processing_mode: 'local' } as MailSettings;
  const next = mergeMailSetting(fresh, 'openai_model', 'd');
  assert.equal(next.processing_mode, 'local', 'the fresh mode is kept, not the one read at load');
  assert.equal(next.openai_model, 'd');
  assert.equal(next.local_model, 'a');
  assert.equal(fresh.openai_model, 'b', 'the fresh copy is not changed in place');
});

test('the mail model save reads the mail settings again right before it saves', () => {
  const mail = source(ADV + 'MailModels.tsx');
  const start = mail.indexOf('async function save(');
  const body = mail.slice(start, mail.indexOf('\n  }\n', start));
  const read = body.indexOf('mailWorkspace()');
  const merge = body.indexOf('mergeMailSetting(');
  const write = body.indexOf('mailSaveSettings(');
  assert.ok(read > 0 && merge > read && write > merge, 'mailWorkspace() then mergeMailSetting( then mailSaveSettings(');
  // A model chosen on the same page reloads the mail rows, so "In use for mail" is current.
  const models = source(ADV + 'Models.tsx');
  assert.match(models, /<ChooseModel [^>]*onChosen=/);
  assert.match(models, /<MailModels [^>]*reloadKey=/);
  assert.match(source(ADV + 'ChooseModel.tsx'), /onChosen\(\)/);
});

// Review fix 2: inside the dialog the AWS keys are in Cloud providers, not below.
test('the Bedrock picker in the provider order says the AWS keys are in Advanced > Cloud providers', () => {
  const ready = (reason: string): BedrockListState => ({ status: 'ready', list: { source: 'fallback', reason, region: null, missing_permissions: [], default_model: { id: 'sample', name: null } } } as unknown as BedrockListState);
  const en = (a: string) => a;
  const zh = (_: string, b: string) => b;
  assert.equal(listStatus(ready('no_credentials'), en, 'settings', 'cloud-providers'), `Built-in list: no AWS credentials are saved. Save them in Advanced → Cloud providers. ${BUILT_IN_REGION_NOTE[0]}`);
  assert.equal(listStatus(ready('no_credentials'), zh, 'settings', 'cloud-providers'), `內置清單：尚未儲存 AWS 憑證。請在進階 → 雲端服務中儲存。${BUILT_IN_REGION_NOTE[1]}`);
  for (const reason of ['no_credentials', 'unreadable_credentials', 'credentials_refused']) {
    for (const t of [en, zh]) {
      const line = listStatus(ready(reason), t, 'settings', 'cloud-providers');
      assert.doesNotMatch(line, /below|下方/, reason);
      assert.match(line, /Advanced → Cloud providers|進階 → 雲端服務/, reason);
    }
  }
  assert.match(listStatus(ready('no_credentials'), en, 'settings'), /under AWS Bedrock below/, 'the old page keeps its own line');
  assert.match(source(ADV + 'ProviderOrder.tsx'), /<BedrockModelPicker [^>]*credentialsIn="cloud-providers"/);
});

// Review fix 3: a remove that worked is not reported as failed when only the status reload fails.
test('a key Remove that worked says so even when the saved-key status cannot be read again', () => {
  const cloud = source(ADV + 'CloudProviders.tsx');
  const fn = (name: string) => { const start = cloud.indexOf(`async function ${name}(`); return cloud.slice(start, cloud.indexOf('\n  }\n', start)); };
  const drop = fn('dropKey');
  assert.ok(drop.indexOf('removeApiKey(') >= 0 && drop.indexOf('refreshAfter(') > drop.indexOf('removeApiKey('), 'the status is read again after the remove');
  assert.match(fn('refreshAfter'), /try \{\s*await readKeys\(\);\s*\} catch/, 'the reload is caught on its own');
  assert.doesNotMatch(fn('refreshAfter'), /setLoadError/, 'a failed reload does not replace the page with an error');
  assert.match(cloud, /text\('Removed\. Could not refresh status\.', '已移除，但未能更新狀態。'\)/);
});

// Review fix 4: 密鑰, as Your AI and LocalMailSetup say.
test('the Advanced AI pages and the key rows say 密鑰', () => {
  for (const file of ALL) assert.doesNotMatch(source(file), /金鑰/, file);
  for (const id of ['key-gemini', 'key-claude', 'key-openai', 'key-typesafe']) {
    const r = ROWS.find(x => x.id === id)!;
    assert.match(JSON.stringify(r), /密鑰/, id);
    assert.doesNotMatch(JSON.stringify(r), /金鑰/, id);
  }
});

// Final review M1 and the T7 colon: one word for a key app-wide, and no
// space after a full-width colon where two strings are joined.
test('no user-facing string in src says 金鑰, and the Your AI key rows search as 密鑰', () => {
  const offenders = filesUnder('src').filter(f => /\.(ts|tsx)$/.test(f) && /金鑰/.test(source(f)));
  assert.deepEqual(offenders, []);
  for (const id of ['ai-key', 'ai-model']) assert.match(JSON.stringify(ROWS.find(x => x.id === id)), /密鑰/, id);
});

test('no joined string puts a space after a full-width colon', () => {
  const joined = /：['"`]\)\}? +[{$]|：['"`]\)\}\{' '\}/;
  const offenders = filesUnder('src').filter(f => /\.(ts|tsx)$/.test(f) && joined.test(source(f)));
  assert.deepEqual(offenders, []);
  assert.match(source('src/settings/sections/Connections.tsx'), /`\$\{text\('Connected as ', '已連接帳戶：'\)\}\$\{who\}`/);
});

function filesUnder(dir: string): string[] {
  return readdirSync(join(root, dir)).flatMap(name => {
    const path = `${dir}/${name}`;
    return statSync(join(root, path)).isDirectory() ? filesUnder(path) : [path];
  });
}

test('the LocalMailSetup panel is rendered in exactly one settings file, and only there with a confirm', () => {
  const rendering = filesUnder('src/settings').filter(f => /<LocalMailSetup(Panel)?\b/.test(source(f)));
  assert.deepEqual(rendering, [ADV + 'LocalAi.tsx']);
  assert.match(source(ADV + 'LocalAi.tsx'), /<LocalMailSetupPanel[^>]*confirmPrepare=\{\{ message: text\('[^']+', '[^']+'\)/);
  const panel = source('src/components/LocalMailSetup.tsx');
  assert.match(panel, /confirmPrepare\?: \{ message: string; label\?: string \}/);
  assert.match(panel, /confirmPrepare \? <Button kind="danger" confirm=\{confirmPrepare\}/);
  // Onboarding and the old page keep the plain button.
  for (const file of ['src/components/ChooseAi.tsx', 'src/components/LocalAiSetup.tsx']) {
    const code = source(file);
    for (const tag of code.match(/<LocalMailSetup[^>]*>/g) ?? []) assert.doesNotMatch(tag, /confirmPrepare/, file);
  }
});

test('Local AI reuses the engine setup logic of LocalAiSetup', () => {
  assert.match(source('src/components/LocalAiSetup.tsx'), /export function useLocalEngines\(/);
  assert.match(source(ADV + 'LocalEngines.tsx'), /useLocalEngines\(/);
});

test('On this Mac in Your AI links to Advanced > Local AI', () => {
  assert.match(source('src/settings/ai/LocalCard.tsx'), /openSettings\('advanced-local'\)/);
});

test('the preview answers the commands the three sections call', () => {
  const preview = source('src/dev/workspacePreview.ts');
  for (const command of ['update_provider_chain', 'set_api_key', 'set_aws_credentials', 'clear_aws_credentials', 'list_bedrock_models', 'mail_save_settings',
    'add_local_endpoint', 'remove_local_endpoint', 'test_endpoint_connection', 'list_endpoint_models', 'check_system_compatibility', 'check_backend_prerequisites',
    'get_recommended_models', 'pull_model', 'cancel_model_pull']) {
    assert.match(preview, new RegExp(`case '${command}'`), command);
  }
});

test('each new file stays at about 250 lines or fewer', () => {
  for (const file of ALL) {
    const lines = source(file).split('\n').length;
    assert.ok(lines <= 250, `${file} has ${lines} lines`);
  }
});

// Review fix (Task 19, inventory row 26): Advanced > Models tests through
// choose_ai too, so both of its Test buttons wait for the same tick.
test('Advanced > Models keeps both Test buttons disabled without the tick', () => {
  const code = readFileSync(new URL('../src/settings/sections/advanced/ChooseModel.tsx', import.meta.url), 'utf8');
  assert.match(code, /<TestConsent option="other" name=\{serviceName\} consent=\{urlConsent\} disabled=\{busy\} onChange=\{setUrlConsent\} \/>/);
  assert.match(code, /<TestConsent option=\{provider\} name=\{name\} consent=\{modelConsent\} disabled=\{busy\} onChange=\{setModelConsent\} \/>/);
  assert.match(code, /disabled=\{!listedModel \|\| !urlConsent\}/);
  assert.match(code, /<Button kind="secondary" busy=\{busy\} disabled=\{!modelConsent\} onClick=\{\(\) => run\('model'/);
  assert.match(code, /onChange=\{v => \{ setProvider\(v\); setModel\(''\); setModelConsent\(false\); \}\}/, 'a new service needs the tick again');
  assert.doesNotMatch(code, /<p className="ui-hint">\{testDisclosure\(/);
});

// Final review M2: a tick given for one address does not cover another.
test('editing the base URL clears the test-message consent', () => {
  assert.match(source(ADV + 'ChooseModel.tsx'), /onChange=\{e => \{ setBaseUrl\(e\.target\.value\); setListed\(null\); setListedModel\(''\); setUrlConsent\(false\); \}\}/);
});
