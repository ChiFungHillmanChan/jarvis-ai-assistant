import { useEffect, useMemo, useRef } from "react";
import { dayKey, dayStart, layoutAllDay, layoutTimed, type CalendarEntry } from "../../lib/calendar-layout";
import { dateFormat } from "../../lib/date-format";

const HOUR_HEIGHT = 48;
const HOURS = Array.from({ length: 24 }, (_, hour) => hour);

interface Props {
  days: string[];
  entries: CalendarEntry[];
  now: Date;
  selectedId: string | null;
  locale: string;
  text: (english: string, traditionalChinese: string) => string;
  onSelect: (entry: CalendarEntry) => void;
  onOpenDay: (day: string) => void;
}

export const formatTime = (value: Date, locale: string) => dateFormat(locale, { hour: "numeric", minute: "2-digit" }).format(value);

function offsetLabel(now: Date) {
  const minutes = -now.getTimezoneOffset();
  const hours = Math.floor(Math.abs(minutes) / 60);
  const rest = Math.abs(minutes) % 60;
  return `GMT${minutes < 0 ? "-" : "+"}${String(hours).padStart(2, "0")}${rest ? `:${String(rest).padStart(2, "0")}` : ""}`;
}

/** Google-style day and week view: all-day lane on top, 24-hour grid below. */
export default function TimeGrid({ days, entries, now, selectedId, locale, text, onSelect, onOpenDay }: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const today = dayKey(now);
  const blocks = useMemo(() => layoutTimed(entries, days), [entries, days]);
  const bars = useMemo(() => layoutAllDay(entries, days), [entries, days]);
  const lanes = bars.reduce((count, bar) => Math.max(count, bar.lane + 1), 0);
  const showsToday = days.includes(today);
  const nowTop = (now.getHours() + now.getMinutes() / 60) * HOUR_HEIGHT;

  // Open at 8am, or an hour before now when that is later.
  useEffect(() => {
    const hour = showsToday ? Math.max(8, new Date().getHours() - 1) : 8;
    scroller.current?.scrollTo({ top: hour * HOUR_HEIGHT });
  }, [days[0], days.length, showsToday]);

  const hourLabel = (hour: number) => dateFormat(locale, { hour: "numeric" }).format(new Date(2000, 0, 1, hour));
  const entryClass = (entry: CalendarEntry, base: string) => `${base}${entry.source === "mail" ? ` ${base}--mail` : ""}${entry.id === selectedId ? " is-selected" : ""}`;

  // One scroll container with a sticky header keeps the columns aligned with the scrollbar.
  return <div className="cal-grid" ref={scroller} style={{ ["--cal-days" as string]: days.length }}>
    <div className="cal-grid__top">
      <div className="cal-grid__head">
        <span className="cal-grid__zone">{offsetLabel(now)}</span>
        {days.map(day => {
          const date = dayStart(day);
          return <button key={day} className={`cal-grid__day${day === today ? " is-today" : ""}`} onClick={() => onOpenDay(day)} aria-label={dateFormat(locale, { weekday: "long", month: "long", day: "numeric" }).format(date)}>
            <span>{dateFormat(locale, { weekday: "short" }).format(date)}</span>
            <strong>{date.getDate()}</strong>
          </button>;
        })}
      </div>
      <div className="cal-grid__allday" style={{ gridTemplateRows: `repeat(${Math.max(lanes, 1)}, 24px)` }}>
        <span className="cal-grid__allday-label" style={{ gridRow: `1 / span ${Math.max(lanes, 1)}` }}>{text("All day", "全日")}</span>
        {bars.map(bar => <button key={bar.entry.id} className={entryClass(bar.entry, "cal-bar")} style={{ gridColumn: `${bar.startCol + 2} / span ${bar.span}`, gridRow: bar.lane + 1 }} onClick={() => onSelect(bar.entry)} title={bar.entry.title}>{bar.entry.title}</button>)}
      </div>
    </div>
    <div className="cal-grid__canvas" style={{ height: 24 * HOUR_HEIGHT }}>
      <div className="cal-grid__hours" aria-hidden="true">{HOURS.map(hour => <span key={hour} style={{ top: hour * HOUR_HEIGHT }}>{hour === 0 ? "" : hourLabel(hour)}</span>)}</div>
      {days.map(day => <div key={day} className={`cal-grid__col${day === today ? " is-today" : ""}`}>
        {blocks.filter(block => block.day === day).map(block => {
          const height = Math.max((block.endMin - block.startMin) / 60 * HOUR_HEIGHT, 20);
          return <button key={block.entry.id} className={entryClass(block.entry, "cal-block")} onClick={() => onSelect(block.entry)} title={block.entry.title}
            style={{ top: block.startMin / 60 * HOUR_HEIGHT, height, left: `calc(${block.column / block.columns * 100}% + 2px)`, width: `calc(${100 / block.columns}% - 4px)` }}>
            <strong>{block.entry.title}</strong>
            {height >= 34 && <small>{formatTime(block.entry.start, locale)} – {formatTime(block.entry.end, locale)}</small>}
            {height >= 56 && block.entry.location && <small>{block.entry.location}</small>}
          </button>;
        })}
        {day === today && <div className="cal-grid__now" style={{ top: nowTop }} role="presentation" />}
      </div>)}
    </div>
  </div>;
}
