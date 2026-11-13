use crate::ai::Workload;
use std::cell::Cell;

fn workload_str(w: Workload) -> &'static str {
    match w {
        Workload::Volume => "volume",
        Workload::Tools => "tools",
        Workload::Premium => "premium",
    }
}

/// What a usage row was spent on, stored in `inference_metrics.feature` (V33).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// A chat turn, typed or spoken, that ran no tool.
    Chat,
    /// A chat turn in which the model ran at least one tool.
    Tools,
    MailAnalysis,
    /// Mail triage: sorting new mail into categories, with summaries and dates.
    MailSort,
    Briefing,
    Stt,
    Tts,
    /// Listed with the others for the registry work (P04-P07); no AI request
    /// is made for memory yet (the `remember` tool writes it during a chat).
    #[allow(dead_code)]
    Memory,
    /// Anything else, such as the one-word test when the user chooses an AI.
    Other,
}

impl Feature {
    pub fn as_str(self) -> &'static str {
        match self {
            Feature::Chat => "chat",
            Feature::Tools => "tools",
            Feature::MailAnalysis => "mail_analysis",
            Feature::MailSort => "mail_sort",
            Feature::Briefing => "briefing",
            Feature::Stt => "stt",
            Feature::Tts => "tts",
            Feature::Memory => "memory",
            Feature::Other => "other",
        }
    }
}

tokio::task_local! {
    /// The feature of the request `AiRouter` is running. The provider loops
    /// record usage deep inside the chain, where only this is in reach.
    static ROUTER_FEATURE: Cell<Feature>;
}

/// Run a provider-chain request for `feature`.
pub async fn with_feature<F: std::future::Future>(feature: Feature, request: F) -> F::Output {
    ROUTER_FEATURE.scope(Cell::new(feature), request).await
}

/// The feature of the provider-chain request in progress; Other outside one.
pub fn current_feature() -> Feature {
    ROUTER_FEATURE.try_with(Cell::get).unwrap_or(Feature::Other)
}

/// Called for every tool the model runs: a chat turn that runs one is
/// counted as Tools. A briefing that runs a tool stays a briefing.
pub fn note_tool_call() {
    let _ = ROUTER_FEATURE.try_with(|feature| {
        if feature.get() == Feature::Chat {
            feature.set(Feature::Tools);
        }
    });
}

/// The provider id stored with usage: the chain's provider type ("openai",
/// "claude", "gemini", "bedrock", "local"), or "compat:<host>" for an
/// OpenAI-compatible service, so Kimi's two regions and a custom URL stay
/// apart and every row of one service has one label.
pub fn provider_id(provider_type: &str, base_url: Option<&str>) -> String {
    match (provider_type, base_url) {
        ("openai", Some(base)) => format!("compat:{}", crate::ai::compat::host(base)),
        (other, _) => other.to_string(),
    }
}

/// Store one request. A local model is stored at 0 with its tokens; a cloud
/// model missing from the price list is stored with its tokens, the cost
/// NULL and `price_unknown` set (ai/models.rs).
#[allow(clippy::too_many_arguments)]
pub fn record_inference(
    db: &crate::db::Database,
    provider: &str,
    feature: Feature,
    workload: Workload,
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    latency_ms: i64,
) {
    let usd_micro = crate::ai::models::usd_cost_micro(provider, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens);
    let cost = stored_cost(provider, usd_micro);
    insert_row(db, provider, feature, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost, latency_ms);
}

