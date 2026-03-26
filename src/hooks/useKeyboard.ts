import { useEffect, useRef } from "react";

export function useKeyboard(shortcuts: Record<string, () => void>) {
  const ref = useRef(shortcuts);
  ref.current = shortcuts;

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      const s = ref.current;
      if (e.metaKey && e.key === "k") { e.preventDefault(); s["cmd+k"]?.(); }
      if (e.key === "Escape") { e.preventDefault(); s["escape"]?.(); }
      if (e.metaKey && e.shiftKey && e.key === "j") { e.preventDefault(); s["cmd+shift+j"]?.(); }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
}
