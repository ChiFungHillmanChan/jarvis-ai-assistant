import { invoke } from "@tauri-apps/api/core";
import type { Task, ChatMessage, DashboardData, Settings, EmailSummary, EmailStats, CalendarEventView, CronJobView, CronRunView, NotionPageView, GitHubItemView, GitHubStats, VoiceState, VoiceSettings, EmailRule, BriefingResult } from "./types";

export async function getTasks(): Promise<Task[]> {
  return invoke("get_tasks");
}
export async function createTask(title: string, description: string | null, deadline: string | null, priority: number): Promise<number> {
  return invoke("create_task", { title, description, deadline, priority });
}
export async function updateTask(id: number, status: string): Promise<void> {
  return invoke("update_task", { id, status });
}
export async function sendMessage(message: string): Promise<ChatMessage> {
  return invoke("send_message", { message });
}
export async function getConversations(): Promise<ChatMessage[]> {
  return invoke("get_conversations");
}
export async function getDashboardData(): Promise<DashboardData> {
  return invoke("get_dashboard_data");
}
export async function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}
export async function updateSetting(key: string, value: string): Promise<void> {
  return invoke("update_setting", { key, value });
}

// Email
export async function getEmails(limit?: number): Promise<EmailSummary[]> { return invoke("get_emails", { limit }); }
export async function syncEmails(): Promise<string> { return invoke("sync_emails"); }
export async function archiveEmail(gmailId: string): Promise<void> { return invoke("archive_email", { gmail_id: gmailId }); }
export async function getEmailStats(): Promise<EmailStats> { return invoke("get_email_stats"); }

// Calendar
export async function getEvents(days?: number): Promise<CalendarEventView[]> { return invoke("get_events", { days }); }
export async function syncCalendar(): Promise<string> { return invoke("sync_calendar"); }
export async function createEvent(summary: string, start: string, end: string, description?: string): Promise<string> { return invoke("create_event", { summary, start, end, description }); }
export async function getTodaysEvents(): Promise<CalendarEventView[]> { return invoke("get_todays_events"); }

// Cron
export async function getCronJobs(): Promise<CronJobView[]> { return invoke("get_cron_jobs"); }
export async function getCronRuns(jobId: number, limit?: number): Promise<CronRunView[]> { return invoke("get_cron_runs", { job_id: jobId, limit }); }

// Google Auth
export async function googleConnect(): Promise<string> { return invoke("google_connect"); }
export async function googleStatus(): Promise<boolean> { return invoke("google_status"); }

// Notion
export async function getNotionPages(limit?: number): Promise<NotionPageView[]> { return invoke("get_notion_pages", { limit }); }
export async function syncNotion(): Promise<string> { return invoke("sync_notion"); }
export async function saveNotionToken(token: string): Promise<void> { return invoke("save_notion_token", { token }); }
export async function getNotionStats(): Promise<number> { return invoke("get_notion_stats"); }

// GitHub
export async function getGitHubItems(itemType?: string): Promise<GitHubItemView[]> { return invoke("get_github_items", { item_type: itemType }); }
export async function syncGitHub(): Promise<string> { return invoke("sync_github"); }
export async function saveGitHubToken(token: string): Promise<void> { return invoke("save_github_token", { token }); }
export async function getGitHubStats(): Promise<GitHubStats> { return invoke("get_github_stats"); }

// Voice
export async function startListening(): Promise<string> { return invoke("start_listening"); }
export async function stopListening(): Promise<string> { return invoke("stop_listening"); }
export async function getVoiceState(): Promise<VoiceState> { return invoke("get_voice_state"); }
export async function toggleMute(): Promise<boolean> { return invoke("toggle_mute"); }
export async function getVoiceSettings(): Promise<VoiceSettings> { return invoke("get_voice_settings"); }
export async function setVoiceSetting(key: string, value: string): Promise<void> { return invoke("set_voice_setting", { key, value }); }
export async function listTtsVoices(): Promise<string[]> { return invoke("list_tts_voices"); }

// Email Rules
export async function getSuggestedRules(): Promise<EmailRule[]> { return invoke("get_suggested_rules"); }
export async function acceptEmailRule(ruleId: number): Promise<void> { return invoke("accept_email_rule", { rule_id: ruleId }); }
export async function dismissEmailRule(ruleId: number): Promise<void> { return invoke("dismiss_email_rule", { rule_id: ruleId }); }
export async function getActiveRules(): Promise<EmailRule[]> { return invoke("get_active_rules"); }

// Custom Cron
export async function createCustomCron(description: string): Promise<CronJobView> { return invoke("create_custom_cron", { description }); }
export async function deleteCronJob(jobId: number): Promise<void> { return invoke("delete_cron_job", { job_id: jobId }); }
export async function toggleCronJob(jobId: number): Promise<string> { return invoke("toggle_cron_job", { job_id: jobId }); }

// Assistant
export async function getBriefing(): Promise<BriefingResult> { return invoke("get_briefing"); }
export async function speakBriefing(): Promise<BriefingResult> { return invoke("speak_briefing"); }
export async function askJarvis(question: string): Promise<string> { return invoke("ask_jarvis", { question }); }
export async function searchConversations(query: string): Promise<string> { return invoke("search_conversations", { query }); }