/// Speech usage, which the price list cannot turn into money: MiniMax bills
/// per character, and Gemini bills audio input at its own rate. Stored as a
/// Volume row with the cost unknown; `input_units` is tokens for Gemini and
/// characters for MiniMax.
pub fn record_unpriced(db: &crate::db::Database, provider: &str, feature: Feature, model_id: &str, input_units: i64, output_tokens: i64, latency_ms: i64) {
    insert_row(db, provider, feature, Workload::Volume, model_id, input_units, output_tokens, 0, 0, StoredCost::Unknown, latency_ms);
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum StoredCost {
    Micro(i64),
    Unknown,
}

/// `usd_cost_micro` is None for a local model (free) and for a cloud model
/// without a price.
fn stored_cost(provider: &str, usd_micro: Option<i64>) -> StoredCost {
    match usd_micro {
        Some(micro) => StoredCost::Micro(micro),
        None if crate::ai::models::is_local(provider) => StoredCost::Micro(0),
        None => StoredCost::Unknown,
    }
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
    price_unknown: bool,
}

impl TurnUsage {
    pub fn new(provider: &str, model_id: &str) -> Self {
        TurnUsage { provider: provider.into(), model_id: model_id.into(), input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, usd_micro: 0, price_unknown: false }
    }

    /// Add one request's usage.
    pub fn add_request(&mut self, input_tokens: i64, output_tokens: i64, cache_read_tokens: i64, cache_write_tokens: i64) {
        self.input_tokens += input_tokens;
        self.output_tokens += output_tokens;
        self.cache_read_tokens += cache_read_tokens;
        self.cache_write_tokens += cache_write_tokens;
        let usd_micro = crate::ai::models::usd_cost_micro(&self.provider, &self.model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens);
        match stored_cost(&self.provider, usd_micro) {
            StoredCost::Micro(micro) => self.usd_micro += micro,
            StoredCost::Unknown => self.price_unknown = true,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0 && self.cache_read_tokens == 0 && self.cache_write_tokens == 0
    }

    /// Store the turn, under the feature of the request in progress. Taking
    /// `self` means a turn is stored at most once.
    pub fn record(self, db: &crate::db::Database, workload: Workload, latency_ms: i64) {
        let cost = if self.price_unknown { StoredCost::Unknown } else { StoredCost::Micro(self.usd_micro) };
        insert_row(db, &self.provider, current_feature(), workload, &self.model_id, self.input_tokens, self.output_tokens, self.cache_read_tokens, self.cache_write_tokens, cost, latency_ms);
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
    feature: Feature,
    workload: Workload,
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    cost: StoredCost,
    latency_ms: i64,
) {
    let (usd_micro, price_unknown) = match cost {
        StoredCost::Micro(micro) => (Some(micro), false),
        StoredCost::Unknown => (None, true),
    };
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
             provider, usd_cost_micro, feature, price_unknown)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            feature.as_str(),
            price_unknown,
        ],
    ) {
        log::warn!("cost: insert failed: {e}");
    } else {
        log::debug!(
            "cost: recorded provider={provider} feature={} model={model_id} workload={} in={input_tokens} out={output_tokens} usd_micro={usd_micro:?} latency_ms={latency_ms}",
            feature.as_str(),
            workload_str(workload)
        );
    }
}

/// The rate builds before V33 billed a model missing from the price list at:
/// US$3 / US$15 per 1M, cache read US$0.30, cache write US$3.75
/// (`UNKNOWN_PRICE` in ai/models.rs at 23058ce, and in ai/cost.rs from
/// 214481b, 2026-08-07). Before that day an unknown model was billed at zero.
const OLD_GUESS: crate::ai::models::Price = crate::ai::models::Price {
    input_per_million: 3.0,
    output_per_million: 15.0,
    cache_read_per_million: 0.30,
    cache_write_per_million: 3.75,
};

/// A chat turn row adds up its requests, each rounded to the micro-USD on
/// its own (`TurnUsage`, since d4b9631), and a turn takes at most five
/// requests, so a guessed turn can be up to 2.5 micro-USD from the guess on
/// its summed tokens.
const GUESS_ROUNDING_MICROS: i64 = 3;

/// Written to user_preferences once the guessed costs are cleared.
const GUESSES_CLEARED_KEY: &str = "usage_guessed_costs_cleared";

