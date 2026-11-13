import { normalizeUiLocale, type UiLocale } from './catalog.ts';

type LocaleSnapshot = {
  locale: UiLocale;
  ready: boolean;
  hasSavedChoice: boolean;
  saving: boolean;
  saveError: boolean;
  loadError: boolean;
};

/** The native preference is authoritative; a browser language is only a suggestion. */
export function createLocaleStore(initialLocale: UiLocale, read: () => Promise<string>, write: (locale: UiLocale) => Promise<void>) {
  let snapshot: LocaleSnapshot = { locale: initialLocale, ready: false, hasSavedChoice: false, saving: false, saveError: false, loadError: false };
  let choiceVersion = 0;
  let pendingWrite = Promise.resolve();
  const listeners = new Set<() => void>();
  function update(change: Partial<LocaleSnapshot>) {
    snapshot = { ...snapshot, ...change };
    listeners.forEach(listener => listener());
  }
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    async restore() {
      const startingVersion = choiceVersion;
      update({ loadError: false });
      try {
        const saved = await read();
        if (choiceVersion !== startingVersion) return;
        update({ locale: normalizeUiLocale(saved, snapshot.locale), hasSavedChoice: saved === 'en' || saved === 'zh-HK', ready: true });
      } catch {
        if (choiceVersion === startingVersion) update({ loadError: true, ready: false });
      }
    },
    /**
     * Saves the language in use when no choice was ever saved, so messages
     * written by the backend (it reads the saved ui_locale) match the
     * interface. Only for someone past the language step: on first launch
     * the suggestion stays a suggestion until the user picks. A choice made
     * meanwhile wins, and a failed write changes nothing on screen.
     */
    keepLocaleInUse(): Promise<boolean> {
      if (!snapshot.ready || snapshot.hasSavedChoice || snapshot.saving) return Promise.resolve(false);
      const startingVersion = choiceVersion;
      const locale = snapshot.locale;
      const kept = pendingWrite.then(() => {
        if (choiceVersion !== startingVersion) return false;
        return write(locale).then(() => {
          if (choiceVersion === startingVersion) update({ hasSavedChoice: true });
          return true;
        }, () => false);
      });
      pendingWrite = kept.then(() => {});
      return kept;
    },
    setLocale(locale: UiLocale): Promise<boolean> {
      const saveVersion = ++choiceVersion;
      update({ locale, ready: true, saving: true, saveError: false, loadError: false });
      // Serialize writes so the last selection also wins after restarting the app.
      const saved = pendingWrite.then(() => write(locale)).then(() => {
        if (choiceVersion === saveVersion) update({ hasSavedChoice: true, saving: false, saveError: false });
        return true;
      }, () => {
        if (choiceVersion === saveVersion) update({ saving: false, saveError: true });
        return false;
      });
      pendingWrite = saved.then(() => {});
      return saved;
    },
  };
}
