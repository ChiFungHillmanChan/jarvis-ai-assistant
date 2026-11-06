use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailAccount {
    pub id: String,
    pub email: String,
    pub connected: bool,
    pub last_synced: Option<String>,
    pub next_page_token: Option<String>,
    pub message_count: i64,
    /// Whether JARVIS may read this account's calendar (mail::additional).
    pub calendar: bool,
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
    /// None until the message has been triaged.
    #[serde(default)]
    pub triage: Option<MailTriage>,
    /// Earliest open follow-up date extracted from this message.
    #[serde(default)]
    pub next_due: Option<String>,
}

/// Categories a message can be triaged into. The order is the display order.
pub const MAIL_CATEGORIES: [&str; 7] =
    ["work", "jobs", "finance", "schedule", "account", "personal", "promotions"];

/// One message's triage. The probabilities are Jev's raw answers; `attention`
/// and `sent_by_owner` are decided in code (mail/triage.rs), not by a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailTriage {
    pub category: String,
    pub category_confidence: f64,
    pub needs_reply: f64,
    pub action_required: f64,
    pub has_date: f64,
    pub importance: f64,
    pub sent_by_owner: bool,
    pub attention: bool,
    pub summary: Option<String>,
    /// "jev", or "luna" when gpt-6-luna overrode an uncertain category.
    pub decided_by: String,
}

/// A dated follow-up extracted from a message. Local only until the user
/// adds it to Google Calendar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailItem {
    pub id: i64,
    pub message_id: i64,
    pub kind: String,
    pub title: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub end_time: Option<String>,
    pub location: Option<String>,
    pub evidence: String,
    pub evidence_verified: bool,
    pub status: String,
    pub remind_at: Option<String>,
    pub reminded_at: Option<String>,
    pub calendar_event_id: Option<String>,
    pub subject: String,
    pub sender: String,
    /// The newer item that replaced this one when a later email corrected it.
    pub superseded_by: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailTriageStatus {
    pub enabled: bool,
    pub typesafe_key: bool,
    pub openai_key: bool,
    /// Name of the AI chosen in setup, which sorts mail without TypeSafe.
    pub chosen_ai: Option<String>,
    pub untriaged: i64,
    pub attention: i64,
    pub open_items: i64,
    /// (category, message count), in MAIL_CATEGORIES order.
    pub categories: Vec<(String, i64)>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MailTriageRun {
    pub triaged: usize,
    pub summarized: usize,
    pub items: usize,
    pub failed: usize,
    pub remaining: i64,
    pub jev_input_tokens: i64,
    pub luna_input_tokens: i64,
    pub luna_output_tokens: i64,
    /// None when only a local model ran: it is free, so show the tokens.
    pub cost_usd: Option<f64>,
    pub warning: Option<String>,
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
    crate::ai::models::OPENAI_DEFAULT.into()
}

#[derive(Serialize)]
pub struct MailWorkspace {
    pub accounts: Vec<MailAccount>,
    pub groups: Vec<MailGroup>,
    pub settings: MailSettings,
    pub triage: MailTriageStatus,
}

#[derive(Serialize)]
pub struct MailPage {
    pub messages: Vec<MailMessage>,
    /// None for a search: counting every match would read each body twice.
    pub total: Option<i64>,
    pub has_more: bool,
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
    /// Reported by OpenAI; zero for local and Bedrock analysis.
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
}
