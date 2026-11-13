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
//! Days are UTC days (`ts / 86400`) in both levels, so a period is exact to
//! the day. Nothing here leaves the Mac, and the log gets counts only.
//!
//! The interface. Other code, P07 and P10 included, reads usage only through
//! these, never from the two tables:
//! - [`Period::last`]: the last N UTC days including today, N in [`PERIODS`].
//! - [`totals`]: the sums over a period, from both levels.
//! - [`groups`]: the same sums split by model, provider, feature or time.
//! - [`expensive_requests`]: the costliest single requests, from the
//!   per-request level only (a daily total has no single requests).
//! - [`any_usage_except`]: whether usage from any other provider than one is
//!   on record, in either level.
//! - [`roll_up_on_open`] and [`roll_up_if_due`] keep the levels; [`roll_up`]
//!   is the step itself, and [`per_request_days`] its window.

use rusqlite::{Connection, OptionalExtension};

/// Seconds in a UTC day.
const DAY: i64 = 86_400;

/// The periods the Usage page offers, in days.
pub const PERIODS: [i64; 5] = [30, 90, 180, 365, 730];

/// How long each request keeps its own row, unless a shorter Storage
/// clean-up window asks for less.
pub const PER_REQUEST_DAYS: i64 = 30;

/// How long the daily totals are kept.
pub const DAILY_TOTALS_DAYS: i64 = 730;

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

/// Usage added up, the same way for both levels.
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
}

impl Sums {
    /// Input without the cache, the way AWS reports inputTokens.
    pub fn input_without_cache(&self) -> i64 {
        (self.input_tokens - self.cache_read_tokens - self.cache_write_tokens).max(0)
    }

    fn add(&mut self, other: &Sums) {
        self.requests += other.requests;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.known_cost_micro += other.known_cost_micro;
        self.price_unknown_requests += other.price_unknown_requests;
    }

    /// Read from the columns [`SUMS`] selects, starting at `at`.
    fn read(row: &rusqlite::Row, at: usize) -> rusqlite::Result<Sums> {
        Ok(Sums {
            requests: row.get(at)?,
            input_tokens: row.get(at + 1)?,
            output_tokens: row.get(at + 2)?,
            cache_read_tokens: row.get(at + 3)?,
            cache_write_tokens: row.get(at + 4)?,
            known_cost_micro: row.get(at + 5)?,
            price_unknown_requests: row.get(at + 6)?,
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

/// The columns [`Sums::read`] reads.
const SUMS: &str = "
    COALESCE(SUM(requests), 0), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
    COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_write_tokens), 0),
    COALESCE(SUM(CASE WHEN price_unknown = 0 THEN usd_cost_micro END), 0),
    COALESCE(SUM(CASE WHEN price_unknown = 1 THEN requests ELSE 0 END), 0)";

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
    pub sums: Sums,
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
        "SELECT {key}, {provider}, {SUMS} FROM ({BOTH_LEVELS}) GROUP BY 1, 2 ORDER BY {order}"
    ))?;
    let rows = stmt.query_map(period_params(period), |r| Ok(Group { key: r.get(0)?, provider: r.get(1)?, sums: Sums::read(r, 2)? }))?;
    rows.collect()
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

