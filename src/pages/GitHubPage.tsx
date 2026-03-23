import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { GitHubItemView } from "../lib/types";
import { syncGitHub } from "../lib/commands";

export default function GitHubPage() {
  const { data: items, refetch } = useTauriCommand<GitHubItemView[]>("get_github_items");
  const [syncing, setSyncing] = useState(false);

  async function handleSync() {
    setSyncing(true);
    try { await syncGitHub(); refetch(); } catch (e) { console.error(e); }
    finally { setSyncing(false); }
  }

  const prs = items?.filter(i => i.item_type === "pr" || i.item_type === "pr_review") ?? [];
  const issues = items?.filter(i => i.item_type === "issue") ?? [];

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <span className="system-text">GITHUB</span>
        <button onClick={handleSync} disabled={syncing} style={styles.syncBtn}>
          {syncing ? "SYNCING..." : "SYNC NOW"}
        </button>
      </div>
      {(!items || items.length === 0) ? (
        <div style={styles.empty}>No GitHub items. Add your token in Settings, then sync.</div>
      ) : (
        <div style={styles.list}>
          {prs.length > 0 && (
            <div>
              <div className="label" style={{ marginBottom: 8 }}>PULL REQUESTS</div>
              {prs.map(pr => (
                <div key={pr.id} style={styles.item}>
                  <div style={styles.itemHeader}>
                    <span style={styles.itemTitle}>{pr.title}</span>
                    <span style={{ ...styles.badge, color: pr.item_type === "pr_review" ? "rgba(255, 180, 0, 0.8)" : "rgba(16, 185, 129, 0.8)" }}>
                      {pr.item_type === "pr_review" ? "REVIEW" : "OPEN"}
                    </span>
                  </div>
                  <div style={styles.itemMeta}>{pr.repo} #{pr.number}</div>
                </div>
              ))}
            </div>
          )}
          {issues.length > 0 && (
            <div style={{ marginTop: 16 }}>
              <div className="label" style={{ marginBottom: 8 }}>ISSUES</div>
              {issues.map(issue => (
                <div key={issue.id} style={styles.item}>
                  <div style={styles.itemTitle}>{issue.title}</div>
                  <div style={styles.itemMeta}>{issue.repo} #{issue.number}</div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 24, height: "100%", display: "flex", flexDirection: "column" },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 },
  syncBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 10 },
  list: { flex: 1, overflowY: "auto" },
  empty: { color: "rgba(0, 180, 255, 0.3)", fontSize: 12, fontStyle: "italic", padding: 20 },
  item: { padding: "10px 0", borderBottom: "1px solid rgba(0, 180, 255, 0.08)" },
  itemHeader: { display: "flex", justifyContent: "space-between", alignItems: "center" },
  itemTitle: { color: "rgba(0, 180, 255, 0.8)", fontSize: 12 },
  itemMeta: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, fontFamily: "var(--font-mono)", marginTop: 2 },
  badge: { fontSize: 8, fontFamily: "var(--font-mono)", letterSpacing: 1 },
};
