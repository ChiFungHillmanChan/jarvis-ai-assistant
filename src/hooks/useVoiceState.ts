import { useState, useCallback } from "react";
import type { VoiceState } from "../lib/types";
import { startListening, stopListening } from "../lib/commands";

export function useVoiceState() {
  const [state, setState] = useState<VoiceState>("Idle");
  const [lastResponse, setLastResponse] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const startVoice = useCallback(async () => {
    setError(null);
    try { await startListening(); setState("Listening"); }
    catch (e) { setError(String(e)); }
  }, []);

  const stopVoice = useCallback(async () => {
    try { setState("Processing"); const response = await stopListening(); setLastResponse(response); setState("Idle"); }
    catch (e) { setError(String(e)); setState("Idle"); }
  }, []);

  return { state, lastResponse, error, startVoice, stopVoice };
}
