import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CalendarEventView } from "../lib/types";
import { syncCalendar } from "../lib/commands";

export default function CalendarPage() {
  const { data: events, refetch } = useTauriCommand<CalendarEventView[]>("get_events", { days: 7 });
  const [syncing, setSyncing] = useState(false);

  async function handleSync() {
    setSyncing(true);
    try { await syncCalendar(); refetch(); } catch (e) { console.error(e); }
    finally { setSyncing(false); }
  }

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <span className="system-text">CALENDAR -- NEXT 7 DAYS</span>
        <button onClick={handleSync} disabled={syncing} style={styles.syncBtn}>
          {syncing ? "SYNCING..." : "SYNC NOW"}
        </button>
      </div>
      <div style={styles.list}>
        {!events || events.length === 0 ? (
          <div style={styles.empty}>No events synced. Connect Google in Settings, then sync.</div>
        ) : events.map((event) => (
          <div key={event.id} style={styles.eventItem}>
            <div style={styles.timeBlock}>
              <div style={styles.time}>{new Date(event.start_time).toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" })}</div>
              <div style={styles.time}>{new Date(event.start_time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} - {new Date(event.end_time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</div>
            </div>
            <div style={styles.eventInfo}>
              <div style={styles.eventTitle}>{event.summary}</div>
              {event.location && <div style={styles.eventMeta}>{event.location}</div>}
              {event.attendees && <div style={styles.eventMeta}>{event.attendees}</div>}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 24, height: "100%", display: "flex", flexDirection: "column" },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 },
  syncBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 10 },
  list: { flex: 1, overflowY: "auto" },
  empty: { color: "rgba(0, 180, 255, 0.3)", fontSize: 12, fontStyle: "italic", padding: 20 },
  eventItem: { display: "flex", gap: 16, padding: "12px 0", borderBottom: "1px solid rgba(0, 180, 255, 0.08)" },
  timeBlock: { width: 100, flexShrink: 0 },
  time: { color: "rgba(0, 180, 255, 0.5)", fontSize: 10, fontFamily: "var(--font-mono)" },
  eventInfo: { flex: 1 },
  eventTitle: { color: "rgba(0, 180, 255, 0.8)", fontSize: 13, fontWeight: 500, marginBottom: 4 },
  eventMeta: { color: "rgba(0, 180, 255, 0.4)", fontSize: 11 },
};
