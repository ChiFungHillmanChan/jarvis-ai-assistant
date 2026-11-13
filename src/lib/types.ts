export interface Task {
  id: number | null;
  title: string;
  description: string | null;
  deadline: string | null;
  priority: number;
  status: string;
  source: string;
  created_at: string | null;
}

export interface ChatMessage {
  id: number | null;
  role: "user" | "assistant";
  content: string;
  created_at: string | null;
  /** A voice request whose recording was kept and can be played back. */
  has_recording?: boolean;
  /** What JARVIS did for this reply. Kept for this app session only, never saved. */
  steps?: ChatStep[];
}

export interface ChatStep { status: string; detail: string | null }

export interface DashboardData {
  greeting: string;
  task_count: number;
  pending_tasks: Task[];
}

export interface LaunchReadiness {
  launch_stage: string;
  aws_region: string | null;
  bedrock_model_id: string;
  aws_credentials_present: boolean;
  provider_chain_has_bedrock: boolean;
  waitlist_count: number;
  feedback_count: number;
  pro_price_hkd: string;
  team_price_hkd: string;
}

export interface BetaTesterInput {
  name?: string;
  email: string;
  segment?: string;
  use_case?: string;
}

export interface BetaTesterView {
  id: number;
  name: string | null;
  email: string;
  segment: string | null;
  use_case: string | null;
  status: string;
  created_at: string;
}

export interface BetaFeedbackInput {
  tester_email?: string;
  category: string;
  rating: number;
  message: string;
}

export interface Settings {
  values: Record<string, string>;
}

export interface EmailSummary {
  id: number;
  gmail_id: string;
  subject: string | null;
  sender: string;
  snippet: string | null;
  is_read: boolean;
  is_spam: boolean;
  received_at: string;
}

export interface EmailStats {
  unread: number;
  total: number;
  spam: number;
}

export interface CalendarEventView {
  id: number;
  google_id: string;
  summary: string;
  description: string | null;
  location: string | null;
  start_time: string;
  end_time: string;
  attendees: string;
  status: string;
  /** The additional Google account the event came from; null for the main account. */
  account: string | null;
}

export interface CronJobView {
  id: number;
  name: string;
  schedule: string;
  action_type: string;
  status: string;
  last_run: string | null;
  next_run: string | null;
  description: string | null;
}

export interface CronRunView {
  id: number;
  job_id: number;
  started_at: string;
  finished_at: string | null;
  status: string;
  result: string | null;
  error: string | null;
  /** scheduled | catch_up | manual | retry_interrupted; null for a row that is not a run. */
  trigger: string | null;
  /** The scheduled time this row is for, with its UTC offset; null for a manual run. */
  slot: string | null;
}

export interface KeepAwakeView {
  /** The setting. */
  enabled: boolean;
  /** Idle sleep is being prevented right now. False on battery even with the setting on. */
  active: boolean;
}

export interface DailyBriefingSettings {
  enabled: boolean;
  /** "HH:MM", local time. */
  time: string;
  voice: boolean;
  /** The app-wide text-to-speech switch. With it off the briefing is never read aloud. */
  speech_enabled: boolean;
}

export interface DailyBrief {
  /** The local day it is for, YYYY-MM-DD. */
  date: string;
  shown_at: string;
  trigger: string;
  /** From what is saved on this Mac. */
  local: string;
  summary: string | null;
  summary_pending: boolean;
  summary_error: string | null;
}

export interface NotionPageView {
  id: number;
  notion_id: string;
  title: string;
  url: string | null;
  parent_type: string | null;
  last_edited: string | null;
}

export interface GitHubItemView {
  id: number;
  item_type: string;
  title: string;
  repo: string;
  number: number | null;
  state: string;
  url: string | null;
  author: string | null;
  updated_at: string | null;
}

export interface GitHubStats {
  open_prs: number;
  assigned_issues: number;
  review_requested: number;
}

export type VoiceState =
  | "Idle"
  | "Listening"
  | "Processing"
  | "Speaking"
  | "WakeWordListening"
  | "WakeWordDetected"
  | "WakeWordProcessing"
  | "WakeWordSpeaking"
  | "Disabled"
  | { ModelDownloading: number }
  | { Error: string };

export type AiState = "idle" | "thinking" | "speaking";

export type AssistantPhase =
  | "idle"
  | "listening"
  | "transcribing"
  | "thinking"
  | "planning"
  | "acting"
  | "responding"
  | "speaking";

export interface ChatTokenPayload {
  token: string;
  done: boolean;
  // Set by the router when a provider failed mid-stream: everything streamed so far
  // belongs to the failed provider and must be thrown away before the next one starts.
  discard?: boolean;
}

export interface ChatStatusPayload {
  status: string;
  phase?: AssistantPhase;
  detail?: string | null;
}

export interface ChatStatePayload {
  state: AiState;
}

export interface ChatThinkingPayload {
  text: string;
  done?: boolean;
}

export interface TtsAmplitudePayload {
  amplitude: number;
}

export interface ToolCallPayload {
  tool_name: string;
}

