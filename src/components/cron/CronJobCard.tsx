import { memo, useEffect, useState } from "react";
import type { CronJobView } from "../../lib/types";
import { getUpcomingRuns } from "../../lib/commands";

interface CronJobCardProps {
  job: CronJobView;
  isSelected: boolean;
  onSelect: () => void;
  onToggle: () => void;
  onDelete: () => void;
}

function formatCountdown(dateStr: string): string {
  const now = new Date();
  const target = new Date(dateStr);
  const diffMs = target.getTime() - now.getTime();
  if (diffMs < 0) return "overdue";
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return "< 1m";
  if (diffMin < 60) return `in ${diffMin}m`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `in ${diffHours}h`;
  const diffDays = Math.floor(diffHours / 24);
  return `in ${diffDays}d`;
}

const CronJobCard = memo(function CronJobCard({
  job,
  isSelected,
  onSelect,
  onToggle,
  onDelete,
}: CronJobCardProps) {
  const [nextRun, setNextRun] = useState<string | null>(null);
  const isActive = job.status === "active";

  useEffect(() => {
    getUpcomingRuns(job.schedule, 1)
      .then((runs) => {
        if (runs.length > 0) setNextRun(runs[0]);
      })
      .catch(() => {});
  }, [job.schedule]);

  return (
    <div
      onClick={onSelect}
      style={{
        ...styles.card,
        borderColor: isSelected
          ? "rgba(0, 180, 255, 0.5)"
          : "rgba(0, 180, 255, 0.12)",
        background: isSelected
          ? "rgba(0, 180, 255, 0.06)"
          : "rgba(0, 180, 255, 0.02)",
      }}
    >
      <div style={styles.header}>
        <div style={styles.headerLeft}>
          <span
            style={{
              ...styles.statusDot,
              background: isActive
                ? "rgba(16, 185, 129, 0.8)"
                : "rgba(255, 100, 100, 0.8)",
              boxShadow: isActive
                ? "0 0 6px rgba(16, 185, 129, 0.4)"
                : "0 0 6px rgba(255, 100, 100, 0.3)",
            }}
          />
          <span style={styles.name}>{job.name}</span>
        </div>
      </div>

      <div style={styles.schedule}>
        {job.description || job.schedule}
      </div>

      {job.description && (
        <div style={styles.cronExpr}>{job.schedule}</div>
      )}

      {nextRun && (
        <div style={styles.nextRun}>
          Next: {formatCountdown(nextRun)}
        </div>
      )}

      {isSelected && (
        <div style={styles.actions}>
          <button
            onClick={(e) => {
              e.stopPropagation();
              onToggle();
            }}
            style={styles.actionBtn}
          >
            {isActive ? "PAUSE" : "RESUME"}
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
            style={styles.deleteBtn}
          >
            DELETE
          </button>
        </div>
      )}
    </div>
  );
});

export default CronJobCard;

const styles: Record<string, React.CSSProperties> = {
  card: {
    border: "1px solid",
    borderRadius: 8,
    padding: 12,
    cursor: "pointer",
    transition: "border-color 0.2s, background 0.2s",
  },
  header: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: 6,
  },
  headerLeft: {
    display: "flex",
    alignItems: "center",
    gap: 8,
  },
  statusDot: {
    width: 6,
    height: 6,
    borderRadius: "50%",
    flexShrink: 0,
  },
  name: {
    color: "rgba(0, 180, 255, 0.85)",
    fontSize: 12,
    fontWeight: 500,
    fontFamily: "var(--font-mono)",
  },
  schedule: {
    color: "rgba(0, 180, 255, 0.55)",
    fontSize: 11,
    lineHeight: "1.4",
    marginBottom: 2,
  },
  cronExpr: {
    color: "rgba(0, 180, 255, 0.3)",
    fontSize: 9,
    fontFamily: "var(--font-mono)",
    letterSpacing: 0.5,
    marginTop: 2,
  },
  nextRun: {
    color: "rgba(0, 180, 255, 0.4)",
    fontSize: 10,
    fontFamily: "var(--font-mono)",
    marginTop: 6,
  },
  actions: {
    display: "flex",
    gap: 6,
    marginTop: 8,
    borderTop: "1px solid rgba(0, 180, 255, 0.08)",
    paddingTop: 8,
  },
  actionBtn: {
    background: "rgba(0, 180, 255, 0.06)",
    border: "1px solid rgba(0, 180, 255, 0.2)",
    borderRadius: 4,
    padding: "3px 8px",
    color: "rgba(0, 180, 255, 0.6)",
    fontSize: 9,
    fontFamily: "var(--font-mono)",
    cursor: "pointer",
    letterSpacing: 0.5,
  },
  deleteBtn: {
    background: "rgba(255, 100, 100, 0.04)",
    border: "1px solid rgba(255, 100, 100, 0.2)",
    borderRadius: 4,
    padding: "3px 8px",
    color: "rgba(255, 100, 100, 0.6)",
    fontSize: 9,
    fontFamily: "var(--font-mono)",
    cursor: "pointer",
    letterSpacing: 0.5,
  },
};
