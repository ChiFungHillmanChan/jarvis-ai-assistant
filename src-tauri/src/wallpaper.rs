use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;

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
    let _ = main_win.set_size(tauri::Size::Logical(tauri::LogicalSize {
        width: 1200.0,
        height: 800.0,
    }));

    WALLPAPER_ACTIVE.store(false, Ordering::SeqCst);
    WALLPAPER_RAISED.store(false, Ordering::SeqCst);
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
    log::info!("Wallpaper lowered to background");
    Ok(())
}

// -- Tauri commands --

#[tauri::command]
pub fn enable_wallpaper(app: tauri::AppHandle) -> Result<(), String> {
    enable(&app)
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
