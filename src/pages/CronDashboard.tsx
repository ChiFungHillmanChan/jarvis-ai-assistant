import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CronJobView, CronRunView } from "../lib/types";
import { createCustomCron, deleteCronJob, toggleCronJob } from "../lib/commands";

export default function CronDashboard() {
  const { data: jobs } = useTauriCommand<CronJobView[]>("get_cron_jobs");
  const [selectedJob, setSelectedJob] = useState<number | null>(null);
  const [newJobDesc, setNewJobDesc] = useState("");
  const [creating, setCreating] = useState(false);

  async function handleCreateJob() {
    if (!newJobDesc.trim() || creating) return;
    setCreating(true);
    try {
      await createCustomCron(newJobDesc);
      setNewJobDesc("");
      window.location.reload();
    } catch (e) {
      console.error(e);
    } finally {
      setCreating(false);
    }
  }

  return (
    <div style={styles.container}>
      <div className="system-text" style={{ marginBottom: 16 }}>CRON JOBS</div>
      <div style={styles.grid}>
        <div style={styles.jobList}>
          <div style={{ marginBottom: 12, display: "flex", gap: 6 }}>
            <input type="text" value={newJobDesc} onChange={(e) => setNewJobDesc(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCreateJob()}
              placeholder="e.g. Every Monday check email for spam..."
              style={{ flex: 1, background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "6px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)", outline: "none" }} />
            <button onClick={handleCreateJob} disabled={creating}
              style={{ background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: creating ? "wait" : "pointer", fontFamily: "var(--font-mono)", fontSize: 10, whiteSpace: "nowrap" }}>
              {creating ? "CREATING..." : "+ NEW JOB"}
            </button>
          </div>
          {jobs?.map((job) => (
            <button key={job.id} onClick={() => setSelectedJob(job.id)}
              style={{ ...styles.jobCard, borderColor: selectedJob === job.id ? "rgba(0, 180, 255, 0.4)" : "rgba(0, 180, 255, 0.12)" }}>
              <div style={styles.jobHeader}>
                <span style={styles.jobName}>{job.name}</span>
                <span style={{ ...styles.jobStatus, color: job.status === "active" ? "rgba(16, 185, 129, 0.7)" : "rgba(255, 100, 100, 0.7)" }}>
                  {job.status.toUpperCase()}
                </span>
              </div>
              <div style={styles.jobMeta}>Schedule: {job.schedule}</div>
              {job.last_run && <div style={styles.jobMeta}>Last run: {new Date(job.last_run).toLocaleString()}</div>}
              <div style={{ display: "flex", gap: 4, marginTop: 6 }}>
                <button onClick={(e) => { e.stopPropagation(); toggleCronJob(job.id).then(() => window.location.reload()); }}
                  style={{ background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "2px 6px", color: "rgba(0, 180, 255, 0.5)", fontSize: 8, fontFamily: "var(--font-mono)", cursor: "pointer" }}>
                  {job.status === "active" ? "PAUSE" : "RESUME"}
                </button>
                <button onClick={(e) => { e.stopPropagation(); deleteCronJob(job.id).then(() => window.location.reload()); }}
                  style={{ background: "transparent", border: "1px solid rgba(255, 100, 100, 0.2)", borderRadius: 4, padding: "2px 6px", color: "rgba(255, 100, 100, 0.5)", fontSize: 8, fontFamily: "var(--font-mono)", cursor: "pointer" }}>
                  DELETE
                </button>
              </div>
            </button>
          ))}
        </div>
        <div style={styles.runHistory}>
          {selectedJob ? <RunHistory jobId={selectedJob} /> : (
            <div style={styles.placeholder}>Select a job to view run history</div>
          )}
        </div>
      </div>
    </div>
  );
}

function RunHistory({ jobId }: { jobId: number }) {
  const { data: runs } = useTauriCommand<CronRunView[]>("get_cron_runs", { job_id: jobId, limit: 20 });
  if (!runs || runs.length === 0) {
    return <div style={styles.placeholder}>No runs yet</div>;
  }
  return (
    <div>
      <div className="label" style={{ marginBottom: 12 }}>RUN HISTORY</div>
      {runs.map((run) => (
        <div key={run.id} style={styles.runItem}>
          <div style={styles.runHeader}>
            <span style={{ ...styles.runStatus,
              color: run.status === "completed" ? "rgba(16, 185, 129, 0.7)" : run.status === "failed" ? "rgba(255, 100, 100, 0.7)" : "rgba(255, 180, 0, 0.7)"
            }}>{run.status.toUpperCase()}</span>
            <span style={styles.runTime}>{new Date(run.started_at).toLocaleString()}</span>
          </div>
          {run.result && <div style={styles.runDetail}>{run.result}</div>}
          {run.error && <div style={{ ...styles.runDetail, color: "rgba(255, 100, 100, 0.7)" }}>{run.error}</div>}
        </div>
      ))}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 24, height: "100%", overflowY: "auto" },
  grid: { display: "flex", gap: 16, height: "calc(100% - 40px)" },
  jobList: { width: 300, display: "flex", flexDirection: "column", gap: 8, overflowY: "auto" },
  jobCard: { background: "rgba(0, 180, 255, 0.02)", border: "1px solid", borderRadius: 8, padding: 12, cursor: "pointer", textAlign: "left" as const, width: "100%" },
  jobHeader: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6 },
  jobName: { color: "rgba(0, 180, 255, 0.8)", fontSize: 12, fontWeight: 500 },
  jobStatus: { fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1 },
  jobMeta: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 2 },
  runHistory: { flex: 1, overflowY: "auto" },
  placeholder: { color: "rgba(0, 180, 255, 0.3)", fontSize: 12, fontStyle: "italic", padding: 20 },
  runItem: { borderBottom: "1px solid rgba(0, 180, 255, 0.08)", padding: "10px 0" },
  runHeader: { display: "flex", justifyContent: "space-between", alignItems: "center" },
  runStatus: { fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1 },
  runTime: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10 },
  runDetail: { color: "rgba(0, 180, 255, 0.5)", fontSize: 11, marginTop: 4 },
};
