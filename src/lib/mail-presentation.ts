import type { MailAnalysis, MailMessage } from './mail';

export function mailSourceUrl(message: Pick<MailMessage, 'provider_id' | 'account_email'>): string | null {
  if (!/^[a-f\d]{1,64}$/i.test(message.provider_id) || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(message.account_email)) return null;
  const url = new URL('https://mail.google.com/mail/');
  url.searchParams.set('authuser', message.account_email);
  url.hash = `all/${message.provider_id}`;
  return url.href;
}

export function revealMailSource(
  requestedId: number | null,
  loadedId: number | null,
  reader: Pick<HTMLElement, 'scrollIntoView' | 'focus'> | null,
): boolean {
  if (requestedId === null || requestedId !== loadedId || !reader) return false;
  reader.scrollIntoView({ block: 'start', inline: 'nearest' });
  reader.focus({ preventScroll: true });
  return true;
}

/** Split a From header into what the list shows in bold and what it shows muted. */
export function parseSender(raw: string): { name: string; address: string } {
  const value = raw.trim();
  const match = /^(.*?)\s*<([^<>\s]+@[^<>\s]+)>$/.exec(value);
  if (!match) return { name: value, address: '' };
  const name = match[1].trim().replace(/^"(.*)"$/, '$1').trim();
  return name ? { name, address: match[2] } : { name: match[2], address: '' };
}

export function fileSize(bytes: number): string {
  if (!bytes) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Stored bodies start with the header block get_message_full writes for the AI
 * (integrations/gmail.rs). The reader already shows sender, subject and date.
 */
export function readableBody(body: string): string {
  const headers = /^From: [^\n]*\nTo: [^\n]*\nDate: [^\n]*\nSubject: [^\n]*\n\n/.exec(body);
  return headers ? body.slice(headers[0].length) : body;
}

/**
 * Gmail returns snippets HTML-escaped. Sync decodes new ones, but rows saved
 * before that fix still read "Here&#39;s your label &amp; …" in the list.
 */
export function decodeEntities(value: string): string {
  const named: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' };
  return value.replace(/&(#x[\da-f]+|#\d+|[a-z]+);/gi, (match, code: string) => {
    if (code[0] === '#') {
      const point = code[1].toLowerCase() === 'x' ? parseInt(code.slice(2), 16) : parseInt(code.slice(1), 10);
      return Number.isFinite(point) && point > 0 && point <= 0x10ffff ? String.fromCodePoint(point) : match;
    }
    return named[code.toLowerCase()] ?? match;
  });
}

export type Text = (en: string, zh: string) => string;
export const MAIL_CATEGORIES = ['work', 'jobs', 'finance', 'schedule', 'account', 'personal', 'promotions'] as const;
export type Category = typeof MAIL_CATEGORIES[number];

export function categoryLabel(category: string, text: Text): string {
  switch (category) {
    case 'work': return text('Work and projects', '工作及項目');
    case 'jobs': return text('Jobs and recruiting', '求職及招聘');
    case 'finance': return text('Bills and money', '帳單及財務');
    case 'schedule': return text('Bookings and events', '預約及活動');
    case 'account': return text('Accounts and security', '帳戶及安全');
    case 'personal': return text('Personal', '個人事務');
    case 'promotions': return text('Promotions and newsletters', '推廣及電子報');
    default: return category;
  }
}

/**
 * Who answered a group analysis. With "Same AI as the assistant" the backend
 * reports "Provider · model" for the AI that answered (mail/analysis.rs), so
 * that string already names it; only "local" mode is Ollama on this Mac.
 */
export function analysisSource(result: Pick<MailAnalysis, 'mode' | 'model'>, text: Text): string {
  switch (result.mode) {
    case 'assistant': return result.model;
    case 'openai': return `OpenAI ${result.model}`;
    case 'bedrock': return `AWS Bedrock ${result.model}`;
    case 'local': return text(`Local Ollama ${result.model}`, `本機 Ollama ${result.model}`);
    default: return result.model;
  }
}

export function itemKindLabel(kind: string, text: Text): string {
  switch (kind) {
    case 'task': return text('To do', '待辦');
    case 'deadline': return text('Deadline', '截止');
    case 'event': return text('Event', '活動');
    case 'reply': return text('Reply', '待回覆');
    default: return kind;
  }
}

/** Local calendar date as YYYY-MM-DD. */
export function dayKey(value: Date): string {
  return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, '0')}-${String(value.getDate()).padStart(2, '0')}`;
}

export type DueBucket = 'overdue' | 'today' | 'week' | 'later' | 'undated';
export const DUE_BUCKETS: DueBucket[] = ['overdue', 'today', 'week', 'later', 'undated'];

/** Where an item belongs on the agenda, by its due date against `today`. */
export function dueBucket(dueDate: string | null, today: Date): DueBucket {
  if (!dueDate) return 'undated';
  const key = dayKey(today);
  if (dueDate < key) return 'overdue';
  if (dueDate === key) return 'today';
  const weekEnd = new Date(today); weekEnd.setDate(today.getDate() + 7);
  return dueDate <= dayKey(weekEnd) ? 'week' : 'later';
}

export function dueBucketLabel(bucket: DueBucket, text: Text): string {
  switch (bucket) {
    case 'overdue': return text('Overdue', '已過期');
    case 'today': return text('Today', '今日');
    case 'week': return text('Next 7 days', '未來 7 日');
    case 'later': return text('Later', '較後');
    case 'undated': return text('No date', '未定日期');
  }
}

/** "Wed 30 Sep 15:00" / "9月30日（週三）15:00" for a local date and optional time. */
export function formatDue(date: string | null, time: string | null, locale: string, weekday = true): string {
  if (!date) return '';
  const day = new Date(`${date}T12:00:00`);
  if (Number.isNaN(day.getTime())) return date;
  let label: string;
  if (locale.startsWith('zh')) {
    const week = weekday ? `（${day.toLocaleDateString(locale, { weekday: 'short' })}）` : '';
    label = `${day.getMonth() + 1}月${day.getDate()}日${week}`;
  } else {
    label = day.toLocaleDateString(locale, weekday ? { weekday: 'short', day: 'numeric', month: 'short' } : { day: 'numeric', month: 'short' });
  }
  return time ? `${label} ${time}` : label;
}

function localMinute(value: Date): string {
  return `${dayKey(value)}T${String(value.getHours()).padStart(2, '0')}:${String(value.getMinutes()).padStart(2, '0')}`;
}

/**
 * A sensible first reminder: an hour before a timed item, 09:00 on the day
 * for a dated one, 09:00 tomorrow for an undated one. Never in the past.
 */
export function defaultRemindAt(dueDate: string | null, dueTime: string | null, now: Date): string {
  let at: Date;
  if (dueDate && dueTime) { at = new Date(`${dueDate}T${dueTime}:00`); at.setHours(at.getHours() - 1); }
  else if (dueDate) at = new Date(`${dueDate}T09:00:00`);
  else { at = new Date(now); at.setDate(now.getDate() + 1); at.setHours(9, 0, 0, 0); }
  if (Number.isNaN(at.getTime()) || at <= now) { at = new Date(now); at.setMinutes(0, 0, 0); at.setHours(at.getHours() + 1); }
  return localMinute(at);
}

/** SQLite datetime('now') values are UTC without a zone marker. */
export function backendUtc(value: string): Date {
  return new Date(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value) ? `${value.replace(' ', 'T')}Z` : value);
}
