interface GreetingHeaderProps {
  greeting: string;
  taskCount: number;
}
export default function GreetingHeader({ greeting, taskCount }: GreetingHeaderProps) {
  return (
    <div style={styles.container}>
      <div style={styles.topBar}>
        <span className="system-text">JARVIS OS v0.1</span>
        <span style={styles.date}>
          {new Date().toLocaleDateString("en-US", { weekday: "short", day: "numeric", month: "short", year: "numeric" }).toUpperCase()}
        </span>
      </div>
      <div style={styles.greeting}>{greeting}</div>
      <div style={styles.summary}>
        {taskCount > 0 ? `You have ${taskCount} pending task${taskCount !== 1 ? "s" : ""}.` : "All clear. No pending tasks."}
      </div>
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  container: { padding: "0 0 16px 0", borderBottom: "1px solid rgba(0, 180, 255, 0.1)", marginBottom: 16 },
  topBar: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 12 },
  date: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, fontFamily: "var(--font-mono)" },
  greeting: { color: "rgba(0, 180, 255, 0.7)", fontSize: 16, fontWeight: 300, marginBottom: 4 },
  summary: { color: "rgba(0, 180, 255, 0.4)", fontSize: 12 },
};
