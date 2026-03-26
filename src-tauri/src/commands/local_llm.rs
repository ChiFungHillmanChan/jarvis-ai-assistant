use crate::ai::local::backend::{BackendType, EndpointHealth, LocalEndpoint, LocalModel};
use crate::ai::local::{self, backend::ToolCapability};
use crate::ai::{AiRouter, ProviderChainEntry};
use crate::db::Database;
use std::sync::{Arc, Mutex};
use tauri::State;

#[tauri::command]
pub async fn list_local_endpoints(db: State<'_, Arc<Database>>) -> Result<Vec<LocalEndpoint>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, url, backend_type, api_key, use_tls, \
             connection_timeout_ms, keep_alive_minutes, is_active, \
             last_health_check, last_health_status FROM local_endpoints ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let endpoints = stmt
        .query_map([], |row| {
            Ok(LocalEndpoint {
                id: row.get(0)?,
                name: row.get(1)?,
                url: row.get(2)?,
                backend_type: BackendType::from_str(&row.get::<_, String>(3).unwrap_or_default()),
                api_key: row.get(4)?,
                use_tls: row.get::<_, i32>(5).unwrap_or(0) != 0,
                connection_timeout_ms: row.get::<_, u32>(6).unwrap_or(5000),
                keep_alive_minutes: row.get::<_, u32>(7).unwrap_or(30),
                is_active: row.get::<_, i32>(8).unwrap_or(1) != 0,
                last_health_check: row.get(9)?,
                last_health_status: row.get::<_, Option<i32>>(10)?.map(|v| v != 0),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(endpoints)
}

#[tauri::command]
pub async fn add_local_endpoint(
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    name: String,
    url: String,
    backend_type: Option<String>,
    api_key: Option<String>,
) -> Result<LocalEndpoint, String> {
    // Auto-detect backend if not specified
    let detected = match backend_type {
        Some(ref bt) => BackendType::from_str(bt),
        None => local::detect_backend(&url).await,
    };

    // Health check
    let backend = local::get_backend(&detected);
    let healthy = backend
        .health_check(&url, api_key.as_deref())
        .await
        .unwrap_or(false);

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO local_endpoints (id, name, url, backend_type, api_key, last_health_check, last_health_status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            id,
            name,
            url,
            detected.as_str(),
            api_key,
            now,
            healthy as i32,
        ],
    )
    .map_err(|e| e.to_string())?;

    let endpoint = LocalEndpoint {
        id,
        name,
        url,
        backend_type: detected,
        api_key,
        use_tls: false,
        connection_timeout_ms: 5000,
        keep_alive_minutes: 30,
        is_active: true,
        last_health_check: Some(now),
        last_health_status: Some(healthy),
    };

    // Sync in-memory router
    {
        let mut r = router.lock().map_err(|e| e.to_string())?;
        let mut eps = r.local_endpoints().to_vec();
        eps.push(endpoint.clone());
        r.set_local_endpoints(eps);
    }

    Ok(endpoint)
}

#[tauri::command]
pub async fn update_local_endpoint(
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    id: String,
    name: Option<String>,
    url: Option<String>,
    api_key: Option<String>,
    connection_timeout_ms: Option<u32>,
    keep_alive_minutes: Option<u32>,
    is_active: Option<bool>,
) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;

    if let Some(name) = name {
        conn.execute(
            "UPDATE local_endpoints SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(url) = url {
        conn.execute(
            "UPDATE local_endpoints SET url = ?1 WHERE id = ?2",
            rusqlite::params![url, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(key) = api_key {
        conn.execute(
            "UPDATE local_endpoints SET api_key = ?1 WHERE id = ?2",
            rusqlite::params![key, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(timeout) = connection_timeout_ms {
        conn.execute(
            "UPDATE local_endpoints SET connection_timeout_ms = ?1 WHERE id = ?2",
            rusqlite::params![timeout, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(keep_alive) = keep_alive_minutes {
        conn.execute(
            "UPDATE local_endpoints SET keep_alive_minutes = ?1 WHERE id = ?2",
            rusqlite::params![keep_alive, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(active) = is_active {
        conn.execute(
            "UPDATE local_endpoints SET is_active = ?1 WHERE id = ?2",
            rusqlite::params![active as i32, id],
        )
        .map_err(|e| e.to_string())?;
    }
    drop(conn);

    // Sync in-memory router with DB state
    {
        let mut r = router.lock().map_err(|e| e.to_string())?;
        r.load_from_db(&db);
    }

    Ok(())
}

#[tauri::command]
pub async fn remove_local_endpoint(
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    id: String,
) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM local_endpoints WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
    drop(conn);

    // Sync in-memory router
    {
        let mut r = router.lock().map_err(|e| e.to_string())?;
        let eps: Vec<_> = r.local_endpoints().iter().filter(|e| e.id != id).cloned().collect();
        r.set_local_endpoints(eps);
    }

    Ok(())
}

#[tauri::command]
pub async fn test_endpoint_connection(
    db: State<'_, Arc<Database>>,
    endpoint_id: String,
) -> Result<EndpointHealth, String> {
    let (url, backend_type_str, api_key) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT url, backend_type, api_key FROM local_endpoints WHERE id = ?1",
            rusqlite::params![endpoint_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .map_err(|e| format!("Endpoint not found: {}", e))?
    };

    let backend_type = BackendType::from_str(&backend_type_str);
    let backend = local::get_backend(&backend_type);

    let start = std::time::Instant::now();
    let reachable = backend
        .health_check(&url, api_key.as_deref())
        .await
        .unwrap_or(false);
    let latency_ms = start.elapsed().as_millis() as u64;

    let model_count = if reachable {
        backend
            .list_models(&url, api_key.as_deref())
            .await
            .map(|m| m.len() as u32)
            .unwrap_or(0)
    } else {
        0
    };

    // Update health status in DB
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE local_endpoints SET last_health_check = ?1, last_health_status = ?2 WHERE id = ?3",
            rusqlite::params![now, reachable as i32, endpoint_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(EndpointHealth {
        reachable,
        model_count,
        latency_ms,
    })
}

#[tauri::command]
pub async fn list_endpoint_models(
    db: State<'_, Arc<Database>>,
    endpoint_id: String,
) -> Result<Vec<LocalModel>, String> {
    let (url, backend_type_str, api_key) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT url, backend_type, api_key FROM local_endpoints WHERE id = ?1",
            rusqlite::params![endpoint_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .map_err(|e| format!("Endpoint not found: {}", e))?
    };

    let backend_type = BackendType::from_str(&backend_type_str);
    let backend = local::get_backend(&backend_type);

    let models = backend
        .list_models(&url, api_key.as_deref())
        .await?;

    let mut result = Vec::new();
    for m in models {
        let cap = backend.detect_tool_capability(&url, &m.id).await;

        // Check for user overrides
        let (ctx_override, cap_override) = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let ctx: Option<u32> = conn
                .query_row(
                    "SELECT context_length FROM local_model_overrides WHERE endpoint_id = ?1 AND model_id = ?2",
                    rusqlite::params![endpoint_id, m.id],
                    |row| row.get(0),
                )
                .ok();
            let cap_ov: Option<String> = conn
                .query_row(
                    "SELECT tool_capability FROM local_model_overrides WHERE endpoint_id = ?1 AND model_id = ?2",
                    rusqlite::params![endpoint_id, m.id],
                    |row| row.get(0),
                )
                .ok();
            (ctx, cap_ov)
        };

        let final_cap = if let Some(ref ov) = cap_override {
            if ov != "auto" {
                ToolCapability::from_str(ov)
            } else {
                cap
            }
        } else {
            cap
        };

        result.push(LocalModel {
            id: m.id,
            endpoint_id: endpoint_id.clone(),
            context_length: ctx_override.unwrap_or(m.context_length.unwrap_or(4096)),
            supports_tools: final_cap,
        });
    }

    Ok(result)
}

#[tauri::command]
pub async fn get_provider_chain(db: State<'_, Arc<Database>>) -> Result<Vec<ProviderChainEntry>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT position, provider_type, endpoint_id, model_id, enabled \
             FROM provider_chain ORDER BY position ASC",
        )
        .map_err(|e| e.to_string())?;

    let chain = stmt
        .query_map([], |row| {
            Ok(ProviderChainEntry {
                position: row.get(0)?,
                provider_type: row.get(1)?,
                endpoint_id: row.get(2)?,
                model_id: row.get(3)?,
                enabled: row.get::<_, i32>(4).unwrap_or(1) != 0,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // If no chain configured, return defaults based on legacy setting
    if chain.is_empty() {
        let ai_provider = conn
            .query_row(
                "SELECT value FROM user_preferences WHERE key = 'ai_provider'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "claude_primary".to_string());

        let defaults = match ai_provider.as_str() {
            "openai_primary" => vec![
                ProviderChainEntry { position: 0, provider_type: "openai".into(), endpoint_id: None, model_id: None, enabled: true },
                ProviderChainEntry { position: 1, provider_type: "claude".into(), endpoint_id: None, model_id: None, enabled: true },
            ],
            "claude_only" => vec![
                ProviderChainEntry { position: 0, provider_type: "claude".into(), endpoint_id: None, model_id: None, enabled: true },
            ],
            "openai_only" => vec![
                ProviderChainEntry { position: 0, provider_type: "openai".into(), endpoint_id: None, model_id: None, enabled: true },
            ],
            _ => vec![
                ProviderChainEntry { position: 0, provider_type: "claude".into(), endpoint_id: None, model_id: None, enabled: true },
                ProviderChainEntry { position: 1, provider_type: "openai".into(), endpoint_id: None, model_id: None, enabled: true },
            ],
        };
        return Ok(defaults);
    }

    Ok(chain)
}

#[tauri::command]
pub async fn update_provider_chain(
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    chain: Vec<ProviderChainEntry>,
) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;

    // Validate local provider references
    for entry in &chain {
        if entry.provider_type == "local" {
            if let Some(ref eid) = entry.endpoint_id {
                let exists: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM local_endpoints WHERE id = ?1",
                        rusqlite::params![eid],
                        |row| row.get::<_, i32>(0),
                    )
                    .unwrap_or(0)
                    > 0;
                if !exists {
                    return Err(format!("Endpoint '{}' does not exist", eid));
                }
            } else {
                return Err("Local provider entry must have endpoint_id".to_string());
            }
        }
    }

    // Clear existing chain and insert new
    conn.execute("DELETE FROM provider_chain", [])
        .map_err(|e| e.to_string())?;

    for (i, entry) in chain.iter().enumerate() {
        conn.execute(
            "INSERT INTO provider_chain (position, provider_type, endpoint_id, model_id, enabled) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                i as i32,
                entry.provider_type,
                entry.endpoint_id,
                entry.model_id,
                entry.enabled as i32,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    drop(conn);

    // Sync in-memory router
    {
        let mut r = router.lock().map_err(|e| e.to_string())?;
        r.set_provider_chain(chain);
    }

    Ok(())
}

#[tauri::command]
pub async fn update_model_override(
    db: State<'_, Arc<Database>>,
    endpoint_id: String,
    model_id: String,
    context_length: Option<u32>,
    tool_capability: Option<String>,
    system_prompt_suffix: Option<String>,
) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO local_model_overrides (endpoint_id, model_id, context_length, tool_capability, system_prompt_suffix) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(endpoint_id, model_id) DO UPDATE SET \
         context_length = COALESCE(?3, context_length), \
         tool_capability = COALESCE(?4, tool_capability), \
         system_prompt_suffix = COALESCE(?5, system_prompt_suffix)",
        rusqlite::params![
            endpoint_id,
            model_id,
            context_length,
            tool_capability,
            system_prompt_suffix,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}
