import type { MessageKey } from '../i18n/catalog';

// Mirrors GMAIL_WRONG_ACCOUNT and GMAIL_NOT_GRANTED in
// src-tauri/src/commands/google_auth.rs; tests/google-gmail.test.ts checks
// they still match.
export const GMAIL_WRONG_ACCOUNT = 'Gmail must be connected on the main Google account';
export const GMAIL_NOT_GRANTED = 'Google did not grant Gmail access';

/** Settings offers Connect Gmail only while the main account works without it. */
export function offersConnectGmail(main: { state: string; gmail: boolean }): boolean {
  return main.state === 'connected' && !main.gmail;
}

/** The catalog message for a google_connect_gmail refusal, or null to show the backend text. */
export function gmailConnectErrorKey(error: unknown): MessageKey | null {
  const message = String(error);
  if (message === GMAIL_WRONG_ACCOUNT) return 'googleGmailWrongAccount';
  if (message === GMAIL_NOT_GRANTED) return 'googleGmailNotGranted';
  return null;
}
