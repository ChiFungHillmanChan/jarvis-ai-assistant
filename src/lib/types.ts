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
}

export interface DashboardData {
  greeting: string;
  task_count: number;
  pending_tasks: Task[];
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
}

export interface ChatStatusPayload {
  status: string;
  phase?: AssistantPhase;
  detail?: string;
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
  provider_type: "claude" | "openai" | "local";
  endpoint_id?: string;
  model_id?: string;
  enabled: boolean;
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
