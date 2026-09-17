// What the sidebar's bottom row shows for the person using this Mac.
//
// JARVIS has no account of its own: the name is whatever the user typed into
// Settings, so it can be missing, blank, or in any script. The caller passes
// the localised fallback rather than this module importing the catalog, which
// keeps the rule testable without a locale.

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
