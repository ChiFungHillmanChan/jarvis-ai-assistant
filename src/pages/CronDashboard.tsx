import { useState, useEffect, useCallback } from "react";
import type { CronJobView } from "../lib/types";
import { getCronJobs, toggleCronJob, deleteCronJob } from "../lib/commands";
import ConversionFlow from "../components/cron/ConversionFlow";
import CronJobCard from "../components/cron/CronJobCard";
import CronTimeline from "../components/cron/CronTimeline";

export default function CronDashboard() {
  const [jobs, setJobs] = useState<CronJobView[]>([]);
  const [selectedJobId, setSelectedJobId] = useState<number | null>(null);

  const fetchJobs = useCallback(() => {
    getCronJobs().then(setJobs).catch(console.error);
  }, []);

  useEffect(() => {
    fetchJobs();
  }, [fetchJobs]);

  const selectedJob = jobs.find((j) => j.id === selectedJobId);

  const handleJobCreated = useCallback((job: CronJobView) => {
    setJobs((prev) => [...prev, job]);
    setSelectedJobId(job.id);
  }, []);

  const handleToggle = useCallback(
    async (id: number) => {
      await toggleCronJob(id);
      fetchJobs();
    },
    [fetchJobs]
  );

  const handleDelete = useCallback(
    async (id: number) => {
      await deleteCronJob(id);
      if (selectedJobId === id) setSelectedJobId(null);
      fetchJobs();
    },
    [selectedJobId, fetchJobs]
  );

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <span className="system-text">CRON SCHEDULING</span>
      </div>
      <ConversionFlow onJobCreated={handleJobCreated} />
      <div style={styles.grid}>
        {jobs.map((job) => (
          <CronJobCard
            key={job.id}
            job={job}
            isSelected={selectedJobId === job.id}
            onSelect={() =>
              setSelectedJobId(selectedJobId === job.id ? null : job.id)
            }
            onToggle={() => handleToggle(job.id)}
            onDelete={() => handleDelete(job.id)}
          />
        ))}
        {jobs.length === 0 && (
          <div style={styles.empty}>No cron jobs yet. Create one above.</div>
        )}
      </div>
      {selectedJob && (
        <div style={styles.timeline}>
          <div style={styles.timelineHeader}>
            <span className="system-text">
              {selectedJob.name.toUpperCase()}
            </span>
          </div>
          <CronTimeline jobId={selectedJob.id} schedule={selectedJob.schedule} />
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: { padding: 24, maxWidth: 800, margin: "0 auto" },
  header: { marginBottom: 8 },
  grid: {
    display: "grid",
    gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
    gap: 12,
    padding: "16px 0",
  },
  empty: {
    color: "rgba(0, 180, 255, 0.3)",
    fontSize: 12,
    fontStyle: "italic",
    gridColumn: "1 / -1",
    textAlign: "center" as const,
    padding: 40,
  },
  timeline: {
    marginTop: 8,
    background: "rgba(0, 180, 255, 0.02)",
    border: "1px solid rgba(0, 180, 255, 0.1)",
    borderRadius: 8,
  },
  timelineHeader: {
    padding: "12px 16px",
    borderBottom: "1px solid rgba(0, 180, 255, 0.08)",
  },
};
