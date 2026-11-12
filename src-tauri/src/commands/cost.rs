use crate::db::Database;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct ProviderBreakdown {
    pub provider: String,
    pub turns: i64,
    /// None for a local provider: it is free, so the tokens are the spend.
    /// For a cloud provider, the turns with an unknown price are left out.
    pub usd_cost: Option<f64>,
    /// Includes the cache figures below, as stored for every provider.
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// Turns whose model has no price (ai/models.rs).
    pub price_unknown_turns: i64,
}

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
    /// None for a local provider, and when the price is unknown.
    pub usd_cost: Option<f64>,
    pub price_unknown: bool,
    pub latency_ms: i64,
}

#[derive(Serialize)]
pub struct CostStats {
    pub total_turns_30d: i64,
    pub total_usd_30d: f64,
    pub avg_usd_per_turn_30d: f64,
    pub local_turns_30d: i64,
    pub local_share_pct: f64,
    /// Turns left out of the totals because their price is unknown.
    pub price_unknown_turns_30d: i64,
    pub by_provider_30d: Vec<ProviderBreakdown>,
    pub top_expensive_30d: Vec<ExpensiveTurn>,
}

#[tauri::command]
pub async fn get_cost_stats(db: State<'_, Arc<Database>>) -> Result<CostStats, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let cutoff = chrono::Utc::now().timestamp() - (30 * 24 * 3600);
    stats_since(&conn, cutoff)
}

fn stats_since(conn: &rusqlite::Connection, cutoff: i64) -> Result<CostStats, String> {
    let (turns, micro_total, price_unknown_turns): (i64, i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(usd_cost_micro), 0), COALESCE(SUM(price_unknown), 0)
             FROM inference_metrics WHERE ts >= ?1",
            rusqlite::params![cutoff],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap_or((0, 0, 0));

    let local_turns: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM inference_metrics
             WHERE ts >= ?1 AND provider = 'local'",
            rusqlite::params![cutoff],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut by_provider: Vec<ProviderBreakdown> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT provider, COUNT(*), COALESCE(SUM(usd_cost_micro), 0),
                        COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(price_unknown), 0),
                        COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_write_tokens), 0)
                 FROM inference_metrics WHERE ts >= ?1
                 GROUP BY provider
                 ORDER BY COALESCE(SUM(usd_cost_micro), 0) DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![cutoff], |r| {
                let provider: String = r.get(0)?;
                let micro: i64 = r.get(2)?;
                Ok(ProviderBreakdown {
                    usd_cost: (!crate::ai::models::is_local(&provider)).then(|| micro as f64 / 1_000_000.0),
                    provider,
                    turns: r.get(1)?,
                    input_tokens: r.get(3)?,
                    output_tokens: r.get(4)?,
                    cache_read_tokens: r.get(6)?,
                    cache_write_tokens: r.get(7)?,
                    price_unknown_turns: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            if let Ok(r) = row {
                by_provider.push(r);
            }
        }
    }

    let mut top_expensive: Vec<ExpensiveTurn> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT ts, provider, model_id, workload, input_tokens, output_tokens,
                        usd_cost_micro, latency_ms, feature, price_unknown,
                        COALESCE(cache_read_tokens, 0), COALESCE(cache_write_tokens, 0)
                 FROM inference_metrics WHERE ts >= ?1
                 ORDER BY usd_cost_micro DESC
                 LIMIT 5",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![cutoff], |r| {
                // NULL when the price is unknown (V33).
                let micro: Option<i64> = r.get(6)?;
                let provider: String = r.get(1)?;
                Ok(ExpensiveTurn {
                    ts: r.get(0)?,
                    usd_cost: micro.filter(|_| !crate::ai::models::is_local(&provider)).map(|micro| micro as f64 / 1_000_000.0),
                    provider,
                    model_id: r.get(2)?,
                    workload: r.get(3)?,
                    feature: r.get(8)?,
                    input_tokens: r.get(4)?,
                    output_tokens: r.get(5)?,
                    cache_read_tokens: r.get(10)?,
                    cache_write_tokens: r.get(11)?,
                    price_unknown: r.get(9)?,
                    latency_ms: r.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            if let Ok(r) = row {
                top_expensive.push(r);
            }
        }
    }

    let total_usd = micro_total as f64 / 1_000_000.0;
    // Over the turns with a price: an unknown one is not a free one.
    let priced_turns = turns - price_unknown_turns;
    let avg = if priced_turns > 0 { total_usd / priced_turns as f64 } else { 0.0 };
    let local_share = if turns > 0 {
        100.0 * local_turns as f64 / turns as f64
    } else {
        0.0
    };

    Ok(CostStats {
        total_turns_30d: turns,
        total_usd_30d: total_usd,
        avg_usd_per_turn_30d: avg,
        local_turns_30d: local_turns,
        local_share_pct: local_share,
        price_unknown_turns_30d: price_unknown_turns,
        by_provider_30d: by_provider,
        top_expensive_30d: top_expensive,
    })
}

#[derive(serde::Serialize)]
pub struct BlendedCost {
    /// Over the requests whose price is known.
    pub total_usd: f64,
    /// Days with usage whose price is known.
    pub active_days: i64,
    /// total_usd over active_days: what a day of use cost, on average. It is
    /// not a forecast of a bill.
    pub usd_per_active_day: f64,
    /// Requests with no price. They are left out of the figures above, which
    /// are therefore low by what these cost; the panel says so.
    pub price_unknown_turns: i64,
}

