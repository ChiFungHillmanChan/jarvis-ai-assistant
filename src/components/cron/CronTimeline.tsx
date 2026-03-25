import { useEffect, useState } from "react";
import type { CronRunView } from "../../lib/types";
import { getUpcomingRuns, getCronRuns } from "../../lib/commands";

interface CronTimelineProps {
  jobId: number;
  schedule: string;
}

function formatRelative(dateStr: string): string {
  const now = new Date();
  const target = new Date(dateStr);
  const nowDate = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const targetDate = new Date(
    target.getFullYear(),
    target.getMonth(),
    target.getDate()
  );
  const diffDays = Math.round(
    (targetDate.getTime() - nowDate.getTime()) / (1000 * 60 * 60 * 24)
  );
  if (diffDays === 0) return "today";
  if (diffDays === 1) return "in 1d";
  if (diffDays > 1) return `in ${diffDays}d`;
  if (diffDays === -1) return "1d ago";
  return `${Math.abs(diffDays)}d ago`;
}

function formatDate(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export default function CronTimeline({ jobId, schedule }: CronTimelineProps) {
  const [upcoming, setUpcoming] = useState<string[]>([]);
  const [runs, setRuns] = useState<CronRunView[]>([]);

  useEffect(() => {
    getUpcomingRuns(schedule, 3)
      .then(setUpcoming)
      .catch(() => setUpcoming([]));
  }, [schedule]);

  useEffect(() => {
    getCronRuns(jobId, 5)
      .then(setRuns)
      .catch(() => setRuns([]));
  }, [jobId]);

  return (
    <div style={styles.container}>
      {/* Left column: Upcoming Runs */}
      <div style={styles.column}>
        <div style={styles.columnHeader}>
          <span className="system-text" style={{ fontSize: 10 }}>
            UPCOMING RUNS
          </span>
        </div>
        {upcoming.length === 0 && (
          <div style={styles.emptyText}>No upcoming runs</div>
        )}
        {upcoming.map((run, i) => {
          const isFirst = i === 0;
          return (
            <div key={i} style={styles.timelineItem}>
              <div style={styles.dotColumn}>
                <span
                  style={{
                    ...styles.dot,
                    width: isFirst ? 10 : 6,
                    height: isFirst ? 10 : 6,
                    background: isFirst
                      ? "rgba(0, 180, 255, 0.8)"
                      : "rgba(0, 180, 255, 0.3)",
                    boxShadow: isFirst
                      ? "0 0 8px rgba(0, 180, 255, 0.5)"
                      : "none",
                    border: isFirst
                      ? "1px solid rgba(0, 180, 255, 0.6)"
                      : "1px solid rgba(0, 180, 255, 0.15)",
                  }}
                />
                {i < upcoming.length - 1 && <div style={styles.connector} />}
              </div>
              <div
                style={{
                  ...styles.itemContent,
                  opacity: isFirst ? 1 : 0.5,
                }}
              >
                <span style={styles.itemDate}>{formatDate(run)}</span>
                <span style={styles.itemRelative}>{formatRelative(run)}</span>
              </div>
            </div>
          );
        })}
      </div>

      {/* Right column: Recent Runs */}
      <div style={styles.column}>
        <div style={styles.columnHeader}>
          <span className="system-text" style={{ fontSize: 10 }}>
            RECENT RUNS
          </span>
        </div>
        {runs.length === 0 && (
          <div style={styles.emptyText}>No runs yet</div>
        )}
        {runs.map((run) => {
          const isSuccess =
            run.status === "completed" || run.status === "done";
          return (
            <div key={run.id} style={styles.runItem}>
              <div style={styles.runHeader}>
                <span
                  style={{
                    ...styles.statusBadge,
                    color: isSuccess
                      ? "rgba(16, 185, 129, 0.8)"
                      : "rgba(255, 100, 100, 0.8)",
                    borderColor: isSuccess
                      ? "rgba(16, 185, 129, 0.25)"
                      : "rgba(255, 100, 100, 0.25)",
                  }}
                >
                  {isSuccess ? "DONE" : "FAIL"}
                </span>
                <span style={styles.runTimestamp}>
                  {formatDate(run.started_at)}
                </span>
              </div>
              {run.result && (
                <div style={styles.runDetail}>{run.result}</div>
              )}
              {run.error && (
                <div style={styles.runError}>{run.error}</div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    display: "flex",
    gap: 24,
    padding: 16,
  },
  column: {
    flex: 1,
    minWidth: 0,
  },
  columnHeader: {
    marginBottom: 12,
    paddingBottom: 6,
    borderBottom: "1px solid rgba(0, 180, 255, 0.08)",
  },
  emptyText: {
    color: "rgba(0, 180, 255, 0.25)",
    fontSize: 11,
    fontStyle: "italic",
    padding: "8px 0",
  },
  timelineItem: {
    display: "flex",
    gap: 10,
    minHeight: 32,
  },
  dotColumn: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    width: 12,
    paddingTop: 4,
  },
  dot: {
    borderRadius: "50%",
    flexShrink: 0,
  },
  connector: {
    width: 1,
    flex: 1,
    background: "rgba(0, 180, 255, 0.1)",
    marginTop: 4,
    marginBottom: 4,
  },
  itemContent: {
    display: "flex",
    flexDirection: "column",
    gap: 2,
    paddingBottom: 8,
  },
  itemDate: {
    color: "rgba(0, 180, 255, 0.7)",
    fontSize: 11,
    fontFamily: "var(--font-mono)",
  },
  itemRelative: {
    color: "rgba(0, 180, 255, 0.35)",
    fontSize: 10,
    fontFamily: "var(--font-mono)",
  },
  runItem: {
    padding: "6px 0",
    borderBottom: "1px solid rgba(0, 180, 255, 0.05)",
  },
  runHeader: {
    display: "flex",
    alignItems: "center",
    gap: 8,
  },
  statusBadge: {
    fontSize: 9,
    fontFamily: "var(--font-mono)",
    letterSpacing: 0.5,
    border: "1px solid",
    borderRadius: 3,
    padding: "1px 5px",
  },
  runTimestamp: {
    color: "rgba(0, 180, 255, 0.4)",
    fontSize: 10,
    fontFamily: "var(--font-mono)",
  },
  runDetail: {
    color: "rgba(0, 180, 255, 0.45)",
    fontSize: 10,
    marginTop: 4,
    lineHeight: "1.3",
  },
  runError: {
    color: "rgba(255, 100, 100, 0.6)",
    fontSize: 10,
    marginTop: 4,
    lineHeight: "1.3",
  },
};
