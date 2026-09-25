// What the sidebar's bottom row shows for the person using this Mac, and the
// small amount of shared state around the optional JARVIS account.
//
// The display name is whatever the user typed into Settings, so it can be
// missing, blank, or in any script. The caller passes the localised fallback
// rather than this module importing the catalog, which keeps the rules
// testable without a locale.

import type { AccountStatus } from './types';

export interface AccountBadge {
  label: string;
  /** A single character for the avatar circle, or null to draw an icon instead. */
  initial: string | null;
  isPlaceholder: boolean;
}

/** Settings key holding the name; there is no server-side profile behind it. */
export const DISPLAY_NAME_KEY = 'display_name';
const DISPLAY_NAME_EVENT = 'jarvis:display-name-changed';

/** Tells an open sidebar to re-read the name after Settings saves it. */
export function displayNameChanged() {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new Event(DISPLAY_NAME_EVENT));
}

export function onDisplayNameChanged(listener: () => void): () => void {
  if (typeof window === 'undefined') return () => {};
  window.addEventListener(DISPLAY_NAME_EVENT, listener);
  return () => window.removeEventListener(DISPLAY_NAME_EVENT, listener);
}

export function accountBadge(displayName: string | null | undefined, fallback: string): AccountBadge {
  const name = displayName?.trim() ?? '';
  if (!name) return { label: fallback, initial: null, isPlaceholder: true };
  // Array.from splits by code point; charAt would cut a surrogate pair in half.
  const first = Array.from(name)[0] ?? '';
  return { label: name, initial: first.toUpperCase(), isPlaceholder: false };
}

/** Error strings the Rust account commands return; see auth/cognito.rs. */
export const NOT_SIGNED_IN = 'not_signed_in';
export const SIGN_IN_CANCELLED = 'sign_in_cancelled';

const ACCOUNT_EVENT = 'jarvis:account-changed';
const SESSION_EVENT = 'jarvis:account-session-changing';
let sessionRevision = 0;
let sessionChanging = false;

/** Every sign-in/out invalidates billing work from the previous session. */
export function beginAccountSessionChange(): number {
  sessionRevision++;
  sessionChanging = true;
  if (typeof window !== 'undefined') window.dispatchEvent(new CustomEvent(SESSION_EVENT, { detail: true }));
  return sessionRevision;
}

export function finishAccountSessionChange(revision: number) {
  if (revision !== sessionRevision) return;
  sessionChanging = false;
  if (typeof window !== 'undefined') window.dispatchEvent(new CustomEvent(SESSION_EVENT, { detail: false }));
}

export function isAccountSessionChanging() { return sessionChanging; }

export function onAccountSessionChange(listener: (changing: boolean) => void): () => void {
  if (typeof window === 'undefined') return () => {};
  const receive = (event: Event) => listener((event as CustomEvent<boolean>).detail);
  window.addEventListener(SESSION_EVENT, receive);
  return () => window.removeEventListener(SESSION_EVENT, receive);
}

export function createAccountOperationGuard() {
  let revision = 0;
  return {
    invalidate() { revision++; },
    capture() {
      const local = revision;
      const session = sessionRevision;
      return () => local === revision && session === sessionRevision;
    },
  };
}

/** Tells the sidebar and Settings to re-read the account after sign-in or sign-out. */
export function accountChanged(status?: AccountStatus) {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent(ACCOUNT_EVENT, { detail: status }));
}

export function onAccountChanged(listener: (status?: AccountStatus) => void): () => void {
  if (typeof window === 'undefined') return () => {};
  const receive = (event: Event) => listener((event as CustomEvent<AccountStatus | undefined>).detail);
  window.addEventListener(ACCOUNT_EVENT, receive);
  return () => window.removeEventListener(ACCOUNT_EVENT, receive);
}

export type TierKey = 'tierFree' | 'tierPro' | 'tierTeam' | 'tierUnknown';

/** Anything the server did not clearly say reads as unknown, never as a paid plan. */
export function tierKey(tier: string | null | undefined): TierKey {
  switch (tier) {
    case 'free': return 'tierFree';
    case 'pro': return 'tierPro';
    case 'team': return 'tierTeam';
    default: return 'tierUnknown';
  }
}

/** The typed display name wins; a signed-in account's email is next; then the fallback. */
export function accountLabel(displayName: string | null | undefined, email: string | null | undefined, fallback: string): AccountBadge {
  const name = displayName?.trim() ? displayName : email;
  return accountBadge(name, fallback);
}

/**
 * Whether Settings should show its Account section. Mirrors the sidebar's own
 * `account?.available` check. A status that has not loaded yet (null or
 * undefined) reads as hidden, never as shown -- the section must not flash
 * into view only to disappear once the real status arrives.
 */
export function accountSectionVisible(status: AccountStatus | null | undefined): boolean {
  return status?.available === true;
}
