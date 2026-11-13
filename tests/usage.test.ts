import test from 'node:test';
import assert from 'node:assert/strict';
import {
  PERIODS, byMonth, costLabel, emptyPeriodLine, expensiveHeading, expensiveRequests, featureLabel, formatUsd, groupByLabel, groupColumnLabel,
  groupRows, historyLine, perDayLine, periodLabel, providerLabel, timeLabel, tokenFigures, unpricedNote, usageHeadline,
  type BlendedCost, type CostStats, type ExpensiveTurn, type UsageGroup,
} from '../src/lib/usage.ts';

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

function stats(change: Partial<CostStats>): CostStats {
  return {
    days: 30,
    total_requests: 12,
    total_usd: 0,
    avg_usd_per_request: 0,
    local_requests: 0,
    local_share_pct: 0,
    price_unknown_requests: 0,
    expensive_days: 30,
    top_expensive: [],
    ...change,
  };
}

function blended(change: Partial<BlendedCost>): BlendedCost {
  return { total_usd: 0, active_days: 0, usd_per_active_day: 0, price_unknown_requests: 0, ...change };
}

function row(change: Partial<ExpensiveTurn>): ExpensiveTurn {
  return {
    ts: 0, provider: 'bedrock', model_id: 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', workload: 'tools', feature: 'chat',
    input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, usd_cost: 0.001, price_unknown: false, latency_ms: 1,
    ...change,
  };
}

test('a user whose only AI has no price never sees US$0', () => {
  const kimiOnly = stats({ price_unknown_requests: 12 });
  for (const text of [en, zh]) {
    const headline = usageHeadline(kimiOnly, text);
    assert.doesNotMatch(`${headline.total} ${headline.perRequest}`, /US\$0/);
  }
  assert.deepEqual(usageHeadline(kimiOnly, en), { total: 'Price unknown', perRequest: 'Price unknown', perRequestLabel: 'Per priced request' });
  assert.deepEqual(usageHeadline(kimiOnly, zh), { total: '價格未知', perRequest: '價格未知', perRequestLabel: '每次已定價請求' });
  const perDay = perDayLine(blended({ price_unknown_requests: 12 }), 30, en);
  assert.equal(perDay, 'Cost per day of use: price unknown');
  assert.equal(perDayLine(blended({ price_unknown_requests: 12 }), 30, zh), '每個使用日的費用：價格未知');
});

// R1: free local requests beside unpriced ones are not "US$0 per request".
test('local requests beside unpriced ones do not read US$0 per request', () => {
  const localAndUnpriced = stats({ total_requests: 15, local_requests: 10, local_share_pct: 66.7, price_unknown_requests: 5 });
  for (const text of [en, zh]) {
    const headline = usageHeadline(localAndUnpriced, text);
    assert.doesNotMatch(`${headline.total} ${headline.perRequest}`, /US\$0/);
  }
  assert.deepEqual(usageHeadline(localAndUnpriced, en), { total: 'Price unknown', perRequest: 'Price unknown', perRequestLabel: 'Per priced request' });
  assert.equal(perDayLine(blended({ active_days: 4, price_unknown_requests: 5 }), 30, en), 'Cost per day of use: price unknown');
});

test('a total that leaves unpriced requests out is a lower bound', () => {
  const mixed = stats({ total_usd: 1.5, avg_usd_per_request: 0.15, price_unknown_requests: 2 });
  assert.deepEqual(usageHeadline(mixed, en), { total: 'At least US$1.50', perRequest: 'US$0.150', perRequestLabel: 'Per priced request' });
  assert.equal(usageHeadline(mixed, zh).total, '最少 US$1.50');
});

