import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { CheckoutPage, PaymentPage } from "./types";
import type { UiLocale } from "../i18n/catalog";
import type { MailAccount } from "./mail";
import { beginAccountSessionChange, finishAccountSessionChange } from "./account.ts";
import type { Task, ChatMessage, DashboardData, Settings, EmailSummary, EmailStats, CalendarEventView, CronJobView, CronRunView, NotionPageView, GitHubItemView, GitHubStats, VoiceState, VoiceSettings, WakeWordStatus, EmailRule, BriefingResult, LocalEndpoint, LocalModel, ProviderChainEntry, EndpointHealth, SystemCompatibility, PrerequisiteCheck, RecommendedModel, LaunchReadiness, BetaTesterInput, BetaTesterView, BetaFeedbackInput, AwsCredsStatus, StorageUsage, SweepReport, StorageScope, AccountStatus } from "./types";

// Tauri v2 commands default to rename_all = "camelCase", so a Rust parameter named
// `gmail_id` is looked up on the IPC boundary as `gmailId`. A required arg sent as
// snake_case is rejected ("missing required key"); an Option arg silently arrives as
// None. Normalising the top-level arg keys here makes that drift impossible to
// reintroduce. Shallow on purpose: nested values (structs, Vec<T>) are deserialized by
// serde and keep their snake_case field names.
function toCamelCaseKeys(args: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(args)) {
    out[key.replace(/_([a-z0-9])/g, (_m, c: string) => c.toUpperCase())] = args[key];
  }
  return out;
}

// Every IPC call in this file goes through here, never through tauriInvoke directly.
function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return tauriInvoke<T>(command, args ? toCamelCaseKeys(args) : undefined);
}

export async function getTasks(): Promise<Task[]> {
  return invoke("get_tasks");
}
export async function createTask(title: string, description: string | null, deadline: string | null, priority: number): Promise<number> {
  return invoke("create_task", { title, description, deadline, priority });
}
export async function updateTask(id: number, status: string): Promise<void> {
  return invoke("update_task", { id, status });
}
export async function sendMessage(message: string, workload?: "tools" | "premium", sessionId?: string, runId?: string): Promise<ChatMessage> {
  return invoke("send_message", { message, workload, sessionId, runId });
}
export async function getConversations(sessionId?: string): Promise<ChatMessage[]> {
  return invoke("get_conversations", { sessionId });
}
export async function listChatSessions(): Promise<{ id: string; title: string; updated_at: string }[]> {
  return invoke("list_chat_sessions");
}
export async function clearConversations(sessionId: string): Promise<void> {
  return invoke("clear_conversations", { sessionId });
}
export async function getDashboardData(): Promise<DashboardData> {
  return invoke("get_dashboard_data");
}

// Launch
export async function getLaunchReadiness(): Promise<LaunchReadiness> { return invoke("get_launch_readiness"); }
export async function saveBetaTester(input: BetaTesterInput): Promise<BetaTesterView> { return invoke("save_beta_tester", { input }); }
export async function submitBetaFeedback(input: BetaFeedbackInput): Promise<void> { return invoke("submit_beta_feedback", { input }); }

// Account (Cognito). Signing in is optional; it is required for checkout.
export async function accountStatus(refresh = false): Promise<AccountStatus> {
  return invoke("account_status", { refresh });
}
export async function accountSignIn(provider?: "google"): Promise<AccountStatus> {
  const revision = beginAccountSessionChange();
  try { return await invoke("account_sign_in", { provider: provider ?? null }); }
  finally { finishAccountSessionChange(revision); }
}
export async function accountSignOut(): Promise<void> {
  const revision = beginAccountSessionChange();
  try { await invoke("account_sign_out"); }
  finally { finishAccountSessionChange(revision); }
}

