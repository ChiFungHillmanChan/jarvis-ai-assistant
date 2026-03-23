import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { NotionPageView } from "../lib/types";
import { syncNotion } from "../lib/commands";

export default function NotionPage() {
  const { data: pages, refetch } = useTauriCommand<NotionPageView[]>("get_notion_pages", { limit: 50 });
  const [syncing, setSyncing] = useState(false);

  async function handleSync() {
    setSyncing(true);
    try { await syncNotion(); refetch(); } catch (e) { console.error(e); }
    finally { setSyncing(false); }
  }

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <span className="system-text">NOTION PAGES</span>
        <button onClick={handleSync} disabled={syncing} style={styles.syncBtn}>
          {syncing ? "SYNCING..." : "SYNC NOW"}
        </button>
      </div>
      <div style={styles.list}>
        {!pages || pages.length === 0 ? (
          <div style={styles.empty}>No Notion pages. Add your API key in Settings, then sync.</div>
        ) : pages.map((page) => (
          <div key={page.id} style={styles.pageItem}>
            <div style={styles.pageTitle}>{page.title}</div>
            <div style={styles.pageMeta}>
              {page.parent_type && <span>{page.parent_type}</span>}
              {page.last_edited && <span> -- edited {new Date(page.last_edited).toLocaleDateString()}</span>}
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
  pageItem: { padding: "10px 0", borderBottom: "1px solid rgba(0, 180, 255, 0.08)" },
  pageTitle: { color: "rgba(0, 180, 255, 0.8)", fontSize: 13 },
  pageMeta: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 2 },
};
