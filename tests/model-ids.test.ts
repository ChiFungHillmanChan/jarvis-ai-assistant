// P05: the app takes every model id from the model registry at runtime, so
// no model id is written in the app's code. This scans what goes into the
// app (src/, src-tauri/ and the HTML pages Vite builds from) for model ids:
// every id in the registry, anything shaped like one (SHAPE, any case), and
// a family prefix an id is built from (FRAGMENT: format!("gpt-{}", ..)).
// Comments are skipped, and so is code that never reaches a user's build:
// Rust items and modules compiled only for tests or the native QA build
// (#[cfg(test)], #[cfg(feature = "native-qa")] and
// #[cfg(any(test, feature = "native-qa"))]).
//
// Every file the shipped Rust code compiles in (include_str!, include_bytes!)
// is scanned too, whatever its type: the system prompt's .md, the JSON data.
//
// The places not scanned are NOT_SCANNED below, each with its reason. Every
// remaining hit is listed in ALLOWED, or its whole file in ALLOWED_FILES,
// with the reason.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';

const root = new URL('..', import.meta.url).pathname;
const read = (path: string) => readFileSync(join(root, path), 'utf8');

/** Where an id may stay, and why. The key is `file|id`. */
const ALLOWED: Record<string, string> = {
  // The browser preview (npm run dev without Tauri) answers commands with
  // made-up data; it is never part of the app.
  'src/dev/workspacePreview.ts|qwen3:8b': 'browser preview sample data, not shipped in the app',
  'src/dev/workspacePreview.ts|jev': 'browser preview sample data (a decided_by value), not shipped in the app',
  // Sorting rows record who decided ("rule", "jev" or the user): a fixed
  // value of the mail_triage.decided_by column, not a model choice.
  'src-tauri/src/mail/triage.rs|jev': 'the decided_by value of a sorting row, not a model id',
  'src/lib/mail.ts|jev': 'the decided_by value of a sorting row, not a model id',
};

/**
 * Directories the scan does not read, and why. Paths are from the repo root.
 * src-tauri/migrations/ is compiled into the app (refinery's
 * embed_migrations!), so it is an exception and not "outside the app": the
 * migrations that write a model id are named in MIGRATIONS_WITH_IDS.
 */
export const NOT_SCANNED: Record<string, string> = {
  'registry/': 'the registry itself, the one place model ids are kept',
  'src-tauri/migrations/':
    'history: a migration must keep the SQL it ran with (refinery checks each applied migration against its file), so a model id one wrote stays; the ones that write one are listed in MIGRATIONS_WITH_IDS, and no migration may be changed or added for P05',
  'src-tauri/target/': 'build output, not source',
  'src-tauri/gen/': 'files Tauri generates at build time, not source',
  'src-tauri/icons/': 'images',
  'tests/': 'test fixtures',
  'infra/': 'the server side (the registry Lambda and, from P08, the proxy), which is not the app',
};

/**
 * The migrations whose SQL (comments aside) writes a model id, and what a
 * fresh install gets from each. A new migration that writes one fails the
 * scan test until it is listed here with the reason.
 */
export const MIGRATIONS_WITH_IDS: Record<string, string> = {
  'src-tauri/migrations/V13__aws_credentials_and_routing.sql':
    'seeds workload_routing (volume, tools, premium) with three Bedrock ids; no request path reads that table (ai/mod.rs no_request_path_reads_workload_routing)',
  'src-tauri/migrations/V20__default_chain_fallback.sql':
    "moves the old default chain's Gemini row from gemini-3-flash-preview to gemini-3.1-pro-preview; a fresh install has no chain rows, so it writes nothing",
  'src-tauri/migrations/V21__beta_cleanup.sql': 'moves Gemini chain rows pinned to gemini-3-flash-preview to gemini-3.7-flash; nothing on a fresh install',
  'src-tauri/migrations/V23__mail_openai.sql': "mail_settings.openai_model DEFAULT 'gpt-5.6-luna'; the one mail_settings row (V22) gets it",
  'src-tauri/migrations/V27__mail_triage.sql': "moves mail_settings.openai_model from 'gpt-5.6-luna' (or '') to 'gpt-6-luna'; mail_triage.decided_by DEFAULT 'jev' is a sorting value, not a model",
  'src-tauri/migrations/V30__choose_one_ai.sql': "rebuilds mail_settings with openai_model DEFAULT 'gpt-6-luna' and copies the row, so a fresh install holds 'gpt-6-luna'",
};

