//! Usage history on this Mac: the one place that reads usage, and the
//! roll-up that keeps it for two years.
//!
//! Usage is kept in two levels. `inference_metrics` holds one row per request
//! (ai/cost.rs writes it) for the per-request window: 30 days, or the Storage
//! clean-up window when that is on and shorter. [`roll_up`] moves every
//! request from a UTC day wholly older than that window into `usage_daily`,
//! one row per UTC day, provider, model, feature and price-known flag, and
//! deletes it from `inference_metrics` in the same transaction. Daily totals
//! older than two years are deleted. A request is therefore in exactly one
//! level, and every read here adds the two: nothing is counted twice, and
//! nothing is missed at the boundary.
//!
//! What expiry gives up, by design. The first day kept moves forward by at
//! most one day per roll-up, so that a wrong clock cannot delete the
//! history; where the roll-up runs less than once a day (JARVIS left open
//! for weeks, the Usage page seldom opened) totals are kept somewhat longer
//! than two years. And a request stored while the clock was more than two
//! years back is dated before the first day kept, which never moves back:
//! the roll-up that moves it deletes it with the oldest totals (pinned by
//! `a_request_stored_under_a_clock_two_years_back_is_deleted_when_rolled_up`).
//! When the first day kept is first recorded, it is the later of the oldest
//! total and the first day usage could exist, but never later than the day
//! two years back, which until 3 May 2028 is the day recorded on every Mac.
//! So a total from such a clock that is there before any day is recorded goes
//! at the next run too, instead of holding expiry back for years
//! (`the_first_day_kept_never_starts_before_usage_could_exist`).
//!
//! Days are UTC days (`ts / 86400`) in both levels, so a period is exact to
//! the day. Nothing here leaves the Mac, and the log gets counts only.
//!
//! The interface. Other code, P07 and P10 included, reads usage only through
//! these, never from the two tables:
//! - [`Period::last`]: the last N UTC days including today, N in [`PERIODS`].
//! - [`totals`]: the sums over a period, from both levels.
//! - [`groups`]: the same sums split by model, provider, feature or time;
//!   the oldest month of a period that starts after the 1st says so.
//! - [`expensive_requests`]: the costliest single requests, from the
//!   per-request level only (a daily total has no single requests).
//! - [`any_usage_except`]: whether usage from any other provider than one is
//!   on record, in either level.
//! - [`roll_up_on_open`] and [`roll_up_if_due`] keep the levels; [`roll_up`]
//!   is the step itself (inside this crate only), and [`per_request_days`]
//!   its window.

use rusqlite::{Connection, OptionalExtension};

/// Seconds in a UTC day.
const DAY: i64 = 86_400;

/// The periods the Usage page offers, in days.
pub const PERIODS: [i64; 5] = [30, 90, 180, 365, 730];

/// How long each request keeps its own row, unless a shorter Storage
/// clean-up window asks for less.
pub const PER_REQUEST_DAYS: i64 = 30;

/// The shortest per-request window. Settings offers no clean-up window under
/// 30 days; one set shorter by hand is held to this.
pub const MIN_PER_REQUEST_DAYS: i64 = 7;

/// How long the daily totals are kept.
pub const DAILY_TOTALS_DAYS: i64 = 730;

/// The first UTC day JARVIS could have stored usage: inference_metrics came
/// with V13, 57da92f ("feat(db): V13 add aws_credentials, workload_routing,
/// inference_metrics"), authored 2026-05-04T18:16:54Z (`git log -1
/// --format=%at 57da92f`, divided by 86,400). A total dated earlier was
/// stored under a wrong clock.
const FIRST_USAGE_DAY: i64 = 20_577; // 57da92f

/// user_preferences: the UTC day of the last roll-up.
const ROLLED_UP_DAY_KEY: &str = "usage_rolled_up_day";

/// user_preferences: the first UTC day whose daily totals are kept; every
/// earlier day has been deleted. It never moves back, and moves forward by
/// at most one day per roll-up.
const KEPT_FROM_KEY: &str = "usage_daily_kept_from";

/// The UTC day of a unix time, in days since 1970-01-01.
pub fn utc_day(ts: i64) -> i64 {
    ts.div_euclid(DAY)
}

/// The last `days` UTC days, today included. A request dated after today
/// (stored while the clock was ahead) is in every period rather than in none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Period {
    days: i64,
    first_day: i64,
}

impl Period {
    /// One of the [`PERIODS`], ending on the UTC day of `now`.
    pub fn last(days: i64, now: i64) -> Result<Period, String> {
        if !PERIODS.contains(&days) {
            return Err(format!("A usage period is one of {PERIODS:?} days, not {days}."));
        }
        Ok(Period::ending(days, now))
    }

    fn ending(days: i64, now: i64) -> Period {
        Period { days, first_day: utc_day(now) - (days - 1) }
    }

    pub fn days(self) -> i64 {
        self.days
    }

    /// The first UTC day in the period.
    pub fn first_day(self) -> i64 {
        self.first_day
    }
}

/// The one provider whose usage is counted in characters, not tokens:
/// MiniMax speech bills per character, and ai/cost.rs `record_unpriced`
/// stores the characters in the input column.
const BILLED_BY_CHARACTER: &str = "minimax";

/// Usage added up, the same way for both levels. Speech characters are
/// never added into a token figure.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sums {
    pub requests: i64,
    /// As stored: includes the cache reads and writes.
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// Over the requests whose price is known; a local request adds 0.
    pub known_cost_micro: i64,
    /// Requests whose price is unknown. What they cost is in no figure.
    pub price_unknown_requests: i64,
    /// Input without the cache, the way AWS reports inputTokens, worked out
    /// row by row (a request, or a day's total) and then added up. A row
    /// whose input is smaller than its cache reads and writes cannot hold
    /// the cache (ai/cost.rs `correct_old_rows` corrects every such Bedrock
    /// row before anything reads them, and a Claude row with cache figures,
    /// stored only since chat turns ask Anthropic to cache, holds the cache
    /// in its input), so its input counts as stored: the figure is
    /// never below zero, and nothing is clamped away. So a group's figure
    /// changes when its requests are rolled into a day's total if one of them
    /// has input below its cache (input 100 with 300 cached and 1,000 with
    /// 200 give 900 one by one and 600 as one day), which after the
    /// correction only a service reporting more cached tokens than input
    /// could store.
    pub input_without_cache: i64,
    /// Characters of MiniMax speech ([`BILLED_BY_CHARACTER`]); none of them
    /// is in the token figures above.
    pub speech_characters: i64,
}

impl Sums {
    fn add(&mut self, other: &Sums) {
        self.requests += other.requests;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.known_cost_micro += other.known_cost_micro;
        self.price_unknown_requests += other.price_unknown_requests;
        self.input_without_cache += other.input_without_cache;
        self.speech_characters += other.speech_characters;
    }

    /// Read from the columns [`sums_sql`] selects, starting at `at`.
    fn read(row: &rusqlite::Row, at: usize) -> rusqlite::Result<Sums> {
        Ok(Sums {
            requests: row.get(at)?,
            input_tokens: row.get(at + 1)?,
            output_tokens: row.get(at + 2)?,
            cache_read_tokens: row.get(at + 3)?,
            cache_write_tokens: row.get(at + 4)?,
            known_cost_micro: row.get(at + 5)?,
            price_unknown_requests: row.get(at + 6)?,
            input_without_cache: row.get(at + 7)?,
            speech_characters: row.get(at + 8)?,
        })
    }
}

/// Both levels as one set of rows, each holding its number of requests. ?1
/// is the period's first second, ?2 its first day: the same boundary.
const BOTH_LEVELS: &str = "
    SELECT CAST(ts / 86400 AS INTEGER) AS day, provider, model_id, feature, price_unknown,
           1 AS requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro
    FROM inference_metrics WHERE ts >= ?1
    UNION ALL
    SELECT day, provider, model_id, feature, price_unknown,
           requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro
    FROM usage_daily WHERE day >= ?2";

/// The columns [`Sums::read`] reads, over [`BOTH_LEVELS`].
fn sums_sql() -> String {
    let characters = format!("provider = '{BILLED_BY_CHARACTER}'");
    let tokens = |column: &str| format!("COALESCE(SUM(CASE WHEN {characters} THEN 0 ELSE {column} END), 0)");
    format!(
        "COALESCE(SUM(requests), 0), {}, {}, {}, {},
         COALESCE(SUM(CASE WHEN price_unknown = 0 THEN usd_cost_micro END), 0),
         COALESCE(SUM(CASE WHEN price_unknown = 1 THEN requests ELSE 0 END), 0),
         COALESCE(SUM(CASE WHEN {characters} THEN 0
                           WHEN input_tokens >= cache_read_tokens + cache_write_tokens
                               THEN input_tokens - cache_read_tokens - cache_write_tokens
                           ELSE input_tokens END), 0),
         COALESCE(SUM(CASE WHEN {characters} THEN input_tokens ELSE 0 END), 0)",
        tokens("input_tokens"),
        tokens("output_tokens"),
        tokens("cache_read_tokens"),
        tokens("cache_write_tokens"),
    )
}

fn period_params(period: Period) -> [i64; 2] {
    [period.first_day * DAY, period.first_day]
}

/// Everything recorded over a period.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub sums: Sums,
    /// Requests answered by a model on this Mac (`models::is_local`).
    pub local_requests: i64,
    /// UTC days with a request whose price is known. A day with only
    /// unpriced usage is left out, so the spend per day of use is not
    /// lowered by spend that is in no total.
    pub priced_days: i64,
}

/// The totals of a period, from both levels.
pub fn totals(conn: &Connection, period: Period) -> rusqlite::Result<Totals> {
    let mut totals = Totals::default();
    for group in groups(conn, period, GroupBy::Provider)? {
        if crate::ai::models::is_local(&group.key) {
            totals.local_requests += group.sums.requests;
        }
        totals.sums.add(&group.sums);
    }
    totals.priced_days = conn.query_row(
        &format!("SELECT COUNT(DISTINCT day) FROM ({BOTH_LEVELS}) WHERE price_unknown = 0"),
        period_params(period),
        |r| r.get(0),
    )?;
    Ok(totals)
}

/// How [`groups`] splits a period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupBy {
    /// Provider with model id.
    Model,
    Provider,
    Feature,
    /// By UTC day, or by calendar month (UTC) for a period over 90 days
    /// ([`by_month`]).
    Time,
}

impl GroupBy {
    pub fn parse(name: &str) -> Result<GroupBy, String> {
        match name {
            "model" => Ok(GroupBy::Model),
            "provider" => Ok(GroupBy::Provider),
            "feature" => Ok(GroupBy::Feature),
            "time" => Ok(GroupBy::Time),
            other => Err(format!("Usage cannot be grouped by {other}.")),
        }
    }
}

/// Whether time groups are calendar months rather than days.
pub fn by_month(period: Period) -> bool {
    period.days > 90
}

