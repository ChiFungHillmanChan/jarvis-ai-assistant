import { memo } from "react";
import type { Task } from "../lib/types";
import TimelineItem from "./TimelineItem";
interface TimelineProps { tasks: Task[]; }
export default memo(function Timeline({ tasks }: TimelineProps) {
  return (
    <div style={styles.container}>
      <div className="label" style={{ marginBottom: 12 }}>TIMELINE</div>
      <div style={styles.line}>
        {tasks.length === 0
          ? <div style={styles.empty}>No tasks. All clear.</div>
          : tasks.map((task) => <TimelineItem key={task.id} task={task} />)}
      </div>
    </div>
  );
})
const styles: Record<string, React.CSSProperties> = {
  container: { flex: 1, overflowY: "auto", paddingRight: 8 },
  line: { borderLeft: "1px solid rgba(0, 180, 255, 0.15)", paddingLeft: 16, marginLeft: 4 },
  empty: { color: "rgba(0, 180, 255, 0.3)", fontSize: 12, fontStyle: "italic", padding: "20px 0" },
};
