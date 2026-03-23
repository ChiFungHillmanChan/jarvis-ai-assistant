interface CommandBarProps { onActivate: () => void; }
export default function CommandBar({ onActivate }: CommandBarProps) {
  return (
    <div style={styles.container}>
      <button onClick={onActivate} style={styles.bar}>
        <span style={styles.icon}>&gt;_</span>
        <span style={styles.text}>Talk to JARVIS...</span>
        <span style={styles.shortcut}>Cmd+K</span>
      </button>
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  container: { padding: "8px 16px 16px 16px" },
  bar: { width: "100%", display: "flex", alignItems: "center", gap: 8, padding: "8px 14px", background: "rgba(0, 180, 255, 0.02)", border: "1px solid rgba(0, 180, 255, 0.12)", borderRadius: 20, cursor: "pointer", transition: "border-color 0.2s ease" },
  icon: { color: "rgba(0, 180, 255, 0.4)", fontFamily: "var(--font-mono)", fontSize: 11 },
  text: { color: "rgba(0, 180, 255, 0.3)", fontSize: 12, flex: 1, textAlign: "left" as const },
  shortcut: { color: "rgba(0, 180, 255, 0.2)", fontFamily: "var(--font-mono)", fontSize: 10 },
};
