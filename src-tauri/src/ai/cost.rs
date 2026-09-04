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
    } else if m.contains("gemini-3.5-flash-lite") {
        // Prices below for the 3.x Flash family read off Google's pricing page
        // 2026-08-07. Order matters: the -lite ids contain their non-lite
        // prefix, so they must match first.
        ModelPrice { input_per_million: 0.30, output_per_million: 2.50, cache_read_per_million: 0.075, cache_write_per_million: 0.30 }
    } else if m.contains("gemini-3.5-flash") {
        ModelPrice { input_per_million: 1.50, output_per_million: 9.0, cache_read_per_million: 0.375, cache_write_per_million: 1.50 }
    } else if m.contains("gemini-3.6-flash") {
        ModelPrice { input_per_million: 1.50, output_per_million: 7.50, cache_read_per_million: 0.375, cache_write_per_million: 1.50 }
    } else if m.contains("gemini-3.7-flash") {
        // Read off ai.google.dev/gemini-api/docs/pricing on 2026-09-04.
        // WARNING -- these are INTRODUCTORY rates that expire. Google lists
        // "$0.75 through December 31, 2026. $1.50 starting January 1, 2027."
        // for input and "$3.75 ... $7.50" for output: the rate DOUBLES on
        // 2027-01-01. Cache read goes $0.075 -> $0.15 and cached-content
        // storage $0.50 -> $1.00 per 1M tokens per hour on the same date.
        // Update these four numbers on that date or every blended-cost figure
        // for the default chain silently reads half of what it costs.
        ModelPrice { input_per_million: 0.75, output_per_million: 3.75, cache_read_per_million: 0.075, cache_write_per_million: 0.75 }
    } else if m.contains("gemini-3.1-flash-lite") {
        ModelPrice { input_per_million: 0.25, output_per_million: 1.50, cache_read_per_million: 0.0625, cache_write_per_million: 0.25 }
    } else if m.contains("gemini-3-flash") {
        // gemini-3-flash-preview is the configured MODEL_FLASH, but it no
        // longer appears on Google's public pricing page (checked 2026-08-07).
        // $0.50/$3.00 is the last known preview rate; verify the model id is
        // still served before relying on this number for a business claim.
        ModelPrice { input_per_million: 0.50, output_per_million: 3.0, cache_read_per_million: 0.125, cache_write_per_million: 0.50 }
    } else if m.contains("gemini-2.5-pro") || m.contains("gemini-pro") {
        ModelPrice { input_per_million: 1.25, output_per_million: 10.0, cache_read_per_million: 0.31, cache_write_per_million: 1.25 }
    } else if m.contains("gemini-2.5-flash") || m.contains("gemini-flash") {
        ModelPrice { input_per_million: 0.30, output_per_million: 2.50, cache_read_per_million: 0.075, cache_write_per_million: 0.30 }
    } else if m.contains("gpt-4o-mini") {
        ModelPrice { input_per_million: 0.15, output_per_million: 0.60, cache_read_per_million: 0.075, cache_write_per_million: 0.15 }
    } else if m.contains("gpt-4o") {
        ModelPrice { input_per_million: 2.50, output_per_million: 10.0, cache_read_per_million: 1.25, cache_write_per_million: 2.50 }
    } else if m.contains("gpt-5.6-luna") {
        // Read off developers.openai.com/api/docs/models/gpt-5.6-luna on
        // 2026-09-04: $0.20 input, $0.02 cached input, $1.20 output, and cache
        // writes at 1.25x uncached input ($0.25).
        //
        // This arm MUST stay above the `gpt-5` arm below: "gpt-5.6-luna"
        // contains "gpt-5", so ordering it after would silently bill Luna at
        // GPT-5's $1.25/$10.00 -- roughly 6x input and 8x output over the real
        // rate, with no test to catch it.
        //
        // Not modelled: Luna's long-context tier (2x input / 1.5x output above
        // 272K input tokens). `long_context_price_for` keys off a single
        // 200K threshold, which is the wrong boundary for this model, so a
        // >272K Luna request is currently billed at the base rate.
        ModelPrice { input_per_million: 0.20, output_per_million: 1.20, cache_read_per_million: 0.02, cache_write_per_million: 0.25 }
    } else if m.contains("gpt-5") {
        ModelPrice { input_per_million: 1.25, output_per_million: 10.0, cache_read_per_million: 0.125, cache_write_per_million: 1.25 }
    } else {
        // An unrecognised model must never price at zero. The match is
        // substring-based and brittle by design -- "gemini-3.5-flash" and
        // "gpt-5.1" match nothing here -- so the first symptom of a model-id
        // bump would otherwise be a cost dashboard confidently reporting
        // $0.00/mo, which is the exact failure 0d5e3b8 fixed for read errors.
        // Fall back to the most expensive tier in the table so the number errs
        // toward alarming rather than reassuring, and say so in the log.
        log::error!(
            "cost: no price entry for model '{model_id}' -- billing it at the \
             frontier rate ($3/$15 per 1M). Add it to price_for() or the \
             blended-cost report will overstate spend."
        );
        ModelPrice { input_per_million: 3.0, output_per_million: 15.0, cache_read_per_million: 0.30, cache_write_per_million: 3.75 }
    }
}

