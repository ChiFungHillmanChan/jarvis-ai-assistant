import type { Text } from '../lib/mail-presentation';
import { featureLabel, formatUsd, timeLabel, type CostStats, type PeriodDays, type UsageGroup } from '../lib/usage.ts';

// Settings > Usage: the figures the tiles and charts show, from the same
// commands as the detail table (lib/usage.ts). Money is HK$ first, then US$.

/** A fixed rate, so the same usage always reads the same HK$ amount. */
export const HKD_PER_USD = 7.8;

/** The periods the main Segmented offers; 180 days and 2 years stay in Show details. */
export const MAIN_PERIODS: PeriodDays[] = [30, 90, 365];

export function toHkd(usd: number): number {
  return usd * HKD_PER_USD;
}

function formatHkd(usd: number): string {
  const hkd = toHkd(usd);
  if (hkd === 0) return 'HK$0';
  // A known amount too small to show is still not nothing.
  if (hkd < 0.01) return '< HK$0.01';
  return `HK$${hkd.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

/** "HK$18.80 (US$2.41)": every amount on the page reads this way. */
export function formatMoney(usd: number): string {
  return `${formatHkd(usd)} (${formatUsd(usd)})`;
}

/**
 * The Spent tile, split so the HK$ can be large and the US$ small beside it.
 * Requests with no price are in no amount: with some, the amount is a lower
 * bound, and with nothing priced it reads "Price unknown", never HK$0.
 */
export function spentFigure(usd: number, unpriced: number, text: Text): { value: string; usd: string | null } {
  if (unpriced > 0 && usd === 0) return { value: text('Price unknown', '價格未知'), usd: null };
  const hkd = formatHkd(usd);
  return { value: unpriced > 0 ? text(`At least ${hkd}`, `最少 ${hkd}`) : hkd, usd: `(${formatUsd(usd)})` };
}

function requestsFor(byFeature: UsageGroup[], features: string[]): number {
  return byFeature.filter(g => features.includes(g.key)).reduce((sum, g) => sum + g.requests, 0);
}

/**
 * The three tiles. Requests, not emails: sorting one email can take two
 * requests, so the mail figure is never called "emails sorted".
 */
export function usageTiles(byFeature: UsageGroup[], stats: CostStats): { spentUsd: number; mailRequests: number; chatRequests: number; unpriced: number } {
  return {
    spentUsd: stats.total_usd,
    mailRequests: requestsFor(byFeature, ['mail_sort']),
    chatRequests: requestsFor(byFeature, ['chat', 'tools']),
    unpriced: stats.price_unknown_requests,
  };
}

/** Bar heights in px: 0 stays 0, the largest is maxPx, any other value at least 2 px so it can be seen. */
export function barHeights(values: number[], maxPx: number): number[] {
  const max = Math.max(0, ...values);
  if (max === 0) return values.map(() => 0);
  return values.map(v => (v > 0 ? Math.max(2, Math.round((v / max) * maxPx)) : 0));
}

/** Each feature's part of the known cost, largest first. With no cost every share is 0. */
export function splitShares(byFeature: UsageGroup[]): { feature: string; usd: number; share: number }[] {
  const total = byFeature.reduce((sum, g) => sum + g.usd_cost, 0);
  return byFeature
    .map(g => ({ feature: g.key, usd: g.usd_cost, share: total > 0 ? g.usd_cost / total : 0 }))
    .sort((a, b) => b.usd - a.usd);
}

/** The time grouping arrives newest first (ai/usage.rs); a chart reads oldest first. */
export function chronological(groups: UsageGroup[]): UsageGroup[] {
  return [...groups].sort((a, b) => a.key.localeCompare(b.key));
}

const KEY = /^\d{4}-\d{2}(-\d{2})?$/;

function emptyGroup(key: string): UsageGroup {
  return { key, provider: null, requests: 0, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, usd_cost: 0, price_unknown_requests: 0, speech_characters: 0, from_day: null };
}

/** The UTC day ("2026-11-13") or month ("2026-11") after `key`. */
function nextKey(key: string): string {
  const [year, month, day] = key.split('-').map(Number);
  if (day) return new Date(Date.UTC(year, month - 1, day + 1)).toISOString().slice(0, 10);
  return month === 12 ? `${year + 1}-01` : `${year}-${String(month + 1).padStart(2, '0')}`;
}

/**
 * The time grouping, oldest first, with every UTC day (or month) between the
 * first and the last given as a zero bar, so the axis never skips. The backend
 * returns only the days with usage.
 */
export function fillDays(points: UsageGroup[]): UsageGroup[] {
  const sorted = chronological(points);
  if (sorted.length < 2) return sorted;
  const first = sorted[0].key;
  const last = sorted[sorted.length - 1].key;
  // Mixed or unexpected keys are drawn as given rather than guessed at.
  if (!sorted.every(p => KEY.test(p.key) && p.key.length === first.length)) return sorted;
  const byKey = new Map(sorted.map(p => [p.key, p]));
  const filled: UsageGroup[] = [];
  // Two years of days at most (PERIODS); the bound only guards the loop.
  for (let key = first; filled.length <= 800; key = nextKey(key)) {
    filled.push(byKey.get(key) ?? emptyGroup(key));
    if (key >= last) break;
  }
  return filled;
}

/** The chart's heading: its bars are days up to 90 days and months beyond (lib/usage.ts byMonth). */
export function dailyTitle(monthly: boolean, text: Text): string {
  return monthly ? text('Cost per month', '每月費用') : text('Cost per day', '每日費用');
}

/** The daily (or, past 90 days, monthly) chart in one sentence, for its aria-label. Filled days without usage are not counted. */
export function dailySummary(points: UsageGroup[], text: Text, monthly = false): string {
  const n = points.filter(p => p.requests > 0).length;
  const span = monthly ? text(n === 1 ? 'month' : 'months', '月份') : text(n === 1 ? 'day' : 'days', '日子');
  const total = points.reduce((sum, p) => sum + p.usd_cost, 0);
  if (total === 0) return text(`${n} ${span} with usage, none with a known cost.`, `${n} 個有用量的${span}，均沒有已知費用。`);
  const top = points.reduce((a, b) => (b.usd_cost > a.usd_cost ? b : a));
  const when = timeLabel(top.key, text, top.from_day);
  return text(
    `Cost on ${n} ${span} with usage. Highest: ${formatMoney(top.usd_cost)} on ${when}. Total: ${formatMoney(total)}.`,
    `${n} 個有用量的${span}。最高：${when}，${formatMoney(top.usd_cost)}。總額：${formatMoney(total)}。`,
  );
}

/** The "Where it went" chart in one sentence, for its aria-label. */
export function splitSummary(shares: { feature: string; share: number; usd: number }[], text: Text): string {
  const parts = shares.filter(s => s.usd > 0).map(s => `${featureLabel(s.feature, text)} ${Math.round(s.share * 100)}%`);
  if (parts.length === 0) return text('No cost in this period.', '此期間沒有費用。');
  return text(`${parts.join(', ')}.`, `${parts.join('，')}。`);
}
