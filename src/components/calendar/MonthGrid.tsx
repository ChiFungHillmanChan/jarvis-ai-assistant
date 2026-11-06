import { useMemo } from "react";
import { dayKey, dayStart, entriesByDay, type CalendarEntry } from "../../lib/calendar-layout";
import { dateFormat } from "../../lib/date-format";
import { formatTime } from "./TimeGrid";

const VISIBLE_PER_DAY = 3;

interface Props {
  days: string[];
  month: string;
  entries: CalendarEntry[];
  now: Date;
  selectedId: string | null;
  locale: string;
  text: (english: string, traditionalChinese: string) => string;
  onSelect: (entry: CalendarEntry) => void;
  onOpenDay: (day: string) => void;
}

/** Six-week month grid; a day with more than three items links to its Day view. */
export default function MonthGrid({ days, month, entries, now, selectedId, locale, text, onSelect, onOpenDay }: Props) {
  const today = dayKey(now);
  const byDay = useMemo(() => {
    const grouped = entriesByDay(entries, days);
    for (const items of grouped.values()) items.sort((a, b) => Number(b.allDay) - Number(a.allDay) || a.start.getTime() - b.start.getTime());
    return grouped;
  }, [days, entries]);

  return <div className="cal-month">
    <div className="cal-month__weekdays" aria-hidden="true">{days.slice(0, 7).map(day => <span key={day}>{dateFormat(locale, { weekday: "short" }).format(dayStart(day))}</span>)}</div>
    <div className="cal-month__cells">
      {days.map(day => {
        const date = dayStart(day);
        const items = byDay.get(day) ?? [];
        const hidden = items.length - VISIBLE_PER_DAY;
        const label = date.getDate() === 1 ? dateFormat(locale, { month: "short", day: "numeric" }).format(date) : String(date.getDate());
        return <div key={day} className={`cal-month__cell${day.slice(0, 7) !== month ? " is-outside" : ""}${day === today ? " is-today" : ""}`}>
          <button className="cal-month__date" onClick={() => onOpenDay(day)} aria-label={dateFormat(locale, { weekday: "long", month: "long", day: "numeric" }).format(date)}>{label}</button>
          {items.slice(0, hidden > 0 ? VISIBLE_PER_DAY - 1 : VISIBLE_PER_DAY).map(entry => <button key={entry.id}
            className={`cal-chip${entry.allDay ? " cal-chip--allday" : ""}${entry.source === "mail" ? " cal-chip--mail" : ""}${entry.id === selectedId ? " is-selected" : ""}`}
            onClick={() => onSelect(entry)} title={entry.title}>
            {!entry.allDay && dayKey(entry.start) === day && <time>{formatTime(entry.start, locale)}</time>}{entry.title}
          </button>)}
          {hidden > 0 && <button className="cal-month__more" onClick={() => onOpenDay(day)}>{text(`${hidden + 1} more`, `另有 ${hidden + 1} 項`)}</button>}
        </div>;
      })}
    </div>
  </div>;
}
