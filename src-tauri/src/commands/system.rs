use crate::system::control;

#[tauri::command]
pub async fn open_application(name: String) -> Result<String, String> {
    control::open_app(&name).await
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<String, String> {
    control::open_url(&url).await
}

#[tauri::command]
pub async fn run_shell_command(command: String) -> Result<String, String> {
    control::run_command(&command).await
}

#[tauri::command]
pub async fn find_files(query: String, path: Option<String>) -> Result<Vec<String>, String> {
    control::find_files(&query, path.as_deref()).await
}

#[tauri::command]
pub async fn open_file(path: String) -> Result<String, String> {
    control::open_file(&path).await
}

#[tauri::command]
pub async fn get_system_info() -> Result<String, String> {
    control::system_info().await
}

#[tauri::command]
pub async fn write_quick_note(path: String, content: String, append: bool) -> Result<String, String> {
    control::write_note(&path, &content, append).await
}
