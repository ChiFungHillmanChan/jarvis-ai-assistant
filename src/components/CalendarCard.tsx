import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CalendarEventView } from "../lib/types";

export default function CalendarCard() {
  const { data: events } = useTauriCommand<CalendarEventView[]>("get_todays_events");
  const count = events?.length ?? 0;
  const next = events?.[0];
  return (
    <div className="panel" style={styles.card}>
      <div className="label">CALENDAR</div>
      <div style={styles.value}>{count}</div>
      <div style={styles.detail}>{count === 0 ? "no meetings today" : `meeting${count !== 1 ? "s" : ""} today`}</div>
      {next && (
        <div style={styles.next}>
          Next: {next.summary}<br />
          {new Date(next.start_time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </div>
      )}
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  card: { padding: 12 },
  value: { color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 },
  detail: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 },
  next: { color: "rgba(0, 180, 255, 0.5)", fontSize: 9, marginTop: 8, borderTop: "1px solid rgba(0, 180, 255, 0.1)", paddingTop: 6 },
};