// R2: what a day of use cost, never a monthly forecast, and no "at least"
// in front of a rate.
test('the line under the headline is the spend per day of use', () => {
  const allPriced = blended({ total_usd: 1.5, active_days: 3, usd_per_active_day: 0.5 });
  assert.equal(perDayLine(allPriced, 30, en), 'About US$0.500 per day of use, 3 days with usage in the last 30 days');
  assert.equal(perDayLine(allPriced, 30, zh), '每個使用日平均約 US$0.500，最近 30 日內有 3 日有使用');
  assert.equal(perDayLine(blended({ total_usd: 0.42, active_days: 1, usd_per_active_day: 0.42 }), 30, en), 'About US$0.420 per day of use, 1 day with usage in the last 30 days');

  const someUnpriced = blended({ total_usd: 1.5, active_days: 3, usd_per_active_day: 0.5, price_unknown_requests: 2 });
  assert.equal(perDayLine(someUnpriced, 30, en), 'About US$0.500 per day of use for requests with a known price, 3 days with such usage in the last 30 days');
  assert.equal(perDayLine(someUnpriced, 30, zh), '已知價格的請求每個使用日平均約 US$0.500，最近 30 日內有 3 日有這類用量');

  for (const line of [perDayLine(allPriced, 30, en), perDayLine(someUnpriced, 30, en), perDayLine(someUnpriced, 30, zh)]) {
    assert.doesNotMatch(line ?? '', /month|月|at least|最少/i);
  }
  assert.equal(perDayLine(blended({}), 30, en), null, 'nothing to average');
  assert.equal(perDayLine(blended({ total_usd: 0.00002, active_days: 2, usd_per_active_day: 0.00001 }), 30, en), 'Under US$0.0001 per day of use, 2 days with usage in the last 30 days');
});

// R15: the unpriced count is said once, for every amount on the panel.
test('the number of unpriced requests is stated once', () => {
  assert.equal(unpricedNote(stats({}), en), null);
  assert.equal(unpricedNote(stats({ price_unknown_requests: 1 }), en), '1 request has no known price and is in none of the amounts above.');
  assert.equal(unpricedNote(stats({ price_unknown_requests: 12 }), en), '12 requests have no known price and are in none of the amounts above.');
  assert.equal(unpricedNote(stats({ price_unknown_requests: 12 }), zh), '12 次請求價格未知，以上金額均未計入。');
  const someUnpriced = perDayLine(blended({ total_usd: 1.5, active_days: 3, usd_per_active_day: 0.5, price_unknown_requests: 2 }), 30, en) ?? '';
  assert.doesNotMatch(someUnpriced, /\b2\b/, 'the per-day line does not repeat the count');
});

test('with every price known the figures are plain amounts', () => {
  const priced = stats({ total_usd: 0.42, avg_usd_per_request: 0.035 });
  assert.deepEqual(usageHeadline(priced, en), { total: 'US$0.420', perRequest: 'US$0.035', perRequestLabel: 'Per request' });
  // Local models are free, so US$0 is the true figure there.
  assert.equal(usageHeadline(stats({ local_requests: 12, local_share_pct: 100 }), en).total, 'US$0');
  assert.equal(perDayLine(blended({ active_days: 2 }), 30, en), 'About US$0 per day of use, 2 days with usage in the last 30 days');
});

// R14: a known amount below four decimals is not printed as zero.
test('small amounts keep their digits and tiny ones do not read as zero', () => {
  assert.equal(formatUsd(0), 'US$0');
  assert.equal(formatUsd(0.0000126), '< US$0.0001', 'one 300-token sort');
  assert.equal(formatUsd(0.0001), 'US$0.0001');
  assert.equal(formatUsd(0.000105), 'US$0.0001');
  assert.equal(formatUsd(0.5), 'US$0.500');
  assert.equal(formatUsd(12.345), 'US$12.35');
  const tinyAndUnpriced = stats({ total_usd: 0.0000126, avg_usd_per_request: 0.0000126, price_unknown_requests: 2 });
  assert.equal(usageHeadline(tinyAndUnpriced, en).total, 'More than US$0');
  assert.equal(usageHeadline(tinyAndUnpriced, zh).total, '多於 US$0');
  assert.equal(usageHeadline(tinyAndUnpriced, en).perRequest, '< US$0.0001');
});