#[tauri::command]
pub async fn get_blended_cost(
    db: tauri::State<'_, std::sync::Arc<crate::db::Database>>,
    days: i64,
) -> Result<BlendedCost, String> {
    let days = days.clamp(1, 3650);
    let now = chrono::Utc::now().timestamp();
    blended_since(&db, now - days * 86400)
}

fn blended_since(db: &crate::db::Database, since: i64) -> Result<BlendedCost, String> {
    let blended = crate::ai::cost::blended_cost_micro(db, since)?;
    let total_usd = blended.total_micro as f64 / 1_000_000.0;
    Ok(BlendedCost {
        total_usd,
        active_days: blended.active_days,
        usd_per_active_day: usd_per_active_day(total_usd, blended.active_days),
        price_unknown_turns: blended.price_unknown_turns,
    })
}

fn usd_per_active_day(total_usd: f64, active_days: i64) -> f64 {
    if active_days > 0 {
        total_usd / active_days as f64
    } else {
        0.0
    }
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
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(&format!(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                    ({now}, 'tools', 'gpt-6-luna', 1000, 10, 5, 'openai', 105, 'chat', 0),
                    ({now}, 'tools', 'gpt-unlisted-1', 2000, 20, 5, 'openai', NULL, 'tools', 1),
                    ({now}, 'volume', 'speech-02-hd', 12, 0, 5, 'minimax', NULL, 'tts', 1);"
            ))
            .unwrap();
        }
        let conn = db.conn.lock().unwrap();
        let stats = stats_since(&conn, now - 60).unwrap();
        assert_eq!(stats.total_turns_30d, 3);
        assert_eq!(stats.price_unknown_turns_30d, 2);
        assert!((stats.total_usd_30d - 0.000105).abs() < 1e-12);
        assert!((stats.avg_usd_per_turn_30d - 0.000105).abs() < 1e-12, "the average is over priced turns");
        let openai = stats.by_provider_30d.iter().find(|p| p.provider == "openai").unwrap();
        assert_eq!((openai.turns, openai.price_unknown_turns), (2, 1));
        let minimax = stats.by_provider_30d.iter().find(|p| p.provider == "minimax").unwrap();
        assert_eq!((minimax.turns, minimax.price_unknown_turns), (1, 1));
        let unknown: Vec<(&str, Option<f64>, bool)> = stats.top_expensive_30d.iter().filter(|t| t.price_unknown).map(|t| (t.feature.as_str(), t.usd_cost, t.price_unknown)).collect();
        assert_eq!(unknown.len(), 2);
        assert!(unknown.iter().all(|(_, cost, _)| cost.is_none()), "{unknown:?}");
    }

    /// The blended figure says how many requests it could not price, so a low
    /// figure is never silently low.
    #[test]
    fn the_blended_cost_says_how_much_usage_had_no_price() {
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                    ({now}, 'tools', 'kimi-k2', 2000, 20, 5, 'compat:api.moonshot.ai', NULL, 'chat', 1),
                    ({now}, 'tools', 'kimi-k2', 1000, 10, 5, 'compat:api.moonshot.ai', NULL, 'chat', 1);"
            ))
            .unwrap();
        let blended = blended_since(&db, now - 60).unwrap();
        assert_eq!((blended.total_usd, blended.active_days, blended.usd_per_active_day, blended.price_unknown_turns), (0.0, 0, 0.0, 2));
    }

    #[test]
    fn per_active_day_with_zero_days_is_zero_not_nan() {
        assert_eq!(usd_per_active_day(0.0, 0), 0.0);
        assert_eq!(usd_per_active_day(5.0, 0), 0.0);
    }

    /// US$1.50 spent over 3 days of use is US$0.50 a day of use, whatever the
    /// window: the figure is not scaled up to a month.
    #[test]
    fn spend_per_day_of_use_is_the_total_over_the_days_with_usage() {
        assert!((usd_per_active_day(1.5, 3) - 0.5).abs() < 1e-9);
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                    ({now}, 'tools', 'gpt-6-luna', 1000, 10, 5, 'openai', 1000000, 'chat', 0),
                    ({}, 'tools', 'gpt-6-luna', 1000, 10, 5, 'openai', 500000, 'chat', 0);",
                now - 86_400
            ))
            .unwrap();
        let blended = blended_since(&db, now - 30 * 86_400).unwrap();
        assert_eq!((blended.total_usd, blended.active_days, blended.usd_per_active_day), (1.5, 2, 0.75));
    }

    /// The panel shows input without the cache, as AWS reports inputTokens,
    /// so the cache figures are handed over with each row and each provider.
    #[test]
    fn the_cache_figures_reach_the_panel() {
        let db = crate::db::test_database();
        let now = chrono::Utc::now().timestamp();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(&format!(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                    ({now}, 'tools', 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', 1320, 45, 800, 400, 5, 'bedrock', 900, 'chat', 0),
                    ({now}, 'tools', 'apac.anthropic.claude-haiku-4-5-20251001-v1:0', 9, 2, 0, 0, 5, 'bedrock', 100, 'chat', 0);"
            ))
            .unwrap();
        }
        let conn = db.conn.lock().unwrap();
        let stats = stats_since(&conn, now - 60).unwrap();
        let bedrock = &stats.by_provider_30d[0];
        assert_eq!((bedrock.input_tokens, bedrock.output_tokens, bedrock.cache_read_tokens, bedrock.cache_write_tokens), (1_329, 47, 800, 400));
        let top = &stats.top_expensive_30d[0];
        assert_eq!((top.input_tokens, top.output_tokens, top.cache_read_tokens, top.cache_write_tokens), (1_320, 45, 800, 400));
    }
}
