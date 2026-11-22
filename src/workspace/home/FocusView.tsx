import { useState, type ReactNode } from 'react';
import Button from '../../ui/Button';
import { useUiLocale } from '../../i18n';
import { updateTask } from '../../lib/commands';
import { mailItemToCalendar, mailUpdateItem, type MailItem } from '../../lib/mail';
import { parseSender } from '../../lib/mail-presentation';
import { localDate } from '../../lib/follow-ups';
import type { CalendarEventView, Task } from '../../lib/types';
import type { FocusItem } from './home-logic';
import { canWaitLine, remindTomorrow } from './home-items';
import DayList from './DayList';

type Text = (en: string, zh: string) => string;

export interface FocusProps {
  items: FocusItem[];
  /** All that need the user today; items holds the top three. */
  total: number;
  tasks: Map<number, Task>;
  mail: Map<number, MailItem>;
  events: CalendarEventView[] | null;
  filed: number | null;
  /** The setup card when not ready; it replaces the cards. */
  setup: ReactNode;
  askBar: ReactNode;
  onChanged(): void;
  onDraftReply(item: MailItem): void;
  onOpenEmail(item: MailItem): void;
  onReviewMail(): void;
}

function dueLabel(due: string | null, text: Text, locale: string): string {
  if (!due) return '';
  const today = localDate(new Date());
  if (due < today) return text('Overdue', '已逾期');
  if (due === today) return text('Due today', '今天到期');
  return new Date(`${due}T00:00`).toLocaleDateString(locale, { month: 'short', day: 'numeric' });
}

const from = (item: MailItem, text: Text) => [parseSender(item.sender).name, item.subject || text('(No subject)', '（無主旨）')].filter(Boolean).join(' · ');

function FocusCard({ n, focus, task, item, onChanged, onDraftReply, onOpenEmail }: { n: number; focus: FocusItem; task?: Task; item?: MailItem } & Pick<FocusProps, 'onChanged' | 'onDraftReply' | 'onOpenEmail'>) {
  const { text, locale } = useUiLocale();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  async function act(run: () => Promise<unknown>) {
    if (pending) return;
    setPending(true); setError('');
    try { await run(); onChanged(); }
    catch (e) { setError(text(`That did not work: ${String(e)}`, `未能完成：${String(e)}`)); }
    finally { setPending(false); }
  }
  const due = dueLabel(focus.due, text, locale);
  let meta = due, quote = '';
  let actions: ReactNode = null;
  if (focus.kind === 'follow-up' && task && task.id !== null) {
    const taskId = task.id;
    meta = [due, task.description ?? ''].filter(Boolean).join(' · ');
    actions = <Button kind="primary" disabled={pending} onClick={() => act(() => updateTask(taskId, 'completed'))}>{text('Mark done', '標為完成')}</Button>;
  } else if (focus.kind === 'reply' && item) {
    meta = [due, from(item, text)].filter(Boolean).join(' · ');
    quote = item.evidence_verified ? item.evidence : '';
    actions = <>
      <Button kind="primary" disabled={pending} onClick={() => onDraftReply(item)}>{text('Draft a reply', '草擬回覆')}</Button>
      <Button kind="secondary" disabled={pending} onClick={() => onOpenEmail(item)}>{text('Open email', '開啟郵件')}</Button>
      <Button kind="quiet" disabled={pending} onClick={() => act(() => mailUpdateItem(item.id, 'open', remindTomorrow(new Date())))}>{text('Remind me tomorrow', '明天提醒我')}</Button>
    </>;
  } else if (focus.kind === 'task' && item) {
    meta = [due, from(item, text)].filter(Boolean).join(' · ');
    actions = <>
      <Button kind="primary" disabled={pending} onClick={() => act(() => mailUpdateItem(item.id, 'done', item.remind_at))}>{text('Mark done', '標為完成')}</Button>
      {item.due_date && !item.calendar_event_id && <Button kind="secondary" disabled={pending} onClick={() => act(() => mailItemToCalendar(item.id))}>{text('Add to calendar', '加入日曆')}</Button>}
    </>;
  }
  return <article className="home-card">
    <span className={`home-card-n${focus.overdue ? ' is-overdue' : ''}`} aria-hidden="true">{n}</span>
    <div className="home-card-body">
      <h2>{focus.title}</h2>
      {meta && <p className={focus.overdue ? 'is-overdue' : undefined}>{meta}</p>}
      {quote && <blockquote>{text(`“${quote}”`, `「${quote}」`)}</blockquote>}
      {actions && <div className="home-card-actions">{actions}</div>}
      {error && <p className="home-error" role="alert">{error}</p>}
    </div>
  </article>;
}

/** Focus: up to three things that need the user today, and the day beside them. */
export default function FocusView({ items, total, tasks, mail, events, filed, setup, askBar, onChanged, onDraftReply, onOpenEmail, onReviewMail }: FocusProps) {
  const { text } = useUiLocale();
  return <div className="home-focus">
    <div className="home-focus-main">
      {setup ?? (items.length
        ? items.map((focus, i) => <FocusCard key={`${focus.kind}:${focus.id}`} n={i + 1} focus={focus}
            task={focus.kind === 'follow-up' ? tasks.get(focus.id) : undefined} item={focus.kind === 'follow-up' ? undefined : mail.get(focus.id)}
            onChanged={onChanged} onDraftReply={onDraftReply} onOpenEmail={onOpenEmail} />)
        : <section className="home-card home-card-empty">
            <h2>{text('Nothing needs you today', '今天沒有需要你處理的事項')}</h2>
            {filed !== null && <p>{canWaitLine(filed, text)}</p>}
          </section>)}
      {!setup && total > items.length && <p className="home-more">{text(`Showing the top 3 of ${total}`, `顯示最重要的 3 項，共 ${total} 項`)}</p>}
      {askBar}
    </div>
    {!setup && <aside className="home-focus-side">
      <section className="home-panel">
        <h2>{text('Your day', '今天的行程')}</h2>
        <DayList events={events} compact />
      </section>
      <section className="home-panel">
        <h2>{text('Can wait', '可以稍後處理')}</h2>
        {filed !== null && <p>{canWaitLine(filed, text)}</p>}
        <button type="button" className="text-button" onClick={onReviewMail}>{text('Review sorted mail', '查看已分類郵件')}</button>
      </section>
    </aside>}
  </div>;
}
