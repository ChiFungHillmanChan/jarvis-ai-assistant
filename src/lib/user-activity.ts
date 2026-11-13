// Whether someone has clicked or pressed a key in this window since JARVIS
// started. A request that only a person looking at a page should cause (the
// Bedrock model list in Settings) waits for it, so a page restored at launch,
// or one open in a window nobody has used yet, asks nothing.
import { useSyncExternalStore } from "react";

let acted = false;
const listeners = new Set<() => void>();

function mark() {
  if (acted) return;
  acted = true;
  for (const listener of listeners) listener();
}

type Target = { addEventListener(type: string, listener: () => void, capture: boolean): void };

/** Called once from main.tsx, before the app renders. */
export function watchUserActivity(target: Target = document): void {
  target.addEventListener("pointerdown", mark, true);
  target.addEventListener("keydown", mark, true);
}

export function userHasActed(): boolean {
  return acted;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useUserHasActed(): boolean {
  return useSyncExternalStore(subscribe, userHasActed);
}
