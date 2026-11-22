import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { GEMINI_SORT_MEASURED, costRatio, geminiDefault, localUnavailable, sortCostPer1000 } from '../src/settings/ai-choice.ts';
import { PROVIDER_GUIDES, TEST_AND_SAVE } from '../src/settings/ai/providerGuides.ts';
import type { AiModelCatalog, CloudModel } from '../src/lib/ai-models.ts';
import type { LocalSetupProbe } from '../src/lib/setup.ts';

// Task 10: Settings > Your AI. The pure rules are imported; the JSX cannot be
// imported under node, so its wiring is checked in the source.

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const yourAi = source('../src/settings/sections/YourAi.tsx');
const gemini = source('../src/settings/ai/GeminiCard.tsx');
const local = source('../src/settings/ai/LocalCard.tsx');
const other = source('../src/settings/ai/OtherServiceCard.tsx');
const hook = source('../src/settings/hooks/useProviderChain.ts');
const chooseAi = source('../src/components/ChooseAi.tsx');
const localMail = source('../src/components/LocalMailSetup.tsx');
const dialog = source('../src/settings/SettingsDialog.tsx');
const preview = source('../src/dev/workspacePreview.ts');

const model = (id: string, input: number, output: number, provider = 'gemini'): CloudModel => ({
  provider, id, label: id, family: false, status: 'active', max_output_tokens: null,
  price: { input_per_million: input, output_per_million: output, cache_read_per_million: 0, cache_write_per_million: 0 },
});
const catalog = (cloud: CloudModel[], mailSort: string) => ({ defaults: { gemini_mail_sort: mailSort }, cloud }) as unknown as AiModelCatalog;
const lite = model('gemini-3.1-flash-lite', 0.25, 1.5);
const flash = model('gemini-3.8-flash', 0.75, 3.75);
const cloud = [model('gemini-2.5-flash-lite', 0.1, 0.4), lite, flash];

test('the Gemini default is the registry mail sort default, not the cheapest model', () => {
  assert.equal(geminiDefault(catalog(cloud, 'gemini-3.1-flash-lite'))?.id, 'gemini-3.1-flash-lite');
  assert.equal(geminiDefault(catalog(cloud, 'gemini-3.8-flash'))?.id, 'gemini-3.8-flash');
  assert.equal(geminiDefault(catalog(cloud, 'gemini-unlisted')), null);
  assert.equal(geminiDefault(null), null);
});

test('the sort cost line uses the 22 Nov measurement, only for the measured model', () => {
  assert.equal(GEMINI_SORT_MEASURED.usdPerEmail, 0.00099);
  const measured = catalog(cloud, 'gemini-3.1-flash-lite');
  assert.equal(sortCostPer1000(lite, measured), '1.00');
  assert.equal(sortCostPer1000(flash, measured), null, 'another model hides the line');
  // A different default, or the same id at another price, is not what was measured.
  assert.equal(sortCostPer1000(flash, catalog(cloud, 'gemini-3.8-flash')), null);
  assert.equal(sortCostPer1000(model('gemini-3.1-flash-lite', 0.5, 3), catalog(cloud, 'gemini-3.1-flash-lite')), null);
  assert.equal(sortCostPer1000(null, measured), null);
});

test('cost hints compare a model with the default by input price', () => {
  assert.equal(costRatio(flash, lite), 3);
  assert.equal(costRatio(lite, lite), 1);
  assert.equal(costRatio(model('x', 0.1, 0.4), lite), 0.4);
});

const probe = (overrides: Partial<LocalSetupProbe>): LocalSetupProbe => ({
  os: 'macos', arch: 'aarch64', macosVersion: '15.2', totalRamGb: 16, availableDiskGb: 100,
  ollamaInstalled: false, ollamaRunning: false, recommendedTag: 'sample', downloadUrl: 'https://ollama.com/download/mac', installedModels: [], ...overrides,
});

test('On this Mac is unavailable with the reason the local setup checks', () => {
  assert.equal(localUnavailable(probe({})), null);
  assert.equal(localUnavailable(probe({ macosVersion: '14.0' })), null);
  assert.equal(localUnavailable(probe({ os: 'linux' })), 'unsupported_os');
  assert.equal(localUnavailable(probe({ macosVersion: '13.6' })), 'unsupported_os_version');
  assert.equal(localUnavailable(probe({ macosVersion: null })), 'unsupported_os_version');
  assert.equal(localUnavailable(probe({ recommendedTag: null })), 'insufficient_ram');
});

