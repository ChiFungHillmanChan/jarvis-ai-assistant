import { invoke } from "@tauri-apps/api/core";

/** `calendar`: JARVIS may read this account's calendar (it is then an additional Google account in Settings). */
export interface MailAccount { id: string; email: string; connected: boolean; last_synced: string | null; next_page_token: string | null; message_count: number; calendar: boolean }
export interface MailMessage { id: number; account_id: string; account_email: string; provider_id: string; thread_id: string | null; subject: string; sender: string; snippet: string; body: string | null; is_read: boolean; is_inbox: boolean; received_at: string; attachments: MailAttachment[] | null; triage: MailTriage | null; next_due: string | null }
/** null: synced before attachments were recorded, so unknown until a rescan. */
export interface MailAttachment { filename: string; mime_type: string; size: number }
export interface MailGroup { id: number; name: string; instructions: string; message_count: number }
export interface MailMemory { id: number; group_id: number; content: string; kind: string; source_message_id: number | null; expires_at: string | null; status: string; updated_at: string }
export interface MailMemoryInput { id: number | null; group_id: number; content: string; kind: string; source_message_id: number | null; expires_at: string | null; status: string }
export interface MailSettings { processing_mode: string; local_model: string; bedrock_model: string; openai_model: string }
export interface MailWorkspace { accounts: MailAccount[]; groups: MailGroup[]; settings: MailSettings; triage: MailTriageStatus }
/** Jev's raw answers plus the policy decided in code (src-tauri/src/mail/triage.rs). */
export interface MailTriage { category: MailCategory; category_confidence: number; needs_reply: number; action_required: number; has_date: number; importance: number; sent_by_owner: boolean; attention: boolean; summary: string | null; decided_by: 'jev' | 'luna' | 'ai' }
export type MailCategory = 'work' | 'jobs' | 'finance' | 'schedule' | 'account' | 'personal' | 'promotions';
export type MailView = 'attention' | 'due' | 'untriaged' | MailCategory;
export interface MailTriageStatus { enabled: boolean; typesafe_key: boolean; openai_key: boolean; chosen_ai: string | null; untriaged: number; attention: number; open_items: number; categories: [MailCategory, number][] }
export interface MailTriageRun { triaged: number; summarized: number; items: number; failed: number; remaining: number; jev_input_tokens: number; luna_input_tokens: number; luna_output_tokens: number; cost_usd: number; warning: string | null }
export interface MailItem { id: number; message_id: number; kind: 'task' | 'deadline' | 'event' | 'reply'; title: string; due_date: string | null; due_time: string | null; end_time: string | null; location: string | null; evidence: string; evidence_verified: boolean; status: 'open' | 'done' | 'dismissed'; remind_at: string | null; reminded_at: string | null; calendar_event_id: string | null; subject: string; sender: string; superseded_by: number | null }
export interface MailPage { messages: MailMessage[]; total: number }
export interface MailSyncResult { saved: number; has_more: boolean; warning: string | null }
export interface MailAnalysis { text: string; mode: string; model: string; source_ids: number[]; omitted_messages: number; input_tokens: number; output_tokens: number; cached_tokens: number }
export const mailWorkspace = (accountId?: string) => invoke<MailWorkspace>("mail_workspace", { accountId: accountId || undefined });
export const mailConnectAccount = () => invoke<MailAccount>("mail_connect_account");
export const mailCancelConnect = () => invoke<void>("mail_cancel_connect");
export const mailDisconnectAccount = (accountId: string) => invoke<void>("mail_disconnect_account", { accountId });
export const mailRemoveAccount = (accountId: string) => invoke<void>("mail_remove_account", { accountId });
export const mailSyncAccount = (accountId: string, older: boolean) => invoke<MailSyncResult>("mail_sync_account", { accountId, older });
export const mailRestartSync = (accountId: string) => invoke<MailSyncResult>("mail_restart_sync", { accountId });
export const mailListMessages = (args: { accountId?: string; groupId?: number; query?: string; offset?: number; view?: MailView }) => invoke<MailPage>("mail_list_messages", args);
export const mailGetMessage = (messageId: number) => invoke<MailMessage>("mail_get_message", { messageId });
export const mailSaveGroup = (name: string, instructions: string, groupId?: number) => invoke<MailGroup>("mail_save_group", { groupId, name, instructions });
export const mailDeleteGroup = (groupId: number) => invoke<void>("mail_delete_group", { groupId });
export const mailSetGroupMembers = (groupId: number, messageIds: number[], add: boolean) => invoke<void>("mail_set_group_members", { groupId, messageIds, add });
export const mailListMemories = (groupId: number) => invoke<MailMemory[]>("mail_list_memories", { groupId });
export const mailSaveMemory = (input: MailMemoryInput) => invoke<MailMemory>("mail_save_memory", { input });
export const mailDeleteMemory = (memoryId: number) => invoke<void>("mail_delete_memory", { memoryId });
export const mailSaveSettings = (input: MailSettings) => invoke<void>("mail_save_settings", { input });
export const mailAnalyzeGroup = (groupId: number) => invoke<MailAnalysis>("mail_analyze_group", { groupId });
export const mailTriageRun = (language: string) => invoke<MailTriageRun>("mail_triage_run", { language });
export const mailSetAutoTriage = (enabled: boolean) => invoke<void>("mail_set_auto_triage", { enabled });
export const mailSetAttention = (messageId: number, attention: boolean) => invoke<void>("mail_set_attention", { messageId, attention });
export const mailListItems = (messageId?: number, includeClosed = false) => invoke<MailItem[]>("mail_list_items", { messageId, includeClosed });
export const mailUpdateItem = (itemId: number, status: MailItem['status'], remindAt: string | null) => invoke<MailItem>("mail_update_item", { itemId, status, remindAt });
export const mailItemToCalendar = (itemId: number) => invoke<MailItem>("mail_item_to_calendar", { itemId });
export const saveTypeSafeKey = (value: string) => invoke<void>("set_api_key", { provider: "typesafe", value });
