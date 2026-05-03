use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct LaunchReadiness {
    pub launch_stage: String,
    pub aws_region: Option<String>,
    pub bedrock_model_id: String,
    pub aws_credentials_present: bool,
    pub provider_chain_has_bedrock: bool,
    pub waitlist_count: i64,
    pub feedback_count: i64,
    pub pro_price_hkd: String,
    pub team_price_hkd: String,
}

#[derive(Serialize)]
pub struct BetaTesterView {
    pub id: i64,
    pub name: Option<String>,
    pub email: String,
    pub segment: Option<String>,
    pub use_case: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct BetaTesterInput {
    pub name: Option<String>,
    pub email: String,
    pub segment: Option<String>,
    pub use_case: Option<String>,
}

#[derive(Deserialize)]
pub struct BetaFeedbackInput {
    pub tester_email: Option<String>,
    pub category: String,
    pub rating: i32,
    pub message: String,
}

#[tauri::command]
pub fn get_launch_readiness(db: State<Arc<Database>>) -> Result<LaunchReadiness, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let setting = |key: &str, fallback: &str| -> String {
        conn.query_row(
            "SELECT value FROM user_preferences WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| fallback.to_string())
    };

    let provider_chain_has_bedrock = conn
        .query_row(
            "SELECT COUNT(*) FROM provider_chain WHERE provider_type = 'bedrock' AND enabled = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    let waitlist_count = conn
        .query_row("SELECT COUNT(*) FROM beta_testers", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0);
    let feedback_count = conn
        .query_row("SELECT COUNT(*) FROM beta_feedback", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0);

    let aws_region = std::env::var("AWS_BEDROCK_REGION")
        .ok()
        .or_else(|| std::env::var("AWS_REGION").ok())
        .filter(|value| !value.trim().is_empty());
    let bedrock_model_id = std::env::var("AWS_BEDROCK_MODEL_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(crate::ai::bedrock::default_model);
    let aws_credentials_present = std::env::var("AWS_ACCESS_KEY_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_some()
        && std::env::var("AWS_SECRET_ACCESS_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some();

    Ok(LaunchReadiness {
        launch_stage: setting("launch_stage", "private_beta"),
        aws_region,
        bedrock_model_id,
        aws_credentials_present,
        provider_chain_has_bedrock,
        waitlist_count,
        feedback_count,
        pro_price_hkd: setting("pro_price_hkd", "148"),
        team_price_hkd: setting("team_price_hkd", "298"),
    })
}

#[tauri::command]
pub fn save_beta_tester(
    db: State<Arc<Database>>,
    input: BetaTesterInput,
) -> Result<BetaTesterView, String> {
    let email = input.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err("Enter a valid email address.".to_string());
    }

    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO beta_testers (name, email, segment, use_case, status)
         VALUES (?1, ?2, ?3, ?4, 'waitlisted')
         ON CONFLICT(email) DO UPDATE SET
         name = excluded.name,
         segment = excluded.segment,
         use_case = excluded.use_case",
        rusqlite::params![
            clean_optional(input.name),
            email,
            clean_optional(input.segment),
            clean_optional(input.use_case),
        ],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    get_beta_tester_by_id_or_email(&conn, id, &email)
}

#[tauri::command]
pub fn submit_beta_feedback(
    db: State<Arc<Database>>,
    input: BetaFeedbackInput,
) -> Result<(), String> {
    let category = input.category.trim();
    let message = input.message.trim();
    if message.len() < 8 {
        return Err("Feedback needs at least 8 characters.".to_string());
    }
    if !(1..=5).contains(&input.rating) {
        return Err("Rating must be between 1 and 5.".to_string());
    }

    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO beta_feedback (tester_email, category, rating, message)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            clean_optional(input.tester_email).map(|email| email.to_lowercase()),
            if category.is_empty() { "general" } else { category },
            input.rating,
            message,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn get_beta_tester_by_id_or_email(
    conn: &rusqlite::Connection,
    id: i64,
    email: &str,
) -> Result<BetaTesterView, String> {
    conn.query_row(
        "SELECT id, name, email, segment, use_case, status, created_at
         FROM beta_testers
         WHERE id = ?1 OR email = ?2
         ORDER BY id DESC
         LIMIT 1",
        rusqlite::params![id, email],
        |row| {
            Ok(BetaTesterView {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                segment: row.get(3)?,
                use_case: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}
