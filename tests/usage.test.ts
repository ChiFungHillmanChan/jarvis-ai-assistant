import test from 'node:test';
import assert from 'node:assert/strict';
import { formatUsd, monthlyLine, usageHeadline, type BlendedCost, type CostStats } from '../src/lib/usage.ts';

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
  return { total_usd: 0, active_days: 0, implied_monthly_usd: 0, price_unknown_turns: 0, ...change };
}

test('a user whose only AI has no price never sees US$0', () => {
  const kimiOnly = stats({ price_unknown_turns_30d: 12 });
  for (const text of [en, zh]) {
    const headline = usageHeadline(kimiOnly, text);
    assert.doesNotMatch(`${headline.total} ${headline.perRequest}`, /US\$0/);
  }
  assert.deepEqual(usageHeadline(kimiOnly, en), { total: 'Price unknown', perRequest: 'Price unknown', perRequestLabel: 'Per priced request' });
  assert.deepEqual(usageHeadline(kimiOnly, zh), { total: '價格未知', perRequest: '價格未知', perRequestLabel: '每次已定價請求' });
  const monthly = monthlyLine(blended({ price_unknown_turns: 12 }), en);
  assert.equal(monthly, 'Monthly cost: price unknown (12 requests have no price)');
  assert.doesNotMatch(monthly, /US\$0/);
  assert.equal(monthlyLine(blended({ price_unknown_turns: 12 }), zh), '每月費用：價格未知（12 次請求沒有價格）');
});

test('a total that leaves unpriced requests out is a lower bound', () => {
  const mixed = stats({ total_usd_30d: 1.5, avg_usd_per_turn_30d: 0.15, price_unknown_turns_30d: 2 });
  assert.deepEqual(usageHeadline(mixed, en), { total: 'At least US$1.50', perRequest: 'US$0.150', perRequestLabel: 'Per priced request' });
  assert.equal(usageHeadline(mixed, zh).total, '最少 US$1.50');
  const monthly = blended({ total_usd: 1.5, active_days: 3, implied_monthly_usd: 15, price_unknown_turns: 2 });
  assert.equal(monthlyLine(monthly, en), 'At least US$15.00 a month at this rate (3 days with priced usage). 2 requests with no price are not included.');
  assert.equal(monthlyLine(monthly, zh), '按此用量每月最少 US$15.00（3 日有已定價用量），未計入 2 次沒有價格的請求。');
});

test('with every price known the figures are plain amounts', () => {
  const priced = stats({ total_usd_30d: 0.42, avg_usd_per_turn_30d: 0.035 });
  assert.deepEqual(usageHeadline(priced, en), { total: 'US$0.420', perRequest: 'US$0.035', perRequestLabel: 'Per request' });
  assert.equal(monthlyLine(blended({ total_usd: 0.42, active_days: 2, implied_monthly_usd: 6.3 }), en), 'About US$6.30 a month at this rate (2 days with usage)');
  // Local models are free, so US$0 is the true figure there.
  assert.equal(usageHeadline(stats({ local_turns_30d: 12, local_share_pct: 100 }), en).total, 'US$0');
});

test('small amounts keep their digits', () => {
  assert.equal(formatUsd(0), 'US$0');
  assert.equal(formatUsd(0.000105), 'US$0.0001');
  assert.equal(formatUsd(0.5), 'US$0.500');
  assert.equal(formatUsd(12.345), 'US$12.35');
});
