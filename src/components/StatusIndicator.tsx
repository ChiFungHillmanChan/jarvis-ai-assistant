interface StatusIndicatorProps { status: "online" | "offline" | "syncing"; }
export default function StatusIndicator({ status }: StatusIndicatorProps) {
  const color = status === "online" ? "rgba(16, 185, 129, 0.7)" : status === "syncing" ? "rgba(255, 180, 0, 0.7)" : "rgba(255, 100, 100, 0.7)";
  return (
    <div style={styles.container}>
      <div style={{ ...styles.dot, background: color }} />
      <span style={{ ...styles.text, color }}>{status.toUpperCase()}</span>
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", alignItems: "center", gap: 6 },
  dot: { width: 6, height: 6, borderRadius: "50%" },
  text: { fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1 },
};
