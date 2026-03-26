pub mod claude;
pub mod local;
pub mod openai;
pub mod tools;

use crate::voice::tts::TtsCommand;
use local::backend::{BackendType, LocalEndpoint};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderChainEntry {
    pub position: i32,
    pub provider_type: String, // "claude", "openai", "local"
    pub endpoint_id: Option<String>,
    pub model_id: Option<String>,
    pub enabled: bool,
}

#[derive(Clone)]
pub struct AiRouter {
    claude_key: Option<String>,
    openai_key: Option<String>,
    provider_chain: Vec<ProviderChainEntry>,
    local_endpoints: Vec<LocalEndpoint>,
}

impl AiRouter {
    pub fn new(
        claude_key: Option<String>,
        openai_key: Option<String>,
        provider_setting: &str,
    ) -> Self {
        // Build default chain from legacy setting for backward compatibility
        let chain = match provider_setting {
            "openai_primary" => vec![
                ProviderChainEntry {
                    position: 0,
                    provider_type: "openai".into(),
                    endpoint_id: None,
                    model_id: None,
                    enabled: true,
                },
                ProviderChainEntry {
                    position: 1,
                    provider_type: "claude".into(),
                    endpoint_id: None,
                    model_id: None,
                    enabled: true,
                },
            ],
            "claude_only" => vec![ProviderChainEntry {
                position: 0,
                provider_type: "claude".into(),
                endpoint_id: None,
                model_id: None,
                enabled: true,
            }],
            "openai_only" => vec![ProviderChainEntry {
                position: 0,
                provider_type: "openai".into(),
                endpoint_id: None,
                model_id: None,
                enabled: true,
            }],
            _ => vec![
                // claude_primary (default)
                ProviderChainEntry {
                    position: 0,
                    provider_type: "claude".into(),
                    endpoint_id: None,
                    model_id: None,
                    enabled: true,
                },
                ProviderChainEntry {
                    position: 1,
                    provider_type: "openai".into(),
                    endpoint_id: None,
                    model_id: None,
                    enabled: true,
                },
            ],
        };

        AiRouter {
            claude_key,
            openai_key,
            provider_chain: chain,
            local_endpoints: Vec::new(),
        }
    }

