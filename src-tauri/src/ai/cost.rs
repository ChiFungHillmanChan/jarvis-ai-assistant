use crate::ai::Workload;
use std::cell::Cell;

fn workload_str(w: Workload) -> &'static str {
    match w {
        Workload::Volume => "volume",
        Workload::Tools => "tools",
        Workload::Premium => "premium",
    }
}

/// What a usage row was spent on, stored in `inference_metrics.feature` (V34).
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
/// NULL and `price_unknown` set (ai/models.rs). Priced with the registry in
/// use now, whose version the row records (V35).
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
    let registry = crate::ai::registry::current();
    let usd_micro = registry.usd_cost_micro(provider, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens);
    let cost = stored_cost(provider, usd_micro);
    insert_row(db, provider, feature, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost, latency_ms, registry.version);
}

/// Speech usage, which the price list cannot turn into money: MiniMax bills
/// per character, and Gemini bills audio input at its own rate. Stored as a
/// Volume row with the cost unknown; `input_units` is tokens for Gemini and
/// characters for MiniMax.
pub fn record_unpriced(db: &crate::db::Database, provider: &str, feature: Feature, model_id: &str, input_units: i64, output_tokens: i64, latency_ms: i64) {
    let version = crate::ai::registry::current().version;
    insert_row(db, provider, feature, Workload::Volume, model_id, input_units, output_tokens, 0, 0, StoredCost::Unknown, latency_ms, version);
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
///
/// The registry in use when the turn starts prices every request of it, and
/// the row records that registry's version: a registry fetched mid-turn
/// never splits one turn across two price lists.
pub struct TurnUsage {
    registry: std::sync::Arc<crate::ai::registry::Registry>,
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
        TurnUsage { registry: crate::ai::registry::current(), provider: provider.into(), model_id: model_id.into(), input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, usd_micro: 0, price_unknown: false }
    }

    /// Add one request's usage.
    pub fn add_request(&mut self, input_tokens: i64, output_tokens: i64, cache_read_tokens: i64, cache_write_tokens: i64) {
        self.input_tokens += input_tokens;
        self.output_tokens += output_tokens;
        self.cache_read_tokens += cache_read_tokens;
        self.cache_write_tokens += cache_write_tokens;
        let usd_micro = self.registry.usd_cost_micro(&self.provider, &self.model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens);
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
        insert_row(db, &self.provider, current_feature(), workload, &self.model_id, self.input_tokens, self.output_tokens, self.cache_read_tokens, self.cache_write_tokens, cost, latency_ms, self.registry.version);
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
    registry_version: u64,
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
             provider, usd_cost_micro, feature, price_unknown, registry_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            registry_version as i64,
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

/// The rate builds before V34 billed a model missing from the price list at:
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
/// requests. The sum of the rounded guesses is within 2.5 micro-USD of the
/// unrounded guess on the summed tokens, and so within 3 of that guess once
/// it is rounded, which is what the step compares with.
const GUESS_ROUNDING_MICROS: i64 = 3;

/// Before this time no build stored Bedrock input with the cache included.
/// The fix that started storing it that way, as every other provider's is,
/// is d4b9631 ("各 AI 供應商的 token 用量按實際計費記錄", authored
/// 2026-10-31T01:02:08Z; it reached main rebased, with the commit time
/// 2026-11-12T16:57:10Z, and shipped in v0.5.11). It was written on top of
/// a1e44e9 ("Merge #23"), main's head from 2026-10-29 until 2026-11-07,
/// and this is a1e44e9's author time, 2026-10-29T21:20:41Z: no build of the
/// fix, a development build of the uncommitted change included, can be
/// older than the commit it was written on.
/// What it leaves uncorrected: the Bedrock rows released builds up to
/// v0.5.10 stored between that time and the user's update to v0.5.11 or
/// later whose input is at least their cache reads and writes; they are
/// summed as stored, so their input, and the cost worked out from it, read
/// low by the cache (DECISIONS.md, 2026-11-13: one constant to change).
const INCLUSIVE_INPUT_SINCE: i64 = 1_793_308_841; // a1e44e9: `git log -1 --format=%at a1e44e9`

/// Written to user_preferences, in the same transaction as the corrections,
/// once the old rows are corrected.
const OLD_ROWS_CORRECTED_KEY: &str = "usage_old_rows_corrected";

/// What [`correct_old_rows`] changed, in rows, rule by rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Corrected {
    /// 1: billed at the old guess; now price unknown.
    pub guessed: usize,
    /// 2: billed at zero for a model with no price; now price unknown.
    pub billed_at_zero: usize,
    /// 3: a local model's row that held a cost; now free.
    pub local_made_free: usize,
    /// 4: Bedrock input stored without the cache; now with it.
    pub cache_added: usize,
}

/// Rows stored by earlier builds that the totals would misread, corrected
/// once per database, straight after the migrations (db.rs) and so before
/// anything reads the totals or rolls a row into the daily totals
/// (ai/usage.rs, which also runs this first: a daily total is never
/// corrected afterwards). Only the columns named below change; time,
/// provider, model, workload, feature, latency and the other tokens are kept.
///
/// 1. Not local, no price today, price_unknown = 0, and a stored cost above
///    zero that is what the old guess gives for its tokens, within rounding:
///    usd_cost_micro becomes NULL and price_unknown 1 (founder, 2026-11-13).
///    The price list once narrowed (7f9fd11, 2026-10-07: "sonnet-4-6" became
///    "claude-sonnet-4-6", and the same for haiku-4-5, haiku-5-5, opus-5-5
///    and sonnet-5-5), so an id only the old pattern matched was stored at a
///    real rate and has no price today. Such a row keeps its cost unless the
///    cost is the guess, which for Sonnet 4.6 it also was.
/// 2. Not local, a provider on record (not the pre-V16 'unknown', whose
///    rows all cost zero and may be local), no price today, price_unknown =
///    0 and a stored cost of zero: price unknown the same way. These are a
///    compatible service that sent no usage (zero tokens), and usage billed
///    at zero before 214481b gave an unknown model the guess.
/// 3. A local model's row with a cost above zero (stored as 'ollama' or
///    '本機 ai' by v0.5.0 to v0.5.4, which priced only 'local' as free): the
///    cost becomes 0 with price_unknown 0. Local work is free, not unknown.
/// 4. A Bedrock row with cache tokens that is proven to be from before the
///    cache fix. Up to v0.5.10 Bedrock stored AWS's inputTokens, which leaves
///    the cache out, and the cost then took the cache out of it again, so the
///    cost was too low. Proven old: stored before [`INCLUSIVE_INPUT_SINCE`],
///    or with input smaller than its cache reads and writes, which a row
///    stored the new way can never be. The input becomes the sum of input,
///    cache read and cache write, the convention new rows use; when the
///    row's price is known today its cost becomes what a new row with those
///    tokens gets (`models::usd_cost_micro`). Otherwise rules 1 and 2 have
///    decided the cost. A row that cannot be proven old, and one without
///    cache tokens (the bug did not touch it), is left alone. Every release
///    stored one Bedrock row per request, so the price, a long-context tier
///    included, applies to one request, as it does to a new row.
///    A Claude row is never changed by this rule, whatever it holds. No
///    release up to v0.5.12 asked Anthropic to cache, so the Claude rows
///    those releases stored hold no cache figures and their costs were
///    right. Chat turns ask from the build that added ai/claude.rs
///    `system_blocks` on, and those rows hold the cache in their input, as
///    every new row does (`RoundUsage::figures`).
///
/// Rules 1 to 3 decide the cost on the tokens as stored, which is what the
/// old guess was worked out on; rule 4 then corrects the input. A row that
/// matches rule 1 or 2 and rule 4 ends with the price unknown and the cache
/// in its input.
///
/// Once: a row corrected by rules 1 to 3 no longer matches them, but rule 4
/// on a row from before the cutoff is not repeatable: run again, it would
/// add the cache a second time. Exactly once therefore rests on the marker,
/// which is written in the same transaction as every correction: a failure
/// anywhere leaves every row and the marker as they were, and the whole
/// step runs again at the next open.
///
/// The price list: the step reads [`FrozenPrices`] only, never the
/// registry (bundled, cached or fetched), and must keep doing so. It runs
/// once, at the first open of whichever release a user installs next, so
/// its result must depend neither on what was fetched or when nor on which
/// release that is: a user who skips a release gets the same result
/// (DECISIONS.md, "P04 and P05:"; the lead's ruling of 2026-11-17).
pub fn correct_old_rows(conn: &rusqlite::Connection) -> rusqlite::Result<Corrected> {
    let frozen = FrozenPrices::get();
    let is_local = |provider: &str| frozen.is_local(provider);
    let price_known = |provider: &str, model_id: &str| frozen.price_known(provider, model_id);
    let usd_cost_micro = |provider: &str, model_id: &str, input: i64, output: i64, cache_read: i64, cache_write: i64| frozen.usd_cost_micro(provider, model_id, input, output, cache_read, cache_write);
    let done: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM user_preferences WHERE key = ?1)", [OLD_ROWS_CORRECTED_KEY], |r| r.get(0))?;
    if done {
        return Ok(Corrected::default());
    }
    let tx = conn.unchecked_transaction()?;
    let mut corrected = Corrected::default();

    // Rules 1 and 2, asked once per model. `price_known` is the test a new
    // row goes through, without the warning `price_for` logs; a local
    // model is priced (free), so neither rule touches it.
    let models: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT DISTINCT provider, model_id FROM inference_metrics WHERE price_unknown = 0 AND usd_cost_micro >= 0")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (provider, model_id) in models {
        if price_known(&provider, &model_id) {
            continue;
        }
        if provider != "unknown" {
            corrected.billed_at_zero += tx.execute(
                "UPDATE inference_metrics SET usd_cost_micro = NULL, price_unknown = 1
                 WHERE provider = ?1 AND model_id = ?2 AND price_unknown = 0 AND usd_cost_micro = 0",
                [&provider, &model_id],
            )?;
        }
        let rows: Vec<(i64, i64, i64, i64, i64, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro
                 FROM inference_metrics
                 WHERE provider = ?1 AND model_id = ?2 AND price_unknown = 0 AND usd_cost_micro > 0",
            )?;
            let rows = stmt.query_map([&provider, &model_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (id, input, output, cache_read, cache_write, stored) in rows {
            let guess = crate::ai::models::cost_micro_at(OLD_GUESS, input, output, cache_read, cache_write);
            if (stored - guess).abs() <= GUESS_ROUNDING_MICROS {
                corrected.guessed += tx.execute("UPDATE inference_metrics SET usd_cost_micro = NULL, price_unknown = 1 WHERE id = ?1", [id])?;
            }
        }
    }

    // Rule 3.
    let providers: Vec<String> = {
        let mut stmt = tx.prepare("SELECT DISTINCT provider FROM inference_metrics WHERE price_unknown = 0 AND usd_cost_micro > 0")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for provider in providers.iter().filter(|provider| is_local(provider)) {
        corrected.local_made_free += tx.execute(
            "UPDATE inference_metrics SET usd_cost_micro = 0 WHERE provider = ?1 AND price_unknown = 0 AND usd_cost_micro > 0",
            [provider],
        )?;
    }

    // Rule 4, after the costs above were decided on the stored tokens.
    // (id, provider, model, input, output, cache read, cache write, price unknown)
    type Stored = (i64, String, String, i64, i64, i64, i64, bool);
    let rows: Vec<Stored> = {
        let mut stmt = tx.prepare(
            "SELECT id, provider, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, price_unknown
             FROM inference_metrics
             WHERE provider = 'bedrock' AND cache_read_tokens + cache_write_tokens > 0
               AND (ts < ?1 OR input_tokens < cache_read_tokens + cache_write_tokens)",
        )?;
        let rows = stmt.query_map([INCLUSIVE_INPUT_SINCE], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (id, provider, model_id, input, output, cache_read, cache_write, price_unknown) in rows {
        let input = input + cache_read + cache_write;
        let cost = if price_unknown || !price_known(&provider, &model_id) { None } else { usd_cost_micro(&provider, &model_id, input, output, cache_read, cache_write) };
        corrected.cache_added += match cost {
            Some(micro) => tx.execute("UPDATE inference_metrics SET input_tokens = ?2, usd_cost_micro = ?3 WHERE id = ?1", [id, input, micro])?,
            None => tx.execute("UPDATE inference_metrics SET input_tokens = ?2 WHERE id = ?1", [id, input])?,
        };
    }

    tx.execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES (?1, 'true')", [OLD_ROWS_CORRECTED_KEY])?;
    tx.commit()?;
    Ok(corrected)
}

/// `correct_old_rows` as the app runs it when it opens the database,
/// logging counts only. A failure is logged and never stops JARVIS from
/// opening. Every roll-up (ai/usage.rs `roll_up`, the first of them straight
/// after this one) runs the step again first and moves no request while it
/// fails.
pub fn correct_old_rows_on_open(conn: &rusqlite::Connection) {
    match correct_old_rows(conn) {
        Ok(corrected) if corrected == Corrected::default() => {}
        Ok(Corrected { guessed, billed_at_zero, local_made_free, cache_added }) => log::info!(
            "cost: old usage rows corrected: {guessed} billed at the guessed rate and {billed_at_zero} billed at zero now count as price unknown, {local_made_free} local rows are free, {cache_added} Bedrock rows now count their cache in the input"
        ),
        Err(e) => log::warn!("cost: could not correct the old usage rows; every roll-up tries again first and moves no request until it succeeds: {e}"),
    }
}

/// The price list of [`correct_old_rows`] and of nothing else:
/// old_rows_prices.frozen.json, the priced entries of registry/registry.json
/// version 1 with the lookup rule they were read with. The file never
/// changes (`the_frozen_price_list_is_never_edited`), and neither the
/// registry's format nor its lookup reaches it, so every release corrects
/// old rows the same way.
#[derive(serde::Deserialize)]
struct FrozenPrices {
    local_providers: Vec<String>,
    models: Vec<FrozenModel>,
}

#[derive(serde::Deserialize)]
struct FrozenModel {
    id: String,
    input: f64,
    output: f64,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
    tiers: Vec<FrozenTier>,
}

#[derive(serde::Deserialize)]
struct FrozenTier {
    above_input_tokens: i64,
    input: f64,
    output: f64,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
}

impl FrozenPrices {
    /// Parsed once. The file is compiled in and its bytes are pinned, and
    /// `the_frozen_prices_are_version_1_of_the_registry` parses it, so this
    /// cannot fail in a build whose tests passed.
    fn get() -> &'static FrozenPrices {
        static FROZEN: std::sync::LazyLock<FrozenPrices> = std::sync::LazyLock::new(|| serde_json::from_str(include_str!("old_rows_prices.frozen.json")).expect("the frozen price list parses"));
        &FROZEN
    }

    fn is_local(&self, provider: &str) -> bool {
        let provider = provider.to_lowercase();
        self.local_providers.contains(&provider)
    }

    /// The entry whose id is the longest one contained in the lowercased id.
    fn find(&self, model_id: &str) -> Option<&FrozenModel> {
        let wanted = model_id.to_lowercase();
        self.models.iter().filter(|m| wanted.contains(m.id.as_str())).max_by_key(|m| m.id.len())
    }

    fn price_known(&self, provider: &str, model_id: &str) -> bool {
        self.is_local(provider) || self.find(model_id).is_some()
    }

    /// None for a local model (free) and for an unpriced one. The highest
    /// tier the input exceeds bills the whole request.
    fn usd_cost_micro(&self, provider: &str, model_id: &str, input_tokens: i64, output_tokens: i64, cache_read_tokens: i64, cache_write_tokens: i64) -> Option<i64> {
        if self.is_local(provider) {
            return None;
        }
        let m = self.find(model_id)?;
        let price = |input: f64, output: f64, cache_read: Option<f64>, cache_write: Option<f64>| crate::ai::models::Price {
            input_per_million: input,
            output_per_million: output,
            cache_read_per_million: cache_read.unwrap_or(0.0),
            cache_write_per_million: cache_write.unwrap_or(0.0),
        };
        let p = match m.tiers.iter().rev().find(|t| input_tokens > t.above_input_tokens) {
            Some(t) => price(t.input, t.output, t.cache_read, t.cache_write),
            None => price(m.input, m.output, m.cache_read, m.cache_write),
        };
        Some(crate::ai::models::cost_micro_at(p, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// In-memory sqlite seeded with rows_sql. Nothing here needs a file on disk,
    /// and a file-backed fixture leaks its .db/-wal/-shm into $TMPDIR whenever an
    /// assertion fails before the cleanup line -- which is exactly the state the
    /// fixture is in while someone is iterating on it.
    /// Mirrors the V35 inference_metrics DDL (minus id/NOT NULL/CHECK) --
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
                feature TEXT, price_unknown INTEGER, registry_version INTEGER);
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

    /// 2025-11-12, before the cache fix.
    const OLD: i64 = 1_762_900_000;
    /// A day after the cutoff.
    const NEW: i64 = INCLUSIVE_INPUT_SINCE + 86_400;
    const HAIKU: &str = "apac.anthropic.claude-haiku-4-5-20251001-v1:0";

    /// One row as a fixture states it: (ts, provider, model, input, output,
    /// cache read, cache write, cost, feature, price unknown).
    type Row = (i64, &'static str, &'static str, i64, i64, i64, i64, Option<i64>, &'static str, i64);

    fn insert(conn: &rusqlite::Connection, rows: &[Row]) {
        for (ts, provider, model, input, output, cache_read, cache_write, cost, feature, unknown) in rows {
            conn.execute(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown)
                 VALUES (?1, 'tools', ?2, ?3, ?4, ?5, ?6, 40, ?7, ?8, ?9, ?10)",
                rusqlite::params![ts, model, input, output, cache_read, cache_write, provider, cost, feature, unknown],
            )
            .unwrap();
        }
    }

    fn marked(conn: &rusqlite::Connection) -> bool {
        conn.query_row("SELECT EXISTS(SELECT 1 FROM user_preferences WHERE key = ?1)", [OLD_ROWS_CORRECTED_KEY], |r| r.get(0)).unwrap()
    }

    /// A row is priced with the registry in use when it is written and
    /// records that registry's version; a chat turn keeps the registry it
    /// started with, even when a newer one arrives mid-turn.
    #[test]
    fn each_row_records_the_registry_version_it_was_priced_at() {
        let db = crate::db::test_database();
        let sonnet = |input: f64| move |r: &mut crate::ai::registry::Registry| {
            r.models.iter_mut().find(|m| m.id == "claude-sonnet-4-6").unwrap().prices.as_mut().unwrap().input = input;
        };
        let turn = {
            let change = sonnet(10.0);
            let _v7 = crate::ai::registry::test_override::with(|r| {
                r.version = 7;
                change(r);
            });
            TurnUsage::new("claude", "claude-sonnet-4-6")
        };
        let mut turn = turn;
        {
            let change = sonnet(20.0);
            let _v8 = crate::ai::registry::test_override::with(|r| {
                r.version = 8;
                change(r);
            });
            turn.add_request(1_000_000, 0, 0, 0);
            record_inference(&db, "claude", Feature::MailSort, Workload::Volume, "claude-sonnet-4-6", 1_000_000, 0, 0, 0, 5);
            record_unpriced(&db, "minimax", Feature::Tts, "speech-02-hd", 12, 0, 5);
        }
        turn.record(&db, Workload::Volume, 5);
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT feature, registry_version, usd_cost_micro FROM inference_metrics ORDER BY id").unwrap();
        let rows: Vec<(String, i64, Option<i64>)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(rows, vec![("mail_sort".into(), 8, Some(20_000_000)), ("tts".into(), 8, None), ("other".into(), 7, Some(10_000_000))]);
    }

    /// The correction prices with the frozen list, never the copy in use:
    /// here the copy in use prices nothing, and the rows the frozen list
    /// prices still keep their cost.
    #[test]
    fn old_rows_are_corrected_with_the_frozen_prices_not_the_registry_in_use() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        let _fetched = crate::ai::registry::test_override::with(|r| {
            for m in &mut r.models {
                m.prices = None;
            }
        });
        assert!(!crate::ai::models::price_known("bedrock", HAIKU), "the copy in use prices nothing");
        insert(&conn, &[
            (OLD, "bedrock", HAIKU, 120, 45, 800, 400, Some(805), "chat", 0),
            (OLD, "openai", "gpt-6-luna", 3_000, 40, 0, 0, Some(0), "mail_sort", 0),
        ]);
        let before = snapshot(&conn);
        assert_eq!(correct_old_rows(&conn).unwrap(), Corrected { guessed: 0, billed_at_zero: 0, local_made_free: 0, cache_added: 1 });
        let after = snapshot(&conn);
        assert_eq!(after[0].1 .4, Some(925), "repriced at the shipped Haiku rate");
        assert_eq!(after[1], before[1], "a priced model billed at zero is not marked unknown");
    }

    /// Never edit or regenerate old_rows_prices.frozen.json, and never
    /// change this hash to make the test pass. The correction runs once, at
    /// the first open of whichever release a user installs next. A list
    /// that changed between releases would correct the same rows one way
    /// for a user who took every release and another way for one who
    /// skipped some (a model that gains or loses a price flips rules 1 and
    /// 2). DECISIONS.md, "P04 and P05:", as the lead ruled on 2026-11-17.
    /// A new price belongs in registry/registry.json.
    #[test]
    fn the_frozen_price_list_is_never_edited() {
        let digest = ring::digest::digest(&ring::digest::SHA256, include_bytes!("old_rows_prices.frozen.json"));
        let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "af323dadc1f975271f8a64f95d6da949d3a8c71463b1697fc5fa74ecedbdaed2");
    }

    /// The frozen list prices every id as registry version 1 did, the
    /// tiers and local providers included (the fixture is that version).
    #[test]
    fn the_frozen_prices_are_version_1_of_the_registry() {
        let frozen = FrozenPrices::get();
        let v1 = crate::ai::registry::fixture();
        assert_eq!(v1.version, 1);
        assert!(!frozen.models.is_empty());
        let mut ids: Vec<String> = v1.models.iter().map(|m| m.id.clone()).collect();
        ids.extend([HAIKU, "us.meta.llama3-3-70b-instruct-v1:0", "GPT-6-Luna-2026-01-01", "kimi-k2-0905-preview", ""].map(String::from));
        let mut tokens = vec![(0, 0, 0, 0), (12_000, 800, 2_000, 300)];
        for tier in v1.models.iter().filter_map(|m| m.prices.as_ref()).flat_map(|p| &p.tiers) {
            tokens.extend([(tier.above_input_tokens, 10, 0, 0), (tier.above_input_tokens + 1, 10, 5, 5)]);
        }
        for provider in ["bedrock", "openai", "compat:api.moonshot.ai", "unknown", "ollama", "Local", "本機 AI"] {
            for id in &ids {
                assert_eq!(frozen.price_known(provider, id), v1.price_known(provider, id), "{provider} {id}");
                for &(input, output, cache_read, cache_write) in &tokens {
                    assert_eq!(frozen.usd_cost_micro(provider, id, input, output, cache_read, cache_write), v1.usd_cost_micro(provider, id, input, output, cache_read, cache_write), "{provider} {id} {input}");
                }
            }
        }
    }

    /// The same rows end the same way whatever registry is in use or
    /// bundled: here both differ from the frozen list (a model it did not
    /// price has a price, a model it priced is gone, and one price has
    /// changed), and the result is the one version 1 gives.
    #[test]
    fn old_rows_are_corrected_the_same_whatever_registry_is_bundled_or_in_use() {
        let llama = "us.meta.llama3-3-70b-instruct-v1:0";
        let nova = "apac.amazon.nova-pro-v1:0";
        let rows: &[Row] = &[
            (OLD, "bedrock", llama, 1_000, 50, 0, 0, Some(0), "chat", 0),
            (OLD, "bedrock", HAIKU, 120, 45, 800, 400, Some(805), "chat", 0),
            (OLD, "bedrock", nova, 2_000, 100, 500, 0, Some(1_700), "chat", 0),
        ];
        let correct = || {
            let db = crate::db::test_database();
            let conn = db.conn.lock().unwrap();
            insert(&conn, rows);
            (correct_old_rows(&conn).unwrap(), snapshot(&conn))
        };
        let (corrected, at_v1) = correct();
        assert_eq!(corrected, Corrected { guessed: 0, billed_at_zero: 1, local_made_free: 0, cache_added: 2 });
        assert_eq!((at_v1[0].1 .4, at_v1[0].1 .6), (None, 1), "an unpriced model billed at zero is marked unknown");
        assert_eq!(at_v1[1].1 .4, Some(925), "repriced at the Haiku rate of version 1");
        // 2,000 x 0.80 + 100 x 3.20 + 500 x 0.20, per million: Nova Pro in version 1.
        assert_eq!(at_v1[2].1 .4, Some(2_020), "repriced at the Nova Pro rate of version 1");

        fn differ(r: &mut crate::ai::registry::Registry) {
            let mut priced = r.models.iter().find(|m| m.prices.is_some() && !m.managed).unwrap().clone();
            priced.id = "llama3-3-70b".into();
            r.models.push(priced);
            let haiku = HAIKU.to_lowercase();
            r.models.retain(|m| !haiku.contains(m.id.as_str()));
            let nova_pro = r.models.iter_mut().find(|m| m.id == "nova-pro").unwrap();
            nova_pro.prices.as_mut().unwrap().input *= 10.0;
        }
        let _live = crate::ai::registry::test_override::with(differ);
        let _bundled = crate::ai::registry::test_override::bundled_with(differ);
        for (copy, registry) in [("in use", crate::ai::registry::current()), ("bundled", crate::ai::registry::bundled())] {
            assert!(registry.price_known("bedrock", llama), "the copy {copy} prices a model version 1 did not");
            assert!(!registry.price_known("bedrock", HAIKU), "the copy {copy} no longer prices one version 1 did");
            assert_eq!(registry.usd_cost_micro("bedrock", nova, 2_500, 100, 500, 0), Some(16_420), "the copy {copy} prices Nova Pro differently");
        }
        assert_eq!(correct(), (corrected, at_v1));
    }

    /// The step reads no registry, so no later release can change it: its
    /// code, the frozen list's included, names neither the registry nor the
    /// app's lookups (`models::price_known` and the rest read the copy in
    /// use).
    #[test]
    fn the_old_row_correction_reads_no_registry() {
        let source = include_str!("cost.rs");
        let start = source.find("pub fn correct_old_rows(").unwrap();
        let end = source.find("#[cfg(test)]\npub(crate) mod tests {").unwrap();
        let code: String = source[start..end].lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        assert!(code.contains("FrozenPrices::get()") && code.contains("fn usd_cost_micro("), "the range holds the step and the frozen list");
        for read in ["registry", "current()", "models::price_known", "models::usd_cost_micro", "models::price_for", "models::is_local"] {
            assert!(!code.contains(read), "the old-row correction reads {read}");
        }
    }

    /// The four corrections, each on the rows it names and two of them on
    /// one row; every other row and column stays as it was, and a second run
    /// changes nothing.
    #[test]
    fn old_rows_are_corrected_once_by_the_four_rules() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        // Five requests of one chat turn, each rounded on its own (TurnUsage):
        // 1,500 stored, where the guess on the summed tokens is 1,502.
        let turn: i64 = (0..5).map(|_| guessed(101, 0, 1, 0)).sum();
        assert_eq!((turn, guessed(505, 0, 5, 0)), (1_500, 1_502), "the turn's rounding differs from the guess on its sums");
        let kimi = guessed(12_000, 800, 2_000, 0);
        assert_eq!(kimi, 42_600);
        let llama = "us.meta.llama3-3-70b-instruct-v1:0";
        assert!(!crate::ai::models::price_known("bedrock", llama));
        insert(&conn, &[
            // 1: Kimi, billed at the guess before V34. Rule 1.
            (OLD, "compat:api.moonshot.ai", "kimi-k2-0905-preview", 12_000, 800, 2_000, 0, Some(kimi), "chat", 0),
            // 2: a guessed turn of five requests on Qwen. Rule 1.
            (OLD, "compat:dashscope-intl.aliyuncs.com", "qwen3-max", 505, 0, 5, 0, Some(turn), "tools", 0),
            // 3: a compatible service that sent no usage. Rule 2.
            (NEW, "compat:api.example.com", "example-chat", 0, 0, 0, 0, Some(0), "chat", 0),
            // 4: an unknown model billed at zero before the guess existed. Rule 2.
            (OLD, "openai", "gpt-unlisted-1", 3_000, 40, 0, 0, Some(0), "mail_sort", 0),
            // 5: a local model billed at the guess (v0.5.0 to v0.5.4). Rule 3.
            (OLD, "local", "llama3.2", 12_000, 800, 2_000, 0, Some(kimi), "mail_analysis", 0),
            // 6: Bedrock Haiku 4.5 before the cache fix, priced today: AWS's
            // inputTokens 120, stored at 805 micro-USD by v0.5.10. Rule 4.
            (OLD, "bedrock", HAIKU, 120, 45, 800, 400, Some(805), "chat", 0),
            // 7: an unpriced model on Bedrock before the cache fix, billed at
            // the guess. Rules 1 and 4.
            (OLD, "bedrock", llama, 120, 45, 800, 400, Some(guessed(120, 45, 800, 400)), "chat", 0),
            // 8: the same, billed at zero. Rules 2 and 4.
            (OLD, "bedrock", llama, 120, 45, 800, 400, Some(0), "chat", 0),
            // 9: an id only the pre-7f9fd11 pattern "haiku-4-5" priced, at
            // Haiku 4.5's real rate: no price today and not the guess, so the
            // cost stays. A Claude row with cache figures: rule 4 never
            // changes a Claude row. Untouched.
            (OLD, "claude", "haiku-4-5-20251001", 12_000, 800, 2_000, 0, Some(14_200), "chat", 0),
            // 10: a local model, free. Untouched.
            (OLD, "local", "qwen3:4b", 12_000, 800, 0, 0, Some(0), "chat", 0),
            // 11: stored since V34 with the price unknown. Untouched.
            (NEW, "compat:api.moonshot.ai", "kimi-k2-0905-preview", 3_000, 10, 0, 0, None, "chat", 1),
            // 12: from before V16: no provider and no cost. Untouched.
            (OLD, "unknown", "qwen3:4b", 4_000, 900, 0, 0, Some(0), "other", 0),
            // 13: Bedrock stored the new way after the fix. Untouched.
            (NEW, "bedrock", HAIKU, 1_320, 45, 800, 400, Some(925), "chat", 0),
            // 14: a model priced at zero for this usage. Untouched.
            (OLD, "typesafe", "jev", 0, 0, 0, 0, Some(0), "mail_sort", 0),
        ]);
        let before = snapshot(&conn);

        assert_eq!(correct_old_rows(&conn).unwrap(), Corrected { guessed: 3, billed_at_zero: 3, local_made_free: 1, cache_added: 3 });
        assert!(marked(&conn));
        let unknown = |row: &Snapshot| {
            let mut row = row.clone();
            row.1 .4 = None;
            row.1 .6 = 1;
            row
        };
        let input = |row: &Snapshot, input: i64| {
            let mut row = row.clone();
            row.0 .4 = input;
            row
        };
        let cost = |row: &Snapshot, cost: i64| {
            let mut row = row.clone();
            row.1 .4 = Some(cost);
            row
        };
        let b = &before;
        assert_eq!(crate::ai::models::usd_cost_micro("bedrock", HAIKU, 1_320, 45, 800, 400), Some(925));
        let after = snapshot(&conn);
        assert_eq!(
            after,
            vec![
                unknown(&b[0]),
                unknown(&b[1]),
                unknown(&b[2]),
                unknown(&b[3]),
                cost(&b[4], 0),
                cost(&input(&b[5], 1_320), 925),
                unknown(&input(&b[6], 1_320)),
                unknown(&input(&b[7], 1_320)),
                b[8].clone(),
                b[9].clone(),
                b[10].clone(),
                b[11].clone(),
                b[12].clone(),
                b[13].clone(),
            ]
        );

        // A second run changes nothing.
        assert_eq!(correct_old_rows(&conn).unwrap(), Corrected::default());
        assert_eq!(snapshot(&conn), after);
        // Without the marker, rules 1 to 3 find nothing more, but rule 4
        // would add the cache to the old rows a second time: exactly once
        // rests on the marker written with the corrections.
        conn.execute("DELETE FROM user_preferences WHERE key = ?1", [OLD_ROWS_CORRECTED_KEY]).unwrap();
        assert_eq!(correct_old_rows(&conn).unwrap(), Corrected { guessed: 0, billed_at_zero: 0, local_made_free: 0, cache_added: 3 });
    }

    /// (input, output, cache read, cache write, cost) of each row, in order.
    fn tokens_and_cost(conn: &rusqlite::Connection) -> Vec<(i64, i64, i64, i64, Option<i64>)> {
        let mut stmt = conn.prepare("SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro FROM inference_metrics ORDER BY id").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap();
        rows.map(Result::unwrap).collect()
    }

    /// Rule 4. AWS returned inputTokens 120, outputTokens 45, cache read 800
    /// and cache write 400, and v0.5.10 stored input 120 at 805 micro-USD.
    /// The row gets the input and the cost a new row with those tokens gets.
    /// A row that cannot be proven old, or has no cache, is left alone, and
    /// so is every Claude row, even one with cache figures and an old date.
    #[test]
    fn old_bedrock_input_gets_its_cache_back() {
        // a1e44e9's author time: `git log -1 --format=%aI a1e44e9`.
        assert_eq!(chrono::DateTime::from_timestamp(INCLUSIVE_INPUT_SINCE, 0).unwrap().to_rfc3339(), "2026-10-29T21:20:41+00:00");
        let today = crate::db::test_database();
        record_inference(&today, "bedrock", Feature::Chat, Workload::Tools, HAIKU, 1_320, 45, 800, 400, 40);
        let stored_today = tokens_and_cost(&today.conn.lock().unwrap());
        assert_eq!(stored_today, vec![(1_320, 45, 800, 400, Some(925))]);

        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        // A Claude row with cache figures, input below its cache and an old
        // date: what rule 4 would take for proven old on Bedrock. No release
        // stored one (none up to v0.5.12 asked Anthropic to cache, and the
        // builds that do store the cache in the input), and rule 4 never
        // changes a Claude row.
        let sonnet = crate::ai::models::cost_micro_at(crate::ai::models::price_for("claude-sonnet-4-6").unwrap(), 2_750, 770, 24_300, 120);
        insert(&conn, &[
            // Before the cutoff, input below its cache: corrected.
            (OLD, "bedrock", HAIKU, 120, 45, 800, 400, Some(805), "chat", 0),
            // After the cutoff, input at least its cache: stored the new way.
            (NEW, "bedrock", HAIKU, 1_320, 45, 800, 400, Some(925), "chat", 0),
            // After the cutoff, input below its cache: only the old way
            // stores that (v0.5.10 still running after the fix was made).
            (NEW, "bedrock", HAIKU, 120, 45, 800, 400, Some(805), "chat", 0),
            // No cache: the bug did not touch it. Stored at an earlier
            // rate, so a recomputed cost would show.
            (OLD, "claude", "claude-sonnet-4-6", 5_000, 100, 0, 0, Some(15_000), "chat", 0),
            // Claude with cache figures before the cutoff: left as stored.
            (OLD, "claude", "claude-sonnet-4-6", 2_750, 770, 24_300, 120, Some(sonnet), "tools", 0),
        ]);
        let before = snapshot(&conn);
        assert_eq!(correct_old_rows(&conn).unwrap().cache_added, 2);
        let after = tokens_and_cost(&conn);
        assert_eq!(
            after,
            vec![stored_today[0], (1_320, 45, 800, 400, Some(925)), stored_today[0], (5_000, 100, 0, 0, Some(15_000)), (2_750, 770, 24_300, 120, Some(sonnet))]
        );
        assert_eq!(snapshot(&conn)[4], before[4], "the Claude row, every column");
        assert_eq!(correct_old_rows(&conn).unwrap(), Corrected::default(), "a second run changes nothing");
        assert_eq!(tokens_and_cost(&conn), after);
    }

    /// A failure part-way, inside rule 4 or at the marker, leaves every row
    /// as it was and no marker; the next run makes every correction.
    #[test]
    fn a_failure_part_way_changes_no_row_and_writes_no_marker() {
        let refusals = [
            "CREATE TEMP TRIGGER refuse BEFORE UPDATE OF input_tokens ON inference_metrics BEGIN SELECT RAISE(ABORT, 'disk full'); END;".to_string(),
            format!("CREATE TEMP TRIGGER refuse BEFORE INSERT ON user_preferences WHEN NEW.key = '{OLD_ROWS_CORRECTED_KEY}' BEGIN SELECT RAISE(ABORT, 'disk full'); END;"),
        ];
        for refuse in refusals {
            let db = crate::db::test_database();
            let conn = db.conn.lock().unwrap();
            insert(&conn, &[
                (OLD, "compat:api.moonshot.ai", "kimi-k2-0905-preview", 12_000, 800, 2_000, 0, Some(guessed(12_000, 800, 2_000, 0)), "chat", 0),
                (NEW, "compat:api.example.com", "example-chat", 0, 0, 0, 0, Some(0), "chat", 0),
                (OLD, "local", "llama3.2", 3_000, 200, 0, 0, Some(guessed(3_000, 200, 0, 0)), "mail_sort", 0),
                (OLD, "bedrock", HAIKU, 120, 45, 800, 400, Some(805), "chat", 0),
            ]);
            let before = snapshot(&conn);
            conn.execute_batch(&refuse).unwrap();

            assert!(correct_old_rows(&conn).is_err(), "{refuse}");
            assert_eq!(snapshot(&conn), before, "{refuse}");
            assert!(!marked(&conn), "{refuse}");

            conn.execute_batch("DROP TRIGGER refuse;").unwrap();
            assert_eq!(correct_old_rows(&conn).unwrap(), Corrected { guessed: 1, billed_at_zero: 1, local_made_free: 1, cache_added: 1 });
            assert!(marked(&conn));
        }
    }

    /// N7. A database from before P02's migration, with rows labelled as
    /// old builds wrote them, through V34 and the step: from the last
    /// release (v0.5.11, V32), and from P01's V33.
    #[test]
    fn an_update_corrects_the_rows_old_builds_wrote() {
        for from in [32, 33] {
            an_update_corrects_the_rows_old_builds_wrote_from(from);
        }
    }

    fn an_update_corrects_the_rows_old_builds_wrote_from(from: i32) {
        let directory = std::env::temp_dir().join(format!("jarvis-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("usage.db");
        let sorted = guessed(3_000, 200, 0, 0);
        assert_eq!(sorted, 12_000);
        {
            let db = crate::db::test_database_at_version(&path, from);
            let conn = db.conn.lock().unwrap();
            for (provider, model, input, output, cache_read, cache_write, cost) in [
                // v0.5.x on Kimi, billed at the guess.
                ("kimi", "kimi-k2-0905-preview", 12_000, 800, 2_000, 0, guessed(12_000, 800, 2_000, 0)),
                // v0.5.0 to v0.5.4: mail sorting on Ollama, and on a local
                // OpenAI-compatible server, each billed at the guess.
                ("ollama", "llama3.2", 3_000, 200, 0, 0, sorted),
                ("本機 ai", "qwen3:4b", 3_000, 200, 0, 0, sorted),
                // v0.5.10 on Bedrock: input without the cache.
                ("bedrock", HAIKU, 120, 45, 800, 400, 805),
                // A custom compatible URL that sent no usage.
                ("api.example.com", "example-chat", 0, 0, 0, 0, 0),
                // Claude without cache, priced: only the label columns move.
                ("claude", "claude-sonnet-4-6", 5_000, 100, 0, 0, 16_500),
            ] {
                conn.execute(
                    "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro)
                     VALUES (?1, 'volume', ?2, ?3, ?4, ?5, ?6, 40, ?7, ?8)",
                    rusqlite::params![OLD, model, input, output, cache_read, cache_write, provider, cost],
                )
                .unwrap();
            }
        }
        let db = crate::db::test_database_at(&path);
        let conn = db.conn.lock().unwrap();
        assert_eq!(correct_old_rows(&conn).unwrap(), Corrected { guessed: 1, billed_at_zero: 1, local_made_free: 2, cache_added: 1 });
        let mut stmt = conn.prepare("SELECT provider, input_tokens, usd_cost_micro, price_unknown FROM inference_metrics ORDER BY id").unwrap();
        let rows: Vec<(String, i64, Option<i64>, i64)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(
            rows,
            vec![
                ("kimi".into(), 12_000, None, 1),
                ("local".into(), 3_000, Some(0), 0),
                ("local".into(), 3_000, Some(0), 0),
                ("bedrock".into(), 1_320, Some(925), 0),
                ("compat:api.example.com".into(), 0, None, 1),
                ("claude".into(), 5_000, Some(16_500), 0),
            ]
        );
        drop(stmt);
        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// The step runs when the app opens its database: after the migrations,
    /// first of the usage steps, before the database is handed to anything
    /// that reads the totals (the roll-up's order: ai/usage.rs).
    #[test]
    fn the_old_rows_are_corrected_when_the_database_opens() {
        let source = include_str!("../db.rs");
        let open = &source[source.find("pub fn new()").expect("Database::new exists")..];
        let open = &open[..open.find("fn db_path()").expect("db_path follows new")];
        let migrated = open.find("embedded::migrations::runner().run(&mut conn)?;").expect("migrations run");
        let prepared = open.find("prepare_usage(&conn);").expect("the usage steps run on open");
        let handed_over = open.find("Ok(Database {").expect("the database is returned");
        assert!(migrated < prepared && prepared < handed_over);
        let steps = &source[source.find("pub(crate) fn prepare_usage(conn: &Connection) {").expect("the usage steps")..];
        let first = steps.lines().nth(1).unwrap_or_default().trim();
        assert_eq!(first, "crate::ai::cost::correct_old_rows_on_open(conn);");
    }

    /// A failure is logged, never fatal: JARVIS still opens.
    #[test]
    fn a_failed_correction_does_not_stop_the_database_opening() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(correct_old_rows(&conn).is_err(), "no tables");
        correct_old_rows_on_open(&conn);
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

    /// P01's scheduled daily briefing gets its AI summary from
    /// `generate_briefing`, which sends it as a briefing: its usage row is
    /// stored under the provider and model that answered, with the feature
    /// "briefing", and priced like any other request. Its speech goes
    /// through the system voice (`speak_while`), which stores no usage.
    #[test]
    fn the_scheduled_briefing_is_stored_as_briefing_usage() {
        let scheduled = include_str!("../scheduler/scheduled_briefing.rs");
        // The service's own methods: the first bodies after the trait's declarations.
        let summarise = &scheduled[scheduled.find("async fn summarise(&self) -> Result<String, String> {").expect("the briefing's AI summary")..];
        let summarise = &summarise[..summarise.find("\n    }\n").unwrap()];
        assert!(summarise.contains("crate::assistant::briefing::generate_briefing("), "{summarise}");
        let briefing = include_str!("../assistant/briefing.rs");
        assert!(briefing.contains("Workload::Tools, Feature::Briefing)"), "the summary is sent as a briefing");
        assert_eq!(Feature::Briefing.as_str(), "briefing");
        let speak = &scheduled[scheduled.find("async fn speak(&self, text: &str, may_go_on: &(dyn Fn() -> bool + Send + Sync)) {").expect("the briefing's voice")..];
        let speak = &speak[..speak.find("\n    }\n").unwrap()];
        assert!(speak.contains("tts.speak_while(") && !speak.contains("minimax"), "{speak}");
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

    /// V34 on a database with usage from v0.5.11 (V32), or with P01's V33
    /// as well: every row becomes 'other', labels collapse to the provider
    /// id, costs are kept, and the new columns hold an unknown price.
    #[test]
    fn v34_backfills_the_feature_and_normalises_provider_labels() {
        for from in [32, 33] {
            v34_backfills_the_feature_and_normalises_provider_labels_from(from);
        }
    }

    fn v34_backfills_the_feature_and_normalises_provider_labels_from(from: i32) {
        let directory = std::env::temp_dir().join(format!("jarvis-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("usage.db");
        {
            let db = crate::db::test_database_at_version(&path, from);
            // U9.2: the cost column every old row comes from is NOT NULL (V16),
            // so each row satisfies V34's two-way CHECK and the copy cannot stop.
            let not_null: bool = db
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT \"notnull\" FROM pragma_table_info('inference_metrics') WHERE name = 'usd_cost_micro'", [], |r| r.get(0))
                .unwrap();
            assert!(not_null, "V{from}: usd_cost_micro is NOT NULL");
            db.conn.lock().unwrap().execute_batch(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro) VALUES
                    (1, 'tools', 'gpt-6-luna', 10, 1, 5, 'openai', 7),
                    (2, 'volume', 'gpt-6-luna', 20, 2, 5, 'openai-mail', 8),
                    (3, 'volume', 'deepseek-chat', 30, 3, 5, 'api.deepseek.com', 9),
                    (4, 'tools', 'llama3', 40, 4, 5, 'localhost', 10),
                    (5, 'tools', 'kimi-k2', 50, 5, 5, 'kimi', 11),
                    (6, 'tools', 'claude-sonnet-4-6', 60, 6, 5, 'claude', 12),
                    (7, 'volume', 'qwen3.5:4b', 70, 7, 5, 'local', 0),
                    (8, 'volume', 'jev', 80, 0, 5, 'typesafe', 13),
                    (10, 'volume', 'llama3.2', 90, 9, 5, 'ollama', 14),
                    (11, 'volume', 'qwen3:4b', 100, 10, 5, '本機 ai', 15);",
            ).unwrap();
            db.conn.lock().unwrap().execute("INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms) VALUES (9, 'volume', 'old', 1, 1, 1)", []).unwrap();
        }
        let db = crate::db::test_database_at(&path);
        let rows = usage_rows(&db);
        let providers: Vec<&str> = rows.iter().map(|row| row.provider.as_str()).collect();
        assert_eq!(providers, vec!["openai", "openai", "compat:api.deepseek.com", "compat:localhost", "kimi", "claude", "local", "typesafe", "local", "local", "unknown"]);
        assert!(rows.iter().all(|row| row.feature == "other" && !row.price_unknown), "{rows:?}");
        let costs: Vec<Option<i64>> = rows.iter().map(|row| row.usd_cost_micro).collect();
        // The migration keeps every cost; the step after it corrects them.
        assert_eq!(costs, vec![Some(7), Some(8), Some(9), Some(10), Some(11), Some(12), Some(0), Some(13), Some(14), Some(15), Some(0)]);

        // The new rows: an unknown price is NULL, and only then.
        record_inference(&db, "openai", Feature::Chat, Workload::Tools, "gpt-unlisted-1", 1, 1, 0, 0, 1);
        assert_eq!(usage_rows(&db).last().map(|row| (row.usd_cost_micro, row.price_unknown)), Some((None, true)));
        let conn = db.conn.lock().unwrap();
        assert!(
            conn.execute("INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, usd_cost_micro, price_unknown) VALUES (1, 'tools', 'x', 1, 1, 1, 5, 1)", []).is_err(),
            "a row cannot carry both a cost and an unknown price"
        );
        assert!(
            conn.execute("INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, usd_cost_micro, price_unknown) VALUES (1, 'tools', 'x', 1, 1, 1, NULL, 0)", []).is_err(),
            "nor a known price without a cost: both tables hold the same rule"
        );
        let indexes: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_inference_metrics_ts', 'idx_inference_metrics_provider_ts')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(indexes, 2);
        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// V35 on a database with usage from V34: every request and every daily
    /// total is kept as it was, at registry version 0, and the totals keep
    /// their key and checks with the version added.
    #[test]
    fn v35_keeps_every_request_and_total_at_version_zero() {
        let directory = std::env::temp_dir().join(format!("jarvis-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("usage.db");
        let totals = "SELECT day, provider, model_id, feature, price_unknown, requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro FROM usage_daily ORDER BY 1, 2, 3, 4, 5";
        type Total = (i64, String, String, String, i64, i64, i64, i64, i64, i64, Option<i64>);
        let read = |conn: &rusqlite::Connection| -> Vec<Total> {
            let mut stmt = conn.prepare(totals).unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))).unwrap().map(Result::unwrap).collect()
        };
        let before = {
            let db = crate::db::test_database_at_version(&path, 34);
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown) VALUES
                    (1, 'tools', 'gpt-6-luna', 10, 1, 5, 'openai', 7, 'chat', 0),
                    (2, 'volume', 'kimi-k2', 20, 2, 5, 'kimi', NULL, 'chat', 1);
                 INSERT INTO usage_daily (day, provider, model_id, feature, price_unknown, requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro) VALUES
                    (5, 'openai', 'gpt-6-luna', 'chat', 0, 3, 30, 3, 0, 0, 21),
                    (5, 'kimi', 'kimi-k2', 'chat', 1, 2, 40, 4, 0, 0, NULL);",
            )
            .unwrap();
            read(&conn)
        };
        let db = crate::db::test_database_at(&path);
        let conn = db.conn.lock().unwrap();
        assert_eq!(read(&conn), before);
        let versions: Vec<i64> = conn.prepare("SELECT registry_version FROM inference_metrics UNION ALL SELECT registry_version FROM usage_daily").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(versions, vec![0, 0, 0, 0]);
        // Same day, same model, another version: a second total, not a conflict.
        conn.execute("INSERT INTO usage_daily (day, provider, model_id, feature, price_unknown, requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro, registry_version) VALUES (5, 'openai', 'gpt-6-luna', 'chat', 0, 1, 1, 1, 0, 0, 1, 2)", []).unwrap();
        assert!(
            conn.execute("INSERT INTO usage_daily (day, provider, model_id, feature, price_unknown, requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro, registry_version) VALUES (6, 'openai', 'x', 'chat', 1, 1, 1, 1, 0, 0, 5, 2)", []).is_err(),
            "the totals keep the cost rule"
        );
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
