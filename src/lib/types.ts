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

export type VoiceState = "Idle" | "Listening" | "Processing" | "Speaking" | "Disabled" | { Error: string };

export interface VoiceSettings {
  enabled: boolean;
  tts_voice: string;
  tts_rate: number;
  tts_enabled: boolean;
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
