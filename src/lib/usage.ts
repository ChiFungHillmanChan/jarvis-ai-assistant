import type { Text } from './mail-presentation';

/** One request among the most expensive, from get_cost_stats (src-tauri/src/commands/cost.rs). */
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
  /**
   * Always a number: the backend lists only requests whose price is known
   * and above zero (src-tauri/src/commands/cost.rs), so a local or unpriced
   * request is never here.
   */
  usd_cost: number | null;
  price_unknown: boolean;
  latency_ms: number;
}

/**
 * get_cost_stats for one period. Each request is one stored row, and what a
 * row is depends on the provider. On Gemini, Claude, OpenAI-compatible
 * services and local models one chat turn is one row, however many tool
 * rounds it took. On Bedrock each request is a row, so a chat turn that runs
 * a tool is two or more rows. Mail sorting, mail analysis and the other
 * single completions are one row per request on every provider, and speech
 * is one row per call. The panel counts rows as "Requests": a known limit
 * until one provider interface records them the same way (P07). Rows older
 * than the per-request window are counted from the daily totals they were
 * moved into (src-tauri/src/ai/usage.rs).
 */
export interface CostStats {
  /** The period: the last `days` UTC days, today included. */
  days: number;
  total_requests: number;
  /** Over the requests whose price is known. */
  total_usd: number;
  /** Over the requests whose price is known. */
  avg_usd_per_request: number;
  local_requests: number;
  local_share_pct: number;
  /** Requests left out of the money figures because their price is unknown. */
  price_unknown_requests: number;
  /** How long each request keeps its own row on this Mac now, in days. */
  per_request_days: number;
  /**
   * The UTC days the most expensive requests cover, today included: the
   * per-request window, or fewer when some of its days are already daily
   * totals (a clock that was ahead moved them early).
   */
  expensive_days: number;
  top_expensive: ExpensiveTurn[];
}

/** get_blended_cost for one period. */
export interface BlendedCost {
  /** Over the requests whose price is known. */
  total_usd: number;
  /** UTC days with usage whose price is known. */
  active_days: number;
  /** total_usd over active_days. Not a forecast of a bill. */
  usd_per_active_day: number;
  /** Requests with no price: the figures above leave out what these cost. */
  price_unknown_requests: number;
}

/** One row of the usage table, from get_usage_groups. */
export interface UsageGroup {
  /** The model id, provider id or feature; for time, "2026-11-13" or "2026-11". */
  key: string;
  /** The model's provider, when grouped by model. */
  provider: string | null;
  requests: number;
  /** Without the cache, as AWS reports inputTokens; never below zero. No speech characters in any token figure. */
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  /** Over the requests whose price is known. */
  usd_cost: number;
  price_unknown_requests: number;
  /** MiniMax speech, counted in characters, not tokens. */
  speech_characters: number;
  /**
   * On the oldest month of a period that starts after the 1st: the period's
   * first day ("2026-05-18"), from the backend, so the row says it counts
   * from that day. null on every other row.
   */
  from_day: string | null;
}

/** The periods the page offers, in days. */
export const PERIODS = [30, 90, 180, 365, 730] as const;
export type PeriodDays = (typeof PERIODS)[number];

export const GROUP_BYS = ['model', 'provider', 'feature', 'time'] as const;
export type GroupBy = (typeof GROUP_BYS)[number];

/** A period as the selector and the sentences name it. */
export function periodLabel(days: PeriodDays, text: Text): string {
  switch (days) {
    case 30: return text('30 days', '30 日');
    case 90: return text('90 days', '90 日');
    case 180: return text('6 months', '6 個月');
    case 365: return text('1 year', '1 年');
    case 730: return text('2 years', '2 年');
  }
}

export function groupByLabel(by: GroupBy, text: Text): string {
  switch (by) {
    case 'model': return text('Model', '模型');
    case 'provider': return text('Provider', '供應商');
    case 'feature': return text('Feature', '功能');
    case 'time': return text('Time', '時間');
  }
}

/** Time is grouped by UTC day up to 90 days and by month beyond (src-tauri/src/ai/usage.rs by_month). */
export function byMonth(days: PeriodDays): boolean {
  return days > 90;
}

