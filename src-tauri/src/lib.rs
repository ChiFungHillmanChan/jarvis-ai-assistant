pub mod ai;
pub mod assistant;
pub mod aws;
pub mod auth;
pub mod commands;
pub mod chat_session;
pub mod db;
pub mod integration_keys;
pub mod integrations;
pub mod mail;
#[cfg(feature = "native-qa")]
mod native_qa;
pub mod payments;
pub mod scheduler;
pub mod security;
pub mod system;
pub mod tray;
pub mod util;
pub mod voice;
pub mod wallpaper;

use ai::AiRouter;
use db::Database;
use tauri::Manager;

pub fn run() {
    #[cfg(not(feature = "native-qa"))]
    {
        if let Some(data_dir) = dirs::data_dir() {
            let env_path = data_dir.join("jarvis").join(".env");
            if env_path.exists() {
                dotenvy::from_path(&env_path).ok();
            }
        }
        dotenvy::dotenv().ok();
    }
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // A failure here is total -- no database, no app -- and in a
            // packaged .app the panic message goes to a stderr nobody reads, so
            // JARVIS just fails to launch with no window and no explanation.
            // Name the file and the error before giving up.
            let db = match Database::new() {
                Ok(db) => db,
                Err(e) => {
                    #[cfg(feature = "native-qa")]
                    let db_path = std::env::var("JARVIS_NATIVE_QA_DIR")
                        .map(|d| std::path::PathBuf::from(d).join("jarvis.db").display().to_string())
                        .unwrap_or_else(|_| "JARVIS_NATIVE_QA_DIR is not configured".to_string());
                    #[cfg(not(feature = "native-qa"))]
                    let db_path = dirs::data_dir()
                        .map(|d| d.join("jarvis").join("jarvis.db").display().to_string())
                        .unwrap_or_else(|| "~/Library/Application Support/jarvis/jarvis.db".to_string());
                    let message = format!(
                        "JARVIS cannot start: its database could not be opened.\n\n{}\n\nDatabase file: {}\n\nIf that file is damaged, move it aside and launch JARVIS again to start with an empty database.",
                        e, db_path
                    );
                    log::error!("{}", message);
                    show_fatal_error(&message);
                    return Err(e);
                }
            };
            // Older builds wrote the GitHub, Notion, Obsidian and MiniMax
            // credentials into user_preferences in plaintext. Move anything
            // still sitting there into the Keychain before the first read.
            // Best effort: an unavailable Keychain must not block startup.
            integration_keys::migrate_plaintext_to_keychain(&db);

            // Bound what JARVIS keeps on disk before anything else starts
            // writing to it. Mail sync walks backwards through the mailbox
            // across restarts, so without this the database only ever grows.
            commands::storage::sweep_on_startup(&db);

            let ai_provider = {
                let conn = db.conn.lock().unwrap();
                conn.query_row("SELECT value FROM user_preferences WHERE key = 'ai_provider'", [], |row| row.get::<_, String>(0))
                    .unwrap_or_else(|_| "gemini_primary".to_string())
            };
            log::info!("AI provider: {}", ai_provider);
            let mut router = AiRouter::new(&ai_provider);
            router.load_from_db(&db);
            router.reload_keys();
            router.append_default_fallbacks();
            log::info!("Provider chain: {} entries, {} local endpoints",
                router.provider_chain().len(), router.local_endpoints().len());

            #[cfg(feature = "native-qa")]
            let google_auth = auth::google::GoogleAuth::new_empty();
            #[cfg(not(feature = "native-qa"))]
            let google_auth = auth::google::GoogleAuth::new()
                .unwrap_or_else(|| {
                    log::warn!("Google credentials not configured");
                    auth::google::GoogleAuth::new_empty()
                });
            google_auth.load_from_db(&db);

            let db_arc = std::sync::Arc::new(db);
            let auth_arc = std::sync::Arc::new(google_auth);

            // Warm the Google access token from the persisted refresh token so
            // background jobs and UI status work immediately after restart.
            if auth_arc.has_refresh_token() {
                let auth_warm = std::sync::Arc::clone(&auth_arc);
                tauri::async_runtime::spawn(async move {
                    match auth_warm.refresh_access_token().await {
                        Ok(()) => log::info!("Google access token warmed on startup"),
                        Err(e) => log::warn!("Google token warm-up failed: {}", e),
                    }
                });
            }

            // The scheduler handle is published into Tauri state so cron
            // commands can register and unregister jobs on the running
            // scheduler. Without it a job created in the dashboard never fired
            // until the next launch.
            let scheduler_handle = std::sync::Arc::new(scheduler::SchedulerHandle::new());
            let db_for_scheduler = std::sync::Arc::clone(&db_arc);
            let auth_for_scheduler = std::sync::Arc::clone(&auth_arc);
            let handle_for_scheduler = std::sync::Arc::clone(&scheduler_handle);
            tauri::async_runtime::spawn(async move {
                match scheduler::Scheduler::new(db_for_scheduler, auth_for_scheduler).await {
                    Ok(sched) => {
                        let sched = std::sync::Arc::new(sched);
                        if let Err(e) = sched.start().await {
                            log::error!("Scheduler start failed: {}", e);
                        }
                        handle_for_scheduler.set(sched);
                    }
                    Err(e) => log::error!("Scheduler init failed: {}", e),
                }
            });

            // Detect a locally-running Ollama on first boot. If reachable,
            // ensure an endpoint row exists so the user can wire it into the
            // provider chain from Settings without manual URL entry. We do
            // not auto-insert into provider_chain -- that stays a user choice.
            let db_for_ollama = std::sync::Arc::clone(&db_arc);
            let handle_for_ollama = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let client = match reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(2))
                    .build()
                {
                    Ok(c) => c,
                    Err(_) => return,
                };
                let body = match client.get("http://localhost:11434/").send().await {
                    Ok(r) => r.text().await.unwrap_or_default(),
                    Err(_) => return,
                };
                if !body.contains("Ollama") {
                    return;
                }
                let inserted = match db_for_ollama.conn.lock() {
                    Ok(conn) => {
                        let res = conn.execute(
                            "INSERT OR IGNORE INTO local_endpoints
                                (id, name, url, backend_type, is_active)
                             VALUES ('ollama-local', 'Ollama (local)',
                                     'http://localhost:11434', 'ollama', 1)",
                            [],
                        );
                        match res {
                            Ok(0) => {
                                log::debug!("Ollama endpoint already registered");
                                false
                            }
                            Ok(_) => {
                                log::info!("Detected local Ollama at :11434, endpoint registered");
                                true
                            }
                            Err(e) => {
                                log::warn!("Ollama endpoint insert failed: {e}");
                                false
                            }
                        }
                    }
                    Err(_) => false,
                };
                // The managed router loaded its endpoints before this row
                // existed, so without a reload the freshly registered Ollama
                // could not be chained until the next relaunch -- the Settings
                // list showed it and selecting it did nothing. The lock is
                // taken only after the database guard above has been dropped.
                if inserted {
                    match handle_for_ollama.try_state::<std::sync::Mutex<AiRouter>>() {
                        Some(managed) => match managed.lock() {
                            Ok(mut router) => {
                                router.load_from_db(&db_for_ollama);
                                log::info!("Router endpoints reloaded after Ollama detection");
                            }
                            Err(e) => log::warn!("Router lock failed after Ollama detection: {e}"),
                        },
                        None => log::warn!("Router unavailable after Ollama detection"),
                    }
                }
            });

            let voice_engine = std::sync::Arc::new(voice::VoiceEngine::new(
                &db_arc,
                Some(app.handle().clone()),
            ));
            let wake_service = std::sync::Arc::new(voice::wake_word::WakeWordService::new(
                std::sync::Arc::clone(&voice_engine),
                std::sync::Arc::clone(&db_arc),
                std::sync::Arc::clone(&auth_arc),
                app.handle().clone(),
            ));

            // Auto-briefing on startup (only if enabled)
            let db_brief = std::sync::Arc::clone(&db_arc);
            let auth_brief = std::sync::Arc::clone(&auth_arc);
            let voice_engine_brief = std::sync::Arc::clone(&voice_engine);
            // Same provider the managed router was built from. This used to be
            // the literal "gemini_primary", which quietly ignored the provider
            // the user picked in Settings.
            let provider_brief = ai_provider.clone();
            let app_handle_brief = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                // Check if auto-briefing is enabled. Defaults to off, matching
                // the '0' seed in V15 -- an app that starts talking out loud
                // three seconds after first launch is not a good introduction.
                let enabled = {
                    let conn = db_brief.conn.lock().unwrap();
                    conn.query_row(
                        "SELECT value FROM user_preferences WHERE key = 'auto_briefing'",
                        [], |row| row.get::<_, String>(0)
                    ).unwrap_or_else(|_| "false".to_string()) == "true"
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

                let mut router = crate::ai::AiRouter::new(&provider_brief);
                router.load_from_db(&db_brief);
                router.reload_keys();
                router.append_default_fallbacks();
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
                        // Share VoiceEngine's TextToSpeech, and therefore its
                        // cancel flag, rather than building a private one: the
                        // briefing is the longest utterance the app ever
                        // speaks, and a private flag is one nothing in the UI
                        // can reach, so it was uninterruptible.
                        let tts = match voice_engine_brief.tts.lock() {
                            Ok(guard) => guard.clone(),
                            Err(e) => {
                                log::warn!("Briefing TTS unavailable: {}", e);
                                return;
                            }
                        };
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
            app.manage(commands::model_pull::ModelPullState::new());
            app.manage(commands::local_setup::LocalSetupState::new());
            app.manage(mail::sync::MailSyncState::new());
            app.manage(std::sync::Arc::clone(&scheduler_handle));
            app.manage(std::sync::Arc::new(auth::cognito::CognitoSession::new(
                auth::cognito::CognitoConfig::from_env(),
            )));
            // A tray that fails to build is a missing menu bar icon, not a
            // reason to abort startup with an invisible panic.
            if let Err(e) = tray::create_tray(app) {
                log::error!("System tray unavailable: {}", e);
            }

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
                let registered = app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
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
                });
                // A conflict here used to abort setup() entirely, so JARVIS
                // silently refused to launch on any Mac where another app
                // already owned Cmd+Shift+W. The shortcut only toggles an
                // optional wallpaper mode -- never worth failing startup.
                match registered {
                    Ok(()) => log::info!("Global shortcut Cmd+Shift+W registered for wallpaper interaction"),
                    Err(e) => log::warn!("Cmd+Shift+W unavailable (another app owns it): {}", e),
                }
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
            commands::mail::mail_workspace,
            commands::mail::mail_connect_account,
            commands::mail::mail_disconnect_account,
            commands::mail::mail_remove_account,
            commands::mail::mail_sync_account,
            commands::mail::mail_restart_sync,
            commands::mail::mail_list_messages,
            commands::mail::mail_get_message,
            commands::mail::mail_save_group,
            commands::mail::mail_delete_group,
            commands::mail::mail_set_group_members,
            commands::mail::mail_list_memories,
            commands::mail::mail_save_memory,
            commands::mail::mail_delete_memory,
            commands::mail::mail_save_settings,
            commands::mail::mail_analyze_group,
            commands::tasks::get_tasks,
            commands::tasks::create_task,
            commands::tasks::update_task,
            commands::settings::get_settings,
            commands::settings::update_setting,
            commands::settings::get_user_preference,
            commands::settings::set_user_preference,
            commands::settings::set_api_key,
            commands::settings::get_api_key_status,
            commands::chat::send_message,
            commands::chat::get_conversations,
            commands::chat::clear_conversations,
            commands::chat::list_chat_sessions,
            commands::cost::get_cost_stats,
            commands::cost::get_blended_cost,
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
            commands::cron::preview_custom_cron,
            commands::cron::create_reviewed_cron,
            commands::cron::set_cron_job_status,
            commands::cron::delete_cron_job,
            commands::cron::toggle_cron_job,
            commands::cron::get_upcoming_runs,
            commands::google_auth::google_connect,
            commands::google_auth::google_status,
            commands::google_auth::google_disconnect,
            commands::notion::get_notion_pages,
            commands::notion::sync_notion,
            commands::notion::save_notion_token,
            commands::notion::get_notion_stats,
            commands::notion::notion_connect,
            commands::notion::notion_cancel_connect,
            commands::notion::notion_connection_status,
            commands::notion::notion_disconnect,
            commands::github::get_github_items,
            commands::github::sync_github,
            commands::github::save_github_token,
            commands::github::get_github_stats,
            commands::github::github_begin_connect,
            commands::github::github_complete_connect,
            commands::github::github_cancel_connect,
            commands::github::github_connection_status,
            commands::github::github_disconnect,
            commands::github::github_set_client_id,
            commands::launch::get_launch_readiness,
            commands::launch::save_beta_tester,
            commands::launch::submit_beta_feedback,
            commands::payments::payments_start_checkout,
            commands::payments::payments_open_portal,
            commands::payments::payments_reopen_url,
            commands::account::account_status,
            commands::account::account_sign_in,
            commands::account::account_sign_out,
            voice::commands::start_listening,
            voice::commands::stop_listening,
            voice::commands::stop_speaking,
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
            commands::obsidian::obsidian_select_vault,
            commands::obsidian::obsidian_connection_status,
            commands::obsidian::obsidian_disconnect,
            commands::obsidian::open_obsidian,
            commands::aws::set_aws_credentials,
            commands::aws::get_aws_credentials_status,
            commands::aws::clear_aws_credentials,
            commands::onboarding::check_onboarding_status,
            commands::onboarding::mark_onboarding_complete,
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
            commands::system_check::check_system_compatibility,
            commands::system_check::check_backend_prerequisites,
            commands::system_check::get_recommended_models,
            commands::model_pull::pull_model,
            commands::setup::get_setup_status,
            commands::setup::test_mail_ai_connection,
            commands::setup::apply_tested_mail_settings,
            commands::local_setup::probe_local_setup,
            commands::local_setup::prepare_local_ai,
            commands::local_setup::cancel_local_setup,
            commands::model_pull::cancel_model_pull,
            commands::scheduler::trigger_daily_briefing,
            commands::voice_setup::enable_offline_voice,
            commands::voice_setup::check_cosyvoice_binary,
            commands::storage::get_storage_usage,
            commands::storage::run_storage_sweep,
            commands::storage::purge_storage,
            commands::storage::set_retention_policy,
        ])
        .run(tauri::generate_context!())
        .expect("error while running JARVIS");
}

