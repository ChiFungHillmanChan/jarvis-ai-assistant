pub mod claude;
pub mod openai;
pub mod tools;

use crate::voice::tts::TtsCommand;
use serde_json::json;
use tauri::Emitter;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    ClaudePrimary,
    OpenAIPrimary,
    ClaudeOnly,
    OpenAIOnly,
}

#[derive(Clone)]
pub struct AiRouter {
    claude_key: Option<String>,
    openai_key: Option<String>,
    provider: AiProvider,
}

impl AiRouter {
    pub fn new(
        claude_key: Option<String>,
        openai_key: Option<String>,
        provider_setting: &str,
    ) -> Self {
        let provider = match provider_setting {
            "claude_primary" => AiProvider::ClaudePrimary,
            "openai_primary" => AiProvider::OpenAIPrimary,
            "claude_only" => AiProvider::ClaudeOnly,
            "openai_only" => AiProvider::OpenAIOnly,
            _ => AiProvider::ClaudePrimary,
        };
        AiRouter {
            claude_key,
            openai_key,
            provider,
        }
    }

    pub fn provider(&self) -> AiProvider {
        self.provider
    }

    pub async fn send(
        &self,
        messages: Vec<(String, String)>,
        db: &crate::db::Database,
        google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
        app_handle: &tauri::AppHandle,
        tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
    ) -> Result<String, String> {
        match self.provider {
            AiProvider::ClaudePrimary => {
                if let Some(ref key) = self.claude_key {
                    match claude::send(
                        key,
                        messages.clone(),
                        db,
                        google_auth,
                        app_handle,
                        tts_tx.clone(),
                    )
                    .await
                    {
                        Ok(response) => return Ok(response),
                        Err(e) => {
                            log::warn!("Claude failed, trying OpenAI fallback: {}", e);
                            let _ =
                                app_handle.emit("chat-token", json!({"token": "", "done": true}));
                            let _ = app_handle.emit(
                                "chat-status",
                                json!({
                                    "status": "Retrying with fallback model...",
                                    "phase": "thinking"
                                }),
                            );
                        }
                    }
                }
                if let Some(ref key) = self.openai_key {
                    openai::send(key, messages, db, google_auth, app_handle, tts_tx)
                        .await
                        .map_err(|e| format!("Both AI providers failed. OpenAI: {}", e))
                } else {
                    Err("Claude failed and no OpenAI key configured".to_string())
                }
            }
            AiProvider::OpenAIPrimary => {
                if let Some(ref key) = self.openai_key {
                    match openai::send(
                        key,
                        messages.clone(),
                        db,
                        google_auth,
                        app_handle,
                        tts_tx.clone(),
                    )
                    .await
                    {
                        Ok(response) => return Ok(response),
                        Err(e) => {
                            log::warn!("OpenAI failed, trying Claude fallback: {}", e);
                            let _ =
                                app_handle.emit("chat-token", json!({"token": "", "done": true}));
                            let _ = app_handle.emit(
                                "chat-status",
                                json!({
                                    "status": "Retrying with fallback model...",
                                    "phase": "thinking"
                                }),
                            );
                        }
                    }
                }
                if let Some(ref key) = self.claude_key {
                    claude::send(key, messages, db, google_auth, app_handle, tts_tx)
                        .await
                        .map_err(|e| format!("Both AI providers failed. Claude: {}", e))
                } else {
                    Err("OpenAI failed and no Claude key configured".to_string())
                }
            }
            AiProvider::ClaudeOnly => {
                let key = self
                    .claude_key
                    .as_ref()
                    .ok_or("No Claude API key configured")?;
                claude::send(key, messages, db, google_auth, app_handle, tts_tx)
                    .await
                    .map_err(|e| format!("Claude error: {}", e))
            }
            AiProvider::OpenAIOnly => {
                let key = self
                    .openai_key
                    .as_ref()
                    .ok_or("No OpenAI API key configured")?;
                openai::send(key, messages, db, google_auth, app_handle, tts_tx)
                    .await
                    .map_err(|e| format!("OpenAI error: {}", e))
            }
        }
    }
}
