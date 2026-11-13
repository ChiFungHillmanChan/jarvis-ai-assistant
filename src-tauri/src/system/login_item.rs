//! Open at login.
//!
//! Scheduled work stops at a reboot unless JARVIS is opened again, so opening
//! at login is on by default. The default is applied once and never again:
//! after that the user's own choice in Settings is the only thing that changes
//! it.

use crate::db::Database;
use rusqlite::OptionalExtension;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Whether "open at login" should be turned on by default now.
///
/// Only once onboarding has finished, and only if the default has never been
/// applied on this Mac. A user who then turns it off is never switched back
/// on: by then `default_applied` is true for good. Someone who finished
/// onboarding in an earlier version gets it on the first launch after
/// updating, the same way.
///
/// And only for an app that is running from where it is installed (see
/// `installed_in_applications`). Until it is, the default stays unapplied, so
/// the first launch from the Applications folder applies it.
pub fn should_apply_default(onboarding_complete: bool, default_applied: bool, installed: bool) -> bool {
    onboarding_complete && !default_applied && installed
}

/// Whether the running executable is inside `/Applications` or the user's own
/// `~/Applications`.
///
/// A login item points at the executable that registered it. JARVIS opened
/// straight from its disk image runs from `/Volumes/...`, and a quarantined
/// copy opened from Downloads runs from a random, read-only path that macOS
/// makes up for that one launch (app translocation). A login item made then
/// points at something that is gone by the next login.
pub fn installed_in_applications(executable: &Path, home: Option<&Path>) -> bool {
    executable.starts_with("/Applications") || home.is_some_and(|home| executable.starts_with(home.join("Applications")))
}

/// The Mac's login item for this app.
pub trait LoginItem: Send + Sync {
    fn is_enabled(&self) -> Result<bool, String>;
    fn set_enabled(&self, enabled: bool) -> Result<(), String>;
    /// The preference that records that the default was applied to this
    /// login item.
    fn default_applied_key(&self) -> &'static str {
        "open_at_login_default_applied"
    }
    /// Whether the app is running from where it is installed, so that a login
    /// item made now still points at it at the next login. Only the default
    /// asks; the switch in Settings is the user's own choice wherever the app
    /// is running from.
    fn installed(&self) -> bool {
        true
    }
}

/// The real login item: a LaunchAgent written by tauri-plugin-autostart.
/// `is_enabled` reads the Mac, not a saved preference, so the Settings toggle
/// shows what will actually happen at the next login.
#[cfg(not(any(debug_assertions, feature = "native-qa")))]
struct SystemLoginItem(tauri::AppHandle);

#[cfg(not(any(debug_assertions, feature = "native-qa")))]
impl LoginItem for SystemLoginItem {
    fn is_enabled(&self) -> Result<bool, String> {
        use tauri_plugin_autostart::ManagerExt;
        self.0.autolaunch().is_enabled().map_err(|e| e.to_string())
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        let manager = self.0.autolaunch();
        let changed = if enabled { manager.enable() } else { manager.disable() };
        changed.map_err(|e| e.to_string())
    }

    fn installed(&self) -> bool {
        std::env::current_exe().is_ok_and(|executable| installed_in_applications(&executable, dirs::home_dir().as_deref()))
    }
}

/// A stand-in that keeps the setting in memory and writes nothing to the Mac.
///
/// Development and QA builds use it. The real login item points at the
/// running executable, so a `tauri dev` or QA binary that registered itself
/// would be started at every login, against whatever database it finds.
#[derive(Default)]
pub struct MemoryLoginItem(AtomicBool);

impl LoginItem for MemoryLoginItem {
    fn is_enabled(&self) -> Result<bool, String> {
        Ok(self.0.load(Ordering::SeqCst))
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        self.0.store(enabled, Ordering::SeqCst);
        Ok(())
    }

    /// Its own record, so a development build pointed at the same database as
    /// the released app cannot mark the real default as applied when all it
    /// changed was this stand-in.
    fn default_applied_key(&self) -> &'static str {
        "open_at_login_default_applied_dev"
    }
}

/// Tauri state: the login item this build uses.
pub struct LoginItemState(pub Arc<dyn LoginItem>);

impl LoginItemState {
    /// The real login item in a release build; the in-memory stand-in in a
    /// development or QA build.
    pub fn for_this_build(app: &tauri::AppHandle) -> Self {
        #[cfg(not(any(debug_assertions, feature = "native-qa")))]
        {
            LoginItemState(Arc::new(SystemLoginItem(app.clone())))
        }
        #[cfg(any(debug_assertions, feature = "native-qa"))]
        {
            let _ = app;
            log::info!("Open at login: this is a development or QA build, so the setting is kept in memory only");
            LoginItemState(Arc::new(MemoryLoginItem::default()))
        }
    }
}

fn preference_is_true(conn: &rusqlite::Connection, key: &str) -> Result<bool, String> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM user_preferences WHERE key = ?1", [key], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(value.as_deref() == Some("true"))
}

