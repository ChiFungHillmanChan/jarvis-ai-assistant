import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { GEMINI_SORT_MEASURED, costRatio, geminiDefault, geminiEffectiveModel, geminiModelNote, geminiModelToSave, localUnavailable, sortCostPer1000 } from '../src/settings/ai-choice.ts';
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
  assert.match(yourAi, /useSettingsLocalSetup\('ai'\)/);
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

// Fix round 1.

test('the mail sort default is saved as no model, so chat keeps the registry chat default', () => {
  const lite = 'gemini-3.1-flash-lite';
  assert.deepEqual(geminiModelToSave(lite, null, lite), { model: undefined, changed: false }, 'new user or null saved: default selected');
  assert.deepEqual(geminiModelToSave('gemini-3.8-flash', null, lite), { model: 'gemini-3.8-flash', changed: true });
  assert.deepEqual(geminiModelToSave(lite, 'gemini-3.8-flash', lite), { model: undefined, changed: true }, 'back to the default unpins');
  assert.deepEqual(geminiModelToSave('gemini-3.8-flash', 'gemini-3.8-flash', lite), { model: 'gemini-3.8-flash', changed: false });
  assert.deepEqual(geminiModelToSave(lite, lite, lite), { model: undefined, changed: false }, 'a pin equal to the default is not a change');
});

