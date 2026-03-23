import { useEffect } from "react";
export function useKeyboard(shortcuts: Record<string, () => void>) {
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.metaKey && e.key === "k") { e.preventDefault(); shortcuts["cmd+k"]?.(); }
      if (e.key === "Escape") { e.preventDefault(); shortcuts["escape"]?.(); }
      if (e.metaKey && e.shiftKey && e.key === "j") { e.preventDefault(); shortcuts["cmd+shift+j"]?.(); }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [shortcuts]);
}
