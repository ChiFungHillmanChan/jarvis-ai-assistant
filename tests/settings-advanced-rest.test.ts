import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { ROWS } from '../src/settings/rows.ts';
import { clearAiError, lastAiError, recordAiError, subscribeAiErrors } from '../src/settings/ai-error-log.ts';

// Task 15: Advanced > Integrations and Developer, and the machine half of the
// inventory walk: every command the old Settings page could call must still be
// callable from the new settings dialog.

const root = new URL('..', import.meta.url).pathname;
const source = (path: string) => readFileSync(join(root, path), 'utf8');
const ADV = 'src/settings/sections/advanced/';
const INTEGRATIONS = ADV + 'Integrations.tsx';
const DEVELOPER = ADV + 'Developer.tsx';

// ---------------------------------------------------------------------------
// Command extraction, the way the inventory did it ("wrapper -> command"):
//
// - A command is the first argument of an `invoke(...)` or
//   `useTauriCommand(...)` call (src/hooks/useTauriCommand.ts, used by
//   CostPanel and Usage): a string literal, or a template literal kept as
//   written (`${service}_disconnect`), which is fine because both sides reach
//   it through the same src/lib wrapper.
// - A component, hook or other non-lib file counts as a whole: its own invoke
//   calls, plus everything it imports. That is how rendered child components
//   count on both sides (CostPanel, AiConsents, OtherGoogleAccounts,
//   BackgroundSettings, LocalMailSetup, BedrockModelPicker, ChooseAi, ...),
//   without a hand-kept list that can go stale.
// - A src/lib file counts per function: importing `saveNotionToken` from
//   lib/commands counts save_notion_token, not the other 120 commands in that
//   file. A lib function that calls another lib function (same file or an
//   imported lib file) counts that function's commands too.
// - Only relative imports are followed; packages are not app commands.
//   Static imports, `import * as x` (its `x.member` uses) and `import('...')`
//   all count.
// ---------------------------------------------------------------------------

