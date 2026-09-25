import { WORKDAY_END_HOUR, WORKDAY_START_HOUR, type CalendarEntry, type DueItem, type TodaySummary } from "../../lib/calendar-layout";
import { formatTime } from "./TimeGrid";

interface Props {
  summary: TodaySummary;
  now: Date;
  loading: boolean;
  locale: string;
  text: (english: string, traditionalChinese: string) => string;
  onSelect: (entry: CalendarEntry) => void;
  onSelectMail: (id: number) => void;
}

function duration(minutes: number, text: Props["text"]) {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (!hours) return text(`${rest}m`, `${rest} 分鐘`);
  if (!rest) return text(`${hours}h`, `${hours} 小時`);
  return text(`${hours}h ${rest}m`, `${hours} 小時 ${rest} 分鐘`);
}

/** Facts about today, worked out in code: agenda, now/next, free working time, and what is due. */
export default function TodayPanel({ summary, now, loading, locale, text, onSelect, onSelectMail }: Props) {
  const { entries, current, next, free, due } = summary;
  const minutesTo = (value: Date) => Math.max(1, Math.round((value.getTime() - now.getTime()) / 60_000));
  const dueLabel = (item: DueItem) => item.overdueDays > 0
    ? text(item.overdueDays === 1 ? "Overdue 1 day" : `Overdue ${item.overdueDays} days`, `已逾期 ${item.overdueDays} 天`)
    : item.time ? text(`Due by ${item.time}`, `${item.time} 前到期`) : text("Due today", "今天到期");
  const workStart = new Date(2000, 0, 1, WORKDAY_START_HOUR).toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });
  const workEnd = new Date(2000, 0, 1, WORKDAY_END_HOUR).toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });

  return <div className="cal-today">
    <div className="tool-panel__heading"><h2>{text("Today", "今日")}</h2><span>{now.toLocaleDateString(locale, { weekday: "short", month: "short", day: "numeric" })}</span></div>
    <p className="cal-today__lead">
      <strong>{text(entries.length === 1 ? "1 event" : `${entries.length} events`, `${entries.length} 項活動`)}</strong>
      {current ? <span>{text(`Now: ${current.title}, until ${formatTime(current.end, locale)}`, `現正進行：${current.title}，至 ${formatTime(current.end, locale)}`)}</span> : null}
      {next ? <span>{text(`Next: ${next.title} at ${formatTime(next.start, locale)} (in ${duration(minutesTo(next.start), text)})`, `下一項：${formatTime(next.start, locale)} ${next.title}（${duration(minutesTo(next.start), text)}後）`)}</span>
        : !current && <span>{loading ? text("Loading…", "載入中…") : text("Nothing else scheduled today.", "今天沒有其他已排定的活動。")}</span>}
    </p>

    <section className="cal-today__section">
      <h3>{text("Schedule", "日程")}</h3>
      {entries.length ? <ul>{entries.map(entry => <li key={entry.id}><button className={`cal-today__row${entry.source === "mail" ? " cal-today__row--mail" : ""}`} onClick={() => onSelect(entry)}>
        <time>{entry.allDay ? text("All day", "全日") : formatTime(entry.start, locale)}</time><span>{entry.title}</span>
      </button></li>)}</ul> : <p className="cal-today__empty">{text("No events today.", "今天沒有活動。")}</p>}
    </section>

    <section className="cal-today__section">
      <h3>{text("Free time", "空閒時間")}</h3>
      {free.length ? <ul>{free.map(block => <li key={block.start.getTime()} className="cal-today__free">
        <time>{formatTime(block.start, locale)} – {formatTime(block.end, locale)}</time><span>{duration(Math.round((block.end.getTime() - block.start.getTime()) / 60_000), text)}</span>
      </li>)}</ul> : <p className="cal-today__empty">{text(`No free block of 30 minutes or more left between ${workStart} and ${workEnd}.`, `${workStart} 至 ${workEnd} 之間已沒有 30 分鐘或以上的空檔。`)}</p>}
    </section>

    <section className="cal-today__section">
      <h3>{text("Due", "到期事項")}</h3>
      {due.length ? <ul>{due.map(item => <li key={`${item.source}-${item.id}`}>
        {item.source === "mail"
          ? <button className="cal-today__row cal-today__row--mail" onClick={() => onSelectMail(item.id)}><span>{item.title}</span><small className={item.overdueDays ? "is-overdue" : undefined}>{text("Email", "郵件")} · {dueLabel(item)}</small></button>
          : <div className="cal-today__row"><span>{item.title}</span><small className={item.overdueDays ? "is-overdue" : undefined}>{text("Task", "工作")} · {dueLabel(item)}</small></div>}
      </li>)}</ul> : <p className="cal-today__empty">{text("Nothing due today.", "今天沒有到期事項。")}</p>}
    </section>
  </div>;
}
