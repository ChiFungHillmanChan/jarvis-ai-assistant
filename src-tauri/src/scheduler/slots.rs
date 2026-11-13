//! Slot arithmetic for the scheduler. Pure: no clock, no database, no network.
//!
//! The words used here have one meaning each, in code, tests and the pull
//! request:
//!
//! - **Slot**: one scheduled occurrence of a job, as a wall-clock timestamp in
//!   the job's time zone. It is kept together with the UTC offset in force, so
//!   both the wall-clock reading and the instant can be compared.
//! - **Handled slot**: a slot whose run reached a final state (`completed`,
//!   `failed` or `skipped_offline_final`). Stored in `cron_jobs.handled_slot`.
//! - **Baseline**: the earliest time a missed slot can count. Stored in
//!   `cron_jobs.baseline_at`; set when a job is created and again when it goes
//!   from paused to active.
//! - **Missed slot**: the latest slot `s` with
//!   `max(handled_slot, baseline_at) < s <= now`. Only that one runs. Older
//!   missed slots collapse into it.
//! - **Trigger**: why a run started. See [`Trigger`].

use chrono::{DateTime, Duration, FixedOffset, Offset, TimeZone, Utc};
use croner::Cron;

/// One scheduled occurrence: the wall-clock reading plus the offset it was
/// read under.
pub type Slot = DateTime<FixedOffset>;

/// Why a run started. Stored in `cron_runs.trigger`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The live tick, within two minutes of the slot, with no wake or launch
    /// since the slot.
    Scheduled,
    /// Anything else that runs a missed slot: after sleep, after JARVIS was
    /// quit, after waiting for the network.
    CatchUp,
    /// The user asked for a run. Never changes `handled_slot`.
    Manual,
    /// A slot run again because its last run was cut short: once after
    /// JARVIS exited under it, and after every wake for a run the Mac slept
    /// under.
    RetryInterrupted,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Scheduled => "scheduled",
            Trigger::CatchUp => "catch_up",
            Trigger::Manual => "manual",
            Trigger::RetryInterrupted => "retry_interrupted",
        }
    }
}

/// How late the live tick may be and still count as on time.
pub const SCHEDULED_WINDOW_SECS: i64 = 120;

/// A job whose slots are at most this far apart follows real time when the
/// local clock moves backwards. See [`is_frequent`].
const FREQUENT_GAP_SECS: i64 = 3600;

/// A job that was never handled is not searched further back than this.
const MAX_LOOKBACK_DAYS: i64 = 400;

/// The collapsed count in a run record stops here; the record then says
/// "at least".
pub const COLLAPSED_COUNT_CAP: u32 = 5000;

/// The one parser. Validation, the preview in the dashboard and the scheduler
/// all read an expression in exactly this configuration (six fields, croner
/// weekdays from Sunday = 0), so the user is never shown a schedule other than
/// the one that runs.
pub fn parse_schedule(schedule: &str) -> Result<Cron, String> {
    Cron::new(schedule)
        .with_seconds_required()
        .with_dom_and_dow()
        .parse()
        .map_err(|e| format!("Invalid cron expression: {}", e))
}

pub fn format_slot(slot: &Slot) -> String {
    slot.to_rfc3339()
}

pub fn parse_slot(text: &str) -> Option<Slot> {
    DateTime::parse_from_rfc3339(text).ok()
}

/// The missed slot to run, and what collapsed into it.
#[derive(Clone, Debug, PartialEq)]
pub struct Missed {
    pub slot: Slot,
    /// Older missed slots that will not run. Capped at
    /// [`COLLAPSED_COUNT_CAP`].
    pub collapsed: u32,
    pub oldest_collapsed: Option<Slot>,
}

/// Whether consecutive slots of this schedule come an hour or less apart.
///
/// The distinction is the one Vixie cron makes for clock changes. A job that
/// fires at a set time of day ("09:00 daily") is identified by its wall-clock
/// reading, so the same local slot never runs twice after the clock moves
/// back. A job that fires every few minutes is a poll: holding it to
/// wall-clock identity would stop every sync, task alert and reminder for
/// eight hours after a flight from Hong Kong to London, because those
/// wall-clock slots "already ran". Such jobs follow real time instead.
pub fn is_frequent(cron: &Cron, around: DateTime<Utc>) -> bool {
    let slots: Vec<DateTime<Utc>> = cron.iter_after(around).take(4).collect();
    slots
        .windows(2)
        .any(|pair| (pair[1] - pair[0]).num_seconds() <= FREQUENT_GAP_SECS)
}