/// One group of a period's usage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The model id, provider id or feature; for time, the UTC day
    /// ("2026-11-13") or month ("2026-11").
    pub key: String,
    /// The provider of the model, when grouped by model.
    pub provider: Option<String>,
    /// On the oldest month of a period that starts after that month's
    /// first day: the period's first day ("2026-05-18"), so the row can say
    /// it holds only that day onwards. None on every other group.
    pub from_day: Option<String>,
    pub sums: Sums,
}

/// The month a period enters part-way and the day it enters it, as the
/// time groups name them: ("2026-05", "2026-05-18"). None when the period
/// starts on the 1st.
fn part_month(period: Period) -> Option<(String, String)> {
    let first = chrono::DateTime::from_timestamp(period.first_day * DAY, 0)?.date_naive();
    (chrono::Datelike::day(&first) != 1).then(|| (first.format("%Y-%m").to_string(), first.format("%Y-%m-%d").to_string()))
}

/// A period's usage by model, provider, feature or time. Model, provider and
/// feature groups come highest known cost first; time groups newest first.
pub fn groups(conn: &Connection, period: Period, by: GroupBy) -> rusqlite::Result<Vec<Group>> {
    let (key, provider) = match by {
        GroupBy::Model => ("model_id", "provider"),
        GroupBy::Provider => ("provider", "NULL"),
        GroupBy::Feature => ("feature", "NULL"),
        GroupBy::Time if by_month(period) => ("strftime('%Y-%m', day * 86400, 'unixepoch')", "NULL"),
        GroupBy::Time => ("strftime('%Y-%m-%d', day * 86400, 'unixepoch')", "NULL"),
    };
    // Columns: 1 key, 2 provider, 3 requests, ..., 8 known cost.
    let order = if by == GroupBy::Time { "1 DESC" } else { "8 DESC, 3 DESC, 1, 2" };
    let mut stmt = conn.prepare(&format!(
        "SELECT {key}, {provider}, {} FROM ({BOTH_LEVELS}) GROUP BY 1, 2 ORDER BY {order}",
        sums_sql()
    ))?;
    let rows = stmt.query_map(period_params(period), |r| Ok(Group { key: r.get(0)?, provider: r.get(1)?, from_day: None, sums: Sums::read(r, 2)? }))?;
    let mut groups = rows.collect::<rusqlite::Result<Vec<Group>>>()?;
    if by == GroupBy::Time && by_month(period) {
        if let Some((month, first)) = part_month(period) {
            for group in groups.iter_mut().filter(|group| group.key == month) {
                group.from_day = Some(first.clone());
            }
        }
    }
    Ok(groups)
}

/// One request, as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub ts: i64,
    pub provider: String,
    pub model_id: String,
    pub workload: String,
    pub feature: String,
    /// As stored: includes the cache reads and writes.
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub usd_cost_micro: i64,
    pub latency_ms: i64,
}

/// The `limit` requests with the highest known cost above zero, and the
/// number of UTC days, today included, they are taken from. That is the last
/// [`per_request_days`] days, unless some of those days are already daily
/// totals (a clock that was ahead, or a shorter window, moved them early):
/// then it is the days after the newest such total, today at least, so the
/// number said with the list is always the days it covers. From the
/// per-request level only; local requests are free and not listed.
pub fn expensive_requests(conn: &Connection, now: i64, limit: usize) -> rusqlite::Result<(i64, Vec<Request>)> {
    let window = Period::ending(per_request_days(conn), now);
    let today = utc_day(now);
    let newest_total: Option<i64> = conn.query_row("SELECT MAX(day) FROM usage_daily WHERE day >= ?1", [window.first_day], |r| r.get(0))?;
    let first_day = newest_total.map_or(window.first_day, |day| (day + 1).min(today));
    let days = today - first_day + 1;
    let mut stmt = conn.prepare(
        "SELECT ts, provider, model_id, workload, feature, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, usd_cost_micro, latency_ms
         FROM inference_metrics
         WHERE ts >= ?1 AND price_unknown = 0 AND usd_cost_micro > 0
         ORDER BY usd_cost_micro DESC, ts DESC",
    )?;
    let rows = stmt.query_map([first_day * DAY], |r| {
        Ok(Request {
            ts: r.get(0)?,
            provider: r.get(1)?,
            model_id: r.get(2)?,
            workload: r.get(3)?,
            feature: r.get(4)?,
            input_tokens: r.get(5)?,
            output_tokens: r.get(6)?,
            cache_read_tokens: r.get(7)?,
            cache_write_tokens: r.get(8)?,
            usd_cost_micro: r.get(9)?,
            latency_ms: r.get(10)?,
        })
    })?;
    let mut requests = Vec::new();
    for request in rows {
        if requests.len() == limit {
            break;
        }
        let request = request?;
        if !crate::ai::models::is_local(&request.provider) {
            requests.push(request);
        }
    }
    Ok((days, requests))
}

/// Whether any usage from another provider than `provider` is on record, in
/// either level.
pub fn any_usage_except(conn: &Connection, provider: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM inference_metrics WHERE provider <> ?1)
             OR EXISTS(SELECT 1 FROM usage_daily WHERE provider <> ?1)",
        [provider],
        |r| r.get(0),
    )
}

/// The per-request window for a Storage clean-up window (None: clean-up
/// off): 7 to 30 days.
pub fn per_request_days_for(clean_up_days: Option<i64>) -> i64 {
    match clean_up_days {
        Some(days) if days < PER_REQUEST_DAYS => days.max(MIN_PER_REQUEST_DAYS),
        _ => PER_REQUEST_DAYS,
    }
}

/// The per-request window this Mac uses now.
pub fn per_request_days(conn: &Connection) -> i64 {
    per_request_days_for(crate::commands::storage::retention_window(conn))
}

/// What one roll-up did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RolledUp {
    /// Requests moved into the daily totals.
    pub requests_moved: usize,
    /// Daily totals deleted for being older than two years.
    pub totals_expired: usize,
}

/// Move every request from a UTC day wholly older than `per_request_days`
/// into the daily totals, then delete the daily totals older than two years.
///
/// One transaction: a failure or a crash at any point leaves both tables as
/// they were, and a request is never in both or in neither. A day is moved
/// only once all of it is older than the window, so its requests stay
/// together; running again changes nothing. A request stored later for a
/// day already moved (a clock that went back) is added to that day's total.
///
/// The rows earlier builds stored wrongly are corrected first (ai/cost.rs
/// `correct_old_rows`): a daily total is never corrected afterwards. That
/// returns at once when it is done.
///
/// Expiry is the one step that cannot be undone, so a wrong clock is not
/// trusted with it: the first day kept moves forward by at most one day per
/// run, and never back. A clock set two years ahead deletes one day of the
/// oldest totals per run, not two years. Moving requests early under such a
/// clock loses nothing: each stays under its own day.
///
/// A window outside [`MIN_PER_REQUEST_DAYS`] to [`PER_REQUEST_DAYS`] is held
/// to the nearer end, so no caller can move nearly every request, or keep
/// requests one by one past 30 days.
pub(crate) fn roll_up(conn: &Connection, now: i64, per_request_days: i64) -> rusqlite::Result<RolledUp> {
    crate::ai::cost::correct_old_rows(conn)?;
    let today = utc_day(now);
    let window = per_request_days.clamp(MIN_PER_REQUEST_DAYS, PER_REQUEST_DAYS);
    // The day before this one ended at least `window` days ago.
    let first_kept_ts = (today - window) * DAY;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO usage_daily
            (day, provider, model_id, feature, price_unknown, requests,
             input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro, registry_version)
         SELECT CAST(ts / 86400 AS INTEGER), provider, model_id, feature, price_unknown, COUNT(*),
                SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens),
                CASE WHEN price_unknown = 0 THEN COALESCE(SUM(usd_cost_micro), 0) END,
                registry_version
         FROM inference_metrics WHERE ts < ?1
         GROUP BY 1, 2, 3, 4, 5, 12
         ON CONFLICT (day, provider, model_id, feature, price_unknown, registry_version) DO UPDATE SET
            requests = requests + excluded.requests,
            input_tokens = input_tokens + excluded.input_tokens,
            output_tokens = output_tokens + excluded.output_tokens,
            cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
            cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
            usd_cost_micro = usd_cost_micro + excluded.usd_cost_micro",
        [first_kept_ts],
    )?;
    let requests_moved = tx.execute("DELETE FROM inference_metrics WHERE ts < ?1", [first_kept_ts])?;
    let totals_expired = expire(&tx, today)?;
    write_day(&tx, ROLLED_UP_DAY_KEY, today)?;
    tx.commit()?;
    Ok(RolledUp { requests_moved, totals_expired })
}

/// Delete the daily totals of the days wholly older than two years, by the
/// same rule as the roll-up, at most one more day than the last run.
fn expire(tx: &Connection, today: i64) -> rusqlite::Result<usize> {
    let due = today - DAILY_TOTALS_DAYS;
    let from = match read_day(tx, KEPT_FROM_KEY)? {
        Some(day) => day,
        None => {
            // The first run with totals only records where expiry starts and
            // deletes nothing: the later of the oldest total and the first
            // day usage could exist, but never later than the day two years
            // back, which until 3 May 2028 is the day recorded on every Mac.
            // A total dated before the day recorded, from a clock years back,
            // goes at the next run. Any later run can move that day forward
            // by one and delete what is before it, and that includes the
            // clean-up at start-up of the same launch (commands/storage.rs
            // `sweep` runs a roll-up of its own). So with the clock two years
            // ahead at the first launch after the update and clean-up on, the
            // first day of real usage still goes at that launch; each run
            // takes at most one day.
            if let Some(oldest) = tx.query_row("SELECT MIN(day) FROM usage_daily", [], |r| r.get::<_, Option<i64>>(0))? {
                write_day(tx, KEPT_FROM_KEY, oldest.max(FIRST_USAGE_DAY).min(due))?;
            }
            return Ok(0);
        }
    };
    let kept_from = from.max(due.min(from + 1));
    let expired = tx.execute("DELETE FROM usage_daily WHERE day < ?1", [kept_from])?;
    write_day(tx, KEPT_FROM_KEY, kept_from)?;
    Ok(expired)
}

fn read_day(conn: &Connection, key: &str) -> rusqlite::Result<Option<i64>> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM user_preferences WHERE key = ?1", [key], |r| r.get(0))
        .optional()?;
    Ok(value.and_then(|value| value.trim().parse().ok()))
}

fn write_day(conn: &Connection, key: &str, day: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO user_preferences (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        rusqlite::params![key, day.to_string()],
    )?;
    Ok(())
}

/// [`roll_up`] with this Mac's window, logging counts only. A failure leaves
/// the rows where they were, to be moved at the next run; the reads add both
/// levels, so they are right either way.
fn roll_up_logged(conn: &Connection, now: i64) {
    match roll_up(conn, now, per_request_days(conn)) {
        Ok(RolledUp { requests_moved: 0, totals_expired: 0 }) => {}
        Ok(RolledUp { requests_moved, totals_expired }) => log::info!(
            "usage: {requests_moved} requests moved into daily totals, {totals_expired} daily totals older than two years deleted"
        ),
        Err(e) => log::warn!("usage: roll-up failed, usage left as it was until the next run: {e}"),
    }
}

