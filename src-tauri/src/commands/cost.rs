use crate::db::Database;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct ProviderBreakdown {
    pub provider: String,
    pub turns: i64,
    /// None for a local provider: it is free, so the tokens are the spend.
    pub usd_cost: Option<f64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Serialize)]
pub struct ExpensiveTurn {
    pub ts: i64,
    pub provider: String,
    pub model_id: String,
    pub workload: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// None for a local provider.
    pub usd_cost: Option<f64>,
    pub latency_ms: i64,
}

#[derive(Serialize)]
pub struct CostStats {
    pub total_turns_30d: i64,
    pub total_usd_30d: f64,
    pub avg_usd_per_turn_30d: f64,
    pub local_turns_30d: i64,
    pub local_share_pct: f64,
    pub by_provider_30d: Vec<ProviderBreakdown>,
    pub top_expensive_30d: Vec<ExpensiveTurn>,
}

#[tauri::command]
pub async fn get_cost_stats(db: State<'_, Arc<Database>>) -> Result<CostStats, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let cutoff = chrono::Utc::now().timestamp() - (30 * 24 * 3600);

    let (turns, micro_total): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(usd_cost_micro), 0)
             FROM inference_metrics WHERE ts >= ?1",
            rusqlite::params![cutoff],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap_or((0, 0));

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
                        COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
                 FROM inference_metrics WHERE ts >= ?1
                 GROUP BY provider
                 ORDER BY SUM(usd_cost_micro) DESC",
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
                        usd_cost_micro, latency_ms
                 FROM inference_metrics WHERE ts >= ?1
                 ORDER BY usd_cost_micro DESC
                 LIMIT 5",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![cutoff], |r| {
                let micro: i64 = r.get(6)?;
                let provider: String = r.get(1)?;
                Ok(ExpensiveTurn {
                    ts: r.get(0)?,
                    usd_cost: (!crate::ai::models::is_local(&provider)).then(|| micro as f64 / 1_000_000.0),
                    provider,
                    model_id: r.get(2)?,
                    workload: r.get(3)?,
                    input_tokens: r.get(4)?,
                    output_tokens: r.get(5)?,
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
    let avg = if turns > 0 { total_usd / turns as f64 } else { 0.0 };
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
        by_provider_30d: by_provider,
        top_expensive_30d: top_expensive,
    })
}

#[derive(serde::Serialize)]
pub struct BlendedCost {
    pub total_usd: f64,
    pub active_days: i64,
    pub implied_monthly_usd: f64,
}

#[tauri::command]
pub async fn get_blended_cost(
    db: tauri::State<'_, std::sync::Arc<crate::db::Database>>,
    days: i64,
) -> Result<BlendedCost, String> {
    let days = days.clamp(1, 3650);
    let now = chrono::Utc::now().timestamp();
    let since = now - days * 86400;
    let (total_micro, active_days) = crate::ai::cost::blended_cost_micro(&db, since)?;
    let total_usd = total_micro as f64 / 1_000_000.0;
    Ok(BlendedCost {
        total_usd,
        active_days,
        implied_monthly_usd: implied_monthly_usd(total_usd, active_days),
    })
}

fn implied_monthly_usd(total_usd: f64, active_days: i64) -> f64 {
    if active_days > 0 {
        total_usd / active_days as f64 * 30.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implied_monthly_zero_days_is_zero_not_nan() {
        assert_eq!(implied_monthly_usd(0.0, 0), 0.0);
        assert_eq!(implied_monthly_usd(5.0, 0), 0.0);
    }

    #[test]
    fn implied_monthly_scales_daily_average_to_30_days() {
        // 1.75 USD over 2 active days -> 0.875/day -> 26.25/mo
        assert!((implied_monthly_usd(1.75, 2) - 26.25).abs() < 1e-9);
    }
}
