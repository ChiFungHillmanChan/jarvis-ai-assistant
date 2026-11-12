import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

// Settings.tsx and App.tsx are JSX, which these tests cannot render, so
// their wiring is checked in the source. A real window shows the rest.
const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const settings = source('../src/pages/Settings.tsx');

/** The text between two markers, so a check reads one block, not the file. */
function between(text: string, start: string, end: string): string {
  const from = text.indexOf(start);
  assert.ok(from >= 0, `missing: ${start}`);
  const to = text.indexOf(end, from + start.length);
  assert.ok(to > from, `missing after ${start}: ${end}`);
  return text.slice(from, to);
}

// R6: the panel was once built and never shown (I1). This fails if Settings
// stops listing the Usage section or stops rendering the panel in it.
test('Settings lists a Usage section that shows the usage panel', () => {
  assert.match(settings, /^import CostPanel from "\.\.\/components\/CostPanel";$/m);
  const sections = between(settings, 'const SECTIONS', '];');
  assert.match(sections, /\{ id: "usage", label: text\("Usage", "用量"\)/);
  const usage = between(settings, '{section === "usage" && (', '{section === "storage"');
  assert.match(usage, /<CostPanel \/>/);
  assert.match(usage, /Usage recorded on this Mac/);
});

// R13: a refused model id is shown at the field the user just saved.
test('the Bedrock model field shows its own save error', () => {
  const field = between(settings, 'function BedrockModelField(', 'function formatBytes(');
  assert.match(field, /onSave: \(model: string\) => Promise<string \| null>/);
  assert.match(field, /setError\(await onSave\(draft\.trim\(\)\)\)/);
  assert.match(field, /\{error && <p role="alert" className="settings-bedrock-model-error">\{error\}<\/p>\}/);
  const handler = between(settings, 'async function handleSetBedrockModel(', 'async function handleToggleChainEntry(');
  assert.doesNotMatch(handler, /setLlmError/, 'the error is not sent to the top of the page');
  assert.match(handler, /return String\(e\);/);
});

// R8: the effect that saves the language in use for someone past the
// language step (the store and the screen rule are tested in
// onboarding-locale.test.ts).
test('the app saves the language in use once the startup screen is known', () => {
  const app = source('../src/App.tsx');
  assert.match(app, /if \(localeReady && keepsLocaleInUse\(screen\)\) void keepUiLocaleInUse\(\);/);
});
