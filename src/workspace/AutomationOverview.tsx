import { useEffect, useState, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { ArrowUpRight, Check, Clock3, Ear, Mic, Pause, Play, RefreshCw, MessageSquare } from 'lucide-react';
import JarvisScene from '../components/3d/JarvisScene';
import { disableWakeWord, downloadModel, enableWakeWord, getCronJobs, getCronRuns, getDailyBriefing, getTasks, getWakeWordStatus, setCronJobStatus, updateTask } from '../lib/commands';
import { cronRunNote, cronStatusLabel, schedulerText } from '../lib/cron-runs';
import { canSwitchListening, setAlwaysListening, voiceStateKind } from '../lib/always-listening';
import { showToast } from '../components/Toast';
import { followUps, localDate } from '../lib/follow-ups';
import type { CronJobView, CronRunView, DailyBrief, Task, VoiceState, WakeWordStatus } from '../lib/types';
import type { AssistantActivity } from './useAssistantActivity';
import { useWorkspace } from './WorkspaceContext';
import { useChatContext } from '../hooks/ChatContext';
import MessageRenderer from '../components/chat/MessageRenderer';
import { useUiLocale } from '../i18n';
import { chatStatusText } from '../lib/chat-status';
function timeLabel(value: string | null, empty: string, locale: string) { if (!value) return empty; const d = new Date(value.includes('T') || /Z$/.test(value) ? value : value.replace(' ', 'T') + 'Z'); return isNaN(d.getTime()) ? value : d.toLocaleString(locale, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }
const listeningApi = { status: getWakeWordStatus, download: () => downloadModel(false), enable: enableWakeWord, disable: disableWakeWord };
export default function AutomationOverview({ activity, onVoice, listening, voiceState }: { activity: AssistantActivity; onVoice: () => void; listening: boolean; voiceState: VoiceState }) {
  const { text, t, locale } = useUiLocale();
  const when = (value: string | null) => timeLabel(value, text('No run yet', '從未執行'), locale);
  const [jobs, setJobs] = useState<CronJobView[]>([]);
  const [runs, setRuns] = useState<Record<number, CronRunView>>({});
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<number | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [completing, setCompleting] = useState<number | null>(null);
  const [wake, setWake] = useState<WakeWordStatus | null>(null);
  const [wakeBusy, setWakeBusy] = useState(false);
  const [brief, setBrief] = useState<DailyBrief | null>(null);
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
      try { const [{ next, latest, incomplete }, openTasks] = await Promise.all([refresh(), getTasks().catch(() => null)]); if (mounted) { setJobs(next); setRuns(latest); if (openTasks) setTasks(openTasks); setError(incomplete ? text('Some run history could not be refreshed.', '部分執行紀錄未能更新。') : openTasks ? '' : text('Follow-ups could not be refreshed.', '未能更新待跟進工作。')); } }
      catch (e) { if (mounted) setError(String(e)); }
      finally { fetching = false; if (mounted) setLoading(false); }
    }
    void load(); const timer = setInterval(() => { if (!document.hidden) void load(); }, 15000);
    const update = () => void load(); window.addEventListener('jarvis-automations-changed', update);
    return () => { mounted = false; clearInterval(timer); window.removeEventListener('jarvis-automations-changed', update); };
  }, [refresh]);
  // Today's briefing. It is redrawn in place when the backend says it changed
  // (first shown, then again when the AI summary arrives), and re-read when
  // the page comes back so yesterday's brief does not linger past midnight.
  useEffect(() => {
    let mounted = true;
    const read = () => { void getDailyBriefing().then(next => { if (mounted) setBrief(next); }).catch(() => {}); };
    read();
    const off = listen<DailyBrief | null>('daily-briefing-updated', event => { if (mounted) setBrief(event.payload); });
    const visible = () => { if (!document.hidden) read(); };
    document.addEventListener('visibilitychange', visible);
    return () => { mounted = false; document.removeEventListener('visibilitychange', visible); void off.then(fn => fn()).catch(() => {}); };
  }, []);
  const voiceKind = voiceStateKind(voiceState);
  // Listening can start, stop or fail from here, Settings, a download or startup: re-read on every change of kind.
  useEffect(() => { void getWakeWordStatus().then(setWake).catch(() => {}); }, [voiceKind]);
  async function toggleAlwaysListening() {
    if (!wake || wakeBusy) return; setWakeBusy(true);
    try { setWake(await setAlwaysListening(!wake.running, listeningApi)); }
    catch (e) { showToast(String(e), 'error'); void getWakeWordStatus().then(setWake).catch(() => {}); }
    finally { setWakeBusy(false); }
  }
  const downloading = typeof voiceState === 'object' && 'ModelDownloading' in voiceState ? voiceState.ModelDownloading : null;
  async function toggle(job: CronJobView) {
    if (busy !== null) return; setBusy(job.id);
    try { await setCronJobStatus(job.id, job.status === 'active' ? 'paused' : 'active'); const { next, latest } = await refresh(); setJobs(next); setRuns(latest); setError(''); window.dispatchEvent(new Event('jarvis-automations-changed')); }
    catch (e) { setError(String(e)); } finally { setBusy(null); }
  }
  async function complete(task: Task) {
    if (task.id === null || completing !== null) return; setCompleting(task.id);
    try { await updateTask(task.id, 'completed'); setTasks(list => list.filter(t => t.id !== task.id)); }
    catch (e) { setError(String(e)); } finally { setCompleting(null); }
  }
  const today = localDate(new Date());
  const due = followUps(tasks, today);
  const dueLabel = (deadline: string) => deadline < today ? text('Overdue', '已逾期') : deadline === today ? text('Due today', '今天到期') : new Date(`${deadline}T00:00`).toLocaleDateString(locale, { month: 'short', day: 'numeric' });
  const enabled = jobs.filter(j => j.status === 'active');
  const running = Object.values(runs).filter(r => r.status === 'running');
  const selectedRun = selected === null ? undefined : runs[selected];
  const currentStatus = listening ? text('Listening', '聆聽中') : (activity.status ? chatStatusText(activity.status, text) : '') || (running.length ? text(`${running.length} routines running`, `${running.length} 項排程執行中`) : text('Standing by', '待命中'));
  const activityLevel = listening ? 'listening' : activity.state === 'speaking' ? 'active' : activity.state === 'thinking' || running.length ? 'processing' : 'idle';
  return <div className="automation-overview">
    <header className="workspace-heading"><div><h1>{text('Automation', '自動化')}</h1><p>{text('Your enabled routines, working in the background.', '已啟用的排程會在背景執行。')}</p></div><button className="workspace-button" onClick={() => open('cron')}><Clock3 size={16} />{text('Manage automations', '管理自動排程')}</button></header>
    {error && <div className="workspace-error" role="alert">{error}<button className="text-button" onClick={() => window.dispatchEvent(new Event('jarvis-automations-changed'))}><RefreshCw size={14} />{text('Retry', '重試')}</button></div>}
    <div className="automation-layout"><section className="automation-brain" aria-label={text('JARVIS automation overview', 'JARVIS 自動化概覽')}>
      <div className="automation-presence"><span className={`status-dot ${activityLevel !== 'idle' ? 'is-working' : ''}`} /><span>{loading ? text('Loading automations…', '正在載入自動排程…') : text(`${enabled.length} enabled`, `已啟用 ${enabled.length} 項`)}</span><span className="presence-separator" /><span>{currentStatus}</span></div>
      <div className="automation-scene"><JarvisScene contained minimalLabels activityLevel={activityLevel} assistantPhase={listening ? 'listening' : activity.phase} ttsAmplitudeRef={activity.tts} micAmplitudeRef={activity.mic} pendingToolCall={activity.tool} onToolCallConsumed={activity.clearTool} /></div>
      <div className="automation-command"><p>{activityLevel === 'idle' ? text('Ready for your next instruction.', '已準備好接收下一項工作指示。') : currentStatus}</p><div><button className="workspace-button primary" onClick={() => { const id = newTask(); chat.selectSession(id); }}><MessageSquare size={17} />{text('Give JARVIS a task', '向 JARVIS 提供工作指示')}</button><button className="workspace-button" onClick={onVoice} disabled={chat.busy}><Mic size={17} />{listening ? text('Finish listening', '結束語音輸入') : text('Speak', '語音輸入')}</button><button className={`workspace-button ${wake?.running ? 'is-on' : ''}`} aria-pressed={!!wake?.running} disabled={!canSwitchListening(wake, wakeBusy, voiceState)} onClick={() => void toggleAlwaysListening()}><Ear size={17} />{downloading !== null ? text(`Downloading speech model ${downloading}%`, `正在下載語音模型 ${downloading}%`) : wake?.running ? text('Always listening: on', '一直聆聽：開') : text('Always listening: off', '一直聆聽：關')}</button></div>{wake?.running ? <small>{text('Say "Hey Jarvis", then your request. What you say stays on this Mac until you say it; then your request goes to your chosen AI.', '先說「Hey Jarvis」，再說出指示。說出喚醒詞之前，你說的話只留在此 Mac；之後你的指示會傳送至你選擇的 AI。')}</small> : wake && !wake.model_downloaded && <small>{text('Turning it on downloads a speech model (about 148 MB) once.', '首次開啟會下載語音模型（約 148 MB）。')}</small>}<small>{text('Only enabled routines run. Opening this view starts no new actions.', '系統只會執行已啟用的排程。開啟此頁面不會啟動新工作。')}</small></div>
    </section><aside className="automation-work">{brief && <section className="automation-brief" aria-label={t('briefTitle')}><div className="section-heading"><h2>{t('briefTitle')}</h2><span>{when(brief.shown_at)}</span></div>
      {brief.summary && <div><MessageRenderer content={brief.summary} /></div>}
      <pre>{brief.local}</pre>
      {brief.summary_pending && <small role="status">{t('briefSummaryPending')}</small>}
      {brief.summary_error && <small>{t('briefSummaryFailed', { reason: brief.summary_error })}</small>}
      {(brief.trigger === 'catch_up' || brief.trigger === 'retry_interrupted') && <small>{t('briefShownLate')}</small>}</section>}
      {due.length > 0 && <section className="automation-followups" aria-label={text('Follow-ups', '待跟進')}><div className="section-heading"><h2>{text('Follow-ups', '待跟進')}</h2><span>{due.length}</span></div>
      {due.map(task => <div className="automation-job" key={task.id}><div className="automation-job-main"><span className="automation-job-name">{task.title}</span><span className={`run-state ${task.deadline! < today ? 'failed' : ''}`}>{dueLabel(task.deadline!)}</span></div><button className="icon-button" aria-label={text(`Mark ${task.title} as done`, `將「${task.title}」標為完成`)} disabled={completing !== null} onClick={() => void complete(task)}><Check size={15} /></button></div>)}</section>}
      <div className="section-heading"><h2>{text('Enabled routines', '已啟用的排程')}</h2><span>{enabled.length}</span></div>
      <div className="automation-joblist">{loading ? <p className="workspace-empty">{text('Loading your routines…', '正在載入排程…')}</p> : !enabled.length ? <div className="workspace-empty"><p>{text('No routines enabled.', '目前沒有已啟用的排程。')}</p><button className="text-button" onClick={() => open('cron')}>{jobs.length ? text('Manage routines', '管理排程') : text('Create an automation', '建立自動排程')}<ArrowUpRight size={14} /></button></div> : enabled.map(job => { const run = runs[job.id]; return <div className={`automation-job ${selected === job.id ? 'is-selected' : ''}`} key={job.id}><button className="automation-job-main" onClick={() => setSelected(job.id)}><span className="automation-job-name">{job.name}</span><span className={`run-state ${run?.status === 'failed' ? 'failed' : ''}`}>{job.status === 'paused' ? text('Paused', '已暫停') : run?.status === 'running' ? text('Running', '執行中') : text('Enabled', '已啟用')}</span><small>{run?.status === 'failed' ? text('Last run failed · Review result', '上次執行失敗 · 請核對結果') : job.next_run ? `${text("Next", "下次")} ${when(job.next_run)}` : `${text("Last", "上次")} ${when(job.last_run)}`}</small></button><button className="icon-button" aria-label={`${job.status === 'active' ? text('Pause', '暫停') : text('Resume', '恢復')} ${job.name}`} disabled={busy !== null} onClick={() => void toggle(job)}>{job.status === 'active' ? <Pause size={15} /> : <Play size={15} />}</button></div>; })}</div>
      {selectedRun && <section className="automation-result"><div className="section-heading"><h2>{text('Last result', '最近結果')}</h2><span>{cronStatusLabel(selectedRun.status, text)}</span></div><small>{when(selectedRun.started_at)}</small><div><MessageRenderer content={cronRunNote(selectedRun, text) || schedulerText(selectedRun.error || selectedRun.result || '', text) || (selectedRun.status === 'running' ? text('This routine is still running.', '此排程仍在執行中。') : text('No text output for this run.', '本次執行沒有文字結果。'))} /></div><button className="text-button" onClick={() => { writeValue('tool:cron', 'selectedJobId', selected); open('cron'); }}>{text("View run history", "查看執行紀錄")}<ArrowUpRight size={14} /></button></section>}
      {!selectedRun && enabled.length > 0 && <p className="automation-hint">{text('Select a routine to see its latest result. Your tools stay available while routines run.', '選取排程查看最近結果。排程執行期間，你仍可使用其他工具。')}</p>}
    </aside></div>
  </div>;
}
