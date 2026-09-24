import type { MailMessage } from './mail';

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
