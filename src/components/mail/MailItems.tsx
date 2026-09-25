import { useEffect, useState } from "react";
import * as mail from "../../lib/mail";
import type { MailItem, MailTriage } from "../../lib/mail";
import { categoryLabel, defaultRemindAt, dueBucket, dueBucketLabel, DUE_BUCKETS, formatDue, itemKindLabel, parseSender, type Text } from "../../lib/mail-presentation";

/** A coloured dot plus the category name. Colour is never the only signal. */
export function CategoryTag({ category, text, compact = false }: { category: string; text: Text; compact?: boolean }) {
  const label = categoryLabel(category, text);
  return <span className="mail-category" data-category={category} title={label}><i aria-hidden="true" />{compact ? <span className="visually-hidden">{label}</span> : label}</span>;
}

type ItemActions = {
  text: Text;
  locale: string;
  onChanged: (item: MailItem) => void;
  onError: (message: string) => void;
  onOpenSource?: (messageId: number) => void;
};

export function MailItemRow({ item, text, locale, onChanged, onError, onOpenSource }: ItemActions & { item: MailItem }) {
  const [pending, setPending] = useState("");
  const [remindDraft, setRemindDraft] = useState<string | null>(null);
  const closed = item.status !== "open";
  async function act(label: string, action: () => Promise<MailItem>) {
    if (pending) return;
    setPending(label);
    try { onChanged(await action()); }
    catch (e) { onError(String(e)); }
    finally { setPending(""); }
  }
  const when = formatDue(item.due_date, item.due_time, locale);
  const until = item.end_time ? `–${item.end_time}` : "";
  return <li className={`mail-item${closed ? " is-closed" : ""}`} data-kind={item.kind}>
    <input type="checkbox" checked={item.status === "done"} disabled={!!pending || item.status === "dismissed"} aria-label={text(`Mark "${item.title}" done`, `將「${item.title}」標記為完成`)}
      onChange={e => act("status", () => mail.mailUpdateItem(item.id, e.target.checked ? "done" : "open", item.remind_at))} />
    <div className="mail-item-main">
      <p className="mail-item-title">{item.title}</p>
      <p className="mail-item-when"><span className="mail-item-kind">{itemKindLabel(item.kind, text)}</span>{when ? <time dateTime={item.due_date ?? undefined}>{when}{until}</time> : <span>{text("No date in the email", "郵件沒有列明日期")}</span>}{item.location && <span>{item.location}</span>}</p>
      {item.evidence && <blockquote className={item.evidence_verified ? "" : "is-unverified"}>
        <span>{text(`“${item.evidence}”`, `「${item.evidence}」`)}</span>
        {!item.evidence_verified && <small>{text("This quote was not found in the email. Check the original.", "未能在原文找到這句，請核對原信。")}</small>}
      </blockquote>}
      {onOpenSource && <button type="button" className="mail-link" onClick={() => onOpenSource(item.message_id)}>{parseSender(item.sender).name || text("Source email", "來源郵件")}<span>{item.subject || text("(No subject)", "（無主旨）")}</span></button>}
    </div>
    {!closed && <div className="mail-item-actions">
      {item.due_date && (item.calendar_event_id
        ? <span className="mail-item-done">{text("In Google Calendar", "已加入日曆")}</span>
        : <button type="button" disabled={!!pending} onClick={() => act("calendar", () => mail.mailItemToCalendar(item.id))}>{pending === "calendar" ? text("Adding…", "加入中…") : text("Add to calendar", "加入日曆")}</button>)}
      {remindDraft === null
        ? <button type="button" disabled={!!pending} onClick={() => setRemindDraft(item.remind_at ?? defaultRemindAt(item.due_date, item.due_time, new Date()))}>{item.remind_at ? text(`Reminder ${formatDue(item.remind_at.slice(0, 10), item.remind_at.slice(11), locale)}`, `提醒：${formatDue(item.remind_at.slice(0, 10), item.remind_at.slice(11), locale)}`) : text("Remind me", "提醒我")}</button>
        : <form className="mail-remind" onSubmit={e => { e.preventDefault(); const at = remindDraft; setRemindDraft(null); void act("remind", () => mail.mailUpdateItem(item.id, "open", at || null)); }}>
          <input type="datetime-local" required value={remindDraft} aria-label={text("Reminder time", "提醒時間")} onChange={e => setRemindDraft(e.target.value)} />
          <button type="submit" className="mail-primary">{text("Set", "設定")}</button>
          {item.remind_at && <button type="button" onClick={() => { setRemindDraft(null); void act("remind", () => mail.mailUpdateItem(item.id, "open", null)); }}>{text("Remove", "取消提醒")}</button>}
          <button type="button" onClick={() => setRemindDraft(null)}>{text("Cancel", "取消")}</button>
        </form>}
      <button type="button" className="mail-quiet" disabled={!!pending} onClick={() => act("dismiss", () => mail.mailUpdateItem(item.id, "dismissed", null))}>{text("Dismiss", "忽略")}</button>
    </div>}
    {item.status === "dismissed" && <div className="mail-item-actions"><button type="button" className="mail-quiet" disabled={!!pending} onClick={() => act("status", () => mail.mailUpdateItem(item.id, "open", null))}>{text("Restore", "還原")}</button></div>}
  </li>;
}