export interface VoiceSettings {
  enabled: boolean;
  tts_voice: string;
  tts_rate: number;
  tts_enabled: boolean;
  keep_recordings: boolean;
}

export interface WakeWordStatus {
  enabled: boolean;
  running: boolean;
  model_downloaded: boolean;
  voice_state: VoiceState;
}

export interface EmailRule {
  id: number;
  sender: string;
  archive_count: number;
  rule_status: string;
}

export interface BriefingResult {
  greeting: string;
  briefing: string;
  has_overdue: boolean;
  task_count: number;
}

// Local LLM types
export interface LocalEndpoint {
  id: string;
  name: string;
  url: string;
  backend_type: "ollama" | "vllm" | "generic";
  api_key?: string;
  use_tls: boolean;
  connection_timeout_ms: number;
  keep_alive_minutes: number;
  is_active: boolean;
  last_health_check?: string;
  last_health_status?: boolean;
}

export interface LocalModel {
  id: string;
  endpoint_id: string;
  context_length: number;
  supports_tools: "native" | "prompt_injected" | "chat_only";
}

export interface ProviderChainEntry {
  position: number;
  provider_type: "gemini" | "claude" | "openai" | "bedrock" | "local";
  endpoint_id?: string;
  model_id?: string;
  enabled: boolean;
  /** OpenAI-compatible service (Kimi, Qwen...); absent means api.openai.com. */
  base_url?: string | null;
}

export interface EndpointHealth {
  reachable: boolean;
  model_count: number;
  latency_ms: number;
}

export interface ChatProviderPayload {
  provider: string;
  status: "trying" | "failed" | "active";
}

// System compatibility types
export interface GpuInfo {
  has_metal: boolean;
  has_cuda: boolean;
  cuda_version?: string;
  vram_gb?: number;
}

export interface OllamaStatus {
  installed: boolean;
  running: boolean;
  version?: string;
}

export interface VllmStatus {
  installed: boolean;
  python_available: boolean;
}

export interface SystemCompatibility {
  os: string;
  arch: string;
  total_ram_gb: number;
  available_ram_gb: number;
  gpu: GpuInfo;
  ollama: OllamaStatus;
  vllm: VllmStatus;
  recommended_max_params: string;
}

export interface PrerequisiteCheck {
  name: string;
  description: string;
  passed: boolean;
  required: boolean;
  fix_command?: string;
  fix_label?: string;
}

export interface RecommendedModel {
  id: string;
  name: string;
  param_size: string;
  download_size_gb: number;
  min_ram_gb: number;
  tool_capable: boolean;
  description: string;
  already_pulled: boolean;
}

export interface ModelPullProgress {
  jobId: string;
  endpoint_id: string;
  model: string;
  status: "downloading" | "verifying" | "complete" | "error";
  percent: number;
  completed_bytes: number;
  total_bytes: number;
  error?: string;
}

export interface AwsCredsStatus {
  configured: boolean;
  region: string | null;
}

/// Matches StorageUsage in src-tauri/src/commands/storage.rs. Byte counts are
/// what is on disk now; the counts below them are what the retention sweep acts on.
export interface StorageUsage {
  databaseBytes: number;
  walBytes: number;
  modelsBytes: number;
  cacheBytes: number;
  backupsBytes: number;
  totalBytes: number;
  mailMessageCount: number;
  mailBodyBytes: number;
  conversationCount: number;
  retentionDays: number;
  retentionEnabled: boolean;
  dataDir: string;
}

export interface SweepReport {
  mailBodiesCleared: number;
  conversationsDeleted: number;
  emailsDeleted: number;
  /** AI usage requests combined into the daily totals, which are kept two years: not removed. */
  usageRowsRolledUp: number;
  briefingsDeleted: number;
}

export type StorageScope = "cache" | "backups" | "models" | "mailBodies" | "conversations";

// google_connect and google_disconnect both return a plain status string. This
// is the one value the UI has to recognise: google_connect returns it verbatim
// when an account is already linked and the caller did not ask to switch, so it
// means "nothing happened", not "connected". Must match the value of
// ALREADY_CONNECTED in src-tauri/src/commands/google_auth.rs.
export const GOOGLE_ALREADY_CONNECTED = "Already connected to Google";

/** Mirrors auth::cognito::AccountStatus. `tier` is null when signed in but the server was unreachable. */
export interface AccountStatus {
  available: boolean;
  signed_in: boolean;
  email: string | null;
  tier: string | null;
  subscription_status: string | null;
  trial_ends_at: string | null;
  period_ends_at: string | null;
  cancel_at_period_end: boolean;
  /** Free trial days a checkout would offer now; 0 once the account has subscribed. */
  trial_days: number;
  /** Where `tier` came from: the server now, a saved confirmation while offline, or nowhere. */
  plan_checked: 'live' | 'saved' | 'none';
}

export interface PaymentPage {
  url: string;
  browser_opened: boolean;
  session_generation: number;
}

export interface CheckoutPage extends PaymentPage {
  session_id: string;
  already_subscribed: boolean;
}