test('the save button label is "Test and save", the label the Gemini guide tells the user to press', () => {
  assert.deepEqual(TEST_AND_SAVE, { en: 'Test and save', zh: '測試並儲存' });
  const steps = PROVIDER_GUIDES.find(g => g.id === 'gemini')!.steps;
  assert.match(steps[3].en, /"Test and save"/);
  assert.match(steps[3].zh, /「測試並儲存」/);
  for (const card of [gemini, other, local]) {
    assert.match(card, /TEST_AND_SAVE/);
    assert.doesNotMatch(card, /'Test and save'|'測試並儲存'/, 'the label comes from TEST_AND_SAVE only');
  }
});

test('Your AI confirms before replacing a provider chain, then uses the existing choose_ai command', () => {
  const confirm = yourAi.indexOf('needsChainConfirm(');
  const choose = yourAi.indexOf('chooseAi(');
  assert.ok(confirm > 0 && choose > confirm, 'needsChainConfirm( comes before chooseAi(');
  assert.match(yourAi, /confirmAction\(text\('This replaces your provider order in Advanced\. Continue\?', '這會取代你在進階設定中的供應商次序。要繼續嗎？'\)/);
  assert.doesNotMatch(yourAi, /AbortController/, 'closing the dialog does not cancel a save');
});

test('Your AI lists the main-path Gemini models and shows JARVIS AI only when it is offered', () => {
  assert.match(yourAi, /mainPathModels\(/);
  assert.match(yourAi, /jarvisAiAvailable\(/);
  assert.match(yourAi, /const recommended: Card = jarvis \? 'jarvis' : 'gemini'/, 'Recommended goes on JARVIS AI when shown, else Gemini');
  assert.match(yourAi, /openSettings\('advanced-models'\)/);
  assert.match(dialog, /ai: YourAi/);
});

test('Other services use the cheapest model of the chosen service and the guides from one file', () => {
  assert.match(other, /cheapestFor\(/);
  assert.match(other, /guideFor\(/);
  assert.match(gemini, /guideFor\(/);
  assert.match(other, /listAiModels\(/, 'Kimi and Qwen need a model the service lists');
});

test('On this Mac reuses the local setup flow and its compatibility check', () => {
  assert.match(yourAi, /useLocalSetup\(/);
  assert.match(yourAi, /localUnavailable\(/);
  assert.match(yourAi, /disabled: !!unavailable/, 'the card is disabled when the check fails');
  assert.match(local, /setupError\(unavailable, text\)/, 'and says why');
  assert.match(localMail, /export function useLocalSetup\(/);
  assert.match(localMail, /Prepare and use for mail/, 'onboarding keeps its local setup panel');
});

test('ChooseAi keeps its onboarding form and shares its parts', () => {
  for (const name of ['optionFor', 'useAiConsent', 'testDisclosure', 'chosenNotice']) assert.match(chooseAi, new RegExp(`export (function|const) ${name}\\b`));
  assert.match(chooseAi, /Test and use this AI/);
  assert.match(chooseAi, /type="checkbox" checked=\{consent\}/);
  assert.match(yourAi, /useAiConsent\(/);
  assert.match(yourAi, /<ConsentCard /);
  assert.match(hook, /getProviderChain\(/);
});

test('the new files name no model id and use written Chinese', () => {
  for (const file of [yourAi, gemini, local, other, hook]) {
    assert.doesNotMatch(file, /gemini-\d|gpt-\d|claude-/);
    assert.doesNotMatch(file, /[嘅咗唔係冇啲喺嚟]/);
    assert.doesNotMatch(file, /<select/);
  }
});

test('the preview answers the commands Your AI calls', () => {
  for (const command of ['choose_ai', 'probe_local_setup', 'list_ai_models', 'ai_consent_needed', 'ai_google_content_note']) {
    assert.match(preview, new RegExp(`case '${command}'`));
  }
  assert.match(preview, /gemini_mail_sort/);
});
