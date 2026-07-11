import { MessageSquare, ArrowUp } from "./icons";

interface CommandBarProps { onActivate: () => void; }
export default function CommandBar({ onActivate }: CommandBarProps) {
  return (
    <div style={styles.container}>
      <button onClick={onActivate} style={styles.bar} aria-label="Talk to JARVIS">
        <span style={styles.icon}><MessageSquare size={18} /></span>
        <span style={styles.text}>Talk to JARVIS...</span>
        <span style={styles.kbd}>&#8984; K</span>
        <span style={styles.send}><ArrowUp size={16} /></span>
      </button>
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  container: { padding: "8px 16px 16px 16px" },
  bar: { width: "100%", display: "flex", alignItems: "center", gap: 12, padding: "0 8px 0 16px", height: 46, background: "rgba(8,14,26,0.7)", border: "1px solid var(--border-strong)", borderRadius: 23, cursor: "pointer", boxShadow: "0 0 30px -12px rgba(0,180,255,0.5)", transition: "border-color 0.2s ease" },
  icon: { color: "rgb(0,180,255)", display: "flex" },
  text: { color: "var(--muted)", fontSize: 13.5, flex: 1, textAlign: "left" as const },
  kbd: { color: "var(--cyan-dim)", fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: 1, border: "1px solid var(--border-strong)", borderRadius: 6, padding: "4px 8px", background: "rgba(0,180,255,0.06)" },
  send: { width: 32, height: 32, borderRadius: "50%", display: "flex", alignItems: "center", justifyContent: "center", color: "#04121c", background: "linear-gradient(150deg, rgb(0,180,255), #0a86c4)", flexShrink: 0 },
};
