import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { RefreshCw } from 'lucide-react';
import { getCronJobs, getDailyBriefing, getSettings, getTasks, getTodaysEvents, googleAccounts, type GoogleAccounts } from '../lib/commands';
import { currentAi } from '../lib/ai-choice';
import { mailListItems, mailWorkspace, type MailItem, type MailTriageStatus } from '../lib/mail';
import { localDate } from '../lib/follow-ups';
import { nameToShow } from '../lib/user-name';
import { onDisplayNameChanged } from '../lib/account';
import type { CalendarEventView, CronJobView, DailyBrief, Task, VoiceState } from '../lib/types';
import type { AssistantActivity } from './useAssistantActivity';
import { useWorkspace } from './WorkspaceContext';
import { useChatContext } from '../hooks/ChatContext';
import { useSettingsDialog } from '../settings/SettingsContext';
import { useUiLocale } from '../i18n';
import { focusItems, homeState, homeView, type HomeView } from './home/home-logic';
import { draftReplyPrompt, filedCount, focusInputs, followUpsDue, openReplies } from './home/home-items';
import HomeHeader from './home/HomeHeader';
import BriefingLine from './home/BriefingLine';
import AskBar from './home/AskBar';
import FocusView from './home/FocusView';
import OverviewView from './home/OverviewView';
import HomeSetupCard from './home/HomeSetupCard';
import './home/home.css';

interface HomeData { jobs: CronJobView[]; tasks: Task[]; items: MailItem[]; events: CalendarEventView[] | null; triage: MailTriageStatus | null; google: GoogleAccounts | null; googleRead: boolean; aiReady: boolean | null }
const EMPTY: HomeData = { jobs: [], tasks: [], items: [], events: null, triage: null, google: null, googleRead: false, aiReady: null };

