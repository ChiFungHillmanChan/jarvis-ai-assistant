// The one name the frontend shows for the person using this Mac, in the same
// order the backend uses in replies: what JARVIS calls you (assistant_address),
// then a display name typed in an older build, then the Google profile name.

export interface SavedNames {
  assistant_address?: string | null;
  display_name?: string | null;
  user_name?: string | null;
}

export function nameToShow(v: SavedNames): string | null {
  for (const value of [v.assistant_address, v.display_name, v.user_name]) {
    const name = value?.trim();
    if (name) return name;
  }
  return null;
}
