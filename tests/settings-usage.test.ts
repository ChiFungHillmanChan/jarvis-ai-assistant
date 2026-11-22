import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

// The settings sections and App.tsx are JSX, which these tests cannot render,
// so their wiring is checked in the source. A real window shows the rest.
const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

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
  const sections = source('../src/settings/sections.ts');
  assert.match(between(sections, 'export const NAV_GROUPS', '];'), /'usage'/);
  assert.match(sections, /case 'usage': return text\('Usage', '用量'\);/);
  assert.match(source('../src/settings/SettingsDialog.tsx'), /^  usage: Usage,$/m);
  const usage = source('../src/settings/sections/Usage.tsx');
  assert.match(usage, /^import CostPanel from '\.\.\/\.\.\/components\/CostPanel';$/m);
  assert.match(usage, /\{detailsOpen && <CostPanel \/>\}/);
});

// R13: a refused model id is shown at the field the user just saved.
test('the Bedrock model field shows its own save error', () => {
  const field = source('../src/components/BedrockModelPicker.tsx');
  assert.match(field, /onSave: \(model: string\) => Promise<string \| null>/);
  assert.match(field, /dispatch\(\{ type: "saved", error: await onSave\(value\) \}\);/);
  assert.match(field, /\{state\.error && <p role="alert" className="settings-bedrock-model-error">\{state\.error\}<\/p>\}/);
  // Provider order saves the model quietly: the error goes back to the field,
  // not to the order's own error line.
  assert.match(source('../src/settings/sections/advanced/ProviderOrder.tsx'), /onSave=\{model => save\(setEntryModel\(chain, i, model\), true\)\}/);
  const handler = between(source('../src/settings/hooks/useChainEditor.ts'), 'const save = useCallback(', '}, [');
  assert.match(handler, /if \(!quiet\) setSaveError\(String\(e\)\);/, 'the error is not sent to the top of the section');
  assert.match(handler, /return String\(e\);/);
});

// R8: the effect that saves the language in use for someone past the
// language step (the store and the screen rule are tested in
// onboarding-locale.test.ts).
test('the app saves the language in use once the startup screen is known', () => {
  const app = source('../src/App.tsx');
  assert.match(app, /if \(localeReady && keepsLocaleInUse\(screen\)\) void keepUiLocaleInUse\(\);/);
});

// Usage history: the page offers the five periods and the four groupings,
// and reads the table for the chosen ones.
test('the usage panel has the period selector and the group-by control', () => {
  const panel = source('../src/components/CostPanel.tsx');
  assert.match(panel, /useState<PeriodDays>\(30\)/, 'the period starts at 30 days');
  assert.match(panel, /useState<GroupBy>\("model"\)/, 'grouped by model first');
  assert.match(panel, /<Select label=\{text\("Period", "期間"\)\} showLabel value=\{String\(days\)\}/);
  assert.match(panel, /setDays\(Number\(v\) as PeriodDays\)/, 'the period is a number again at the boundary');
  assert.match(panel, /PERIODS\.map\(\(period\) => \(\{ value: String\(period\), label: periodLabel\(period, text\) \}\)\)/);
  assert.match(panel, /<Select label=\{text\("Group by", "分組方式"\)\} showLabel value=\{groupBy\}/);
  assert.match(panel, /GROUP_BYS\.map\(\(by\) => \(\{ value: by, label: groupByLabel\(by, text\) \}\)\)/);
  assert.match(panel, /useTauriCommand<CostStats>\("get_cost_stats", \{ days \}\)/);
  assert.match(panel, /useTauriCommand<BlendedCost>\("get_blended_cost", \{ days \}\)/);
  assert.match(panel, /useTauriCommand<UsageGroup\[\]>\("get_usage_groups", \{ days, groupBy \}\)/);
  assert.match(panel, /groupRows\(groups, groupBy, text\)/);
  assert.match(panel, /\{row\.input\}\s*\{row\.characters && <span style=\{styles\.detail\}>\{row\.characters\}<\/span>\}/, 'speech characters under the input, apart from the tokens');
  assert.match(panel, /expensiveHeading\(data\.expensive_days, text\)/);
  assert.match(panel, /historyLine\(data\.per_request_days, text\)/, 'the history line says the window, the heading the days it covers');
});

// The clean-up text says what happens to AI usage, in both languages, and
// the report comes from the function its test checks.
test('the Storage clean-up text and report say usage is kept as daily totals', () => {
  const privacy = source('../src/settings/sections/Privacy.tsx');
  const cleanup = between(privacy, "<SettingGroup title={text('Automatic clean-up', '自動清理')}>", '</SettingGroup>');
  assert.match(cleanup, /AI usage records are combined into daily totals after 30 days and kept for two years\./);
  assert.match(cleanup, /AI 用量記錄會在 30 日後合併為每日用量總數，保留兩年。/);
  assert.doesNotMatch(cleanup, /related cached records|相關快取記錄/);
  // U8.1: Settings offers no clean-up window under 30 days.
  assert.match(privacy, /^const WINDOWS = \['30', '90', '180', '365'\] as const;$/m);
  assert.doesNotMatch(cleanup, /if that is shorter|如清理期限較短/);
  assert.match(source('../src/settings/hooks/useStorage.ts'), /const sweep = \(\) => run\('cleanup-now', async \(\) => sweepSummary\(await runStorageSweep\(\), text\)\);/);
});

// The button that adds the Gemini premium default shows that model's
// registry name (addModelLabel), so its text follows the registry the app is
// using; "+ Gemini Pro" went stale when the default became Gemini 4 Neon.
test('the Settings button that adds the Gemini premium default is named from the registry', () => {
  const order = source('../src/settings/sections/advanced/ProviderOrder.tsx');
  assert.match(order, /^import \{[^}]*\baddModelLabel\b[^}]*\} from '\.\.\/\.\.\/\.\.\/lib\/ai-catalog';$/m);
  const button = between(order, "add({ provider_type: 'gemini', model_id: defaults.gemini_pro })}", '</Button>');
  assert.match(button, />\{addModelLabel\(catalog, defaults\.gemini_pro\)\}$/);
});

// Inventory row 61: the old Usage panel said what is recorded and that the
// figures are estimates; the provider's bill is the final figure.
test('Usage says what is recorded and that the bill is the final figure', () => {
  const usage = source('../src/settings/sections/Usage.tsx');
  assert.match(usage, /<p className="ui-hint settings-usage-about">\{text\(/);
  assert.match(usage, /recorded only on this Mac\. Costs are worked out from list prices where JARVIS knows them, so your provider's bill is the final figure\./);
  assert.match(usage, /只記錄在此 Mac。費用按 JARVIS 已知的公開價目計算，實際收費以供應商帳單為準。/);
  assert.match(usage, /your AWS bill is the final figure\./);
});
