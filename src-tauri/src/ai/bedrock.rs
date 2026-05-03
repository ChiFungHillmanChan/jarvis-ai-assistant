use super::tools;
use crate::voice::tts::TtsCommand;
use aws_config::BehaviorVersion;
use aws_sdk_bedrockruntime::config::Region;
use aws_sdk_bedrockruntime::types::{
    ContentBlock, ConversationRole, InferenceConfiguration, Message, StopReason,
    SystemContentBlock, ToolConfiguration, ToolInputSchema, ToolResultBlock,
    ToolResultContentBlock, ToolResultStatus, ToolSpecification,
};
use aws_smithy_types::{Document, Number};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tauri::Emitter;

pub const DEFAULT_MODEL: &str = "anthropic.claude-sonnet-4-6";

pub fn default_model() -> String {
    std::env::var("AWS_BEDROCK_MODEL_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

pub async fn send(
    model: &str,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    app_handle: &tauri::AppHandle,
    tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut config_loader = aws_config::defaults(BehaviorVersion::latest());
    if let Some(region) = bedrock_region() {
        config_loader = config_loader.region(region);
    }
    let sdk_config = config_loader.load().await;
    let client = aws_sdk_bedrockruntime::Client::new(&sdk_config);
    let tool_config = build_tool_config()?;

    let mut bedrock_messages = build_messages(messages)?;
    let system = SystemContentBlock::Text(tools::build_system_prompt(db));
    let inference_config = InferenceConfiguration::builder()
        .max_tokens(4096)
        .temperature(0.2)
        .build();

    let max_iterations = 5;
    for _ in 0..max_iterations {
        let response = client
            .converse()
            .model_id(model)
            .set_messages(Some(bedrock_messages.clone()))
            .system(system.clone())
            .tool_config(tool_config.clone())
            .inference_config(inference_config.clone())
            .send()
            .await?;

        let output_message = match response.output() {
            Some(aws_sdk_bedrockruntime::types::ConverseOutput::Message(message)) => {
                message.clone()
            }
            _ => return Err("Bedrock returned no message output".into()),
        };

        let text_parts = collect_text(&output_message);
        let tool_uses = collect_tool_uses(&output_message);

        if !tool_uses.is_empty() || response.stop_reason() == &StopReason::ToolUse {
            bedrock_messages.push(output_message);

            let mut executed_tools: HashSet<String> = HashSet::new();
            let mut narrated = false;
            let mut result_blocks: Vec<ContentBlock> = Vec::new();

            for tool_use in &tool_uses {
                let name = tool_use.name();
                let content = if executed_tools.contains(name) {
                    log::info!("JARVIS skipping duplicate tool call: {}", name);
                    "Skipped: already executed this tool in this batch.".to_string()
                } else {
                    executed_tools.insert(name.to_string());
                    let args_value = document_to_json(tool_use.input());
                    let args_str = serde_json::to_string(&args_value).unwrap_or_default();
                    log::info!("JARVIS tool call: {}({})", name, args_str);

                    let label = tools::tool_status_label(name);
                    let _ = app_handle.emit(
                        "chat-status",
                        json!({
                            "status": format!("Running: {}", label),
                            "phase": "acting"
                        }),
                    );
                    let _ = app_handle.emit("chat-tool-call", json!({"tool_name": name}));

                    if !narrated {
                        if let Some(ref tx) = tts_tx {
                            let narration = tools::tool_voice_narration(name);
                            let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                            narrated = true;
                        }
                    }

                    let result = tools::execute_tool(name, &args_str, db, google_auth).await;
                    log::info!("JARVIS tool result: {}", &result[..result.len().min(200)]);
                    result
                };

                let tool_result = ToolResultBlock::builder()
                    .tool_use_id(tool_use.tool_use_id())
                    .content(ToolResultContentBlock::Text(content))
                    .status(ToolResultStatus::Success)
                    .build()?;
                result_blocks.push(ContentBlock::ToolResult(tool_result));
            }

            bedrock_messages.push(
                Message::builder()
                    .role(ConversationRole::User)
                    .set_content(Some(result_blocks))
                    .build()?,
            );

            let _ = app_handle.emit(
                "chat-status",
                json!({
                    "status": "Reviewing tool results...",
                    "phase": "thinking"
                }),
            );
            continue;
        }

        let final_text = text_parts.join("");
        let _ = app_handle.emit(
            "chat-status",
            json!({
                "status": "Composing response...",
                "phase": "responding"
            }),
        );
        if !final_text.is_empty() {
            let _ = app_handle.emit("chat-token", json!({"token": final_text, "done": false}));
            if let Some(ref tx) = tts_tx {
                let _ = tx.try_send(TtsCommand::TextChunk(final_text.clone()));
            }
        }
        let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
        return Ok(final_text);
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    Ok("I've completed the actions.".into())
}

fn bedrock_region() -> Option<Region> {
    std::env::var("AWS_BEDROCK_REGION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(Region::new)
}

fn build_messages(
    messages: Vec<(String, String)>,
) -> Result<Vec<Message>, Box<dyn std::error::Error + Send + Sync>> {
    messages
        .into_iter()
        .map(|(role, content)| {
            let role = if role == "assistant" {
                ConversationRole::Assistant
            } else {
                ConversationRole::User
            };
            Ok(Message::builder()
                .role(role)
                .content(ContentBlock::Text(content))
                .build()?)
        })
        .collect()
}

fn build_tool_config() -> Result<ToolConfiguration, Box<dyn std::error::Error + Send + Sync>> {
    let tool_specs = tools::get_tool_definitions()
        .into_iter()
        .map(|tool| {
            let spec = ToolSpecification::builder()
                .name(tool.name)
                .description(tool.description)
                .input_schema(ToolInputSchema::Json(json_to_document(&tool.parameters)))
                .build()?;
            Ok(aws_sdk_bedrockruntime::types::Tool::ToolSpec(spec))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;

    Ok(ToolConfiguration::builder()
        .set_tools(Some(tool_specs))
        .build()?)
}

fn collect_text(message: &Message) -> Vec<String> {
    message
        .content()
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn collect_tool_uses(message: &Message) -> Vec<aws_sdk_bedrockruntime::types::ToolUseBlock> {
    message
        .content()
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse(tool_use) => Some(tool_use.clone()),
            _ => None,
        })
        .collect()
}

fn json_to_document(value: &serde_json::Value) -> Document {
    match value {
        serde_json::Value::Null => Document::Null,
        serde_json::Value::Bool(value) => Document::Bool(*value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_u64() {
                Document::from(value)
            } else if let Some(value) = value.as_i64() {
                Document::from(value)
            } else if let Some(value) = value.as_f64() {
                Document::from(value)
            } else {
                Document::Null
            }
        }
        serde_json::Value::String(value) => Document::String(value.clone()),
        serde_json::Value::Array(values) => {
            Document::Array(values.iter().map(json_to_document).collect())
        }
        serde_json::Value::Object(values) => Document::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), json_to_document(value)))
                .collect::<HashMap<_, _>>(),
        ),
    }
}

fn document_to_json(document: &Document) -> serde_json::Value {
    match document {
        Document::Null => serde_json::Value::Null,
        Document::Bool(value) => serde_json::Value::Bool(*value),
        Document::String(value) => serde_json::Value::String(value.clone()),
        Document::Array(values) => {
            serde_json::Value::Array(values.iter().map(document_to_json).collect())
        }
        Document::Object(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), document_to_json(value)))
                .collect(),
        ),
        Document::Number(Number::PosInt(value)) => serde_json::Value::Number((*value).into()),
        Document::Number(Number::NegInt(value)) => serde_json::Value::Number((*value).into()),
        Document::Number(Number::Float(value)) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
    }
}