/// Rows stored before V33 for a model with no price kept the guessed cost,
/// with price_unknown = 0, so the totals counted money nobody was charged.
/// Once per database, straight after the migrations (db.rs) and so before
/// anything reads the totals or rolls a row into the daily totals
/// (ai/usage.rs, which also runs this first), such a row loses that cost: usd_cost_micro
/// becomes NULL and price_unknown 1. Its tokens, time, provider, model,
/// workload and feature are kept.
///
/// A row is cleared only when all of these hold:
/// - price_unknown = 0, and a stored cost above zero (no money, no guess);
/// - today's lookup has no price for it: the `usd_cost_micro` and
///   `stored_cost` a new row goes through, so a local row (free) and a row
///   whose model has a price today are never touched;
/// - its stored cost is what the old guess gives for its tokens, within
///   rounding. The price list once narrowed (7f9fd11, 2026-10-07:
///   "sonnet-4-6" became "claude-sonnet-4-6", and the same for haiku-4-5,
///   haiku-5-5, opus-5-5 and sonnet-5-5), so an id only the old pattern
///   matched was stored at a real rate and has no price today. Such a row is
///   kept unless its cost is the guess, which for Sonnet 4.6 it also was.
///
/// Rows billed at zero by the fallback before 214481b are not touched: rows
/// from before V16 carry provider 'unknown' and a cost of zero too, and a
/// free local row among them cannot be told apart.
///
/// Running it again changes nothing: the key returns at once, and without it
/// a cleared row no longer qualifies. Returns how many rows were cleared.
pub fn clear_guessed_costs(conn: &rusqlite::Connection) -> rusqlite::Result<usize> {
    let done: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM user_preferences WHERE key = ?1)", [GUESSES_CLEARED_KEY], |r| r.get(0))?;
    if done {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let models: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT DISTINCT provider, model_id FROM inference_metrics WHERE price_unknown = 0 AND usd_cost_micro > 0")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut cleared = 0;
    for (provider, model_id) in models {
        // Asked once per model, the way record_inference asks for a new row.
        if stored_cost(&provider, crate::ai::models::usd_cost_micro(&provider, &model_id, 0, 0, 0, 0)) != StoredCost::Unknown {
            continue;
        }
        let rows: Vec<(i64, i64, i64, i64, i64, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro
                 FROM inference_metrics
                 WHERE provider = ?1 AND model_id = ?2 AND price_unknown = 0 AND usd_cost_micro > 0",
            )?;
            let rows = stmt.query_map(rusqlite::params![provider, model_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (id, input, output, cache_read, cache_write, stored) in rows {
            let guess = crate::ai::models::cost_micro_at(OLD_GUESS, input, output, cache_read, cache_write);
            if (stored - guess).abs() <= GUESS_ROUNDING_MICROS {
                cleared += tx.execute("UPDATE inference_metrics SET usd_cost_micro = NULL, price_unknown = 1 WHERE id = ?1", [id])?;
            }
        }
    }
    tx.execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES (?1, 'true')", [GUESSES_CLEARED_KEY])?;
    tx.commit()?;
    Ok(cleared)
}

/// `clear_guessed_costs` as the app runs it when it opens the database. A
/// failure is logged and tried again at the next start; it never stops
/// JARVIS from opening.
pub fn clear_guessed_costs_on_open(conn: &rusqlite::Connection) {
    match clear_guessed_costs(conn) {
        Ok(0) => {}
        Ok(cleared) => log::info!("cost: {cleared} usage rows billed at the old guessed rate now count as price unknown"),
        Err(e) => log::warn!("cost: could not clear the old guessed costs; trying again at the next start: {e}"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// In-memory sqlite seeded with rows_sql. Nothing here needs a file on disk,
    /// and a file-backed fixture leaks its .db/-wal/-shm into $TMPDIR whenever an
    /// assertion fails before the cleanup line -- which is exactly the state the
    /// fixture is in while someone is iterating on it.
    /// Mirrors the V33 inference_metrics DDL (minus id/NOT NULL/CHECK) --
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
                latency_ms INTEGER, provider TEXT, usd_cost_micro INTEGER,
                feature TEXT, price_unknown INTEGER);
             {rows_sql}"
        ))
        .unwrap();
        crate::db::Database { conn: Mutex::new(conn) }
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

    /// One usage row as the usage panel reads it.
    #[derive(Debug, PartialEq)]
    pub(crate) struct UsageRow {
        pub provider: String,
        pub feature: String,
        pub model_id: String,
        pub input_tokens: i64,
        pub output_tokens: i64,
        pub usd_cost_micro: Option<i64>,
        pub price_unknown: bool,
    }

    /// Every column of one row: (id, ts, workload, model, input, output) and
    /// (cache read, cache write, latency, provider, cost, feature, price unknown).
    type Snapshot = ((i64, i64, String, String, i64, i64), (i64, i64, i64, String, Option<i64>, String, i64));

    /// Every column of every row, in order, to compare before and after.
    fn snapshot(conn: &rusqlite::Connection) -> Vec<Snapshot> {
        let mut stmt = conn
            .prepare("SELECT id, ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown FROM inference_metrics ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?),
                    (r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?),
                ))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    }

    /// The old guess for one request, as main computed it.
    fn guessed(input: i64, output: i64, cache_read: i64, cache_write: i64) -> i64 {
        crate::ai::models::cost_micro_at(OLD_GUESS, input, output, cache_read, cache_write)
    }

    /// M2 / D5. A row an older build billed at the guessed rate for a model
    /// that has no price today loses that cost and counts as price unknown;
    /// every other row stays as it was, and a second run changes nothing.
    #[test]
    fn old_guessed_costs_become_price_unknown_once_and_nothing_else_changes() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        // Five requests of one chat turn, each rounded on its own (TurnUsage):
        // 1,500 stored, where the guess on the summed tokens is 1,502.
        let turn: i64 = (0..5).map(|_| guessed(101, 0, 1, 0)).sum();
        assert_eq!((turn, guessed(505, 0, 5, 0)), (1_500, 1_502), "the turn's rounding differs from the guess on its sums");
        let kimi = guessed(12_000, 800, 2_000, 0);
        assert_eq!(kimi, 42_600);
        let rows = [
            // 1: Kimi, billed at the guess before V33.
            ("compat:api.moonshot.ai", "kimi-k2-0905-preview", 12_000, 800, 2_000, 0, Some(kimi), "chat", 0),
            // 2: Claude Sonnet 4.6, whose real price is the guess rate: priced today.
            ("claude", "claude-sonnet-4-6", 12_000, 800, 2_000, 0, Some(kimi), "chat", 0),
            // 3: a local model, free.
            ("local", "qwen3:4b", 12_000, 800, 0, 0, Some(0), "chat", 0),
            // 4: a local provider that, whatever its cost, is never touched.
            ("ollama", "llama3.2", 12_000, 800, 2_000, 0, Some(kimi), "mail_analysis", 0),
            // 5: an id only the pre-7f9fd11 pattern "haiku-4-5" priced, at
            // Haiku 4.5's real rate: no price today, and not the guess.
            ("claude", "haiku-4-5-20251001", 12_000, 800, 2_000, 0, Some(14_200), "chat", 0),
            // 6: a guessed turn of five requests on Qwen.
            ("compat:dashscope-intl.aliyuncs.com", "qwen3-max", 505, 0, 5, 0, Some(turn), "tools", 0),
            // 7: stored since V33 with the price unknown.
            ("compat:api.moonshot.ai", "kimi-k2-0905-preview", 3_000, 10, 0, 0, None, "chat", 1),
            // 8: from before V16: no provider and no cost.
            ("unknown", "qwen3:4b", 4_000, 900, 0, 0, Some(0), "other", 0),
        ];
        for (provider, model, input, output, cache_read, cache_write, cost, feature, unknown) in rows {
            conn.execute(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown)
                 VALUES (1762900000, 'tools', ?1, ?2, ?3, ?4, ?5, 40, ?6, ?7, ?8, ?9)",
                rusqlite::params![model, input, output, cache_read, cache_write, provider, cost, feature, unknown],
            )
            .unwrap();
        }
        let before = snapshot(&conn);

        assert_eq!(clear_guessed_costs(&conn).unwrap(), 2);
        let after = snapshot(&conn);
        assert_eq!(after.len(), before.len());
        for (i, (old, new)) in before.iter().zip(&after).enumerate() {
            let row = i + 1;
            if row == 1 || row == 6 {
                // Only the cost and the flag change: tokens, time, provider,
                // model, workload and feature are as they were.
                let mut expected = old.clone();
                expected.1 .4 = None;
                expected.1 .6 = 1;
                assert_eq!(new, &expected, "row {row}");
            } else {
                assert_eq!(new, old, "row {row} is not touched");
            }
        }

        // A second run changes nothing, and neither does one without the key.
        assert_eq!(clear_guessed_costs(&conn).unwrap(), 0);
        assert_eq!(snapshot(&conn), after);
        conn.execute("DELETE FROM user_preferences WHERE key = ?1", [GUESSES_CLEARED_KEY]).unwrap();
        assert_eq!(clear_guessed_costs(&conn).unwrap(), 0);
        assert_eq!(snapshot(&conn), after);
    }

    /// The step runs when the app opens its database: after the migrations,
    /// first of the usage steps, before the database is handed to anything
    /// that reads the totals (the roll-up's order: ai/usage.rs).
    #[test]
    fn the_guessed_costs_are_cleared_when_the_database_opens() {
        let source = include_str!("../db.rs");
        let open = &source[source.find("pub fn new()").expect("Database::new exists")..];
        let open = &open[..open.find("fn db_path()").expect("db_path follows new")];
        let migrated = open.find("embedded::migrations::runner().run(&mut conn)?;").expect("migrations run");
        let prepared = open.find("prepare_usage(&conn);").expect("the usage steps run on open");
        let handed_over = open.find("Ok(Database {").expect("the database is returned");
        assert!(migrated < prepared && prepared < handed_over);
        let steps = &source[source.find("pub(crate) fn prepare_usage(conn: &Connection) {").expect("the usage steps")..];
        let first = steps.lines().nth(1).unwrap_or_default().trim();
        assert_eq!(first, "crate::ai::cost::clear_guessed_costs_on_open(conn);");
    }

    /// A failure is logged, never fatal: JARVIS still opens.
    #[test]
    fn a_failed_clearing_does_not_stop_the_database_opening() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(clear_guessed_costs(&conn).is_err(), "no tables");
        clear_guessed_costs_on_open(&conn);
    }

    /// (cache read, cache write) of each row, in order.
    pub(crate) fn cache_tokens(db: &crate::db::Database) -> Vec<(i64, i64)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT cache_read_tokens, cache_write_tokens FROM inference_metrics ORDER BY rowid").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    pub(crate) fn usage_rows(db: &crate::db::Database) -> Vec<UsageRow> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT provider, feature, model_id, input_tokens, output_tokens, usd_cost_micro, price_unknown FROM inference_metrics ORDER BY rowid")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok(UsageRow {
                    provider: r.get(0)?,
                    feature: r.get(1)?,
                    model_id: r.get(2)?,
                    input_tokens: r.get(3)?,
                    output_tokens: r.get(4)?,
                    usd_cost_micro: r.get(5)?,
                    price_unknown: r.get(6)?,
                })
            })
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn features(db: &crate::db::Database) -> Vec<String> {
        usage_rows(db).into_iter().map(|row| row.feature).collect()
    }

    fn one_turn(db: &crate::db::Database) {
        let mut turn = TurnUsage::new("openai", "gpt-6-luna");
        turn.add_request(1_000, 10, 0, 0);
        turn.record(db, Workload::Tools, 1);
    }

    /// The providers record deep in the chain; the feature comes from the
    /// request that started it.
    #[tokio::test]
    async fn a_chain_turn_is_stored_under_the_feature_of_its_request() {
        let db = temp_metrics_db("");
        with_feature(Feature::Chat, async { one_turn(&db) }).await;
        with_feature(Feature::Briefing, async { one_turn(&db) }).await;
        one_turn(&db);
        assert_eq!(features(&db), vec!["chat", "briefing", "other"]);
    }

    /// A chat turn that ran a tool is "tools"; a briefing that ran one is
    /// still a briefing.
    #[tokio::test]
    async fn a_chat_turn_that_ran_a_tool_is_stored_as_tools() {
        let db = temp_metrics_db("");
        with_feature(Feature::Chat, async {
            note_tool_call();
            one_turn(&db);
        })
        .await;
        with_feature(Feature::Briefing, async {
            note_tool_call();
            one_turn(&db);
        })
        .await;
        note_tool_call();
        assert_eq!(features(&db), vec!["tools", "briefing"]);
    }

    #[test]
    fn every_feature_has_its_stored_name() {
        let all = [Feature::Chat, Feature::Tools, Feature::MailAnalysis, Feature::MailSort, Feature::Briefing, Feature::Stt, Feature::Tts, Feature::Memory, Feature::Other];
        let names: Vec<&str> = all.iter().map(|feature| feature.as_str()).collect();
        assert_eq!(names, vec!["chat", "tools", "mail_analysis", "mail_sort", "briefing", "stt", "tts", "memory", "other"]);
    }

    #[test]
    fn usage_is_labelled_by_provider_id() {
        assert_eq!(provider_id("openai", None), "openai");
        assert_eq!(provider_id("claude", None), "claude");
        assert_eq!(provider_id("gemini", None), "gemini");
        assert_eq!(provider_id("bedrock", None), "bedrock");
        assert_eq!(provider_id("local", Some("http://localhost:11434")), "local");
        assert_eq!(provider_id("openai", Some("https://api.moonshot.ai/v1")), "compat:api.moonshot.ai");
        assert_eq!(provider_id("openai", Some("https://API.DeepSeek.com/v1")), "compat:api.deepseek.com");
    }

    /// A model the price list does not know is stored with its tokens and no
    /// cost, never at a guessed rate.
    #[test]
    fn an_unknown_model_is_stored_with_the_price_unknown() {
        let db = temp_metrics_db("");
        record_inference(&db, "openai", Feature::MailSort, Workload::Volume, "gpt-unlisted-1", 2_000, 100, 0, 0, 1);
        let mut turn = TurnUsage::new("compat:api.deepseek.com", "deepseek-unlisted");
        turn.add_request(500, 50, 0, 0);
        turn.add_request(500, 50, 0, 0);
        turn.record(&db, Workload::Tools, 1);
        let rows = usage_rows(&db);
        assert_eq!(rows[0], UsageRow { provider: "openai".into(), feature: "mail_sort".into(), model_id: "gpt-unlisted-1".into(), input_tokens: 2_000, output_tokens: 100, usd_cost_micro: None, price_unknown: true });
        assert_eq!((rows[1].input_tokens, rows[1].usd_cost_micro, rows[1].price_unknown), (1_000, None, true));
    }

    /// One unpriced request makes the whole turn's cost unknown: a partial
    /// sum would read as the full price.
    #[test]
    fn a_turn_with_any_unpriced_request_has_no_cost() {
        let db = temp_metrics_db("");
        let mut turn = TurnUsage::new("openai", "gpt-unlisted-1");
        turn.add_request(1_000, 10, 0, 0);
        turn.record(&db, Workload::Tools, 1);
        assert_eq!((usage_rows(&db)[0].usd_cost_micro, usage_rows(&db)[0].price_unknown), (None, true));
    }

    #[test]
    fn speech_is_stored_with_the_price_unknown() {
        let db = temp_metrics_db("");
        record_unpriced(&db, "minimax", Feature::Tts, "speech-02-hd", 12, 0, 1);
        assert_eq!(usage_rows(&db), vec![UsageRow { provider: "minimax".into(), feature: "tts".into(), model_id: "speech-02-hd".into(), input_tokens: 12, output_tokens: 0, usd_cost_micro: None, price_unknown: true }]);
    }

    /// V33 on a database with usage from v0.5.11: every row becomes 'other',
    /// labels collapse to the provider id, costs are kept, and the new
    /// columns hold an unknown price.
    #[test]
    fn v33_backfills_the_feature_and_normalises_provider_labels() {
        let directory = std::env::temp_dir().join(format!("jarvis-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("usage.db");
        {
            let db = crate::db::test_database_at_version(&path, 32);
            db.conn.lock().unwrap().execute_batch(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro) VALUES
                    (1, 'tools', 'gpt-6-luna', 10, 1, 5, 'openai', 7),
                    (2, 'volume', 'gpt-6-luna', 20, 2, 5, 'openai-mail', 8),
                    (3, 'volume', 'deepseek-chat', 30, 3, 5, 'api.deepseek.com', 9),
                    (4, 'tools', 'llama3', 40, 4, 5, 'localhost', 10),
                    (5, 'tools', 'kimi-k2', 50, 5, 5, 'kimi', 11),
                    (6, 'tools', 'claude-sonnet-4-6', 60, 6, 5, 'claude', 12),
                    (7, 'volume', 'qwen3.5:4b', 70, 7, 5, 'local', 0),
                    (8, 'volume', 'jev', 80, 0, 5, 'typesafe', 13);",
            ).unwrap();
            db.conn.lock().unwrap().execute("INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms) VALUES (9, 'volume', 'old', 1, 1, 1)", []).unwrap();
        }
        let db = crate::db::test_database_at(&path);
        let rows = usage_rows(&db);
        let providers: Vec<&str> = rows.iter().map(|row| row.provider.as_str()).collect();
        assert_eq!(providers, vec!["openai", "openai", "compat:api.deepseek.com", "compat:localhost", "kimi", "claude", "local", "typesafe", "unknown"]);
        assert!(rows.iter().all(|row| row.feature == "other" && !row.price_unknown), "{rows:?}");
        let costs: Vec<Option<i64>> = rows.iter().map(|row| row.usd_cost_micro).collect();
        assert_eq!(costs, vec![Some(7), Some(8), Some(9), Some(10), Some(11), Some(12), Some(0), Some(13), Some(0)]);

        // The new rows: an unknown price is NULL, and only then.
        record_inference(&db, "openai", Feature::Chat, Workload::Tools, "gpt-unlisted-1", 1, 1, 0, 0, 1);
        assert_eq!(usage_rows(&db).last().map(|row| (row.usd_cost_micro, row.price_unknown)), Some((None, true)));
        let conn = db.conn.lock().unwrap();
        assert!(
            conn.execute("INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, usd_cost_micro, price_unknown) VALUES (1, 'tools', 'x', 1, 1, 1, 5, 1)", []).is_err(),
            "a row cannot carry both a cost and an unknown price"
        );
        let indexes: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_inference_metrics_ts', 'idx_inference_metrics_provider_ts')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(indexes, 2);
        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&directory).unwrap();
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
