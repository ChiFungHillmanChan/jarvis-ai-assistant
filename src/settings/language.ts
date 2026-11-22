// The reply language follows the interface language until the user picks a
// reply language themselves. `language_follows_ui` records that choice. When
// it is missing (everyone before this rule), the reply language counts as
// following only if it already matches the interface, so an old deliberate
// choice is never overwritten. A UI rule only: the backend reads `language`.

export type ReplyLanguage = 'en' | 'yue' | 'bilingual';
export type UiLanguage = 'en' | 'zh-HK';

export interface SavedLanguage {
  language_follows_ui?: string | null;
  language?: string | null;
}

export function replyLanguageForUi(ui: UiLanguage): ReplyLanguage {
  return ui === 'zh-HK' ? 'yue' : 'en';
}

export function followsUi(saved: SavedLanguage, ui: UiLanguage): boolean {
  if (saved.language_follows_ui === 'true') return true;
  if (saved.language_follows_ui === 'false') return false;
  return !saved.language || saved.language === replyLanguageForUi(ui);
}

/** The reply language to save after the interface changes to `next`, or null to leave it. */
export function replyLanguageAfterUiChange(next: UiLanguage, saved: SavedLanguage): ReplyLanguage | null {
  return followsUi(saved, next) ? replyLanguageForUi(next) : null;
}
