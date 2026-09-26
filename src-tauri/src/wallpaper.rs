use crate::db::Database;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};

static WALLPAPER_ACTIVE: AtomicBool = AtomicBool::new(false);
static WALLPAPER_RAISED: AtomicBool = AtomicBool::new(false);

const DESKTOP_WINDOW_LEVEL: i64 = -2147483623 + 1;
#[cfg(target_os = "macos")]
const NATIVE_WINDOW_NOT_READY: &str = "Native window not ready";

pub fn is_active() -> bool {
    WALLPAPER_ACTIVE.load(Ordering::SeqCst)
}

pub fn is_raised() -> bool {
    WALLPAPER_RAISED.load(Ordering::SeqCst)
}

/// Preference holding the normal window frame (x, y, width, height in
/// physical pixels) while wallpaper mode is on. The window-state plugin keeps
/// recording the window during wallpaper mode, so it only knows the fullscreen
/// frame; leaving wallpaper mode restores from here instead.
const NORMAL_FRAME_KEY: &str = "window_frame_before_wallpaper";

/// Keeps the first frame: after a restart in wallpaper mode the window comes
/// back at screen size, which is not the frame to return to.
fn remember_frame(conn: &rusqlite::Connection, frame: (i32, i32, u32, u32)) -> rusqlite::Result<()> {
    let (x, y, width, height) = frame;
    conn.execute(
        "INSERT OR IGNORE INTO user_preferences (key, value) VALUES (?1, ?2)",
        rusqlite::params![NORMAL_FRAME_KEY, format!("{},{},{},{}", x, y, width, height)],
    )?;
    Ok(())
}

fn take_frame(conn: &rusqlite::Connection) -> Option<(i32, i32, u32, u32)> {
    let value: String = conn
        .query_row("SELECT value FROM user_preferences WHERE key = ?1", [NORMAL_FRAME_KEY], |row| row.get(0))
        .ok()?;
    let _ = conn.execute("DELETE FROM user_preferences WHERE key = ?1", [NORMAL_FRAME_KEY]);
    let mut parts = value.split(',');
    let frame = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    (frame.2 > 0 && frame.3 > 0).then_some(frame)
}

fn emit_wallpaper_state(app_handle: &tauri::AppHandle) {
    let _ = app_handle.emit("wallpaper-status", is_active());
    let _ = app_handle.emit("wallpaper-raised", is_raised());
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn with_ns_window(
    main_win: &tauri::WebviewWindow,
    action: impl FnOnce(cocoa::base::id) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    use cocoa::base::id;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::mpsc::sync_channel;

    let main_win = main_win.clone();
    let window_for_closure = main_win.clone();
    let (tx, rx) = sync_channel(1);

    main_win
        .run_on_main_thread(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                let ns_window: id = window_for_closure
                    .ns_window()
                    .map_err(|e| format!("{}", e))? as id;
                action(ns_window)
            }))
            .unwrap_or_else(|_| Err(NATIVE_WINDOW_NOT_READY.to_string()));

            let _ = tx.send(result);
        })
        .map_err(|e| format!("{}", e))?;

    rx.recv()
        .map_err(|_| "Failed to receive native window state".to_string())?
}

/// Turn the main JARVIS window into a fullscreen desktop wallpaper.
/// The full app UI is visible behind all other windows and desktop icons.
/// Click-through is enabled so the user can interact with the desktop normally.
#[allow(deprecated)]
pub fn enable(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let main_win = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    if !main_win.is_fullscreen().unwrap_or(false) {
        if let (Ok(position), Ok(size)) = (main_win.outer_position(), main_win.inner_size()) {
            let db = app_handle.state::<Arc<Database>>();
            let remembered = db.conn.lock().map(|conn| remember_frame(&conn, (position.x, position.y, size.width, size.height)));
            if let Ok(Err(e)) = remembered {
                log::warn!("Could not remember the window frame before wallpaper mode: {}", e);
            }
        }
    }
    let _ = main_win.set_fullscreen(true);

    #[cfg(target_os = "macos")]
    {
        with_ns_window(&main_win, |ns_window| {
            use cocoa::appkit::{NSWindow, NSWindowCollectionBehavior};
            use cocoa::base::{NO, YES};

            unsafe {
                ns_window.setLevel_(DESKTOP_WINDOW_LEVEL);
                ns_window.setCollectionBehavior_(
                    NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                        | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
                        | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle,
                );
                ns_window.setIgnoresMouseEvents_(YES);
                ns_window.setHasShadow_(NO);
            }

            Ok(())
        })?;
    }

    WALLPAPER_ACTIVE.store(true, Ordering::SeqCst);
    WALLPAPER_RAISED.store(false, Ordering::SeqCst);
    emit_wallpaper_state(app_handle);
    log::info!("Wallpaper mode enabled -- main window is now desktop wallpaper");
    Ok(())
}

/// Disable wallpaper mode and restore the main window to normal.
#[allow(deprecated)]
pub fn disable(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let main_win = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    #[cfg(target_os = "macos")]
    {
        with_ns_window(&main_win, |ns_window| {
            use cocoa::appkit::{NSWindow, NSWindowCollectionBehavior};
            use cocoa::base::{NO, YES};

            unsafe {
                ns_window.setLevel_(0); // NSNormalWindowLevel
                ns_window.setCollectionBehavior_(
                    NSWindowCollectionBehavior::NSWindowCollectionBehaviorDefault,
                );
                ns_window.setIgnoresMouseEvents_(NO);
                ns_window.setHasShadow_(YES);
            }

            Ok(())
        })?;
    }

    let _ = main_win.set_fullscreen(false);
    let frame = app_handle.state::<Arc<Database>>().conn.lock().ok().and_then(|conn| take_frame(&conn));
    match frame {
        Some((x, y, width, height)) => {
            let _ = main_win.set_size(tauri::Size::Physical(tauri::PhysicalSize { width, height }));
            let _ = main_win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
        }
        None => {
            let _ = main_win.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: 1200.0,
                height: 800.0,
            }));
        }
    }

    WALLPAPER_ACTIVE.store(false, Ordering::SeqCst);
    WALLPAPER_RAISED.store(false, Ordering::SeqCst);
    emit_wallpaper_state(app_handle);
    log::info!("Wallpaper mode disabled -- main window restored");
    Ok(())
}

