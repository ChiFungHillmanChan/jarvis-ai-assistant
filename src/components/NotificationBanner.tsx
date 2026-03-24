import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { Settings } from "../lib/types";

export default function NotificationBanner() {
  const { data: settings } = useTauriCommand<Settings>("get_settings");
  const [dismissed, setDismissed] = useState(false);
  const alerts = settings?.values["last_alerts"];

  if (!alerts || dismissed) return null;

  return (
    <div style={styles.banner}>
      <span style={styles.dot} />
      <span style={styles.text}>{alerts}</span>
      <button onClick={() => setDismissed(true)} style={styles.closeBtn}>x</button>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  banner: {
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    padding: "3px 10px",
    background: "rgba(255, 100, 100, 0.06)",
    border: "1px solid rgba(255, 100, 100, 0.12)",
    borderRadius: 12,
    marginBottom: 6,
    maxWidth: "fit-content",
  },
  dot: {
    width: 5,
    height: 5,
    borderRadius: "50%",
    background: "rgba(255, 100, 100, 0.7)",
    flexShrink: 0,
  },
  text: {
    color: "rgba(255, 100, 100, 0.6)",
    fontSize: 9,
    fontFamily: "var(--font-mono)",
    letterSpacing: 0.5,
  },
  closeBtn: {
    background: "transparent",
    border: "none",
    color: "rgba(255, 100, 100, 0.3)",
    fontSize: 9,
    cursor: "pointer",
    padding: "0 2px",
    fontFamily: "var(--font-mono)",
  },
};
