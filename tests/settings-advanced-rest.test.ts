import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
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
// - A shared hook (or any non-lib file) counts as a whole: once one section
//   uses useIntegrationConnection, every command that hook can call counts for
//   the new side, whichever section actually reaches it. So this test shows a
//   command is still callable, not that the right section calls it; the
//   targeted per-section assertions further down are required as well.
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

/** New side: every file under src/settings/ and what they render. */
const newCommands = () => reachableCommands(filesUnder('src/settings'), () => false);

/**
 * The old side, snapshotted from src/pages/Settings.tsx at 001eae0 (the last
 * commit that has it), by running this extraction on that file (following its
 * imports, but not into src/settings/, the new dialog it reached through
 * SettingsContext): every command the old page and the components it rendered
 * could call, sorted. The page is deleted in Task 19, so the superset test
 * compares against this list instead of reading the file. Do not edit it by
 * hand.
 */
const OLD_SETTINGS_COMMANDS: readonly string[] = [
  '${service}_cancel_connect', '${service}_connection_status', '${service}_disconnect', 'account_sign_in',
  'account_sign_out', 'account_status', 'add_local_endpoint', 'ai_consent_needed', 'ai_google_content_note',
  'ai_models', 'apply_tested_mail_settings', 'cancel_local_setup', 'cancel_model_pull',
  'check_backend_prerequisites', 'check_system_compatibility', 'choose_ai', 'clear_aws_credentials',
  'cloud_speech', 'current_ai', 'delete_all_data', 'disable_wake_word', 'disable_wallpaper',
  'download_model', 'enable_wake_word', 'enable_wallpaper', 'get_api_key_status',
  'get_aws_credentials_status', 'get_blended_cost', 'get_cost_stats', 'get_daily_briefing_settings',
  'get_keep_awake', 'get_open_at_login', 'get_provider_chain', 'get_recommended_models', 'get_settings',
  'get_storage_usage', 'get_usage_groups', 'get_user_preference', 'get_voice_settings', 'get_voice_state',
  'get_wake_word_status', 'get_wallpaper_status', 'github_begin_connect', 'github_complete_connect',
  'github_set_client_id', 'give_ai_consent', 'google_accounts', 'google_cancel_connect', 'google_connect',
  'google_connect_gmail', 'google_disconnect', 'is_model_downloaded', 'list_ai_consents', 'list_ai_models',
  'list_bedrock_models', 'list_endpoint_models', 'list_local_endpoints', 'list_tts_voices',
  'mail_cancel_connect', 'mail_connect_account', 'mail_disconnect_account', 'mail_sync_account',
  'mail_workspace', 'notion_connect', 'obsidian_select_vault', 'open_obsidian', 'open_url',
  'payments_open_portal', 'payments_reopen_url', 'prepare_local_ai', 'probe_local_setup', 'pull_model',
  'purge_storage', 'remove_local_endpoint', 'revoke_ai_consent', 'run_storage_sweep', 'save_github_token',
  'save_notion_token', 'save_obsidian_key', 'set_api_key', 'set_aws_credentials',
  'set_daily_briefing_settings', 'set_keep_awake', 'set_open_at_login', 'set_retention_policy',
  'set_user_preference', 'set_voice_setting', 'start_listening', 'stop_listening', 'sync_calendar',
  'test_endpoint_connection', 'test_mail_ai_connection', 'update_provider_chain', 'update_setting',
];

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

test('the snapshot is sorted, has no repeats, and the old page is gone', () => {
  assert.equal(OLD_SETTINGS_COMMANDS.length, 94);
  assert.deepEqual([...OLD_SETTINGS_COMMANDS], [...new Set(OLD_SETTINGS_COMMANDS)].sort());
  assert.ok(!existsSync(join(root, 'src/pages/Settings.tsx')));
});

test('the new settings dialog calls every command the old Settings page called', () => {
  const before = new Set(OLD_SETTINGS_COMMANDS);
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
  const after = newCommands();
  // Direct invoke in a section, lib wrappers, and a child component's own
  // commands; each was on the old side too.
  for (const command of ['cloud_speech', 'save_notion_token', 'save_obsidian_key', 'revoke_ai_consent', 'get_cost_stats', '${service}_disconnect', 'ai_models']) {
    assert.ok(OLD_SETTINGS_COMMANDS.includes(command), command);
    assert.ok(after.has(command), command);
  }
  // One wrapper does not pull in its whole file.
  assert.ok(!after.has('get_notion_stats'), 'only imported wrappers count');
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
  // Notion's Remove keeps an earlier official sign-in, and says so.
  const notion = stops[0];
  assert.match(notion[1], /official Notion sign-in is not deleted/);
  assert.match(notion[2], /Notion 官方授權不會被刪除/);
});

test('each Remove makes the call that deletes only that token or key', () => {
  const code = source(INTEGRATIONS);
  // Notion and Obsidian: a blank save deletes only the token or key
  // (save_rest_token / save_obsidian_key with ''). GitHub rejects a blank token,
  // so it disconnects; its one stored item is the token itself.
  assert.match(code, /\{ service: 'notion', remove: 'blank',/);
  assert.match(code, /\{ service: 'github', remove: 'disconnect',/);
  assert.match(code, /\{ service: 'obsidian', remove: 'blank',/);
  const remove = code.slice(code.indexOf('async function removeToken('), code.indexOf('const tokenSaved'));
  assert.match(remove, /if \(k\.remove === 'blank'\) \{\s*try \{ result = await connection\.saveToken\(''\); \}/);
  assert.match(remove, /\} else result = await connection\.disconnect\(\);/);
  assert.match(code, /onRemove=\{\(\) => removeToken\(k\)\}/);
  // The blank value reaches the command: saveToken only trims it.
  const hook = source('src/hooks/useIntegrationConnection.ts');
  const save = hook.slice(hook.indexOf('async function saveToken('), hook.indexOf('async function openService('));
  assert.match(save, /await TOKEN_SAVE\[service\]\(value\.trim\(\)\);/);
  assert.doesNotMatch(save, /if \(!value|value\.trim\(\) === ''|!value\.trim\(\)/, 'a blank value is not stopped before the command');
});

test('a Remove or save that cannot run, or fails, says so at its row', () => {
  const code = source(INTEGRATIONS);
  assert.match(code, /text\('Another change is still running\. Try again in a moment\.', '另一項更改仍在進行，請稍後再試。'\)/);
  assert.match(code, /text\('Could not remove: ', '無法移除：'\)/);
  assert.match(code, /result === 'busy' \? 'busy' : 'failed'/);
  assert.match(code, /e === BUSY \? busyText/);
  const hook = source('src/hooks/useIntegrationConnection.ts');
  assert.match(hook, /if \(!operation\) return 'busy';/);
  assert.match(hook, /if \(currentOperation\(service\)\) return 'busy';/);
  assert.match(hook, /return done \? 'done' : 'failed';/);
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
