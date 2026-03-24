use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

pub fn create_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show JARVIS", true, None::<&str>)?;
    let wallpaper = MenuItem::with_id(app, "wallpaper", "Toggle Wallpaper", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &wallpaper, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if crate::wallpaper::is_active() {
                    let _ = crate::wallpaper::raise_for_interaction(app);
                } else {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
            "wallpaper" => {
                match crate::wallpaper::toggle_wallpaper(app.clone()) {
                    Ok(_active) => {}
                    Err(e) => {
                        log::error!("Failed to toggle wallpaper: {}", e);
                    }
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { .. } = event {
                let app = tray.app_handle();
                if crate::wallpaper::is_active() {
                    let _ = crate::wallpaper::raise_for_interaction(app);
                } else {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}
