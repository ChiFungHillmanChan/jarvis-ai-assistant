use std::sync::Arc;

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::db::Database;
use crate::payments::{create_checkout_session, open_billing_portal, CheckoutRequest, Tier};

#[derive(Debug, Serialize)]
pub struct CheckoutPayload {
    pub url: String,
    pub session_id: String,
}

/// Stable per-install identity sent to the payment backend as user_id
/// (Stripe client_reference_id / DynamoDB key). Replaced by the Cognito
/// subject once C1.5 lands; until then this maps one install to one customer.
fn resolve_install_id(conn: &Connection) -> Result<String, String> {
    if let Ok(id) = conn.query_row(
        "SELECT value FROM user_preferences WHERE key = 'install_id'",
        [],
        |r| r.get::<_, String>(0),
    ) {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO user_preferences (key, value) VALUES ('install_id', ?1)",
        [&id],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub async fn payments_start_checkout(
    db: State<'_, Arc<Database>>,
    tier: String,
    email: String,
) -> Result<CheckoutPayload, String> {
    let parsed_tier = match tier.as_str() {
        "pro" => Tier::Pro,
        "team" => Tier::Team,
        other => return Err(format!("invalid tier: {}", other)),
    };
    if !email.contains('@') {
        return Err("valid billing email required".to_string());
    }

    let user_id = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        resolve_install_id(&conn)?
    };

    let resp = create_checkout_session(CheckoutRequest {
        tier: parsed_tier,
        user_id: &user_id,
        email: &email,
    })
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = open::that(&resp.url) {
        log::warn!("failed to open browser for checkout: {}", e);
    }

    Ok(CheckoutPayload {
        url: resp.url,
        session_id: resp.session_id,
    })
}

#[tauri::command]
pub async fn payments_open_portal(db: State<'_, Arc<Database>>) -> Result<String, String> {
    let user_id = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        resolve_install_id(&conn)?
    };
    let url = open_billing_portal(&user_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Err(e) = open::that(&url) {
        log::warn!("failed to open browser for portal: {}", e);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_id_is_generated_once_and_stable() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE user_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        let first = resolve_install_id(&conn).unwrap();
        let second = resolve_install_id(&conn).unwrap();
        assert_eq!(first, second, "install_id must be stable across calls");
        assert_eq!(first.len(), 36, "uuid v4 string format");
    }
}
