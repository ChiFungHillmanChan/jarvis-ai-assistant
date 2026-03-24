pub mod ai;
pub mod assistant;
pub mod auth;
pub mod commands;
pub mod db;
pub mod integrations;
pub mod scheduler;
pub mod tray;
pub mod voice;

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
        .setup(|app| {
            let db = Database::new().expect("Failed to initialize database");
            let claude_key = std::env::var("ANTHROPIC_API_KEY").ok();
            let openai_key = std::env::var("OPENAI_API_KEY").ok();
            let router = AiRouter::new(claude_key, openai_key, "claude_primary");

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

            let voice_engine = std::sync::Arc::new(voice::VoiceEngine::new());

            // Auto-briefing on startup (only if enabled)
            let db_brief = std::sync::Arc::clone(&db_arc);
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
                match crate::assistant::briefing::generate_briefing(&db_brief, &router).await {
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
                        let tts = crate::voice::tts::TextToSpeech::new();
                        let speech = format!("{}. {}", result.greeting, result.briefing);
                        if let Err(e) = tts.speak(&speech).await {
                            log::warn!("Briefing TTS failed: {}", e);
                        }
                    }
                    Err(e) => log::warn!("Briefing generation failed: {}", e),
                }
            });

            app.manage(db_arc);
            app.manage(auth_arc);
            app.manage(router);
            app.manage(voice_engine);
            tray::create_tray(app).expect("Failed to create system tray");
            log::info!("JARVIS started successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::tasks::get_tasks,
            commands::tasks::create_task,
            commands::tasks::update_task,
            commands::settings::get_settings,
            commands::settings::update_setting,
            commands::chat::send_message,
            commands::chat::get_conversations,
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
            commands::assistant::get_briefing,
            commands::assistant::speak_briefing,
            commands::assistant::ask_jarvis,
            commands::assistant::search_conversations,
            commands::obsidian::search_obsidian,
            commands::obsidian::get_obsidian_note,
            commands::obsidian::save_obsidian_note,
            commands::obsidian::list_obsidian_files,
            commands::obsidian::save_obsidian_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running JARVIS");
}
