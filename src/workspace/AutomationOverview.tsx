import { useEffect, useState, useCallback } from 'react';
import { ArrowUpRight, Clock3, Mic, Pause, Play, RefreshCw, MessageSquare } from 'lucide-react';
import JarvisScene from '../components/3d/JarvisScene';
import { getCronJobs, getCronRuns, setCronJobStatus } from '../lib/commands';
import type { CronJobView, CronRunView } from '../lib/types';
import type { AssistantActivity } from './useAssistantActivity';
import { useWorkspace } from './WorkspaceContext';
import { useChatContext } from '../hooks/ChatContext';
import MessageRenderer from '../components/chat/MessageRenderer';
function timeLabel(value: string | null) { if (!value) return 'No run yet'; const d = new Date(value.includes('T') || /Z$/.test(value) ? value : value.replace(' ', 'T') + 'Z'); return isNaN(d.getTime()) ? value : d.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }
export default function AutomationOverview({ activity, onVoice, listening }: { activity: AssistantActivity; onVoice: () => void; listening: boolean }) {
  const [jobs, setJobs] = useState<CronJobView[]>([]);
  const [runs, setRuns] = useState<Record<number, CronRunView>>({});
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<number | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const { open, newTask, writeValue } = useWorkspace();
  const chat = useChatContext();
  const refresh = useCallback(async () => {
    const next = await getCronJobs();
    const results = await Promise.allSettled(next.map(j => getCronRuns(j.id, 1)));
    const latest: Record<number, CronRunView> = {};
    results.forEach((r, i) => { if (r.status === 'fulfilled' && r.value[0]) latest[next[i].id] = r.value[0]; });
    return { next, latest, incomplete: results.some(r => r.status === 'rejected') };
  }, []);
  useEffect(() => {
    let mounted = true, fetching = false;
    async function load() {
      if (fetching) return; fetching = true;
      try { const { next, latest, incomplete } = await refresh(); if (mounted) { setJobs(next); setRuns(latest); setError(incomplete ? 'Some run history could not be refreshed.' : ''); } }
      catch (e) { if (mounted) setError(String(e)); }
      finally { fetching = false; if (mounted) setLoading(false); }
    }
    void load(); const timer = setInterval(() => { if (!document.hidden) void load(); }, 15000);
    const update = () => void load(); window.addEventListener('jarvis-automations-changed', update);
    return () => { mounted = false; clearInterval(timer); window.removeEventListener('jarvis-automations-changed', update); };
  }, [refresh]);
  async function toggle(job: CronJobView) {
    if (busy !== null) return; setBusy(job.id);
    try { await setCronJobStatus(job.id, job.status === 'active' ? 'paused' : 'active'); const { next, latest } = await refresh(); setJobs(next); setRuns(latest); setError(''); window.dispatchEvent(new Event('jarvis-automations-changed')); }
    catch (e) { setError(String(e)); } finally { setBusy(null); }
  }
  const enabled = jobs.filter(j => j.status === 'active');
  const running = Object.values(runs).filter(r => r.status === 'running');
  const selectedRun = selected === null ? undefined : runs[selected];
  const currentStatus = listening ? 'Listening' : activity.status || (running.length ? `${running.length} automation${running.length === 1 ? '' : 's'} running` : 'Standing by');
  const activityLevel = listening ? 'listening' : activity.state === 'speaking' ? 'active' : activity.state === 'thinking' || running.length ? 'processing' : 'idle';
  return <div className="automation-overview">
    <header className="workspace-heading"><div><h1>Automation</h1><p>Your enabled routines, working in the background.</p></div><button className="workspace-button" onClick={() => open('cron')}><Clock3 size={16} />Manage automations</button></header>
    {error && <div className="workspace-error" role="alert">{error}<button className="text-button" onClick={() => window.dispatchEvent(new Event('jarvis-automations-changed'))}><RefreshCw size={14} />Retry</button></div>}
    <div className="automation-layout"><section className="automation-brain" aria-label="JARVIS automation overview">
      <div className="automation-presence"><span className={`status-dot ${activityLevel !== 'idle' ? 'is-working' : ''}`} /><span>{loading ? 'Loading automations…' : `${enabled.length} enabled`}</span><span className="presence-separator" /><span>{currentStatus}</span></div>
      <div className="automation-scene"><JarvisScene contained minimalLabels activityLevel={activityLevel} assistantPhase={listening ? 'listening' : activity.phase} ttsAmplitudeRef={activity.tts} micAmplitudeRef={activity.mic} pendingToolCall={activity.tool} onToolCallConsumed={activity.clearTool} /></div>
      <div className="automation-command"><p>{activityLevel === 'idle' ? 'Ready for your next instruction.' : currentStatus}</p><div><button className="workspace-button primary" onClick={() => { const id = newTask(); chat.selectSession(id); }}><MessageSquare size={17} />Give JARVIS a task</button><button className="workspace-button" onClick={onVoice} disabled={chat.busy}><Mic size={17} />{listening ? 'Finish listening' : 'Speak'}</button></div><small>Only enabled routines run. Opening this view starts no new actions.</small></div>
    </section><aside className="automation-work"><div className="section-heading"><h2>Enabled routines</h2><span>{enabled.length}</span></div>
      <div className="automation-joblist">{loading ? <p className="workspace-empty">Loading your routines…</p> : !enabled.length ? <div className="workspace-empty"><p>No routines enabled.</p><button className="text-button" onClick={() => open('cron')}>{jobs.length ? "Manage routines" : "Create an automation"}<ArrowUpRight size={14} /></button></div> : enabled.map(job => { const run = runs[job.id]; return <div className={`automation-job ${selected === job.id ? 'is-selected' : ''}`} key={job.id}><button className="automation-job-main" onClick={() => setSelected(job.id)}><span className="automation-job-name">{job.name}</span><span className={`run-state ${run?.status === 'failed' ? 'failed' : ''}`}>{job.status === 'paused' ? 'Paused' : run?.status === 'running' ? 'Running' : 'Enabled'}</span><small>{run?.status === 'failed' ? 'Last run failed · Review result' : job.next_run ? `Next ${timeLabel(job.next_run)}` : `Last ${timeLabel(job.last_run)}`}</small></button><button className="icon-button" aria-label={`${job.status === 'active' ? 'Pause' : 'Resume'} ${job.name}`} disabled={busy !== null} onClick={() => void toggle(job)}>{job.status === 'active' ? <Pause size={15} /> : <Play size={15} />}</button></div>; })}</div>
      {selectedRun && <section className="automation-result"><div className="section-heading"><h2>Last result</h2><span>{selectedRun.status}</span></div><small>{timeLabel(selectedRun.started_at)}</small><div><MessageRenderer content={selectedRun.error || selectedRun.result || (selectedRun.status === 'running' ? 'This routine is still running.' : 'No text output for this run.')} /></div><button className="text-button" onClick={() => { writeValue('tool:cron', 'selectedJobId', selected); open('cron'); }}>View run history<ArrowUpRight size={14} /></button></section>}
      {!selectedRun && enabled.length > 0 && <p className="automation-hint">Select a routine to see its latest result. Your tools stay available while routines run.</p>}
    </aside></div>
  </div>;
}
