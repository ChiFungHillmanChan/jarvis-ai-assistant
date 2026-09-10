import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { NotionPageView } from "../lib/types";
import { openUrl, syncNotion } from "../lib/commands";
import { useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";

export default function NotionPage() {
  const { data, error: loadError, loading, refetch } = useTauriCommand<NotionPageView[]>("get_notion_pages", { limit: 50 });
  const [selectedId, setSelectedId] = useWorkspaceValue<number | null>("notionPageId", null); const [syncing, setSyncing] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null); const [notice, setNotice] = useState<string | null>(null);
  const selected = (data ?? []).find(page => page.id === selectedId) ?? null;
  async function sync() { setSyncing(true); setActionError(null); try { setNotice(await syncNotion()); refetch(); } catch (e) { setActionError(String(e)); } finally { setSyncing(false); } }
  async function openSelected() { if (!selected?.url) return; setActionError(null); try { await openUrl(selected.url); } catch (e) { setActionError(String(e)); } }
  return <main className="tool-page"><header className="tool-page__header"><div><p className="tool-page__eyebrow">Notes</p><h1>Notion sources</h1><p>Browse page metadata available from your Notion connection.</p></div><button className="tool-button" onClick={sync} disabled={syncing}>{syncing ? "Syncing…" : "Sync Notion"}</button></header>
    {(loadError || actionError || notice) && <div className={loadError || actionError ? "tool-alert tool-alert--error" : "tool-alert"} role="status">{actionError || loadError || notice}</div>}
    <div className="tool-split"><section className="tool-panel tool-list-pane" aria-busy={loading}><div className="tool-panel__heading"><h2>Pages</h2><span>{data?.length ?? 0} synced</span></div>{loading && !data ? <p className="tool-empty">Loading Notion pages…</p> : !data?.length ? <p className="tool-empty">No synced pages. Add a Notion integration key in Settings, then sync.</p> : data.map(page => <button key={page.id} className={`tool-list-row${page.id === selectedId ? " is-selected" : ""}`} onClick={() => setSelectedId(page.id)}><span><strong>{page.title || "Untitled page"}</strong><small>{page.parent_type || "Notion page"}</small></span>{page.last_edited && <span className="tool-status">{new Date(page.last_edited).toLocaleDateString()}</span>}</button>)}</section>
    <aside className="tool-panel tool-detail-pane">{selected ? <><div className="tool-panel__heading"><h2>Source details</h2><span>Notion</span></div><h3>{selected.title || "Untitled page"}</h3><dl className="tool-details"><dt>Source</dt><dd>Notion</dd><dt>Parent type</dt><dd>{selected.parent_type || "Not provided"}</dd>{selected.last_edited && <><dt>Last edited</dt><dd>{new Date(selected.last_edited).toLocaleString()}</dd></>}</dl><p className="tool-help">JARVIS has synced this page's title and source metadata, not its body content.</p>{selected.url ? <button className="tool-button" onClick={openSelected}>Open in Notion</button> : <p className="tool-help">No provider link was included in this sync.</p>}</> : <p className="tool-empty">Select a page to inspect its source details.</p>}</aside></div>
  </main>;
}
