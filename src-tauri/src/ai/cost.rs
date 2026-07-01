use crate::ai::Workload;

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_read_per_million: f64,
    pub cache_write_per_million: f64,
}

pub fn price_for(model_id: &str) -> ModelPrice {
    let m = model_id.to_lowercase();
    if m.contains("sonnet-4-6") || m.contains("sonnet-4.6") {
        ModelPrice { input_per_million: 3.0, output_per_million: 15.0, cache_read_per_million: 0.30, cache_write_per_million: 3.75 }
    } else if m.contains("haiku-4-5") || m.contains("haiku-4.5") {
        ModelPrice { input_per_million: 1.0, output_per_million: 5.0, cache_read_per_million: 0.10, cache_write_per_million: 1.25 }
    } else if m.contains("nova-micro") {
        ModelPrice { input_per_million: 0.035, output_per_million: 0.14, cache_read_per_million: 0.00875, cache_write_per_million: 0.04375 }
    } else if m.contains("nova-lite") {
        ModelPrice { input_per_million: 0.06, output_per_million: 0.24, cache_read_per_million: 0.015, cache_write_per_million: 0.075 }
    } else if m.contains("nova-pro") {
        ModelPrice { input_per_million: 0.80, output_per_million: 3.20, cache_read_per_million: 0.20, cache_write_per_million: 1.00 }
    } else if m.contains("gemini-3.1-pro") {
        ModelPrice { input_per_million: 2.0, output_per_million: 12.0, cache_read_per_million: 0.50, cache_write_per_million: 2.0 }
    } else if m.contains("gemini-3-flash") {
        ModelPrice { input_per_million: 0.50, output_per_million: 3.0, cache_read_per_million: 0.125, cache_write_per_million: 0.50 }
    } else if m.contains("gemini-2.5-pro") || m.contains("gemini-pro") {
        ModelPrice { input_per_million: 1.25, output_per_million: 10.0, cache_read_per_million: 0.31, cache_write_per_million: 1.25 }
    } else if m.contains("gemini-2.5-flash") || m.contains("gemini-flash") {
        ModelPrice { input_per_million: 0.30, output_per_million: 2.50, cache_read_per_million: 0.075, cache_write_per_million: 0.30 }
    } else if m.contains("gpt-4o-mini") {
        ModelPrice { input_per_million: 0.15, output_per_million: 0.60, cache_read_per_million: 0.075, cache_write_per_million: 0.15 }
    } else if m.contains("gpt-4o") {
        ModelPrice { input_per_million: 2.50, output_per_million: 10.0, cache_read_per_million: 1.25, cache_write_per_million: 2.50 }
    } else if m.contains("gpt-5") {
        ModelPrice { input_per_million: 1.25, output_per_million: 10.0, cache_read_per_million: 0.125, cache_write_per_million: 1.25 }
    } else {
        ModelPrice { input_per_million: 0.0, output_per_million: 0.0, cache_read_per_million: 0.0, cache_write_per_million: 0.0 }
    }
}

pub fn compute_usd_cost_micro(
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
) -> i64 {
    let p = price_for(model_id);
    let billable_input = (input_tokens - cache_read_tokens - cache_write_tokens).max(0);
    let usd = (billable_input as f64) * p.input_per_million / 1_000_000.0
        + (output_tokens as f64) * p.output_per_million / 1_000_000.0
        + (cache_read_tokens as f64) * p.cache_read_per_million / 1_000_000.0
        + (cache_write_tokens as f64) * p.cache_write_per_million / 1_000_000.0;
    (usd * 1_000_000.0).round() as i64
}

fn workload_str(w: Workload) -> &'static str {
    match w {
        Workload::Volume => "volume",
        Workload::Tools => "tools",
        Workload::Premium => "premium",
    }
}

pub fn record_inference(
    db: &crate::db::Database,
    provider: &str,
    workload: Workload,
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    latency_ms: i64,
) {
    let now = chrono::Utc::now().timestamp();
    let usd_micro = compute_usd_cost_micro(
        model_id,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
    );
    let conn = match db.conn.lock() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("cost: db lock failed: {e}");
            return;
        }
    };
    if let Err(e) = conn.execute(
        "INSERT INTO inference_metrics
            (ts, workload, model_id, input_tokens, output_tokens,
             cache_read_tokens, cache_write_tokens, latency_ms,
             provider, usd_cost_micro)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            now,
            workload_str(workload),
            model_id,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            latency_ms,
            provider,
            usd_micro,
        ],
    ) {
        log::warn!("cost: insert failed: {e}");
    }
}

/// 回傳(自 since_ts 起嘅 usd_cost_micro 總和, 有 inference 嘅唯一 UTC 日數)。
pub fn blended_cost_micro(db: &crate::db::Database, since_ts: i64) -> (i64, i64) {
    let conn = match db.conn.lock() {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };
    let total: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(usd_cost_micro), 0) FROM inference_metrics WHERE ts >= ?1",
            rusqlite::params![since_ts],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let days: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT CAST(ts / 86400 AS INTEGER)) FROM inference_metrics WHERE ts >= ?1",
            rusqlite::params![since_ts],
            |r| r.get(0),
        )
        .unwrap_or(0);
    (total, days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sonnet_cost_matches_known_pricing() {
        // 1k input, 500 output on Sonnet 4.6: 1000*3/1e6 + 500*15/1e6 = 0.003 + 0.0075 = 0.0105 USD
        let micro = compute_usd_cost_micro("claude-sonnet-4-6", 1000, 500, 0, 0);
        assert_eq!(micro, 10_500);
    }

    #[test]
    fn cache_read_subtracts_from_billable_input() {
        // 1000 total input, 800 cache_read on Sonnet: billable = 200
        // 200*3/1e6 + 800*0.30/1e6 + 0 output = 0.0006 + 0.00024 = 0.00084
        let micro = compute_usd_cost_micro("claude-sonnet-4-6", 1000, 0, 800, 0);
        assert_eq!(micro, 840);
    }

    #[test]
    fn unknown_model_returns_zero() {
        assert_eq!(compute_usd_cost_micro("mystery-model", 1000, 1000, 0, 0), 0);
    }

    #[tokio::test]
    async fn blended_sums_and_counts_active_days() {
        use rusqlite::Connection;
        use std::sync::Mutex;
        let path = std::env::temp_dir()
            .join(format!("jarvis-cost-{}.db", uuid::Uuid::new_v4()));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE inference_metrics (
                ts INTEGER, workload TEXT, model_id TEXT,
                input_tokens INTEGER, output_tokens INTEGER,
                cache_read_tokens INTEGER, cache_write_tokens INTEGER,
                latency_ms INTEGER, provider TEXT, usd_cost_micro INTEGER);
             INSERT INTO inference_metrics (ts, usd_cost_micro) VALUES
               (1000, 500000),        -- day A: 0.50 USD
               (1500, 250000),        -- day A: 0.25 USD
               (90000, 1000000);      -- day B: 1.00 USD",
        ).unwrap();
        let db = crate::db::Database { conn: Mutex::new(conn) };
        let (total, days) = blended_cost_micro(&db, 0);
        assert_eq!(total, 1_750_000); // 1.75 USD
        assert_eq!(days, 2);          // 兩個唔同 UTC 日
    }
}