// Payments (backend set via JARVIS_PAYMENT_API).
// Requires a signed-in account; rejects with "not_signed_in" otherwise.
export async function paymentsStartCheckout(tier: "pro" | "team", locale: UiLocale = "en"): Promise<CheckoutPage> {
  return invoke("payments_start_checkout", { tier, locale });
}
export async function paymentsOpenPortal(locale: UiLocale = "en"): Promise<PaymentPage> {
  return invoke("payments_open_portal", { locale });
}
export async function paymentsReopenUrl(url: string, sessionGeneration: number): Promise<PaymentPage> {
  return invoke("payments_reopen_url", { url, sessionGeneration });
}
export async function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}
export async function updateSetting(key: string, value: string): Promise<void> {
  return invoke("update_setting", { key, value });
}
// API keys go to the macOS Keychain, never to user_preferences.
export async function setApiKey(provider: string, value: string): Promise<void> {
  return invoke("set_api_key", { provider, value });
}
export async function getApiKeyStatus(): Promise<Record<string, boolean>> {
  return invoke("get_api_key_status");
}

// Email
export async function getEmails(limit?: number): Promise<EmailSummary[]> { return invoke("get_emails", { limit }); }
export async function syncEmails(): Promise<string> { return invoke("sync_emails"); }
export async function archiveEmail(gmailId: string): Promise<void> { return invoke("archive_email", { gmailId }); }
export async function getEmailStats(): Promise<EmailStats> { return invoke("get_email_stats"); }

// Calendar
export async function getEvents(days?: number, dateFrom?: string, dateTo?: string): Promise<CalendarEventView[]> { return invoke("get_events", { days, dateFrom, dateTo }); }
export async function syncCalendar(dateFrom?: string, dateTo?: string): Promise<string> { return invoke("sync_calendar", dateFrom && dateTo ? { dateFrom, dateTo } : {}); }
export async function createEvent(summary: string, start: string, end: string, description?: string): Promise<string> { return invoke("create_event", { summary, start, end, description }); }
export async function getTodaysEvents(): Promise<CalendarEventView[]> { return invoke("get_todays_events"); }

// Cron
export async function getCronJobs(): Promise<CronJobView[]> { return invoke("get_cron_jobs"); }
export async function getCronRuns(jobId: number, limit?: number): Promise<CronRunView[]> { return invoke("get_cron_runs", { jobId, limit }); }
export interface CronInterpretation { name: string; schedule: string; action_type: string; description: string; }
export async function previewCustomCron(description: string): Promise<CronInterpretation> { return invoke("preview_custom_cron", { description }); }
export async function createReviewedCron(interpretation: CronInterpretation): Promise<CronJobView> { return invoke("create_reviewed_cron", { interpretation }); }
export async function setCronJobStatus(jobId: number, status: "active" | "paused"): Promise<string> { return invoke("set_cron_job_status", { jobId, status }); }

// Google Auth
// `force` is how the user switches accounts: the backend revokes the current
// grant and drops the local mirror before re-running the OAuth flow. Without it
// an already-linked account is reused and the call returns GOOGLE_ALREADY_CONNECTED.
export async function googleConnect(force = false): Promise<string> { return invoke("google_connect", { force }); }
export async function googleCancelConnect(): Promise<void> { return invoke("google_cancel_connect"); }
export async function googleStatus(): Promise<boolean> { return invoke("google_status"); }
/** Settings > Google. "reconnect": Google no longer accepts the main account's grant. */
export interface GoogleAccounts { main: { state: "connected" | "reconnect" | "not_connected"; name: string | null; email: string | null }; others: MailAccount[] }
export async function googleAccounts(): Promise<GoogleAccounts> { return invoke("google_accounts"); }
// Revokes the grant at Google, deletes the refresh token from the Keychain and
// clears the mail, events and briefings cached from that account.
export async function googleDisconnect(): Promise<string> { return invoke("google_disconnect"); }

// Notion
export async function getNotionPages(limit?: number): Promise<NotionPageView[]> { return invoke("get_notion_pages", { limit }); }
export async function syncNotion(): Promise<string> { return invoke("sync_notion"); }
export async function saveNotionToken(token: string): Promise<void> { return invoke("save_notion_token", { token }); }
export async function getNotionStats(): Promise<number> { return invoke("get_notion_stats"); }

// GitHub
export async function getGitHubItems(itemType?: string): Promise<GitHubItemView[]> { return invoke("get_github_items", { itemType }); }
export async function syncGitHub(): Promise<string> { return invoke("sync_github"); }
export async function saveGitHubToken(token: string): Promise<void> { return invoke("save_github_token", { token }); }
export async function getGitHubStats(): Promise<GitHubStats> { return invoke("get_github_stats"); }

