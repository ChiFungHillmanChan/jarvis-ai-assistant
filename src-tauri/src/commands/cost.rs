//! The Settings > Usage page's reads. Every figure comes from ai/usage.rs,
//! which adds the per-request rows and the daily totals; each command first
//! runs the roll-up when the UTC day has changed since the last one.

use crate::ai::usage::{self, GroupBy, Period};
use crate::db::Database;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct ExpensiveTurn {
    pub ts: i64,
    pub provider: String,
    pub model_id: String,
    pub workload: String,
    pub feature: String,
    /// Includes the cache figures below, as stored for every provider.
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// Always known: only priced requests are listed.
    pub usd_cost: Option<f64>,
    pub price_unknown: bool,
    pub latency_ms: i64,
}

#[derive(Serialize)]
pub struct CostStats {
    /// The period: the last `days` UTC days, today included.
    pub days: i64,
    pub total_requests: i64,
    /// Over the requests whose price is known.
    pub total_usd: f64,
    /// Over the requests whose price is known.
    pub avg_usd_per_request: f64,
    pub local_requests: i64,
    pub local_share_pct: f64,
    /// Requests left out of the money figures because their price is unknown.
    pub price_unknown_requests: i64,
    /// The UTC days "Most expensive requests" covers: the per-request window,
    /// whatever the period.
    pub expensive_days: i64,
    /// From the per-request rows of those days only.
    pub top_expensive: Vec<ExpensiveTurn>,
}

fn micro_to_usd(micro: i64) -> f64 {
    micro as f64 / 1_000_000.0
}

#[tauri::command]
pub async fn get_cost_stats(db: State<'_, Arc<Database>>, days: i64) -> Result<CostStats, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    usage::roll_up_if_due(&conn, now);
    stats_for(&conn, now, days)
}

fn stats_for(conn: &Connection, now: i64, days: i64) -> Result<CostStats, String> {
    let totals = usage::totals(conn, Period::last(days, now)?).map_err(|e| e.to_string())?;
    let (expensive_days, expensive) = usage::expensive_requests(conn, now, 5).map_err(|e| e.to_string())?;
    let sums = &totals.sums;
    let total_usd = micro_to_usd(sums.known_cost_micro);
    // Over the requests with a price: an unknown one is not a free one.
    let priced = sums.requests - sums.price_unknown_requests;
    Ok(CostStats {
        days,
        total_requests: sums.requests,
        total_usd,
        avg_usd_per_request: if priced > 0 { total_usd / priced as f64 } else { 0.0 },
        local_requests: totals.local_requests,
        local_share_pct: if sums.requests > 0 { 100.0 * totals.local_requests as f64 / sums.requests as f64 } else { 0.0 },
        price_unknown_requests: sums.price_unknown_requests,
        expensive_days,
        top_expensive: expensive
            .into_iter()
            .map(|r| ExpensiveTurn {
                ts: r.ts,
                provider: r.provider,
                model_id: r.model_id,
                workload: r.workload,
                feature: r.feature,
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
                cache_read_tokens: r.cache_read_tokens,
                cache_write_tokens: r.cache_write_tokens,
                usd_cost: Some(micro_to_usd(r.usd_cost_micro)),
                price_unknown: false,
                latency_ms: r.latency_ms,
            })
            .collect(),
    })
}

#[derive(Serialize)]
pub struct BlendedCost {
    /// Over the requests whose price is known.
    pub total_usd: f64,
    /// UTC days with usage whose price is known.
    pub active_days: i64,
    /// total_usd over active_days: what a day of use cost, on average. It is
    /// not a forecast of a bill.
    pub usd_per_active_day: f64,
    /// Requests with no price. They are left out of the figures above, which
    /// are therefore low by what these cost; the panel says so.
    pub price_unknown_requests: i64,
}

#[tauri::command]
pub async fn get_blended_cost(db: State<'_, Arc<Database>>, days: i64) -> Result<BlendedCost, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    usage::roll_up_if_due(&conn, now);
    blended_for(&conn, now, days)
}

