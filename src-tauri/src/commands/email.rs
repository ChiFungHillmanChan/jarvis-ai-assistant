use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::gmail;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct EmailSummary { pub id: i64, pub gmail_id: String, pub subject: Option<String>, pub sender: String, pub snippet: Option<String>, pub is_read: bool, pub is_spam: bool, pub received_at: String }

#[derive(Serialize)]
pub struct EmailStats { pub unread: i64, pub total: i64, pub spam: i64 }

#[derive(Serialize)]
pub struct EmailRule {
    pub id: i64,
    pub sender: String,
    pub archive_count: i64,
    pub rule_status: String,
}

#[tauri::command]
pub fn get_emails(db: State<Arc<Database>>, limit: Option<u32>) -> Result<Vec<EmailSummary>, String> {
    let limit = limit.unwrap_or(50);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id, gmail_id, subject, sender, snippet, is_read, is_spam, received_at FROM emails ORDER BY received_at DESC LIMIT ?1").map_err(|e| e.to_string())?;
    let emails = stmt.query_map(rusqlite::params![limit], |row| {
        Ok(EmailSummary { id: row.get(0)?, gmail_id: row.get(1)?, subject: row.get(2)?, sender: row.get(3)?, snippet: row.get(4)?, is_read: row.get::<_, i32>(5)? != 0, is_spam: row.get::<_, i32>(6)? != 0, received_at: row.get(7)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(emails)
}

#[tauri::command]
pub async fn sync_emails(db: State<'_, Arc<Database>>, auth: State<'_, Arc<GoogleAuth>>) -> Result<String, String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    let messages = gmail::fetch_inbox(&token, 20).await?;
    let count = messages.len();
    gmail::save_to_db(&db, &messages)?;
    Ok(format!("Synced {} emails", count))
}

/// Called after archiving an email -- increments the sender's archive count
/// and promotes to 'suggested' when threshold (3) is reached
fn track_archive(db: &Database, sender: &str) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;

    // Upsert: increment count or insert new
    conn.execute(
        "INSERT INTO email_rules (sender, archive_count, updated_at)
         VALUES (?1, 1, datetime('now'))
         ON CONFLICT(sender) DO UPDATE SET
            archive_count = archive_count + 1,
            updated_at = datetime('now')",
        rusqlite::params![sender],
    ).map_err(|e| e.to_string())?;

    // Promote to 'suggested' if count >= 3 and still 'pending'
    conn.execute(
        "UPDATE email_rules SET rule_status = 'suggested', updated_at = datetime('now')
         WHERE sender = ?1 AND archive_count >= 3 AND rule_status = 'pending'",
        rusqlite::params![sender],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn archive_email(
    auth: State<'_, Arc<GoogleAuth>>,
    db: State<'_, Arc<Database>>,
    gmail_id: String,
) -> Result<(), String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    gmail::archive_message(&token, &gmail_id).await?;

    // Track sender for rule learning
    let sender = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT sender FROM emails WHERE gmail_id = ?1",
            rusqlite::params![gmail_id], |row| row.get::<_, String>(0),
        ).ok()
    };
    if let Some(sender) = sender {
        track_archive(&db, &sender)?;
    }

    Ok(())
}

#[tauri::command]
pub fn get_email_stats(db: State<Arc<Database>>) -> Result<EmailStats, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let unread: i64 = conn.query_row("SELECT COUNT(*) FROM emails WHERE is_read = 0", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM emails", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    let spam: i64 = conn.query_row("SELECT COUNT(*) FROM emails WHERE is_spam = 1", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    Ok(EmailStats { unread, total, spam })
}

#[tauri::command]
pub fn get_suggested_rules(db: State<Arc<Database>>) -> Result<Vec<EmailRule>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, sender, archive_count, rule_status FROM email_rules
         WHERE rule_status = 'suggested' ORDER BY archive_count DESC"
    ).map_err(|e| e.to_string())?;

    let rules = stmt.query_map([], |row| {
        Ok(EmailRule { id: row.get(0)?, sender: row.get(1)?, archive_count: row.get(2)?, rule_status: row.get(3)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(rules)
}

#[tauri::command]
pub fn accept_email_rule(db: State<Arc<Database>>, rule_id: i64) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE email_rules SET rule_status = 'active', updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![rule_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn dismiss_email_rule(db: State<Arc<Database>>, rule_id: i64) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE email_rules SET rule_status = 'dismissed', updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![rule_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_active_rules(db: State<Arc<Database>>) -> Result<Vec<EmailRule>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, sender, archive_count, rule_status FROM email_rules
         WHERE rule_status = 'active' ORDER BY sender ASC"
    ).map_err(|e| e.to_string())?;

    let rules = stmt.query_map([], |row| {
        Ok(EmailRule { id: row.get(0)?, sender: row.get(1)?, archive_count: row.get(2)?, rule_status: row.get(3)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(rules)
}