/// One job's slots: its schedule, the zone they are read in, and what has
/// already been settled.
struct Lookup<'a, Z: TimeZone> {
    cron: &'a Cron,
    zone: &'a Z,
    handled: Option<&'a Slot>,
    baseline: DateTime<Utc>,
    /// `Some` for a frequent job: the offset in force now. A frequent job
    /// follows real time, so its slots are laid out on that offset. A job at
    /// set times uses the zone's own rules instead, so "09:00" stays 09:00
    /// across a daylight-saving change.
    real_time_offset: Option<FixedOffset>,
}

struct Scan {
    first: Option<Slot>,
    last: Option<Slot>,
    count: u32,
}

impl<'a, Z: TimeZone> Lookup<'a, Z> {
    fn new(cron: &'a Cron, zone: &'a Z, handled: Option<&'a Slot>, baseline: DateTime<Utc>, now: DateTime<Utc>) -> Self {
        let real_time_offset = is_frequent(cron, now).then(|| zone.offset_from_utc_datetime(&now.naive_utc()).fix());
        Lookup { cron, zone, handled, baseline, real_time_offset }
    }

    /// Whether `slot` is still owed.
    ///
    /// The baseline is an instant (the moment the job was created or
    /// re-enabled). The handled slot is compared as an instant and, for a job
    /// at set times, as a wall-clock reading too: after the offset drops (a
    /// flight west, the end of daylight saving) the same local "09:00" comes
    /// round again at a later instant, and it must not run a second time.
    fn owed(&self, slot: &Slot) -> bool {
        if slot.with_timezone(&Utc) <= self.baseline {
            return false;
        }
        match self.handled {
            None => true,
            Some(handled) => {
                slot > handled && (self.real_time_offset.is_some() || slot.naive_local() > handled.naive_local())
            }
        }
    }

    /// The earliest instant a slot can still be owed from.
    fn lower_bound(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        let handled_at = self.handled.map(|slot| slot.with_timezone(&Utc));
        let lower = handled_at.map_or(self.baseline, |at| at.max(self.baseline));
        lower.max(now - Duration::days(MAX_LOOKBACK_DAYS))
    }

    /// Owed slots with `after < slot <= until`, oldest first, stopping after
    /// `limit` of them.
    fn scan(&self, after: DateTime<Utc>, until: DateTime<Utc>, limit: u32) -> Scan {
        match self.real_time_offset {
            Some(offset) => self.scan_in(&offset, after, until, limit),
            None => self.scan_in(self.zone, after, until, limit),
        }
    }

    fn scan_in<T: TimeZone>(&self, zone: &T, after: DateTime<Utc>, until: DateTime<Utc>, limit: u32) -> Scan {
        let mut found = Scan { first: None, last: None, count: 0 };
        for candidate in self.cron.iter_after(after.with_timezone(zone)) {
            let slot = candidate.fixed_offset();
            if slot.with_timezone(&Utc) > until {
                break;
            }
            // A wall-clock reading that occurs twice (the hour repeated when
            // daylight saving ends) is resolved to its first occurrence, which
            // can lie at or before `after`.
            if slot.with_timezone(&Utc) <= after || !self.owed(&slot) {
                continue;
            }
            if found.first.is_none() {
                found.first = Some(slot);
            }
            found.last = Some(slot);
            found.count += 1;
            if found.count >= limit {
                break;
            }
        }
        found
    }

    /// The latest owed slot at or before `now`.
    fn latest(&self, now: DateTime<Utc>) -> Option<Slot> {
        let lower = self.lower_bound(now);
        if lower >= now {
            return None;
        }
        // Widening windows back from now: a five-minute job that was off for a
        // month is settled by the first window, not by walking 8,640 slots.
        for window in [Duration::hours(1), Duration::days(1), Duration::days(32), Duration::days(MAX_LOOKBACK_DAYS)] {
            let start = lower.max(now - window);
            let found = self.scan(start, now, u32::MAX).last;
            if found.is_some() || start == lower {
                return found;
            }
        }
        None
    }
}

