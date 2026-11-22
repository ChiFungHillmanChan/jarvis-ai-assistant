import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
  HKD_PER_USD, MAIN_PERIODS, barHeights, chronological, dailySummary, dailyTitle, fillDays, formatMoney, splitShares, splitSummary, spentFigure, toHkd, usageTiles,
} from '../src/settings/usage-view.ts';
import { ROWS, ROWS_BY_ID } from '../src/settings/rows.ts';
import type { CostStats, UsageGroup } from '../src/lib/usage.ts';

// Task 12: Settings > Usage. The figures are pure functions; the JSX cannot be
// imported under node, so its wiring is checked in the source.

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;
const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const usage = () => source('../src/settings/sections/Usage.tsx');
const charts = () => source('../src/settings/usage/charts.tsx');

function group(key: string, requests: number, usd = 0, unpriced = 0): UsageGroup {
  return { key, provider: null, requests, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, usd_cost: usd, price_unknown_requests: unpriced, speech_characters: 0, from_day: null };
}
function stats(over: Partial<CostStats> = {}): CostStats {
  return { days: 30, total_requests: 15, total_usd: 2.41, avg_usd_per_request: 0.16, local_requests: 0, local_share_pct: 0, price_unknown_requests: 0, per_request_days: 30, expensive_days: 30, top_expensive: [], ...over };
}

test('HK$ is US$ at a fixed 7.8', () => {
  assert.equal(HKD_PER_USD, 7.8);
  assert.ok(Math.abs(toHkd(2.41) - 18.8) < 0.01);
});

test('money reads HK$ first, then US$, the same way everywhere', () => {
  assert.equal(formatMoney(2.41), 'HK$18.80 (US$2.41)');
  assert.equal(formatMoney(0), 'HK$0 (US$0)');
  assert.equal(formatMoney(200), 'HK$1,560.00 (US$200.00)');
  assert.equal(formatMoney(0.0004), '< HK$0.01 (US$0.0004)');
});

test('the Spent tile leaves out the requests with no price, never reading HK$0 for them', () => {
  assert.deepEqual(spentFigure(2.41, 0, en), { value: 'HK$18.80', usd: '(US$2.41)' });
  assert.deepEqual(spentFigure(2.41, 3, en), { value: 'At least HK$18.80', usd: '(US$2.41)' });
  assert.deepEqual(spentFigure(2.41, 3, zh), { value: '最少 HK$18.80', usd: '(US$2.41)' });
  assert.deepEqual(spentFigure(0, 3, en), { value: 'Price unknown', usd: null });
});

test('usageTiles counts mail sorting, and chat with tools', () => {
  const tiles = usageTiles([group('mail_sort', 10, 1.2), group('chat', 3, 0.5), group('tools', 2, 0.3), group('briefing', 4, 0.1)], stats({ price_unknown_requests: 2 }));
  assert.deepEqual(tiles, { spentUsd: 2.41, mailRequests: 10, chatRequests: 5, unpriced: 2 });
  assert.deepEqual(usageTiles([], stats({ total_usd: 0 })), { spentUsd: 0, mailRequests: 0, chatRequests: 0, unpriced: 0 });
});

test('barHeights: 0 stays 0, the largest is the full height, a small value is still seen', () => {
  assert.deepEqual(barHeights([0, 5, 10], 168), [0, 84, 168]);
  assert.equal(barHeights([1, 1000], 168)[0], 2);
  assert.deepEqual(barHeights([0, 0], 168), [0, 0]);
  assert.deepEqual(barHeights([], 168), []);
});

test('splitShares sorts by cost and its shares sum to 1', () => {
  const shares = splitShares([group('chat', 3, 1), group('mail_sort', 10, 3)]);
  assert.deepEqual(shares.map(s => s.feature), ['mail_sort', 'chat']);
  assert.deepEqual(shares.map(s => s.share), [0.75, 0.25]);
  const zero = splitShares([group('chat', 3, 0), group('mail_sort', 1, 0)]);
  assert.deepEqual(zero.map(s => s.share), [0, 0]);
});

test('the time grouping comes newest first and the chart reads oldest first', () => {
  assert.deepEqual(chronological([group('2026-11-21', 1), group('2026-11-20', 1)]).map(g => g.key), ['2026-11-20', '2026-11-21']);
});

