import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { ROWS } from '../src/settings/rows.ts';
import type { SettingsSectionId } from '../src/settings/sections.ts';

// Task 7: Connections, Privacy and data, Setup guide and About. The JSX cannot
// be imported under node, so the wiring is checked in the source.

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const privacy = () => source('../src/settings/sections/Privacy.tsx');
const connections = () => source('../src/settings/sections/Connections.tsx');

/** True when the call at `index` sits inside the opening tag or body of a <Button kind="danger">. */
function insideDanger(code: string, index: number): boolean {
  const open = code.lastIndexOf('<Button', index);
  if (open === -1) return false;
  const between = code.slice(open, index);
  return between.includes('kind="danger"') && !between.includes('</Button>');
}

function assertRowsInOrder(path: string, section: SettingsSectionId) {
  const code = source(path);
  const ids = ROWS.filter(r => r.section === section).map(r => r.id);
  assert.ok(ids.length > 0, section);
  let last = -1;
  // A row is placed by row('<id>') (title, description and DOM id) or, for an
  // anchor around a reused component, by id="<id>".
  const place = (id: string) => [code.indexOf(`row('${id}')`), code.indexOf(`id="${id}"`)].filter(i => i !== -1).sort((a, b) => a - b)[0] ?? -1;
  for (const id of ids) {
    const at = place(id);
    assert.ok(at !== -1, `row ${id} is missing from ${path}`);
    assert.ok(at > last, `row ${id} is out of order in ${path}`);
    last = at;
  }
}

test('the dialog shows the four sections through their own components', () => {
  const dialog = source('../src/settings/SettingsDialog.tsx');
  for (const [id, name] of [['connections', 'Connections'], ['privacy', 'Privacy'], ['setup', 'SetupGuide'], ['about', 'About']]) {
    assert.match(dialog, new RegExp(`import ${name} from '\\./sections/${name}'`));
    assert.match(dialog, new RegExp(`${id}: ${name}`));
  }
});

test('each section renders its rows, in order, by row id', () => {
  assertRowsInOrder('../src/settings/sections/Connections.tsx', 'connections');
  assertRowsInOrder('../src/settings/sections/Privacy.tsx', 'privacy');
  assertRowsInOrder('../src/settings/sections/SetupGuide.tsx', 'setup');
  assertRowsInOrder('../src/settings/sections/About.tsx', 'about');
});

test('Privacy has no raw button, only the shared Button', () => {
  assert.doesNotMatch(privacy(), /<button/);
  assert.match(privacy(), /<Button/);
});

