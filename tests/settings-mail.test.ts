import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { ROWS } from '../src/settings/rows.ts';
import { canSave, mergeMailForm, type CanSaveInput } from '../src/settings/hooks/useMailSettings.ts';
import { sortedByText } from '../src/lib/mail-presentation.ts';
import type { MailSettings } from '../src/lib/mail.ts';

// Task 13: Settings > Mail and follow-ups. The Save rule and the merge are
// pure and imported; the JSX cannot be imported under node, so its wiring is
// checked in the source.

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const MAIL = '../src/settings/sections/Mail.tsx';
const HOOK = '../src/settings/hooks/useMailSettings.ts';
const EMAIL = '../src/pages/EmailPage.tsx';
const en = (english: string) => english;

const ready: CanSaveInput = { processingMode: 'assistant', analysisConsent: true, sorting: true, sortingConsent: true, bedrockModel: '' };

test('canSave is false with either tick missing, and true with both', () => {
  assert.equal(canSave(ready), true);
  assert.equal(canSave({ ...ready, analysisConsent: false }), false);
  assert.equal(canSave({ ...ready, sortingConsent: false }), false);
  assert.equal(canSave({ ...ready, analysisConsent: false, sortingConsent: false }), false);
});

// The rule as EmailPage.tsx had it: the analysis tick was `required` for
// every mode except Ollama on this Mac, the sorting tick only while sorting
// was ticked, and Bedrock needed a model.
test('canSave keeps the old conditions: local needs no analysis tick, sorting off needs no sorting tick, Bedrock needs a model', () => {
  assert.equal(canSave({ ...ready, processingMode: 'local', analysisConsent: false }), true);
  for (const mode of ['assistant', 'openai', 'bedrock']) {
    assert.equal(canSave({ ...ready, processingMode: mode, analysisConsent: false, bedrockModel: 'm' }), false, mode);
  }
  assert.equal(canSave({ ...ready, sorting: false, sortingConsent: false }), true);
  assert.equal(canSave({ ...ready, processingMode: 'bedrock', bedrockModel: '  ' }), false);
  assert.equal(canSave({ ...ready, processingMode: 'bedrock', bedrockModel: 'apac.amazon.nova-micro-v1:0' }), true);
});

test('mergeMailForm takes the models from the fresh copy and only the processing mode from the form', () => {
  const fresh: MailSettings = { processing_mode: 'assistant', local_model: 'qwen3:8b', openai_model: 'gpt-6-luna', bedrock_model: 'fresh-bedrock' };
  const form = { processing_mode: 'bedrock', local_model: 'stale', openai_model: 'stale', bedrock_model: 'stale' } as MailSettings;
  const merged = mergeMailForm(fresh, form);
  assert.deepEqual(merged, { processing_mode: 'bedrock', local_model: 'qwen3:8b', openai_model: 'gpt-6-luna', bedrock_model: 'fresh-bedrock' });
  assert.equal(fresh.processing_mode, 'assistant', 'the fresh copy is not changed in place');
});

test('Save reads the mail settings again just before it writes', () => {
  const hook = source(HOOK);
  const body = hook.slice(hook.indexOf('async function save('));
  const read = body.indexOf('(await mailWorkspace()).settings');
  const merge = body.indexOf('mergeMailForm(');
  const write = body.indexOf('await mailSaveSettings(');
  assert.ok(read > 0 && merge > read && write > merge, 'read, merge, then write');
});

test('the sorting status reuses the Mail page text', () => {
  assert.equal(sortedByText({ typesafe_key: false, chosen_ai: null }, en), 'Choose your AI in Settings → Your AI first.');
  assert.match(sortedByText({ typesafe_key: false, chosen_ai: 'Gemini' }, en), /^Gemini, the AI you chose/);
  assert.match(sortedByText({ typesafe_key: true, chosen_ai: 'Gemini' }, en), /^TypeSafe Jev decides/);
});

test('the dialog shows Mail through its own component', () => {
  const dialog = source('../src/settings/SettingsDialog.tsx');
  assert.match(dialog, /import Mail from '\.\/sections\/Mail'/);
  assert.match(dialog, /\n {2}mail: Mail,/);
});

test('Mail renders its rows, in order, by row id', () => {
  const code = source(MAIL);
  const ids = ROWS.filter(r => r.section === 'mail').map(r => r.id);
  assert.deepEqual(ids, ['mail-auto-sort', 'mail-triage-consent', 'mail-ai-status', 'mail-processing', 'mail-analysis-consent']);
  let last = -1;
  for (const id of ids) {
    const at = code.indexOf(`row('${id}')`);
    assert.ok(at > last, `row ${id} is missing or out of order`);
    last = at;
  }
});

test('Mail: sorting Switch with a Recommended badge, one Save, links to Advanced and Your AI, no model fields', () => {
  const code = source(MAIL);
  const hook = source(HOOK);
  assert.match(code, /<Switch /);
  assert.match(code, /badge=\{text\('Recommended', '推薦'\)\}/);
  assert.match(hook, /await mailSetAutoTriage\(next\)/);
  assert.equal(code.split('kind="primary"').length, 2, 'one primary button: Save');
  assert.match(code, /openSettings\('advanced-models'\)/);
  assert.match(code, /Choose the mail model in Advanced → Models/);
  assert.match(code, /在進階 → 模型中選擇郵件模型/);
  assert.match(code, /openSettings\('ai'\)/);
  assert.match(code, /Settings → Advanced → Local AI/);
  for (const field of ['local_model', 'openai_model', 'BedrockModelPicker', 'TypeSafe API key', '<select']) {
    assert.ok(!code.includes(field), `Mail.tsx has no ${field}`);
  }
  assert.ok(!hook.includes('saveTypeSafeKey'), 'the TypeSafe key is saved in Advanced > Cloud providers');
});

test('a failed sorting toggle rolls back and says so in both languages', () => {
  const hook = source(HOOK);
  assert.match(hook, /catch[\s\S]{0,200}setSorting\(previous\)/);
  assert.match(hook, /Could not change AI sorting/);
  assert.match(hook, /無法更改 AI 整理設定/);
});

test('EmailPage opens Settings > Mail and no longer saves mail settings itself', () => {
  const email = source(EMAIL);
  assert.match(email, /openSettings\('mail'\)/);
  assert.ok(!email.includes('mailSaveSettings('), 'no mailSaveSettings(');
  assert.ok(!email.includes('type: "settings"'), 'the settings dialog branch is gone');
  assert.ok(!email.includes('saveTypeSafeKey'), 'the TypeSafe key field is gone');
  assert.ok(!email.includes('Settings → Local AI'), 'no stale Settings → Local AI');
  assert.ok(!email.includes('Settings → Models'), 'no stale Settings → Models');
});

test('the new Mail files are written Chinese and stay small', () => {
  for (const file of [MAIL, HOOK]) {
    const code = source(file);
    assert.doesNotMatch(code, /[嘅咗唔係冇啲喺嚟]/, file);
    assert.ok(code.split('\n').length <= 250, `${file} is over 250 lines`);
  }
});
