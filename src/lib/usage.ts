import type { Text } from './mail-presentation';

/** One provider's usage over 30 days, from get_cost_stats (src-tauri/src/commands/cost.rs). */
export interface ProviderBreakdown {
  provider: string;
  /** Stored rows; what one row is depends on the provider (see CostStats). */
  turns: number;
  /** null for a local model: free, so the tokens are shown instead. */
  usd_cost: number | null;
  /** Includes the cache reads and writes, as stored for every provider. */
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  /** Rows whose model has no price; left out of usd_cost. */
  price_unknown_turns: number;
}

export interface ExpensiveTurn {
  ts: number;
  provider: string;
  model_id: string;
  workload: string;
  feature: string;
  /** Includes the cache reads and writes, as stored for every provider. */
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  /** null for a local model, and when the price is unknown. */
  usd_cost: number | null;
  price_unknown: boolean;
  latency_ms: number;
}

/**
 * get_cost_stats. Each "turn" is one stored row, and what a row is depends
 * on the provider. On Gemini, Claude, OpenAI-compatible services and local
 * models one chat turn is one row, however many tool rounds it took. On
 * Bedrock each request is a row, so a chat turn that runs a tool is two or
 * more rows. Mail sorting, mail analysis and the other single completions
 * are one row per request on every provider, and speech is one row per call.
 * The panel counts rows as "Requests": a known limit until one provider
 * interface records them the same way (P07).
 */
export interface CostStats {
  total_turns_30d: number;
  /** Over the rows whose price is known. */
  total_usd_30d: number;
  /** Over the rows whose price is known. */
  avg_usd_per_turn_30d: number;
  local_turns_30d: number;
  local_share_pct: number;
  /** Rows left out of the money figures because their price is unknown. */
  price_unknown_turns_30d: number;
  by_provider_30d: ProviderBreakdown[];
  top_expensive_30d: ExpensiveTurn[];
}

/** get_blended_cost. */
export interface BlendedCost {
  /** Over the rows whose price is known. */
  total_usd: number;
  /** Days with usage whose price is known. */
  active_days: number;
  /** total_usd over active_days. Not a forecast of a bill. */
  usd_per_active_day: number;
  /** Rows with no price: the figures above leave out what these cost. */
  price_unknown_turns: number;
}

export function formatUsd(usd: number): string {
  if (usd === 0) return 'US$0';
  // A known amount too small for four decimals is still not nothing.
  if (usd < 0.0001) return '< US$0.0001';
  if (usd < 0.01) return `US$${usd.toFixed(4)}`;
  if (usd < 1) return `US$${usd.toFixed(3)}`;
  return `US$${usd.toFixed(2)}`;
}

/**
 * The panel's money headline. A request with an unknown price is in no total,
 * so a total that leaves some out reads as a lower bound, and one with nothing
 * priced reads "price unknown": never as US$0. The same holds for the amount
 * per request, so free local requests beside unpriced ones do not read US$0.
 */
export function usageHeadline(stats: CostStats, text: Text): { total: string; perRequest: string; perRequestLabel: string } {
  const unknown = stats.price_unknown_turns_30d;
  const priced = stats.total_turns_30d - unknown;
  const priceUnknown = text('Price unknown', '價格未知');
  const total = unknown === 0
    ? formatUsd(stats.total_usd_30d)
    : stats.total_usd_30d > 0
      ? stats.total_usd_30d < 0.0001
        ? text('More than US$0', '多於 US$0')
        : text(`At least ${formatUsd(stats.total_usd_30d)}`, `最少 ${formatUsd(stats.total_usd_30d)}`)
      : priceUnknown;
  return {
    total,
    perRequest: priced > 0 && (unknown === 0 || stats.total_usd_30d > 0) ? formatUsd(stats.avg_usd_per_turn_30d) : priceUnknown,
    perRequestLabel: unknown > 0 ? text('Per priced request', '每次已定價請求') : text('Per request', '每次請求'),
  };
}

/**
 * The line under the headline, from get_blended_cost: what a day of use
 * cost on average over the window. It says nothing about a month or a bill.
 * Null when there is nothing to average.
 */
export function perDayLine(blended: BlendedCost, windowDays: number, text: Text): string | null {
  const unknown = blended.price_unknown_turns;
  const days = blended.active_days;
  if (unknown > 0 && blended.total_usd === 0) {
    return text('Cost per day of use: price unknown', '每個使用日的費用：價格未知');
  }
  if (days === 0) return null;
  const perDay = blended.usd_per_active_day;
  const amount = perDay > 0 && perDay < 0.0001
    ? text('Under US$0.0001', '少於 US$0.0001')
    : text(`About ${formatUsd(perDay)}`, `約 ${formatUsd(perDay)}`);
  const dayCount = days === 1 ? '1 day' : `${days} days`;
  if (unknown === 0) {
    return text(
      `${amount} per day of use, ${dayCount} with usage in the last ${windowDays}`,
      `每個使用日平均${amount}，最近 ${windowDays} 日內有 ${days} 日有使用`,
    );
  }
  return text(
    `${amount} per day of use for requests with a known price, ${dayCount} with such usage in the last ${windowDays}`,
    `已知價格的請求每個使用日平均${amount}，最近 ${windowDays} 日內有 ${days} 日有這類用量`,
  );
}

/** How many requests had no price, said once for every amount on the panel. */
export function unpricedNote(stats: CostStats, text: Text): string | null {
  const count = stats.price_unknown_turns_30d;
  if (count === 0) return null;
  return text(
    count === 1 ? '1 request has no known price and is in none of the amounts above.' : `${count} requests have no known price and are in none of the amounts above.`,
    `${count} 次請求價格未知，以上金額均未計入。`,
  );
}

/**
 * A row's tokens the way the provider reports them: input without the cache
 * (AWS's inputTokens), output, then the cache reads and writes when there are
 * any. Stored input includes the cache for every provider, so the cache is
 * taken out here; the stored figures do not change.
 */
export function tokenFigures(
  row: { input_tokens: number; output_tokens: number; cache_read_tokens: number; cache_write_tokens: number },
  text: Text,
): string {
  const count = (tokens: number) => tokens.toLocaleString('en-US');
  const read = row.cache_read_tokens ?? 0;
  const write = row.cache_write_tokens ?? 0;
  const input = Math.max(0, row.input_tokens - read - write);
  const parts = [
    text(`${count(input)} in`, `輸入 ${count(input)}`),
    text(`${count(row.output_tokens)} out`, `輸出 ${count(row.output_tokens)}`),
  ];
  if (read > 0) parts.push(text(`${count(read)} cache read`, `快取讀取 ${count(read)}`));
  if (write > 0) parts.push(text(`${count(write)} cache write`, `快取寫入 ${count(write)}`));
  const figures = parts.join(' · ');
  return text(`${figures} tokens`, `token：${figures}`);
}

/**
 * The rows "Most expensive requests" lists: those with a known cost above
 * zero. Rows with no price, or free local ones, would only fill the list, so
 * with nothing priced the list is not shown.
 */
export function expensiveRequests(stats: CostStats): ExpensiveTurn[] {
  return stats.top_expensive_30d.filter(row => !row.price_unknown && row.usd_cost !== null && row.usd_cost > 0);
}
