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

// Usage history: the page offers the five periods and the four groupings,
// and reads the table for the chosen ones.
test('the usage panel has the period selector and the group-by control', () => {
  const panel = source('../src/components/CostPanel.tsx');
  assert.match(panel, /useState<PeriodDays>\(30\)/, 'the period starts at 30 days');
  assert.match(panel, /useState<GroupBy>\("model"\)/, 'grouped by model first');
  assert.match(panel, /aria-label=\{text\("Period", "期間"\)\}/);
  assert.match(panel, /\{PERIODS\.map\(\(period\) => <option key=\{period\} value=\{String\(period\)\}>\{periodLabel\(period, text\)\}<\/option>\)\}/);
  assert.match(panel, /aria-label=\{text\("Group by", "分組方式"\)\}/);
  assert.match(panel, /\{GROUP_BYS\.map\(\(by\) => <option key=\{by\} value=\{by\}>\{groupByLabel\(by, text\)\}<\/option>\)\}/);
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
  const storage = between(settings, '{section === "storage" && (<>', 'Free up space');
  assert.match(storage, /AI usage records are not deleted: .* combined into daily usage totals, which hold no prompts or mail and are kept for two years\./);
  assert.match(storage, /AI 用量記錄不會被刪除：.*合併為每日用量總數；每日用量總數不含提示或郵件內容，保留兩年。/);
  assert.doesNotMatch(storage, /related cached records|相關快取記錄/);
  // U8.1: Settings offers no clean-up window under 30 days.
  assert.match(storage, /after 30 days they are combined into daily usage totals/);
  assert.doesNotMatch(storage, /if that is shorter|如清理期限較短/);
  const sweep = between(settings, 'function handleSweep()', 'function handlePurge(');
  assert.match(sweep, /return sweepSummary\(await runStorageSweep\(\), text\);/);
});
