import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { GitHubItemView } from "../lib/types";
import { openUrl, syncGitHub } from "../lib/commands";
import { useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";
import { useUiLocale } from "../i18n";


export default function GitHubPage() {
  const { locale, text } = useUiLocale();
  const typeName = (type: string) => type === "pr_review" ? text("Review requested", "請求審查") : type === "pr" ? text("Pull request", "合併請求") : text("Issue", "議題");
  const stateName = (state: string) => state === "open" ? text("Open", "未關閉") : state === "closed" ? text("Closed", "已關閉") : state === "merged" ? text("Merged", "已合併") : state;
  const { data, error: loadError, loading, refetch } = useTauriCommand<GitHubItemView[]>("get_github_items");
  const [selectedId, setSelectedId] = useWorkspaceValue<number | null>("githubItemId", null);
  const [syncing, setSyncing] = useState(false); const [actionError, setActionError] = useState<string | null>(null); const [notice, setNotice] = useState<string | null>(null);
  const selected = (data ?? []).find(item => item.id === selectedId) ?? null;
  async function sync() { setSyncing(true); setActionError(null); try { setNotice(await syncGitHub()); refetch(); } catch (e) { setActionError(String(e)); } finally { setSyncing(false); } }
  async function openSelected() { if (!selected?.url) return; setActionError(null); try { await openUrl(selected.url); } catch (e) { setActionError(String(e)); } }
  return <main className="tool-page"><header className="tool-page__header"><div><p className="tool-page__eyebrow">GitHub</p><h1>{text("Review and issue inbox", "審查同議題收件匣")}</h1><p>{text("Assigned work and review requests from your latest sync.", "最近一次同步嘅指派工作同審查請求。")}</p></div><button className="tool-button" onClick={sync} disabled={syncing}>{syncing ? text("Syncing…", "同步緊…") : text("Sync GitHub", "同步 GitHub")}</button></header>
    {(loadError || actionError || notice) && <div className={loadError || actionError ? "tool-alert tool-alert--error" : "tool-alert"} role={loadError || actionError ? "alert" : "status"}>{actionError || loadError || notice}</div>}
    <div className="tool-split"><section className="tool-panel tool-list-pane" aria-busy={loading}><div className="tool-panel__heading"><h2>{text("Items", "項目")}</h2><span>{locale === "zh-HK" ? `已同步 ${data?.length ?? 0} 項` : `${data?.length ?? 0} synced`}</span></div>{loading && !data ? <p className="tool-empty">{text("Loading GitHub items…", "載入 GitHub 項目緊…")}</p> : !data?.length ? <p className="tool-empty">{text("No synced items. Add a GitHub token in Settings, then sync.", "未有已同步項目。請先喺設定加入 GitHub token，再同步。")}</p> : data.map(item => <button key={item.id} className={`tool-list-row${item.id === selectedId ? " is-selected" : ""}`} onClick={() => setSelectedId(item.id)}><span><strong>{item.title}</strong><small>{item.repo}{item.number != null ? ` #${item.number}` : ""}</small></span><span className="tool-status">{typeName(item.item_type)}</span></button>)}</section>
    <aside className="tool-panel tool-detail-pane">{selected ? <><div className="tool-panel__heading"><h2>{text("Item details", "項目詳情")}</h2><span>{stateName(selected.state)}</span></div><h3>{selected.title}</h3><dl className="tool-details"><dt>{text("Type", "類型")}</dt><dd>{typeName(selected.item_type)}</dd><dt>{text("Repository", "儲存庫")}</dt><dd>{selected.repo}</dd>{selected.number != null && <><dt>{text("Number", "編號")}</dt><dd>#{selected.number}</dd></>}{selected.author && <><dt>{text("Author", "作者")}</dt><dd>{selected.author}</dd></>}{selected.updated_at && <><dt>{text("Updated", "更新時間")}</dt><dd>{new Date(selected.updated_at).toLocaleString(locale)}</dd></>}</dl>{selected.url ? <button className="tool-button" onClick={openSelected}>{text("Open on GitHub", "喺 GitHub 開啟")}</button> : <p className="tool-help">{text("No provider link was included in this sync.", "今次同步冇提供來源連結。")}</p>}</> : <p className="tool-empty">{text("Select an item to inspect its real synced metadata.", "選取項目查看已同步嘅資料。")}</p>}</aside></div>
  </main>;
}
