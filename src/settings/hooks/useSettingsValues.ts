import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { disableWallpaper, enableWallpaper, getSettings, getWallpaperStatus, setPreference, updateSetting } from '../../lib/commands';

/**
 * The saved preferences (get_settings), and a setter that writes one through
 * set_user_preference. The setter shows the new value at once, puts the old
 * one back if the write fails, and rejects so the caller can say so.
 */
export function useSettingsValues() {
  const [values, setValues] = useState<Record<string, string | null>>({});
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef(values);
  latest.current = values;

  const reload = useCallback(async () => {
    try {
      const settings = await getSettings();
      setValues(settings.values);
      setReady(true);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => { void reload(); }, [reload]);

  const set = useCallback(async (key: string, value: string) => {
    const previous = latest.current[key] ?? null;
    setValues(v => ({ ...v, [key]: value }));
    try {
      await setPreference(key, value);
    } catch (e) {
      setValues(v => ({ ...v, [key]: previous }));
      throw e;
    }
  }, []);

  return { values, ready, error, set, reload };
}

/** Wallpaper mode as the backend reports it, and a toggle that rejects on failure. */
export function useWallpaper() {
  const [active, setActive] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getWallpaperStatus().then(setActive).catch(() => {});
    const unlisten = listen<boolean>('wallpaper-status', event => setActive(event.payload));
    return () => { void unlisten.then(stop => stop()); };
  }, []);

  const toggle = useCallback(async (enabled: boolean) => {
    setBusy(true);
    try {
      if (enabled) await enableWallpaper();
      else await disableWallpaper();
      setActive(enabled);
      await updateSetting('wallpaper_mode_enabled', String(enabled));
    } finally {
      setBusy(false);
    }
  }, []);

  return { active, busy, toggle };
}