// R4: a row reads the way AWS reports it. AWS returned inputTokens 120,
// outputTokens 45, cacheReadInputTokens 800, cacheWriteInputTokens 400; the
// row stores input 1,320 (inclusive, as for every provider).
test('token figures show input without the cache, as AWS reports inputTokens', () => {
  const aws = { input_tokens: 1_320, output_tokens: 45, cache_read_tokens: 800, cache_write_tokens: 400 };
  assert.equal(tokenFigures(aws, en), '120 in · 45 out · 800 cache read · 400 cache write tokens');
  assert.equal(tokenFigures(aws, zh), 'token：輸入 120 · 輸出 45 · 快取讀取 800 · 快取寫入 400');
  assert.equal(tokenFigures({ input_tokens: 9, output_tokens: 2, cache_read_tokens: 0, cache_write_tokens: 0 }, en), '9 in · 2 out tokens', 'no cache, no cache figures');
  assert.equal(tokenFigures({ input_tokens: 12_000, output_tokens: 1_250, cache_read_tokens: 2_000, cache_write_tokens: 0 }, en), '10,000 in · 1,250 out · 2,000 cache read tokens');
  assert.doesNotMatch(tokenFigures(aws, zh), /tokens/, 'the Chinese form has no English plural');
});

// N2: v0.5.10 stored the same Bedrock request as input 120, without the
// cache. Input below the cache cannot include it, so it is shown as stored.
test('a row stored with less input than its cache shows its input as stored', () => {
  const old = { input_tokens: 120, output_tokens: 45, cache_read_tokens: 800, cache_write_tokens: 400 };
  assert.equal(tokenFigures(old, en), '120 in · 45 out · 800 cache read · 400 cache write tokens');
  assert.equal(tokenFigures(old, zh), 'token：輸入 120 · 輸出 45 · 快取讀取 800 · 快取寫入 400');
  assert.equal(tokenFigures({ input_tokens: 1_200, output_tokens: 1, cache_read_tokens: 800, cache_write_tokens: 400 }, en), '0 in · 1 out · 800 cache read · 400 cache write tokens', 'all of the input was cache');
});

// N5: MiniMax speech is billed per character; the characters are never shown as tokens.
test('speech characters are shown as characters, never as tokens', () => {
  const speech = groupRows([group({ key: 'speech-02-hd', provider: 'minimax', requests: 2, speech_characters: 2_468, price_unknown_requests: 2 })], 'model', en)[0];
  assert.deepEqual(
    [speech.input, speech.output, speech.cacheRead, speech.cacheWrite, speech.characters, speech.cost],
    ['', '', '', '', '2,468 characters of speech', 'Price unknown'],
  );
  const day = groupRows([group({ key: '2026-11-12', provider: null, requests: 3, input_tokens: 1_700, output_tokens: 40, speech_characters: 1_234, usd_cost: 0.0001, price_unknown_requests: 2 })], 'time', zh)[0];
  assert.deepEqual([day.input, day.output, day.characters], ['1,700', '40', '語音 1,234 個字元'], 'a day of chat and speech: tokens for the chat, characters for the speech');
  assert.equal(groupRows([group({})], 'model', en)[0].characters, null);
});

// R15: with nothing priced, "Most expensive requests" is not shown.
test('only requests with a known cost are listed as the most expensive', () => {
  const unknown = row({ usd_cost: null, price_unknown: true });
  const free = row({ provider: 'local', model_id: 'qwen3', usd_cost: null });
  assert.deepEqual(expensiveRequests(stats({ top_expensive: [unknown, unknown, free] })), []);
  const priced = row({ usd_cost: 0.0042 });
  assert.deepEqual(expensiveRequests(stats({ top_expensive: [priced, unknown, free] })), [priced]);
});

// Usage history: the periods, the group-by control and the table.

function group(change: Partial<UsageGroup>): UsageGroup {
  return {
    key: 'gpt-6-luna', provider: 'openai', requests: 1, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0,
    usd_cost: 0, price_unknown_requests: 0, speech_characters: 0, ...change,
  };
}

