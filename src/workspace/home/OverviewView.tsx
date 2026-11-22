import { useState, type ReactNode } from 'react';
import Button from '../../ui/Button';
import { useUiLocale } from '../../i18n';
import { updateTask } from '../../lib/commands';
import type { MailItem } from '../../lib/mail';
import { parseSender } from '../../lib/mail-presentation';
import { localDate } from '../../lib/follow-ups';
import type { CalendarEventView, CronJobView, Task } from '../../lib/types';
import type { WorkspaceView } from '../model';
import { runningSummary } from './home-items';
import DayList from './DayList';

const SHOWN = 3;

export interface OverviewProps {
  replies: MailItem[];
  /** False when Google is connected without Gmail: the column says where to turn it on. */
  gmail: boolean;
  followUps: Task[];
  events: CalendarEventView[] | null;
  jobs: CronJobView[];
  setup: ReactNode;
  askBar: ReactNode;
  open(view: WorkspaceView): void;
  onSeeFollowUps(): void;
  onConnectGmail(): void;
  onDraftReply(item: MailItem): void;
  onChanged(): void;
}

function Column({ title, count, children, seeAll, onSeeAll }: { title: string; count?: number; children: ReactNode; seeAll: string; onSeeAll(): void }) {
  return <section className="home-column">
    <h2>{title}{count ? <span className="home-count">{count}</span> : null}</h2>
    {children}
    <button type="button" className="text-button home-see-all" onClick={onSeeAll}>{seeAll}</button>
  </section>;
}

/** Overview: what needs a reply, what is due, and today's calendar, side by side. */
export default function OverviewView({ replies, gmail, followUps, events, jobs, setup, askBar, open, onSeeFollowUps, onConnectGmail, onDraftReply, onChanged }: OverviewProps) {
  const { text } = useUiLocale();
  const [completing, setCompleting] = useState<number | null>(null);
  const [error, setError] = useState('');
  const today = localDate(new Date());
  const running = runningSummary(jobs, text);

  async function complete(task: Task) {
    if (task.id === null || completing !== null) return;
    setCompleting(task.id); setError('');
    try { await updateTask(task.id, 'completed'); onChanged(); }
    catch (e) { setError(text(`Could not mark it done: ${String(e)}`, `未能標為完成：${String(e)}`)); }
    finally { setCompleting(null); }
  }
  const dueLabel = (deadline: string) => deadline.slice(0, 10) < today ? text('Overdue', '已逾期') : text('Today', '今天');

  return <div className="home-overview">
    {setup}
    {askBar}
    {!setup && <>
      {error && <p className="home-error" role="alert">{error}</p>}
      <div className="home-columns">
        <Column title={text('Needs your reply', '需要你回覆')} count={replies.length} seeAll={text('See all mail', '查看所有郵件')} onSeeAll={() => open('email')}>
          {!gmail ? <div className="home-empty">
            <p>{text('Turn on Gmail to see the emails that need a reply.', '開啟 Gmail，即可查看需要回覆的郵件。')}</p>
            <button type="button" className="text-button" onClick={onConnectGmail}>{text('Open Connections', '開啟連接設定')}</button>
          </div> : !replies.length ? <p className="home-empty">{text('No emails need a reply.', '沒有需要回覆的郵件。')}</p>
          : <ul className="home-rows">{replies.slice(0, SHOWN).map(item => <li key={item.id}>
            <strong>{parseSender(item.sender).name || item.subject}</strong>
            <span>{item.title}</span>
            <Button kind="secondary" onClick={() => onDraftReply(item)}>{text('Draft reply', '草擬回覆')}</Button>
          </li>)}</ul>}
        </Column>
        <Column title={text('Follow-ups due', '到期的待跟進事項')} count={followUps.length} seeAll={text('See all follow-ups', '查看所有待跟進事項')} onSeeAll={onSeeFollowUps}>
          {!followUps.length ? <p className="home-empty">{text('No follow-ups due today.', '今天沒有到期的待跟進事項。')}</p>
          : <ul className="home-rows">{followUps.slice(0, SHOWN).map(task => <li key={task.id} className="home-check">
            <input type="checkbox" checked={false} disabled={completing !== null} onChange={() => void complete(task)}
              aria-label={text(`Mark "${task.title}" done`, `將「${task.title}」標為完成`)} />
            <span>{task.title}</span>
            <span className={task.deadline!.slice(0, 10) < today ? 'home-due is-overdue' : 'home-due'}>{dueLabel(task.deadline!)}</span>
          </li>)}</ul>}
        </Column>
        <Column title={text('Today', '今天')} seeAll={text('Open calendar', '開啟日曆')} onSeeAll={() => open('calendar')}>
          <DayList events={events} />
        </Column>
      </div>
      <footer className="home-running">
        <span className={`status-dot${running ? ' is-on' : ''}`} aria-hidden="true" />
        <span>{running ?? text('No automations are on.', '目前沒有已啟用的自動排程。')}</span>
        <span className="grow" />
        <button type="button" className="text-button" onClick={() => open('cron')}>{text('Manage automations', '管理自動排程')}</button>
      </footer>
    </>}
  </div>;
}