/** The AI card at the top of the reader: what the email is about and what to do. */
export function MailAiCard({ triage, items, text, locale, onChanged, onError, onAttention }: ItemActions & { triage: MailTriage; items: MailItem[]; onAttention: (attention: boolean) => void }) {
  const confidence = Math.round(triage.category_confidence * 100);
  const fallback = triage.sent_by_owner ? text("You sent this email.", "這是你寄出的郵件。")
    : triage.attention ? text("This email needs your attention.", "這封郵件需要你處理。")
    : text("Nothing to do.", "不需要處理。");
  return <section className={`mail-ai${triage.attention ? " needs-attention" : ""}`} aria-label={text("AI triage", "AI 整理")}>
    <p className="mail-ai-summary">{triage.summary || fallback}</p>
    <p className="mail-ai-meta">
      <CategoryTag category={triage.category} text={text} />
      {triage.attention && <span className="mail-attention-tag">{text("Needs attention", "需要處理")}</span>}
      <span title={text("How sure Jev was about the category", "Jev 對分類的把握程度")}>{text(`Jev ${confidence}% sure`, `Jev 把握 ${confidence}%`)}</span>
      {triage.decided_by === "luna" && <span>{text("category checked by gpt-6-luna", "分類經 gpt-6-luna 覆核")}</span>}
      {!triage.sent_by_owner && <button type="button" className="mail-quiet mail-ai-toggle" onClick={() => onAttention(!triage.attention)}>{triage.attention ? text("Mark as handled", "標記為已處理") : text("Needs my attention", "標記為需要處理")}</button>}
    </p>
    {items.length > 0 && <ul className="mail-items">{items.map(item => <MailItemRow key={item.id} item={item} text={text} locale={locale} onChanged={onChanged} onError={onError} />)}</ul>}
  </section>;
}

/** Every open follow-up from every mailbox, by date. */
export function MailAgenda({ text, locale, revision, onOpenSource, onError }: { text: Text; locale: string; revision: number; onOpenSource: (messageId: number) => void; onError: (message: string) => void }) {
  const [items, setItems] = useState<MailItem[] | null>(null);
  useEffect(() => {
    let current = true;
    mail.mailListItems(undefined, false).then(data => { if (current) setItems(data); }).catch(e => { if (current) onError(String(e)); });
    return () => { current = false; };
  }, [revision]);
  if (!items) return <p className="mail-empty">{text("Loading follow-ups…", "正在載入待辦…")}</p>;
  if (!items.length) return <div className="mail-empty"><p>{text("No open follow-ups.", "沒有未完成的待辦。")}</p><p>{text("Dates, tasks and replies found by AI triage appear here.", "AI 整理從郵件找到的日期、待辦及待回覆事項會列在這裏。")}</p></div>;
  const today = new Date();
  const replace = (next: MailItem) => setItems(list => (list ?? []).map(item => item.id === next.id ? next : item).filter(item => item.status === "open" || item.id === next.id));
  return <div className="mail-agenda">{DUE_BUCKETS.map(bucket => {
    const group = items.filter(item => dueBucket(item.due_date, today) === bucket);
    if (!group.length) return null;
    return <section key={bucket} className="mail-agenda-group" data-bucket={bucket}>
      <h3>{dueBucketLabel(bucket, text)}<span>{group.length}</span></h3>
      <ul className="mail-items">{group.map(item => <MailItemRow key={item.id} item={item} text={text} locale={locale} onChanged={replace} onError={onError} onOpenSource={onOpenSource} />)}</ul>
    </section>;
  })}</div>;
}
