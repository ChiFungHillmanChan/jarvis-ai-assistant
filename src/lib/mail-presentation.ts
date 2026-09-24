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
