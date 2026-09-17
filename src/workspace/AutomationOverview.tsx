import { useEffect, useState, useCallback } from 'react';
import { ArrowUpRight, Clock3, Mic, Pause, Play, RefreshCw, MessageSquare } from 'lucide-react';
import JarvisScene from '../components/3d/JarvisScene';
import { getCronJobs, getCronRuns, setCronJobStatus } from '../lib/commands';
import type { CronJobView, CronRunView } from '../lib/types';
import type { AssistantActivity } from './useAssistantActivity';
import { useWorkspace } from './WorkspaceContext';
import { useChatContext } from '../hooks/ChatContext';
import MessageRenderer from '../components/chat/MessageRenderer';
import { useUiLocale } from '../i18n';
function timeLabel(value: string | null, empty: string, locale: string) { if (!value) return empty; const d = new Date(value.includes('T') || /Z$/.test(value) ? value : value.replace(' ', 'T') + 'Z'); return isNaN(d.getTime()) ? value : d.toLocaleString(locale, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }
export default function AutomationOverview({ activity, onVoice, listening }: { activity: AssistantActivity; onVoice: () => void; listening: boolean }) {
  const { text, locale } = useUiLocale();
  const when = (value: string | null) => timeLabel(value, text('No run yet', '從未執行'), locale);
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
      try { const { next, latest, incomplete } = await refresh(); if (mounted) { setJobs(next); setRuns(latest); setError(incomplete ? text('Some run history could not be refreshed.', '部分執行紀錄未能更新。') : ''); } }
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
  const currentStatus = listening ? text('Listening', '聆聽中') : activity.status || (running.length ? text(`${running.length} routines running`, `${running.length} 項排程執行中`) : text('Standing by', '待命中'));
  const activityLevel = listening ? 'listening' : activity.state === 'speaking' ? 'active' : activity.state === 'thinking' || running.length ? 'processing' : 'idle';
  return <div className="automation-overview">
    <header className="workspace-heading"><div><h1>{text('Automation', '自動化')}</h1><p>{text('Your enabled routines, working in the background.', '已啟用的排程會在背景執行。')}</p></div><button className="workspace-button" onClick={() => open('cron')}><Clock3 size={16} />{text('Manage automations', '管理自動排程')}</button></header>
    {error && <div className="workspace-error" role="alert">{error}<button className="text-button" onClick={() => window.dispatchEvent(new Event('jarvis-automations-changed'))}><RefreshCw size={14} />{text('Retry', '重試')}</button></div>}
    <div className="automation-layout"><section className="automation-brain" aria-label={text('JARVIS automation overview', 'JARVIS 自動化概覽')}>
      <div className="automation-presence"><span className={`status-dot ${activityLevel !== 'idle' ? 'is-working' : ''}`} /><span>{loading ? text('Loading automations…', '正在載入自動排程…') : text(`${enabled.length} enabled`, `已啟用 ${enabled.length} 項`)}</span><span className="presence-separator" /><span>{currentStatus}</span></div>
      <div className="automation-scene"><JarvisScene contained minimalLabels activityLevel={activityLevel} assistantPhase={listening ? 'listening' : activity.phase} ttsAmplitudeRef={activity.tts} micAmplitudeRef={activity.mic} pendingToolCall={activity.tool} onToolCallConsumed={activity.clearTool} /></div>
      <div className="automation-command"><p>{activityLevel === 'idle' ? text('Ready for your next instruction.', '已準備好接收下一項工作指示。') : currentStatus}</p><div><button className="workspace-button primary" onClick={() => { const id = newTask(); chat.selectSession(id); }}><MessageSquare size={17} />{text('Give JARVIS a task', '向 JARVIS 提供工作指示')}</button><button className="workspace-button" onClick={onVoice} disabled={chat.busy}><Mic size={17} />{listening ? text('Finish listening', '結束語音輸入') : text('Speak', '語音輸入')}</button></div><small>{text('Only enabled routines run. Opening this view starts no new actions.', '系統只會執行已啟用的排程。開啟此頁面不會啟動新工作。')}</small></div>
    </section><aside className="automation-work"><div className="section-heading"><h2>{text('Enabled routines', '已啟用的排程')}</h2><span>{enabled.length}</span></div>
      <div className="automation-joblist">{loading ? <p className="workspace-empty">{text('Loading your routines…', '正在載入排程…')}</p> : !enabled.length ? <div className="workspace-empty"><p>{text('No routines enabled.', '目前沒有已啟用的排程。')}</p><button className="text-button" onClick={() => open('cron')}>{jobs.length ? text('Manage routines', '管理排程') : text('Create an automation', '建立自動排程')}<ArrowUpRight size={14} /></button></div> : enabled.map(job => { const run = runs[job.id]; return <div className={`automation-job ${selected === job.id ? 'is-selected' : ''}`} key={job.id}><button className="automation-job-main" onClick={() => setSelected(job.id)}><span className="automation-job-name">{job.name}</span><span className={`run-state ${run?.status === 'failed' ? 'failed' : ''}`}>{job.status === 'paused' ? text('Paused', '已暫停') : run?.status === 'running' ? text('Running', '執行中') : text('Enabled', '已啟用')}</span><small>{run?.status === 'failed' ? text('Last run failed · Review result', '上次執行失敗 · 請核對結果') : job.next_run ? `${text("Next", "下次")} ${when(job.next_run)}` : `${text("Last", "上次")} ${when(job.last_run)}`}</small></button><button className="icon-button" aria-label={`${job.status === 'active' ? text('Pause', '暫停') : text('Resume', '恢復')} ${job.name}`} disabled={busy !== null} onClick={() => void toggle(job)}>{job.status === 'active' ? <Pause size={15} /> : <Play size={15} />}</button></div>; })}</div>
      {selectedRun && <section className="automation-result"><div className="section-heading"><h2>{text('Last result', '最近結果')}</h2><span>{selectedRun.status}</span></div><small>{when(selectedRun.started_at)}</small><div><MessageRenderer content={selectedRun.error || selectedRun.result || (selectedRun.status === 'running' ? text('This routine is still running.', '此排程仍在執行中。') : text('No text output for this run.', '本次執行沒有文字結果。'))} /></div><button className="text-button" onClick={() => { writeValue('tool:cron', 'selectedJobId', selected); open('cron'); }}>{text("View run history", "查看執行紀錄")}<ArrowUpRight size={14} /></button></section>}
      {!selectedRun && enabled.length > 0 && <p className="automation-hint">{text('Select a routine to see its latest result. Your tools stay available while routines run.', '選取排程查看最近結果。排程執行期間，你仍可使用其他工具。')}</p>}
    </aside></div>
  </div>;
}
