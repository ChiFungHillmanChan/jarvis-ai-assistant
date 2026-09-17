use crate::db::Database;
use crate::integrations::{obsidian, obsidian_vault::VaultGrant};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, State};

#[derive(Serialize)]
pub struct ObsidianNoteView {
    pub path: String,
    pub content: Option<String>,
}

#[derive(Serialize)]
pub struct ObsidianConnectionStatus {
    pub status: &'static str,
    pub identity: Option<String>,
    pub method: Option<&'static str>,
    pub message: Option<String>,
}

async fn connection_status(db: &Database) -> Result<ObsidianConnectionStatus, String> {
    let generation = obsidian::capture_generation(db)?;
    let (identity, method, verified) = match obsidian::connection(db)? {
        obsidian::Connection::Disconnected => {
            obsidian::check_generation(db, &generation)?;
            return Ok(ObsidianConnectionStatus {
                status: "not_connected",
                identity: None,
                method: None,
                message: Some(
                    "選取 Obsidian 資料庫資料夾即可授予讀取權限，無需 API key。".to_string(),
                ),
            });
        }
        obsidian::Connection::Vault(grant) => (grant.name(), "vault", grant.verify()),
        obsidian::Connection::Rest { base_url, key } => (
            "Obsidian Local REST API".to_string(),
            "rest",
            obsidian::verify_rest(&base_url, &key).await,
        ),
    };
    obsidian::check_generation(db, &generation)?;
    Ok(ObsidianConnectionStatus {
        status: if verified.is_ok() { "connected" } else { "configured" },
        identity: Some(identity), method: Some(method),
        message: Some(match verified {
            Ok(()) if method == "vault" => "已授予此資料庫的 Markdown 讀取權限。每次最多掃描 10,000 個項目、搜尋 16 MiB 文字，單篇上限為 1 MiB。".to_string(),
            Ok(()) => "已驗證 Local REST API 授權。".to_string(),
            Err(error) => error,
        }),
    })
}

#[tauri::command]
pub async fn obsidian_connection_status(
    db: State<'_, Arc<Database>>,
) -> Result<ObsidianConnectionStatus, String> {
    connection_status(&db).await
}

static PICKER_ACTIVE: AtomicBool = AtomicBool::new(false);
struct PickerGuard;
impl Drop for PickerGuard {
    fn drop(&mut self) {
        PICKER_ACTIVE.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
async fn select_directory(app: &AppHandle) -> Result<Option<std::path::PathBuf>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        use cocoa::appkit::{NSModalResponse, NSOpenPanel, NSSavePanel};
        use cocoa::base::{nil, NO, YES};
        use cocoa::foundation::{NSAutoreleasePool, NSString, NSURL};
        // AppKit objects stay on the main thread. Only an owned Rust path
        // crosses back to the asynchronous IPC handler.
        let selected = unsafe {
            let pool = NSAutoreleasePool::new(nil);
            let panel = NSOpenPanel::openPanel(nil);
            panel.setCanChooseFiles_(NO);
            panel.setCanChooseDirectories_(YES);
            panel.setCanCreateDirectories(NO);
            panel.setAllowsMultipleSelection_(NO);
            panel.setResolvesAliases_(NO);
            let result = if panel.runModal() == NSModalResponse::NSModalResponseOk {
                let url = panel.URL();
                if url == nil {
                    Err("未能讀取所選資料夾。".to_string())
                } else {
                    let path = url.path();
                    if path == nil {
                        Err("未能讀取所選資料夾路徑。".to_string())
                    } else {
                        let text = path.UTF8String();
                        if text.is_null() {
                            Err("所選資料夾的路徑編碼不受支援。".to_string())
                        } else {
                            std::ffi::CStr::from_ptr(text)
                                .to_str()
                                .map(|path| Some(std::path::PathBuf::from(path)))
                                .map_err(|_| "所選資料夾的路徑編碼不受支援。".to_string())
                        }
                    }
                }
            } else {
                Ok(None)
            };
            pool.drain();
            result
        };
        let _ = sender.send(selected);
    })
    .map_err(|_| "未能開啟資料夾選擇視窗。")?;
    receiver
        .await
        .map_err(|_| "資料夾選擇視窗已關閉，請重試。".to_string())?
}

#[cfg(not(target_os = "macos"))]
async fn select_directory(_: &AppHandle) -> Result<Option<std::path::PathBuf>, String> {
    Err("目前只支援在 macOS 選取 Obsidian 資料庫。".to_string())
}

#[tauri::command]
pub async fn obsidian_select_vault(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
) -> Result<ObsidianConnectionStatus, String> {
    if PICKER_ACTIVE.swap(true, Ordering::SeqCst) {
        return Err("資料夾選擇視窗已開啟。".to_string());
    }
    let _guard = PickerGuard;
    let Some(path) = select_directory(&app).await? else {
        let mut status = connection_status(&db).await?;
        status.message = Some("已取消選取，原有連接保持不變。".to_string());
        return Ok(status);
    };
    let grant = tokio::task::spawn_blocking(move || VaultGrant::authorize(&path))
        .await
        .map_err(|_| "未能驗證所選資料庫。")??;
    obsidian::store_vault(&db, &grant)?;
    connection_status(&db).await
}

#[tauri::command]
pub fn obsidian_disconnect(db: State<Arc<Database>>) -> Result<(), String> {
    obsidian::disconnect(&db)
}

#[tauri::command]
pub async fn open_obsidian(db: State<'_, Arc<Database>>) -> Result<(), String> {
    let uri = obsidian::open_uri(&db)?;
    tokio::task::spawn_blocking(move || open::that(&uri))
        .await
        .map_err(|_| "未能開啟 Obsidian。請確認已安裝應用程式。")?
        .map_err(|_| "未能開啟 Obsidian。請確認已安裝應用程式。".to_string())
}

#[tauri::command]
pub async fn search_obsidian(
    db: State<'_, Arc<Database>>,
    query: String,
) -> Result<Vec<ObsidianNoteView>, String> {
    Ok(obsidian::search_connected(&db, &query)
        .await?
        .into_iter()
        .map(|note| ObsidianNoteView {
            path: note.path,
            content: note.content,
        })
        .collect())
}

#[tauri::command]
pub async fn get_obsidian_note(
    db: State<'_, Arc<Database>>,
    path: String,
) -> Result<String, String> {
    obsidian::read_connected(&db, &path).await
}

#[tauri::command]
pub async fn save_obsidian_note(
    db: State<'_, Arc<Database>>,
    path: String,
    content: String,
) -> Result<(), String> {
    obsidian::save_connected(&db, &path, &content).await
}

#[tauri::command]
pub async fn list_obsidian_files(db: State<'_, Arc<Database>>) -> Result<Vec<String>, String> {
    obsidian::list_connected(&db).await
}

#[tauri::command]
pub fn save_obsidian_key(db: State<Arc<Database>>, key: String) -> Result<(), String> {
    // The secret stays in Keychain. A nonempty manual credential explicitly
    // switches to REST mode; merely clearing it does not revoke vault access.
    crate::integration_keys::store(&db, "obsidian_api_key", &key)?;
    if !key.trim().is_empty() {
        obsidian::select_rest_mode(&db)?;
    } else {
        obsidian::invalidate_reads(&db)?;
    }
    Ok(())
}
