use crate::system::control;

#[tauri::command]
pub async fn open_url(url: String) -> Result<String, String> {
    control::open_url(&url).await
}
