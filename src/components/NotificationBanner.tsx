// jarvis/src/components/NotificationBanner.tsx
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { Settings } from "../lib/types";

export default function NotificationBanner() {
  const { data: settings } = useTauriCommand<Settings>("get_settings");
  const alerts = settings?.values["last_alerts"];

  if (!alerts) return null;

  return (
    <div style={styles.banner}>
      <span style={styles.label}>ALERT</span>
      <span style={styles.text}>{alerts}</span>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  banner: { display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", background: "rgba(255, 100, 100, 0.04)", border: "1px solid rgba(255, 100, 100, 0.15)", borderRadius: 6, marginBottom: 8 },
  label: { color: "rgba(255, 100, 100, 0.8)", fontSize: 8, fontFamily: "var(--font-mono)", letterSpacing: 1.5, flexShrink: 0 },
  text: { color: "rgba(255, 100, 100, 0.7)", fontSize: 11 },
};
