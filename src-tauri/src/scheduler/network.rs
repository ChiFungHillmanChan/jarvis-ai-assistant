//! Which jobs need the network, whether the Mac has one, and what a failed
//! run means when it does not.

/// Whether a job can do its work without the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkClass {
    /// Waits for the network: the job is nothing but a call to a service.
    Required,
    /// Never waits: it reads what is already saved on this Mac.
    Local,
}

/// The network class of a job, by its `cron_jobs.action_type`.
///
/// `daily_briefing` is local: the briefing is shown from saved data at its
/// time. Only its AI summary needs the network, and that step waits on its own
/// (see `scheduled_briefing`). `proactive_check` covers task alerts, mail
/// follow-up reminders and the meeting notices, all read from the local
/// database.
pub fn network_class(action_type: &str) -> NetworkClass {
    match action_type {
        "email_sync" | "calendar_sync" | "mail_workspace_sync" | "notion_sync" | "github_digest" | "auto_archive_emails" => {
            NetworkClass::Required
        }
        _ => NetworkClass::Local,
    }
}

/// How a run ended. Stored in `cron_runs.status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    /// A real error. Final: the slot is handled and the failure alert applies.
    Failed,
    /// The run was cut short: JARVIS exited, or the Mac slept under it. Not
    /// final: the slot is run once more.
    Interrupted,
    /// The Mac had no usable network. Not final: the slot runs once the
    /// network is back, unless a newer slot supersedes it first.
    SkippedOffline,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Interrupted => "interrupted",
            RunStatus::SkippedOffline => "skipped_offline",
        }
    }

    /// A final state is what makes a slot handled.
    pub fn is_final(self) -> bool {
        matches!(self, RunStatus::Completed | RunStatus::Failed)
    }
}

/// What a finished run is recorded as.
///
/// An error counts as offline ONLY when macOS itself reports no usable network
/// at the moment of the error. A timeout, a DNS failure, a TLS error or a 5xx
/// while the network is reachable is a real failure and raises the usual
/// alert; the text of the error is deliberately not consulted. A run the Mac
/// slept under did not fail either: its connection died with the sleep.
pub fn classify_outcome(succeeded: bool, slept_during_run: bool, class: NetworkClass, online_at_error: bool) -> RunStatus {
    if succeeded {
        RunStatus::Completed
    } else if slept_during_run {
        RunStatus::Interrupted
    } else if class == NetworkClass::Required && !online_at_error {
        RunStatus::SkippedOffline
    } else {
        RunStatus::Failed
    }
}

/// How long a network-required slot waits for the network before the wait is
/// written down as `skipped_offline`. The slot is still owed after that.
pub const OFFLINE_WAIT_SECS: u64 = 300;

/// After the network comes back it has to stay up this long before a waiting
/// job runs. Reachability turns positive when Wi-Fi associates, a moment
/// before DNS answers; running in that moment is how a wake used to produce a
/// burst of failed runs.
pub const NETWORK_SETTLE_SECS: u64 = 5;

/// The pause before a slot is tried again after a run that ended offline:
/// 5 s, 10 s, 20 s and so on, never more than five minutes. `attempts` is the
/// number of runs that have already ended that way.
pub fn offline_retry_delay_secs(attempts: u32) -> u64 {
    let doubled = NETWORK_SETTLE_SECS.saturating_mul(1u64 << attempts.min(16));
    doubled.min(OFFLINE_WAIT_SECS)
}

/// Whether the Mac has a usable network right now.
pub trait Reachability: Send + Sync {
    fn online(&self) -> bool;
}

const REACHABLE: u32 = 1 << 1;
const CONNECTION_REQUIRED: u32 = 1 << 2;
const CONNECTION_ON_TRAFFIC: u32 = 1 << 3;
const INTERVENTION_REQUIRED: u32 = 1 << 4;
const CONNECTION_ON_DEMAND: u32 = 1 << 5;

/// Reads SCNetworkReachability flags the way Apple's Reachability sample
/// does. Reachable is not enough on its own: a link that still has to be
/// brought up counts only if the system will bring it up without asking the
/// user. `None` means the flags could not be read, which is treated as no
/// network.
pub fn usable_network(flags: Option<u32>) -> bool {
    let Some(flags) = flags else { return false };
    if flags & REACHABLE == 0 {
        return false;
    }
    if flags & CONNECTION_REQUIRED == 0 {
        return true;
    }
    let automatic = flags & (CONNECTION_ON_DEMAND | CONNECTION_ON_TRAFFIC) != 0;
    automatic && flags & INTERVENTION_REQUIRED == 0
}

/// macOS's own view of the network, from SCNetworkReachability. The check is
/// against the zero address, so it asks "is there a route out" and sends
/// nothing: it does not prove any one service answers.
pub struct SystemReachability;