/// Owed slots that will not run because a later one ran in their place.
#[derive(Clone, Debug, PartialEq)]
pub struct Collapsed {
    /// Capped at [`COLLAPSED_COUNT_CAP`].
    pub count: u32,
    pub oldest: Option<Slot>,
    /// With a capped count this is the newest one counted, not necessarily
    /// the slot just before the one that ran.
    pub newest: Option<Slot>,
}

/// The owed slots older than `slot`: the ones that collapse into it.
pub fn collapsed_before<Z: TimeZone>(
    cron: &Cron,
    zone: &Z,
    handled: Option<&Slot>,
    baseline: DateTime<Utc>,
    slot: &Slot,
    now: DateTime<Utc>,
) -> Collapsed {
    let lookup = Lookup::new(cron, zone, handled, baseline, now);
    let before = slot.with_timezone(&Utc) - Duration::seconds(1);
    let older = lookup.scan(lookup.lower_bound(now), before, COLLAPSED_COUNT_CAP);
    Collapsed { count: older.count, oldest: older.first, newest: older.last }
}

/// The missed slot for a job: the latest slot `s` with
/// `max(handled_slot, baseline_at) < s <= now`, plus how many older ones
/// collapse into it. `None` when nothing is owed.
pub fn missed_slot<Z: TimeZone>(
    cron: &Cron,
    zone: &Z,
    handled: Option<&Slot>,
    baseline: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<Missed> {
    let slot = Lookup::new(cron, zone, handled, baseline, now).latest(now)?;
    let older = collapsed_before(cron, zone, handled, baseline, &slot, now);
    Some(Missed { slot, collapsed: older.count, oldest_collapsed: older.oldest })
}

/// The first owed slot after `now`: what `cron_jobs.next_run` shows, and when
/// the scheduler next has to look at the job.
pub fn next_slot<Z: TimeZone>(
    cron: &Cron,
    zone: &Z,
    handled: Option<&Slot>,
    baseline: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<Slot> {
    let lookup = Lookup::new(cron, zone, handled, baseline, now);
    lookup.scan(now, now + Duration::days(MAX_LOOKBACK_DAYS * 5), 1).first
}

/// How far ahead of now a handled slot or a baseline may lie before it counts
/// as written under a clock that has since been set back. A correction from
/// the time server is far smaller than this; a clock set back by less than
/// this only delays a job by that much.
pub const CLOCK_SET_BACK_TOLERANCE_SECS: i64 = 60;

/// Whether the handled slot, and whether the baseline, lie ahead of `now`.
///
/// Both are written at or before the moment they describe, so one that lies
/// ahead of now means the clock was ahead when it was written and has been
/// set back since. Instants are compared, so a flight west or the end of
/// daylight saving never looks like this: the local reading goes back there,
/// the instant does not.
pub fn written_ahead_of(now: DateTime<Utc>, handled: Option<&Slot>, baseline: Option<DateTime<Utc>>) -> (bool, bool) {
    let limit = now + Duration::seconds(CLOCK_SET_BACK_TOLERANCE_SECS);
    (handled.is_some_and(|slot| slot.with_timezone(&Utc) > limit), baseline.is_some_and(|at| at > limit))
}

/// `scheduled` only for the live tick: within two minutes of the slot, with no
/// wake or launch since the slot. Everything else that runs a missed slot is a
/// catch-up.
pub fn trigger_for(slot: &Slot, now: DateTime<Utc>, last_wake_or_launch: DateTime<Utc>) -> Trigger {
    let slot_at = slot.with_timezone(&Utc);
    let on_time = (now - slot_at).num_seconds() <= SCHEDULED_WINDOW_SECS;
    if on_time && last_wake_or_launch < slot_at {
        Trigger::Scheduled
    } else {
        Trigger::CatchUp
    }
}

#[cfg(test)]
pub(crate) mod test_zone {
    use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone};

    /// A zone whose offset changes once, at `change_utc`: a daylight-saving
    /// switch. Travel is not modelled this way. A flight replaces the Mac's
    /// zone outright, so a travel test reads the slots in a second, fixed
    /// zone.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct TestZone {
        pub change_utc: NaiveDateTime,
        pub before: FixedOffset,
        pub after: FixedOffset,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct TestOffset {
        zone: TestZone,
        fixed: FixedOffset,
    }

    impl Offset for TestOffset {
        fn fix(&self) -> FixedOffset {
            self.fixed
        }
    }

    impl TestZone {
        pub fn fixed(hours: i32) -> TestZone {
            let offset = FixedOffset::east_opt(hours * 3600).unwrap();
            TestZone { change_utc: NaiveDateTime::MAX, before: offset, after: offset }
        }

        /// `before_hours` until `change_utc` ("2026-10-25T01:00:00"), then
        /// `after_hours`.
        pub fn changing(change_utc: &str, before_hours: i32, after_hours: i32) -> TestZone {
            TestZone {
                change_utc: NaiveDateTime::parse_from_str(change_utc, "%Y-%m-%dT%H:%M:%S").unwrap(),
                before: FixedOffset::east_opt(before_hours * 3600).unwrap(),
                after: FixedOffset::east_opt(after_hours * 3600).unwrap(),
            }
        }

        fn wrap(&self, fixed: FixedOffset) -> TestOffset {
            TestOffset { zone: *self, fixed }
        }
    }

    impl TimeZone for TestZone {
        type Offset = TestOffset;

        fn from_offset(offset: &TestOffset) -> Self {
            offset.zone
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<TestOffset> {
            self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<TestOffset> {
            let valid_before = *local - self.before < self.change_utc;
            let valid_after = *local - self.after >= self.change_utc;
            match (valid_before, valid_after) {
                (true, true) => LocalResult::Ambiguous(self.wrap(self.before), self.wrap(self.after)),
                (true, false) => LocalResult::Single(self.wrap(self.before)),
                (false, true) => LocalResult::Single(self.wrap(self.after)),
                (false, false) => LocalResult::None,
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> TestOffset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> TestOffset {
            self.wrap(if *utc < self.change_utc { self.before } else { self.after })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_zone::TestZone;
    use super::*;

    const DAILY_0900: &str = "0 0 9 * * *";
    const EVERY_5_MIN: &str = "0 */5 * * * *";

    fn cron(expr: &str) -> Cron {
        parse_schedule(expr).unwrap()
    }

    /// An instant written with its offset: "2026-11-12T09:00:00+08:00".
    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc)
    }

    fn slot(text: &str) -> Slot {
        parse_slot(text).unwrap()
    }

    fn hk() -> TestZone {
        TestZone::fixed(8)
    }

    fn missed(expr: &str, zone: &TestZone, handled: Option<&str>, baseline: &str, now: &str) -> Option<Missed> {
        let handled = handled.map(slot);
        missed_slot(&cron(expr), zone, handled.as_ref(), at(baseline), at(now))
    }

    // Case: new job, never run. No catch-up for slots before the baseline;
    // the first run is at the next slot.
    #[test]
    fn a_new_job_does_not_catch_up_slots_before_its_baseline() {
        // Created at 10:00; the 09:00 slot that day is before the baseline.
        let created = "2026-11-12T10:00:00+08:00";
        assert_eq!(missed(DAILY_0900, &hk(), None, created, "2026-11-12T23:00:00+08:00"), None);
        // The next 09:00 is the first run.
        let first = missed(DAILY_0900, &hk(), None, created, "2026-11-13T09:00:00+08:00").unwrap();
        assert_eq!(first.slot, slot("2026-11-13T09:00:00+08:00"));
        assert_eq!(first.collapsed, 0);
        // And before it arrives, it is what `next_run` shows.
        let next = next_slot(&cron(DAILY_0900), &hk(), None, at(created), at("2026-11-12T23:00:00+08:00"));
        assert_eq!(next, Some(slot("2026-11-13T09:00:00+08:00")));
    }

    // Case: Mac awake, on time. The live tick runs the slot as `scheduled`.
    #[test]
    fn the_live_tick_on_time_is_scheduled() {
        let due = missed(DAILY_0900, &hk(), Some("2026-11-11T09:00:00+08:00"), "2026-11-01T00:00:00+08:00", "2026-11-12T09:00:01+08:00").unwrap();
        assert_eq!(due.slot, slot("2026-11-12T09:00:00+08:00"));
        assert_eq!(due.collapsed, 0);
        let launched = at("2026-11-12T07:00:00+08:00");
        assert_eq!(trigger_for(&due.slot, at("2026-11-12T09:00:01+08:00"), launched), Trigger::Scheduled);
        // Once handled, nothing is owed until the next slot.
        assert_eq!(missed(DAILY_0900, &hk(), Some("2026-11-12T09:00:00+08:00"), "2026-11-01T00:00:00+08:00", "2026-11-12T09:00:02+08:00"), None);
    }

    // Case: slept through one or more slots. One run of the latest missed
    // slot, as a catch-up; the older ones collapse.
    #[test]
    fn sleeping_through_slots_leaves_one_catch_up_of_the_latest() {
        // Asleep from 10:02 to 13:01: the 10:05 to 13:00 slots were missed.
        let due = missed(EVERY_5_MIN, &hk(), Some("2026-11-12T10:00:00+08:00"), "2026-11-01T00:00:00+08:00", "2026-11-12T13:01:00+08:00").unwrap();
        assert_eq!(due.slot, slot("2026-11-12T13:00:00+08:00"));
        assert_eq!(due.collapsed, 35, "10:05 to 12:55 collapse into the 13:00 run");
        assert_eq!(due.oldest_collapsed, Some(slot("2026-11-12T10:05:00+08:00")));
        let woke = at("2026-11-12T13:00:59+08:00");
        assert_eq!(trigger_for(&due.slot, at("2026-11-12T13:01:00+08:00"), woke), Trigger::CatchUp);
    }

    // Case: JARVIS quit through slots. Same as slept, on the next launch.
    #[test]
    fn slots_missed_while_quit_collapse_the_same_way_on_launch() {
        // Quit on the 9th after the 09:00 run, opened again on the 12th.
        let due = missed(DAILY_0900, &hk(), Some("2026-11-09T09:00:00+08:00"), "2026-11-01T00:00:00+08:00", "2026-11-12T15:00:00+08:00").unwrap();
        assert_eq!(due.slot, slot("2026-11-12T09:00:00+08:00"));
        assert_eq!(due.collapsed, 2);
        assert_eq!(due.oldest_collapsed, Some(slot("2026-11-10T09:00:00+08:00")));
        let launched = at("2026-11-12T15:00:00+08:00");
        assert_eq!(trigger_for(&due.slot, at("2026-11-12T15:00:00+08:00"), launched), Trigger::CatchUp);
    }

    /// A launch a few seconds after the slot is still a catch-up: within two
    /// minutes is not enough when JARVIS was not running at the slot.
    #[test]
    fn a_slot_just_before_launch_or_wake_is_a_catch_up() {
        let s = slot("2026-11-12T08:30:00+08:00");
        let now = at("2026-11-12T08:30:20+08:00");
        assert_eq!(trigger_for(&s, now, at("2026-11-12T08:30:10+08:00")), Trigger::CatchUp);
        assert_eq!(trigger_for(&s, now, at("2026-11-12T08:29:50+08:00")), Trigger::Scheduled);
        // Awake throughout, but the tick came more than two minutes late.
        assert_eq!(trigger_for(&s, at("2026-11-12T08:32:01+08:00"), at("2026-11-12T07:00:00+08:00")), Trigger::CatchUp);
        assert_eq!(trigger_for(&s, at("2026-11-12T08:32:00+08:00"), at("2026-11-12T07:00:00+08:00")), Trigger::Scheduled);
    }

    // Case: paused, then re-enabled. No catch-up for slots while paused; the
    // baseline is the re-enable time.
    #[test]
    fn re_enabling_does_not_catch_up_the_paused_period() {
        // Handled on the 5th, paused, re-enabled on the 12th at 10:00.
        let handled = Some("2026-11-05T09:00:00+08:00");
        let re_enabled = "2026-11-12T10:00:00+08:00";
        assert_eq!(missed(DAILY_0900, &hk(), handled, re_enabled, "2026-11-12T10:00:05+08:00"), None);
        let next = missed(DAILY_0900, &hk(), handled, re_enabled, "2026-11-13T09:00:00+08:00").unwrap();
        assert_eq!(next.slot, slot("2026-11-13T09:00:00+08:00"));
        assert_eq!(next.collapsed, 0, "the paused days are not collapsed runs either");
    }

    // Case: UTC offset change by travel, westwards. Travel replaces the Mac's
    // zone outright, so the slots are read in the new zone and compared with
    // what was handled in the old one. The same local slot does not run twice.
    #[test]
    fn flying_west_does_not_run_the_same_local_slot_twice() {
        // 09:00 ran in Hong Kong on the 12th. The Mac lands in Los Angeles,
        // where 09:00 on the 12th is still to come.
        let los_angeles = TestZone::fixed(-8);
        let handled = Some("2026-11-12T09:00:00+08:00");
        let baseline = "2026-11-01T00:00:00+08:00";
        // 09:00 on the 12th, Los Angeles time: same wall-clock slot.
        assert_eq!(missed(DAILY_0900, &los_angeles, handled, baseline, "2026-11-12T09:00:30-08:00"), None);
        assert_eq!(missed(DAILY_0900, &los_angeles, handled, baseline, "2026-11-12T23:00:00-08:00"), None);
        // The next local day's 09:00 is a new slot.
        let next = missed(DAILY_0900, &los_angeles, handled, baseline, "2026-11-13T09:00:00-08:00").unwrap();
        assert_eq!(next.slot, slot("2026-11-13T09:00:00-08:00"));
        assert_eq!(next.collapsed, 0);
        assert_eq!(
            next_slot(&cron(DAILY_0900), &los_angeles, Some(&slot("2026-11-12T09:00:00+08:00")), at(baseline), at("2026-11-12T08:00:00-08:00")),
            Some(slot("2026-11-13T09:00:00-08:00")),
            "re-registered on the new offset, the next run is tomorrow's 09:00, not today's again"
        );
    }

    // Case: UTC offset change by travel, eastwards. A local slot the flight
    // jumped over is one catch-up; a slot still ahead runs on the new offset.
    #[test]
    fn flying_east_runs_each_local_slot_once_on_the_new_offset() {
        // 09:00 ran in Los Angeles on the 11th (17:00 UTC). The Mac lands in
        // Hong Kong.
        let hong_kong = TestZone::fixed(8);
        let handled = Some("2026-11-11T09:00:00-08:00");
        let baseline = "2026-11-01T00:00:00-08:00";
        // Landing before 09:00 local on the 12th: nothing owed yet, and the
        // 12th's 09:00 is next, eight real hours after the last run.
        assert_eq!(missed(DAILY_0900, &hong_kong, handled, baseline, "2026-11-12T07:00:00+08:00"), None);
        let on_time = missed(DAILY_0900, &hong_kong, handled, baseline, "2026-11-12T09:00:00+08:00").unwrap();
        assert_eq!(on_time.slot, slot("2026-11-12T09:00:00+08:00"));
        // Landing after 09:00 local: the jumped slot is owed once.
        let jumped = missed(DAILY_0900, &hong_kong, handled, baseline, "2026-11-12T20:00:00+08:00").unwrap();
        assert_eq!(jumped.slot, slot("2026-11-12T09:00:00+08:00"));
        assert_eq!(jumped.collapsed, 0);
        assert_eq!(trigger_for(&jumped.slot, at("2026-11-12T20:00:00+08:00"), at("2026-11-01T00:00:00Z")), Trigger::CatchUp);
        // A slot that ran late in the day in Los Angeles is not owed again in
        // Hong Kong for the same local date, even though that date's 09:00 has
        // a later wall-clock reading than nothing at all: it is earlier in
        // real time than the run that already happened.
        let ran_on_the_12th = Some("2026-11-12T09:00:00-08:00");
        assert_eq!(missed(DAILY_0900, &hong_kong, ran_on_the_12th, baseline, "2026-11-13T08:00:00+08:00"), None);
        let thirteenth = missed(DAILY_0900, &hong_kong, ran_on_the_12th, baseline, "2026-11-13T09:00:00+08:00").unwrap();
        assert_eq!(thirteenth.slot, slot("2026-11-13T09:00:00+08:00"));
    }

    /// The reason frequent jobs follow real time: after a flight west the
    /// five-minute jobs (sync, task alerts, reminders) must keep running,
    /// although every wall-clock slot for the next sixteen hours "already ran".
    #[test]
    fn a_frequent_job_keeps_running_after_the_clock_moves_back() {
        let zone = TestZone::fixed(-8);
        // Last run 23:55 on the 12th in Hong Kong; five minutes later the Mac
        // reads 08:00 on the 12th, Los Angeles time.
        let due = missed(EVERY_5_MIN, &zone, Some("2026-11-12T23:55:00+08:00"), "2026-11-01T00:00:00+08:00", "2026-11-12T08:00:10-08:00").unwrap();
        assert_eq!(due.slot, slot("2026-11-12T08:00:00-08:00"));
        assert_eq!(due.collapsed, 0);
        assert!(is_frequent(&cron(EVERY_5_MIN), at("2026-11-12T00:00:00Z")));
        assert!(is_frequent(&cron("0 0 * * * *"), at("2026-11-12T00:00:00Z")), "hourly counts as frequent");
        assert!(!is_frequent(&cron(DAILY_0900), at("2026-11-12T00:00:00Z")));
        assert!(!is_frequent(&cron("0 0 9,18 * * *"), at("2026-11-12T00:00:00Z")));
        // Frequent inside its window, even when asked just before the window
        // closes for the day.
        assert!(is_frequent(&cron("0 */5 9-17 * * *"), at("2026-11-12T17:52:00Z")));
    }

    // Case: UTC offset change by daylight saving, clocks back. London,
    // 25 October 2026: 02:00 BST becomes 01:00 GMT, so 01:30 happens twice.
    #[test]
    fn the_repeated_hour_runs_a_set_time_job_once() {
        let london = TestZone::changing("2026-10-25T01:00:00", 1, 0);
        let nightly = "0 30 1 * * *";
        let baseline = "2026-10-01T00:00:00+01:00";
        let first = missed(nightly, &london, Some("2026-10-24T01:30:00+01:00"), baseline, "2026-10-25T01:30:00+01:00").unwrap();
        assert_eq!(first.slot, slot("2026-10-25T01:30:00+01:00"));
        // An hour later the wall clock reads 01:30 again.
        let handled = Some("2026-10-25T01:30:00+01:00");
        assert_eq!(missed(nightly, &london, handled, baseline, "2026-10-25T01:30:00+00:00"), None);
        assert_eq!(missed(nightly, &london, handled, baseline, "2026-10-25T23:00:00+00:00"), None);
        let next = missed(nightly, &london, handled, baseline, "2026-10-26T01:30:00+00:00").unwrap();
        assert_eq!(next.slot, slot("2026-10-26T01:30:00+00:00"));
    }

    /// A five-minute job runs through the repeated hour at its real cadence.
    #[test]
    fn the_repeated_hour_does_not_pause_a_frequent_job() {
        let london = TestZone::changing("2026-10-25T01:00:00", 1, 0);
        let baseline = "2026-10-01T00:00:00+01:00";
        // 01:55 BST ran; five minutes later it is 01:00 GMT.
        let due = missed(EVERY_5_MIN, &london, Some("2026-10-25T01:55:00+01:00"), baseline, "2026-10-25T01:00:05+00:00").unwrap();
        assert_eq!(due.slot, slot("2026-10-25T01:00:00+00:00"));
        assert_eq!(due.collapsed, 0);
    }

    // Case: UTC offset change by daylight saving, clocks forward. London,
    // 29 March 2026: 01:00 GMT becomes 02:00 BST, so 01:30 never happens.
    #[test]
    fn a_slot_inside_the_skipped_hour_runs_once_when_the_clock_jumps() {
        let london = TestZone::changing("2026-03-29T01:00:00", 0, 1);
        let nightly = "0 30 1 * * *";
        let baseline = "2026-03-01T00:00:00+00:00";
        let handled = Some("2026-03-28T01:30:00+00:00");
        assert_eq!(missed(nightly, &london, handled, baseline, "2026-03-29T00:59:59+00:00"), None);
        let due = missed(nightly, &london, handled, baseline, "2026-03-29T02:00:00+01:00").unwrap();
        assert_eq!(due.slot, slot("2026-03-29T02:00:00+01:00"), "the first valid moment after the gap");
        // And 09:00 stays 09:00 local across the change, not 10:00.
        let morning = missed(DAILY_0900, &london, Some("2026-03-28T09:00:00+00:00"), baseline, "2026-03-29T09:00:00+01:00").unwrap();
        assert_eq!(morning.slot, slot("2026-03-29T09:00:00+01:00"));
        assert_eq!(missed(DAILY_0900, &london, Some("2026-03-28T09:00:00+00:00"), baseline, "2026-03-29T08:59:59+01:00"), None);
    }

    /// A job off for a very long time is still settled quickly, and the
    /// record says "at least" rather than counting for ever.
    #[test]
    fn a_long_gap_is_bounded() {
        let due = missed(EVERY_5_MIN, &hk(), Some("2026-01-01T00:00:00+08:00"), "2025-01-01T00:00:00+08:00", "2026-11-12T13:01:00+08:00").unwrap();
        assert_eq!(due.slot, slot("2026-11-12T13:00:00+08:00"));
        assert_eq!(due.collapsed, COLLAPSED_COUNT_CAP);
    }

    /// A handled slot or a baseline that lies ahead of now was written under a
    /// clock that has since been set back. Travel and daylight saving never
    /// look like that: there the local reading goes back, the instant does not.
    #[test]
    fn only_a_clock_set_back_puts_a_handled_slot_ahead_of_now() {
        let now = at("2026-11-12T10:00:00+08:00");
        let ahead = |handled: &str| written_ahead_of(now, Some(&slot(handled)), None).0;
        assert!(ahead("2026-11-12T12:00:00+08:00"), "two hours ahead");
        assert!(ahead("2026-12-12T09:00:00+08:00"), "a month ahead");
        assert!(!ahead("2026-11-12T10:00:00+08:00"), "handled just now");
        assert!(!ahead("2026-11-12T10:00:30+08:00"), "a correction from the time server");
        assert!(!ahead("2026-11-11T09:00:00+08:00"), "yesterday");
        assert_eq!(written_ahead_of(now, None, None), (false, false), "a job that has never run");
        assert_eq!(written_ahead_of(now, None, Some(at("2026-12-12T09:58:00+08:00"))), (false, true), "created under a clock that was ahead");
        assert_eq!(written_ahead_of(now, None, Some(at("2026-11-12T09:58:00+08:00"))), (false, false));

        // A flight west: 09:00 in Hong Kong was handled, and an hour later the
        // Mac reads 18:00 the day before, in Los Angeles.
        let after_landing = at("2026-11-11T18:00:00-08:00");
        assert_eq!(written_ahead_of(after_landing, Some(&slot("2026-11-12T09:00:00+08:00")), None), (false, false));
        // The end of daylight saving in London: 01:30 summer time was handled,
        // and ten minutes later the Mac reads 01:10.
        let after_the_change = at("2026-10-25T01:10:00+00:00");
        assert_eq!(written_ahead_of(after_the_change, Some(&slot("2026-10-25T01:30:00+01:00")), None), (false, false));
    }

    #[test]
    fn slots_round_trip_through_storage() {
        let s = slot("2026-11-12T09:00:00+08:00");
        assert_eq!(format_slot(&s), "2026-11-12T09:00:00+08:00");
        assert_eq!(parse_slot(&format_slot(&s)), Some(s));
        // The baseline written by the migration is UTC with a Z.
        assert_eq!(parse_slot("2026-11-12T01:00:00Z").unwrap(), s);
        assert_eq!(parse_slot("yesterday"), None);
    }

    #[test]
    fn triggers_have_the_stored_names() {
        assert_eq!(Trigger::Scheduled.as_str(), "scheduled");
        assert_eq!(Trigger::CatchUp.as_str(), "catch_up");
        assert_eq!(Trigger::Manual.as_str(), "manual");
        assert_eq!(Trigger::RetryInterrupted.as_str(), "retry_interrupted");
    }
}