/// The roll-up when the database opens: after the migrations and the
/// correction of old rows (db.rs), before the Storage clean-up (lib.rs).
pub fn roll_up_on_open(conn: &Connection) {
    roll_up_logged(conn, chrono::Utc::now().timestamp());
}

/// The roll-up before a usage read, when the UTC day has changed since the
/// last one: JARVIS can stay open for days.
pub fn roll_up_if_due(conn: &Connection, now: i64) {
    if read_day(conn, ROLLED_UP_DAY_KEY).ok().flatten() != Some(utc_day(now)) {
        roll_up_logged(conn, now);
    }
}

/// The native QA build's usage, so the Usage page can be checked in a real
/// window with both levels filled. Compiled only into tests and the
/// `native-qa` build (native_qa.rs seeds once, then rolls up), never into a
/// normal one. Requests 0, 5, 40, 80, 200 and 400 days old on a priced
/// cloud model, a model with no known price, a local model and a Bedrock
/// model with cache tokens, priced by the built-in list as a request of
/// this build would be, and one MiniMax speech request.
#[cfg(any(test, feature = "native-qa"))]
pub(crate) fn seed_for_qa(conn: &Connection, now: i64) -> rusqlite::Result<()> {
    use crate::ai::models::{is_local, usd_cost_micro};
    let openai = crate::ai::models::openai_default();
    let registry_version = crate::ai::registry::current().version as i64;
    // provider, model, feature, workload, input (cache included), output, cache read, cache write
    let kinds = [
        ("openai", openai.as_str(), "chat", "tools", 1_200, 300, 200, 0),
        ("compat:api.moonshot.ai", "kimi-k2-0905-preview", "mail_sort", "volume", 3_000, 40, 0, 0),
        ("local", "qwen3:4b", "mail_analysis", "volume", 4_000, 900, 0, 0),
        ("bedrock", "apac.anthropic.claude-haiku-4-5-20251001-v1:0", "tools", "tools", 1_320, 45, 800, 400),
    ];
    let mut rows = Vec::new();
    // 179 and 364 days back are the first days of the 6-month and the 1-year
    // period, so the time view shows a part-month row with its first day.
    for back in [0, 5, 40, 80, 179, 200, 364, 400] {
        let ts = now - back * DAY - 60;
        for (provider, model, feature, workload, input, output, cache_read, cache_write) in kinds {
            let cost = usd_cost_micro(provider, model, input, output, cache_read, cache_write).or(is_local(provider).then_some(0));
            rows.push((ts, provider, model, feature, workload, input, output, cache_read, cache_write, cost));
        }
    }
    // MiniMax bills characters, stored in the input column (ai/cost.rs
    // record_unpriced): one request kept as it is, one in a daily total.
    for back in [5, 40] {
        rows.push((now - back * DAY - 120, BILLED_BY_CHARACTER, "speech-02-hd", "tts", "volume", 1_234, 0, 0, 0, None));
    }
    for (ts, provider, model, feature, workload, input, output, cache_read, cache_write, cost) in rows {
        conn.execute(
            "INSERT INTO inference_metrics
                (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                 latency_ms, provider, usd_cost_micro, feature, price_unknown, registry_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 900, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![ts, workload, model, input, output, cache_read, cache_write, provider, cost, feature, cost.is_none(), registry_version],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-13, 12:00 UTC.
    const TODAY: i64 = 20_770;
    const NOW: i64 = TODAY * DAY + 12 * 3_600;
    const HAIKU: &str = "apac.anthropic.claude-haiku-4-5-20251001-v1:0";

    /// One request as a fixture states it.
    #[derive(Clone, Debug)]
    struct Req {
        ts: i64,
        provider: &'static str,
        model: &'static str,
        feature: &'static str,
        /// Stored input, cache included.
        input: i64,
        output: i64,
        cache_read: i64,
        cache_write: i64,
        /// None: the price is unknown.
        cost: Option<i64>,
    }

    #[allow(clippy::too_many_arguments)]
    fn req(ts: i64, provider: &'static str, model: &'static str, feature: &'static str, input: i64, output: i64, cache_read: i64, cache_write: i64, cost: Option<i64>) -> Req {
        Req { ts, provider, model, feature, input, output, cache_read, cache_write, cost }
    }

    /// A database as JARVIS opens one: migrated, and the one-time
    /// correction of old rows already made (on an empty table, as on a new
    /// install), so the rows a test stores count as stored by this build.
    fn opened() -> crate::db::Database {
        let db = crate::db::test_database();
        crate::ai::cost::correct_old_rows(&db.conn.lock().unwrap()).unwrap();
        db
    }

    fn store(conn: &Connection, requests: &[Req]) {
        for r in requests {
            conn.execute(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown)
                 VALUES (?1, 'tools', ?2, ?3, ?4, ?5, ?6, 40, ?7, ?8, ?9, ?10)",
                rusqlite::params![r.ts, r.model, r.input, r.output, r.cache_read, r.cache_write, r.provider, r.cost, r.feature, r.cost.is_none()],
            )
            .unwrap();
        }
    }

    /// The sums of the fixture's requests that `keep` keeps, worked out
    /// without SQL.
    fn brute(requests: &[Req], keep: impl Fn(&Req) -> bool) -> Sums {
        let mut sums = Sums::default();
        for r in requests.iter().filter(|r| keep(r)) {
            sums.requests += 1;
            sums.input_tokens += r.input;
            sums.output_tokens += r.output;
            sums.cache_read_tokens += r.cache_read;
            sums.cache_write_tokens += r.cache_write;
            sums.known_cost_micro += r.cost.unwrap_or(0);
            sums.price_unknown_requests += i64::from(r.cost.is_none());
            sums.input_without_cache += r.input - r.cache_read - r.cache_write;
        }
        sums
    }

    /// Priced, unpriced and local requests on several models, features and
    /// days, on both sides of every period's first day, at the first and
    /// last second of a day.
    fn mixed() -> Vec<Req> {
        let mut requests = Vec::new();
        let back = [0, 1, 3, 29, 30, 31, 45, 89, 90, 91, 179, 180, 181, 364, 365, 366, 600, 729, 730];
        for (i, back) in back.into_iter().enumerate() {
            let start = (TODAY - back) * DAY;
            let i = i as i64;
            requests.push(req(start, "openai", "gpt-6-luna", "chat", 1_000 + i, 10 + i, 200, 0, Some(105 + i)));
            requests.push(req(start + DAY - 1, "bedrock", HAIKU, "tools", 1_320, 45, 800, 400, Some(900)));
            requests.push(req(start + 3_600, "compat:api.moonshot.ai", "kimi-k2-0905-preview", "mail_sort", 3_000 + i, 20, 0, 0, None));
            if i % 2 == 0 {
                requests.push(req(start + 7_200, "local", "qwen3:4b", "mail_analysis", 4_000, 900, 0, 0, Some(0)));
            }
            if i % 3 == 0 {
                // A second request of the same group on the same day.
                requests.push(req(start + 10_000, "openai", "gpt-6-luna", "chat", 50, 5, 0, 0, Some(7)));
            }
        }
        requests
    }

    /// Both tables added up by day, provider, model, feature and price flag:
    /// (day, provider, model, feature, unknown) and (requests, input, output,
    /// cache read, cache write, cost).
    type ByDay = ((i64, String, String, String, i64), (i64, i64, i64, i64, i64, Option<i64>));

    fn both_levels_by_day(conn: &Connection) -> Vec<ByDay> {
        let mut stmt = conn
            .prepare(
                "SELECT day, provider, model_id, feature, price_unknown, SUM(n), SUM(i), SUM(o), SUM(cr), SUM(cw), SUM(c) FROM (
                    SELECT CAST(ts / 86400 AS INTEGER) AS day, provider, model_id, feature, price_unknown, 1 AS n, input_tokens AS i, output_tokens AS o, cache_read_tokens AS cr, cache_write_tokens AS cw, usd_cost_micro AS c FROM inference_metrics
                    UNION ALL
                    SELECT day, provider, model_id, feature, price_unknown, requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro FROM usage_daily)
                 GROUP BY 1, 2, 3, 4, 5 ORDER BY 1, 2, 3, 4, 5",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?), (r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))))
            .unwrap();
        rows.map(Result::unwrap).collect()
    }

    /// Every column of every per-request row: (id, ts, model, provider,
    /// feature, unknown) and (input, output, cache read, cache write, cost).
    type Stored = ((i64, i64, String, String, String, i64), (i64, i64, i64, i64, Option<i64>));

    fn per_request(conn: &Connection) -> Vec<Stored> {
        let mut stmt = conn
            .prepare("SELECT id, ts, model_id, provider, feature, price_unknown, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro FROM inference_metrics ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?), (r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))))
            .unwrap();
        rows.map(Result::unwrap).collect()
    }

    /// Every daily total, as stored.
    fn daily(conn: &Connection) -> Vec<ByDay> {
        let mut stmt = conn
            .prepare("SELECT day, provider, model_id, feature, price_unknown, requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro FROM usage_daily ORDER BY 1, 2, 3, 4, 5")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?), (r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))))
            .unwrap();
        rows.map(Result::unwrap).collect()
    }

    fn daily_days(conn: &Connection) -> Vec<i64> {
        let mut days: Vec<i64> = daily(conn).into_iter().map(|((day, ..), _)| day).collect();
        days.dedup();
        days
    }

    fn all_periods(conn: &Connection, now: i64) -> Vec<Totals> {
        PERIODS.iter().map(|days| totals(conn, Period::last(*days, now).unwrap()).unwrap()).collect()
    }

    fn set_pref(conn: &Connection, key: &str, value: &str) {
        conn.execute("INSERT INTO user_preferences (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value]).unwrap();
    }

    /// Requests, the four token figures, the known cost and the unpriced
    /// count are the same before and after, for every day, provider, model
    /// and feature, and so for every period.
    #[test]
    fn a_roll_up_keeps_every_sum() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let requests = mixed();
        store(&conn, &requests);
        let by_day = both_levels_by_day(&conn);
        let periods = all_periods(&conn, NOW);

        let old = requests.iter().filter(|r| utc_day(r.ts) < TODAY - 30).count();
        assert_eq!(roll_up(&conn, NOW, 30).unwrap(), RolledUp { requests_moved: old, totals_expired: 0 });
        assert!(old > 0 && !daily(&conn).is_empty(), "something was moved");
        assert_eq!(per_request(&conn).len(), requests.len() - old);
        assert_eq!(both_levels_by_day(&conn), by_day);
        assert_eq!(all_periods(&conn, NOW), periods);
    }

    /// The rows inside the window are left exactly as they were, and a
    /// second run changes nothing in either table.
    #[test]
    fn a_second_roll_up_changes_nothing_and_the_window_is_untouched() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        store(&conn, &mixed());
        let inside: Vec<Stored> = per_request(&conn).into_iter().filter(|((_, ts, ..), _)| *ts >= (TODAY - 30) * DAY).collect();

        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(per_request(&conn), inside);
        let moved = daily(&conn);

        assert_eq!(roll_up(&conn, NOW, 30).unwrap(), RolledUp::default());
        assert_eq!((per_request(&conn), daily(&conn)), (inside, moved));
    }

    /// Part of the 31st day back is older than 30 days, but its requests
    /// stay together until all of the day is.
    #[test]
    fn a_day_that_straddles_the_cutoff_is_moved_only_once_all_of_it_is_old() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let day = TODAY - 30;
        store(&conn, &[req(day * DAY + 5, "openai", "gpt-6-luna", "chat", 10, 1, 0, 0, Some(3)), req(day * DAY + DAY - 5, "openai", "gpt-6-luna", "chat", 20, 2, 0, 0, Some(4))]);
        assert!(NOW - (day * DAY + 5) > 30 * DAY, "the first request is older than 30 days");

        assert_eq!(roll_up(&conn, NOW, 30).unwrap().requests_moved, 0);
        assert_eq!(per_request(&conn).len(), 2);

        assert_eq!(roll_up(&conn, (TODAY + 1) * DAY, 30).unwrap().requests_moved, 2);
        assert_eq!(daily(&conn), vec![((day, "openai".into(), "gpt-6-luna".into(), "chat".into(), 0), (2, 30, 3, 0, 0, Some(7)))]);
    }

    /// Writing the totals succeeds, deleting the requests fails: the
    /// transaction leaves both tables as they were.
    #[test]
    fn a_failure_between_writing_totals_and_deleting_rows_changes_neither_table() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        store(&conn, &mixed());
        let before = (per_request(&conn), daily(&conn));
        conn.execute_batch("CREATE TEMP TRIGGER refuse_delete BEFORE DELETE ON inference_metrics BEGIN SELECT RAISE(ABORT, 'disk full'); END;").unwrap();

        assert!(roll_up(&conn, NOW, 30).is_err());
        assert_eq!((per_request(&conn), daily(&conn)), before);
        assert_eq!(read_day(&conn, ROLLED_UP_DAY_KEY).unwrap(), None, "the run is not recorded either");

        conn.execute_batch("DROP TRIGGER refuse_delete;").unwrap();
        assert!(roll_up(&conn, NOW, 30).unwrap().requests_moved > 0, "the next run moves them");
    }

    /// U7.3. The writes after the requests are deleted fail in turn: the
    /// expiry's delete of an old daily total, and the record of the run.
    /// The same transaction leaves both tables and both markers as they were.
    #[test]
    fn a_failure_after_the_requests_are_deleted_changes_neither_table() {
        let refusals = [
            "CREATE TEMP TRIGGER refuse BEFORE DELETE ON usage_daily BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
            "CREATE TEMP TRIGGER refuse BEFORE INSERT ON user_preferences WHEN NEW.key = 'usage_rolled_up_day' BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        ];
        for refusal in refusals {
            let db = opened();
            let conn = db.conn.lock().unwrap();
            store(&conn, &mixed());
            // A total the expiry deletes at this run.
            conn.execute(
                "INSERT INTO usage_daily (day, provider, model_id, feature, price_unknown, requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro)
                 VALUES (?1, 'openai', 'gpt-6-luna', 'chat', 0, 1, 1, 1, 0, 0, 1)",
                [TODAY - 801],
            )
            .unwrap();
            write_day(&conn, KEPT_FROM_KEY, TODAY - 801).unwrap();
            let markers = |conn: &Connection| (read_day(conn, KEPT_FROM_KEY).unwrap(), read_day(conn, ROLLED_UP_DAY_KEY).unwrap());
            let before = (per_request(&conn), daily(&conn), markers(&conn));
            conn.execute_batch(refusal).unwrap();

            assert!(roll_up(&conn, NOW, 30).is_err(), "{refusal}");
            assert_eq!((per_request(&conn), daily(&conn), markers(&conn)), before, "{refusal}");

            conn.execute_batch("DROP TRIGGER refuse;").unwrap();
            let rolled = roll_up(&conn, NOW, 30).unwrap();
            assert!(rolled.requests_moved > 0 && rolled.totals_expired == 1, "the next run does it all: {rolled:?}");
        }
    }

    /// U7.2. What the roll-up carries: every column of a request is in its
    /// daily total, or on the list of what a daily total does not keep, and
    /// the totals have exactly their twelve columns, each written by the
    /// roll-up. A column added to requests alone fails here instead of being
    /// dropped without notice at 30 days.
    #[test]
    fn every_request_column_is_in_the_daily_totals_or_listed_as_dropped() {
        // `ts` becomes `day`.
        const DROPPED: [&str; 4] = ["id", "ts", "workload", "latency_ms"];
        const DAILY: [&str; 12] = [
            "day", "provider", "model_id", "feature", "price_unknown", "requests",
            "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens", "usd_cost_micro", "registry_version",
        ];
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        let columns = |table: &str| -> Vec<String> {
            let mut stmt = conn.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid").unwrap();
            stmt.query_map([table], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        let daily = columns("usage_daily");
        assert_eq!(daily, DAILY);
        let requests = columns("inference_metrics");
        assert!(requests.len() > DROPPED.len());
        let lost: Vec<&String> = requests.iter().filter(|c| !DROPPED.contains(&c.as_str()) && !daily.contains(c)).collect();
        assert!(lost.is_empty(), "add these to usage_daily and the roll-up, or to DROPPED: {lost:?}");

        let source = include_str!("usage.rs");
        let insert = &source[source.find("tx.execute(\n        \"INSERT INTO usage_daily").expect("the roll-up's insert")..];
        let listed = &insert[insert.find('(').unwrap() + 1..];
        let listed = &listed[listed.find('(').unwrap() + 1..listed.find(')').unwrap()];
        let listed: Vec<&str> = listed.split(',').map(str::trim).collect();
        assert_eq!(listed, DAILY, "the roll-up writes every column of the totals");
    }

    /// A day whose requests were priced at two registry versions keeps one
    /// total per version, and the totals still add up.
    #[test]
    fn the_roll_up_keeps_one_total_per_registry_version() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        let old = 40 * DAY + 3_600;
        for (version, cost) in [(0, 100), (3, 200), (3, 300), (4, 400)] {
            conn.execute(
                "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider, usd_cost_micro, feature, price_unknown, registry_version)
                 VALUES (?1, 'volume', 'm', 10, 1, 5, 'openai', ?2, 'chat', 0, ?3)",
                rusqlite::params![old, cost, version],
            )
            .unwrap();
        }
        roll_up(&conn, 80 * DAY, PER_REQUEST_DAYS).unwrap();
        let mut stmt = conn.prepare("SELECT registry_version, requests, usd_cost_micro FROM usage_daily ORDER BY registry_version").unwrap();
        let totals: Vec<(i64, i64, i64)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(totals, vec![(0, 1, 100), (3, 2, 500), (4, 1, 400)]);
    }

    /// A clean-up window under 30 days moves requests sooner, and they still
    /// count in the 30-day view. With clean-up off the window is 30 days.
    #[test]
    fn a_short_clean_up_window_moves_requests_without_losing_them() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        set_pref(&conn, "storage_retention_enabled", "1");
        set_pref(&conn, "storage_retention_days", "7");
        assert_eq!(per_request_days(&conn), 7);
        let requests = [3, 10, 20].map(|back| req((TODAY - back) * DAY + 60, "openai", "gpt-6-luna", "chat", 100 * back, back, 0, 0, Some(back)));
        store(&conn, &requests);

        assert_eq!(roll_up(&conn, NOW, per_request_days(&conn)).unwrap().requests_moved, 2);
        assert_eq!(per_request(&conn).len(), 1);
        assert_eq!(daily_days(&conn), vec![TODAY - 20, TODAY - 10]);
        assert_eq!(totals(&conn, Period::last(30, NOW).unwrap()).unwrap().sums, brute(&requests, |_| true));

        set_pref(&conn, "storage_retention_days", "29");
        assert_eq!(per_request_days(&conn), 29);
        set_pref(&conn, "storage_retention_days", "3");
        assert_eq!(per_request_days(&conn), 30, "a clean-up window under 7 is refused (commands/storage.rs), so off");
        for days in ["30", "90", "365"] {
            set_pref(&conn, "storage_retention_days", days);
            assert_eq!(per_request_days(&conn), 30);
        }
        set_pref(&conn, "storage_retention_days", "7");
        set_pref(&conn, "storage_retention_enabled", "0");
        assert_eq!(per_request_days(&conn), 30, "clean-up off");
    }

    /// U9.1. A window outside 7 to 30 days is held to the nearer end: 1
    /// moves what 7 moves, and 90 what 30 moves.
    #[test]
    fn the_roll_up_window_is_held_between_7_and_30_days() {
        for (asked, moved) in [(1, 3), (7, 3), (30, 1), (90, 1)] {
            let db = opened();
            let conn = db.conn.lock().unwrap();
            store(&conn, &[3, 10, 20, 40].map(|back| req((TODAY - back) * DAY + 60, "openai", "gpt-6-luna", "chat", 10, 1, 0, 0, Some(back))));
            assert_eq!(roll_up(&conn, NOW, asked).unwrap().requests_moved, moved, "asked for {asked} days");
        }
        assert_eq!([0, 3, 7, 29, 30, 365].map(|days| per_request_days_for(Some(days))), [7, 7, 7, 29, 30, 30]);
    }

    /// A day's total goes once all of the day is older than 730 days.
    #[test]
    fn daily_totals_are_deleted_after_730_days() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let requests = [731, 730, 729, 5].map(|back| req((TODAY - back) * DAY + 60, "openai", "gpt-6-luna", "chat", 1_000, back, 0, 0, Some(back)));
        store(&conn, &requests);

        assert_eq!(roll_up(&conn, NOW, 30).unwrap(), RolledUp { requests_moved: 3, totals_expired: 0 }, "the first run only records where the totals start");
        // The oldest total is older than the first day usage could exist
        // (FIRST_USAGE_DAY), so the day recorded is the one two years back.
        assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), Some(TODAY - 730));
        assert_eq!(roll_up(&conn, NOW, 30).unwrap(), RolledUp { requests_moved: 0, totals_expired: 1 });
        assert_eq!(daily_days(&conn), vec![TODAY - 730, TODAY - 729]);
        let two_years = totals(&conn, Period::last(730, NOW).unwrap()).unwrap();
        assert_eq!(two_years.sums, brute(&requests, |r| utc_day(r.ts) >= TODAY - 729));

        assert_eq!(roll_up(&conn, NOW + DAY, 30).unwrap().totals_expired, 1);
        assert_eq!(daily_days(&conn), vec![TODAY - 729]);
        assert_eq!(roll_up(&conn, NOW + DAY, 30).unwrap().totals_expired, 0);
    }

    /// A clock two years ahead may move requests early, but deletes at most
    /// one day of totals per run; set back, it deletes nothing, and the
    /// first day kept never moves back.
    #[test]
    fn a_wrong_clock_cannot_delete_the_history() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let requests = [729, 725, 400, 2].map(|back| req((TODAY - back) * DAY + 60, "openai", "gpt-6-luna", "chat", 1_000, back, 0, 0, Some(back)));
        store(&conn, &requests);
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), Some(TODAY - 730));

        let ahead = NOW + 730 * DAY;
        for run in 1..=10 {
            roll_up(&conn, ahead, 30).unwrap();
            assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), Some(TODAY - 730 + run), "run {run}");
        }
        assert_eq!(daily_days(&conn), vec![TODAY - 400, TODAY - 2], "ten runs, ten days: the two oldest are gone, nothing else");
        assert!(per_request(&conn).is_empty(), "moved early, under its own day");

        for wrong in [NOW, NOW - 730 * DAY] {
            assert_eq!(roll_up(&conn, wrong, 30).unwrap(), RolledUp::default());
            assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), Some(TODAY - 720));
        }
        assert_eq!(daily_days(&conn), vec![TODAY - 400, TODAY - 2]);
        assert_eq!(totals(&conn, Period::last(30, NOW).unwrap()).unwrap().sums, brute(&requests, |r| r.ts > (TODAY - 3) * DAY));
    }

    /// U4. The first launch after the update, with the clock two years
    /// ahead: every request is moved early under its own day, and the run
    /// that first records where the totals start deletes none of them. Each
    /// later run under that clock may delete one day, the bound above.
    #[test]
    fn the_first_run_with_totals_only_records_where_they_start() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let requests = [40, 20, 3].map(|back| req((TODAY - back) * DAY + 60, "openai", "gpt-6-luna", "chat", 1_000, back, 0, 0, Some(back)));
        store(&conn, &requests);
        assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), None);

        let ahead = NOW + 730 * DAY;
        assert_eq!(roll_up(&conn, ahead, 30).unwrap(), RolledUp { requests_moved: 3, totals_expired: 0 });
        assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), Some(TODAY - 40), "no later than the oldest total");
        assert_eq!(daily_days(&conn), vec![TODAY - 40, TODAY - 20, TODAY - 3]);
        assert_eq!(roll_up(&conn, ahead, 30).unwrap().totals_expired, 1, "the next run: one day");
        assert_eq!(daily_days(&conn), vec![TODAY - 20, TODAY - 3]);
    }

    /// R4-4. No first day kept is recorded yet; a total dated 2001 (a clock
    /// reset before the first roll-up) and a real one. The first run deletes
    /// nothing and records a day no earlier than the first day usage could
    /// exist; the next run deletes the 2001 total and keeps the real one.
    /// Before 3 May 2028 the day recorded is the one two years back, which is
    /// earlier than that first day; from then on it is that first day.
    #[test]
    fn the_first_day_kept_never_starts_before_usage_could_exist() {
        // 2026-05-04: `git log -1 --format=%aI 57da92f` is 2026-05-04T19:16:54+01:00.
        assert_eq!(chrono::DateTime::from_timestamp(FIRST_USAGE_DAY * DAY, 0).unwrap().format("%Y-%m-%d").to_string(), "2026-05-04");
        let reset = 11_323; // 2001-01-01
        for (now, recorded, next) in [(NOW, TODAY - 730, TODAY - 730), (NOW + 1_000 * DAY, FIRST_USAGE_DAY, FIRST_USAGE_DAY + 1)] {
            let db = opened();
            let conn = db.conn.lock().unwrap();
            store(&conn, &[
                req(reset * DAY + 60, "local", "qwen3:4b", "chat", 4_000, 900, 0, 0, Some(0)),
                req((FIRST_USAGE_DAY + 10) * DAY + 60, "openai", "gpt-6-luna", "chat", 10, 1, 0, 0, Some(3)),
            ]);
            assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), None);
            assert_eq!(roll_up(&conn, now, 30).unwrap(), RolledUp { requests_moved: 2, totals_expired: 0 }, "{now}");
            assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), Some(recorded), "{now}");
            assert!(recorded > reset, "not 2001");
            assert_eq!(roll_up(&conn, now, 30).unwrap().totals_expired, 1, "{now}");
            assert_eq!(daily_days(&conn), vec![FIRST_USAGE_DAY + 10], "{now}: the 2001 total is gone, the real one is there");
            // Expiry goes on from there, not one day at a time from 2001.
            assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), Some(next), "{now}");
        }
    }

    /// U5. A Mac whose clock reset to 1 January 2001: local requests stored
    /// then are dated before the first day kept, so the roll-up that moves
    /// them deletes them with the oldest totals. They are in no figure
    /// afterwards; the module comment says so.
    #[test]
    fn a_request_stored_under_a_clock_two_years_back_is_deleted_when_rolled_up() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[req((TODAY - 40) * DAY + 60, "openai", "gpt-6-luna", "chat", 10, 1, 0, 0, Some(3))]);
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(read_day(&conn, KEPT_FROM_KEY).unwrap(), Some(TODAY - 730));
        // 2001-01-01.
        let reset = 11_323;
        assert_eq!(chrono::DateTime::from_timestamp(reset * DAY, 0).unwrap().format("%Y-%m-%d").to_string(), "2001-01-01");
        store(&conn, &vec![req(reset * DAY + 60, "local", "qwen3:4b", "chat", 4_000, 900, 0, 0, Some(0)); 20]);

        assert_eq!(roll_up(&conn, NOW, 30).unwrap(), RolledUp { requests_moved: 20, totals_expired: 1 });
        assert!(per_request(&conn).is_empty());
        assert_eq!(daily_days(&conn), vec![TODAY - 40], "the 20 requests of 2001 are gone");
        assert_eq!(totals(&conn, Period::last(730, NOW).unwrap()).unwrap().sums.requests, 1);
    }

    /// Each period is the brute-force sum of the requests on its days, over
    /// both levels, a request on its first day counted and one on the day
    /// before not.
    #[test]
    fn every_period_is_the_sum_of_its_days_over_both_levels() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let requests = mixed();
        store(&conn, &requests);
        roll_up(&conn, NOW, 30).unwrap();
        assert!(!per_request(&conn).is_empty() && !daily(&conn).is_empty(), "both levels hold usage");

        for days in PERIODS {
            let first = TODAY - (days - 1);
            assert!(requests.iter().any(|r| utc_day(r.ts) == first) && requests.iter().any(|r| utc_day(r.ts) == first - 1), "{days}: both sides of the boundary");
            let keep = |r: &Req| utc_day(r.ts) >= first;
            let period = Period::last(days, NOW).unwrap();
            assert_eq!(period.first_day(), first);
            let found = totals(&conn, period).unwrap();
            let mut priced_days: Vec<i64> = requests.iter().filter(|r| keep(r) && r.cost.is_some()).map(|r| utc_day(r.ts)).collect();
            priced_days.sort();
            priced_days.dedup();
            let expected = Totals {
                sums: brute(&requests, keep),
                local_requests: requests.iter().filter(|r| keep(r) && r.provider == "local").count() as i64,
                priced_days: priced_days.len() as i64,
            };
            assert_eq!(found, expected, "{days} days");
        }
        assert!(Period::last(31, NOW).is_err());
        assert!(Period::last(0, NOW).is_err());
    }

    /// The groups, with the AWS figures of one Bedrock request (inputTokens
    /// 120, outputTokens 45, cache read 800, cache write 400) stored as
    /// input 1,320, unpriced groups and a local one; two of the requests
    /// are in the daily totals.
    #[test]
    fn usage_is_grouped_by_model_provider_feature_and_time() {
        assert_eq!(chrono::DateTime::from_timestamp(NOW, 0).unwrap().format("%Y-%m-%d").to_string(), "2026-11-13");
        let db = opened();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[
            req((TODAY - 2) * DAY + 100, "bedrock", HAIKU, "chat", 1_320, 45, 800, 400, Some(900)),
            req((TODAY - 40) * DAY + 100, "bedrock", HAIKU, "tools", 1_320, 45, 800, 400, Some(900)),
            req((TODAY - 2) * DAY + 200, "compat:api.moonshot.ai", "kimi-k2-0905-preview", "chat", 3_000, 20, 0, 0, None),
            req((TODAY - 70) * DAY + 200, "openai", "gpt-6-luna", "mail_sort", 1_000, 10, 0, 0, Some(105)),
            req((TODAY - 70) * DAY + 300, "openai", "gpt-unlisted-1", "mail_sort", 500, 5, 0, 0, None),
            req((TODAY - 1) * DAY, "local", "qwen3:4b", "mail_analysis", 4_000, 900, 0, 0, Some(0)),
        ]);
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(daily(&conn).len(), 3);
        let p90 = Period::last(90, NOW).unwrap();
        let sums = |requests, input, output, cache_read, cache_write, known, unknown| Sums {
            requests, input_tokens: input, output_tokens: output, cache_read_tokens: cache_read, cache_write_tokens: cache_write, known_cost_micro: known, price_unknown_requests: unknown,
            input_without_cache: input - cache_read - cache_write, speech_characters: 0,
        };
        let keys = |groups: &[Group]| groups.iter().map(|g| g.key.clone()).collect::<Vec<_>>();

        let models = groups(&conn, p90, GroupBy::Model).unwrap();
        assert_eq!(keys(&models), [HAIKU, "gpt-6-luna", "gpt-unlisted-1", "kimi-k2-0905-preview", "qwen3:4b"]);
        assert_eq!(models[0], Group { key: HAIKU.into(), provider: Some("bedrock".into()), from_day: None, sums: sums(2, 2_640, 90, 1_600, 800, 1_800, 0) });
        assert_eq!(models[0].sums.input_without_cache, 240, "120 per request, as AWS reported it");
        assert_eq!(models[3], Group { key: "kimi-k2-0905-preview".into(), provider: Some("compat:api.moonshot.ai".into()), from_day: None, sums: sums(1, 3_000, 20, 0, 0, 0, 1) });

        let providers = groups(&conn, p90, GroupBy::Provider).unwrap();
        assert_eq!(keys(&providers), ["bedrock", "openai", "compat:api.moonshot.ai", "local"]);
        assert_eq!(providers[1], Group { key: "openai".into(), provider: None, from_day: None, sums: sums(2, 1_500, 15, 0, 0, 105, 1) });

        let features = groups(&conn, p90, GroupBy::Feature).unwrap();
        assert_eq!(keys(&features), ["chat", "tools", "mail_sort", "mail_analysis"]);
        assert_eq!(features[0].sums, sums(2, 4_320, 65, 800, 400, 900, 1));

        let days = groups(&conn, p90, GroupBy::Time).unwrap();
        assert_eq!(keys(&days), ["2026-11-12", "2026-11-11", "2026-10-04", "2026-09-04"]);
        assert_eq!(days[1].sums, sums(2, 4_320, 65, 800, 400, 900, 1));
        assert_eq!(keys(&groups(&conn, Period::last(30, NOW).unwrap(), GroupBy::Time).unwrap()), ["2026-11-12", "2026-11-11"]);

        let months = groups(&conn, Period::last(180, NOW).unwrap(), GroupBy::Time).unwrap();
        assert_eq!(keys(&months), ["2026-11", "2026-10", "2026-09"]);
        assert_eq!(months[2].sums, sums(2, 1_500, 15, 0, 0, 105, 1));
        assert!(!by_month(p90) && by_month(Period::last(180, NOW).unwrap()));
        assert_eq!(GroupBy::parse("feature"), Ok(GroupBy::Feature));
        assert!(GroupBy::parse("prompt").is_err());
    }

    /// U1. Six months from 13 November 2026 start on 18 May. The May row
    /// holds 18 to 31 May and says so; the request of 17 May is in no row,
    /// though it is still on this Mac. A period that starts on the 1st, and
    /// a period grouped by day, have no part-month.
    #[test]
    fn the_oldest_month_says_from_which_day_it_counts() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let six_months = Period::last(180, NOW).unwrap();
        let first = six_months.first_day();
        assert_eq!(chrono::DateTime::from_timestamp(first * DAY, 0).unwrap().format("%Y-%m-%d").to_string(), "2026-05-18");
        let requests = [
            req((first - 1) * DAY + 60, "openai", "gpt-6-luna", "chat", 1_000, 10, 0, 0, Some(105)),
            req(first * DAY + 60, "openai", "gpt-6-luna", "chat", 2_000, 20, 0, 0, Some(210)),
            req((first + 20) * DAY + 60, "openai", "gpt-6-luna", "chat", 4_000, 40, 0, 0, Some(420)),
        ];
        store(&conn, &requests);
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(daily_days(&conn), vec![first - 1, first, first + 20], "all three are daily totals, the 17 May one too");

        let months = groups(&conn, six_months, GroupBy::Time).unwrap();
        let found: Vec<(&str, Option<&str>, i64)> = months.iter().map(|g| (g.key.as_str(), g.from_day.as_deref(), g.sums.requests)).collect();
        assert_eq!(found, [("2026-06", None, 1), ("2026-05", Some("2026-05-18"), 1)]);
        assert_eq!(months[1].sums, brute(&requests[1..2], |_| true), "the 17 May request is left out");

        // 30 November 2026: a year starts on 1 December 2025.
        let year = Period::last(365, (TODAY + 17) * DAY + 3_600).unwrap();
        store(&conn, &[req(year.first_day() * DAY + 60, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(1))]);
        let months = groups(&conn, year, GroupBy::Time).unwrap();
        assert_eq!(months.last().map(|g| (g.key.as_str(), g.from_day.as_deref())), Some(("2025-12", None)));
        assert!(groups(&conn, Period::last(90, NOW).unwrap(), GroupBy::Time).unwrap().iter().all(|g| g.from_day.is_none()));
        assert!(groups(&conn, six_months, GroupBy::Model).unwrap().iter().all(|g| g.from_day.is_none()));
    }

    /// A day with only unpriced usage is not a day of use for the average.
    #[test]
    fn priced_days_are_the_utc_days_with_a_known_price() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let period = Period::last(30, NOW).unwrap();
        assert_eq!(totals(&conn, period).unwrap(), Totals::default(), "nothing recorded");
        store(&conn, &[
            req((TODAY - 3) * DAY + 10, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(500_000)),
            req((TODAY - 3) * DAY + 20, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(250_000)),
            req((TODAY - 3) * DAY + 30, "openai", "gpt-unlisted-1", "chat", 1, 1, 0, 0, None),
            req((TODAY - 2) * DAY + 10, "openai", "gpt-unlisted-1", "chat", 1, 1, 0, 0, None),
            req(TODAY * DAY + 10, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(1_000_000)),
        ]);
        let found = totals(&conn, period).unwrap();
        assert_eq!((found.sums.known_cost_micro, found.priced_days, found.sums.price_unknown_requests), (1_750_000, 2, 2));
    }

    /// The costliest requests come from the per-request rows of the last 30
    /// UTC days; unpriced and local requests are not listed.
    #[test]
    fn the_most_expensive_requests_are_recent_single_requests() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[
            req((TODAY - 1) * DAY, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(5_000)),
            req((TODAY - 40) * DAY, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(9_000)),
            req((TODAY - 30) * DAY, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(8_000)),
            req((TODAY - 29) * DAY, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(100)),
            req((TODAY - 1) * DAY, "openai", "gpt-unlisted-1", "chat", 1, 1, 0, 0, None),
            req((TODAY - 1) * DAY, "local", "qwen3:4b", "chat", 1, 1, 0, 0, Some(0)),
            req((TODAY - 1) * DAY, "ollama", "llama3.2", "chat", 1, 1, 0, 0, Some(7_000)),
        ]);
        roll_up(&conn, NOW, 30).unwrap();
        let (days, requests) = expensive_requests(&conn, NOW, 5).unwrap();
        assert_eq!(days, 30);
        assert_eq!(requests.iter().map(|r| r.usd_cost_micro).collect::<Vec<_>>(), [5_000, 100]);
        assert_eq!(expensive_requests(&conn, NOW, 1).unwrap().1.len(), 1);
    }

    /// U2. Requests moved early are daily totals and cannot be listed: here
    /// a 7-day window moved them, as a clock that was ahead would. The list
    /// then covers the days after the newest total, and says how many; with
    /// today itself moved, it is today.
    #[test]
    fn the_most_expensive_requests_say_how_many_days_they_cover() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[3, 10, 20].map(|back| req((TODAY - back) * DAY + 60, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(1_000 * back))));
        roll_up(&conn, NOW, 7).unwrap();
        assert_eq!(daily_days(&conn), vec![TODAY - 20, TODAY - 10]);
        assert_eq!(per_request_days(&conn), 30, "asked with 30");
        let (days, requests) = expensive_requests(&conn, NOW, 5).unwrap();
        assert_eq!(days, 10, "today and the nine days after the newest total");
        assert_eq!(requests.iter().map(|r| r.usd_cost_micro).collect::<Vec<_>>(), [3_000]);

        // A clock 40 days ahead moved everything, today's request too; the
        // clock is right again and one more request comes in today.
        store(&conn, &[req(NOW - 600, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(9_000))]);
        roll_up(&conn, NOW + 40 * DAY, 30).unwrap();
        assert!(per_request(&conn).is_empty());
        store(&conn, &[req(NOW + 60, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(500))]);
        let (days, requests) = expensive_requests(&conn, NOW + 120, 5).unwrap();
        assert_eq!((days, requests.iter().map(|r| r.usd_cost_micro).collect::<Vec<_>>()), (1, vec![500]));
    }

    /// What the QA seed leaves once the QA launch has rolled up: the 0- and
    /// 5-day requests one by one, the 40- to 400-day ones as daily totals,
    /// speech in both, every kind priced as this build prices it, each period
    /// showing what it should, and a part-month row in the 6-month and the
    /// 1-year time view. native_qa.rs seeds inside its first-run transaction
    /// and rolls up after it.
    #[test]
    fn the_qa_seed_fills_both_levels_after_its_roll_up() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        seed_for_qa(&conn, NOW).unwrap();
        assert_eq!(per_request(&conn).len(), 34, "all seeded as requests");
        roll_up(&conn, NOW, 30).unwrap();

        let ages: Vec<i64> = per_request(&conn).iter().map(|((_, ts, ..), _)| TODAY - utc_day(*ts)).collect();
        assert_eq!((ages.len(), ages.iter().all(|age| *age <= 5)), (9, true), "{ages:?}");
        assert_eq!(daily_days(&conn), vec![TODAY - 400, TODAY - 364, TODAY - 200, TODAY - 179, TODAY - 80, TODAY - 40]);
        let flags: Vec<(String, i64, Option<i64>)> = daily(&conn).into_iter().filter(|((day, ..), _)| *day == TODAY - 40).map(|((_, provider, _, _, unknown), (.., cost))| (provider, unknown, cost.map(|c| c.signum()))).collect();
        assert_eq!(flags, [("bedrock".into(), 0, Some(1)), ("compat:api.moonshot.ai".into(), 1, None), ("local".into(), 0, Some(0)), ("minimax".into(), 1, None), ("openai".into(), 0, Some(1))]);

        let requests: Vec<i64> = PERIODS.iter().map(|days| totals(&conn, Period::last(*days, NOW).unwrap()).unwrap().sums.requests).collect();
        assert_eq!(requests, [9, 18, 22, 30, 34]);
        let month = totals(&conn, Period::last(30, NOW).unwrap()).unwrap();
        assert_eq!((month.sums.speech_characters, month.sums.price_unknown_requests, month.local_requests), (1_234, 3, 2));
        // Speech in a daily total too: the 90-day period adds the 40-day one.
        let speech = totals(&conn, Period::last(90, NOW).unwrap()).unwrap().sums.speech_characters;
        assert_eq!(speech, 2_468);

        // U1 in the window: the 6-month and the 1-year time views each have a
        // part-month row that says from which day it counts.
        let part_months = |days| -> Vec<(String, Option<String>)> {
            groups(&conn, Period::last(days, NOW).unwrap(), GroupBy::Time).unwrap().into_iter().filter(|g| g.from_day.is_some()).map(|g| (g.key, g.from_day)).collect()
        };
        assert_eq!(part_months(180), [("2026-05".into(), Some("2026-05-18".into()))]);
        assert_eq!(part_months(365), [("2025-11".into(), Some("2025-11-14".into()))]);

        let qa = include_str!("../native_qa.rs");
        let seeded = qa.find("crate::ai::usage::seed_for_qa(&tx, chrono::Utc::now().timestamp())?;").expect("the QA seed stores the usage");
        let committed = qa.find("tx.commit()?;").expect("in its transaction");
        let rolled = qa.find("crate::ai::usage::roll_up_on_open(conn);").expect("and rolls up after it");
        assert!(seeded < committed && committed < rolled);
    }

    /// The read gate: once per UTC day. A request stored later for a day
    /// already moved (a clock that went back) joins that day's total.
    #[test]
    fn usage_reads_roll_up_once_a_day() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        let old = req((TODAY - 40) * DAY, "openai", "gpt-6-luna", "chat", 10, 1, 0, 0, Some(3));
        store(&conn, std::slice::from_ref(&old));
        roll_up_if_due(&conn, NOW);
        assert!(per_request(&conn).is_empty());

        store(&conn, &[old]);
        roll_up_if_due(&conn, NOW + 3_600);
        assert_eq!(per_request(&conn).len(), 1, "the same UTC day: not again");
        roll_up_if_due(&conn, NOW + DAY);
        assert!(per_request(&conn).is_empty());
        assert_eq!(daily(&conn), vec![((TODAY - 40, "openai".into(), "gpt-6-luna".into(), "chat".into(), 0), (2, 20, 2, 0, 0, Some(6)))]);
    }

    #[test]
    fn usage_from_another_provider_is_found_in_either_level() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[req((TODAY - 1) * DAY, "gemini", "gemini-3-flash", "chat", 1, 1, 0, 0, Some(1))]);
        assert!(!any_usage_except(&conn, "gemini").unwrap());
        store(&conn, &[req((TODAY - 40) * DAY, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(1))]);
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(daily(&conn).len(), 1);
        assert!(any_usage_except(&conn, "gemini").unwrap());
    }

    /// The old rows are corrected before a roll-up moves a request: they
    /// land in the daily totals corrected, a guessed cost as price unknown
    /// and an old Bedrock row with its cache in the input.
    #[test]
    fn old_rows_reach_the_daily_totals_corrected() {
        // Not opened(): the step has not run on these rows yet.
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[
            // Kimi, billed by an older build at the guess: 42,600 micro-USD.
            req((TODAY - 80) * DAY, "compat:api.moonshot.ai", "kimi-k2-0905-preview", "chat", 12_000, 800, 2_000, 0, Some(42_600)),
            // v0.5.10 on Bedrock: input 120 without the cache, 805 micro-USD.
            req((TODAY - 80) * DAY, "bedrock", HAIKU, "chat", 120, 45, 800, 400, Some(805)),
        ]);
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(
            daily(&conn),
            vec![
                ((TODAY - 80, "bedrock".into(), HAIKU.into(), "chat".into(), 0), (1, 1_320, 45, 800, 400, Some(925))),
                ((TODAY - 80, "compat:api.moonshot.ai".into(), "kimi-k2-0905-preview".into(), "chat".into(), 1), (1, 12_000, 800, 2_000, 0, None)),
            ]
        );
    }

    /// While the correction fails, the roll-up moves no request; once it
    /// succeeds, the next roll-up corrects and moves them (what the log line
    /// of ai/cost.rs `correct_old_rows_on_open` says).
    #[test]
    fn a_failed_correction_stops_the_roll_up_until_it_succeeds() {
        // Not opened(): the step has not run on these rows yet.
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[req((TODAY - 80) * DAY, "bedrock", HAIKU, "chat", 120, 45, 800, 400, Some(805))]);
        conn.execute_batch("CREATE TEMP TRIGGER refuse BEFORE UPDATE OF input_tokens ON inference_metrics BEGIN SELECT RAISE(ABORT, 'disk full'); END;").unwrap();
        assert!(roll_up(&conn, NOW, 30).is_err());
        crate::ai::cost::correct_old_rows_on_open(&conn);
        assert!(roll_up(&conn, NOW, 30).is_err());
        assert_eq!(daily(&conn), vec![]);
        let stored: (i64, i64) = conn.query_row("SELECT COUNT(*), SUM(input_tokens) FROM inference_metrics", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(stored, (1, 120), "the request is still stored, uncorrected");

        conn.execute_batch("DROP TRIGGER refuse;").unwrap();
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(daily(&conn), vec![((TODAY - 80, "bedrock".into(), HAIKU.into(), "chat".into(), 0), (1, 1_320, 45, 800, 400, Some(925)))]);
    }

    /// N2. A row whose input is smaller than its cache counts its input as
    /// stored; a group's input without the cache is never below zero.
    #[test]
    fn input_below_its_cache_counts_as_stored_and_never_below_zero() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[
            req((TODAY - 1) * DAY, "bedrock", HAIKU, "chat", 1_320, 45, 800, 400, Some(925)),
            req((TODAY - 1) * DAY, "bedrock", HAIKU, "chat", 120, 45, 800, 400, Some(805)),
            req((TODAY - 2) * DAY, "claude", "claude-sonnet-4-6", "tools", 50, 400, 9_100, 120, Some(9_330)),
        ]);
        let groups = groups(&conn, Period::last(30, NOW).unwrap(), GroupBy::Model).unwrap();
        let without_cache: Vec<(String, i64, i64)> = groups.iter().map(|g| (g.key.clone(), g.sums.input_tokens, g.sums.input_without_cache)).collect();
        assert_eq!(without_cache, [("claude-sonnet-4-6".into(), 50, 50), (HAIKU.into(), 1_440, 240)]);
        let all = totals(&conn, Period::last(30, NOW).unwrap()).unwrap().sums;
        assert_eq!((all.input_without_cache, all.input_tokens - all.cache_read_tokens - all.cache_write_tokens), (290, -10_130), "subtracted from the sums it would be negative");
    }

    /// N5. MiniMax speech stores characters in the input column: they are
    /// never in a token figure, in either level or any grouping, and are
    /// counted as characters instead.
    #[test]
    fn speech_characters_are_never_counted_as_tokens() {
        let db = opened();
        let conn = db.conn.lock().unwrap();
        for back in [1, 40] {
            store(&conn, &[
                req((TODAY - back) * DAY + 10, "minimax", "speech-02-hd", "tts", 1_234, 0, 0, 0, None),
                req((TODAY - back) * DAY + 20, "gemini", crate::ai::models::GEMINI_FLASH, "stt", 900, 30, 0, 0, None),
                req((TODAY - back) * DAY + 30, "openai", "gpt-6-luna", "chat", 1_000, 10, 200, 0, Some(105)),
            ]);
        }
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(daily(&conn).len(), 3, "one level each");
        let p90 = Period::last(90, NOW).unwrap();
        let figures = |g: &Group| (g.key.clone(), g.sums.input_tokens, g.sums.output_tokens, g.sums.input_without_cache, g.sums.speech_characters);
        let by_feature: Vec<_> = groups(&conn, p90, GroupBy::Feature).unwrap().iter().map(figures).collect();
        assert_eq!(by_feature, [("chat".into(), 2_000, 20, 1_600, 0), ("stt".into(), 1_800, 60, 1_800, 0), ("tts".into(), 0, 0, 0, 2_468)]);
        let by_provider: Vec<_> = groups(&conn, p90, GroupBy::Provider).unwrap().iter().map(figures).collect();
        assert!(by_provider.contains(&("minimax".into(), 0, 0, 0, 2_468)), "{by_provider:?}");
        let by_day: Vec<_> = groups(&conn, p90, GroupBy::Time).unwrap().iter().map(figures).collect();
        assert_eq!(by_day, [("2026-11-12".into(), 1_900, 40, 1_700, 1_234), ("2026-10-04".into(), 1_900, 40, 1_700, 1_234)], "a day of speech and chat");
        let all = totals(&conn, p90).unwrap().sums;
        assert_eq!((all.input_tokens, all.speech_characters, all.requests), (3_800, 2_468, 6));
        assert!(include_str!("../voice/minimax.rs").contains(&format!("record_unpriced(db, \"{BILLED_BY_CHARACTER}\", ")), "MiniMax speech is stored under the provider counted in characters");
    }

    /// At every open: the migrations, then the correction of old rows, then
    /// the roll-up, then the database is handed out; the Storage clean-up
    /// runs after that, and the scheduler (P01), whose daily briefing makes
    /// an AI call and so stores usage, starts only after the clean-up.
    #[test]
    fn the_roll_up_runs_on_open_after_the_correction_and_before_the_clean_up() {
        let db_rs = include_str!("../db.rs");
        let open = &db_rs[db_rs.find("pub fn new()").unwrap()..];
        let open = &open[..open.find("fn db_path()").unwrap()];
        let migrated = open.find("embedded::migrations::runner().run(&mut conn)?;").unwrap();
        let prepared = open.find("prepare_usage(&conn);").expect("Database::new prepares the usage");
        assert!(migrated < prepared && prepared < open.find("Ok(Database {").unwrap());
        let steps = &db_rs[db_rs.find("pub(crate) fn prepare_usage(").unwrap()..];
        let steps = &steps[..steps.find("\n}\n").unwrap()];
        let corrected = steps.find("crate::ai::cost::correct_old_rows_on_open(conn);").unwrap();
        let rolled = steps.find("crate::ai::usage::roll_up_on_open(conn);").unwrap();
        assert!(corrected < rolled);
        let lib_rs = include_str!("../lib.rs");
        let swept = lib_rs.find("commands::storage::sweep_on_startup(&db);").unwrap();
        assert!(lib_rs.find("let db = match Database::new()").unwrap() < swept);
        assert!(swept < lib_rs.find("scheduler::Scheduler::new(").unwrap(), "the scheduler starts after the usage is prepared");
    }

    /// From v0.5.11 (V32), and from P01's V33: requests 5, 40 and 80 days
    /// old. After the update's first open the 5-day one is still a request,
    /// the others are daily totals, and 90 days shows all three.
    #[test]
    fn an_update_moves_older_requests_into_the_daily_totals() {
        for from in [32, 33] {
            an_update_moves_older_requests_into_the_daily_totals_from(from);
        }
    }

    fn an_update_moves_older_requests_into_the_daily_totals_from(from: i32) {
        let directory = std::env::temp_dir().join(format!("jarvis-usage-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("usage.db");
        let now = chrono::Utc::now().timestamp();
        {
            let db = crate::db::test_database_at_version(&path, from);
            let conn = db.conn.lock().unwrap();
            for (back, cost) in [(5, 1_000), (40, 2_000), (80, 4_000)] {
                conn.execute(
                    "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro)
                     VALUES (?1, 'tools', 'gpt-6-luna', ?2, 10, 100, 0, 40, 'openai', ?3)",
                    rusqlite::params![now - back * DAY, back * 100, cost],
                )
                .unwrap();
            }
        }
        let db = crate::db::test_database_at(&path);
        let conn = db.conn.lock().unwrap();
        crate::db::prepare_usage(&conn);

        let left: Vec<i64> = per_request(&conn).into_iter().map(|((_, ts, ..), _)| ts).collect();
        assert_eq!(left, [now - 5 * DAY]);
        assert_eq!(daily_days(&conn), vec![utc_day(now - 80 * DAY), utc_day(now - 40 * DAY)]);
        let ninety = totals(&conn, Period::last(90, now).unwrap()).unwrap().sums;
        assert_eq!(
            ninety,
            Sums { requests: 3, input_tokens: 12_500, output_tokens: 30, cache_read_tokens: 300, cache_write_tokens: 0, known_cost_micro: 7_000, price_unknown_requests: 0, input_without_cache: 12_200, speech_characters: 0 }
        );
        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// What Rust code runs outside tests: `source` with every comment and
    /// every `#[cfg(test)]` item blanked out, wherever it is in the file,
    /// newlines kept so lines keep their numbers. Strings are kept: the SQL
    /// is in them.
    fn runtime_code(source: &str) -> String {
        #[derive(Clone, Copy, PartialEq)]
        enum Kind {
            Code,
            Text,
            Comment,
        }
        let chars: Vec<char> = source.chars().collect();
        let n = chars.len();
        let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
        let ident = |i: Option<usize>| i.is_some_and(|i| chars[i].is_alphanumeric() || chars[i] == '_');
        let mut kinds = vec![Kind::Code; n];
        let mut i = 0;
        while i < n {
            let start = i;
            let kind = match (chars[i], at(i + 1)) {
                ('/', '/') => {
                    while i < n && chars[i] != '\n' {
                        i += 1;
                    }
                    Kind::Comment
                }
                ('/', '*') => {
                    let mut depth = 0;
                    while i < n {
                        if chars[i] == '/' && at(i + 1) == '*' {
                            depth += 1;
                            i += 2;
                        } else if chars[i] == '*' && at(i + 1) == '/' {
                            depth -= 1;
                            i += 2;
                            if depth == 0 {
                                break;
                            }
                        } else {
                            i += 1;
                        }
                    }
                    Kind::Comment
                }
                ('r', '"' | '#') if !ident(i.checked_sub(1)) || (chars[i - 1] == 'b' && !ident(i.checked_sub(2))) => {
                    let mut j = i + 1;
                    let mut hashes = 0;
                    while at(j) == '#' {
                        hashes += 1;
                        j += 1;
                    }
                    if at(j) != '"' {
                        // A raw identifier such as r#type.
                        i += 1;
                        continue;
                    }
                    j += 1;
                    while j < n && !(chars[j] == '"' && (1..=hashes).all(|k| at(j + k) == '#')) {
                        j += 1;
                    }
                    i = (j + 1 + hashes).min(n);
                    Kind::Text
                }
                ('"', _) => {
                    i += 1;
                    while i < n && chars[i] != '"' {
                        i += if chars[i] == '\\' { 2 } else { 1 };
                    }
                    i = (i + 1).min(n);
                    Kind::Text
                }
                ('\'', '\\') => {
                    i += 3;
                    while i < n && chars[i] != '\'' {
                        i += 1;
                    }
                    i = (i + 1).min(n);
                    Kind::Text
                }
                ('\'', _) if at(i + 2) == '\'' => {
                    i += 3;
                    Kind::Text
                }
                _ => {
                    i += 1;
                    continue;
                }
            };
            kinds[start..i].fill(kind);
        }
        let mut out = chars.clone();
        let blank = |out: &mut Vec<char>, from: usize, to: usize| {
            for c in &mut out[from..to.min(n)] {
                if *c != '\n' {
                    *c = ' ';
                }
            }
        };
        for (i, kind) in kinds.iter().enumerate() {
            if *kind == Kind::Comment {
                blank(&mut out, i, i + 1);
            }
        }
        let attribute: Vec<char> = "#[cfg(test)]".chars().collect();
        let mut i = 0;
        while i + attribute.len() <= n {
            if kinds[i] != Kind::Code || chars[i..i + attribute.len()] != attribute[..] {
                i += 1;
                continue;
            }
            // A variant, a field or an arm on one line ends in a comma: only
            // that line is test code.
            let item = (i + attribute.len()..n).find(|&k| !chars[k].is_whitespace()).unwrap_or(n);
            let line_end = (item..n).find(|&k| chars[k] == '\n').unwrap_or(n);
            if out[item..line_end].iter().rev().find(|c| !c.is_whitespace()) == Some(&',') {
                blank(&mut out, i, line_end);
                i = line_end;
                continue;
            }
            // Any other item ends at its first `;`, or at the brace that
            // closes its first `{`, whichever comes first.
            let mut j = i + attribute.len();
            while j < n && !(kinds[j] == Kind::Code && (chars[j] == '{' || chars[j] == ';')) {
                j += 1;
            }
            if j < n && chars[j] == '{' {
                let mut depth = 0;
                while j < n {
                    if kinds[j] == Kind::Code && chars[j] == '{' {
                        depth += 1;
                    } else if kinds[j] == Kind::Code && chars[j] == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
            }
            blank(&mut out, i, j + 1);
            i = j + 1;
        }
        out.into_iter().collect()
    }

    /// The span of `fn name(`, signature and body, in code from
    /// [`runtime_code`]; braces inside strings and char literals do not count.
    fn function_span(code: &str, signature: &str) -> Option<std::ops::Range<usize>> {
        let start = code.find(signature)?;
        let open = start + code[start..].find('{')?;
        let chars: Vec<(usize, char)> = code[open..].char_indices().collect();
        let (mut depth, mut in_string, mut i) = (0, false, 0);
        while i < chars.len() {
            let c = chars[i].1;
            if in_string {
                match c {
                    '\\' => i += 1,
                    '"' => in_string = false,
                    _ => {}
                }
            } else if c == '"' {
                in_string = true;
            } else if c == '\'' && chars.get(i + 2).is_some_and(|(_, c)| *c == '\'') {
                i += 2;
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    return Some(start..open + chars[i].0 + 1);
                }
            }
            i += 1;
        }
        None
    }

    /// The functions of ai/cost.rs that may name the tables: the one insert,
    /// and the one-time correction of old rows.
    const COST_RS_EXEMPT: [&str; 2] = ["fn insert_row(", "pub fn correct_old_rows("];

    /// Each line of runtime code in `name` (a path under src/) that names
    /// either table, in any case, quoted, qualified or built into a string.
    /// ai/usage.rs may; ai/cost.rs may only inside [`COST_RS_EXEMPT`].
    fn usage_table_mentions(name: &str, source: &str) -> Vec<String> {
        if name == "ai/usage.rs" {
            return Vec::new();
        }
        let mut code = runtime_code(source);
        if name == "ai/cost.rs" {
            for signature in COST_RS_EXEMPT {
                let span = function_span(&code, signature).unwrap_or_else(|| panic!("{signature} is in ai/cost.rs"));
                let blank: String = code[span.clone()].chars().map(|c| if c == '\n' { '\n' } else { ' ' }).collect();
                code.replace_range(span, &blank);
            }
        }
        code.lines()
            .enumerate()
            .filter(|(_, line)| {
                let line = line.to_lowercase();
                line.contains("inference_metrics") || line.contains("usage_daily")
            })
            .map(|(number, line)| format!("{name}:{}: {}", number + 1, line.trim()))
            .collect()
    }

    /// The migrations that define the two tables. A new one that changes or
    /// reads them (a view too) is reviewed with this module before it joins
    /// the list.
    const USAGE_MIGRATIONS: [&str; 4] = ["V13__aws_credentials_and_routing.sql", "V16__cost_telemetry.sql", "V34__usage_feature_and_unknown_price.sql", "V35__usage_registry_version.sql"];

    fn migration_mentions(file: &str, sql: &str) -> bool {
        let sql = sql.to_lowercase();
        !USAGE_MIGRATIONS.contains(&file) && (sql.contains("inference_metrics") || sql.contains("usage_daily"))
    }

    /// Every read of usage goes through this module, so the two levels are
    /// always added together (P07 and P10 too), and nothing else changes
    /// either table: other code may only store a request and correct the old
    /// rows once (the two functions of ai/cost.rs). Searched: all code that
    /// runs outside tests, in every file, for the bare table names, reads
    /// and writes alike; and every migration.
    #[test]
    fn no_other_code_reads_or_writes_the_usage_tables() {
        fn rust_files(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    rust_files(&path, found);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    found.push(path);
                }
            }
        }
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest.join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        assert!(files.len() > 50, "the sources were found");
        let mut found = Vec::new();
        for path in files {
            let name = path.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
            found.extend(usage_table_mentions(&name, &std::fs::read_to_string(&path).unwrap()));
        }
        let mut migrations = 0;
        for entry in std::fs::read_dir(manifest.join("migrations")).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            migrations += 1;
            if migration_mentions(&file, &std::fs::read_to_string(&path).unwrap()) {
                found.push(format!("migrations/{file}"));
            }
        }
        assert!(migrations > 30, "the migrations were found");
        assert!(found.is_empty(), "read and write usage through ai/usage.rs: {found:#?}");
    }

    /// U6. The guard sees each kind of use the review named, and what it
    /// leaves alone. It cannot see a table name assembled from pieces at run
    /// time (concat!("inference", "_metrics")); review has to.
    #[test]
    fn the_usage_guard_sees_each_kind_of_use() {
        let seen = |source: &str| !usage_table_mentions("commands/new.rs", source).is_empty();
        // 1. A table name put in at run time.
        assert!(seen("fn f(conn: &Connection) { let table = \"inference_metrics\"; conn.execute(&format!(\"DELETE FROM {table}\"), []).ok(); }"));
        // 2. Quoted, qualified, or in another case.
        assert!(seen("const Q: &str = \"SELECT * FROM \\\"usage_daily\\\"\";"));
        assert!(seen("const Q: &str = \"SELECT * FROM main.usage_daily\";"));
        assert!(seen("const Q: &str = r#\"SELECT * FROM Inference_Metrics\"#;"));
        // 3. A comma join.
        assert!(seen("const Q: &str = \"SELECT * FROM emails, inference_metrics\";"));
        // 4. Writes.
        assert!(seen("const Q: &str = \"UPDATE usage_daily SET requests = 0\";"));
        assert!(seen("const Q: &str = \"INSERT INTO usage_daily (day) VALUES (1)\";"));
        // 5. Code after a test module.
        assert!(seen("#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn later() { let _ = \"DELETE FROM inference_metrics\"; }"));
        // ... and after a test-only variant, field or arm (R4-5): only its
        // line is test code, so the next block is still searched.
        assert!(seen("enum Target {\n    #[cfg(test)]\n    Loopback(u16), // a port\n}\nfn later() { let _ = \"SELECT 1 FROM usage_daily\"; }"));
        assert!(seen("struct S {\n    #[cfg(test)] probe: u8,\n}\nimpl S { fn read() { let _ = \"SELECT 1 FROM inference_metrics\"; } }"));
        // 6. A migration, a view included.
        assert!(migration_mentions("V35__usage_view.sql", "CREATE VIEW spend AS SELECT * FROM usage_daily;"));
        assert!(!migration_mentions("V35__other.sql", "CREATE TABLE notes (id INTEGER);"));

        // Left alone: comments, test code, this module, and the two
        // functions of ai/cost.rs; another function there is not.
        assert!(!seen("// reads inference_metrics\n/* and usage_daily */\nfn f() {}"));
        assert!(!seen("#[cfg(test)]\nfn fixture() { let _ = \"INSERT INTO inference_metrics\"; }\nfn f() { let c = '{'; let _ = c; }"));
        assert!(!seen("match x {\n    #[cfg(test)]\n    Kind::Probe => \"DELETE FROM usage_daily\",\n    _ => \"\",\n}"));
        assert!(usage_table_mentions("ai/usage.rs", "const Q: &str = \"SELECT * FROM usage_daily\";").is_empty());
        let cost = "fn insert_row() { let _ = \"INSERT INTO inference_metrics\"; }\npub fn correct_old_rows() { if true { let _ = \"UPDATE inference_metrics {\"; } let _ = ('{', \"SELECT 1 FROM inference_metrics\"); }\nfn helper() { let _ = \"DELETE FROM usage_daily\"; }";
        assert_eq!(usage_table_mentions("ai/cost.rs", cost), ["ai/cost.rs:3: fn helper() { let _ = \"DELETE FROM usage_daily\"; }"]);
    }
}
