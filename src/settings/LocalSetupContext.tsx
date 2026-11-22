import { createContext, useContext, useEffect, type ReactNode } from 'react';
import { useLocalSetup } from '../components/LocalMailSetup';

type LocalSetup = ReturnType<typeof useLocalSetup>;
interface LocalSetupValue { ai: LocalSetup; mail: LocalSetup }

const LocalSetupContext = createContext<LocalSetupValue | null>(null);

/**
 * The Ollama runs Settings starts, owned above the dialog. Dialog content
 * unmounts on close and on every section change, and an unmounted run is
 * cancelled, so a multi-GB download must not live in a section. `ai` is
 * Your AI's download (mail settings unchanged); `mail` is Advanced → Local
 * AI's prepare and use for mail. Onboarding keeps its own run.
 */
export function LocalSetupProvider({ children }: { children: ReactNode }) {
  const ai = useLocalSetup({ applyToMail: false, probeOnMount: false, onReady: () => ai.refresh() });
  const mail = useLocalSetup({ probeOnMount: false });
  return <LocalSetupContext.Provider value={{ ai, mail }}>{children}</LocalSetupContext.Provider>;
}

/**
 * One section's run, and whether the other run is going (only one may run at
 * a time). The Mac is checked again each time the section opens, unless a run
 * is going.
 */
export function useSettingsLocalSetup(which: keyof LocalSetupValue): { setup: LocalSetup; otherBusy: boolean } {
  const value = useContext(LocalSetupContext);
  if (!value) throw new Error('LocalSetupProvider is required');
  const setup = value[which];
  const otherBusy = value[which === 'ai' ? 'mail' : 'ai'].busy;
  useEffect(() => { if (!setup.busy) void setup.refresh(); }, []);
  return { setup, otherBusy };
}