fn blended_for(conn: &Connection, now: i64, days: i64) -> Result<BlendedCost, String> {
    let totals = usage::totals(conn, Period::last(days, now)?).map_err(|e| e.to_string())?;
    let total_usd = micro_to_usd(totals.sums.known_cost_micro);
    Ok(BlendedCost {
        total_usd,
        active_days: totals.priced_days,
        usd_per_active_day: usd_per_active_day(total_usd, totals.priced_days),
        price_unknown_requests: totals.sums.price_unknown_requests,
    })
}

fn usd_per_active_day(total_usd: f64, active_days: i64) -> f64 {
    if active_days > 0 {
        total_usd / active_days as f64
    } else {
        0.0
    }
}

/// One row of the Usage table.
#[derive(Debug, PartialEq, Serialize)]
pub struct UsageGroup {
    /// The model id, provider id or feature; for time, the UTC day
    /// ("2026-11-13") or month ("2026-11").
    pub key: String,
    /// The model's provider, when grouped by model.
    pub provider: Option<String>,
    pub requests: i64,
    /// Without the cache, the way AWS reports inputTokens (ai/usage.rs
    /// `Sums::input_without_cache`): never below zero. Speech characters
    /// are in none of the token figures.
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// Over the requests whose price is known.
    pub usd_cost: f64,
    pub price_unknown_requests: i64,
    /// MiniMax speech, which is counted in characters, not tokens.
    pub speech_characters: i64,
    /// On the oldest month of a period that starts after the 1st: the
    /// period's first day ("2026-05-18"), from which the row counts. None on
    /// every other row.
    pub from_day: Option<String>,
}

/// The period's usage by "model", "provider", "feature" or "time" (by UTC day
/// up to 90 days, by month beyond), in the order the table shows it.
#[tauri::command]
pub async fn get_usage_groups(db: State<'_, Arc<Database>>, days: i64, group_by: String) -> Result<Vec<UsageGroup>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    usage::roll_up_if_due(&conn, now);
    groups_for(&conn, now, days, &group_by)
}