test('fillDays draws every UTC day between the first and last, with zero bars for days without usage', () => {
  const filled = fillDays([group('2026-11-10', 2, 0.2), group('2026-11-01', 1, 0.1), group('2026-11-05', 3, 0.3)]);
  assert.equal(filled.length, 10);
  assert.deepEqual(filled.map(g => g.key), ['2026-11-01', '2026-11-02', '2026-11-03', '2026-11-04', '2026-11-05', '2026-11-06', '2026-11-07', '2026-11-08', '2026-11-09', '2026-11-10']);
  assert.deepEqual(filled.map(g => g.requests), [1, 0, 0, 0, 3, 0, 0, 0, 0, 2]);
  assert.equal(filled[1].usd_cost, 0);
  // Across a month end and a year end, in UTC.
  assert.deepEqual(fillDays([group('2026-12-30', 1), group('2027-01-02', 1)]).map(g => g.key), ['2026-12-30', '2026-12-31', '2027-01-01', '2027-01-02']);
  assert.deepEqual(fillDays([]), []);
});

test('fillDays fills missing months the same way, keeping each month as given', () => {
  const months = fillDays([group('2026-11', 4, 1), group('2026-08', 2, 1), { ...group('2025-12', 1, 0.5), from_day: '2025-12-18' }]);
  assert.deepEqual(months.map(g => g.key), ['2025-12', '2026-01', '2026-02', '2026-03', '2026-04', '2026-05', '2026-06', '2026-07', '2026-08', '2026-09', '2026-10', '2026-11']);
  assert.equal(months[0].from_day, '2025-12-18');
  assert.equal(months[1].requests, 0);
});

test('the chart title says day or month, matching its bars', () => {
  assert.equal(dailyTitle(false, en), 'Cost per day');
  assert.equal(dailyTitle(false, zh), '每日費用');
  assert.equal(dailyTitle(true, en), 'Cost per month');
  assert.equal(dailyTitle(true, zh), '每月費用');
  assert.match(usage(), /dailyTitle\(monthly, text\)/);
});

test('each chart has a one-sentence summary for screen readers', () => {
  const days = [group('2026-11-20', 4, 0.5), group('2026-11-21', 6, 1.5)];
  assert.equal(dailySummary(days, en), 'Cost on 2 days with usage. Highest: HK$11.70 (US$1.50) on 21 Nov 2026. Total: HK$15.60 (US$2.00).');
  assert.match(dailySummary(fillDays([group('2026-11-18', 4, 0.5), group('2026-11-21', 6, 1.5)]), en), /^Cost on 2 days with usage\./, 'filled days are not counted as days with usage');
  assert.match(dailySummary(days, zh), /^2 個有用量的日子/);
  const split = splitShares([group('mail_sort', 10, 3), group('chat', 3, 1)]);
  assert.equal(splitSummary(split, en), 'Mail sorting 75%, Chat 25%.');
});

test('the main period choices are 30 days, 90 days and 1 year', () => {
  assert.deepEqual(MAIN_PERIODS, [30, 90, 365]);
});

test('the tiles use the honest labels, never "emails sorted"', () => {
  assert.deepEqual(ROWS_BY_ID['usage-mail-requests'].title, ['AI requests for mail', '郵件 AI 請求']);
  assert.deepEqual(ROWS_BY_ID['usage-chat-requests'].title, ['AI requests in chat', '對話 AI 請求']);
  for (const code of [usage(), charts()]) assert.doesNotMatch(code, /emails sorted|questions answered/i);
});

test('the dialog shows Usage through its own component', () => {
  const dialog = source('../src/settings/SettingsDialog.tsx');
  assert.match(dialog, /import Usage from '\.\/sections\/Usage'/);
  assert.match(dialog, /usage: Usage,/);
});

test('Usage renders its rows, in order, by row id', () => {
  const code = usage();
  let last = -1;
  for (const id of ROWS.filter(r => r.section === 'usage').map(r => r.id)) {
    const at = code.indexOf(`row('${id}')`);
    assert.ok(at > last, `row ${id} missing or out of order`);
    last = at;
  }
});