/// Turn "open at login" on if the default is due, and remember that it was
/// applied. Called at launch and when onboarding finishes. Returns whether it
/// was applied by this call.
///
/// The flag is stored only after the login item was really enabled, so a Mac
/// that refused it once is asked again at the next launch rather than left
/// off without the user ever having chosen that.
pub fn apply_default(db: &Database, login_item: &dyn LoginItem) -> Result<bool, String> {
    let (onboarding_complete, default_applied) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        (preference_is_true(&conn, "onboarding_complete")?, preference_is_true(&conn, login_item.default_applied_key())?)
    };
    if !should_apply_default(onboarding_complete, default_applied, login_item.installed()) {
        return Ok(false);
    }
    login_item.set_enabled(true)?;
    mark_default_applied(db, login_item)?;
    Ok(true)
}

fn mark_default_applied(db: &Database, login_item: &dyn LoginItem) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, 'true', datetime('now')) \
         ON CONFLICT(key) DO UPDATE SET value = 'true', updated_at = datetime('now')",
        [login_item.default_applied_key()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// The user's own choice in Settings. It also settles the default: whatever
/// they chose here is never overridden by it later. Returns the state the Mac
/// reports afterwards.
pub fn set_by_user(db: &Database, login_item: &dyn LoginItem, enabled: bool) -> Result<bool, String> {
    login_item.set_enabled(enabled)?;
    mark_default_applied(db, login_item)?;
    login_item.is_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish_onboarding(db: &Database) {
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO user_preferences (key, value) VALUES ('onboarding_complete', 'true') \
                 ON CONFLICT(key) DO UPDATE SET value = 'true'",
                [],
            )
            .unwrap();
    }

    #[test]
    fn the_default_is_due_once_after_onboarding() {
        assert!(!should_apply_default(false, false, true), "not before onboarding finishes");
        assert!(should_apply_default(true, false, true), "once it has");
        assert!(!should_apply_default(true, true, true), "never a second time");
        assert!(!should_apply_default(false, true, true));
        assert!(!should_apply_default(true, false, false), "not while the app is running from somewhere it will not stay");
    }

    #[test]
    fn only_an_app_in_an_applications_folder_counts_as_installed() {
        let home = Path::new("/Users/someone");
        let installed = |executable: &str| installed_in_applications(Path::new(executable), Some(home));
        assert!(installed("/Applications/JARVIS.app/Contents/MacOS/jarvis"));
        assert!(installed("/Users/someone/Applications/JARVIS.app/Contents/MacOS/jarvis"));
        assert!(installed("/Applications/Tools/JARVIS.app/Contents/MacOS/jarvis"), "a folder inside Applications");

        assert!(!installed("/Volumes/JARVIS/JARVIS.app/Contents/MacOS/jarvis"), "opened from the disk image");
        assert!(
            !installed("/private/var/folders/zz/abc123/T/AppTranslocation/6B1C0C6E-0000-4000-8000-1234567890AB/d/JARVIS.app/Contents/MacOS/jarvis"),
            "translocated by macOS"
        );
        assert!(!installed("/Users/someone/Downloads/JARVIS.app/Contents/MacOS/jarvis"));
        assert!(!installed("/Users/someone/Desktop/Applications/JARVIS.app/Contents/MacOS/jarvis"), "not just any folder with that name");
        assert!(!installed("/ApplicationsOld/JARVIS.app/Contents/MacOS/jarvis"), "whole path components, not a prefix of the text");
        assert!(!installed("/Users/someone-else/Applications/JARVIS.app/Contents/MacOS/jarvis"), "another user's folder");
        assert!(
            !installed_in_applications(Path::new("/Users/someone/Applications/JARVIS.app/Contents/MacOS/jarvis"), None),
            "no home folder known"
        );
    }

    /// A login item that says where the app is running from.
    struct LoginItemAt {
        item: MemoryLoginItem,
        installed: AtomicBool,
    }

    impl LoginItem for LoginItemAt {
        fn is_enabled(&self) -> Result<bool, String> {
            self.item.is_enabled()
        }
        fn set_enabled(&self, enabled: bool) -> Result<(), String> {
            self.item.set_enabled(enabled)
        }
        fn installed(&self) -> bool {
            self.installed.load(Ordering::SeqCst)
        }
    }

    /// First opened from the disk image, later from the Applications folder:
    /// the default waits for the second, and the switch in Settings works in
    /// both.
    #[test]
    fn the_default_waits_until_the_app_is_installed() {
        let db = crate::db::test_database();
        finish_onboarding(&db);
        let item = LoginItemAt { item: MemoryLoginItem::default(), installed: AtomicBool::new(false) };

        for _ in 0..2 {
            assert!(!apply_default(&db, &item).unwrap(), "running from the disk image");
        }
        assert!(!item.is_enabled().unwrap(), "no login item pointing at the disk image");

        item.installed.store(true, Ordering::SeqCst);
        assert!(apply_default(&db, &item).unwrap(), "the first launch from Applications");
        assert!(item.is_enabled().unwrap());
        assert!(!apply_default(&db, &item).unwrap(), "once only");

        // The user's own choice is theirs wherever the app runs from, and it
        // settles the default for good.
        let db = crate::db::test_database();
        finish_onboarding(&db);
        let item = LoginItemAt { item: MemoryLoginItem::default(), installed: AtomicBool::new(false) };
        assert!(set_by_user(&db, &item, true).unwrap(), "turned on in Settings while running from the disk image");
        assert!(!set_by_user(&db, &item, false).unwrap());
        item.installed.store(true, Ordering::SeqCst);
        assert!(!apply_default(&db, &item).unwrap(), "the default does not turn it back on");
    }

    #[test]
    fn open_at_login_turns_on_when_onboarding_finishes() {
        let db = crate::db::test_database();
        let item = MemoryLoginItem::default();

        assert!(!apply_default(&db, &item).unwrap(), "a first launch, still in onboarding");
        assert!(!item.is_enabled().unwrap());

        finish_onboarding(&db);
        assert!(apply_default(&db, &item).unwrap());
        assert!(item.is_enabled().unwrap());
    }

    #[test]
    fn it_is_never_turned_back_on_after_the_user_turns_it_off() {
        let db = crate::db::test_database();
        let item = MemoryLoginItem::default();
        finish_onboarding(&db);
        assert!(apply_default(&db, &item).unwrap());

        // The user switches it off in Settings.
        item.set_enabled(false).unwrap();
        // Every later launch, and a second pass through onboarding.
        for _ in 0..3 {
            assert!(!apply_default(&db, &item).unwrap());
            finish_onboarding(&db);
        }
        assert!(!item.is_enabled().unwrap(), "the user's choice stands");
    }

    /// Someone who finished onboarding in an earlier version: the default is
    /// applied on the first launch after updating, then never again.
    #[test]
    fn an_existing_user_gets_the_default_once_after_updating() {
        let db = crate::db::test_database();
        finish_onboarding(&db);
        let item = MemoryLoginItem::default();
        assert!(apply_default(&db, &item).unwrap(), "first launch after the update");
        assert!(item.is_enabled().unwrap());
        assert!(!apply_default(&db, &item).unwrap(), "second launch");

        // They turn it off in Settings; no later launch turns it back on.
        assert!(!set_by_user(&db, &item, false).unwrap());
        for _ in 0..3 {
            assert!(!apply_default(&db, &item).unwrap());
        }
        assert!(!item.is_enabled().unwrap(), "the user's choice stands");
    }

    /// A choice made in Settings before the default was ever applied (the
    /// Settings page can be opened during onboarding) is a choice all the
    /// same.
    #[test]
    fn a_choice_made_in_settings_is_not_overridden_by_the_default() {
        let db = crate::db::test_database();
        let item = MemoryLoginItem::default();
        assert!(!set_by_user(&db, &item, false).unwrap());
        finish_onboarding(&db);
        assert!(!apply_default(&db, &item).unwrap());
        assert!(!item.is_enabled().unwrap());

        assert!(set_by_user(&db, &item, true).unwrap(), "and it reports what the Mac now says");
    }

    /// A development build can be pointed at the database the released app
    /// uses. What it does to its stand-in must not use up the real default.
    #[test]
    fn a_development_build_does_not_use_up_the_real_default() {
        struct RealLoginItem(MemoryLoginItem);
        impl LoginItem for RealLoginItem {
            fn is_enabled(&self) -> Result<bool, String> {
                self.0.is_enabled()
            }
            fn set_enabled(&self, enabled: bool) -> Result<(), String> {
                self.0.set_enabled(enabled)
            }
        }

        let db = crate::db::test_database();
        finish_onboarding(&db);
        assert!(apply_default(&db, &MemoryLoginItem::default()).unwrap(), "the development build");
        let real = RealLoginItem(MemoryLoginItem::default());
        assert!(apply_default(&db, &real).unwrap(), "the released app, same database");
        assert!(real.is_enabled().unwrap());
    }

    struct RefusingLoginItem;

    impl LoginItem for RefusingLoginItem {
        fn is_enabled(&self) -> Result<bool, String> {
            Ok(false)
        }

        fn set_enabled(&self, _enabled: bool) -> Result<(), String> {
            Err("Operation not permitted".to_string())
        }
    }

    #[test]
    fn a_refused_default_is_tried_again_at_the_next_launch() {
        let db = crate::db::test_database();
        finish_onboarding(&db);
        assert!(apply_default(&db, &RefusingLoginItem).is_err());

        let item = MemoryLoginItem::default();
        assert!(apply_default(&db, &item).unwrap(), "not recorded as applied when it was not");
        assert!(item.is_enabled().unwrap());
    }
}