fn groups_for(conn: &Connection, now: i64, days: i64, group_by: &str) -> Result<Vec<UsageGroup>, String> {
    let groups = usage::groups(conn, Period::last(days, now)?, GroupBy::parse(group_by)?).map_err(|e| e.to_string())?;
    Ok(groups
        .into_iter()
        .map(|group| UsageGroup {
            input_tokens: group.sums.input_without_cache,
            key: group.key,
            provider: group.provider,
            requests: group.sums.requests,
            output_tokens: group.sums.output_tokens,
            cache_read_tokens: group.sums.cache_read_tokens,
            cache_write_tokens: group.sums.cache_write_tokens,
            usd_cost: micro_to_usd(group.sums.known_cost_micro),
            price_unknown_requests: group.sums.price_unknown_requests,
            speech_characters: group.sums.speech_characters,
            from_day: group.from_day,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row with an unknown price is counted, left out of the money, and
    /// shown as unknown rather than free.
    #[test]
    fn unknown_prices_are_reported_not_summed_as_zero() {
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        let conn = db.conn.lock().unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                ({now}, 'tools', 'gpt-6-luna', 1000, 10, 5, 'openai', 105, 'chat', 0),
                ({now}, 'tools', 'gpt-unlisted-1', 2000, 20, 5, 'openai', NULL, 'tools', 1),
                ({now}, 'volume', 'speech-02-hd', 12, 0, 5, 'minimax', NULL, 'tts', 1);"
        ))
        .unwrap();
        let stats = stats_for(&conn, now, 30).unwrap();
        assert_eq!((stats.days, stats.total_requests, stats.price_unknown_requests), (30, 3, 2));
        assert!((stats.total_usd - 0.000105).abs() < 1e-12);
        assert!((stats.avg_usd_per_request - 0.000105).abs() < 1e-12, "the average is over priced requests");
        assert_eq!(stats.top_expensive.len(), 1, "only a priced request is listed");
        let providers = groups_for(&conn, now, 30, "provider").unwrap();
        let openai = providers.iter().find(|g| g.key == "openai").unwrap();
        assert_eq!((openai.requests, openai.price_unknown_requests), (2, 1));
        let minimax = providers.iter().find(|g| g.key == "minimax").unwrap();
        assert_eq!((minimax.requests, minimax.price_unknown_requests, minimax.usd_cost), (1, 1, 0.0));
    }

    /// The blended figure says how many requests it could not price, so a low
    /// figure is never silently low.
    #[test]
    fn the_blended_cost_says_how_much_usage_had_no_price() {
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        let conn = db.conn.lock().unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                ({now}, 'tools', 'kimi-k2', 2000, 20, 5, 'compat:api.moonshot.ai', NULL, 'chat', 1),
                ({now}, 'tools', 'kimi-k2', 1000, 10, 5, 'compat:api.moonshot.ai', NULL, 'chat', 1);"
        ))
        .unwrap();
        let blended = blended_for(&conn, now, 30).unwrap();
        assert_eq!((blended.total_usd, blended.active_days, blended.usd_per_active_day, blended.price_unknown_requests), (0.0, 0, 0.0, 2));
    }

    #[test]
    fn per_active_day_with_zero_days_is_zero_not_nan() {
        assert_eq!(usd_per_active_day(0.0, 0), 0.0);
        assert_eq!(usd_per_active_day(5.0, 0), 0.0);
    }

    /// US$1.50 spent over 2 days of use is US$0.75 a day of use, whatever the
    /// period: the figure is not scaled up to a month.
    #[test]
    fn spend_per_day_of_use_is_the_total_over_the_days_with_usage() {
        assert!((usd_per_active_day(1.5, 3) - 0.5).abs() < 1e-9);
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        let conn = db.conn.lock().unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                ({now}, 'tools', 'gpt-6-luna', 1000, 10, 5, 'openai', 1000000, 'chat', 0),
                ({}, 'tools', 'gpt-6-luna', 1000, 10, 5, 'openai', 500000, 'chat', 0);",
            now - 86_400
        ))
        .unwrap();
        for days in usage::PERIODS {
            let blended = blended_for(&conn, now, days).unwrap();
            assert_eq!((blended.total_usd, blended.active_days, blended.usd_per_active_day), (1.5, 2, 0.75), "{days} days");
        }
    }

    /// The table shows input without the cache, as AWS reports inputTokens;
    /// the most expensive requests keep the stored figures with the cache.
    #[test]
    fn the_cache_figures_reach_the_panel() {
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        let conn = db.conn.lock().unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                ({now}, 'tools', 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', 1320, 45, 800, 400, 5, 'bedrock', 900, 'chat', 0),
                ({now}, 'tools', 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', 9, 2, 0, 0, 5, 'bedrock', 100, 'chat', 0);"
        ))
        .unwrap();
        let bedrock = &groups_for(&conn, now, 30, "provider").unwrap()[0];
        assert_eq!((bedrock.input_tokens, bedrock.output_tokens, bedrock.cache_read_tokens, bedrock.cache_write_tokens), (129, 47, 800, 400));
        let top = &stats_for(&conn, now, 30).unwrap().top_expensive[0];
        assert_eq!((top.input_tokens, top.output_tokens, top.cache_read_tokens, top.cache_write_tokens), (1_320, 45, 800, 400));
    }

    /// N2 and N5: a row stored with less input than its cache (an old
    /// Bedrock row the step did not correct) shows its input as stored, not
    /// zero; MiniMax characters reach the table as characters, never tokens.
    #[test]
    fn stored_input_below_its_cache_and_speech_characters_reach_the_table_as_they_are() {
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        let conn = db.conn.lock().unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                ({now}, 'tools', 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', 120, 45, 800, 400, 5, 'bedrock', 805, 'chat', 0),
                ({now}, 'volume', 'speech-02-hd', 1234, 0, 0, 0, 5, 'minimax', NULL, 'tts', 1);"
        ))
        .unwrap();
        let groups = groups_for(&conn, now, 30, "model").unwrap();
        let figures: Vec<(&str, i64, i64, i64)> = groups.iter().map(|g| (g.key.as_str(), g.input_tokens, g.output_tokens, g.speech_characters)).collect();
        assert_eq!(figures, [("apac.anthropic.claude-haiku-4-5-20251001-v1:0", 120, 45, 0), ("speech-02-hd", 0, 0, 1_234)]);
    }

    /// N1. Usage spread through every day, read at 07:00 UTC (15:00 in Hong
    /// Kong): a request on each of the UTC days the last N x 24 hours touch,
    /// which is N + 1 of them. A period is the last N UTC days including
    /// today, in both levels, so it never counts more than N days of use.
    #[test]
    fn a_period_never_counts_more_days_than_it_has() {
        const DAY: i64 = 86_400;
        // 2026-11-13, 07:00 UTC.
        let today = 20_770;
        let now = today * DAY + 7 * 3_600;
        for days in crate::ai::usage::PERIODS {
            let db = crate::db::test_database();
            let conn = db.conn.lock().unwrap();
            // The evening of the day the last N x 24 hours start on, then a
            // morning on every day since, today's before now.
            let mut stamps = vec![(today - days) * DAY + 23 * 3_600];
            stamps.extend((today - days + 1..=today).map(|day| day * DAY + 3_600));
            assert_eq!(stamps.len() as i64, days + 1);
            assert!(stamps.iter().all(|ts| *ts <= now && now - ts < days * DAY), "all inside the last {days} x 24 hours");
            for ts in stamps {
                conn.execute(
                    "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES (?1, 'volume', 'gpt-6-luna', 10, 1, 5, 'openai', 100, 'mail_sort', 0)",
                    [ts],
                )
                .unwrap();
            }
            crate::ai::usage::roll_up(&conn, now, 30).unwrap();
            let blended = blended_for(&conn, now, days).unwrap();
            assert!(blended.active_days <= days, "{days} days: {}", blended.active_days);
            assert_eq!(blended.active_days, days, "{days} days");
            assert_eq!(stats_for(&conn, now, days).unwrap().total_requests, days, "{days} days");
        }
    }

    /// U1. Six months from 13 November 2026 start on 18 May: the May row
    /// carries that day, so the page can say it counts from it.
    #[test]
    fn the_oldest_month_row_carries_the_period_s_first_day() {
        const DAY: i64 = 86_400;
        // 2026-11-13, 12:00 UTC; six months start on 18 May 2026.
        let now = 20_770 * DAY + 12 * 3_600;
        let first = 20_770 - 179;
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        for ts in [first * DAY, (first + 20) * DAY] {
            conn.execute(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES (?1, 'volume', 'gpt-6-luna', 10, 1, 5, 'openai', 100, 'mail_sort', 0)",
                [ts],
            )
            .unwrap();
        }
        let rows: Vec<(String, Option<String>)> = groups_for(&conn, now, 180, "time").unwrap().into_iter().map(|g| (g.key, g.from_day)).collect();
        assert_eq!(rows, [("2026-06".to_string(), None), ("2026-05".to_string(), Some("2026-05-18".to_string()))]);
        assert!(groups_for(&conn, now, 90, "time").unwrap().iter().all(|g| g.from_day.is_none()), "days need no label");
    }

    #[test]
    fn only_the_offered_periods_and_groupings_are_read() {
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        let conn = db.conn.lock().unwrap();
        assert!(stats_for(&conn, now, 3650).is_err());
        assert!(blended_for(&conn, now, 7).is_err());
        assert!(groups_for(&conn, now, 30, "model; DROP TABLE usage_daily").is_err());
        assert_eq!(stats_for(&conn, now, 730).unwrap().expensive_days, 30);
        assert!(groups_for(&conn, now, 730, "time").unwrap().is_empty());
    }
}
