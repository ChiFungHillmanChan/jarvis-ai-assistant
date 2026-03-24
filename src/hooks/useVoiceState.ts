import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { VoiceState } from "../lib/types";
import { getVoiceState, startListening, stopListening } from "../lib/commands";

export function useVoiceState() {
  const [state, setState] = useState<VoiceState>("Idle");
  const [lastResponse, setLastResponse] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let mounted = true;

    async function subscribe() {
      try {
        const initialState = await getVoiceState();
        if (mounted) {
          setState(initialState);
        }

        unlisten = await listen<VoiceState>("voice-state", (event) => {
          if (mounted) {
            setState(event.payload);
          }
        });
      } catch (e) {
        if (mounted) {
          setError(String(e));
        }
      }
    }

    subscribe();

    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  const startVoice = useCallback(async () => {
    setError(null);
    try {
      await startListening();
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const stopVoice = useCallback(async () => {
    setError(null);
    try {
      const response = await stopListening();
      setLastResponse(response);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return { state, lastResponse, error, startVoice, stopVoice };
}