impl Reachability for SystemReachability {
    fn online(&self) -> bool {
        usable_network(macos::reachability_flags())
    }
}

mod macos {
    use std::ffi::c_void;

    #[link(name = "SystemConfiguration", kind = "framework")]
    extern "C" {
        fn SCNetworkReachabilityCreateWithAddress(allocator: *const c_void, address: *const libc::sockaddr) -> *const c_void;
        fn SCNetworkReachabilityGetFlags(target: *const c_void, flags: *mut u32) -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: *const c_void);
    }

    pub fn reachability_flags() -> Option<u32> {
        // SAFETY: `address` is a zeroed, correctly sized sockaddr_in that
        // outlives the call. The target is checked for null, used once and
        // released on every path.
        unsafe {
            let mut address: libc::sockaddr_in = std::mem::zeroed();
            address.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
            address.sin_family = libc::AF_INET as libc::sa_family_t;
            let target = SCNetworkReachabilityCreateWithAddress(std::ptr::null(), &address as *const libc::sockaddr_in as *const libc::sockaddr);
            if target.is_null() {
                return None;
            }
            let mut flags = 0u32;
            let read = SCNetworkReachabilityGetFlags(target, &mut flags) != 0;
            CFRelease(target);
            read.then_some(flags)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_wait_for_the_network_and_local_work_does_not() {
        for job in ["email_sync", "calendar_sync", "mail_workspace_sync", "notion_sync", "github_digest", "auto_archive_emails"] {
            assert_eq!(network_class(job), NetworkClass::Required, "{job}");
        }
        for job in ["deadline_monitor", "proactive_check", "daily_briefing"] {
            assert_eq!(network_class(job), NetworkClass::Local, "{job}");
        }
    }

    /// The error is the same string in both halves: only what macOS says about
    /// the network decides.
    #[test]
    fn an_error_is_offline_only_when_macos_reports_no_network() {
        let required = NetworkClass::Required;
        // Unreachable network at the time of the error.
        assert_eq!(classify_outcome(false, false, required, false), RunStatus::SkippedOffline);
        // A timeout, DNS failure, TLS error or 5xx with the network up.
        assert_eq!(classify_outcome(false, false, required, true), RunStatus::Failed);
        assert!(!RunStatus::SkippedOffline.is_final(), "the slot is still owed");
        assert!(RunStatus::Failed.is_final(), "a real failure is final");
    }

    #[test]
    fn a_local_job_that_fails_offline_has_really_failed() {
        assert_eq!(classify_outcome(false, false, NetworkClass::Local, false), RunStatus::Failed);
    }

    #[test]
    fn a_run_the_mac_slept_under_is_interrupted_not_failed() {
        assert_eq!(classify_outcome(false, true, NetworkClass::Required, true), RunStatus::Interrupted);
        assert_eq!(classify_outcome(false, true, NetworkClass::Required, false), RunStatus::Interrupted);
        assert!(!RunStatus::Interrupted.is_final());
        // A run that finished its work across a sleep simply completed.
        assert_eq!(classify_outcome(true, true, NetworkClass::Required, false), RunStatus::Completed);
    }

    #[test]
    fn reachability_flags_are_read_like_apples_sample() {
        assert!(!usable_network(None), "unreadable flags are no network");
        assert!(!usable_network(Some(0)), "nothing reachable");
        assert!(usable_network(Some(REACHABLE)));
        assert!(!usable_network(Some(REACHABLE | CONNECTION_REQUIRED)), "a link nobody will bring up");
        assert!(usable_network(Some(REACHABLE | CONNECTION_REQUIRED | CONNECTION_ON_TRAFFIC)));
        assert!(usable_network(Some(REACHABLE | CONNECTION_REQUIRED | CONNECTION_ON_DEMAND)));
        assert!(
            !usable_network(Some(REACHABLE | CONNECTION_REQUIRED | CONNECTION_ON_TRAFFIC | INTERVENTION_REQUIRED)),
            "a link that needs the user (a password, a captive sign-in) is not usable"
        );
        assert!(!usable_network(Some(CONNECTION_REQUIRED | CONNECTION_ON_TRAFFIC)), "not reachable at all");
    }

    #[test]
    fn retries_after_an_offline_run_back_off_to_five_minutes() {
        let delays: Vec<u64> = (0..8).map(offline_retry_delay_secs).collect();
        assert_eq!(delays, vec![5, 10, 20, 40, 80, 160, 300, 300]);
        assert_eq!(offline_retry_delay_secs(u32::MAX), 300);
    }

    /// Reads the real flags once. It asserts nothing about the answer, which
    /// depends on the machine; it proves the call links and returns.
    #[test]
    fn the_system_reachability_call_returns() {
        let _ = SystemReachability.online();
    }
}
