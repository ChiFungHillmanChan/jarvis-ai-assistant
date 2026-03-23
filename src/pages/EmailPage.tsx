import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { EmailSummary } from "../lib/types";
import { archiveEmail, syncEmails } from "../lib/commands";

export default function EmailPage() {
  const { data: emails, refetch } = useTauriCommand<EmailSummary[]>("get_emails", { limit: 50 });
  const [syncing, setSyncing] = useState(false);

  async function handleSync() {
    setSyncing(true);
    try { await syncEmails(); refetch(); } catch (e) { console.error(e); }
    finally { setSyncing(false); }
  }

  async function handleArchive(gmailId: string) {
    try { await archiveEmail(gmailId); refetch(); } catch (e) { console.error(e); }
  }

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <span className="system-text">EMAIL INBOX</span>
        <button onClick={handleSync} disabled={syncing} style={styles.syncBtn}>
          {syncing ? "SYNCING..." : "SYNC NOW"}
        </button>
      </div>
      <div style={styles.list}>
        {!emails || emails.length === 0 ? (
          <div style={styles.empty}>No emails synced. Connect Google in Settings, then sync.</div>
        ) : emails.map((email) => (
          <div key={email.id} style={styles.emailItem}>
            <div style={styles.emailHeader}>
              <span style={styles.sender}>{email.sender}</span>
              <span style={styles.date}>{email.received_at ? new Date(email.received_at).toLocaleDateString() : ""}</span>
            </div>
            <div style={styles.subject}>{email.subject || "(No subject)"}</div>
            {email.snippet && <div style={styles.snippet}>{email.snippet}</div>}
            <div style={styles.actions}>
              {!email.is_read && <span style={styles.unreadBadge}>UNREAD</span>}
              <button onClick={() => handleArchive(email.gmail_id)} style={styles.archiveBtn}>ARCHIVE</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 24, height: "100%", display: "flex", flexDirection: "column" },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 },
  syncBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 10 },
  list: { flex: 1, overflowY: "auto" },
  empty: { color: "rgba(0, 180, 255, 0.3)", fontSize: 12, fontStyle: "italic", padding: 20 },
  emailItem: { padding: "12px 0", borderBottom: "1px solid rgba(0, 180, 255, 0.08)" },
  emailHeader: { display: "flex", justifyContent: "space-between", marginBottom: 4 },
  sender: { color: "rgba(0, 180, 255, 0.8)", fontSize: 12, fontWeight: 500 },
  date: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, fontFamily: "var(--font-mono)" },
  subject: { color: "rgba(0, 180, 255, 0.7)", fontSize: 12, marginBottom: 4 },
  snippet: { color: "rgba(0, 180, 255, 0.4)", fontSize: 11, marginBottom: 6 },
  actions: { display: "flex", gap: 8, alignItems: "center" },
  unreadBadge: { color: "rgba(0, 180, 255, 0.9)", fontSize: 8, fontFamily: "var(--font-mono)", letterSpacing: 1, background: "rgba(0, 180, 255, 0.1)", padding: "2px 6px", borderRadius: 3 },
  archiveBtn: { background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "2px 8px", color: "rgba(0, 180, 255, 0.5)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
};