// Voice
export async function startListening(): Promise<string> { return invoke("start_listening"); }
export async function stopListening(): Promise<string> { return invoke("stop_listening"); }
export async function stopSpeaking(): Promise<void> { return invoke("stop_speaking"); }
export async function getVoiceState(): Promise<VoiceState> { return invoke("get_voice_state"); }
export async function toggleMute(): Promise<boolean> { return invoke("toggle_mute"); }
export async function getVoiceSettings(): Promise<VoiceSettings> { return invoke("get_voice_settings"); }
export async function setVoiceSetting(key: string, value: string): Promise<void> { return invoke("set_voice_setting", { key, value }); }
export async function listTtsVoices(): Promise<string[]> { return invoke("list_tts_voices"); }
export async function getWakeWordStatus(): Promise<WakeWordStatus> { return invoke("get_wake_word_status"); }
export async function enableWakeWord(): Promise<void> { return invoke("enable_wake_word"); }
export async function disableWakeWord(): Promise<void> { return invoke("disable_wake_word"); }
export async function isModelDownloaded(): Promise<boolean> { return invoke("is_model_downloaded"); }
// `force` re-downloads over a file that is already on disk. Without it the
// backend skips the fetch whenever a model file is present, which leaves a
// corrupt one unrecoverable from inside the app.
export async function downloadModel(force = false): Promise<boolean> { return invoke("download_model", { force }); }

// Email Rules
export async function getSuggestedRules(): Promise<EmailRule[]> { return invoke("get_suggested_rules"); }
export async function acceptEmailRule(ruleId: number): Promise<void> { return invoke("accept_email_rule", { ruleId }); }
export async function dismissEmailRule(ruleId: number): Promise<void> { return invoke("dismiss_email_rule", { ruleId }); }
export async function getActiveRules(): Promise<EmailRule[]> { return invoke("get_active_rules"); }

// Custom Cron
export async function createCustomCron(description: string): Promise<CronJobView> { return invoke("create_custom_cron", { description }); }
export async function deleteCronJob(jobId: number): Promise<void> { return invoke("delete_cron_job", { jobId }); }
export async function toggleCronJob(jobId: number): Promise<string> { return invoke("toggle_cron_job", { jobId }); }
export async function getUpcomingRuns(schedule: string, count?: number): Promise<string[]> { return invoke("get_upcoming_runs", { schedule, count }); }

// Assistant
export async function getBriefing(): Promise<BriefingResult> { return invoke("get_briefing"); }
export async function speakBriefing(): Promise<BriefingResult> { return invoke("speak_briefing"); }
export async function askJarvis(question: string): Promise<string> { return invoke("ask_jarvis", { question }); }
export async function searchConversations(query: string): Promise<string> { return invoke("search_conversations", { query }); }

// Obsidian
export async function searchObsidian(query: string): Promise<{ path: string; content: string | null }[]> { return invoke("search_obsidian", { query }); }
export async function getObsidianNote(path: string): Promise<string> { return invoke("get_obsidian_note", { path }); }
export async function saveObsidianNote(path: string, content: string): Promise<void> { return invoke("save_obsidian_note", { path, content }); }
export async function listObsidianFiles(): Promise<string[]> { return invoke("list_obsidian_files"); }
export async function saveObsidianKey(key: string): Promise<void> { return invoke("save_obsidian_key", { key }); }

// System Control
export async function openUrl(url: string): Promise<string> { return invoke("open_url", { url }); }

// Wallpaper
export async function enableWallpaper(): Promise<void> { return invoke("enable_wallpaper"); }
export async function disableWallpaper(): Promise<void> { return invoke("disable_wallpaper"); }
export async function toggleWallpaper(): Promise<boolean> { return invoke("toggle_wallpaper"); }
export async function getWallpaperStatus(): Promise<boolean> { return invoke("get_wallpaper_status"); }
export async function raiseWallpaper(): Promise<void> { return invoke("raise_wallpaper"); }
export async function lowerWallpaper(): Promise<void> { return invoke("lower_wallpaper"); }
export async function isWallpaperRaised(): Promise<boolean> { return invoke("is_wallpaper_raised"); }

