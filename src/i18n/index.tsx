import { createContext, useContext, useEffect, useMemo, useSyncExternalStore, type ReactNode } from 'react';
import { getOptionalPreference, setPreference } from '../lib/commands';
import { normalizeUiLocale, suggestedUiLocale, translate, type MessageKey, type UiLocale } from './catalog';
import { createLocaleStore } from './locale-store';
export { catalog, interpolate, normalizeUiLocale, suggestedUiLocale, translate } from './catalog';
export type { MessageKey, UiLocale } from './catalog';
const storageKey = 'jarvis.ui_locale';
function initialLocale() {
  const suggested = suggestedUiLocale(typeof navigator === 'undefined' ? [] : navigator.languages);
  try { return normalizeUiLocale(localStorage.getItem(storageKey), suggested); }
  catch { return suggested; }
}
const store = createLocaleStore(initialLocale(), () => getOptionalPreference('ui_locale'), locale => setPreference('ui_locale', locale));
export function getUiLocale() { return store.getSnapshot().locale; }
export const subscribeUiLocale = store.subscribe;
export const setUiLocale = store.setLocale;
export const keepUiLocaleInUse = store.keepLocaleInUse;
type LocaleContextValue = {
  locale: UiLocale; ready: boolean; hasSavedChoice: boolean; saving: boolean; loadError: boolean;
  saveError: string | null; retryLoad: () => void; retrySave: () => void;
  setLocale: (locale: UiLocale) => Promise<boolean>;
  t: (key: MessageKey, params?: Record<string, string | number>) => string;
  text: (english: string, traditionalChinese: string) => string;
};
const LocaleContext = createContext<LocaleContextValue | null>(null);
export function UiLocaleProvider({ children }: { children: ReactNode }) {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  useEffect(() => { void store.restore(); }, []);
  useEffect(() => {
    document.documentElement.lang = state.locale;
    try { localStorage.setItem(storageKey, state.locale); } catch { /* Native preferences still persist when browser storage is unavailable. */ }
  }, [state.locale]);
  const value = useMemo<LocaleContextValue>(() => ({
    ...state,
    saveError: state.saveError ? 'Could not save the interface language. Retry before closing JARVIS.' : null,
    retryLoad: () => { void store.restore(); }, retrySave: () => { void setUiLocale(state.locale); }, setLocale: setUiLocale,
    t: (key, params) => translate(state.locale, key, params),
    text: (english, traditionalChinese) => state.locale === 'zh-HK' ? traditionalChinese : english,
  }), [state]);
  return <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>;
}
export function useUiLocale() { const value = useContext(LocaleContext); if (!value) throw new Error('UiLocaleProvider is required'); return value; }
