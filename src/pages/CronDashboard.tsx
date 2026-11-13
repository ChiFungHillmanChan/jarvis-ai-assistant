import { useCallback, useEffect, useState } from "react";
import type { CronJobView, CronRunView } from "../lib/types";
import { getCronJobs, getCronRuns, getUpcomingRuns, runCronJobNow, setCronJobStatus } from "../lib/commands";
import { cronRunNote, cronStatusLabel, cronTriggerLabel, schedulerText } from "../lib/cron-runs";
import ConversionFlow from "../components/cron/ConversionFlow";
import { useWorkspace, useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";
import { useUiLocale } from "../i18n";

const HAS_ZONE = /(?:Z|[+-]\d{2}:?\d{2})$/;
function backendDate(value: string | null, locale: string) {
  if (!value) return locale === "zh-HK" ? "從未執行" : "Never";
  const parsed = HAS_ZONE.test(value) ? value : `${value.replace(" ", "T")}Z`;
  return new Date(parsed).toLocaleString(locale);
}

export default function CronDashboard() {
  const { locale, text } = useUiLocale();
  const statusLabel = (status: string) => cronStatusLabel(status, text);
  const [jobs, setJobs] = useState<CronJobView[]>([]);
  const [selectedId, setSelectedId] = useWorkspaceValue<number | null>("selectedJobId", null);
  const [runs, setRuns] = useState<CronRunView[]>([]);
  const [upcoming, setUpcoming] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [refreshCounter, setRefreshCounter] = useState(0);
  const [mutatingId, setMutatingId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || text("Local time", "本機時間");
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
  // A run started here is outside the schedule: it does not change which
  // scheduled times count as done.
  async function runNow(job: CronJobView) {
    if (mutatingId !== null) return;
    setMutatingId(job.id); setError(null);
    try {
      await runCronJobNow(job.id);
      // The command returns once the run has started; show it, then look
      // again when a short run would have finished.
      await fetchJobs();
      setTimeout(() => { void fetchJobs(); }, 4000);
    } catch (e) { setError(schedulerText(String(e), text)); } finally { setMutatingId(null); }
  }
  const handleCreated = useCallback((job: CronJobView) => {
    setSelectedId(job.id);
    void fetchJobs().then(() => {
      window.dispatchEvent(new CustomEvent("jarvis-automations-changed", { detail: { source: "cron-dashboard" } }));
    });
  }, [fetchJobs, setSelectedId]);

  return <main className="tool-page automation-tool-page">
    <header className="tool-page__header"><div><p className="tool-page__eyebrow">{text("Automations", "自動排程")}</p><h1>{text("Schedules and run history", "排程與執行紀錄")}</h1><p>{locale === "zh-HK" ? `排程按 ${timezone} 執行。選取自動排程以查看產生的結果。` : `Schedules run in ${timezone}. Select an automation to inspect generated results.`}</p></div><button className="tool-button tool-button--quiet" onClick={fetchJobs} disabled={loading}>{loading ? text("Refreshing…", "重新整理中…") : text("Refresh", "重新整理")}</button></header>
    {error && <div className="tool-alert tool-alert--error" role="alert">{error}</div>}
    <ConversionFlow onJobCreated={handleCreated} />
    <div className="tool-split"><section className="tool-panel tool-list-pane"><div className="tool-panel__heading"><h2>{text("Automations", "自動排程")}</h2><span>{locale === "zh-HK" ? `共 ${jobs.length} 項` : `${jobs.length} total`}</span></div>
      {loading && jobs.length === 0 ? <p className="tool-empty">{text("Loading automations…", "正在載入自動排程…")}</p> : jobs.length === 0 ? <p className="tool-empty">{text("No automations yet. Describe one above to create it.", "目前沒有自動排程。可在上方描述要建立的排程。")}</p> : <div className="automation-table" role="table"><div className="automation-table__head" role="row"><span>{text("Name", "名稱")}</span><span>{text("Schedule", "排程")}</span><span>{text("Next run", "下次執行")}</span><span>{text("Status", "狀態")}</span></div>{jobs.map(job => <div className={`automation-row${selectedId === job.id ? " is-selected" : ""}`} role="row" key={job.id}><button className="automation-row__select" onClick={() => setSelectedId(job.id)}><strong>{job.name}</strong><small>{text("Last", "上次執行")}：{backendDate(job.last_run, locale)}</small></button><span>{job.description || job.schedule}</span><span>{job.status === "paused" ? text("Paused", "已暫停") : job.next_run ? backendDate(job.next_run, locale) : text("Calculated from schedule", "按排程計算")}</span><button className="tool-button tool-button--quiet" disabled={mutatingId !== null} onClick={() => toggle(job)}>{mutatingId === job.id ? text("Updating…", "更新中…") : job.status === "active" ? text("Pause", "暫停") : text("Enable", "啟用")} <small>{statusLabel(job.status)}</small></button></div>)}</div>}
    </section>
    <aside className="tool-panel tool-detail-pane">{selected ? <><div className="tool-panel__heading"><h2>{selected.name}</h2><span>{statusLabel(selected.status)}</span></div><dl className="tool-details"><dt>{text("Instructions", "指示")}</dt><dd>{selected.description || selected.action_type}</dd><dt>{text("Schedule", "排程")}</dt><dd>{selected.schedule}</dd><dt>{text("Last run", "上次執行")}</dt><dd>{backendDate(selected.last_run, locale)}</dd><dt>{selected.status === "paused" ? text("Schedule preview", "排程預覽") : text("Next runs", "未來執行時間")}</dt><dd>{detailLoading ? text("Loading…", "載入中…") : upcoming.length ? `${upcoming.join(", ")}${selected.status === "paused" ? text(" (hypothetical while paused)", "（暫停期間的預覽）") : ""}` : selected.status === "paused" ? text("Paused", "已暫停") : text("Unavailable", "未能取得")}</dd></dl><button className="tool-button tool-button--quiet" disabled={mutatingId !== null} onClick={() => void runNow(selected)}>{text("Run now", "立即執行")}</button><h3 className="tool-subheading">{text("Run history", "執行紀錄")}</h3>{detailLoading ? <p className="tool-empty">{text("Loading run history…", "正在載入執行紀錄…")}</p> : runs.length === 0 ? <p className="tool-empty">{text("This automation has no recorded runs.", "此自動排程尚無執行紀錄。")}</p> : <div className="run-list">{runs.map(run => { const note = cronRunNote(run, text); const trigger = cronTriggerLabel(run.trigger, text); const late = run.slot && (run.trigger === "catch_up" || run.trigger === "retry_interrupted") ? text(` (due ${backendDate(run.slot, locale)})`, `（原定 ${backendDate(run.slot, locale)}）`) : ""; return <article key={run.id} className="run-item"><div><strong>{statusLabel(run.status)}{trigger && <small> · {trigger}{late}</small>}</strong><time>{backendDate(run.started_at, locale)}</time></div>{note ? <p>{note}</p> : <>{run.result && <p>{schedulerText(run.result, text)}</p>}{run.error && <p className="run-item__error">{schedulerText(run.error, text)}</p>}</>}</article>; })}</div>}</> : <p className="tool-empty">{text("Select an automation to see its schedule and real run output.", "選取自動排程以查看時間表及實際執行結果。")}</p>}</aside></div>
  </main>;
}