test('the Gemini card sends geminiModelToSave and shows the saved-key button only on a change', () => {
  assert.match(gemini, /geminiModelToSave\(model, saved, recommended\?\.id/);
  assert.match(gemini, /current\?\.provider === 'gemini' && toSave\.changed &&/);
  assert.doesNotMatch(gemini, /model: model \|\| undefined/);
});

test('the default model is labelled Recommended, never lowest cost', () => {
  assert.doesNotMatch(gemini, /lowest|最低/i);
  assert.match(gemini, /text\('Recommended', '推薦'\)/);
  assert.match(gemini, /The recommended model handles mail well and costs little\./);
  assert.match(gemini, /推薦的模型能妥善整理郵件，費用低。/);
});

test('the card face cost line follows the effective Gemini model', () => {
  const measured = catalog(cloud, 'gemini-3.1-flash-lite');
  assert.equal(geminiEffectiveModel(measured, null)?.id, 'gemini-3.1-flash-lite');
  assert.equal(geminiEffectiveModel(measured, 'gemini-3.1-flash-lite')?.id, 'gemini-3.1-flash-lite');
  assert.equal(geminiEffectiveModel(measured, 'gemini-3.8-flash')?.id, 'gemini-3.8-flash');
  assert.equal(sortCostPer1000(geminiEffectiveModel(measured, 'gemini-3.8-flash'), measured), null);
  assert.match(yourAi, /sortCostPer1000\(geminiEffectiveModel\(catalog, current\?\.provider === 'gemini' \? current\.model : null\), catalog\)/);
});

test('On this Mac downloads without touching mail, refreshes, and saves through Test and save', () => {
  assert.match(source('../src/settings/LocalSetupContext.tsx'), /const ai = useLocalSetup\(\{ applyToMail: false, probeOnMount: false, onReady: \(\) => ai\.refresh\(\) \}\);/);
  assert.match(localMail, /applyToMail = true/);
  assert.match(localMail, /if \(!applyToMail\)/);
  assert.match(localMail, /useLocalSetup\(\{ onReady, onBusyChange \}\)/, 'onboarding still applies the model to mail');
  assert.match(local, /onClick=\{setup\.refresh\}>\{text\('Check again', '重新檢查'\)\}/);
  assert.doesNotMatch(local, /The model is ready/);
});

test('Other services carries the OpenAI region warning from ChooseAi', () => {
  assert.match(chooseAi, /export function openAiRegionNote\(/);
  assert.match(chooseAi, /\{openAiRegionNote\(text\)\}/);
  assert.match(other, /service === 'openai' && <p className="ui-hint" role="note">\{openAiRegionNote\(text\)\}/);
  assert.equal((other.match(/Hong Kong/g) ?? []).length, 0, 'the text lives in ChooseAi only');
});

// Review fix (Task 19, inventory row 26): the test-message consent is a tick,
// as in ChooseAi, and Test stays disabled until it is on. One component in
// ChooseAi renders it everywhere, so the copy and behaviour stay in sync.
test('the test consent is one shared tick from ChooseAi', () => {
  assert.match(chooseAi, /export function TestConsent\(/);
  assert.match(chooseAi, /<label id=\{id\} className="setup-checkbox"><input type="checkbox" checked=\{consent\} disabled=\{disabled\} onChange=\{e => onChange\(e\.target\.checked\)\} \/>\{testDisclosure\(option, name, text\)\}<\/label>/);
  assert.match(chooseAi, /<TestConsent option=\{option\} name=\{name\} consent=\{consent\} disabled=\{!!busy\} onChange=\{setConsent\} \/>/);
  assert.match(chooseAi, /const ready = !!provider && consent && /);
  for (const [card, code] of [['Gemini', gemini], ['Other', other], ['Local', local]] as const) {
    assert.match(code, /<TestConsent id="ai-test-consent" /, `${card}: the tick carries the row id`);
    assert.doesNotMatch(code, /<p className="ui-hint">\{testDisclosure\(/, `${card}: no bare statement`);
  }
});

test('each Your AI card keeps Test and save disabled without the tick', () => {
  // Gemini: the key field's Test and save, and "Use this model with the saved key".
  assert.match(gemini, /saveDisabled=\{!consent\}/);
  assert.match(gemini, /<Button kind="secondary" busy=\{busy\} disabled=\{!consent\} onClick=/);
  // Another service: OpenAI and Claude test from the key field; Kimi and Qwen
  // load models from it (not a test, as in ChooseAi) and test from the button.
  assert.match(other, /saveDisabled=\{!named && !consent\}/);
  assert.match(other, /<Button kind="primary" busy=\{busy\} disabled=\{!model \|\| !consent\} onClick=\{\(\) => save\(pending\.current\)\}/);
  assert.match(other, /function pick\(next: Service\) \{[^}]*setConsent\(false\);/, 'a new service needs the tick again');
  // On this Mac.
  assert.match(local, /disabled=\{!chosen \|\| setup\.busy \|\| !consent\}/);
  // A successful test clears the tick, as in ChooseAi.
  for (const code of [gemini, other, local]) assert.match(code, /setConsent\(false\)/);
  // SecretField honours saveDisabled on the button and on Enter.
  const field = source('../src/ui/SecretField.tsx');
  assert.match(field, /if \(!secret \|\| busy \|\| saveDisabled\) return;/);
  assert.match(field, /disabled=\{saveDisabled \|\| !draft\.trim\(\)\}/);
});

test('every Your AI row has its DOM id, so a search hit scrolls to it', () => {
  const all = yourAi + gemini + other + local;
  for (const id of ['ai-current', 'ai-consent', 'ai-google-note', 'ai-choice', 'ai-region', 'ai-key', 'ai-model', 'ai-ollama', 'ai-test-consent', 'ai-test-use', 'ai-mail-link'])
    assert.match(all, new RegExp(`id="${id}"`), id);
});

// Final review I1: closing Settings or changing section must not cancel an
// Ollama download. A run is cancelled when its owner unmounts, and dialog
// content unmounts on both, so Settings' runs are owned above the dialog.
test('the Settings download runs are owned above the dialog, so a close does not cancel them', () => {
  const context = source('../src/settings/LocalSetupContext.tsx');
  assert.match(context, /export function LocalSetupProvider\(/);
  assert.match(context, /const mail = useLocalSetup\(\{ probeOnMount: false \}\);/, 'Advanced → Local AI still applies the model to mail');
  assert.match(source('../src/settings/SettingsContext.tsx'), /<SettingsContext\.Provider value=\{value\}><LocalSetupProvider>\{children\}<\/LocalSetupProvider><\/SettingsContext\.Provider>/);
  assert.doesNotMatch(source('../src/settings/SettingsDialog.tsx'), /LocalSetupProvider/, 'not inside the dialog, which unmounts its content');
  // No section owns a run of its own.
  const owners = ['sections/YourAi.tsx', 'sections/advanced/LocalAi.tsx', 'ai/LocalCard.tsx'].filter(f => /\buseLocalSetup\(|<LocalMailSetup\b/.test(source(`../src/settings/${f}`)));
  assert.deepEqual(owners, []);
  assert.match(source('../src/settings/sections/advanced/LocalAi.tsx'), /useSettingsLocalSetup\('mail'\)/);
  assert.match(source('../src/settings/sections/advanced/LocalAi.tsx'), /<LocalMailSetupPanel setup=\{setup\} blocked=\{otherBusy\}/);
});

test('a reopened Your AI shows the running download, and only one run goes at a time', () => {
  const context = source('../src/settings/LocalSetupContext.tsx');
  assert.match(context, /useEffect\(\(\) => \{ if \(!setup\.busy\) void setup\.refresh\(\); \}, \[\]\);/, 'the Mac is checked on open, never during a run');
  assert.match(context, /const otherBusy = value\[which === 'ai' \? 'mail' : 'ai'\]\.busy;/);
  assert.match(yourAi, /const card = picked \?\? \(local\.busy \? 'local' :/);
  assert.match(local, /disabled=\{busy \|\| blocked \|\| setup\.checking/);
  assert.match(localMail, /disabled=\{busy \|\| blocked \|\| checking/);
});

test('onboarding keeps its own run, which still checks the Mac on mount', () => {
  assert.match(localMail, /export default function LocalMailSetup\([\s\S]*?\) \{\s*const setup = useLocalSetup\(\{ onReady, onBusyChange \}\);\s*return <LocalMailSetupPanel setup=\{setup\} confirmPrepare=\{confirmPrepare\} \/>;/);
  assert.match(localMail, /probeOnMount = true/);
  assert.match(localMail, /if \(probeOnMount\) void refresh\(\);/);
  for (const file of ['../src/components/ChooseAi.tsx', '../src/components/LocalAiSetup.tsx']) assert.match(source(file), /<LocalMailSetup\b/, file);
});

// Final review M4: the Gemini model select says which work the model does.
test('the Gemini model note follows what geminiModelToSave saves, with names from the catalog', () => {
  const en = (a: string) => a;
  const zh = (_: string, b: string) => b;
  const withChat = { defaults: { gemini_mail_sort: lite.id, gemini_flash: flash.id }, cloud } as unknown as AiModelCatalog;
  // The mail sort default saves no model: only mail sorting uses it, chat keeps the registry chat default.
  assert.equal(geminiModelNote(withChat, geminiModelToSave(lite.id, null, lite.id).model, en), `Used for sorting mail. Chat uses ${flash.label}.`);
  assert.equal(geminiModelNote(withChat, geminiModelToSave(lite.id, null, lite.id).model, zh), `用於整理郵件。對話使用 ${flash.label}。`);
  // Any other model is pinned, so chat uses it too.
  assert.equal(geminiModelNote(withChat, geminiModelToSave(flash.id, null, lite.id).model, en), 'Used for chat and for sorting mail.');
  assert.equal(geminiModelNote(withChat, geminiModelToSave(flash.id, null, lite.id).model, zh), '用於對話及整理郵件。');
  // A chat default the catalog does not list shows its id.
  const unlisted = { defaults: { gemini_mail_sort: lite.id, gemini_flash: 'gemini-unlisted' }, cloud } as unknown as AiModelCatalog;
  assert.equal(geminiModelNote(unlisted, undefined, en), 'Used for sorting mail. Chat uses gemini-unlisted.');
  assert.match(gemini, /<p id="ai-model-use" className="ui-hint">\{geminiModelNote\(catalog, toSave\.model, text\)\}<\/p>/);
});