const INVOKE = /\b(?:invoke|useTauriCommand)(?:<[^(]*?>)?\(\s*(['"`])((?:(?!\1).)+)\1/g;
const commandsIn = (code: string) => new Set([...code.matchAll(INVOKE)].map(m => m[2]));

function resolveImport(from: string, spec: string): string | null {
  if (!spec.startsWith('.')) return null;
  const base = join(dirname(from), spec);
  for (const candidate of [base, `${base}.ts`, `${base}.tsx`, join(base, 'index.ts'), join(base, 'index.tsx')]) {
    if (/\.(ts|tsx)$/.test(candidate) && existsSync(join(root, candidate)) && statSync(join(root, candidate)).isFile()) return candidate;
  }
  return null;
}

interface Import { file: string; names: string[] | 'all' }

/** Every relative import of `file`, with the names it takes (or 'all' for a default or side-effect import). */
function importsOf(file: string): Import[] {
  const code = source(file);
  const out: Import[] = [];
  for (const m of code.matchAll(/\b(?:import|export)\s+(type\s+)?([\s\S]*?)\s+from\s+['"]([^'"]+)['"]/g)) {
    if (m[1]) continue; // import type: no runtime code
    const target = resolveImport(file, m[3]);
    if (!target) continue;
    const clause = m[2];
    const namespace = clause.match(/\*\s+as\s+(\w+)/);
    const braces = clause.match(/\{([\s\S]*)\}/);
    const names: string[] = [];
    if (namespace) names.push(...[...code.matchAll(new RegExp(`\\b${namespace[1]}\\.(\\w+)`, 'g'))].map(n => n[1]));
    if (braces) {
      for (const part of braces[1].split(',')) {
        const name = part.trim().replace(/^type\s+/, '').split(/\s+as\s+/)[0];
        if (name && !part.trim().startsWith('type ')) names.push(name);
      }
    }
    const hasDefault = !namespace && !clause.trim().startsWith('{') && clause.trim() !== '*';
    out.push({ file: target, names: hasDefault ? 'all' : names });
  }
  for (const m of code.matchAll(/\bimport\(\s*['"]([^'"]+)['"]\s*\)/g)) {
    const target = resolveImport(file, m[1]);
    if (target) out.push({ file: target, names: 'all' });
  }
  for (const m of code.matchAll(/^import\s+['"]([^'"]+)['"]/gm)) {
    const target = resolveImport(file, m[1]);
    if (target) out.push({ file: target, names: 'all' });
  }
  return out;
}

const isLib = (file: string) => file.startsWith('src/lib/');

/** A lib file's top-level declarations, each with its own source text. */
function libChunks(file: string): Map<string, string> {
  const lines = source(file).split('\n');
  const chunks = new Map<string, string>();
  let name: string | null = null;
  let body: string[] = [];
  const flush = () => { if (name) chunks.set(name, (chunks.get(name) ?? '') + body.join('\n')); };
  for (const line of lines) {
    const m = line.match(/^(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function\*?|const|let|class)\s+(\w+)/);
    if (m) { flush(); name = m[1]; body = []; }
    body.push(line);
  }
  flush();
  return chunks;
}

function libCommands(file: string, name: string, seen = new Set<string>()): Set<string> {
  const key = `${file}#${name}`;
  const found = new Set<string>();
  if (seen.has(key)) return found;
  seen.add(key);
  const chunks = libChunks(file);
  const body = chunks.get(name);
  if (body === undefined) return found;
  for (const c of commandsIn(body)) found.add(c);
  const uses = (n: string) => n !== name && new RegExp(`\\b${n}\\b`).test(body);
  for (const other of chunks.keys()) if (uses(other)) for (const c of libCommands(file, other, seen)) found.add(c);
  for (const imp of importsOf(file)) {
    if (!isLib(imp.file)) continue;
    const names = imp.names === 'all' ? [...libChunks(imp.file).keys()] : imp.names;
    for (const n of names) if (uses(n)) for (const c of libCommands(imp.file, n, seen)) found.add(c);
  }
  return found;
}

/** The commands reachable from `entries`, never entering `excluded` files. */
function reachableCommands(entries: string[], excluded: (file: string) => boolean): Set<string> {
  const commands = new Set<string>();
  const visited = new Set<string>();
  const queue = [...entries];
  while (queue.length) {
    const file = queue.shift()!;
    if (visited.has(file) || excluded(file)) continue;
    visited.add(file);
    for (const c of commandsIn(source(file))) commands.add(c);
    for (const imp of importsOf(file)) {
      if (excluded(imp.file)) continue;
      if (isLib(imp.file)) {
        const names = imp.names === 'all' ? [...libChunks(imp.file).keys()] : imp.names;
        for (const n of names) for (const c of libCommands(imp.file, n)) commands.add(c);
      } else queue.push(imp.file);
    }
  }
  return commands;
}

function filesUnder(dir: string): string[] {
  return readdirSync(join(root, dir), { withFileTypes: true }).flatMap(entry => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? filesUnder(path) : /\.(ts|tsx)$/.test(entry.name) ? [path] : [];
  });
}

const OLD_PAGE = 'src/pages/Settings.tsx';
const BRIDGE = 'src/settings/LegacySection.tsx';

/** Old side: the page and what it renders, but not the new dialog code it reaches through SettingsContext. */
const oldCommands = () => reachableCommands([OLD_PAGE], file => file.startsWith('src/settings/'));
/** New side: every file under src/settings/ and what they render, never the old page or the bridge that shows it. */
const newCommands = () => reachableCommands(filesUnder('src/settings').filter(f => f !== BRIDGE), file => file === OLD_PAGE || file === BRIDGE);

/**
 * Commands the old page called that the new dialog may leave out: only the
 * controls the spec removes, each by the spec line that removes it.
 *
 * At command level none of the three removals leaves a command behind today:
 * - "Display name" (spec 6.3 General, "The separate "Display name" row is
 *   removed") wrote update_setting(display_name); update_setting stays in use
 *   for ai_provider and wallpaper_mode_enabled.
 * - The duplicate speech-model download (spec 6.3 Voice, "The two "Download
 *   model" buttons that call the same command become one row") is the same
 *   download_model command Voice still calls.
 * - The duplicate key field (spec 6.3 Advanced, "The duplicated key field in
 *   the Keys section is removed") is the same set_api_key command Cloud
 *   providers still calls.
 * So the map is empty, and the test below checks each of those commands is
 * still called by the control that was kept.
 */
const ALLOWED_MISSING: Record<string, string> = {};

test('the new settings dialog calls every command the old Settings page called', () => {
  const before = oldCommands();
  const after = newCommands();
  assert.ok(before.size > 40, `old page commands found: ${before.size}`);
  const missing = [...before].filter(c => !after.has(c) && !(c in ALLOWED_MISSING)).sort();
  assert.deepEqual(missing, [], `commands the old Settings page called that no new section calls: ${missing.join(', ')}`);
  for (const allowed of Object.keys(ALLOWED_MISSING)) assert.ok(before.has(allowed) && !after.has(allowed), `stale allow-list entry: ${allowed}`);
});

test('the kept control of each removed duplicate still calls its command', () => {
  const after = newCommands();
  for (const command of ['update_setting', 'download_model', 'set_api_key']) assert.ok(after.has(command), command);
});

test('the extraction finds rendered child components and lib wrappers', () => {
  const before = oldCommands();
  // Direct invoke in the page, lib wrappers, and a child component's own commands.
  for (const command of ['cloud_speech', 'save_notion_token', 'save_obsidian_key', 'revoke_ai_consent', 'get_cost_stats', '${service}_disconnect', 'ai_models'])
    assert.ok(before.has(command), command);
  // One wrapper does not pull in its whole file.
  assert.ok(!before.has('get_notion_stats'), 'only imported wrappers count');
  // The bridge is not counted for the new side.
  assert.ok(!reachableCommands([BRIDGE], f => f === OLD_PAGE).has('save_notion_token'));
  assert.equal(relative(root, join(root, OLD_PAGE)), OLD_PAGE);
});

// ---------------------------------------------------------------------------
// The two sections.
// ---------------------------------------------------------------------------

test('the dialog shows Integrations and Developer through their own components, and no section falls back to the bridge', () => {
  const dialog = source('src/settings/SettingsDialog.tsx');
  for (const [id, name] of [['advanced-integrations', 'Integrations'], ['advanced-developer', 'Developer']]) {
    assert.match(dialog, new RegExp(`import ${name} from '\\./sections/advanced/${name}'`));
    assert.match(dialog, new RegExp(`'${id}': ${name},`));
  }
  const sections = source('src/settings/sections.ts').match(/export type SettingsSectionId =([\s\S]*?);/)![1];
  const ids = [...sections.matchAll(/'([a-z-]+)'/g)].map(m => m[1]);
  const views = dialog.match(/const SECTION_VIEWS[\s\S]*?= \{([\s\S]*?)\};/)![1];
  for (const id of ids) assert.match(views, new RegExp(`(^|\\s)'?${id}'?: `), `${id} has its own view`);
});

for (const [section, file, expected] of [
  ['advanced-integrations', INTEGRATIONS, ['github-client-id', 'notion-token', 'github-token', 'obsidian-key', 'obsidian-endpoint']],
  ['advanced-developer', DEVELOPER, ['ai-error-log', 'speech-model-redownload', 'speech-model-path']],
] as const) {
  test(`${section} renders its rows, in order, by row id`, () => {
    const code = source(file);
    const ids = ROWS.filter(r => r.section === section).map(r => r.id);
    assert.deepEqual(ids, expected);
    // A row is placed by row('<id>'), or by a `row: '<id>'` entry of a list: that
    // list's rows sit where the file renders it with row(k.row), in list order.
    const listed = [...code.matchAll(/row: '([a-z-]+)'/g)].map(m => m[1]);
    const site = code.indexOf('row(k.row)');
    const place = (id: string) => code.includes(`row('${id}')`) ? code.indexOf(`row('${id}')`)
      : listed.includes(id) && site !== -1 ? site + listed.indexOf(id) / 100 : -1;
    let last = -1;
    for (const id of ids) {
      const at = place(id);
      assert.ok(at > last, `row ${id} is missing or out of order`);
      last = at;
    }
    assert.match(code, /useRow\(\)/, 'title, description and DOM id come from rows.ts');
  });
}

test('Integrations saves and removes tokens through the shared connection hook, never its own copy', () => {
  const code = source(INTEGRATIONS);
  assert.match(code, /useIntegrationConnection\(/);
  assert.doesNotMatch(code, /saveNotionToken|saveGitHubToken|saveObsidianKey|disconnectService|githubSetClientId/, 'no direct token commands');
  const hook = source('src/hooks/useIntegrationConnection.ts');
  for (const name of ['saveNotionToken', 'saveGitHubToken', 'saveObsidianKey']) assert.match(hook, new RegExp(`\\b${name}\\b`));
  assert.match(code, /\.saveToken\(/);
  assert.match(code, /\.disconnect\(\)/);
  assert.match(code, /\.saveClient\(/);
});

test('every token or key Remove asks first and says what stops working, in en and zh-HK', () => {
  const code = source(INTEGRATIONS);
  const removes = [...code.matchAll(/onRemove=\{/g)];
  assert.equal(removes.length, 1, 'one SecretField renders the three tokens');
  assert.match(code, /removeConfirm=\{/);
  const stops = [...code.matchAll(/stops: \{ en: '([^']+)', zh: '([^']+)' \}/g)];
  assert.equal(stops.length, 3, 'Notion, GitHub and Obsidian each say what stops');
  for (const [, en, zh] of stops) {
    assert.match(en, /JARVIS can no longer/);
    assert.match(zh, /JARVIS 將無法/);
  }
  assert.match(code, /This cannot be undone\./);
  assert.match(code, /此操作無法復原。/);
});

test('a failed status read after a save or remove is a neutral note, not a failure', () => {
  const hook = source('src/hooks/useIntegrationConnection.ts');
  assert.match(hook, /afterAction/);
  assert.match(hook, /text\('Done\. Could not refresh the status\.', '已完成，但未能更新狀態。'\)/);
  const code = source(INTEGRATIONS);
  assert.match(code, /role="status"/);
  assert.match(code, /role="alert"/);
});

test('the Obsidian endpoint saves on commit through set_user_preference, unvalidated as before', () => {
  const code = source(INTEGRATIONS);
  assert.match(code, /setPreference\('obsidian_base_url', /);
  assert.match(code, /getPreference\('obsidian_base_url'\)/);
  assert.match(code, /<TextField /);
  assert.doesNotMatch(code, /new URL\(|https\?:\\\/\\\//, 'no stricter URL check than the old page');
});

test('the last AI error is kept for the session and can be cleared', () => {
  clearAiError();
  let calls = 0;
  const stop = subscribeAiErrors(() => { calls++; });
  assert.equal(lastAiError(), null);
  recordAiError('first', 1);
  recordAiError('second', 2);
  assert.deepEqual(lastAiError(), { message: 'second', at: 2 });
  clearAiError();
  assert.equal(lastAiError(), null);
  stop();
  recordAiError('after', 3);
  assert.equal(calls, 3, 'a listener hears each change until it stops');
  clearAiError();
  // What the old page's llmError collected: endpoint and provider-order failures.
  assert.equal([...source('src/settings/hooks/useLocalEndpoints.ts').matchAll(/fail\(text\(/g)].length, 3);
  assert.match(source('src/settings/hooks/useLocalEndpoints.ts'), /recordAiError\(message\)/);
  assert.match(source('src/settings/hooks/useChainEditor.ts'), /recordAiError\(String\(e\)\)/);
});

test('Developer links to Voice for the download and reads the model path from the registry', () => {
  const code = source(DEVELOPER);
  assert.doesNotMatch(code, /downloadModel/, 'no second download call');
  assert.match(code, /openSettings\('voice'\)/);
  assert.match(code, /text\('Download it in Voice', '在語音中下載'\)/);
  assert.match(code, /useAiModels\(\)/);
  assert.match(code, /defaults\.whisper/);
  assert.match(code, /lastAiError|useAiErrorLog/);
});

test('the two files use shared controls, written Chinese and stay small', () => {
  for (const file of [INTEGRATIONS, DEVELOPER]) {
    const code = source(file);
    assert.doesNotMatch(code, /<button|<select|<input/, file);
    assert.doesNotMatch(code, /[嘅咗唔係冇啲喺嚟]/, file);
    assert.doesNotMatch(code, /金鑰/, `${file} uses 密鑰`);
    assert.ok(code.split('\n').length <= 250, `${file} is ${code.split('\n').length} lines`);
  }
  assert.ok(ROWS.find(r => r.id === 'obsidian-key')!.title[1].includes('密鑰'));
});

test('Connections links to Advanced > Integrations', () => {
  assert.match(source('src/settings/sections/Connections.tsx'), /openSettings\('advanced-integrations'\)/);
  assert.match(source('src/settings/sections/IntegrationRow.tsx'), /openSettings\('advanced-integrations'\)/);
});