/** Files that may hold any model id, and why. */
const ALLOWED_FILES: Record<string, string> = {
  'src-tauri/src/ai/registry/fixture.json': 'the frozen copy of registry version 1 the tests run on; compiled only into the tests',
  'src-tauri/src/ai/old_rows_prices.frozen.json':
    'the frozen price list of the one-time correction of old usage rows (ai/cost.rs correct_old_rows), kept as registry version 1 priced them so every release corrects them the same way (the lead\'s ruling of 2026-11-17 on DECISIONS.md, "P04 and P05:"); never edited',
};

/**
 * Anything shaped like a model id, whether the registry lists it or not, in
 * any case. A Bedrock id may carry any geography prefix (us., us-gov., au.,
 * ca., global.); its vendor part must look like a model (a "-" or ":" after
 * the first word), so "aws.amazon.com" is not one.
 */
export const SHAPE = /(?<![\w.:-])(?:[a-z][a-z-]*\.)?(?:gpt-\d[\w.:-]*|chatgpt-[\w.:-]+|o\d(?:-[\w.:-]+)?|text-embedding-[\w.:-]+|tts-\d[\w.:-]*|claude-(?:opus|sonnet|haiku|instant|\d)[\w.:-]*|gemini-(?:\d|flash|pro|exp)[\w.:-]*|(?:anthropic|amazon|meta|mistral|cohere|ai21|deepseek)\.[a-z][\w]*[-:][\w.:-]*|qwen\d[\w.:-]*|qwen-[\w.:-]+|qwq[\w.:-]*|llama-?\d[\w.:-]*|gemma\d[\w.:-]*|mistral[:-][\w.-]+|mixtral[\w.:-]*|phi\d[\w.:-]*|deepseek-[\w.:-]+|kimi-[\w.:-]+|moonshot-v\d[\w.:-]*|(?:amazon-)?nova-[\w.:-]+|grok-\d[\w.:-]*|command-r(?:\d\w*|-[\w.:-]+)?|jev-[\w.:-]+|ggml-[\w-]+|whisper-[\w.:-]+|speech-\d[\w.:-]*)(?![\w:-])/gi;

/**
 * A family prefix an id is built from at runtime: "gpt-" just before a
 * placeholder or the end of a string (format!("gpt-{}-luna", 6),
 * concat!("gemini-", "3.7-flash"), `qwen-${size}`).
 */
