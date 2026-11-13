//! Settings for what JARVIS does in the background: opening at login, keeping
//! the Mac awake on mains power, and the scheduled daily briefing.

use crate::db::Database;
use crate::scheduler::scheduled_briefing::{self, Brief, BriefingSettings};
use crate::scheduler::SchedulerHandle;
use crate::system::keep_awake::KeepAwakeState;
use crate::system::login_item::{self, LoginItemState};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

/// Whether JARVIS opens at login, as the Mac reports it.
#[tauri::command]
pub fn get_open_at_login(login_item: State<LoginItemState>) -> Result<bool, String> {
    login_item.0.is_enabled()
}

/// Returns the state the Mac reports after the change.
#[tauri::command]
pub fn set_open_at_login(db: State<Arc<Database>>, login_item: State<LoginItemState>, enabled: bool) -> Result<bool, String> {
    login_item::set_by_user(&db, login_item.0.as_ref(), enabled)
}

#[derive(Serialize)]
pub struct KeepAwakeView {
    /// The setting.
    pub enabled: bool,
    /// Idle sleep is being prevented right now. False on battery even with
    /// the setting on.
    pub active: bool,
}

fn keep_awake_view(state: &KeepAwakeState) -> KeepAwakeView {
    KeepAwakeView { enabled: state.setting_on(), active: state.active() }
}

#[tauri::command]
pub fn get_keep_awake(state: State<Arc<KeepAwakeState>>) -> KeepAwakeView {
    keep_awake_view(&state)
}

#[tauri::command]
pub fn set_keep_awake(db: State<Arc<Database>>, state: State<Arc<KeepAwakeState>>, enabled: bool) -> Result<KeepAwakeView, String> {
    state.set(&db, enabled)?;
    Ok(keep_awake_view(&state))
}

#[tauri::command]
pub fn get_daily_briefing_settings(db: State<Arc<Database>>) -> Result<BriefingSettings, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    scheduled_briefing::read_settings(&conn)
}

/// Save the briefing settings and put them under the running scheduler at
/// once, so a new time applies without a relaunch.
#[tauri::command]
pub async fn set_daily_briefing_settings(
    db: State<'_, Arc<Database>>,
    scheduler: State<'_, Arc<SchedulerHandle>>,
    enabled: bool,
    time: String,
    voice: bool,
) -> Result<BriefingSettings, String> {
    let (saved, settings) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let saved = scheduled_briefing::write_settings(&conn, enabled, &time, voice, chrono::Utc::now())?;
        (saved, scheduled_briefing::read_settings(&conn)?)
    };
    // While the scheduler is still starting there is nothing to tell: it reads
    // the saved row when it comes up.
    if let Some(scheduler) = scheduler.get() {
        if saved.active {
            scheduler.register(saved.job_id, scheduled_briefing::JOB_NAME, &saved.schedule, scheduled_briefing::ACTION_TYPE).await?;
        } else {
            scheduler.unregister(saved.job_id).await;
        }
    }
    Ok(settings)
}

/// Today's briefing, if one has been shown today.
#[tauri::command]
pub fn get_daily_briefing(db: State<Arc<Database>>) -> Result<Option<Brief>, String> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    scheduled_briefing::todays_brief(&db, &today)
}