test('the five periods are named the same way in the selector and the sentences', () => {
  assert.deepEqual([...PERIODS], [30, 90, 180, 365, 730]);
  assert.deepEqual(PERIODS.map(days => periodLabel(days, en)), ['30 days', '90 days', '6 months', '1 year', '2 years']);
  assert.deepEqual(PERIODS.map(days => periodLabel(days, zh)), ['30 日', '90 日', '6 個月', '1 年', '2 年']);
  assert.equal(emptyPeriodLine(180, en), 'No usage recorded on this Mac in the last 6 months.');
  assert.equal(emptyPeriodLine(730, zh), '最近 2 年此 Mac 未有記錄任何用量。');
});

test('the headline and the per-day line follow the chosen period, in both languages', () => {
  const year = stats({ days: 365, total_requests: 400, total_usd: 12.5, avg_usd_per_request: 12.5 / 380, price_unknown_requests: 20 });
  assert.deepEqual(usageHeadline(year, en), { total: 'At least US$12.50', perRequest: 'US$0.033', perRequestLabel: 'Per priced request' });
  assert.equal(usageHeadline(year, zh).total, '最少 US$12.50');
  assert.equal(unpricedNote(year, en), '20 requests have no known price and are in none of the amounts above.');
  const twoYears = blended({ total_usd: 12.5, active_days: 50, usd_per_active_day: 0.25, price_unknown_requests: 20 });
  assert.equal(perDayLine(twoYears, 730, en), 'About US$0.250 per day of use for requests with a known price, 50 days with such usage in the last 2 years');
  assert.equal(perDayLine(twoYears, 730, zh), '已知價格的請求每個使用日平均約 US$0.250，最近 2 年內有 50 日有這類用量');
  const halfYear = blended({ total_usd: 3, active_days: 12, usd_per_active_day: 0.25 });
  assert.equal(perDayLine(halfYear, 180, en), 'About US$0.250 per day of use, 12 days with usage in the last 6 months');
  assert.equal(perDayLine(halfYear, 180, zh), '每個使用日平均約 US$0.250，最近 6 個月內有 12 日有使用');
  const unknownOnly = stats({ days: 90, total_requests: 7, price_unknown_requests: 7 });
  for (const text of [en, zh]) {
    const headline = usageHeadline(unknownOnly, text);
    assert.doesNotMatch(`${headline.total} ${headline.perRequest}`, /US\$0/);
  }
});

test('the group-by control and the first column are named in both languages', () => {
  assert.deepEqual((['model', 'provider', 'feature', 'time'] as const).map(by => groupByLabel(by, en)), ['Model', 'Provider', 'Feature', 'Time']);
  assert.deepEqual((['model', 'provider', 'feature', 'time'] as const).map(by => groupByLabel(by, zh)), ['模型', '供應商', '功能', '時間']);
  assert.deepEqual(PERIODS.map(days => byMonth(days)), [false, false, true, true, true], 'by day up to 90 days, by month beyond');
  assert.equal(groupColumnLabel('time', 90, en), 'Day');
  assert.equal(groupColumnLabel('time', 180, en), 'Month');
  assert.equal(groupColumnLabel('time', 30, zh), '日期');
  assert.equal(groupColumnLabel('time', 730, zh), '月份');
  assert.equal(groupColumnLabel('feature', 30, zh), '功能');
});

test('providers, features and days read as people know them', () => {
  assert.equal(providerLabel('bedrock', en), 'AWS Bedrock');
  assert.equal(providerLabel('compat:api.moonshot.ai', en), 'api.moonshot.ai');
  assert.equal(providerLabel('local', zh), '本機模型');
  assert.equal(providerLabel('unknown', en), 'Not recorded');
  assert.equal(providerLabel('somewhere-new', en), 'somewhere-new');
  assert.equal(featureLabel('mail_sort', en), 'Mail sorting');
  assert.equal(featureLabel('tools', zh), '使用工具的對話');
  assert.equal(featureLabel('other', en), 'Other or not recorded');
  assert.equal(timeLabel('2026-11-03', en), '3 Nov 2026');
  assert.equal(timeLabel('2026-11-03', zh), '2026年11月3日');
  assert.equal(timeLabel('2026-09', en), 'Sep 2026');
  assert.equal(timeLabel('2026-09', zh), '2026年9月');
});

