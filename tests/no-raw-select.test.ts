import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join, relative } from 'node:path';

// Every dropdown goes through the shared Select (src/ui/Select.tsx). The
// allowlist is temporary: Settings is migrated section by section, and the
// list is emptied when the last section moves over.
const ALLOWED = new Set(['src/ui/Select.tsx', 'src/pages/Settings.tsx']);

function tsxFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return tsxFiles(path);
    return entry.name.endsWith('.tsx') ? [path] : [];
  });
}

test('no raw <select> outside the shared Select', () => {
  const offenders: string[] = [];
  for (const file of tsxFiles('src')) {
    const rel = relative('.', file);
    if (ALLOWED.has(rel)) continue;
    readFileSync(file, 'utf8').split('\n').forEach((line, i) => {
      if (line.includes('<select')) offenders.push(`${rel}:${i + 1}`);
    });
  }
  assert.deepEqual(offenders, [], `raw <select> found:\n${offenders.join('\n')}`);
});