export const FRAGMENT = /(?<![\w.:-])(?:gpt|chatgpt|claude|gemini|qwen|llama|gemma|kimi|moonshot|nova|grok|deepseek|mistral|mixtral|jev|ggml|whisper|speech|tts|text-embedding)-(?=["'`{]|\$\{)/gi;

const registryIds = () => {
  const ids = new Set<string>();
  for (const path of ['registry/registry.json', 'src-tauri/src/ai/registry/fixture.json']) {
    for (const model of JSON.parse(read(path)).models as { id: string }[]) ids.add(model.id);
  }
  return ids;
};

/**
 * Every registry id as written. An id with punctuation ("gpt-6-luna",
 * "qwen3.5:4b") counts wherever it stands alone; a plain word ("jev") only
 * as a whole string, so a function named jev() does not count.
 */
export function idPattern(ids: Iterable<string>): RegExp {
  const escape = (id: string) => id.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const sorted = [...ids].sort((a, b) => b.length - a.length);
  const marked = sorted.filter(id => !/^\w+$/.test(id)).map(escape);
  const words = sorted.filter(id => /^\w+$/.test(id)).map(escape);
  const parts = [];
  if (marked.length) parts.push(`(?<![\\w.:-])(?:${marked.join('|')})(?![\\w:-])`);
  if (words.length) parts.push(`(?<=["'\`])(?:${words.join('|')})(?=["'\`])`);
  return parts.length ? new RegExp(parts.join('|'), 'g') : /(?!)/g;
}

function files(dir: string, keep: (path: string) => boolean): string[] {
  return readdirSync(join(root, dir)).flatMap(name => {
    const path = `${dir}/${name}`;
    if (notScanned(path)) return [];
    return statSync(join(root, path)).isDirectory() ? files(path, keep) : keep(path) ? [path] : [];
  });
}

/**
 * Two copies of the source, the same length and lines: `code` with comments
 * blanked (strings kept, so ids in strings count), and `bare` with string
 * contents blanked too (to match braces). Rust: line and
 * nested block comments, "..." and r#"..."# strings, char literals ('"')
 * and lifetimes; TypeScript: line and block comments, '...', "..." and `...`.
 */
export function lex(source: string, rust: boolean): { code: string; bare: string } {
  let code = '';
  let bare = '';
  const blank = (text: string) => text.replace(/[^\n]/g, ' ');
  const keep = (text: string) => { code += text; bare += text; };
  const comment = (text: string) => { code += blank(text); bare += blank(text); };
  const string = (open: string, body: string, close: string) => { keep(open); code += body; bare += blank(body); keep(close); };
  let i = 0;
  while (i < source.length) {
    const c = source[i];
    const next = source[i + 1];
    if (c === '/' && next === '/') {
      const end = source.indexOf('\n', i);
      const stop = end === -1 ? source.length : end;
      comment(source.slice(i, stop)); i = stop; continue;
    }
    if (c === '/' && next === '*') {
      let j = i + 2;
      if (rust) {
        let depth = 1; // Rust block comments nest.
        while (j < source.length && depth > 0) {
          if (source.startsWith('/*', j)) { depth++; j += 2; } else if (source.startsWith('*/', j)) { depth--; j += 2; } else j++;
        }
      } else {
        const end = source.indexOf('*/', j);
        j = end === -1 ? source.length : end + 2;
      }
      comment(source.slice(i, j)); i = j; continue;
    }
    if (rust && c === 'r' && /^r#*"/.test(source.slice(i, i + 8)) && !/\w/.test(source[i - 1] ?? '')) {
      const hashes = source.slice(i + 1).match(/^#*/)![0];
      const open = `r${hashes}"`;
      const close = `"${hashes}`;
      const end = source.indexOf(close, i + open.length);
      const stop = end === -1 ? source.length : end;
      string(open, source.slice(i + open.length, stop), source.slice(stop, stop + close.length)); i = stop + close.length; continue;
    }
    if (rust && c === "'") {
      const char = source.slice(i).match(/^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'/);
      if (char) { string("'", char[0].slice(1, -1), "'"); i += char[0].length; } else { keep(c); i++; } // else a lifetime
      continue;
    }
    if (c === '"' || (!rust && (c === "'" || c === '`'))) {
      let j = i + 1;
      while (j < source.length && source[j] !== c) j += source[j] === '\\' ? 2 : 1;
      string(c, source.slice(i + 1, Math.min(j, source.length)), j < source.length ? c : ''); i = j + 1; continue;
    }
    keep(c); i++;
  }
  return { code, bare };
}

/** Attributes for code that never reaches a user's build. */
const TEST_ONLY = /^#\[cfg\((?:test|feature = "native-qa"|any\(test, feature = "native-qa"\))\)\]$/;

/** Rust `code` without its test-only items (a module, a const, a fn, a use). */
export function withoutTestItems(code: string, bare: string): string {
  const lines = code.split('\n');
  const plain = bare.split('\n');
  for (let i = 0; i < plain.length; i++) {
    if (!TEST_ONLY.test(lines[i].trim())) continue;
    // Blank the item that follows: up to its closing brace, or its semicolon.
    let depth = 0; let opened = false; let j = i + 1;
    for (; j < plain.length; j++) {
      for (const ch of plain[j]) { if (ch === '{') { depth++; opened = true; } else if (ch === '}') depth--; }
      if ((opened && depth <= 0) || (!opened && plain[j].trimEnd().endsWith(';'))) break;
    }
    for (let k = i; k <= j && k < lines.length; k++) lines[k] = '';
    i = j;
  }
  return lines.join('\n');
}

/** What the scan reads of one file. A "-" written as an escape counts as one. */
export function scanned(source: string, rust: boolean): string {
  const { code, bare } = lex(source, rust);
  return (rust ? withoutTestItems(code, bare) : code).replace(/\\x2d|\\u\{0*2d\}|\\u002d/gi, '-');
}

/** The files a shipped Rust file compiles in, from the repo root. */
export function compiledIn(file: string, code: string): string[] {
  return [...code.matchAll(/include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)/g)].map(m => posix.normalize(posix.join(dirname(file), m[1])));
}

const notScanned = (path: string) => Object.keys(NOT_SCANNED).some(dir => `${path}/`.startsWith(dir));

/** Files that are test-only modules: `#[cfg(test)] mod name;` next to them. */
function testModuleFiles(rustFiles: string[]): Set<string> {
  const found = new Set<string>();
  for (const file of rustFiles) {
    const dir = /\/(?:mod|lib|main)\.rs$/.test(file) ? file.replace(/\/[^/]+$/, '') : file.replace(/\.rs$/, '');
    const plain = lex(read(file), true).code.split('\n').map(line => line.trim());
    plain.forEach((line, n) => {
      if (!TEST_ONLY.test(line)) return;
      const module = plain[n + 1]?.match(/^(?:pub(?:\([\w:]+\))?\s+)?mod\s+(\w+)\s*;$/);
      if (module) { found.add(`${dir}/${module[1]}.rs`); found.add(`${dir}/${module[1]}/mod.rs`); }
    });
  }
  return found;
}

/** What the scan reads of `file`: code without comments and test-only items, other files as they stand. */
function readScanned(file: string): string {
  if (file.endsWith('.rs')) return scanned(read(file), true);
  if (/\.(?:ts|tsx|js|mjs)$/.test(file)) return scanned(read(file), false);
  return read(file);
}

function hitsIn(code: string, listed: RegExp): string[] {
  return [listed, SHAPE, FRAGMENT].flatMap(pattern => [...code.matchAll(pattern)].map(match => match[0]));
}

/** The files scanned: the app's sources, and every file shipped Rust compiles in. */
export function scannedFiles(): string[] {
  const rust = files('src-tauri', path => path.endsWith('.rs'));
  const testOnly = testModuleFiles(rust.filter(path => path.startsWith('src-tauri/src/')));
  const shipped = rust.filter(path => !testOnly.has(path));
  const sources = new Set([
    ...files('src', path => /\.(?:ts|tsx|js|mjs|json|html)$/.test(path)),
    ...shipped,
    ...files('src-tauri', path => path.endsWith('.json')),
    ...readdirSync(root).filter(name => name.endsWith('.html')),
  ]);
  for (const file of shipped) {
    for (const target of compiledIn(file, scanned(read(file), true))) if (!notScanned(target)) sources.add(target);
  }
  return [...sources].sort();
}

function scan(): Map<string, Set<string>> {
  const listed = idPattern(registryIds());
  const hits = new Map<string, Set<string>>();
  for (const file of scannedFiles()) {
    for (const id of hitsIn(readScanned(file), listed)) {
      if (!hits.has(file)) hits.set(file, new Set());
      hits.get(file)!.add(id);
    }
  }
  return hits;
}

test('no model id is written in the app code outside the registry and test fixtures', () => {
  const unexpected: string[] = [];
  const used = new Set<string>();
  const usedFiles = new Set<string>();
  for (const [file, ids] of scan()) {
    if (ALLOWED_FILES[file]) {
      usedFiles.add(file);
      continue;
    }
    for (const id of ids) {
      const key = `${file}|${id}`;
      if (ALLOWED[key]) used.add(key); else unexpected.push(key);
    }
  }
  assert.deepEqual(unexpected.sort(), [], 'read these ids from the registry (ai_models or ai::models), or list them in ALLOWED with the reason');
  // An exception that no longer matches anything is dropped from the list.
  assert.deepEqual(Object.keys(ALLOWED).filter(key => !used.has(key)), [], 'remove the exceptions that no longer match');
  assert.deepEqual(Object.keys(ALLOWED_FILES).filter(file => !usedFiles.has(file)), [], 'remove the file exceptions that hold no id or no longer exist');
});

test('the scan finds an id in code and skips comments and test-only code', () => {
  const ids = idPattern(['gpt-6-luna', 'qwen3.5:4b', 'jev', 'gemini-3.7-flash']);
  const find = (source: string, rust: boolean) => [...scanned(source, rust).matchAll(ids)].map(m => m[0]).concat([...scanned(source, rust).matchAll(SHAPE)].map(m => `shape:${m[0]}`));
  const rust = [
    'const A: &str = "gpt-6-luna";',
    '// "gemini-3.7-flash" in a comment',
    "fn f() { let q = '\"'; let s = \"qwen3.5:4b\"; let b = \"{\"; }",
    'pub fn jev() -> String { "jev".into() }',
    '#[cfg(test)]',
    'mod tests {',
    '    const B: &str = "gemini-3.7-flash"; const OPEN: &str = "}";',
    '}',
    '#[cfg(any(test, feature = "native-qa"))]',
    'const C: &str = "gemini-3.7-flash";',
    'const D: &str = r#"say "gpt-6-luna""#;',
  ].join('\n');
  assert.deepEqual(find(rust, true).sort(), ['gpt-6-luna', 'gpt-6-luna', 'jev', 'qwen3.5:4b', 'shape:gpt-6-luna', 'shape:gpt-6-luna', 'shape:qwen3.5:4b'].sort());
  const ts = "const a = 'gemini-3.7-flash'; // 'gpt-6-luna'\nconst b = `x ${'qwen3.5:4b'}`; /* gpt-6-luna */ const c = \"app's\"; const d = 'claude-opus-9';";
  assert.deepEqual(find(ts, false).sort(), ['gemini-3.7-flash', 'qwen3.5:4b', 'shape:claude-opus-9', 'shape:gemini-3.7-flash', 'shape:qwen3.5:4b'].sort());
  // Every registry id is found as a string.
  const all = idPattern(registryIds());
  for (const id of registryIds()) assert.ok(`x = "${id}";`.match(all)?.[0] === id, id);
});

test('the shape covers the families the review found missing, in any case, and ids built from a prefix', () => {
  const found = (line: string) => hitsIn(scanned(line, true), idPattern([])).length > 0;
  for (const id of ['chatgpt-4o-latest', 'o3', 'o1-preview', 'o4-mini', 'text-embedding-3-small', 'tts-1-hd', 'Claude-Sonnet-5-5', 'GPT-6-Luna', 'qwen-max', 'qwen-plus', 'kimi-latest', 'nova-premier', 'amazon-nova-pro', 'grok-4', 'mixtral:8x7b', 'phi4:14b', 'command-r', 'au.anthropic.claude-haiku-4-5-20251001-v1:0', 'us-gov.anthropic.claude-sonnet-4-6', 'ca.amazon.nova-lite-v1:0', 'apac.amazon.nova-pro-v1:0', 'global.anthropic.claude-opus-4-6-v1']) {
    assert.ok(found(`const A: &str = "${id}";`), id);
  }
  for (const line of ['let m = format!("gpt-{}-luna", 6);', 'let m = concat!("gemini-", "3.7-flash");', 'let m = "gpt\\x2d6-luna";', 'let m = "gpt\\u{2d}6-luna";']) assert.ok(found(line), line);
  assert.ok(hitsIn(scanned('const m = `qwen-${size}`;', false), idPattern([])).length > 0, 'a template literal');
  // Not ids: a host, a word that only starts like a family, a Rust path.
  for (const line of ['let u = "https://aws.amazon.com/bedrock";', 'let u = "https://api.openai.com/v1";', 'let k = "gemini_compat";', 'use crate::ai::models::jev;', 'let s = "photo";']) {
    assert.deepEqual(hitsIn(scanned(line, true), idPattern([])), [], line);
  }
});

test('the scan reads every file the app compiles in, and no directory it names as not scanned', () => {
  const scannedNow = scannedFiles();
  assert.ok(scannedNow.includes('src-tauri/src/ai/system_prompt_static.md'), 'the system prompt compiled in by include_str!');
  for (const file of ['src-tauri/build.rs', 'src-tauri/tauri.conf.json', 'index.html']) assert.ok(scannedNow.includes(file), file);
  assert.deepEqual(scannedNow.filter(notScanned), []);
  for (const dir of Object.keys(NOT_SCANNED).filter(dir => !['src-tauri/target/', 'src-tauri/gen/'].includes(dir))) {
    assert.ok(statSync(join(root, dir)).isDirectory(), `${dir} exists`);
  }
  // The include_str! targets are found from the shipped code only.
  assert.deepEqual(compiledIn('src-tauri/src/ai/tools.rs', 'include_str!("system_prompt_static.md")'), ['src-tauri/src/ai/system_prompt_static.md']);
  assert.deepEqual(compiledIn('src-tauri/src/ai/registry/load.rs', 'include_str!("../../../../registry/registry.json")'), ['registry/registry.json']);
});

test('the migrations that write a model id are exactly the ones named', () => {
  const listed = idPattern(registryIds());
  const sql = (file: string) => read(file).replace(/--[^\n]*/g, '');
  const withIds = readdirSync(join(root, 'src-tauri/migrations')).map(name => `src-tauri/migrations/${name}`).filter(file => hitsIn(sql(file), listed).length > 0).sort();
  assert.deepEqual(withIds, Object.keys(MIGRATIONS_WITH_IDS).sort(), 'a migration that writes a model id is listed in MIGRATIONS_WITH_IDS with what it writes');
});
