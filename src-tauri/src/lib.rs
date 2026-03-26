pub mod ai;
pub mod assistant;
pub mod auth;
pub mod commands;
pub mod db;
pub mod integrations;
pub mod scheduler;
pub mod system;
pub mod tray;
pub mod voice;
pub mod wallpaper;

use ai::AiRouter;
use db::Database;
use tauri::Manager;

pub fn run() {
    dotenvy::dotenv().ok();
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let db = Database::new().expect("Failed to initialize database");
            let claude_key = std::env::var("ANTHROPIC_API_KEY").ok();
            let openai_key = std::env::var("OPENAI_API_KEY").ok();
            // Read AI provider preference from DB, default to claude_primary
            let ai_provider = {
                let conn = db.conn.lock().unwrap();
                conn.query_row("SELECT value FROM user_preferences WHERE key = 'ai_provider'", [], |row| row.get::<_, String>(0))
                    .unwrap_or_else(|_| "claude_primary".to_string())
            };
            log::info!("AI provider: {}", ai_provider);
            let mut router = AiRouter::new(claude_key, openai_key, &ai_provider);
            router.load_from_db(&db);
            log::info!("Provider chain: {} entries, {} local endpoints",
                router.provider_chain().len(), router.local_endpoints().len());

            let google_auth = auth::google::GoogleAuth::new()
                .unwrap_or_else(|| {
                    log::warn!("Google credentials not configured");
                    auth::google::GoogleAuth::new_empty()
                });
            google_auth.load_from_db(&db);

            let db_arc = std::sync::Arc::new(db);
            let auth_arc = std::sync::Arc::new(google_auth);

            let db_for_scheduler = std::sync::Arc::clone(&db_arc);
            let auth_for_scheduler = std::sync::Arc::clone(&auth_arc);
            tauri::async_runtime::spawn(async move {
                match scheduler::Scheduler::new(db_for_scheduler, auth_for_scheduler).await {
                    Ok(sched) => {
                        if let Err(e) = sched.start().await {
                            log::error!("Scheduler start failed: {}", e);
                        }
                    }
                    Err(e) => log::error!("Scheduler init failed: {}", e),
                }
            });

            let voice_engine = std::sync::Arc::new(voice::VoiceEngine::new(
                &db_arc,
                Some(app.handle().clone()),
            ));
            let wake_service = std::sync::Arc::new(voice::wake_word::WakeWordService::new(
                std::sync::Arc::clone(&voice_engine),
                std::sync::Arc::clone(&db_arc),
                router.clone(),
                std::sync::Arc::clone(&auth_arc),
                app.handle().clone(),
            ));

            // Auto-briefing on startup (only if enabled)
            let db_brief = std::sync::Arc::clone(&db_arc);
            let auth_brief = std::sync::Arc::clone(&auth_arc);
            let app_handle_brief = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                // Check if auto-briefing is enabled
                let enabled = {
                    let conn = db_brief.conn.lock().unwrap();
                    conn.query_row(
                        "SELECT value FROM user_preferences WHERE key = 'auto_briefing'",
                        [], |row| row.get::<_, String>(0)
                    ).unwrap_or_else(|_| "true".to_string()) == "true"
                };
                if !enabled {
                    log::info!("Auto-briefing disabled");
                    return;
                }
                // Check if we already briefed today
                let already_briefed = {
                    let conn = db_brief.conn.lock().unwrap();
                    conn.query_row(
                        "SELECT value FROM user_preferences WHERE key = 'last_briefing_date'",
                        [], |row| row.get::<_, String>(0)
                    ).unwrap_or_default() == chrono::Local::now().format("%Y-%m-%d").to_string()
                };
                if already_briefed {
                    log::info!("Already briefed today, skipping");
                    return;
                }

                let router = crate::ai::AiRouter::new(
                    std::env::var("ANTHROPIC_API_KEY").ok(),
                    std::env::var("OPENAI_API_KEY").ok(),
                    "claude_primary",
                );
                match crate::assistant::briefing::generate_briefing(&db_brief, &router, &auth_brief, &app_handle_brief).await {
                    Ok(result) => {
                        log::info!("Morning briefing: {}", result.briefing);
                        // Mark as briefed today
                        {
                            let conn = db_brief.conn.lock().unwrap();
                            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                            let _ = conn.execute(
                                "INSERT INTO user_preferences (key, value, updated_at) VALUES ('last_briefing_date', ?1, datetime('now'))
                                 ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
                                rusqlite::params![today],
                            );
                            // Cache the briefing text
                            let _ = conn.execute(
                                "INSERT INTO user_preferences (key, value, updated_at) VALUES ('cached_briefing', ?1, datetime('now'))
                                 ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
                                rusqlite::params![result.briefing],
                            );
                        }
                        let tts = crate::voice::tts::TextToSpeech::from_db(&db_brief);
                        let speech = format!("{}. {}", result.greeting, result.briefing);
                        if let Err(e) = tts.speak(&speech).await {
                            log::warn!("Briefing TTS failed: {}", e);
                        }
                    }
                    Err(e) => log::warn!("Briefing generation failed: {}", e),
                }
            });

            let wake_word_enabled = {
                let conn = db_arc.conn.lock().unwrap();
                conn.query_row(
                    "SELECT value FROM user_preferences WHERE key = 'wake_word_enabled'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap_or_else(|_| "false".to_string())
                    == "true"
            };

            app.manage(std::sync::Arc::clone(&db_arc));
            app.manage(std::sync::Arc::clone(&auth_arc));
            app.manage(std::sync::Mutex::new(router));
            app.manage(voice_engine);
            app.manage(std::sync::Arc::clone(&wake_service));
            tray::create_tray(app).expect("Failed to create system tray");

            if wake_word_enabled {
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = wake_service.enable().await {
                        log::warn!("Wake-word startup enable failed: {}", e);
                    }
                });
            }

            // Wallpaper mode: restore previous state
            let wallpaper_enabled = {
                let conn = db_arc.conn.lock().unwrap();
                conn.query_row(
                    "SELECT value FROM user_preferences WHERE key = 'wallpaper_mode_enabled'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap_or_else(|_| "false".to_string())
                    == "true"
            };

            if wallpaper_enabled {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = wallpaper::enable_on_startup(app_handle).await {
                        log::warn!("Wallpaper mode startup failed: {}", e);
                    }
                });
            }

            // Global shortcut Cmd+Shift+W: raise/lower wallpaper for interaction.
            // This works at the OS level, bypassing setIgnoresMouseEvents_.
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, Code, Modifiers};
                let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyW);
                let app_handle = app.handle().clone();
                app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        if !wallpaper::is_active() {
                            return;
                        }
                        if wallpaper::is_raised() {
                            if let Err(e) = wallpaper::lower_to_background(&app_handle) {
                                log::error!("Failed to lower wallpaper via shortcut: {}", e);
                            }
                        } else {
                            if let Err(e) = wallpaper::raise_for_interaction(&app_handle) {
                                log::error!("Failed to raise wallpaper via shortcut: {}", e);
                            }
                        }
                    }
                }).map_err(|e| format!("Failed to register global shortcut: {}", e))?;
                log::info!("Global shortcut Cmd+Shift+W registered for wallpaper interaction");
            }

            log::info!("JARVIS started successfully");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if crate::wallpaper::is_active() {
                    if let Err(e) = crate::wallpaper::lower_to_background(&window.app_handle()) {
                        log::warn!("Failed to lower wallpaper: {}", e);
                        let _ = window.hide();
                    }
                } else {
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::tasks::get_tasks,
            commands::tasks::create_task,
            commands::tasks::update_task,
            commands::settings::get_settings,
            commands::settings::update_setting,
            commands::chat::send_message,
            commands::chat::get_conversations,
            commands::chat::clear_conversations,
            commands::dashboard::get_dashboard_data,
            commands::email::get_emails,
            commands::email::sync_emails,
            commands::email::archive_email,
            commands::email::get_email_stats,
            commands::email::get_suggested_rules,
            commands::email::accept_email_rule,
            commands::email::dismiss_email_rule,
            commands::email::get_active_rules,
            commands::calendar::get_events,
            commands::calendar::sync_calendar,
            commands::calendar::create_event,
            commands::calendar::get_todays_events,
            commands::cron::get_cron_jobs,
            commands::cron::get_cron_runs,
            commands::cron::create_custom_cron,
            commands::cron::delete_cron_job,
            commands::cron::toggle_cron_job,
            commands::cron::get_upcoming_runs,
            commands::google_auth::google_connect,
            commands::google_auth::google_status,
            commands::notion::get_notion_pages,
            commands::notion::sync_notion,
            commands::notion::save_notion_token,
            commands::notion::get_notion_stats,
            commands::github::get_github_items,
            commands::github::sync_github,
            commands::github::save_github_token,
            commands::github::get_github_stats,
            voice::commands::start_listening,
            voice::commands::stop_listening,
            voice::commands::get_voice_state,
            voice::commands::toggle_mute,
            voice::commands::get_voice_settings,
            voice::commands::set_voice_setting,
            voice::commands::list_tts_voices,
            voice::wake_commands::get_wake_word_status,
            voice::wake_commands::enable_wake_word,
            voice::wake_commands::disable_wake_word,
            voice::wake_commands::is_model_downloaded,
            voice::wake_commands::download_model,
            commands::assistant::get_briefing,
            commands::assistant::speak_briefing,
            commands::assistant::ask_jarvis,
            commands::assistant::search_conversations,
            commands::obsidian::search_obsidian,
            commands::obsidian::get_obsidian_note,
            commands::obsidian::save_obsidian_note,
            commands::obsidian::list_obsidian_files,
            commands::obsidian::save_obsidian_key,
            commands::local_llm::list_local_endpoints,
            commands::local_llm::add_local_endpoint,
            commands::local_llm::update_local_endpoint,
            commands::local_llm::remove_local_endpoint,
            commands::local_llm::test_endpoint_connection,
            commands::local_llm::list_endpoint_models,
            commands::local_llm::get_provider_chain,
            commands::local_llm::update_provider_chain,
            commands::local_llm::update_model_override,
            commands::system::open_application,
            commands::system::open_url,
            commands::system::run_shell_command,
            commands::system::find_files,
            commands::system::open_file,
            commands::system::get_system_info,
            commands::system::write_quick_note,
            wallpaper::enable_wallpaper,
            wallpaper::disable_wallpaper,
            wallpaper::toggle_wallpaper,
            wallpaper::get_wallpaper_status,
            wallpaper::raise_wallpaper,
            wallpaper::lower_wallpaper,
            wallpaper::is_wallpaper_raised,
        ])
        .run(tauri::generate_context!())
        .expect("error while running JARVIS");
}
