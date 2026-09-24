use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailAccount {
    pub id: String,
    pub email: String,
    pub connected: bool,
    pub last_synced: Option<String>,
    pub next_page_token: Option<String>,
    pub message_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailMessage {
    pub id: i64,
    pub account_id: String,
    pub account_email: String,
    pub provider_id: String,
    pub thread_id: Option<String>,
    pub subject: String,
    pub sender: String,
    pub snippet: String,
    pub body: Option<String>,
    pub is_read: bool,
    pub is_inbox: bool,
    pub received_at: String,
    /// None: saved before V26 recorded attachments, so unknown rather than empty.
    pub attachments: Option<Vec<MailAttachment>>,
}

/// A file attached to a message. Metadata only: the file stays with the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailAttachment {
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailGroup {
    pub id: i64,
    pub name: String,
    pub instructions: String,
    pub message_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailMemory {
    pub id: i64,
    pub group_id: i64,
    pub content: String,
    pub kind: String,
    pub source_message_id: Option<i64>,
    pub expires_at: Option<String>,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailMemoryInput {
    pub id: Option<i64>,
    pub group_id: i64,
    pub content: String,
    pub kind: String,
    pub source_message_id: Option<i64>,
    pub expires_at: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailSettings {
    pub processing_mode: String,
    pub local_model: String,
    pub bedrock_model: String,
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
}

fn default_openai_model() -> String {
    "gpt-5.6-luna".into()
}

#[derive(Serialize)]
pub struct MailWorkspace {
    pub accounts: Vec<MailAccount>,
    pub groups: Vec<MailGroup>,
    pub settings: MailSettings,
}

#[derive(Serialize)]
pub struct MailPage {
    pub messages: Vec<MailMessage>,
    pub total: i64,
}

#[derive(Serialize)]
pub struct MailSyncResult {
    pub saved: usize,
    pub has_more: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MailContext {
    pub group: MailGroup,
    pub messages: Vec<MailMessage>,
    pub memories: Vec<MailMemory>,
    pub settings: MailSettings,
}

#[derive(Debug, Serialize)]
pub struct MailAnalysis {
    pub text: String,
    pub mode: String,
    pub model: String,
    pub source_ids: Vec<i64>,
    pub omitted_messages: usize,
}
