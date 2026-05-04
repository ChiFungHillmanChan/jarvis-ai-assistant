use crate::ai::AiRouter;
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use std::sync::Arc;
use tauri::State;

/// Manually trigger the daily email briefing for today.
///
/// This is the primary entry point for A9 until the cron auto-trigger is
/// wired. See `scheduler/daily_briefing.rs` for the deferral reason.
///
/// The AiRouter is stored in Tauri State as `std::sync::Mutex<AiRouter>`.
/// We wrap it in `Arc<Mutex<_>>` so `run_briefing` can hold a reference
/// without keeping the State borrow alive across .await points.
#[tauri::command]
pub async fn trigger_daily_briefing(
    db: State<'_, Arc<Database>>,
    router: State<'_, std::sync::Mutex<AiRouter>>,
    auth: State<'_, Arc<GoogleAuth>>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Clone AiRouter out of Mutex into an Arc<Mutex<_>> that run_briefing
    // can own.  This avoids holding a non-Send MutexGuard across .await.
    let router_clone = router
        .lock()
        .map_err(|e| format!("router lock: {e}"))?
        .clone();
    let router_arc = Arc::new(std::sync::Mutex::new(router_clone));

    crate::scheduler::daily_briefing::run_briefing(
        db.inner().clone(),
        router_arc,
        auth.inner().clone(),
        app_handle,
    )
    .await
}
