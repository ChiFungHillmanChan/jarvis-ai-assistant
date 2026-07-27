use crate::db::Database;
use crate::integrations::obsidian;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct ObsidianNoteView {
    pub path: String,
    pub content: Option<String>,
}

fn get_obsidian_key() -> Result<String, String> {
    crate::integration_keys::resolve("obsidian_api_key")
        .ok_or_else(|| "Obsidian API key not configured. Set it in Settings.".to_string())
}

#[tauri::command]
pub async fn search_obsidian(
    db: State<'_, Arc<Database>>,
    query: String,
) -> Result<Vec<ObsidianNoteView>, String> {
    let key = get_obsidian_key()?;
    let base_url = obsidian::resolve_base_url(&db);
    let notes = obsidian::search_vault(&base_url, &key, &query).await?;
    Ok(notes
        .into_iter()
        .map(|n| ObsidianNoteView {
            path: n.path,
            content: n.content,
        })
        .collect())
}

#[tauri::command]
pub async fn get_obsidian_note(
    db: State<'_, Arc<Database>>,
    path: String,
) -> Result<String, String> {
    let key = get_obsidian_key()?;
    let base_url = obsidian::resolve_base_url(&db);
    obsidian::get_note(&base_url, &key, &path).await
}

#[tauri::command]
pub async fn save_obsidian_note(
    db: State<'_, Arc<Database>>,
    path: String,
    content: String,
) -> Result<(), String> {
    let key = get_obsidian_key()?;
    let base_url = obsidian::resolve_base_url(&db);
    obsidian::save_note(&base_url, &key, &path, &content).await
}

#[tauri::command]
pub async fn list_obsidian_files(db: State<'_, Arc<Database>>) -> Result<Vec<String>, String> {
    let key = get_obsidian_key()?;
    let base_url = obsidian::resolve_base_url(&db);
    obsidian::list_files(&base_url, &key).await
}

#[tauri::command]
pub fn save_obsidian_key(db: State<Arc<Database>>, key: String) -> Result<(), String> {
    // Keychain, not user_preferences -- the row keeps only a marker.
    crate::integration_keys::store(&db, "obsidian_api_key", &key)
}
