//! Keep the Mac awake while it is plugged in (optional, off by default).
//!
//! While the setting is on and the Mac is on mains power, JARVIS holds an
//! IOKit `PreventUserIdleSystemSleep` assertion: the Mac does not go to sleep
//! by itself for being idle. It is released on battery and when the setting
//! goes off. It does not help with the lid closed, and it does not stop the
//! display sleeping or a sleep the user asks for.

use std::sync::Mutex;

/// Whether the assertion should be held: only with the setting on, and only
/// on mains power.
pub fn should_hold(setting_on: bool, on_ac_power: bool) -> bool {
    setting_on && on_ac_power
}

/// Something that can stop the Mac idling to sleep.
pub trait SleepAssertion: Send {
    fn take(&mut self) -> Result<(), String>;
    fn release(&mut self);
    fn held(&self) -> bool;
}

/// Holds the assertion exactly when [`should_hold`] says so.
pub struct KeepAwake<A: SleepAssertion> {
    assertion: A,
    setting_on: bool,
}

impl<A: SleepAssertion> KeepAwake<A> {
    pub fn new(assertion: A, setting_on: bool) -> Self {
        KeepAwake { assertion, setting_on }
    }

    pub fn setting_on(&self) -> bool {
        self.setting_on
    }

    pub fn set_setting(&mut self, on: bool) {
        self.setting_on = on;
    }

    pub fn held(&self) -> bool {
        self.assertion.held()
    }

    /// Bring the assertion in line with the setting and the power source.
    /// Called when the setting changes and whenever the power source is read.
    /// Returns whether the assertion is held afterwards.
    pub fn update(&mut self, on_ac_power: bool) -> bool {
        let wanted = should_hold(self.setting_on, on_ac_power);
        if wanted && !self.assertion.held() {
            if let Err(e) = self.assertion.take() {
                log::warn!("Keep awake: could not prevent idle sleep: {}", e);
            }
        } else if !wanted && self.assertion.held() {
            self.assertion.release();
        }
        self.assertion.held()
    }
}

/// How often the power source is read while the setting is on. Idle sleep is
/// minutes away at the soonest, so a few seconds' lag after the cable is
/// pulled changes nothing.
const POWER_POLL: std::time::Duration = std::time::Duration::from_secs(5);

const PREFERENCE_KEY: &str = "keep_awake_on_ac";

/// Tauri state: the one assertion this app may hold.
pub struct KeepAwakeState(Mutex<KeepAwake<macos::IdleSleepAssertion>>);

impl KeepAwakeState {
    pub fn new(setting_on: bool) -> Self {
        KeepAwakeState(Mutex::new(KeepAwake::new(macos::IdleSleepAssertion::default(), setting_on)))
    }

    /// Start from the saved setting. Off unless the user turned it on.
    pub fn from_db(db: &crate::db::Database) -> Self {
        let setting_on = db
            .conn
            .lock()
            .ok()
            .and_then(|conn| {
                conn.query_row("SELECT value FROM user_preferences WHERE key = ?1", [PREFERENCE_KEY], |row| {
                    row.get::<_, String>(0)
                })
                .ok()
            })
            .is_some_and(|value| value == "true");
        KeepAwakeState::new(setting_on)
    }

    pub fn setting_on(&self) -> bool {
        self.0.lock().is_ok_and(|keep_awake| keep_awake.setting_on())
    }

    /// Whether idle sleep is being prevented right now: the setting is on and
    /// the Mac was on mains power when last read.
    pub fn active(&self) -> bool {
        self.0.lock().is_ok_and(|keep_awake| keep_awake.held())
    }

    /// Change the setting and apply it at once.
    pub fn set(&self, db: &crate::db::Database, on: bool) -> Result<(), String> {
        {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now')) \
                 ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
                rusqlite::params![PREFERENCE_KEY, if on { "true" } else { "false" }],
            )
            .map_err(|e| e.to_string())?;
        }
        let mut keep_awake = self.0.lock().map_err(|e| e.to_string())?;
        keep_awake.set_setting(on);
        keep_awake.update(on && macos::on_ac_power());
        Ok(())
    }

    /// Read the power source and take or release the assertion to match.
    fn refresh(&self) {
        let Ok(mut keep_awake) = self.0.lock() else { return };
        // With the setting off there is nothing to hold, and no reason to ask
        // the Mac about its power source.
        let on_ac_power = keep_awake.setting_on() && macos::on_ac_power();
        keep_awake.update(on_ac_power);
    }

    /// Watch the power source for as long as the app runs.
    pub fn watch(state: std::sync::Arc<KeepAwakeState>) {
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(POWER_POLL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                state.refresh();
            }
        });
    }
}

