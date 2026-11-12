use crate::ai::Workload;

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
    // Prices and the local-is-free rule live in ai/models.rs. A local row is
    // stored at 0 with its tokens.
    let usd_micro = crate::ai::models::usd_cost_micro(provider, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens).unwrap_or(0);
    insert_row(db, provider, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_micro, latency_ms);
}

/// The usage of one chat turn, which can take several requests (tool
/// rounds). Each request is priced on its own: a long-context tier applies
/// to one request, never to the sum of a turn. The turn is then stored as one
/// row, so the dashboard still counts turns.
///
/// `input_tokens` always includes cached tokens, as `usd_cost_micro` expects;
/// a provider that reports them separately (Anthropic, Bedrock) adds them in.
pub struct TurnUsage {
    provider: String,
    model_id: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    usd_micro: i64,
}

impl TurnUsage {
    pub fn new(provider: &str, model_id: &str) -> Self {
        TurnUsage { provider: provider.into(), model_id: model_id.into(), input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, usd_micro: 0 }
    }

    /// Add one request's usage.
    pub fn add_request(&mut self, input_tokens: i64, output_tokens: i64, cache_read_tokens: i64, cache_write_tokens: i64) {
        self.input_tokens += input_tokens;
        self.output_tokens += output_tokens;
        self.cache_read_tokens += cache_read_tokens;
        self.cache_write_tokens += cache_write_tokens;
        self.usd_micro += crate::ai::models::usd_cost_micro(&self.provider, &self.model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens).unwrap_or(0);
    }

    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0 && self.cache_read_tokens == 0 && self.cache_write_tokens == 0
    }

    /// Store the turn. Taking `self` means a turn is stored at most once.
    pub fn record(self, db: &crate::db::Database, workload: Workload, latency_ms: i64) {
        insert_row(db, &self.provider, workload, &self.model_id, self.input_tokens, self.output_tokens, self.cache_read_tokens, self.cache_write_tokens, self.usd_micro, latency_ms);
    }

    /// For a turn that failed part-way: store what earlier requests were
    /// billed, and nothing when no request was.
    pub fn record_if_billed(self, db: &crate::db::Database, workload: Workload, latency_ms: i64) {
        if !self.is_empty() {
            self.record(db, workload, latency_ms);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_row(
    db: &crate::db::Database,
    provider: &str,
    workload: Workload,
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    usd_micro: i64,
    latency_ms: i64,
) {
    let now = chrono::Utc::now().timestamp();
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
    } else {
        log::debug!(
            "cost: recorded provider={provider} model={model_id} workload={} in={input_tokens} out={output_tokens} usd_micro={usd_micro} latency_ms={latency_ms}",
            workload_str(workload)
        );
    }
}

/// 回傳(自 since_ts 起嘅 usd_cost_micro 總和, 有 inference 嘅唯一 UTC 日數)。
/// Errors propagate so a telemetry failure never renders as a real "$0.00/mo".
pub fn blended_cost_micro(db: &crate::db::Database, since_ts: i64) -> Result<(i64, i64), String> {
    let conn = db.conn.lock().map_err(|e| {
        log::warn!("cost: db lock failed reading blended cost (since_ts={since_ts}): {e}");
        format!("db lock failed: {e}")
    })?;
    conn.query_row(
        "SELECT COALESCE(SUM(usd_cost_micro), 0),
                COUNT(DISTINCT CAST(ts / 86400 AS INTEGER))
         FROM inference_metrics WHERE ts >= ?1",
        rusqlite::params![since_ts],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(|e| {
        log::warn!("cost: blended cost query failed: {e}");
        format!("blended cost query failed: {e}")
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// In-memory sqlite seeded with rows_sql. Nothing here needs a file on disk,
    /// and a file-backed fixture leaks its .db/-wal/-shm into $TMPDIR whenever an
    /// assertion fails before the cleanup line -- which is exactly the state the
    /// fixture is in while someone is iterating on it.
    /// Mirrors V13 + V16 inference_metrics DDL (minus id/NOT NULL/CHECK) --
    /// update if that schema changes.
    pub(crate) fn temp_metrics_db(rows_sql: &str) -> crate::db::Database {
        use rusqlite::Connection;
        use std::sync::Mutex;
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE inference_metrics (
                ts INTEGER, workload TEXT, model_id TEXT,
                input_tokens INTEGER, output_tokens INTEGER,
                cache_read_tokens INTEGER, cache_write_tokens INTEGER,
                latency_ms INTEGER, provider TEXT, usd_cost_micro INTEGER);
             {rows_sql}"
        ))
        .unwrap();
        crate::db::Database { conn: Mutex::new(conn) }
    }

    const SEED_ROWS: &str = "INSERT INTO inference_metrics (ts, usd_cost_micro) VALUES
               (1000, 500000),        -- day A: 0.50 USD
               (1500, 250000),        -- day A: 0.25 USD
               (90000, 1000000);      -- day B: 1.00 USD";

    #[test]
    fn blended_sums_and_counts_active_days() {
        let db = temp_metrics_db(SEED_ROWS);
        let (total, days) = blended_cost_micro(&db, 0).unwrap();
        assert_eq!(total, 1_750_000); // 1.75 USD
        assert_eq!(days, 2);          // 兩個唔同 UTC 日
    }

    #[test]
    fn blended_since_ts_excludes_rows_before_window() {
        let db = temp_metrics_db(SEED_ROWS);
        // Window opens after both day-A rows: only the 90000-ts row counts.
        let (total, days) = blended_cost_micro(&db, 50_000).unwrap();
        assert_eq!(total, 1_000_000);
        assert_eq!(days, 1);
    }

    fn rows(db: &crate::db::Database) -> Vec<(i64, i64, i64, i64)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT input_tokens, output_tokens, cache_read_tokens, usd_cost_micro FROM inference_metrics").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    /// Five haiku-5-5 rounds of 60K input are 300K in the turn, but every
    /// request is under the 100K tier, so all five are billed at the base
    /// rate: $0.10/M, 300K -> $0.03. Priced on the sum it read $0.15.
    #[test]
    fn a_turn_prices_each_request_on_its_own_and_is_stored_once() {
        let db = temp_metrics_db("");
        let mut turn = TurnUsage::new("claude", "claude-haiku-5-5");
        for _ in 0..5 {
            turn.add_request(60_000, 0, 0, 0);
        }
        turn.record(&db, Workload::Tools, 10);
        assert_eq!(rows(&db), vec![(300_000, 0, 0, 30_000)]);
    }

    /// gpt-6-luna: 20,173 prompt tokens of which 19,824 cached, 195 out:
    /// 349 x $0.10 + 19,824 x $0.01 + 195 x $0.50 per 1M = 331 micro-USD.
    #[test]
    fn cached_tokens_are_billed_at_the_cached_rate() {
        let db = temp_metrics_db("");
        let mut turn = TurnUsage::new("openai", "gpt-6-luna");
        turn.add_request(20_173, 195, 19_824, 0);
        turn.record(&db, Workload::Tools, 10);
        assert_eq!(rows(&db), vec![(20_173, 195, 19_824, 331)]);
    }

    #[test]
    fn a_failed_turn_stores_only_what_was_billed() {
        let db = temp_metrics_db("");
        TurnUsage::new("openai", "gpt-6-luna").record_if_billed(&db, Workload::Tools, 10);
        assert!(rows(&db).is_empty(), "a request that was never billed is not a row");
        let mut turn = TurnUsage::new("openai", "gpt-6-luna");
        turn.add_request(1_000, 10, 0, 0);
        turn.record_if_billed(&db, Workload::Tools, 10);
        assert_eq!(rows(&db), vec![(1_000, 10, 0, 105)]);
    }

    #[test]
    fn a_local_turn_is_stored_free_with_its_tokens() {
        let db = temp_metrics_db("");
        let mut turn = TurnUsage::new("local", "qwen3.5:4b");
        turn.add_request(4_000, 900, 0, 0);
        turn.record(&db, Workload::Tools, 10);
        assert_eq!(rows(&db), vec![(4_000, 900, 0, 0)]);
    }

    #[test]
    fn blended_empty_table_returns_zero_zero() {
        let db = temp_metrics_db("");
        let (total, days) = blended_cost_micro(&db, 0).unwrap();
        assert_eq!((total, days), (0, 0));
    }

    /// The shipped model ids must still be served by Google, not merely priced by
    /// us. `gemini-3-flash-preview` left the public pricing page on 2026-08-07 and
    /// nothing in CI would notice if it stopped resolving -- the failure would
    /// surface as every default user's first message erroring out.
    ///
    /// Network-dependent, so ignored by default like the real Keychain round trip.
    /// Run deliberately:
    ///   GEMINI_API_KEY=... cargo test --manifest-path src-tauri/Cargo.toml \
    ///     shipped_gemini_model_ids_are_still_served -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn shipped_gemini_model_ids_are_still_served() {
        let key = std::env::var("GEMINI_API_KEY")
            .expect("set GEMINI_API_KEY to run this test");

        // pageSize=200 is not paginated further; a miss here fails safe.
        let url = format!("{}/models?pageSize=200", crate::ai::gemini::API_BASE);
        let response = reqwest::Client::new()
            .get(url)
            .header("x-goog-api-key", &key)
            .send()
            .await
            .expect("models.list request failed");

        let status = response.status();
        let body = response
            .text()
            .await
            .expect("models.list body was not readable");

        assert!(status.is_success(), "models.list returned {status}: {body}");

        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("models.list body was not JSON");
        let served: Vec<String> = parsed["models"]
            .as_array()
            .expect("models.list had no models array")
            .iter()
            .filter_map(|m| m["name"].as_str())
            .map(|n| n.strip_prefix("models/").unwrap_or(n).to_string())
            .collect();

        // Collected across both ids and asserted once: MODEL_PRO is checked
        // first, and an assert! inside the loop would panic on a Pro miss
        // before MODEL_FLASH is ever checked -- hiding the Flash answer, which
        // is the whole reason this test exists.
        let missing: Vec<&str> = [crate::ai::models::GEMINI_PRO, crate::ai::models::GEMINI_FLASH]
            .into_iter()
            .filter(|id| !served.iter().any(|s| s == id))
            .collect();
        assert!(
            missing.is_empty(),
            "{missing:?} are pinned in gemini.rs but Google no longer serves them. \
             Served ids containing 'flash' or 'pro': {:?}",
            served
                .iter()
                .filter(|s| s.contains("flash") || s.contains("pro"))
                .collect::<Vec<_>>()
        );
    }
}