    /// Load provider chain from database (call after construction)
    pub fn load_from_db(&mut self, db: &crate::db::Database) {
        // Load local endpoints
        if let Ok(conn) = db.conn.lock() {
            let mut stmt = conn
                .prepare(
                    "SELECT id, name, url, backend_type, api_key, use_tls, \
                     connection_timeout_ms, keep_alive_minutes, is_active, \
                     last_health_check, last_health_status FROM local_endpoints WHERE is_active = 1",
                )
                .ok();

            if let Some(ref mut stmt) = stmt {
                let endpoints: Vec<LocalEndpoint> = stmt
                    .query_map([], |row| {
                        Ok(LocalEndpoint {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            url: row.get(2)?,
                            backend_type: BackendType::from_str(
                                &row.get::<_, String>(3).unwrap_or_default(),
                            ),
                            api_key: row.get(4)?,
                            use_tls: row.get::<_, i32>(5).unwrap_or(0) != 0,
                            connection_timeout_ms: row.get::<_, u32>(6).unwrap_or(5000),
                            keep_alive_minutes: row.get::<_, u32>(7).unwrap_or(30),
                            is_active: row.get::<_, i32>(8).unwrap_or(1) != 0,
                            last_health_check: row.get(9)?,
                            last_health_status: row
                                .get::<_, Option<i32>>(10)?
                                .map(|v| v != 0),
                        })
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();

                self.local_endpoints = endpoints;
            }

            // Load provider chain
            let mut chain_stmt = conn
                .prepare(
                    "SELECT position, provider_type, endpoint_id, model_id, enabled \
                     FROM provider_chain ORDER BY position ASC",
                )
                .ok();

            if let Some(ref mut stmt) = chain_stmt {
                let chain: Vec<ProviderChainEntry> = stmt
                    .query_map([], |row| {
                        Ok(ProviderChainEntry {
                            position: row.get(0)?,
                            provider_type: row.get(1)?,
                            endpoint_id: row.get(2)?,
                            model_id: row.get(3)?,
                            enabled: row.get::<_, i32>(4).unwrap_or(1) != 0,
                        })
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();

                if !chain.is_empty() {
                    self.provider_chain = chain;
                }
            }
        }
    }

    pub fn provider_chain(&self) -> &[ProviderChainEntry] {
        &self.provider_chain
    }

    pub fn local_endpoints(&self) -> &[LocalEndpoint] {
        &self.local_endpoints
    }

    pub fn set_provider_chain(&mut self, chain: Vec<ProviderChainEntry>) {
        self.provider_chain = chain;
    }

    pub fn set_local_endpoints(&mut self, endpoints: Vec<LocalEndpoint>) {
        self.local_endpoints = endpoints;
    }

    pub fn find_endpoint(&self, endpoint_id: &str) -> Option<&LocalEndpoint> {
        self.local_endpoints.iter().find(|e| e.id == endpoint_id)
    }

    pub async fn send(
        &self,
        messages: Vec<(String, String)>,
        db: &crate::db::Database,
        google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
        app_handle: &tauri::AppHandle,
        tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
    ) -> Result<String, String> {
        let mut errors: Vec<String> = Vec::new();

        for entry in &self.provider_chain {
            if !entry.enabled {
                continue;
            }

            let provider_name = match entry.provider_type.as_str() {
                "local" => {
                    let model = entry.model_id.as_deref().unwrap_or("unknown");
                    format!("{} (local)", model)
                }
                other => other.to_string(),
            };

            let _ = app_handle.emit(
                "chat-provider",
                json!({
                    "provider": &provider_name,
                    "status": "trying"
                }),
            );

            let result = match entry.provider_type.as_str() {
                "claude" => {
                    if let Some(ref key) = self.claude_key {
                        claude::send(
                            key,
                            messages.clone(),
                            db,
                            google_auth,
                            app_handle,
                            tts_tx.clone(),
                        )
                        .await
                        .map_err(|e| e.to_string())
                    } else {
                        Err("No Claude API key configured".to_string())
                    }
                }
                "openai" => {
                    if let Some(ref key) = self.openai_key {
                        openai::send(
                            key,
                            messages.clone(),
                            db,
                            google_auth,
                            app_handle,
                            tts_tx.clone(),
                        )
                        .await
                        .map_err(|e| e.to_string())
                    } else {
                        Err("No OpenAI API key configured".to_string())
                    }
                }
                "local" => {
                    let endpoint_id = match &entry.endpoint_id {
                        Some(id) => id,
                        None => {
                            errors.push("Local provider missing endpoint_id".to_string());
                            continue;
                        }
                    };
                    let model_id = match &entry.model_id {
                        Some(id) => id,
                        None => {
                            errors.push("Local provider missing model_id".to_string());
                            continue;
                        }
                    };
                    let endpoint = match self.find_endpoint(endpoint_id) {
                        Some(ep) => ep,
                        None => {
                            errors.push(format!(
                                "Endpoint '{}' not found or inactive",
                                endpoint_id
                            ));
                            continue;
                        }
                    };

                    // Get model overrides from DB
                    let (context_length, tool_override) = {
                        let conn = db.conn.lock().map_err(|e| e.to_string())?;
                        let ctx_len: u32 = conn
                            .query_row(
                                "SELECT context_length FROM local_model_overrides WHERE endpoint_id = ?1 AND model_id = ?2",
                                rusqlite::params![endpoint_id, model_id],
                                |row| row.get(0),
                            )
                            .unwrap_or(4096);
                        let tool_ov: Option<String> = conn
                            .query_row(
                                "SELECT tool_capability FROM local_model_overrides WHERE endpoint_id = ?1 AND model_id = ?2",
                                rusqlite::params![endpoint_id, model_id],
                                |row| row.get(0),
                            )
                            .ok();
                        (ctx_len, tool_ov)
                    };

                    // Detect tool capability
                    let backend = local::get_backend(&endpoint.backend_type);
                    let detected_cap = backend
                        .detect_tool_capability(&endpoint.url, model_id)
                        .await;

                    local::send_local(
                        endpoint,
                        model_id,
                        messages.clone(),
                        detected_cap,
                        context_length,
                        tool_override.as_deref(),
                        db,
                        google_auth,
                        app_handle,
                        tts_tx.clone(),
                    )
                    .await
                    .map_err(|e| e.to_string())
                }
                unknown => Err(format!("Unknown provider type: {}", unknown)),
            };

            match result {
                Ok(response) => {
                    let _ = app_handle.emit(
                        "chat-provider",
                        json!({
                            "provider": &provider_name,
                            "status": "active"
                        }),
                    );
                    return Ok(response);
                }
                Err(e) => {
                    log::warn!("{} failed: {}", provider_name, e);
                    let _ = app_handle.emit(
                        "chat-provider",
                        json!({
                            "provider": &provider_name,
                            "status": "failed"
                        }),
                    );
                    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
                    let _ = app_handle.emit(
                        "chat-status",
                        json!({
                            "status": "Retrying with next provider...",
                            "phase": "thinking"
                        }),
                    );
                    errors.push(format!("{}: {}", provider_name, e));
                }
            }
        }

        Err(format!(
            "All providers failed: {}",
            errors.join("; ")
        ))
    }
}