mod macos {
    use super::SleepAssertion;
    use core_foundation::base::{CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(assertion_type: CFStringRef, level: u32, name: CFStringRef, id: *mut u32) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: CFTypeRef);
    }

    const ASSERTION_LEVEL_ON: u32 = 255;

    /// Whether the Mac is running on mains power. A desktop Mac always is.
    /// If the power source cannot be read the answer is no, so the assertion
    /// is never held on a guess.
    pub fn on_ac_power() -> bool {
        // SAFETY: the snapshot is checked for null and released after use.
        // IOPSGetProvidingPowerSourceType follows the Get rule: the string it
        // returns is not ours to release, and wrap_under_get_rule retains it
        // for as long as the wrapper lives.
        unsafe {
            let snapshot = IOPSCopyPowerSourcesInfo();
            if snapshot.is_null() {
                return false;
            }
            let source = IOPSGetProvidingPowerSourceType(snapshot);
            let on_ac = !source.is_null() && CFString::wrap_under_get_rule(source) == "AC Power";
            CFRelease(snapshot);
            on_ac
        }
    }

    /// An IOKit `PreventUserIdleSystemSleep` assertion. Released when dropped,
    /// and by macOS itself if the process ends.
    #[derive(Default)]
    pub struct IdleSleepAssertion {
        id: Option<u32>,
    }

    impl SleepAssertion for IdleSleepAssertion {
        fn take(&mut self) -> Result<(), String> {
            if self.id.is_some() {
                return Ok(());
            }
            let kind = CFString::from_static_string("PreventUserIdleSystemSleep");
            let reason = CFString::from_static_string("JARVIS keeps scheduled work running while plugged in");
            let mut id = 0u32;
            // SAFETY: both strings outlive the call and `id` is a valid
            // out-pointer.
            let status = unsafe {
                IOPMAssertionCreateWithName(kind.as_concrete_TypeRef(), ASSERTION_LEVEL_ON, reason.as_concrete_TypeRef(), &mut id)
            };
            if status != 0 {
                return Err(format!("IOPMAssertionCreateWithName returned {status:#x}"));
            }
            self.id = Some(id);
            Ok(())
        }

        fn release(&mut self) {
            if let Some(id) = self.id.take() {
                // SAFETY: `id` came from IOPMAssertionCreateWithName and is
                // released exactly once.
                let status = unsafe { IOPMAssertionRelease(id) };
                if status != 0 {
                    log::warn!("Keep awake: IOPMAssertionRelease returned {status:#x}");
                }
            }
        }

        fn held(&self) -> bool {
            self.id.is_some()
        }
    }

    impl Drop for IdleSleepAssertion {
        fn drop(&mut self) {
            self.release();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeAssertion {
        held: bool,
        taken: u32,
        released: u32,
    }

    impl SleepAssertion for FakeAssertion {
        fn take(&mut self) -> Result<(), String> {
            self.held = true;
            self.taken += 1;
            Ok(())
        }

        fn release(&mut self) {
            self.held = false;
            self.released += 1;
        }

        fn held(&self) -> bool {
            self.held
        }
    }

    #[test]
    fn the_assertion_is_wanted_only_with_the_setting_on_and_on_mains_power() {
        assert!(should_hold(true, true));
        assert!(!should_hold(true, false), "on battery");
        assert!(!should_hold(false, true), "setting off");
        assert!(!should_hold(false, false));
    }

    #[test]
    fn it_is_off_by_default_and_then_nothing_is_ever_held() {
        let mut keep_awake = KeepAwake::new(FakeAssertion::default(), false);
        assert!(!keep_awake.update(true));
        assert!(!keep_awake.update(false));
        assert_eq!(keep_awake.assertion.taken, 0);
    }

    #[test]
    fn it_follows_the_power_source_and_the_setting() {
        let mut keep_awake = KeepAwake::new(FakeAssertion::default(), true);
        assert!(keep_awake.update(true), "plugged in");
        assert!(keep_awake.update(true));
        assert_eq!(keep_awake.assertion.taken, 1, "held once, not taken again at every poll");

        assert!(!keep_awake.update(false), "the cable is pulled");
        assert_eq!(keep_awake.assertion.released, 1);
        assert!(keep_awake.update(true), "plugged back in");

        keep_awake.set_setting(false);
        assert!(!keep_awake.update(true), "the setting goes off while plugged in");
        assert_eq!((keep_awake.assertion.taken, keep_awake.assertion.released), (2, 2));
    }

    /// The real IOKit calls, once: the assertion is taken and released again
    /// inside this test, and reading the power source returns.
    #[test]
    fn the_real_assertion_can_be_taken_and_released() {
        let mut assertion = macos::IdleSleepAssertion::default();
        assert!(!assertion.held());
        assertion.take().expect("IOKit grants an idle-sleep assertion");
        assert!(assertion.held());
        assertion.take().expect("taking it twice is harmless");
        assertion.release();
        assert!(!assertion.held());
        // Nothing is asserted about the power source: it depends on the Mac.
        // Run with `--nocapture` to see what this one reports.
        println!("on_ac_power = {}", macos::on_ac_power());
    }
}