/// Temporarily bring the wallpaper to the foreground for interaction.
/// Disables click-through and raises window level so the user can
/// click buttons, scroll, chat, etc.
#[allow(deprecated)]
pub fn raise_for_interaction(app_handle: &tauri::AppHandle) -> Result<(), String> {
    if !is_active() {
        return Ok(());
    }

    let main_win = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    #[cfg(target_os = "macos")]
    {
        with_ns_window(&main_win, |ns_window| {
            use cocoa::appkit::NSWindow;
            use cocoa::base::NO;

            unsafe {
                ns_window.setLevel_(0); // NSNormalWindowLevel
                ns_window.setIgnoresMouseEvents_(NO);
            }

            Ok(())
        })?;
    }

    let _ = main_win.set_focus();
    WALLPAPER_RAISED.store(true, Ordering::SeqCst);
    emit_wallpaper_state(app_handle);
    log::info!("Wallpaper raised for interaction");
    Ok(())
}

/// Send the wallpaper back to the desktop level after interaction.
#[allow(deprecated)]
pub fn lower_to_background(app_handle: &tauri::AppHandle) -> Result<(), String> {
    if !is_active() {
        return Ok(());
    }

    let main_win = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    #[cfg(target_os = "macos")]
    {
        with_ns_window(&main_win, |ns_window| {
            use cocoa::appkit::NSWindow;
            use cocoa::base::YES;

            unsafe {
                ns_window.setLevel_(DESKTOP_WINDOW_LEVEL);
                ns_window.setIgnoresMouseEvents_(YES);
            }

            Ok(())
        })?;
    }

    WALLPAPER_RAISED.store(false, Ordering::SeqCst);
    emit_wallpaper_state(app_handle);
    log::info!("Wallpaper lowered to background");
    Ok(())
}

// -- Tauri commands --

#[tauri::command]
pub fn enable_wallpaper(app: tauri::AppHandle) -> Result<(), String> {
    enable(&app)?;
    raise_for_interaction(&app)
}

#[tauri::command]
pub fn disable_wallpaper(app: tauri::AppHandle) -> Result<(), String> {
    disable(&app)
}

#[tauri::command]
pub fn toggle_wallpaper(app: tauri::AppHandle) -> Result<bool, String> {
    if is_active() {
        disable(&app)?;
        Ok(false)
    } else {
        enable(&app)?;
        Ok(true)
    }
}

#[tauri::command]
pub fn get_wallpaper_status() -> bool {
    is_active()
}

#[tauri::command]
pub fn raise_wallpaper(app: tauri::AppHandle) -> Result<(), String> {
    raise_for_interaction(&app)
}

#[tauri::command]
pub fn lower_wallpaper(app: tauri::AppHandle) -> Result<(), String> {
    lower_to_background(&app)
}

#[tauri::command]
pub fn is_wallpaper_raised() -> bool {
    is_raised()
}

pub async fn enable_on_startup(app_handle: tauri::AppHandle) -> Result<(), String> {
    const MAX_ATTEMPTS: usize = 20;
    let retry_delay = std::time::Duration::from_millis(250);

    for attempt in 1..=MAX_ATTEMPTS {
        match enable(&app_handle) {
            Ok(()) => return Ok(()),
            #[cfg(target_os = "macos")]
            Err(error) if error == NATIVE_WINDOW_NOT_READY => {
                log::warn!(
                    "Wallpaper startup deferred: native window not ready (attempt {}/{})",
                    attempt,
                    MAX_ATTEMPTS
                );
                tokio::time::sleep(retry_delay).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err("Wallpaper startup failed: native window never became ready".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn prefs() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE user_preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT (datetime('now')));").unwrap();
        conn
    }

    #[test]
    fn leaving_wallpaper_returns_the_frame_from_before_it() {
        let conn = prefs();
        remember_frame(&conn, (120, 80, 2400, 1600)).unwrap();
        assert_eq!(take_frame(&conn), Some((120, 80, 2400, 1600)));
        assert_eq!(take_frame(&conn), None, "the frame is used once");
    }

    #[test]
    fn a_restart_in_wallpaper_mode_keeps_the_original_frame() {
        // After a restart the window comes back at screen size; that must not
        // replace the frame the user had before turning wallpaper mode on.
        let conn = prefs();
        remember_frame(&conn, (120, 80, 2400, 1600)).unwrap();
        remember_frame(&conn, (0, 0, 3024, 1964)).unwrap();
        assert_eq!(take_frame(&conn), Some((120, 80, 2400, 1600)));
    }

    #[test]
    fn a_damaged_or_empty_frame_is_ignored() {
        let conn = prefs();
        conn.execute("INSERT INTO user_preferences (key, value) VALUES (?1, 'not,a,frame')", [NORMAL_FRAME_KEY]).unwrap();
        assert_eq!(take_frame(&conn), None);
        remember_frame(&conn, (0, 0, 0, 800)).unwrap();
        assert_eq!(take_frame(&conn), None);
    }
}
