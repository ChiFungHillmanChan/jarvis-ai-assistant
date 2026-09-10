import { invoke } from "@tauri-apps/api/core";

export interface MailAccount { id: string; email: string; connected: boolean; last_synced: string | null; next_page_token: string | null; message_count: number }
export interface MailMessage { id: number; account_id: string; account_email: string; provider_id: string; thread_id: string | null; subject: string; sender: string; snippet: string; body: string | null; is_read: boolean; is_inbox: boolean; received_at: string }
export interface MailGroup { id: number; name: string; instructions: string; message_count: number }
export interface MailMemory { id: number; group_id: number; content: string; kind: string; source_message_id: number | null; expires_at: string | null; status: string; updated_at: string }
export interface MailMemoryInput { id: number | null; group_id: number; content: string; kind: string; source_message_id: number | null; expires_at: string | null; status: string }
export interface MailSettings { processing_mode: string; local_model: string; bedrock_model: string; openai_model: string }
export interface MailWorkspace { accounts: MailAccount[]; groups: MailGroup[]; settings: MailSettings }
export interface MailPage { messages: MailMessage[]; total: number }
export interface MailSyncResult { saved: number; has_more: boolean; warning: string | null }
export interface MailAnalysis { text: string; mode: string; model: string; source_ids: number[]; omitted_messages: number }
export const mailWorkspace = () => invoke<MailWorkspace>("mail_workspace");
export const mailConnectAccount = () => invoke<MailAccount>("mail_connect_account");
export const mailDisconnectAccount = (accountId: string) => invoke<void>("mail_disconnect_account", { accountId });
export const mailRemoveAccount = (accountId: string) => invoke<void>("mail_remove_account", { accountId });
export const mailSyncAccount = (accountId: string, older: boolean) => invoke<MailSyncResult>("mail_sync_account", { accountId, older });
export const mailRestartSync = (accountId: string) => invoke<MailSyncResult>("mail_restart_sync", { accountId });
export const mailListMessages = (args: { accountId?: string; groupId?: number; query?: string; offset?: number }) => invoke<MailPage>("mail_list_messages", args);
export const mailGetMessage = (messageId: number) => invoke<MailMessage>("mail_get_message", { messageId });
export const mailSaveGroup = (name: string, instructions: string, groupId?: number) => invoke<MailGroup>("mail_save_group", { groupId, name, instructions });
export const mailDeleteGroup = (groupId: number) => invoke<void>("mail_delete_group", { groupId });
export const mailSetGroupMembers = (groupId: number, messageIds: number[], add: boolean) => invoke<void>("mail_set_group_members", { groupId, messageIds, add });
export const mailListMemories = (groupId: number) => invoke<MailMemory[]>("mail_list_memories", { groupId });
export const mailSaveMemory = (input: MailMemoryInput) => invoke<MailMemory>("mail_save_memory", { input });
export const mailDeleteMemory = (memoryId: number) => invoke<void>("mail_delete_memory", { memoryId });
export const mailSaveSettings = (input: MailSettings) => invoke<void>("mail_save_settings", { input });
export const mailAnalyzeGroup = (groupId: number) => invoke<MailAnalysis>("mail_analyze_group", { groupId });
