// What the sidebar's bottom row shows for the person using this Mac, and the
// small amount of shared state around the optional JARVIS account.
//
// The display name is whatever the user typed into Settings, so it can be
// missing, blank, or in any script. The caller passes the localised fallback
// rather than this module importing the catalog, which keeps the rules
// testable without a locale.

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

/** Tells the sidebar and Settings to re-read the account after sign-in or sign-out. */
export function accountChanged() {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new Event(ACCOUNT_EVENT));
}

export function onAccountChanged(listener: () => void): () => void {
  if (typeof window === 'undefined') return () => {};
  window.addEventListener(ACCOUNT_EVENT, listener);
  return () => window.removeEventListener(ACCOUNT_EVENT, listener);
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
