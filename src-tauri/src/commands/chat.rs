use crate::ai::AiRouter;
use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: Option<i64>,
    pub role: String,
    pub content: String,
    pub created_at: Option<String>,
}

#[tauri::command]
pub async fn send_message(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    message: String,
) -> Result<ChatMessage, String> {
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('user', ?1)", rusqlite::params![message])
            .map_err(|e| e.to_string())?;
    }
    let messages = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT role, content FROM conversations ORDER BY id DESC LIMIT 20")
            .map_err(|e| e.to_string())?;
        let mut msgs: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        msgs.reverse();
        msgs
    };
    let response_text = router.send(messages).await?;
    let (final_response, _actions) = crate::assistant::actions::execute_actions(&response_text, &db);
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('assistant', ?1)", rusqlite::params![final_response])
            .map_err(|e| e.to_string())?;
    }
    Ok(ChatMessage { id: None, role: "assistant".to_string(), content: final_response, created_at: None })
}

#[tauri::command]
pub fn get_conversations(db: State<Arc<Database>>) -> Result<Vec<ChatMessage>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id, role, content, created_at FROM conversations ORDER BY id ASC")
        .map_err(|e| e.to_string())?;
    let messages = stmt
        .query_map([], |row| {
            Ok(ChatMessage { id: row.get(0)?, role: row.get(1)?, content: row.get(2)?, created_at: row.get(3)? })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(messages)
}
