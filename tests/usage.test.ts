import test from 'node:test';
import assert from 'node:assert/strict';
import { expensiveRequests, formatUsd, perDayLine, tokenFigures, unpricedNote, usageHeadline, type BlendedCost, type CostStats, type ExpensiveTurn } from '../src/lib/usage.ts';

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

function stats(change: Partial<CostStats>): CostStats {
  return {
    total_turns_30d: 12,
    total_usd_30d: 0,
    avg_usd_per_turn_30d: 0,
    local_turns_30d: 0,
    local_share_pct: 0,
    price_unknown_turns_30d: 0,
    by_provider_30d: [],
    top_expensive_30d: [],
    ...change,
  };
}

function blended(change: Partial<BlendedCost>): BlendedCost {
  return { total_usd: 0, active_days: 0, usd_per_active_day: 0, price_unknown_turns: 0, ...change };
}

function row(change: Partial<ExpensiveTurn>): ExpensiveTurn {
  return {
    ts: 0, provider: 'bedrock', model_id: 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', workload: 'tools', feature: 'chat',
    input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, usd_cost: 0.001, price_unknown: false, latency_ms: 1,
    ...change,
  };
}

test('a user whose only AI has no price never sees US$0', () => {
  const kimiOnly = stats({ price_unknown_turns_30d: 12 });
  for (const text of [en, zh]) {
    const headline = usageHeadline(kimiOnly, text);
    assert.doesNotMatch(`${headline.total} ${headline.perRequest}`, /US\$0/);
  }
  assert.deepEqual(usageHeadline(kimiOnly, en), { total: 'Price unknown', perRequest: 'Price unknown', perRequestLabel: 'Per priced request' });
  assert.deepEqual(usageHeadline(kimiOnly, zh), { total: '價格未知', perRequest: '價格未知', perRequestLabel: '每次已定價請求' });
  const perDay = perDayLine(blended({ price_unknown_turns: 12 }), 30, en);
  assert.equal(perDay, 'Cost per day of use: price unknown');
  assert.equal(perDayLine(blended({ price_unknown_turns: 12 }), 30, zh), '每個使用日的費用：價格未知');
});

// R1: free local requests beside unpriced ones are not "US$0 per request".
test('local requests beside unpriced ones do not read US$0 per request', () => {
  const localAndUnpriced = stats({ total_turns_30d: 15, local_turns_30d: 10, local_share_pct: 66.7, price_unknown_turns_30d: 5 });
  for (const text of [en, zh]) {
    const headline = usageHeadline(localAndUnpriced, text);
    assert.doesNotMatch(`${headline.total} ${headline.perRequest}`, /US\$0/);
  }
  assert.deepEqual(usageHeadline(localAndUnpriced, en), { total: 'Price unknown', perRequest: 'Price unknown', perRequestLabel: 'Per priced request' });
  assert.equal(perDayLine(blended({ active_days: 4, price_unknown_turns: 5 }), 30, en), 'Cost per day of use: price unknown');
});

test('a total that leaves unpriced requests out is a lower bound', () => {
  const mixed = stats({ total_usd_30d: 1.5, avg_usd_per_turn_30d: 0.15, price_unknown_turns_30d: 2 });
  assert.deepEqual(usageHeadline(mixed, en), { total: 'At least US$1.50', perRequest: 'US$0.150', perRequestLabel: 'Per priced request' });
  assert.equal(usageHeadline(mixed, zh).total, '最少 US$1.50');
});