/// The `limit` requests with the highest known cost above zero over the
/// last [`per_request_days`] UTC days, and that number of days. From the
/// per-request level only; local requests are free and not listed.
pub fn expensive_requests(conn: &Connection, now: i64, limit: usize) -> rusqlite::Result<(i64, Vec<Request>)> {
    let days = per_request_days(conn);
    let mut stmt = conn.prepare(
        "SELECT ts, provider, model_id, workload, feature, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, usd_cost_micro, latency_ms
         FROM inference_metrics
         WHERE ts >= ?1 AND price_unknown = 0 AND usd_cost_micro > 0
         ORDER BY usd_cost_micro DESC, ts DESC",
    )?;
    let rows = stmt.query_map([Period::ending(days, now).first_day * DAY], |r| {
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

/// The per-request window for a Storage clean-up window (None: clean-up off).
pub fn per_request_days_for(clean_up_days: Option<i64>) -> i64 {
    match clean_up_days {
        Some(days) if days < PER_REQUEST_DAYS => days,
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
/// The old guessed costs are cleared first (M2 / D5): a daily total is
/// never corrected afterwards. That returns at once when it is done.
///
/// Expiry is the one step that cannot be undone, so a wrong clock is not
/// trusted with it: the first day kept moves forward by at most one day per
/// run, and never back. A clock set two years ahead deletes one day of the
/// oldest totals per run, not two years. Moving requests early under such a
/// clock loses nothing: each stays under its own day.
pub fn roll_up(conn: &Connection, now: i64, per_request_days: i64) -> rusqlite::Result<RolledUp> {
    crate::ai::cost::clear_guessed_costs(conn)?;
    let today = utc_day(now);
    // The day before this one ended at least `per_request_days` days ago.
    let first_kept_ts = (today - per_request_days.max(1)) * DAY;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO usage_daily
            (day, provider, model_id, feature, price_unknown, requests,
             input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro)
         SELECT CAST(ts / 86400 AS INTEGER), provider, model_id, feature, price_unknown, COUNT(*),
                SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens),
                CASE WHEN price_unknown = 0 THEN COALESCE(SUM(usd_cost_micro), 0) END
         FROM inference_metrics WHERE ts < ?1
         GROUP BY 1, 2, 3, 4, 5
         ON CONFLICT (day, provider, model_id, feature, price_unknown) DO UPDATE SET
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
        None => match tx.query_row("SELECT MIN(day) FROM usage_daily", [], |r| r.get::<_, Option<i64>>(0))? {
            // The first run with totals starts no later than the oldest.
            Some(oldest) => oldest.min(due),
            None => return Ok(0),
        },
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

/// The roll-up when the database opens: after the migrations and the old
/// guess correction (db.rs), before the Storage clean-up (lib.rs).
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
        let db = crate::db::test_database();
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
        let db = crate::db::test_database();
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
        let db = crate::db::test_database();
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
        let db = crate::db::test_database();
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

    /// A clean-up window under 30 days moves requests sooner, and they still
    /// count in the 30-day view. With clean-up off the window is 30 days.
    #[test]
    fn a_short_clean_up_window_moves_requests_without_losing_them() {
        let db = crate::db::test_database();
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
        for days in ["30", "90", "365"] {
            set_pref(&conn, "storage_retention_days", days);
            assert_eq!(per_request_days(&conn), 30);
        }
        set_pref(&conn, "storage_retention_days", "7");
        set_pref(&conn, "storage_retention_enabled", "0");
        assert_eq!(per_request_days(&conn), 30, "clean-up off");
    }

    /// A day's total goes once all of the day is older than 730 days.
    #[test]
    fn daily_totals_are_deleted_after_730_days() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        let requests = [731, 730, 729, 5].map(|back| req((TODAY - back) * DAY + 60, "openai", "gpt-6-luna", "chat", 1_000, back, 0, 0, Some(back)));
        store(&conn, &requests);

        assert_eq!(roll_up(&conn, NOW, 30).unwrap(), RolledUp { requests_moved: 3, totals_expired: 1 });
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
        let db = crate::db::test_database();
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

    /// Each period is the brute-force sum of the requests on its days, over
    /// both levels, a request on its first day counted and one on the day
    /// before not.
    #[test]
    fn every_period_is_the_sum_of_its_days_over_both_levels() {
        let db = crate::db::test_database();
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
        let db = crate::db::test_database();
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
        };
        let keys = |groups: &[Group]| groups.iter().map(|g| g.key.clone()).collect::<Vec<_>>();

        let models = groups(&conn, p90, GroupBy::Model).unwrap();
        assert_eq!(keys(&models), [HAIKU, "gpt-6-luna", "gpt-unlisted-1", "kimi-k2-0905-preview", "qwen3:4b"]);
        assert_eq!(models[0], Group { key: HAIKU.into(), provider: Some("bedrock".into()), sums: sums(2, 2_640, 90, 1_600, 800, 1_800, 0) });
        assert_eq!(models[0].sums.input_without_cache(), 240, "120 per request, as AWS reported it");
        assert_eq!(models[3], Group { key: "kimi-k2-0905-preview".into(), provider: Some("compat:api.moonshot.ai".into()), sums: sums(1, 3_000, 20, 0, 0, 0, 1) });

        let providers = groups(&conn, p90, GroupBy::Provider).unwrap();
        assert_eq!(keys(&providers), ["bedrock", "openai", "compat:api.moonshot.ai", "local"]);
        assert_eq!(providers[1], Group { key: "openai".into(), provider: None, sums: sums(2, 1_500, 15, 0, 0, 105, 1) });

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

    /// A day with only unpriced usage is not a day of use for the average.
    #[test]
    fn priced_days_are_the_utc_days_with_a_known_price() {
        let db = crate::db::test_database();
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
        let db = crate::db::test_database();
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

    /// The read gate: once per UTC day. A request stored later for a day
    /// already moved (a clock that went back) joins that day's total.
    #[test]
    fn usage_reads_roll_up_once_a_day() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        let old = req((TODAY - 40) * DAY, "openai", "gpt-6-luna", "chat", 10, 1, 0, 0, Some(3));
        store(&conn, &[old.clone()]);
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
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        store(&conn, &[req((TODAY - 1) * DAY, "gemini", "gemini-3-flash", "chat", 1, 1, 0, 0, Some(1))]);
        assert!(!any_usage_except(&conn, "gemini").unwrap());
        store(&conn, &[req((TODAY - 40) * DAY, "openai", "gpt-6-luna", "chat", 1, 1, 0, 0, Some(1))]);
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(daily(&conn).len(), 1);
        assert!(any_usage_except(&conn, "gemini").unwrap());
    }

    /// The old guessed costs are cleared before a roll-up moves a request:
    /// a guessed request lands in the totals as price unknown.
    #[test]
    fn a_guessed_cost_reaches_the_totals_as_price_unknown() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        // Kimi, billed by an older build at the guess: 42,600 micro-USD.
        store(&conn, &[req((TODAY - 80) * DAY, "compat:api.moonshot.ai", "kimi-k2-0905-preview", "chat", 12_000, 800, 2_000, 0, Some(42_600))]);
        roll_up(&conn, NOW, 30).unwrap();
        assert_eq!(daily(&conn), vec![((TODAY - 80, "compat:api.moonshot.ai".into(), "kimi-k2-0905-preview".into(), "chat".into(), 1), (1, 12_000, 800, 2_000, 0, None))]);
    }

    /// At every open: the migrations, then the old-guess correction, then
    /// the roll-up, then the database is handed out; the Storage clean-up
    /// runs after that.
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
        let corrected = steps.find("crate::ai::cost::clear_guessed_costs_on_open(conn);").unwrap();
        let rolled = steps.find("crate::ai::usage::roll_up_on_open(conn);").unwrap();
        assert!(corrected < rolled);
        let lib_rs = include_str!("../lib.rs");
        assert!(lib_rs.find("let db = match Database::new()").unwrap() < lib_rs.find("commands::storage::sweep_on_startup(&db);").unwrap());
    }

    /// From v0.5.11 (V32): requests 5, 40 and 80 days old. After the
    /// update's first open the 5-day one is still a request, the others are
    /// daily totals, and 90 days shows all three.
    #[test]
    fn an_update_moves_older_requests_into_the_daily_totals() {
        let directory = std::env::temp_dir().join(format!("jarvis-usage-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("usage.db");
        let now = chrono::Utc::now().timestamp();
        {
            let db = crate::db::test_database_at_version(&path, 32);
            let conn = db.conn.lock().unwrap();
            for (back, cost) in [(5, 1_000), (40, 2_000), (80, 4_000)] {
                conn.execute(
                    "INSERT INTO inference_metrics (ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, provider, usd_cost_micro)
                     VALUES (?1, 'tools', 'claude-sonnet-4-6', ?2, 10, 100, 0, 40, 'claude', ?3)",
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
        assert_eq!(ninety, Sums { requests: 3, input_tokens: 12_500, output_tokens: 30, cache_read_tokens: 300, cache_write_tokens: 0, known_cost_micro: 7_000, price_unknown_requests: 0 });
        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// Every read of usage goes through this module, so the two levels are
    /// always added together (P07 and P10 too). Other code may store a
    /// request and clear the old guessed costs (both in ai/cost.rs), but
    /// reads neither table, and deletes from neither.
    #[test]
    fn no_other_code_reads_the_usage_tables() {
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
        /// The source before its test module.
        fn runtime(source: &str) -> &str {
            let mut at = 0;
            while let Some(found) = source[at..].find("#[cfg(test)]") {
                let start = at + found;
                let next = source[start + "#[cfg(test)]".len()..].trim_start();
                if ["mod ", "pub mod ", "pub(crate) mod "].iter().any(|item| next.starts_with(item)) {
                    return &source[..start];
                }
                at = start + 1;
            }
            source
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        assert!(files.len() > 50, "the sources were found");
        let mut reads = Vec::new();
        for path in files {
            let name = path.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
            if name == "ai/usage.rs" {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let mut code = runtime(&source).to_string();
            if name == "ai/cost.rs" {
                let start = code.find("pub fn clear_guessed_costs(").expect("the correction is in cost.rs");
                let end = start + code[start..].find("\n}\n").unwrap();
                code.replace_range(start..end, "");
            }
            let code = code.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            for table in ["inference_metrics", "usage_daily"] {
                for verb in ["from", "join"] {
                    if code.contains(&format!("{verb} {table}")) {
                        reads.push(format!("{name}: {verb} {table}"));
                    }
                }
            }
        }
        assert!(reads.is_empty(), "read usage through ai/usage.rs: {reads:?}");
    }
}