test('Usage uses the shared controls: a Segmented period, no raw select or button', () => {
  const code = usage();
  assert.match(code, /<Segmented /);
  assert.match(code, /MAIN_PERIODS\.map/);
  assert.doesNotMatch(code, /<select|<button/);
  assert.doesNotMatch(charts(), /<select|<button/);
});

test('Usage reads the feature and time groupings and the stats for the chosen period', () => {
  const code = usage();
  assert.match(code, /useTauriCommand<CostStats>\("get_cost_stats", \{ days \}\)/);
  assert.match(code, /useTauriCommand<UsageGroup\[\]>\("get_usage_groups", \{ days, groupBy: "feature" \}\)/);
  assert.match(code, /useTauriCommand<UsageGroup\[\]>\("get_usage_groups", \{ days, groupBy: "time" \}\)/);
  assert.match(code, /unpricedNote\(/);
  assert.match(code, /<DailyBars /);
  assert.match(code, /<SplitBars /);
});

test('with no requests, Usage shows one line and a link to Your AI, not the charts', () => {
  const code = usage();
  const empty = code.slice(code.indexOf('const empty = '), code.indexOf('function figures('));
  assert.match(empty, /emptyPeriodLine\(days, text\)/);
  assert.match(empty, /openSettings\('ai'\)/);
  assert.doesNotMatch(empty, /DailyBars|SplitBars|Tile/);
  assert.match(code, /total_requests === 0 \? empty\s*:/);
});

test('Show details stays mounted whatever the figures are doing, so a period change never collapses it', () => {
  const code = usage();
  // The "what is recorded" line (inventory row 61) sits between the figures and Show details, outside both.
  assert.match(code, /return <>\s*\{period\}\s*\{body\}\s*<p className="ui-hint settings-usage-about">[\s\S]*?<\/p>\s*<details /);
  const component = code.slice(code.indexOf('export default function Usage'), code.indexOf('function Tile'));
  assert.equal((component.match(/^  return /gm) ?? []).length, 1, 'one return at the component level, no early return');
});

test('Show details is the CostPanel as it is, with its own period and every period offered', () => {
  const code = usage();
  assert.match(code, /<details /);
  assert.match(code, /\{detailsOpen && <CostPanel \/>\}/);
  const panel = source('../src/components/CostPanel.tsx');
  assert.match(panel, /export default memo\(function CostPanel\(\) \{/, 'no period props');
  assert.match(panel, /const \[days, setDays\] = useState<PeriodDays>\(30\);/);
  assert.match(panel, /PERIODS\.map\(\(period\) =>/, 'the panel still offers 180 days and 2 years');
});

test('the request tiles show a dash when the features cannot load, not a spinner forever', () => {
  assert.match(usage(), /features\.error \? '–' : features\.data \? count\(n\) : '…'/);
});

test('every chart has an aria-label summary and a visually hidden table', () => {
  const code = charts();
  assert.equal((code.match(/role="img" aria-label=\{/g) ?? []).length, 2);
  assert.equal((code.match(/<div className="visually-hidden"><table>/g) ?? []).length, 2);
  assert.match(code, /barHeights\(/);
  assert.match(source('../src/settings/settings.css'), /\.visually-hidden \{/);
});

test('the chart colours are theme tokens', () => {
  const css = source('../src/settings/settings.css');
  const block = css.slice(css.indexOf('/* Usage (Task 12)'));
  assert.match(block, /background: var\(--ui-focus\)/, 'spend in the accent');
  assert.doesNotMatch(block, /#[0-9a-f]{3,6}\b/i, 'no fixed colours');
});

test('the preview answers the usage commands in a filled and an empty state', () => {
  const preview = source('../src/dev/workspacePreview.ts');
  for (const command of ['get_cost_stats', 'get_blended_cost', 'get_usage_groups']) assert.match(preview, new RegExp(`case '${command}'`));
  assert.match(preview, /usage-empty/);
});

test('Usage zh-HK copy is written Chinese, never colloquial', () => {
  for (const file of ['../src/settings/sections/Usage.tsx', '../src/settings/usage/charts.tsx', '../src/settings/usage-view.ts']) {
    assert.doesNotMatch(source(file), /[嘅咗唔係冇啲喺嚟]/, file);
  }
});