// AWS returned inputTokens 120, outputTokens 45, cacheReadInputTokens 800,
// cacheWriteInputTokens 400 for one request; the backend hands the group's
// input over without the cache.
test('a group with no priced request reads price unknown, one with some reads at least', () => {
  const groups = [
    group({ key: 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', provider: 'bedrock', requests: 1, input_tokens: 120, output_tokens: 45, cache_read_tokens: 800, cache_write_tokens: 400, usd_cost: 0.0009 }),
    group({ key: 'gpt-6-luna', provider: 'openai', requests: 3, input_tokens: 2_500, output_tokens: 30, usd_cost: 1.25, price_unknown_requests: 1 }),
    group({ key: 'kimi-k2-0905-preview', provider: 'compat:api.moonshot.ai', requests: 4, input_tokens: 12_000, output_tokens: 800, price_unknown_requests: 4 }),
    group({ key: 'qwen3:4b', provider: 'local', requests: 2, input_tokens: 8_000, output_tokens: 1_800 }),
  ];
  const rows = groupRows(groups, 'model', en);
  assert.deepEqual(rows[0], {
    id: 'bedrock|apac.anthropic.claude-haiku-4-5-20251001-v1:0', name: 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', detail: 'AWS Bedrock',
    requests: '1', input: '120', output: '45', cacheRead: '800', cacheWrite: '400', characters: null, cost: 'US$0.0009',
  });
  assert.deepEqual(rows.map(row => row.cost), ['US$0.0009', 'At least US$1.25', 'Price unknown', 'US$0']);
  assert.deepEqual(groupRows(groups, 'model', zh).map(row => row.cost), ['US$0.0009', '最少 US$1.25', '價格未知', 'US$0']);
  assert.equal(rows[2].input, '12,000');
  assert.equal(rows[3].detail, 'Local model');

  const byFeature = groupRows([group({ key: 'mail_analysis', provider: null, usd_cost: 0.00002, price_unknown_requests: 2, requests: 5 })], 'feature', zh);
  assert.deepEqual([byFeature[0].name, byFeature[0].detail, byFeature[0].cost], ['郵件分析', null, '多於 US$0']);
  const byProvider = groupRows([group({ key: 'compat:api.moonshot.ai', provider: null, price_unknown_requests: 1 })], 'provider', en);
  assert.deepEqual([byProvider[0].name, byProvider[0].cost], ['api.moonshot.ai', 'Price unknown']);
  const byMonthRows = groupRows([group({ key: '2026-11', provider: null, usd_cost: 3 }), group({ key: '2026-10', provider: null, usd_cost: 9 })], 'time', en);
  assert.deepEqual(byMonthRows.map(row => row.name), ['Nov 2026', 'Oct 2026'], 'the order the backend gives: newest first');
  assert.equal(costLabel(0, 0, en), 'US$0', 'everything priced at zero is free, not unknown');
});

test('the history line and the expensive heading say how many days they cover', () => {
  assert.equal(historyLine(30, en), 'Each request is kept for the last 30 days, then as daily totals for two years, only on this Mac. Days are counted in UTC.');
  assert.equal(historyLine(7, zh), '每次請求的記錄保留最近 7 日，之後以每日總數保留兩年，全部只存於此 Mac。日子按 UTC 計算。');
  assert.equal(expensiveHeading(30, en), 'Most expensive requests, last 30 days');
  assert.equal(expensiveHeading(14, zh), '最近 14 日費用最高的請求');
  for (const line of [historyLine(30, en), historyLine(30, zh), expensiveHeading(30, en)]) {
    assert.doesNotMatch(line, /\p{Extended_Pictographic}/u);
  }
});
