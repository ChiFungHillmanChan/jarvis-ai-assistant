import { useCallback, useEffect, useState } from "react";
import type { CronJobView, CronRunView } from "../lib/types";
import { getCronJobs, getCronRuns, getUpcomingRuns, setCronJobStatus } from "../lib/commands";
import ConversionFlow from "../components/cron/ConversionFlow";
import { useWorkspace, useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";

const HAS_ZONE = /(?:Z|[+-]\d{2}:?\d{2})$/;
function backendDate(value: string | null) {
  if (!value) return "Never";
  const parsed = HAS_ZONE.test(value) ? value : `${value.replace(" ", "T")}Z`;
  return new Date(parsed).toLocaleString();
}

export default function CronDashboard() {
  const [jobs, setJobs] = useState<CronJobView[]>([]);
  const [selectedId, setSelectedId] = useWorkspaceValue<number | null>("selectedJobId", null);
  const [runs, setRuns] = useState<CronRunView[]>([]);
  const [upcoming, setUpcoming] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [refreshCounter, setRefreshCounter] = useState(0);
  const [mutatingId, setMutatingId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || "Local time";
  const { state } = useWorkspace();
  const activeView = state.tabs.find(tab => tab.id === state.activeId)?.view;

  const fetchJobs = useCallback(async () => {
    setLoading(true); setError(null);
    try { setJobs(await getCronJobs()); setRefreshCounter(value => value + 1); } catch (e) { setError(String(e)); } finally { setLoading(false); }
  }, []);
  useEffect(() => { if (activeView === "cron") void fetchJobs(); }, [activeView, state.activeId, fetchJobs]);
  useEffect(() => {
    const changed = (event: Event) => {
      if ((event as CustomEvent<{ source?: string }>).detail?.source !== "cron-dashboard") void fetchJobs();
    };
    window.addEventListener("jarvis-automations-changed", changed);
    return () => window.removeEventListener("jarvis-automations-changed", changed);
  }, [fetchJobs]);
  const selected = jobs.find(job => job.id === selectedId) ?? null;
  useEffect(() => {
    if (!selected) { setRuns([]); setUpcoming([]); return; }
    let current = true; setDetailLoading(true); setError(null);
    Promise.all([getCronRuns(selected.id, 10), getUpcomingRuns(selected.schedule, 3)])
      .then(([nextRuns, future]) => { if (current) { setRuns(nextRuns); setUpcoming(future); } })
      .catch(e => { if (current) setError(String(e)); })
      .finally(() => { if (current) setDetailLoading(false); });
    return () => { current = false; };
  }, [selected?.id, selected?.schedule, selected?.status, refreshCounter]);

  async function toggle(job: CronJobView) {
    if (mutatingId !== null) return;
    setMutatingId(job.id); setError(null);
    try {
      await setCronJobStatus(job.id, job.status === "active" ? "paused" : "active");
      await fetchJobs();
      window.dispatchEvent(new CustomEvent("jarvis-automations-changed", { detail: { source: "cron-dashboard" } }));
    } catch (e) { setError(String(e)); await fetchJobs(); } finally { setMutatingId(null); }
  }
  const handleCreated = useCallback((job: CronJobView) => {
    setSelectedId(job.id);
    void fetchJobs().then(() => {
      window.dispatchEvent(new CustomEvent("jarvis-automations-changed", { detail: { source: "cron-dashboard" } }));
    });
  }, [fetchJobs, setSelectedId]);

  return <main className="tool-page automation-tool-page">
    <header className="tool-page__header"><div><p className="tool-page__eyebrow">Automations</p><h1>Schedules and run history</h1><p>Schedules run in {timezone}. Select an automation to inspect generated results.</p></div><button className="tool-button tool-button--quiet" onClick={fetchJobs} disabled={loading}>{loading ? "Refreshing…" : "Refresh"}</button></header>
    {error && <div className="tool-alert tool-alert--error" role="alert">{error}</div>}
    <ConversionFlow onJobCreated={handleCreated} />
    <div className="tool-split"><section className="tool-panel tool-list-pane"><div className="tool-panel__heading"><h2>Automations</h2><span>{jobs.length} total</span></div>
      {loading && jobs.length === 0 ? <p className="tool-empty">Loading automations…</p> : jobs.length === 0 ? <p className="tool-empty">No automations yet. Describe one above to create it.</p> : <div className="automation-table" role="table"><div className="automation-table__head" role="row"><span>Name</span><span>Schedule</span><span>Next run</span><span>Status</span></div>{jobs.map(job => <div className={`automation-row${selectedId === job.id ? " is-selected" : ""}`} role="row" key={job.id}><button className="automation-row__select" onClick={() => setSelectedId(job.id)}><strong>{job.name}</strong><small>Last: {backendDate(job.last_run)}</small></button><span>{job.description || job.schedule}</span><span>{job.status === "paused" ? "Paused" : job.next_run ? backendDate(job.next_run) : "Calculated from schedule"}</span><button className="tool-button tool-button--quiet" disabled={mutatingId !== null} onClick={() => toggle(job)}>{mutatingId === job.id ? "Updating…" : job.status === "active" ? "Pause" : "Enable"} <small>{job.status}</small></button></div>)}</div>}
    </section>
    <aside className="tool-panel tool-detail-pane">{selected ? <><div className="tool-panel__heading"><h2>{selected.name}</h2><span>{selected.status}</span></div><dl className="tool-details"><dt>Instructions</dt><dd>{selected.description || selected.action_type}</dd><dt>Schedule</dt><dd>{selected.schedule}</dd><dt>Last run</dt><dd>{backendDate(selected.last_run)}</dd><dt>{selected.status === "paused" ? "Schedule preview" : "Next runs"}</dt><dd>{detailLoading ? "Loading…" : upcoming.length ? `${upcoming.join(", ")}${selected.status === "paused" ? " (hypothetical while paused)" : ""}` : selected.status === "paused" ? "Paused" : "Unavailable"}</dd></dl><h3 className="tool-subheading">Run history</h3>{detailLoading ? <p className="tool-empty">Loading run history…</p> : runs.length === 0 ? <p className="tool-empty">This automation has no recorded runs.</p> : <div className="run-list">{runs.map(run => <article key={run.id} className="run-item"><div><strong>{run.status}</strong><time>{backendDate(run.started_at)}</time></div>{run.result && <p>{run.result}</p>}{run.error && <p className="run-item__error">{run.error}</p>}</article>)}</div>}</> : <p className="tool-empty">Select an automation to see its schedule and real run output.</p>}</aside></div>
  </main>;
}
