import { memo } from "react";
import type { Task } from "../lib/types";
interface TimelineItemProps { task: Task; }
export default memo(function TimelineItem({ task }: TimelineItemProps) {
  let isOverdue = false;
  let isDueToday = false;
  if (task.deadline) {
    const dl = new Date(task.deadline);
    const now = new Date();
    isOverdue = dl <= now;
    isDueToday = dl.toDateString() === now.toDateString();
  }
  const dotColor = isOverdue ? "rgba(255, 100, 100, 0.6)" : isDueToday ? "rgba(255, 180, 0, 0.5)" : "rgba(0, 180, 255, 0.4)";
  const textColor = isOverdue ? "rgba(255, 100, 100, 0.8)" : "rgba(0, 180, 255, 0.8)";
  return (
    <div style={styles.item}>
      <div style={{ ...styles.dot, background: dotColor, boxShadow: isOverdue ? `0 0 6px ${dotColor}` : "none" }} />
      <div style={styles.content}>
        {isOverdue && <div style={styles.urgentLabel}>OVERDUE</div>}
        {isDueToday && !isOverdue && <div style={styles.todayLabel}>DUE TODAY</div>}
        <div style={{ color: textColor, fontSize: 12 }}>{task.title}</div>
        {task.deadline && <div style={styles.meta}>{task.deadline}</div>}
        {task.description && <div style={styles.meta}>{task.description}</div>}
      </div>
    </div>
  );
});
const styles: Record<string, React.CSSProperties> = {
  item: { display: "flex", gap: 12, paddingLeft: 4, marginBottom: 14, position: "relative" },
  dot: { width: 8, height: 8, borderRadius: "50%", marginTop: 4, flexShrink: 0 },
  content: { flex: 1 },
  urgentLabel: { color: "rgba(255, 100, 100, 0.8)", fontSize: 9, fontWeight: 600, fontFamily: "var(--font-mono)", letterSpacing: 1, marginBottom: 2 },
  todayLabel: { color: "rgba(255, 180, 0, 0.7)", fontSize: 9, fontWeight: 600, fontFamily: "var(--font-mono)", letterSpacing: 1, marginBottom: 2 },
  meta: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 2 },
};
