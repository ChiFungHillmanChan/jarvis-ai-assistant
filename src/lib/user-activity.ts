// Whether someone has clicked or pressed a key in this window since JARVIS
// started. A request that only a person looking at a page should cause (the
// Bedrock model list in Settings) waits for it, so a page restored at launch,
// or one open in a window nobody has used yet, asks nothing.
//
// A press made through VoiceOver, Voice Control or Switch Control can reach
// the page as a `click` with no `pointerdown` before it (WebKit sent no
// pointer events for such presses before October 2026), so `click` counts
// too. No code in the app clicks or dispatches a click by itself, so nothing
// opens the gate at launch.
import { useSyncExternalStore } from "react";

type Target = { addEventListener(type: string, listener: () => void, capture: boolean): void };

/** The events that count as someone using the window, listened to in the capture phase. */
export const ACTIVITY_EVENTS = ["pointerdown", "keydown", "click"] as const;

/** One gate: closed until the first of ACTIVITY_EVENTS reaches the target. */
export function activityGate() {
  let acted = false;
  const listeners = new Set<() => void>();
  function mark() {
    if (acted) return;
    acted = true;
    for (const listener of listeners) listener();
  }
  return {
    watch(target: Target) {
      for (const type of ACTIVITY_EVENTS) target.addEventListener(type, mark, true);
    },
    hasActed: () => acted,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
  };
}

const gate = activityGate();

/** Called once from main.tsx, before the app renders. */
export function watchUserActivity(target: Target = document): void {
  gate.watch(target);
}

export function userHasActed(): boolean {
  return gate.hasActed();
}

export function useUserHasActed(): boolean {
  return useSyncExternalStore(gate.subscribe, gate.hasActed);
}