test('every purge, remove, clean or withdraw call in Privacy is inside a danger Button', () => {
  const code = privacy();
  const calls = [...code.matchAll(/\b(purge\w*|sweep\w*|deleteAll\w*|remove\w*|clean\w*|withdraw\w*|revoke\w*)\(/gi)];
  assert.ok(calls.length >= 3, 'the sweep, purge and delete-everything calls are found');
  for (const call of calls) {
    assert.ok(insideDanger(code, call.index!), `${call[0]} at ${call.index} is not inside a <Button kind="danger">`);
  }
});

test('all five purges are offered, and Keep recordings stays in Voice', () => {
  const code = privacy();
  const rows: [string, string][] = [['purge-cache', 'cache'], ['purge-backups', 'backups'], ['purge-models', 'models'], ['purge-mail-bodies', 'mailBodies'], ['purge-conversations', 'conversations']];
  for (const [id, scope] of rows) {
    const at = code.indexOf(`store.purge('${id}', '${scope}')`);
    assert.ok(at !== -1, scope);
    assert.ok(insideDanger(code, at), scope);
  }
  assert.doesNotMatch(code, /keep-recordings|keep_recordings/);
});

test('every danger Button in Privacy and Connections has an en and zh-HK confirm', () => {
  for (const code of [privacy(), connections(), source('../src/settings/sections/IntegrationRow.tsx'), source('../src/components/AiConsents.tsx')]) {
    const dangers = [...code.matchAll(/<Button kind="danger"([\s\S]*?)>/g)];
    assert.ok(dangers.length > 0);
    for (const d of dangers) assert.match(d[1], /confirm=\{(\{ message: text\([`'][^`']+[`'], [`'][^`']+[`']\)|disconnectConfirm\(service, text\))/, d[0].slice(0, 120));
  }
  // The shared Disconnect confirm says what is removed and what stays, for each service, in both languages.
  const hook = source('../src/hooks/useIntegrationConnection.ts');
  for (const service of ['notion', 'github', 'obsidian']) assert.match(hook, new RegExp(`case '${service}': return \\{ label, message: text\\(\\s*'[^']+',\\s*'[^']+'\\) \\}`), service);
});

test('Delete everything keeps its type-DELETE step and asks first', () => {
  const code = privacy();
  assert.match(code, /erase !== 'DELETE'/, 'the button stays disabled until DELETE is typed');
  const call = code.indexOf('store.deleteAll(');
  assert.ok(call !== -1);
  assert.ok(insideDanger(code, call));
});

test('Clean up now and the purges call the storage hook, which uses the old commands', () => {
  const hook = source('../src/settings/hooks/useStorage.ts');
  for (const command of ['getStorageUsage', 'runStorageSweep', 'purgeStorage', 'deleteAllData', 'setRetentionPolicy']) assert.match(hook, new RegExp(`${command}\\(`), command);
});

test('withdrawing a consent is a danger Button', () => {
  const consents = source('../src/components/AiConsents.tsx');
  assert.doesNotMatch(consents, /<button/);
  const call = consents.indexOf('withdraw(consent)');
  assert.ok(call !== -1);
  assert.ok(insideDanger(consents, call));
});

test('Google disconnect and switch account are danger Buttons', () => {
  const code = connections();
  assert.doesNotMatch(code, /<button/);
  for (const call of ['google.disconnect(', 'google.connect(true)']) {
    const at = code.indexOf(call);
    assert.ok(at !== -1, call);
    assert.ok(insideDanger(code, at), call);
  }
  assert.match(code, /<OtherGoogleAccounts /);
});

test('Notion, GitHub and Obsidian disconnect is a danger Button, here and on the old screens', () => {
  const row = source('../src/settings/sections/IntegrationRow.tsx');
  assert.doesNotMatch(row, /<button/);
  const at = row.indexOf('connection.disconnect(');
  assert.ok(at !== -1);
  assert.ok(insideDanger(row, at));
  const full = source('../src/components/IntegrationConnection.tsx');
  const fullAt = full.indexOf('connection.disconnect(');
  assert.ok(fullAt !== -1);
  assert.ok(insideDanger(full, fullAt));
  assert.match(source('../src/hooks/useIntegrationConnection.ts'), /disconnectService\(service\)/);
});

test('Connections shows no tokens, keys or addresses and links to Advanced > Integrations', () => {
  const code = connections() + source('../src/settings/sections/IntegrationRow.tsx');
  for (const secret of ['saveNotionToken', 'saveGitHubToken', 'saveObsidianKey', 'obsidian_base_url', 'githubSetClientId', 'type="password"']) assert.ok(!code.includes(secret), secret);
  assert.match(code, /openSettings\('advanced-integrations'\)/);
});

test('the Setup guide opens the same wizard and each unfinished item opens its section', () => {
  const code = source('../src/settings/sections/SetupGuide.tsx');
  assert.match(code, /openSetupGuide\(\)/);
  assert.match(code, /setupChecklist\(/);
  for (const [id, section] of [['ai', 'ai'], ['google', 'connections'], ['triage', 'mail'], ['briefing', 'briefing']]) {
    assert.match(code, new RegExp(`${id}: '${section}'`), id);
  }
  // The same readings the app already uses for "AI ready", "Google connected", triage and the briefing time.
  for (const read of ['currentAi()', 'googleAccounts()', 'mailWorkspace()', 'getDailyBriefingSettings()']) assert.ok(code.includes(read), read);
});

test('About renders RegistryAbout with the model catalog', () => {
  const code = source('../src/settings/sections/About.tsx');
  assert.match(code, /<RegistryAbout catalog=\{/);
  assert.match(code, /useAiModels\(\)/);
});

test('the new sections write zh-HK in written Chinese, never colloquial', () => {
  const files = ['Connections', 'IntegrationRow', 'Privacy', 'SetupGuide', 'About'].map(name => source(`../src/settings/sections/${name}.tsx`));
  const zh = files.flatMap(code => [...code.matchAll(/text\(\s*[`'][^`']*[`'],\s*[`']([^`']*)[`']\s*\)/g)].map(m => m[1])).join('');
  assert.ok(zh.length > 0);
  assert.doesNotMatch(zh, /[嘅咗唔係冇啲喺嚟]/);
});

test('a failed action shows its error at the row in both languages', () => {
  for (const code of [privacy(), connections()]) {
    assert.match(code, /onError=/);
  }
});

test('the preview answers every command the four sections call', () => {
  const preview = source('../src/dev/workspacePreview.ts');
  for (const command of ['google_accounts', 'current_ai', 'list_ai_consents', 'get_storage_usage', 'notion_connection_status', 'github_connection_status', 'obsidian_connection_status', 'ai_models']) {
    assert.ok(preview.includes(`'${command}'`), command);
  }
});

test('each section file stays small', () => {
  for (const name of ['Connections', 'IntegrationRow', 'Privacy', 'SetupGuide', 'About']) {
    const lines = source(`../src/settings/sections/${name}.tsx`).split('\n').length;
    assert.ok(lines <= 260, `${name}.tsx has ${lines} lines`);
  }
});