/// Requests whose prompt exceeds this are billed at the long-context rate by
/// providers that have one. "Triage 500 emails" lands here routinely, so
/// pricing it at the base rate understated exactly the workloads the
/// blended-cost report exists to measure.
const LONG_CONTEXT_THRESHOLD_TOKENS: i64 = 200_000;

/// The >200K-input price, for the models that publish one. Google's pricing
/// page (read 2026-08-07) doubles Gemini 3.1 Pro to $4/$18 and Gemini 2.5 Pro
/// to $2.50/$15 above 200K; current Anthropic models advertise no long-context
/// premium, and the Flash tiers publish a single rate.
fn long_context_price_for(model_id: &str) -> Option<ModelPrice> {
    let m = model_id.to_lowercase();
    if m.contains("gemini-3.1-pro") {
        Some(ModelPrice { input_per_million: 4.0, output_per_million: 18.0, cache_read_per_million: 1.0, cache_write_per_million: 4.0 })
    } else if m.contains("gemini-2.5-pro") {
        Some(ModelPrice { input_per_million: 2.50, output_per_million: 15.0, cache_read_per_million: 0.625, cache_write_per_million: 2.50 })
    } else {
        None
    }
}

pub fn compute_usd_cost_micro(
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
) -> i64 {
    let p = if input_tokens > LONG_CONTEXT_THRESHOLD_TOKENS {
        long_context_price_for(model_id).unwrap_or_else(|| price_for(model_id))
    } else {
        price_for(model_id)
    };
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
    // Inference on the user's own hardware costs us nothing. State that from
    // the provider, not from the model id missing out of the price table --
    // otherwise "free" and "we forgot to price this" are the same value, and
    // price_for's fallback (deliberately pessimistic) would bill every local
    // turn at the frontier rate.
    let usd_micro = if provider == "local" {
        0
    } else {
        compute_usd_cost_micro(
            model_id,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        )
    };
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

    /// A model we have no price for must cost something, and specifically must
    /// cost more than the cheapest known tier -- a silent $0 would make the
    /// blended-cost report read as free the moment a model id is bumped, and
    /// that report is the evidence the Gold Badge unit-economics claim rests on.
    #[test]
    fn unknown_model_is_billed_at_the_frontier_rate_not_zero() {
        let unknown = compute_usd_cost_micro("mystery-model", 1000, 1000, 0, 0);
        let frontier = compute_usd_cost_micro("claude-sonnet-4-6", 1000, 1000, 0, 0);
        assert_eq!(unknown, frontier, "unknown models price at the frontier rate");
        assert!(unknown > 0, "an unknown model must never price at zero");
    }

    /// The substring match is brittle on purpose (it has to catch vendor
    /// suffixes like -preview), so pin the ids actually shipped in gemini.rs.
    /// If either stops resolving, the fallback above starts firing.
    /// `price_for` is an ordered substring ladder, so a specific id that
    /// contains a more general one is only priced correctly while its arm sits
    /// above the general arm. "gpt-5.6-luna" contains "gpt-5"; move the Luna
    /// arm below the GPT-5 arm and every Luna call silently bills at
    /// $1.25/$10.00 instead of $0.20/$1.20 -- 6x input, 8x output, no error, no
    /// log, just a cost dashboard that is quietly wrong. A comment cannot catch
    /// that reordering; this test can.
    #[test]
    fn gpt_5_6_luna_is_priced_as_luna_and_not_as_generic_gpt_5() {
        let luna = price_for("gpt-5.6-luna");
        assert_eq!(luna.input_per_million, 0.20, "Luna input rate");
        assert_eq!(luna.output_per_million, 1.20, "Luna output rate");

        let generic = price_for("gpt-5");
        assert_ne!(
            luna.input_per_million, generic.input_per_million,
            "gpt-5.6-luna resolved to the generic gpt-5 price -- its arm in \
             price_for() has fallen below the `contains(\"gpt-5\")` arm"
        );
    }

    /// Same ladder hazard on the Gemini side: the shipped MODEL_FLASH must
    /// resolve to its own entry, not to a neighbouring Flash tier and not to
    /// the frontier fallback.
    #[test]
    fn the_shipped_flash_id_resolves_to_its_own_price_entry() {
        let p = price_for(crate::ai::gemini::MODEL_FLASH);
        assert_eq!(
            (p.input_per_million, p.output_per_million),
            (0.75, 3.75),
            "MODEL_FLASH ({}) did not resolve to the gemini-3.7-flash entry. \
             If the id was bumped, add its rate to price_for() -- note the \
             3.7 rate doubles to $1.50/$7.50 on 2027-01-01.",
            crate::ai::gemini::MODEL_FLASH
        );
    }

    #[test]
    fn the_shipped_gemini_model_ids_resolve_to_real_prices() {
        for id in [crate::ai::gemini::MODEL_PRO, crate::ai::gemini::MODEL_FLASH] {
            let p = price_for(id);
            assert!(
                p.input_per_million > 0.0 && p.input_per_million < 3.0,
                "{id} fell through to the frontier fallback -- add it to price_for()"
            );
        }
    }

    /// Google bills Gemini 3.1 Pro at $4/$18 above 200K input tokens (pricing
    /// page, 2026-08-07). A 250K-token triage priced at the base rate would
    /// understate cost by half on the exact workload ("summarise my whole
    /// inbox") most likely to produce it.
    #[test]
    fn gemini_pro_long_context_requests_bill_at_the_higher_tier() {
        let base = compute_usd_cost_micro("gemini-3.1-pro-preview", 100_000, 1_000, 0, 0);
        let long = compute_usd_cost_micro("gemini-3.1-pro-preview", 250_000, 1_000, 0, 0);
        // 100K @ $2/1M = $0.20; 250K @ $4/1M = $1.00 (plus identical output).
        assert!(long > base * 2, "long-context input must use the $4 tier, got {long} vs {base}");
        // Models without a published long-context tier keep their base rate.
        let flash_long = compute_usd_cost_micro("gemini-3.5-flash", 250_000, 0, 0, 0);
        assert_eq!(flash_long, (250_000f64 * 1.50 / 1_000_000.0 * 1_000_000.0).round() as i64);
    }

    /// A model-id bump to any current Flash-family id must not fall through to
    /// the frontier fallback -- that failure mode is exactly what the audit
    /// flagged: upgrade the id, and the dashboard silently overstates cost 6x.
    #[test]
    fn current_flash_family_ids_resolve_and_lite_orders_before_non_lite() {
        for (id, expected_input) in [
            ("gemini-3.6-flash", 1.50),
            ("gemini-3.5-flash", 1.50),
            ("gemini-3.5-flash-lite", 0.30),
            ("gemini-3.1-flash-lite", 0.25),
        ] {
            let p = price_for(id);
            assert!(
                (p.input_per_million - expected_input).abs() < f64::EPSILON,
                "{id}: expected ${expected_input}/1M input, got ${}",
                p.input_per_million
            );
        }
    }

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
        let missing: Vec<&str> = [crate::ai::gemini::MODEL_PRO, crate::ai::gemini::MODEL_FLASH]
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