/// Escape a message for use inside an AppleScript string literal. A literal
/// cannot span lines, so newlines become the `\n` escape rather than real ones.
fn applescript_escape(message: &str) -> String {
    message
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Show a blocking native alert for a failure that happens before any window
/// exists. Nothing else on that path is visible to a user running a packaged
/// .app, where stderr goes nowhere they will ever look.
fn show_fatal_error(message: &str) {
    let script = format!(
        "display dialog \"{}\" with title \"JARVIS\" buttons {{\"Quit\"}} default button 1 with icon stop",
        applescript_escape(message)
    );
    if let Err(e) = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .status()
    {
        log::error!("Could not display the fatal error dialog: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fatal-error dialog is the only thing a user sees when the database
    /// will not open. A quote or a newline in the sqlite error message must not
    /// break the script and take the message down with it.
    #[test]
    fn fatal_error_messages_survive_applescript_quoting() {
        let escaped = applescript_escape("file is not a \"database\"\nC:\\path");
        assert_eq!(escaped, "file is not a \\\"database\\\"\\nC:\\\\path");
        assert!(!escaped.contains('\n'), "a literal newline would break the script");
    }

    /// The shipped .app is hardened-runtime, so an empty entitlement set means
    /// macOS never shows the microphone prompt and voice captures silence. The
    /// bundle must point at the entitlements file, and that file must carry the
    /// microphone and Apple Events entitlements; Info.plist must explain the
    /// Apple Events use or the automation prompt is denied without being shown.
    #[test]
    fn release_bundle_carries_the_microphone_entitlement() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json is valid JSON");
        assert_eq!(
            conf["bundle"]["macOS"]["entitlements"],
            "Entitlements.plist",
            "bundle.macOS.entitlements must point at the entitlements file"
        );

        let entitlements = include_str!("../Entitlements.plist");
        assert!(
            entitlements.contains("com.apple.security.device.audio-input"),
            "Entitlements.plist must carry the microphone entitlement"
        );
        assert!(
            entitlements.contains("com.apple.security.automation.apple-events"),
            "Entitlements.plist must carry the Apple Events entitlement"
        );

        let info_plist = include_str!("../Info.plist");
        assert!(
            info_plist.contains("NSAppleEventsUsageDescription"),
            "Info.plist must declare why JARVIS sends Apple Events"
        );
    }

    /// `stop_speaking` used to exist only in an old git stash: the frontend
    /// called `invoke("stop_speaking")` (`src/lib/commands.ts`) against a
    /// command that was neither implemented nor registered, so Escape did
    /// nothing. A `pub fn` left out of `generate_handler!` still compiles
    /// clean -- it is public API surface on this lib crate, not dead code --
    /// so the compiler cannot catch a dropped registration; this checks the
    /// wiring as a property of the source text instead, the same way
    /// `settings_page_offers_the_same_model_ids_as_gemini_rs` in
    /// `ai/gemini.rs` guards a fact the compiler has no way to see.
    #[test]
    fn stop_speaking_is_registered() {
        let lib_rs = include_str!("lib.rs");
        let handler_start = lib_rs
            .find("tauri::generate_handler![")
            .expect("generate_handler! block must exist in lib.rs");
        // Stop at the macro's own closing bracket. Left unbounded the slice ran
        // to end of file and swallowed this test's own source, so the assertion
        // message below matched itself and the test could never fail.
        let handler_len = lib_rs[handler_start..]
            .find(']')
            .expect("generate_handler! block must be closed");
        let handler_block = &lib_rs[handler_start..handler_start + handler_len];
        assert!(
            handler_block.contains("voice::commands::stop_speaking"),
            "voice::commands::stop_speaking must be registered in generate_handler!"
        );
    }

    #[test]
    fn account_commands_are_registered() {
        let lib_rs = include_str!("lib.rs");
        let handler_start = lib_rs
            .find("tauri::generate_handler![")
            .expect("generate_handler! block must exist in lib.rs");
        let handler_len = lib_rs[handler_start..]
            .find(']')
            .expect("generate_handler! block must be closed");
        let handler_block = &lib_rs[handler_start..handler_start + handler_len];
        for command in [
            "commands::account::account_status",
            "commands::account::account_sign_in",
            "commands::account::account_sign_out",
        ] {
            assert!(handler_block.contains(command), "{command} must be registered in generate_handler!");
        }
    }
}
