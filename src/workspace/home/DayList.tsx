import { useUiLocale } from '../../i18n';
import type { CalendarEventView } from '../../lib/types';
import { eventTime, nextEventId, sortEvents } from './home-items';

/** Today's calendar events, the next one to start marked. Used by both views. */
export default function DayList({ events, compact = false }: { events: CalendarEventView[] | null; compact?: boolean }) {
  const { text, locale } = useUiLocale();
  if (events === null) return <p className="home-empty">{text('Loading your calendar…', '正在載入日曆…')}</p>;
  if (!events.length) return <p className="home-empty">{text('Nothing on your calendar today.', '今天的日曆沒有活動。')}</p>;
  const next = nextEventId(events, new Date());
  return <ul className={`home-day${compact ? ' is-compact' : ''}`}>
    {sortEvents(events).map(e => <li key={e.id} className={e.id === next ? 'is-next' : undefined}>
      <span className="home-day-time">{eventTime(e, locale, text)}</span>
      <span className="home-day-title">
        {e.summary || text('(No title)', '（無標題）')}
        {!compact && (e.location || e.id === next) && <small>{[e.id === next ? text('Next', '下一個') : '', e.location ?? ''].filter(Boolean).join(' · ')}</small>}
      </span>
    </li>)}
  </ul>;
}
