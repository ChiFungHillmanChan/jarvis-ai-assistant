import type { Text } from './mail-presentation';

/** One provider's usage over 30 days, from get_cost_stats (src-tauri/src/commands/cost.rs). */
export interface ProviderBreakdown {
  provider: string;
  turns: number;
  /** null for a local model: free, so the tokens are shown instead. */
  usd_cost: number | null;
  input_tokens: number;
  output_tokens: number;
  /** Requests whose model has no price; left out of usd_cost. */
  price_unknown_turns: number;
}

export interface ExpensiveTurn {
  ts: number;
  provider: string;
  model_id: string;
  workload: string;
  feature: string;
  input_tokens: number;
  output_tokens: number;
  /** null for a local model, and when the price is unknown. */
  usd_cost: number | null;
  price_unknown: boolean;
  latency_ms: number;
}

/** get_cost_stats. Each "turn" is one request to a model. */
export interface CostStats {
  total_turns_30d: number;
  /** Over the requests whose price is known. */
  total_usd_30d: number;
  /** Over the requests whose price is known. */
  avg_usd_per_turn_30d: number;
  local_turns_30d: number;
  local_share_pct: number;
  /** Requests left out of the money figures because their price is unknown. */
  price_unknown_turns_30d: number;
  by_provider_30d: ProviderBreakdown[];
  top_expensive_30d: ExpensiveTurn[];
}

/** get_blended_cost. */
export interface BlendedCost {
  /** Over the requests whose price is known. */
  total_usd: number;
  /** Days with usage whose price is known. */
  active_days: number;
  implied_monthly_usd: number;
  /** Requests with no price: the figures above are low by what these cost. */
  price_unknown_turns: number;
}

export function formatUsd(usd: number): string {
  if (usd === 0) return 'US$0';
  if (usd < 0.01) return `US$${usd.toFixed(4)}`;
  if (usd < 1) return `US$${usd.toFixed(3)}`;
  return `US$${usd.toFixed(2)}`;
}

/**
 * The panel's money headline. A request with an unknown price is in no total,
 * so a total that leaves some out reads as a lower bound, and one with nothing
 * priced reads "price unknown": never as US$0.
 */
export function usageHeadline(stats: CostStats, text: Text): { total: string; perRequest: string; perRequestLabel: string } {
  const unknown = stats.price_unknown_turns_30d;
  const priced = stats.total_turns_30d - unknown;
  const priceUnknown = text('Price unknown', '價格未知');
  const total = unknown === 0
    ? formatUsd(stats.total_usd_30d)
    : stats.total_usd_30d > 0
      ? text(`At least ${formatUsd(stats.total_usd_30d)}`, `最少 ${formatUsd(stats.total_usd_30d)}`)
      : priceUnknown;
  return {
    total,
    perRequest: priced > 0 ? formatUsd(stats.avg_usd_per_turn_30d) : priceUnknown,
    perRequestLabel: unknown > 0 ? text('Per priced request', '每次已定價請求') : text('Per request', '每次請求'),
  };
}

/** The monthly rate line under the headline, from get_blended_cost. */
export function monthlyLine(blended: BlendedCost, text: Text): string {
  const unknown = blended.price_unknown_turns;
  const usd = formatUsd(blended.implied_monthly_usd);
  const days = blended.active_days;
  if (unknown === 0) {
    return text(`About ${usd} a month at this rate (${days} days with usage)`, `按此用量約每月 ${usd}（${days} 日有使用）`);
  }
  if (blended.total_usd === 0) {
    return text(`Monthly cost: price unknown (${unknown} requests have no price)`, `每月費用：價格未知（${unknown} 次請求沒有價格）`);
  }
  return text(
    `At least ${usd} a month at this rate (${days} days with priced usage). ${unknown} requests with no price are not included.`,
    `按此用量每月最少 ${usd}（${days} 日有已定價用量），未計入 ${unknown} 次沒有價格的請求。`,
  );
}
