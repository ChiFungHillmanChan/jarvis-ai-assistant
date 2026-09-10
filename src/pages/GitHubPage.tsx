import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { GitHubItemView } from "../lib/types";
import { openUrl, syncGitHub } from "../lib/commands";
import { useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";

const typeName = (type: string) => type === "pr_review" ? "Review requested" : type === "pr" ? "Pull request" : "Issue";

export default function GitHubPage() {
  const { data, error: loadError, loading, refetch } = useTauriCommand<GitHubItemView[]>("get_github_items");
  const [selectedId, setSelectedId] = useWorkspaceValue<number | null>("githubItemId", null);
  const [syncing, setSyncing] = useState(false); const [actionError, setActionError] = useState<string | null>(null); const [notice, setNotice] = useState<string | null>(null);
  const selected = (data ?? []).find(item => item.id === selectedId) ?? null;
  async function sync() { setSyncing(true); setActionError(null); try { setNotice(await syncGitHub()); refetch(); } catch (e) { setActionError(String(e)); } finally { setSyncing(false); } }
  async function openSelected() { if (!selected?.url) return; setActionError(null); try { await openUrl(selected.url); } catch (e) { setActionError(String(e)); } }
  return <main className="tool-page"><header className="tool-page__header"><div><p className="tool-page__eyebrow">GitHub</p><h1>Review and issue inbox</h1><p>Assigned work and review requests from your latest sync.</p></div><button className="tool-button" onClick={sync} disabled={syncing}>{syncing ? "Syncing…" : "Sync GitHub"}</button></header>
    {(loadError || actionError || notice) && <div className={loadError || actionError ? "tool-alert tool-alert--error" : "tool-alert"} role="status">{actionError || loadError || notice}</div>}
    <div className="tool-split"><section className="tool-panel tool-list-pane" aria-busy={loading}><div className="tool-panel__heading"><h2>Items</h2><span>{data?.length ?? 0} synced</span></div>{loading && !data ? <p className="tool-empty">Loading GitHub items…</p> : !data?.length ? <p className="tool-empty">No synced items. Add a GitHub token in Settings, then sync.</p> : data.map(item => <button key={item.id} className={`tool-list-row${item.id === selectedId ? " is-selected" : ""}`} onClick={() => setSelectedId(item.id)}><span><strong>{item.title}</strong><small>{item.repo}{item.number != null ? ` #${item.number}` : ""}</small></span><span className="tool-status">{typeName(item.item_type)}</span></button>)}</section>
    <aside className="tool-panel tool-detail-pane">{selected ? <><div className="tool-panel__heading"><h2>Item details</h2><span>{selected.state}</span></div><h3>{selected.title}</h3><dl className="tool-details"><dt>Type</dt><dd>{typeName(selected.item_type)}</dd><dt>Repository</dt><dd>{selected.repo}</dd>{selected.number != null && <><dt>Number</dt><dd>#{selected.number}</dd></>}{selected.author && <><dt>Author</dt><dd>{selected.author}</dd></>}{selected.updated_at && <><dt>Updated</dt><dd>{new Date(selected.updated_at).toLocaleString()}</dd></>}</dl>{selected.url ? <button className="tool-button" onClick={openSelected}>Open on GitHub</button> : <p className="tool-help">No provider link was included in this sync.</p>}</> : <p className="tool-empty">Select an item to inspect its real synced metadata.</p>}</aside></div>
  </main>;
}