// R2: what a day of use cost, never a monthly forecast, and no "at least"
// in front of a rate.
test('the line under the headline is the spend per day of use', () => {
  const allPriced = blended({ total_usd: 1.5, active_days: 3, usd_per_active_day: 0.5 });
  assert.equal(perDayLine(allPriced, 30, en), 'About US$0.500 per day of use, 3 days with usage in the last 30');
  assert.equal(perDayLine(allPriced, 30, zh), '每個使用日平均約 US$0.500，最近 30 日內有 3 日有使用');
  assert.equal(perDayLine(blended({ total_usd: 0.42, active_days: 1, usd_per_active_day: 0.42 }), 30, en), 'About US$0.420 per day of use, 1 day with usage in the last 30');

  const someUnpriced = blended({ total_usd: 1.5, active_days: 3, usd_per_active_day: 0.5, price_unknown_turns: 2 });
  assert.equal(perDayLine(someUnpriced, 30, en), 'About US$0.500 per day of use for requests with a known price, 3 days with such usage in the last 30');
  assert.equal(perDayLine(someUnpriced, 30, zh), '已知價格的請求每個使用日平均約 US$0.500，最近 30 日內有 3 日有這類用量');

  for (const line of [perDayLine(allPriced, 30, en), perDayLine(someUnpriced, 30, en), perDayLine(someUnpriced, 30, zh)]) {
    assert.doesNotMatch(line ?? '', /month|月|at least|最少/i);
  }
  assert.equal(perDayLine(blended({}), 30, en), null, 'nothing to average');
  assert.equal(perDayLine(blended({ total_usd: 0.00002, active_days: 2, usd_per_active_day: 0.00001 }), 30, en), 'Under US$0.0001 per day of use, 2 days with usage in the last 30');
});

// R15: the unpriced count is said once, for every amount on the panel.
test('the number of unpriced requests is stated once', () => {
  assert.equal(unpricedNote(stats({}), en), null);
  assert.equal(unpricedNote(stats({ price_unknown_turns_30d: 1 }), en), '1 request has no known price and is in none of the amounts above.');
  assert.equal(unpricedNote(stats({ price_unknown_turns_30d: 12 }), en), '12 requests have no known price and are in none of the amounts above.');
  assert.equal(unpricedNote(stats({ price_unknown_turns_30d: 12 }), zh), '12 次請求價格未知，以上金額均未計入。');
  const someUnpriced = perDayLine(blended({ total_usd: 1.5, active_days: 3, usd_per_active_day: 0.5, price_unknown_turns: 2 }), 30, en) ?? '';
  assert.doesNotMatch(someUnpriced, /\b2\b/, 'the per-day line does not repeat the count');
});

test('with every price known the figures are plain amounts', () => {
  const priced = stats({ total_usd_30d: 0.42, avg_usd_per_turn_30d: 0.035 });
  assert.deepEqual(usageHeadline(priced, en), { total: 'US$0.420', perRequest: 'US$0.035', perRequestLabel: 'Per request' });
  // Local models are free, so US$0 is the true figure there.
  assert.equal(usageHeadline(stats({ local_turns_30d: 12, local_share_pct: 100 }), en).total, 'US$0');
  assert.equal(perDayLine(blended({ active_days: 2 }), 30, en), 'About US$0 per day of use, 2 days with usage in the last 30');
});

// R14: a known amount below four decimals is not printed as zero.
test('small amounts keep their digits and tiny ones do not read as zero', () => {
  assert.equal(formatUsd(0), 'US$0');
  assert.equal(formatUsd(0.0000126), '< US$0.0001', 'one 300-token sort');
  assert.equal(formatUsd(0.0001), 'US$0.0001');
  assert.equal(formatUsd(0.000105), 'US$0.0001');
  assert.equal(formatUsd(0.5), 'US$0.500');
  assert.equal(formatUsd(12.345), 'US$12.35');
  const tinyAndUnpriced = stats({ total_usd_30d: 0.0000126, avg_usd_per_turn_30d: 0.0000126, price_unknown_turns_30d: 2 });
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

// R15: with nothing priced, "Most expensive requests" is not shown.
test('only requests with a known cost are listed as the most expensive', () => {
  const unknown = row({ usd_cost: null, price_unknown: true });
  const free = row({ provider: 'local', model_id: 'qwen3', usd_cost: null });
  assert.deepEqual(expensiveRequests(stats({ top_expensive_30d: [unknown, unknown, free] })), []);
  const priced = row({ usd_cost: 0.0042 });
  assert.deepEqual(expensiveRequests(stats({ top_expensive_30d: [priced, unknown, free] })), [priced]);
});
