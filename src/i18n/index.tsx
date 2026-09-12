import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { getPreference, setPreference } from '../lib/commands';
import { normalizeUiLocale, suggestedUiLocale, translate, type MessageKey, type UiLocale } from './catalog';
export { catalog, interpolate, normalizeUiLocale, suggestedUiLocale, translate } from './catalog';
export type { MessageKey, UiLocale } from './catalog';
const storageKey = 'jarvis.ui_locale';
const browserLanguages = () => typeof navigator === 'undefined' ? [] : navigator.languages;
const initial = () => normalizeUiLocale(typeof localStorage === 'undefined' ? undefined : localStorage.getItem(storageKey), suggestedUiLocale(browserLanguages()));
let currentLocale: UiLocale = initial();
let saveError: string | null = null;
let choiceVersion = 0;
const listeners = new Set<() => void>();
export function getUiLocale() { return currentLocale; }
export function subscribeUiLocale(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; }
export function setUiLocale(locale: UiLocale) {
  choiceVersion += 1;
  const saveVersion = choiceVersion;
  currentLocale = locale;
  saveError = null;
  if (typeof localStorage !== 'undefined') localStorage.setItem(storageKey, locale);
  if (typeof document !== 'undefined') document.documentElement.lang = locale;
  listeners.forEach(listener => listener());
  void setPreference('ui_locale', locale).catch(() => { if (choiceVersion !== saveVersion) return; saveError = 'Could not save the interface language. Retry before closing JARVIS.'; listeners.forEach(listener => listener()); });
}
type LocaleContextValue = { locale: UiLocale; saveError: string | null; retrySave: () => void; setLocale: (locale: UiLocale) => void; t: (key: MessageKey, params?: Record<string, string | number>) => string; text: (english: string, cantonese: string) => string };
const LocaleContext = createContext<LocaleContextValue | null>(null);
export function UiLocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<UiLocale>(currentLocale);
  const [, forceRender] = useState(0);
  useEffect(() => subscribeUiLocale(() => { setLocaleState(currentLocale); forceRender(value => value + 1); }), []);
  useEffect(() => { document.documentElement.lang = locale; }, [locale]);
  useEffect(() => {
    document.documentElement.lang = locale;
    let active = true;
    const startingVersion = choiceVersion;
    getPreference('ui_locale').then(value => {
      if (!active || choiceVersion !== startingVersion) return;
      const saved = normalizeUiLocale(value, currentLocale);
      if (saved !== currentLocale) { currentLocale = saved; localStorage.setItem(storageKey, saved); listeners.forEach(listener => listener()); }
    }).catch(() => {});
    return () => { active = false; };
  }, []);
  const value = useMemo<LocaleContextValue>(() => ({ locale, saveError, retrySave: () => setUiLocale(currentLocale), setLocale: setUiLocale, t: (key, params) => translate(locale, key, params), text: (english, cantonese) => locale === 'zh-HK' ? cantonese : english }), [locale, saveError]);
  return <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>;
}
export function useUiLocale() { const value = useContext(LocaleContext); if (!value) throw new Error('UiLocaleProvider is required'); return value; }