// Local LLM
export async function listLocalEndpoints(): Promise<LocalEndpoint[]> { return invoke("list_local_endpoints"); }
export async function addLocalEndpoint(name: string, url: string, backendType?: string, apiKey?: string): Promise<LocalEndpoint> {
  return invoke("add_local_endpoint", { name, url, backendType, apiKey });
}
export async function updateLocalEndpoint(id: string, updates: { name?: string; url?: string; api_key?: string; connection_timeout_ms?: number; keep_alive_minutes?: number; is_active?: boolean }): Promise<void> {
  return invoke("update_local_endpoint", { id, ...updates });
}
export async function removeLocalEndpoint(id: string): Promise<void> { return invoke("remove_local_endpoint", { id }); }
export async function testEndpointConnection(endpointId: string): Promise<EndpointHealth> { return invoke("test_endpoint_connection", { endpointId }); }
export async function listEndpointModels(endpointId: string): Promise<LocalModel[]> { return invoke("list_endpoint_models", { endpointId }); }
export async function getProviderChain(): Promise<ProviderChainEntry[]> { return invoke("get_provider_chain"); }
export async function updateProviderChain(chain: ProviderChainEntry[]): Promise<void> { return invoke("update_provider_chain", { chain }); }
export async function updateModelOverride(endpointId: string, modelId: string, overrides: { context_length?: number; tool_capability?: string; system_prompt_suffix?: string }): Promise<void> {
  return invoke("update_model_override", { endpointId, modelId, ...overrides });
}

// System Check
export async function checkSystemCompatibility(): Promise<SystemCompatibility> { return invoke("check_system_compatibility"); }
export async function checkBackendPrerequisites(backendType: string, url?: string): Promise<PrerequisiteCheck[]> { return invoke("check_backend_prerequisites", { backendType, url }); }
export async function getRecommendedModels(endpointId?: string): Promise<RecommendedModel[]> { return invoke("get_recommended_models", { endpointId }); }

// Model Pull
export async function pullModel(endpointId: string, modelName: string, jobId?: string): Promise<string> { return invoke("pull_model", { endpointId, modelName, jobId }); }
export async function cancelModelPull(jobId?: string): Promise<void> { return invoke("cancel_model_pull", { jobId }); }

// Onboarding
export async function checkOnboardingStatus(): Promise<boolean> {
  return invoke<boolean>("check_onboarding_status");
}

export async function markOnboardingComplete(): Promise<void> {
  return invoke<void>("mark_onboarding_complete");
}

// AWS Bedrock credentials
export async function setAwsCredentials(
  accessKeyId: string,
  secretAccessKey: string,
  region: string,
): Promise<void> {
  await invoke("set_aws_credentials", { accessKeyId, secretAccessKey, region });
}
export async function getAwsCredentialsStatus(): Promise<AwsCredsStatus> {
  return await invoke<AwsCredsStatus>("get_aws_credentials_status");
}
export async function clearAwsCredentials(): Promise<void> {
  await invoke("clear_aws_credentials");
}

// User preferences (single key/value)
export async function getPreference(key: string): Promise<string> {
  return await invoke<string>("get_user_preference", { key });
}
export async function getOptionalPreference(key: string): Promise<string> {
  // get_user_preference rejects absent rows. Optional first-launch preferences
  // use the settings map so a missing key is distinct from a database failure.
  const settings = await getSettings();
  return settings.values[key] ?? '';
}
export async function setPreference(key: string, value: string): Promise<void> {
  await invoke("set_user_preference", { key, value });
}

// Local storage: what JARVIS keeps on disk, and how to get it back
export async function getStorageUsage(): Promise<StorageUsage> {
  return await invoke<StorageUsage>("get_storage_usage");
}
export async function runStorageSweep(): Promise<SweepReport> {
  return await invoke<SweepReport>("run_storage_sweep");
}
/** Returns bytes freed, except for "conversations" which returns rows deleted. */
export async function purgeStorage(scope: StorageScope): Promise<number> {
  return await invoke<number>("purge_storage", { scope });
}
export async function setRetentionPolicy(days: number, enabled: boolean): Promise<void> {
  await invoke("set_retention_policy", { days, enabled });
}

// CosyVoice 2 offline TTS
export async function enableOfflineVoice(): Promise<void> {
  await invoke("enable_offline_voice");
}
export async function checkCosyvoiceBinary(): Promise<boolean> {
  return await invoke<boolean>("check_cosyvoice_binary");
}