/** The heading of the table's first column. */
export function groupColumnLabel(by: GroupBy, days: PeriodDays, text: Text): string {
  if (by !== 'time') return groupByLabel(by, text);
  return byMonth(days) ? text('Month', '月份') : text('Day', '日期');
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
 * An amount that leaves out the requests with no price. A request with an
 * unknown price is in no total, so with some left out the amount is a lower
 * bound, and with nothing priced it reads "Price unknown": never US$0.
 */
export function costLabel(knownUsd: number, unpriced: number, text: Text): string {
  if (unpriced === 0) return formatUsd(knownUsd);
  if (knownUsd > 0) {
    return knownUsd < 0.0001
      ? text('More than US$0', '多於 US$0')
      : text(`At least ${formatUsd(knownUsd)}`, `最少 ${formatUsd(knownUsd)}`);
  }
  return text('Price unknown', '價格未知');
}

/**
 * The panel's money headline, by the rule of costLabel. The same holds for
 * the amount per request, so free local requests beside unpriced ones do not
 * read US$0.
 */
export function usageHeadline(stats: CostStats, text: Text): { total: string; perRequest: string; perRequestLabel: string } {
  const unknown = stats.price_unknown_requests;
  const priced = stats.total_requests - unknown;
  return {
    total: costLabel(stats.total_usd, unknown, text),
    perRequest: priced > 0 && (unknown === 0 || stats.total_usd > 0) ? formatUsd(stats.avg_usd_per_request) : text('Price unknown', '價格未知'),
    perRequestLabel: unknown > 0 ? text('Per priced request', '每次已定價請求') : text('Per request', '每次請求'),
  };
}

/**
 * The line under the headline, from get_blended_cost: what a day of use
 * cost on average over the period. It says nothing about a month or a bill.
 * Null when there is nothing to average.
 */
export function perDayLine(blended: BlendedCost, period: PeriodDays, text: Text): string | null {
  const unknown = blended.price_unknown_requests;
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
  const span = periodLabel(period, text);
  if (unknown === 0) {
    return text(
      `${amount} per day of use, ${dayCount} with usage in the last ${span}`,
      `每個使用日平均${amount}，最近 ${span}內有 ${days} 日有使用`,
    );
  }
  return text(
    `${amount} per day of use for requests with a known price, ${dayCount} with such usage in the last ${span}`,
    `已知價格的請求每個使用日平均${amount}，最近 ${span}內有 ${days} 日有這類用量`,
  );
}

/** How many requests had no price, said once for every amount on the panel. */
export function unpricedNote(stats: CostStats, text: Text): string | null {
  const count = stats.price_unknown_requests;
  if (count === 0) return null;
  return text(
    count === 1 ? '1 request has no known price and is in none of the amounts above.' : `${count} requests have no known price and are in none of the amounts above.`,
    `${count} 次請求價格未知，以上金額均未計入。`,
  );
}

/** Said in place of the figures when the period has no usage. */
export function emptyPeriodLine(period: PeriodDays, text: Text): string {
  const span = periodLabel(period, text);
  return text(`No usage recorded on this Mac in the last ${span}.`, `最近 ${span}此 Mac 未有記錄任何用量。`);
}

/** How the history is kept, in one line under the table. */
export function historyLine(perRequestDays: number, text: Text): string {
  return text(
    `Each request is kept for the last ${perRequestDays} days, then as daily totals for two years, only on this Mac. Days are counted in UTC.`,
    `每次請求的記錄保留最近 ${perRequestDays} 日，之後以每日總數保留兩年，全部只存於此 Mac。日子按 UTC 計算。`,
  );
}

/** The heading of the most expensive requests, which come from the per-request rows only. */
export function expensiveHeading(days: number, text: Text): string {
  if (days === 1) return text('Most expensive requests, today', '今日費用最高的請求');
  return text(`Most expensive requests, last ${days} days`, `最近 ${days} 日費用最高的請求`);
}

/**
 * A row's tokens the way the provider reports them: input without the cache
 * (AWS's inputTokens), output, then the cache reads and writes when there are
 * any. Stored input includes the cache, so the cache is taken out here; the
 * stored figures do not change. A row whose stored input is smaller than its
 * cache cannot include it, so its input is shown as stored: never a negative
 * or clamped figure. (Older Bedrock and Claude rows stored input without the
 * cache; the one-time correction in src-tauri/src/ai/cost.rs gives them the
 * cache back, so this is a guard.)
 */
export function tokenFigures(
  row: { input_tokens: number; output_tokens: number; cache_read_tokens: number; cache_write_tokens: number },
  text: Text,
): string {
  const read = row.cache_read_tokens ?? 0;
  const write = row.cache_write_tokens ?? 0;
  const input = row.input_tokens >= read + write ? row.input_tokens - read - write : row.input_tokens;
  const parts = [
    text(`${count(input)} in`, `輸入 ${count(input)}`),
    text(`${count(row.output_tokens)} out`, `輸出 ${count(row.output_tokens)}`),
  ];
  if (read > 0) parts.push(text(`${count(read)} cache read`, `快取讀取 ${count(read)}`));
  if (write > 0) parts.push(text(`${count(write)} cache write`, `快取寫入 ${count(write)}`));
  const figures = parts.join(' · ');
  return text(`${figures} tokens`, `token：${figures}`);
}

function count(tokens: number): string {
  return tokens.toLocaleString('en-US');
}

/**
 * The rows "Most expensive requests" lists: those with a known cost above
 * zero. Rows with no price, or free local ones, would only fill the list, so
 * with nothing priced the list is not shown.
 */
export function expensiveRequests(stats: CostStats): ExpensiveTurn[] {
  return stats.top_expensive.filter(row => !row.price_unknown && row.usd_cost !== null && row.usd_cost > 0);
}

/** A stored provider id as people know it. */
export function providerLabel(provider: string, text: Text): string {
  switch (provider) {
    case 'openai': return 'OpenAI';
    case 'claude': return 'Claude';
    case 'gemini': return 'Gemini';
    case 'bedrock': return 'AWS Bedrock';
    case 'minimax': return 'MiniMax';
    case 'typesafe': return 'TypeSafe';
    case 'kimi': return 'Kimi';
    case 'qwen': return 'Qwen';
    case 'local':
    case 'ollama': return text('Local model', '本機模型');
    // Rows from before providers were recorded.
    case 'unknown': return text('Not recorded', '未有記錄');
    default: return provider.startsWith('compat:') ? provider.slice('compat:'.length) : provider;
  }
}

/** A stored feature (src-tauri/src/ai/cost.rs Feature) as the table names it. */
export function featureLabel(feature: string, text: Text): string {
  switch (feature) {
    case 'chat': return text('Chat', '對話');
    case 'tools': return text('Chat with tools', '使用工具的對話');
    case 'mail_analysis': return text('Mail analysis', '郵件分析');
    case 'mail_sort': return text('Mail sorting', '郵件分類');
    case 'briefing': return text('Briefing', '簡報');
    case 'stt': return text('Speech to text', '語音轉文字');
    case 'tts': return text('Text to speech', '文字轉語音');
    case 'memory': return text('Memory', '記憶');
    // Every request from before features were recorded is "other".
    case 'other': return text('Other or not recorded', '其他或未有記錄');
    default: return feature;
  }
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/**
 * A UTC day ("2026-11-13") or month ("2026-11") as the table shows it. A
 * month the period enters part-way says from which day it counts, from the
 * period's first day the backend gives (UsageGroup.from_day), never from
 * this machine's clock.
 */
export function timeLabel(key: string, text: Text, fromDay?: string | null): string {
  const [year, month, day] = key.split('-').map(Number);
  if (!year || !month || month > 12) return key;
  if (day) return text(`${day} ${MONTHS[month - 1]} ${year}`, `${year}年${month}月${day}日`);
  const from = fromDay ? Number(fromDay.split('-')[2]) : 0;
  if (from > 1) {
    return text(`${MONTHS[month - 1]} ${year}, from ${from} ${MONTHS[month - 1]}`, `${year}年${month}月（由${from}日起）`);
  }
  return text(`${MONTHS[month - 1]} ${year}`, `${year}年${month}月`);
}

/** One table row, every cell as it is shown. */
export interface GroupRow {
  id: string;
  name: string;
  /** The provider under a model's name. */
  detail: string | null;
  requests: string;
  input: string;
  output: string;
  cacheRead: string;
  cacheWrite: string;
  /** MiniMax speech in the group, as characters; shown under the input, never as tokens. */
  characters: string | null;
  cost: string;
}

/** The table's rows, in the order the backend gives them. */
export function groupRows(groups: UsageGroup[], by: GroupBy, text: Text): GroupRow[] {
  return groups.map(group => {
    const name = by === 'provider'
      ? providerLabel(group.key, text)
      : by === 'feature'
        ? featureLabel(group.key, text)
        : by === 'time'
          ? timeLabel(group.key, text, group.from_day)
          : group.key;
    const characters = group.speech_characters ?? 0;
    // A group of MiniMax speech alone has no tokens: the cells stay empty
    // rather than read 0, and the characters say what was used.
    const speechOnly = characters > 0
      && group.input_tokens === 0 && group.output_tokens === 0 && group.cache_read_tokens === 0 && group.cache_write_tokens === 0;
    const tokens = (figure: number) => (speechOnly ? '' : count(figure));
    return {
      id: `${group.provider ?? ''}|${group.key}`,
      name,
      detail: by === 'model' && group.provider ? providerLabel(group.provider, text) : null,
      requests: count(group.requests),
      input: tokens(group.input_tokens),
      output: tokens(group.output_tokens),
      cacheRead: tokens(group.cache_read_tokens),
      cacheWrite: tokens(group.cache_write_tokens),
      characters: characters > 0 ? text(`${count(characters)} characters of speech`, `語音 ${count(characters)} 個字元`) : null,
      cost: costLabel(group.usd_cost, group.price_unknown_requests, text),
    };
  });
}
