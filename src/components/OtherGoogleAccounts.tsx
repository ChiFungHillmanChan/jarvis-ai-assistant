import { useRef, useState } from "react";
import * as mail from "../lib/mail";
import type { MailAccount } from "../lib/mail";
import { syncCalendar } from "../lib/commands";
import { useUiLocale } from "../i18n";
import "../pages/settings-workspace.css";

/**
 * Settings > Google: the user's second and third Google accounts. Each is a
 * Mail mailbox that also shares its calendar, both read-only
 * (src-tauri/src/mail/additional.rs); the assistant can read them, and only
 * the main account is ever written to.
 */
export default function OtherGoogleAccounts({ accounts, canAdd, onChanged }: { accounts: MailAccount[]; canAdd: boolean; onChanged: () => Promise<void> }) {
  const { text } = useUiLocale();
  // "connect" while a Google sign-in is open, or the id of the account being disconnected.
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const cancelled = useRef(false);

  async function connect() {
    setBusy("connect"); setError(null); setNotice(null); cancelled.current = false;
    try {
      const account = await mail.mailConnectAccount();
      setNotice(account.calendar
        ? text(`${account.email} added. Its mail and calendar are syncing now.`, `已加入 ${account.email}，正在同步郵件及日曆。`)
        : text(`${account.email} added without calendar access. To see its events, connect it again and allow Calendar.`, `已加入 ${account.email}，但未獲日曆權限。如要顯示其日曆，請再次連接並允許存取日曆。`));
      // First sync now instead of at the next five-minute tick. The calendar
      // waits for the mail sync, which holds the Mail lock.
      void (async () => {
        await mail.mailSyncAccount(account.id, false).catch(() => undefined);
        await syncCalendar().catch(() => undefined);
      })();
    } catch (e) {
      // Cancelling a sign-in the user abandoned is not a failure.
      if (cancelled.current) setNotice(text("Google sign-in cancelled.", "已取消 Google 登入。"));
      else setError(String(e));
    } finally {
      setBusy(null);
      await onChanged();
    }
  }
  // Closing the browser tab tells JARVIS nothing; this stops the wait.
  function cancel() {
    cancelled.current = true;
    void mail.mailCancelConnect().catch(() => undefined);
  }
  async function disconnect(account: MailAccount) {
    if (!window.confirm(text(`Disconnect ${account.email}? Its events are removed from JARVIS; its mail copies stay on this Mac.`, `是否中斷 ${account.email} 的連接？其日曆活動會從 JARVIS 移除，郵件副本會保留在此 Mac。`))) return;
    setBusy(account.id); setError(null); setNotice(null);
    try { await mail.mailDisconnectAccount(account.id); }
    catch (e) { setError(String(e)); }
    finally { setBusy(null); await onChanged(); }
  }

  const status = (account: MailAccount) => !account.connected
    ? text("Reconnect needed · Mail copies kept", "需要重新連接 · 郵件副本保留")
    : account.calendar ? text("Mail and calendar", "郵件及日曆") : text("Mail only · Calendar not allowed", "只有郵件 · 未授權日曆");

  return <section className="integration-connection" aria-label={text("Other Google accounts", "其他 Google 帳戶")}>
    <div className="integration-connection__heading"><h3>{text("Other accounts", "其他帳戶")}</h3></div>
    <p className="settings-help">{text("Add your second or third Google account. JARVIS syncs its mail to Mail and its events to Calendar, and the assistant can read both. Access is read-only: drafts and new events always go to the main account.", "加入你的第二或第三個 Google 帳戶。JARVIS 會把其郵件同步至郵件頁、日曆活動同步至日曆頁，助手亦可以讀取。權限為唯讀：草稿及新活動一律建立在主帳戶。")}</p>
    {accounts.length > 0 && <ul className="connections-account-list">
      {accounts.map(account => <li key={account.id} className="connections-account">
        <div><strong>{account.email}</strong><p>{status(account)}</p></div>
        <div className="integration-actions">
          {(!account.connected || !account.calendar) && <button disabled={!!busy} title={text("Choose this account on Google's page.", "請在 Google 頁面選擇此帳戶。")} onClick={() => { void connect(); }}>{account.connected ? text("Allow calendar", "加入日曆") : text("Reconnect", "重新連接")}</button>}
          {account.connected && <button disabled={!!busy} onClick={() => { void disconnect(account); }}>{busy === account.id ? text("Disconnecting…", "正在中斷連接…") : text("Disconnect", "中斷連接")}</button>}
        </div>
      </li>)}
    </ul>}
    <div className="integration-actions">
      {busy === "connect"
        ? <button onClick={cancel}>{text("Cancel Google sign-in", "取消 Google 登入")}</button>
        : canAdd && <button className="connections-primary" disabled={!!busy} onClick={() => { void connect(); }}>{text("Add another Google account", "加入其他 Google 帳戶")}</button>}
    </div>
    {busy === "connect" && <p className="settings-help" role="status">{text("Finish signing in in your browser. If you closed the Google tab, choose Cancel.", "請在瀏覽器完成登入。如已關閉 Google 登入分頁，請按「取消 Google 登入」。")}</p>}
    {!canAdd && busy !== "connect" && <p className="settings-help">{text("Connect the main account first.", "請先連接主帳戶。")}</p>}
    {notice && <p className="settings-help" role="status">{notice}</p>}
    {error && <p className="connections-error settings-help" role="alert">{error}</p>}
  </section>;
}
