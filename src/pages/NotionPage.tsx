import IntegrationConnection from "../components/IntegrationConnection";
import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { NotionPageView } from "../lib/types";
import { openUrl, syncNotion } from "../lib/commands";
import { useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./tool-pages.css";
import { useUiLocale } from "../i18n";

export default function NotionPage() {
  const { locale, text } = useUiLocale();
  const { data, error: loadError, loading, refetch } = useTauriCommand<NotionPageView[]>("get_notion_pages", { limit: 50 });
  const [selectedId, setSelectedId] = useWorkspaceValue<number | null>("notionPageId", null); const [syncing, setSyncing] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null); const [notice, setNotice] = useState<string | null>(null);
  const selected = (data ?? []).find(page => page.id === selectedId) ?? null;
  async function sync() { setSyncing(true); setActionError(null); try { setNotice(await syncNotion()); refetch(); } catch (e) { setActionError(String(e)); } finally { setSyncing(false); } }
  async function openSelected() { if (!selected?.url) return; setActionError(null); try { await openUrl(selected.url); } catch (e) { setActionError(String(e)); } }
  return <main className="tool-page"><header className="tool-page__header"><div><p className="tool-page__eyebrow">Notes</p><h1>{text("Notion sources", "Notion 來源")}</h1><p>{text("Browse page metadata available from your Notion connection.", "瀏覽 Notion 連接提供的頁面資料。")}</p></div><button className="tool-button" onClick={sync} disabled={syncing}>{syncing ? text("Syncing…", "同步中…") : text("Sync Notion", "同步 Notion")}</button></header>
    {(loadError || actionError || notice) && <div className={loadError || actionError ? "tool-alert tool-alert--error" : "tool-alert"} role={loadError || actionError ? "alert" : "status"}>{actionError || loadError || notice}</div>}
    <IntegrationConnection service="notion" />
    <div className="tool-split"><section className="tool-panel tool-list-pane" aria-busy={loading}><div className="tool-panel__heading"><h2>{text("Pages", "頁面")}</h2><span>{locale === "zh-HK" ? `已同步 ${data?.length ?? 0} 頁` : `${data?.length ?? 0} synced`}</span></div>{loading && !data ? <p className="tool-empty">{text("Loading Notion pages…", "正在載入 Notion 頁面…")}</p> : !data?.length ? <p className="tool-empty">{text("No synced pages. Connect Notion below, then sync.", "尚未同步任何頁面。請先在下方連接 Notion，再進行同步。")}</p> : data.map(page => <button key={page.id} className={`tool-list-row${page.id === selectedId ? " is-selected" : ""}`} onClick={() => setSelectedId(page.id)}><span><strong>{page.title || text("Untitled page", "未命名頁面")}</strong><small>{page.parent_type || text("Notion page", "Notion 頁面")}</small></span>{page.last_edited && <span className="tool-status">{new Date(page.last_edited).toLocaleDateString(locale)}</span>}</button>)}</section>
    <aside className="tool-panel tool-detail-pane">{selected ? <><div className="tool-panel__heading"><h2>{text("Source details", "來源詳情")}</h2><span>Notion</span></div><h3>{selected.title || text("Untitled page", "未命名頁面")}</h3><dl className="tool-details"><dt>{text("Source", "來源")}</dt><dd>Notion</dd><dt>{text("Parent type", "上層類型")}</dt><dd>{selected.parent_type || text("Not provided", "未提供")}</dd>{selected.last_edited && <><dt>{text("Last edited", "最近編輯")}</dt><dd>{new Date(selected.last_edited).toLocaleString(locale)}</dd></>}</dl><p className="tool-help">{text("JARVIS has synced this page's title and source metadata, not its body content.", "JARVIS 已同步此頁面的標題及來源資料，尚未同步正文。")}</p>{selected.url ? <button className="tool-button" onClick={openSelected}>{text("Open in Notion", "在 Notion 開啟")}</button> : <p className="tool-help">{text("No provider link was included in this sync.", "本次同步未提供來源連結。")}</p>}</> : <p className="tool-empty">{text("Select a page to inspect its source details.", "選取頁面查看來源資料。")}</p>}</aside></div>
  </main>;
}
