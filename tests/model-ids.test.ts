// P05: the app takes every model id from the model registry at runtime, so
// no model id is written in the app's code. This scans the app's sources
// (src/ and src-tauri/src/) for model ids: every id in the registry, and
// anything shaped like one. Comments are skipped, and so is code that never
// reaches a user's build: Rust items and modules compiled only for tests or
// the native QA build (#[cfg(test)], #[cfg(feature = "native-qa")] and
// #[cfg(any(test, feature = "native-qa"))]).
//
// Not scanned, by design:
// - registry/: the registry itself, the one place ids are kept.
// - src-tauri/src/ai/registry/fixture.json: the frozen copy the tests run on.
// - src-tauri/migrations/: history. A migration that set a model id once
//   (V30's column defaults, the chain rows of earlier ones) must keep the
//   text it ran with; saved choices it wrote stay valid.
// - tests/: test fixtures.
// - infra/: the server side (the registry Lambda and, from P08, the proxy),
//   which is not the app.
//
// Every remaining hit is listed in ALLOWED with its reason.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

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

/** Anything shaped like a model id, whether the registry lists it or not. */
export const SHAPE = /(?<![\w.:-])(?:(?:global|us|eu|apac|jp)\.)?(?:gpt-\d[\w.:-]*|o\d-(?:mini|pro)[\w.:-]*|claude-(?:opus|sonnet|haiku|\d)[\w.:-]*|gemini-(?:\d|flash|pro)[\w.:-]*|(?:anthropic|amazon|meta|mistral|cohere)\.[a-z][\w.:-]*|qwen\d[\w.:-]*|llama\d[\w.:-]*|gemma\d[\w.:-]*|mistral:[\w.-]+|deepseek-[\w.:-]+|kimi-k\d[\w.:-]*|moonshot-v\d[\w.:-]*|jev-[\w.:-]+|ggml-[\w-]+|whisper-[\w.:-]+|speech-\d[\w.:-]*)(?![\w:-])/g;

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
  return new RegExp(parts.join('|'), 'g');
}

function files(dir: string, keep: (path: string) => boolean): string[] {
  return readdirSync(join(root, dir)).flatMap(name => {
    const path = `${dir}/${name}`;
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

/** What the scan reads of one file. */
export function scanned(source: string, rust: boolean): string {
  const { code, bare } = lex(source, rust);
  return rust ? withoutTestItems(code, bare) : code;
}

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

function scan(): Map<string, Set<string>> {
  const listed = idPattern(registryIds());
  const rust = files('src-tauri/src', path => path.endsWith('.rs'));
  const testOnly = testModuleFiles(rust);
  const sources = [...files('src', path => /\.(?:ts|tsx)$/.test(path)), ...rust.filter(path => !testOnly.has(path))];
  const hits = new Map<string, Set<string>>();
  for (const file of sources) {
    const code = scanned(read(file), file.endsWith('.rs'));
    for (const pattern of [listed, SHAPE]) {
      for (const match of code.matchAll(pattern)) {
        if (!hits.has(file)) hits.set(file, new Set());
        hits.get(file)!.add(match[0]);
      }
    }
  }
  return hits;
}

test('no model id is written in the app code outside the registry and test fixtures', () => {
  const unexpected: string[] = [];
  const used = new Set<string>();
  for (const [file, ids] of scan()) {
    for (const id of ids) {
      const key = `${file}|${id}`;
      if (ALLOWED[key]) used.add(key); else unexpected.push(key);
    }
  }
  assert.deepEqual(unexpected.sort(), [], 'read these ids from the registry (ai_models or ai::models), or list them in ALLOWED with the reason');
  // An exception that no longer matches anything is dropped from the list.
  assert.deepEqual(Object.keys(ALLOWED).filter(key => !used.has(key)), [], 'remove the exceptions that no longer match');
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