/** The home page: loads what both views show, then renders the chosen one. */
export default function AutomationOverview({ activity, onVoice, listening, voiceState }: { activity: AssistantActivity; onVoice: () => void; listening: boolean; voiceState: VoiceState }) {
  const { text } = useUiLocale();
  const { open, newTask, writeValue, dispatch } = useWorkspace();
  const chat = useChatContext();
  const { open: settingsOpen, openSettings } = useSettingsDialog();
  const [data, setData] = useState<HomeData>(EMPTY);
  const [failed, setFailed] = useState(false);
  const [brief, setBrief] = useState<DailyBrief | null>(null);
  const [name, setName] = useState<string | null>(null);
  const [view, setView] = useState<HomeView>(homeView(null));
  const alive = useRef(true);
  const fetching = useRef(false);

  // Everything here is read from this Mac; a failed read keeps what was shown and says so.
  const load = useCallback(async () => {
    if (fetching.current) return; fetching.current = true;
    try {
      const [jobs, tasks, items, events, mail, google, ai] = await Promise.allSettled([getCronJobs(), getTasks(), mailListItems(), getTodaysEvents(), mailWorkspace(), googleAccounts(), currentAi()]);
      if (!alive.current) return;
      const ok = <T,>(r: PromiseSettledResult<T>) => r.status === 'fulfilled' ? r.value : undefined;
      setData(prev => ({
        jobs: ok(jobs) ?? prev.jobs, tasks: ok(tasks) ?? prev.tasks, items: ok(items) ?? prev.items, events: ok(events) ?? prev.events,
        triage: ok(mail)?.triage ?? prev.triage, google: ok(google) ?? prev.google, googleRead: prev.googleRead || google.status === 'fulfilled',
        aiReady: ai.status === 'fulfilled' ? !!ai.value : prev.aiReady,
      }));
      setFailed([jobs, tasks, items, events].some(r => r.status === 'rejected'));
    } finally { fetching.current = false; }
  }, []);

  useEffect(() => {
    alive.current = true;
    void load(); const timer = setInterval(() => { if (!document.hidden) void load(); }, 15000);
    const update = () => void load(); window.addEventListener('jarvis-automations-changed', update);
    return () => { alive.current = false; clearInterval(timer); window.removeEventListener('jarvis-automations-changed', update); };
  }, [load]);
  // Connecting Google or choosing an AI happens in the Settings dialog: re-read when it closes.
  useEffect(() => { if (!settingsOpen) void load(); }, [settingsOpen, load]);

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

  // The name and the saved view, from settings; the name again when it is changed.
  useEffect(() => {
    let live = true;
    const read = (first: boolean) => void getSettings().then(s => { if (!live) return; setName(nameToShow(s.values)); if (first) setView(homeView(s.values.home_view)); }).catch(() => {});
    read(true);
    const off = onDisplayNameChanged(() => read(false));
    return () => { live = false; off(); };
  }, []);

  // A new task with the typed text as its first message. send() goes to the
  // selected session, which is the new one only after the next render, and
  // this page unmounts once the task tab is active. So the session is
  // selected first, and the tab is opened (as newTask does) as the message is
  // sent. While another task runs, the text waits in the new task's composer.
  const pending = useRef<{ id: string; prompt: string } | null>(null);
  const startTask = useCallback((prompt: string) => {
    if (chat.busy) {
      const id = newTask();
      writeValue(`task:${id}`, 'assistantDraft', prompt);
      chat.selectSession(id);
      return;
    }
    const id = crypto.randomUUID();
    pending.current = { id, prompt };
    chat.selectSession(id);
  }, [newTask, chat.busy, chat.selectSession, writeValue]);
  useEffect(() => {
    const ask = pending.current;
    if (!ask || chat.sessionId !== ask.id) return;
    pending.current = null;
    void chat.send(ask.prompt, 'tools');
    dispatch({ type: 'open', view: 'chat', id: `task:${ask.id}`, sessionId: ask.id, title: ask.prompt.slice(0, 50) });
  }, [chat.sessionId, chat.send, dispatch]);

  const openEmail = (item: MailItem) => { writeValue('tool:email', 'mailMessageId', item.message_id); open('email'); };
  const seeFollowUps = () => { writeValue('tool:email', 'mailView', 'due'); open('email'); };

  const now = new Date();
  const today = localDate(now);
  // An unread reading counts as ready: a failed check never shows a wrong setup card.
  const state = homeState({ googleConnected: !data.googleRead || data.google?.main.state === 'connected', aiReady: data.aiReady ?? true });
  const ready = state === 'ready';
  const items = useMemo(() => focusItems(focusInputs(data.tasks, data.items, today, now), today), [data.tasks, data.items, today]);
  const taskMap = useMemo(() => new Map(data.tasks.filter(t => t.id !== null).map(t => [t.id!, t])), [data.tasks]);
  const mailMap = useMemo(() => new Map(data.items.map(i => [i.id, i])), [data.items]);
  const setup = ready ? null : <HomeSetupCard state={state} />;
  const askBar = <AskBar onAsk={startTask} onVoice={onVoice} listening={listening} voiceState={voiceState} activity={activity}
    placeholder={view === 'focus' ? text('Anything else? Ask JARVIS…', '還有其他事情嗎？向 JARVIS 提問…') : text('Ask JARVIS to reply, schedule or find something…', '請 JARVIS 回覆、安排或尋找資料…')} />;

  return <div className="home" data-view={view}>
    <HomeHeader name={name} view={view} onView={setView} needCount={ready ? items.length : null} />
    <BriefingLine brief={brief} />
    {failed && <div className="workspace-error" role="alert">{text('Part of your home page could not be refreshed.', '部分主頁內容未能更新。')}<button className="text-button" onClick={() => void load()}><RefreshCw size={14} />{text('Retry', '重試')}</button></div>}
    {view === 'focus'
      ? <FocusView items={items} tasks={taskMap} mail={mailMap} events={data.events} filed={filedCount(data.triage)} setup={setup} askBar={askBar}
          onChanged={() => void load()} onDraftReply={item => startTask(draftReplyPrompt(item, text))} onOpenEmail={openEmail} onReviewMail={() => open('email')} />
      : <OverviewView replies={openReplies(data.items, now)} gmail={data.google?.main.gmail ?? true} followUps={followUpsDue(data.tasks, today)} events={data.events} jobs={data.jobs}
          setup={setup} askBar={askBar} open={open} onSeeFollowUps={seeFollowUps} onConnectGmail={() => openSettings('connections')}
          onDraftReply={item => startTask(draftReplyPrompt(item, text))} onChanged={() => void load()} />}
  </div>;
}
