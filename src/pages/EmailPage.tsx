import { useEffect, useRef, useState, type ReactNode } from "react";
import { Paperclip } from "lucide-react";
import * as mail from "../lib/mail";
import type { MailAccount, MailAnalysis, MailItem, MailMemory, MailMemoryInput, MailMessage, MailPage, MailSettings, MailView, MailWorkspace } from "../lib/mail";
import { useIsActiveTab, useWorkspaceValue } from "../workspace/WorkspaceContext";
import ReadableContent from "../components/content/ReadableContent";
import { CategoryTag, MailAgenda, MailAiCard } from "../components/mail/MailItems";
import { analysisSource, backendUtc, categoryLabel, decodeEntities, fileSize, formatDue, MAIL_CATEGORIES, mailSourceUrl, parseSender, readableBody, revealMailSource } from "../lib/mail-presentation";
import { openUrl } from "../lib/commands";
import { useAiModels } from "../lib/ai-models";
import { cloudModelFor, formatRate } from "../lib/ai-catalog";
import { useUiLocale } from "../i18n";
import BedrockModelPicker from "../components/BedrockModelPicker";
import { useBedrockModelList } from "../hooks/useBedrockModelList";
import { modelChosen } from "../lib/bedrock-models";
import "./email-workspace.css";

type Editor =
  | { type: "group"; id?: number; name: string; instructions: string; members?: number[] }
  | { type: "memory"; input: MailMemoryInput }
  | { type: "settings"; input: MailSettings; consent: boolean; triage: boolean; triageConsent: boolean; typesafeKey: string }
  | { type: "account"; account: MailAccount }
  | { type: "delete"; kind: "account" | "group" | "memory"; id: string | number; name: string };

const PAGE = 50;
/** What the background sync can change: stored mail and how it is sorted. */
const storedState = (data: MailWorkspace) => JSON.stringify([data.accounts.map(a => [a.id, a.connected, a.message_count]), data.triage.untriaged, data.triage.attention, data.triage.open_items]);

export default function EmailPage({ onSetup }: { onSetup?: () => void }) {
  const models = useAiModels();
  // The rate comes from ai/models.rs; a model without a price gets no figure.
  const openAiPriceHint = (model: string) => {
    const priced = cloudModelFor(models, model);
    const rate = priced ? formatRate(priced.price.input_per_million) : null;
    return rate
      ? text(`Uses the OpenAI key saved in Settings. Your OpenAI account pays; ${model} costs about US$${rate} per million input tokens.`, `使用設定頁儲存的 OpenAI 密鑰，費用由你的 OpenAI 帳戶支付；${model} 每百萬輸入 token 約 US$${rate}。`)
      : text("Uses the OpenAI key saved in Settings. Your OpenAI account pays.", "使用設定頁儲存的 OpenAI 密鑰，費用由你的 OpenAI 帳戶支付。");
  };
  const { text, locale } = useUiLocale();
  const dateTime = (value: string | null) => value ? backendUtc(value).toLocaleString(locale, { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }) : text("Not synced", "未同步");
  const [workspace, setWorkspace] = useState<MailWorkspace | null>(null);
  const [accountId, setAccountId] = useWorkspaceValue("mailAccountId", "");
  const [groupId, setGroupId] = useWorkspaceValue<number | undefined>("mailGroupId", undefined);
  const [view, setView] = useWorkspaceValue<MailView | undefined>("mailView", undefined);
  const [query, setQuery] = useWorkspaceValue("mailSearch", "");
  const [offset, setOffset] = useWorkspaceValue("mailOffset", 0);
  const [page, setPage] = useState<MailPage>({ messages: [], total: 0, has_more: false });
  const [selected, setSelected] = useState<number[]>([]);
  const [targetGroup, setTargetGroup] = useState("");
  const [messageId, setMessageId] = useWorkspaceValue<number | null>("mailMessageId", null);
  const [message, setMessage] = useState<MailMessage | null>(null);
  const [items, setItems] = useState<MailItem[]>([]);
  const [memories, setMemories] = useState<MailMemory[]>([]);
  const [analysis, setAnalysis] = useWorkspaceValue<{ groupId: number; result: MailAnalysis } | null>("mailAnalysis", null);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [busy, setBusy] = useState("");
  const [loading, setLoading] = useState(false);
  const [reading, setReading] = useState(false);
  const [memoryLoading, setMemoryLoading] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [revision, setRevision] = useState(0);
  const operation = useRef(false);
  const connectCancelled = useRef(false);
  const stored = useRef("");
  const dialog = useRef<HTMLDialogElement>(null);
  // The Bedrock model list, asked for only while Analysis settings is open
  // on this tab with "Your AWS Bedrock account" chosen (hooks/useBedrockModelList.ts).
  const tabActive = useIsActiveTab();
  const bedrockList = useBedrockModelList(tabActive && editor?.type === "settings" && editor.input.processing_mode === "bedrock");
  const reader = useRef<HTMLElement>(null);
  const requestedSource = useRef<number | null>(null);
  const group = workspace?.groups.find(item => item.id === groupId);
  const account = workspace?.accounts.find(item => item.id === accountId);
  const triage = workspace?.triage;
  const sourceUrl = message ? mailSourceUrl(message) : null;
  const from = message ? parseSender(message.sender) : null;
  const files = message?.attachments ?? null;
  const count = (category: string) => triage?.categories.find(([name]) => name === category)?.[1] ?? 0;
  // Sorting needs the user's opt-in and something to classify with: a
  // TypeSafe key, or the AI chosen in setup.
  const triageReady = !!triage?.enabled && (!!triage.typesafe_key || !!triage.chosen_ai);
  // Sorting is on but nothing can read the mail: say so instead of a button
  // that fails on every email.
  const noAi = !!triage?.enabled && !triageReady;
  const noAiText = text("AI sorting is on, but you have not set up an AI yet. Choose “Set up your AI”.", "已開啟 AI 整理，但你尚未設定 AI。請按「設定你的 AI」。");
  const chosenAi = triage?.chosen_ai ?? null;
  const sortedBy = triage?.typesafe_key
    ? text(`TypeSafe Jev decides the category and whether it needs you; ${chosenAi ?? "no AI yet"} writes a summary and finds dates only for the emails that need one.`, `TypeSafe Jev 判斷分類及是否需要你處理；只有需要摘要或含日期的郵件，才交由${chosenAi ? ` ${chosenAi} ` : "（尚未選擇 AI）"}撰寫摘要及找出日期。`)
    : chosenAi
    ? text(`${chosenAi}, the AI you chose, decides the category, writes a summary and finds dates.`, `由你選擇的 ${chosenAi} 判斷分類、撰寫摘要及找出日期。`)
    : text("Choose your AI in Settings → Your AI first.", "請先到「設定 → 你的 AI」選擇 AI。");

  useEffect(() => {
    let current = true;
    mail.mailWorkspace(accountId).then(data => { if (current) { stored.current = storedState(data); setWorkspace(data); } }).catch(e => { if (current) setError(text(`Could not load the workspace: ${String(e)}`, `載入工作區失敗：${String(e)}`)); });
    return () => { current = false; };
  }, [revision, accountId]);
  // The background sync stores and sorts mail without this page knowing.
  // Reload only when what is stored has changed, so an open message and the
  // list are not refreshed under the reader for nothing.
  useEffect(() => {
    const timer = window.setInterval(() => {
      if (document.hidden || operation.current || editor) return;
      mail.mailWorkspace(accountId).then(data => {
        if (stored.current && storedState(data) !== stored.current) setRevision(n => n + 1);
      }).catch(() => undefined);
    }, 60_000);
    return () => window.clearInterval(timer);
  }, [accountId, editor]);
  useEffect(() => {
    if (view === "due") return;
    let current = true;
    // Keep the current rows on screen while the next page loads.
    setLoading(true);
    const timer = window.setTimeout(() => {
      mail.mailListMessages({ accountId: accountId || undefined, groupId, query, offset, view }).then(data => {
        if (!current) return;
        setPage(data);
        setSelected(ids => ids.filter(id => data.messages.some(m => m.id === id)));
        if (offset > 0 && !data.messages.length) setOffset(0);
      }).catch(e => { if (current) setError(text(`Could not load messages: ${String(e)}`, `未能載入郵件：${String(e)}`)); }).finally(() => { if (current) setLoading(false); });
    }, query ? 200 : 0);
    return () => { current = false; window.clearTimeout(timer); };
  }, [accountId, groupId, view, query, offset, revision]);
  useEffect(() => {
    let current = true;
    setReading(messageId !== null);
    if (messageId === null) { setMessage(null); setItems([]); return; }
    Promise.all([mail.mailGetMessage(messageId), mail.mailListItems(messageId, true)]).then(([data, found]) => {
      if (current) { setMessage(data); setItems(found); }
    }).catch(e => { if (current) { requestedSource.current = null; setMessage(null); setError(text(`Could not open the message: ${String(e)}`, `未能開啟郵件：${String(e)}`)); } }).finally(() => { if (current) setReading(false); });
    return () => { current = false; };
  }, [messageId, revision]);
  useEffect(() => {
    if (requestedSource.current !== messageId) { requestedSource.current = null; return; }
    if (!reading && revealMailSource(requestedSource.current, message?.id ?? null, reader.current)) requestedSource.current = null;
  }, [messageId, message, reading]);
  useEffect(() => {
    let current = true; setMemories([]); setMemoryLoading(groupId !== undefined);
    if (groupId !== undefined) mail.mailListMemories(groupId).then(data => { if (current) setMemories(data); }).catch(e => { if (current) setError(text(`Could not load memories: ${String(e)}`, `載入記憶失敗：${String(e)}`)); }).finally(() => { if (current) setMemoryLoading(false); });
    return () => { current = false; };
  }, [groupId, revision]);
  useEffect(() => { if (editor && dialog.current && !dialog.current.open) dialog.current.showModal(); }, [editor]);
  // j / k move through the list, Escape closes the reader.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      if (editor || event.metaKey || event.ctrlKey || event.altKey || event.isComposing || target?.closest("input, textarea, select, [contenteditable=true]")) return;
      if (event.key === "Escape" && messageId !== null) { setMessageId(null); return; }
      if ((event.key !== "j" && event.key !== "k") || view === "due") return;
      const index = page.messages.findIndex(item => item.id === messageId);
      const next = page.messages[event.key === "j" ? index + 1 : Math.max(0, index - 1)];
      if (next) { event.preventDefault(); setMessageId(next.id); }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [page, messageId, editor, view]);

  function refreshWorkspace() { mail.mailWorkspace(accountId).then(data => { stored.current = storedState(data); setWorkspace(data); }).catch(() => undefined); }
  // A sorting run can take minutes with a slow model; refresh the counts so
  // progress shows instead of a page that looks frozen.
  const sorting = busy === text("AI sorting", "AI 整理");
  useEffect(() => {
    if (!sorting) return;
    const timer = window.setInterval(refreshWorkspace, 3_000);
    return () => window.clearInterval(timer);
  }, [sorting, accountId]);
  async function run(label: string, action: () => Promise<void>, refresh = true) {
    if (operation.current) return;
    operation.current = true; setBusy(label); setError(""); setNotice("");
    try { await action(); if (refresh) { setRevision(n => n + 1); setAnalysis(null); } }
    catch (e) { setError(text(`${label} failed: ${String(e)}`, `${label}失敗：${String(e)}`)); }
    finally { operation.current = false; setBusy(""); }
  }
  const connectLabel = text("Connect Gmail", "連接 Gmail");
  function connectAccount() {
    connectCancelled.current = false;
    return run(connectLabel, async () => {
      try {
        const connected = await mail.mailConnectAccount();
        setAccountId(connected.id); setOffset(0);
        setNotice(text("Connected. Choose Sync to download your mail.", "已連接。請按「同步」下載郵件。"));
      } catch (e) {
        // Cancelling a sign-in the user abandoned is not a failure.
        if (connectCancelled.current) { setNotice(text("Gmail sign-in cancelled.", "已取消 Gmail 登入。")); return; }
        throw e;
      }
    });
  }
  // Closing the browser tab tells JARVIS nothing; this stops the wait.
  function cancelConnect() {
    connectCancelled.current = true;
    void mail.mailCancelConnect().catch(() => {});
  }
  function choose(next: { view?: MailView; groupId?: number }) {
    setView(next.view); setGroupId(next.groupId); setOffset(0); setSelected([]); setMessageId(null);
  }
  function openSource(id: number) {
    requestedSource.current = id;
    setMessageId(id);
    if (!reading && revealMailSource(id, message?.id ?? null, reader.current)) requestedSource.current = null;
  }
  function editMemory(item?: MailMemory) {
    if (!group) return;
    setEditor({ type: "memory", input: item ? { ...item } : { id: null, group_id: group.id, content: "", kind: "fact", source_message_id: null, expires_at: null, status: "active" } });
  }
  function openSettings() {
    if (!workspace) return;
    setEditor({ type: "settings", input: { ...workspace.settings }, consent: workspace.settings.processing_mode !== "local", triage: workspace.triage.enabled, triageConsent: workspace.triage.enabled, typesafeKey: "" });
  }
  async function saveEditor() {
    if (!editor) return;
    const draft = editor;
    await run(text("Save", "儲存"), async () => {
      if (draft.type === "group") {
        const saved = await mail.mailSaveGroup(draft.name.trim(), draft.instructions, draft.id);
        if (draft.members?.length) { await mail.mailSetGroupMembers(saved.id, draft.members, true); setSelected([]); }
      }
      if (draft.type === "memory") await mail.mailSaveMemory({ ...draft.input, content: draft.input.content.trim() });
      if (draft.type === "settings") {
        await mail.mailSaveSettings(draft.input);
        if (draft.typesafeKey.trim()) await mail.saveTypeSafeKey(draft.typesafeKey.trim());
        await mail.mailSetAutoTriage(draft.triage && draft.triageConsent);
      }
      if (draft.type === "delete") {
        if (draft.kind === "account") { await mail.mailRemoveAccount(String(draft.id)); if (accountId === draft.id) setAccountId(""); setMessageId(null); }
        if (draft.kind === "group") { await mail.mailDeleteGroup(Number(draft.id)); if (groupId === draft.id) choose({}); if (targetGroup === String(draft.id)) setTargetGroup(""); }
        if (draft.kind === "memory") await mail.mailDeleteMemory(Number(draft.id));
      }
      setEditor(null); setNotice(draft.type === "delete" ? text("Deleted.", "已刪除。") : text("Saved.", "已儲存。"));
    });
  }
  async function triageNow(runs = 1) {
    let summary = "";
    for (let i = 0; i < runs; i++) {
      const result = await mail.mailTriageRun(locale === "zh-HK" ? "zh-HK" : "en");
      // Local models are free (cost_usd is null): show the tokens they used.
      // A model without a price shows its tokens too, never as free.
      const tokens = (result.jev_input_tokens + result.luna_input_tokens + result.luna_output_tokens).toLocaleString();
      const usd = result.cost_usd === null ? "" : result.cost_usd < 0.01 ? "< US$0.01" : `US$${result.cost_usd.toFixed(2)}`;
      const cost = result.price_unknown
        ? (result.cost_usd === null ? text(`price unknown, ${tokens} tokens`, `價格未知，使用 ${tokens} tokens`) : text(`AI cost ${usd} plus usage with price unknown`, `AI 費用 ${usd}，另有價格未知的用量`))
        : result.cost_usd === null ? text(`local model, free, ${tokens} tokens`, `本機模型免費，使用 ${tokens} tokens`) : text(`AI cost ${usd}`, `AI 費用 ${usd}`);
      summary = text(
        `Sorted ${result.triaged} emails, wrote ${result.summarized} summaries and found ${result.items} follow-ups (${cost}).${result.remaining ? ` ${result.remaining} still to sort.` : ""}`,
        `已整理 ${result.triaged} 封郵件，撰寫 ${result.summarized} 則摘要，找到 ${result.items} 項待辦（${cost}）。${result.remaining ? `尚有 ${result.remaining} 封未整理。` : ""}`)
        + (result.warning ? ` ${result.warning}` : "");
      if (!result.remaining || !result.triaged) break;
    }
    return summary;
  }
  async function sync(target: MailAccount | null, mode: "recent" | "older" | "restart" = "recent") {
    const accounts = target ? [target] : (workspace?.accounts ?? []).filter(item => item.connected);
    if (!accounts.length) return;
    await run(text("Sync mail", "同步郵件"), async () => {
      const notes: string[] = [];
      let saved = 0;
      for (const item of accounts) {
        // One broken mailbox must not stop the others from syncing.
        try {
          const result = mode === "restart" ? await mail.mailRestartSync(item.id) : await mail.mailSyncAccount(item.id, mode === "older");
          saved += result.saved;
          if (result.warning) notes.push(accounts.length > 1 ? `${item.email}：${result.warning}` : result.warning);
        } catch (e) {
          if (accounts.length === 1) throw e;
          notes.push(`${item.email}：${String(e)}`);
        }
      }
      let message = text(`Saved ${saved} new emails.`, `已儲存 ${saved} 封新郵件。`);
      if (notes.length) message += ` ${notes.join(" ")}`;
      if (triageReady && saved > 0) message += ` ${await triageNow(3)}`;
      setNotice(message);
    });
  }
  const statusText = busy ? text(`${busy} in progress…`, `${busy}中…`) : notice;
  const selectionActive = selected.length > 0;
  const heading = view === "attention" ? text("Needs attention", "需要處理")
    : view === "due" ? text("Follow-ups and dates", "待辦及日程")
    : view === "untriaged" ? text("Not sorted yet", "尚未整理")
    : view ? categoryLabel(view, text)
    : group ? group.name
    : text("All mail", "全部郵件");

  const nav = (label: string, active: boolean, onClick: () => void, badge?: number, extra?: ReactNode) =>
    <button className={`mail-nav${active ? " is-active" : ""}`} aria-current={active ? "page" : undefined} onClick={onClick}>{extra}<span className="mail-nav-label">{label}</span>{badge !== undefined && badge > 0 && <span className="mail-count">{badge}</span>}</button>;

  return <div className="mail-workspace">
    <header className="mail-heading">
      <div><h1>{text("Mail", "電郵工作區")}</h1><p role="status" className="mail-status">{statusText || (noAi ? noAiText : triageReady ? text("AI sorts new mail as it arrives. Copies stay on this Mac.", "新郵件同步後由 AI 自動整理；郵件副本保存在此 Mac。") : text("Local copies of your mailboxes. Turn on AI sorting in Analysis settings.", "信箱的本機副本。可在「分析設定」開啟 AI 自動整理。"))}</p></div>
      <div className="mail-heading-actions">{onSetup && <button disabled={!!busy} onClick={onSetup}>{text("Setup guide", "設定導引")}</button>}<button disabled={!!busy || !workspace} onClick={openSettings}>{text("Analysis settings", "分析設定")}</button></div>
    </header>
    {error && <div role="alert" className="mail-alert"><span>{error}</span><button onClick={() => { setError(""); setRevision(n => n + 1); }}>{text("Reload", "重新載入")}</button></div>}
    <div className={`mail-layout${messageId !== null ? " has-message" : ""}`}>
      <nav className="mail-sidebar" aria-label={text("Mail views", "郵件分類")}>
        {nav(text("Needs attention", "需要處理"), view === "attention", () => choose({ view: "attention" }), triage?.attention)}
        {nav(text("Follow-ups and dates", "待辦及日程"), view === "due", () => choose({ view: "due" }), triage?.open_items)}
        {nav(text("All mail", "全部郵件"), !view && groupId === undefined, () => choose({}), undefined)}
        <h2>{text("Sorted by AI", "AI 分類")}</h2>
        {MAIL_CATEGORIES.map(category => <div key={category}>{nav(categoryLabel(category, text), view === category, () => choose({ view: category }), count(category), <CategoryTag category={category} text={text} compact />)}</div>)}
        {!!triage?.untriaged && nav(text("Not sorted yet", "尚未整理"), view === "untriaged", () => choose({ view: "untriaged" }), triage.untriaged)}
        <div className="mail-section-title"><h2>{text("My groups", "我的群組")}</h2><button className="mail-quiet" onClick={() => setEditor({ type: "group", name: "", instructions: "" })}>{text("New", "新增")}</button></div>
        {workspace?.groups.length ? workspace.groups.map(item => <div key={item.id}>{nav(item.name, item.id === groupId, () => choose({ groupId: item.id }), item.message_count)}</div>) : <p className="mail-hint">{text("Groups hold hand-picked mail with your own instructions.", "群組用來收集你挑選的郵件，並附上你的指示。")}</p>}
        <div className="mail-section-title"><h2>{text("Mailboxes", "信箱")}</h2></div>
        {nav(text("All mailboxes", "全部信箱"), !accountId, () => { setAccountId(""); setOffset(0); setMessageId(null); }, workspace?.accounts.reduce((sum, item) => sum + item.message_count, 0))}
        {workspace?.accounts.map(item => <div key={item.id} className="mail-account-row">
          <button className={`mail-nav${accountId === item.id ? " is-active" : ""}`} title={item.email} onClick={() => { setAccountId(item.id); setOffset(0); setMessageId(null); }}>
            <span className="mail-nav-label"><span className="mail-ellipsis">{item.email}</span><small className={item.connected ? "" : "is-warning"}>{item.connected ? text(`Synced ${dateTime(item.last_synced)}`, `上次同步 ${dateTime(item.last_synced)}`) : text("Reconnect needed", "需要重新連接")}</small></span>
          </button>
          <button className="mail-quiet mail-icon-button" aria-label={text(`Manage ${item.email}`, `管理 ${item.email}`)} onClick={() => setEditor({ type: "account", account: item })}>{text("Manage", "管理")}</button>
        </div>)}
        {busy === connectLabel
          ? <button className="mail-wide" onClick={cancelConnect}>{text("Cancel Gmail sign-in", "取消 Gmail 登入")}</button>
          : <button className="mail-wide" disabled={!!busy} onClick={() => { void connectAccount(); }}>{text("Connect Gmail account", "連接 Gmail 帳戶")}</button>}
        <p className="mail-hint">{busy === connectLabel
          ? text("Finish signing in in your browser. If you closed the Google tab, choose Cancel.", "請在瀏覽器完成登入。如已關閉 Google 登入分頁，請按「取消 Gmail 登入」。")
          : text("Google shows a warning that the app is not verified yet. Choose Advanced, then Go to JARVIS, to continue.", "Google 會提示此應用程式尚未經驗證。請按「進階」，再按「前往 JARVIS」繼續。")}</p>
      </nav>

      <section className="mail-list-panel" aria-label={heading}>
        <div className="mail-toolbar">
          <h2>{heading}</h2>
          {view !== "due" && <input type="search" value={query} aria-label={text("Search local mail", "搜尋本機郵件")} placeholder={text("Search subject, sender, text or summary", "搜尋主旨、寄件人、內容或摘要")} onChange={e => { setQuery(e.target.value); setOffset(0); }} />}
          <div className="mail-toolbar-actions">
            <button disabled={!!busy || !(workspace?.accounts ?? []).some(item => item.connected)} onClick={() => sync(account && account.connected ? account : null)}>{busy === text("Sync mail", "同步郵件") ? text("Syncing…", "同步中…") : text("Sync", "同步")}</button>
            {triageReady
              ? <button className="mail-primary" disabled={!!busy || !triage?.untriaged} onClick={() => run(text("AI sorting", "AI 整理"), async () => setNotice(await triageNow(3)))}>{triage?.untriaged ? text(`Sort ${triage.untriaged} with AI`, `AI 整理 ${triage.untriaged} 封`) : text("All sorted", "已全部整理")}</button>
              : noAi
              ? <button className="mail-primary" disabled={!!busy} onClick={onSetup ?? openSettings}>{text("Set up your AI", "設定你的 AI")}</button>
              : <button className="mail-primary" disabled={!!busy || !workspace} onClick={openSettings}>{text("Turn on AI sorting", "開啟 AI 整理")}</button>}
          </div>
        </div>
        {selectionActive && view !== "due" && <div className="mail-selection" role="region" aria-label={text("Selected messages", "已選郵件")}>
          <span>{text(`${selected.length} selected`, `已選 ${selected.length} 封`)}</span>
          <select aria-label={text("Group to add to", "加入的群組")} value={targetGroup} onChange={e => setTargetGroup(e.target.value)}><option value="">{text("Add to group…", "加入群組…")}</option>{workspace?.groups.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select>
          <button disabled={!targetGroup || !!busy} onClick={() => run(text("Add to group", "加入群組"), async () => { await mail.mailSetGroupMembers(Number(targetGroup), selected, true); setSelected([]); setNotice(text("Added to group.", "已加入群組。")); })}>{text("Add", "加入")}</button>
          <button disabled={!!busy} onClick={() => setEditor({ type: "group", name: "", instructions: "", members: selected })}>{text("New group from selection", "以所選建立群組")}</button>
          {group && <button disabled={!!busy} onClick={() => run(text("Remove from group", "移出群組"), async () => { await mail.mailSetGroupMembers(group.id, selected, false); setSelected([]); setNotice(text("Removed from this group; messages are kept.", "已移出此群組，郵件仍然保留。")); })}>{text("Remove from group", "移出此群組")}</button>}
          <button className="mail-quiet" onClick={() => setSelected([])}>{text("Clear", "取消選取")}</button>
        </div>}
        {view === "due" ? <div className="mail-message-list"><MailAgenda text={text} locale={locale} revision={revision} onOpenSource={openSource} onError={setError} /></div> : <>
          <div className="mail-message-list" aria-busy={loading}>
            {page.messages.length === 0 ? (loading ? <p className="mail-empty">{text("Loading…", "正在載入…")}</p>
              : !workspace?.accounts.length ? <div className="mail-empty"><p>{text("Connect a Gmail account to start.", "連接 Gmail 帳戶即可開始。")}</p></div>
              : <p className="mail-empty">{query ? text("No local mail matches this search.", "沒有符合搜尋的本機郵件。") : view === "attention" ? text("Nothing needs your attention right now.", "目前沒有需要處理的郵件。") : text("No mail here yet.", "這裏暫時沒有郵件。")}</p>)
              : page.messages.map(item => {
                const sender = parseSender(item.sender); const attachments = item.attachments?.length ?? 0; const t = item.triage;
                return <div key={item.id} className={`mail-row${messageId === item.id ? " is-active" : ""}${item.is_read ? "" : " is-unread"}${t?.attention ? " needs-attention" : ""}`}>
                  <input type="checkbox" aria-label={text(`Select ${item.subject || "no subject"}`, `選取 ${item.subject || "無主旨"}`)} checked={selected.includes(item.id)} onChange={e => setSelected(ids => e.target.checked ? [...ids, item.id] : ids.filter(id => id !== item.id))} />
                  <button className="mail-row-main" onClick={() => setMessageId(item.id)} aria-current={messageId === item.id ? "true" : undefined}>
                    <span className="mail-row-top"><strong>{t?.sent_by_owner ? text("You (sent)", "你（已寄出）") : sender.name || text("Unknown sender", "未知寄件人")}</strong><time dateTime={item.received_at}>{new Date(item.received_at).toLocaleDateString(locale, { month: "short", day: "numeric" })}</time></span>
                    <span className="mail-row-subject"><span className="mail-ellipsis">{item.subject || text("(No subject)", "（無主旨）")}</span>{attachments > 0 && <Paperclip size={12} aria-label={text(`${attachments} attachments`, `${attachments} 個附件`)} />}{item.next_due && <span className="mail-due">{formatDue(item.next_due, null, locale, false)}</span>}</span>
                    <span className="mail-row-summary">{t && <CategoryTag category={t.category} text={text} compact />}<span className="mail-ellipsis">{t?.summary || decodeEntities(item.snippet)}</span></span>
                  </button>
                </div>;
              })}
          </div>
          <footer className="mail-pagination">
            <label><input type="checkbox" checked={page.messages.length > 0 && selected.length === page.messages.length} disabled={!page.messages.length} onChange={e => setSelected(e.target.checked ? page.messages.map(item => item.id) : [])} />{text("Select page", "全選本頁")}</label>
            <span>{page.messages.length ? `${offset + 1}–${offset + page.messages.length}${page.total === null ? "" : ` / ${page.total}`}` : "0"}</span>
            <span><button disabled={offset === 0 || loading} onClick={() => setOffset(n => Math.max(0, n - PAGE))} aria-label={text("Previous page", "上一頁")}>‹</button><button disabled={!page.has_more || loading} onClick={() => setOffset(n => n + PAGE)} aria-label={text("Next page", "下一頁")}>›</button></span>
          </footer>
        </>}
      </section>

      <section className="mail-detail" aria-label={text("Reading pane", "閱讀窗格")}>
        {reading && !message ? <p className="mail-empty">{text("Opening…", "正在開啟…")}</p>
          : message ? <article ref={reader} tabIndex={-1} className="mail-reader" aria-label={text("Email", "郵件")}>
            <div className="mail-reader-bar"><button onClick={() => setMessageId(null)}>{text("Close", "關閉")}</button>{sourceUrl && <button onClick={() => void openUrl(sourceUrl).catch(() => setError(text("Could not open Gmail. Try again.", "未能開啟 Gmail，請重試。")))}>{text("Open in Gmail", "在 Gmail 開啟")}</button>}</div>
            <h2 className="mail-reader-subject">{message.subject || text("(No subject)", "（無主旨）")}</h2>
            <p className="mail-from"><strong>{from?.name || text("Unknown sender", "未知寄件人")}</strong>{from?.address && <span>{from.address}</span>}</p>
            <p className="mail-hint">{new Date(message.received_at).toLocaleString(locale, { dateStyle: "full", timeStyle: "short" })}<br />{text(`In ${message.account_email}`, `收件信箱：${message.account_email}`)}</p>
            {message.triage ? <MailAiCard triage={message.triage} items={items} text={text} locale={locale} onError={setError}
                onChanged={next => { setItems(list => list.map(item => item.id === next.id ? next : item)); refreshWorkspace(); if (next.status !== "open") void mail.mailGetMessage(message.id).then(setMessage).catch(() => undefined); }}
                onAttention={attention => void mail.mailSetAttention(message.id, attention).then(() => { setMessage(m => m && m.triage ? { ...m, triage: { ...m.triage, attention } } : m); setPage(p => ({ ...p, messages: p.messages.map(m => m.id === message.id && m.triage ? { ...m, triage: { ...m.triage, attention } } : m) })); refreshWorkspace(); }).catch(e => setError(String(e)))} />
              : <p className="mail-ai-pending">{noAi ? noAiText : triageReady ? text("Not sorted yet. Choose “Sort with AI” to summarise it.", "尚未整理。按「AI 整理」即可生成摘要。") : text("Turn on AI sorting to get a summary and follow-ups for each email.", "開啟 AI 整理後，每封郵件都會有摘要及待辦。")}</p>}
            <ReadableContent format="plain" text={(message.body && readableBody(message.body)) || decodeEntities(message.snippet) || text("This email has no readable text.", "此郵件沒有可顯示的文字。")} />
            {files && files.length > 0 && <ul className="mail-attachments" aria-label={text("Attachments", "附件")}>{files.map((file, index) => <li key={index}><Paperclip size={13} aria-hidden="true" /><span>{file.filename}</span>{fileSize(file.size) && <small>{fileSize(file.size)}</small>}</li>)}</ul>}
            {(files === null || files.length > 0) && <p className="mail-hint">{text("Attachments stay in Gmail. Open the email in Gmail to download them.", "附件保留在 Gmail，請在 Gmail 開啟原信下載。")}</p>}
          </article>
          : group ? <section className="mail-group-detail">
            <div className="mail-section-title"><h2>{group.name}</h2><span className="mail-actions"><button onClick={() => setEditor({ type: "group", id: group.id, name: group.name, instructions: group.instructions })}>{text("Edit", "編輯")}</button><button className="mail-danger" onClick={() => setEditor({ type: "delete", kind: "group", id: group.id, name: group.name })}>{text("Delete", "刪除")}</button></span></div>
            <p className="mail-hint">{text(`${group.message_count} emails. Analysis covers the newest 50 in the group, from every mailbox.`, `共 ${group.message_count} 封。分析涵蓋群組內最新 50 封，不受信箱篩選影響。`)}</p>
            <h3>{text("Your instructions", "你的指示")}</h3>
            <p className="mail-preserve">{group.instructions || text("No instructions yet. Say how this group's mail should be organised and followed up.", "尚未設定指示。可寫下此群組郵件的整理及跟進方式。")}</p>
            <div className="mail-section-title"><h3>{text("Confirmed memories", "已確認的記憶")}</h3><button onClick={() => editMemory()}>{text("Add", "新增")}</button></div>
            {memoryLoading ? <p className="mail-hint">{text("Loading…", "正在載入…")}</p> : !memories.length && <p className="mail-hint">{text("Facts, preferences and commitments you confirm. Analysis never adds them by itself.", "由你確認的事實、偏好及承諾；分析不會自行新增。")}</p>}
            {memories.map(item => <div key={item.id} className="mail-memory"><small>{({ fact: text("Fact", "事實"), preference: text("Preference", "偏好"), commitment: text("Commitment", "承諾") } as Record<string, string>)[item.kind] || item.kind}{item.status === "done" ? text(", done", "，已完成") : ""}{item.expires_at ? text(`, until ${item.expires_at}`, `，至 ${item.expires_at}`) : ""}</small><p>{item.content}</p><div className="mail-actions">{item.source_message_id && <button onClick={() => item.source_message_id && openSource(item.source_message_id)}>{text(`Source #${item.source_message_id}`, `來源 #${item.source_message_id}`)}</button>}<button onClick={() => editMemory(item)}>{text("Edit", "編輯")}</button><button className="mail-danger" onClick={() => setEditor({ type: "delete", kind: "memory", id: item.id, name: item.content })}>{text("Delete", "刪除")}</button></div></div>)}
            <div className="mail-analysis">
              <h3>{text("Group analysis", "群組分析")}</h3>
              <p className="mail-hint">{workspace?.settings.processing_mode === "assistant" ? text("Uses the AI chosen in Settings → Your AI. The group's emails, instructions and active memories are sent to it.", "使用在「設定 → 你的 AI」選擇的 AI；群組郵件、指示及有效記憶會傳送至該 AI。") : workspace?.settings.processing_mode === "openai" ? text(`Uses ${workspace.settings.openai_model} on your OpenAI account. The group's emails, instructions and active memories are sent to OpenAI.`, `使用你 OpenAI 帳戶的 ${workspace.settings.openai_model}；群組郵件、指示及有效記憶會傳送至 OpenAI。`) : workspace?.settings.processing_mode === "bedrock" ? text("Uses your AWS Bedrock account. The group's emails, instructions and active memories are sent to AWS.", "使用你的 AWS Bedrock；群組郵件、指示及有效記憶會傳送至 AWS。") : text("Uses Ollama on this Mac only, with no cloud fallback.", "只使用此 Mac 上的 Ollama，不會自動改用雲端。")}</p>
              <button className="mail-primary" disabled={!!busy} onClick={() => run(text("Analyze group", "分析群組"), async () => { const result = await mail.mailAnalyzeGroup(group.id); setAnalysis({ groupId: group.id, result }); }, false)}>{busy === text("Analyze group", "分析群組") ? text("Analysing…", "分析中…") : text("Analyse this group", "分析此群組")}</button>
              {analysis?.groupId === group.id && <div className="mail-analysis-result">
                <p className="mail-hint">{analysisSource(analysis.result, text)}{analysis.result.input_tokens ? text(`, ${analysis.result.input_tokens + analysis.result.output_tokens} tokens`, `，${analysis.result.input_tokens + analysis.result.output_tokens} tokens`) : ""}{analysis.result.omitted_messages ? text(`, ${analysis.result.omitted_messages} emails not included`, `，${analysis.result.omitted_messages} 封未納入`) : ""}</p>
                <ReadableContent text={analysis.result.text} sourceIds={analysis.result.source_ids} onSource={openSource} />
              </div>}
            </div>
          </section>
          : <section className="mail-overview">
            <h2>{text("Overview", "概覽")}</h2>
            <div className="mail-overview-stats">
              <button onClick={() => choose({ view: "attention" })}><strong>{triage?.attention ?? 0}</strong><span>{text("need attention", "封需要處理")}</span></button>
              <button onClick={() => choose({ view: "due" })}><strong>{triage?.open_items ?? 0}</strong><span>{text("open follow-ups", "項未完成待辦")}</span></button>
              <button onClick={() => choose({ view: "untriaged" })}><strong>{triage?.untriaged ?? 0}</strong><span>{text("not sorted yet", "封尚未整理")}</span></button>
            </div>
            {!triageReady && <p className="mail-hint">{text("AI sorting reads each new email once. ", "AI 整理會讀取每封新郵件一次。")}{sortedBy}</p>}
            <p className="mail-hint">{text("Select an email to read it here. Use j and k to move through the list.", "選取郵件即可在此閱讀；可按 j、k 上下移動。")}</p>
          </section>}
      </section>
    </div>

    {editor && <dialog ref={dialog} className="mail-dialog" onCancel={e => { e.preventDefault(); setEditor(null); }}><form onSubmit={e => { e.preventDefault(); void saveEditor(); }}><fieldset disabled={!!busy}>
      {editor.type === "group" && <><h2>{editor.id ? text("Edit group", "編輯群組") : text("New group", "新增群組")}</h2>{editor.members?.length ? <p className="mail-hint">{text(`The ${editor.members.length} selected emails will be added.`, `會加入已選的 ${editor.members.length} 封郵件。`)}</p> : null}<label>{text("Group name", "群組名稱")}<input autoFocus required maxLength={100} value={editor.name} onChange={e => setEditor({ ...editor, name: e.target.value })} /></label><label>{text("Your instructions", "你的指示")}<textarea rows={6} maxLength={8000} value={editor.instructions} onChange={e => setEditor({ ...editor, instructions: e.target.value })} placeholder={text("For example: list the questions I need to answer and their deadlines.", "例如：列出需要我回覆的問題及截止日期。")} /></label></>}
      {editor.type === "memory" && <><h2>{editor.input.id ? text("Edit memory", "編輯記憶") : text("Add memory", "新增記憶")}</h2><label>{text("Content", "內容")}<textarea autoFocus required maxLength={2000} rows={4} value={editor.input.content} onChange={e => setEditor({ ...editor, input: { ...editor.input, content: e.target.value } })} /></label><label>{text("Kind", "種類")}<select value={editor.input.kind} onChange={e => setEditor({ ...editor, input: { ...editor.input, kind: e.target.value } })}><option value="fact">{text("Fact", "事實")}</option><option value="preference">{text("Preference", "偏好")}</option><option value="commitment">{text("Commitment", "承諾")}</option></select></label><label>{text("Status", "狀態")}<select value={editor.input.status} onChange={e => setEditor({ ...editor, input: { ...editor.input, status: e.target.value } })}><option value="active">{text("Active", "進行中")}</option><option value="done">{text("Done", "已完成")}</option></select></label><label>{text("Source email ID (optional; must be in this group)", "來源郵件編號（可留空，必須屬於此群組）")}<input type="number" min={1} step={1} value={editor.input.source_message_id ?? ""} onChange={e => setEditor({ ...editor, input: { ...editor.input, source_message_id: e.target.value ? Number(e.target.value) : null } })} /></label><label>{text("Until (optional)", "到期日（可留空）")}<input type="date" value={editor.input.expires_at ?? ""} onChange={e => setEditor({ ...editor, input: { ...editor.input, expires_at: e.target.value || null } })} /></label><p className="mail-hint">{text("Done or expired memories are left out of analysis.", "已完成或已到期的記憶不會納入分析。")}</p></>}
      {editor.type === "account" && <><h2>{editor.account.email}</h2>
        <p className="mail-hint">{editor.account.connected ? text(`Last complete sync: ${dateTime(editor.account.last_synced)}. ${editor.account.message_count} emails on this Mac.`, `上次完整同步：${dateTime(editor.account.last_synced)}。此 Mac 有 ${editor.account.message_count} 封郵件。`) : text("Google no longer accepts this connection. Reconnect with “Connect Gmail account”; local mail is kept.", "Google 已不接受此連接。請按「連接 Gmail 帳戶」重新連接；本機郵件會保留。")}</p>
        <div className="mail-account-actions">
          <button type="button" disabled={!editor.account.connected || !!busy} onClick={() => { const target = editor.account; setEditor(null); void sync(target, "recent"); }}>{text("Sync new mail", "同步新郵件")}</button>
          <button type="button" disabled={!editor.account.connected || !editor.account.next_page_token || !!busy} onClick={() => { const target = editor.account; setEditor(null); void sync(target, "older"); }}>{text("Load 50 older emails", "載入 50 封較舊郵件")}</button>
          <button type="button" disabled={!editor.account.connected || !!busy} onClick={() => { const target = editor.account; setEditor(null); void sync(target, "restart"); }}>{text("Rescan from newest", "由最新郵件重新掃描")}</button>
          {editor.account.connected && <button type="button" disabled={!!busy} onClick={() => { const target = editor.account; setEditor(null); void run(text("Disconnect account", "斷開帳戶"), async () => { await mail.mailDisconnectAccount(target.id); setNotice(text("Disconnected; local mail is kept.", "已斷開；本機郵件仍然保留。")); }); }}>{text("Disconnect, keep mail", "斷開連接，保留郵件")}</button>}
          <button type="button" className="mail-danger" onClick={() => setEditor({ type: "delete", kind: "account", id: editor.account.id, name: editor.account.email })}>{text("Delete account and local mail", "刪除帳戶及本機郵件")}</button>
        </div>
        <p className="mail-hint">{text("Rescan re-reads the newest 50 emails and restarts “Load older” from there. Use it to fill a gap left by an older version.", "重新掃描會重新讀取最新 50 封，並由此重新開始「載入較舊郵件」；可用來補回舊版本留下的缺口。")}</p></>}
      {editor.type === "settings" && <>
        <h2>{text("Analysis settings", "分析設定")}</h2>
        <section className="mail-settings-block">
          <h3>{text("AI sorting", "AI 自動整理")}</h3>
          <p className="mail-hint">{workspace?.triage.typesafe_key
            ? text(`Each new email is read once. TypeSafe Jev gets the sender, recipients, subject, the first 1,500 characters and your mailbox addresses to decide its category and whether it needs you. Only emails that need a summary or mention a date go to ${chosenAi ?? "your AI"} (up to 6,000 characters). Nothing is sent from your Gmail or added to your calendar without your click.`, `每封新郵件只讀取一次：TypeSafe Jev 會收到寄件人、收件人、主旨、正文開頭 1,500 字及你的信箱地址，用來判斷分類及是否需要你處理；只有需要摘要或含日期的郵件，才會把最多 6,000 字傳送至${chosenAi ? ` ${chosenAi}` : "你選擇的 AI"}。未經你按下按鈕，不會寄出郵件或加入日曆。`)
            : text(`Each new email is read once. The sender, recipients, subject and up to 6,000 characters go to ${chosenAi ?? "the AI you choose"}, which decides the category, writes a summary and finds dates. Nothing is sent from your Gmail or added to your calendar without your click.`, `每封新郵件只讀取一次：寄件人、收件人、主旨及最多 6,000 字正文會傳送至${chosenAi ? ` ${chosenAi}` : "你選擇的 AI"}，用來判斷分類、撰寫摘要及找出日期。未經你按下按鈕，不會寄出郵件或加入日曆。`)}</p>
          {!chosenAi && <p className="mail-hint">{text("No AI chosen yet: choose one in Settings → Your AI.", "尚未選擇 AI：請到「設定 → 你的 AI」選擇。")}</p>}
          <label>{text("TypeSafe API key (optional, cheaper sorting)", "TypeSafe API 密鑰（選用，分類較便宜）")}<input type="password" autoComplete="off" value={editor.typesafeKey} placeholder={workspace?.triage.typesafe_key ? text("Saved. Enter a new key to replace it.", "已儲存；輸入新密鑰即可取代。") : text("Optional", "選用")} onChange={e => setEditor({ ...editor, typesafeKey: e.target.value })} /></label>
          <p className="mail-hint">{text(`With a TypeSafe key, Jev sorts every email first and only the ones that need a summary go to ${chosenAi ?? "your AI"}.`, `加入 TypeSafe 密鑰後，由 Jev 先分類每封郵件，只有需要摘要的郵件才交給${chosenAi ? ` ${chosenAi}` : "你選擇的 AI"}。`)}</p>
          <label className="mail-consent"><input type="checkbox" checked={editor.triage} onChange={e => setEditor({ ...editor, triage: e.target.checked, triageConsent: e.target.checked && editor.triageConsent })} />{text("Sort new mail with AI", "以 AI 整理新郵件")}</label>
          {editor.triage && <label className="mail-consent"><input type="checkbox" required checked={editor.triageConsent} onChange={e => setEditor({ ...editor, triageConsent: e.target.checked })} />{(editor.typesafeKey.trim() || workspace?.triage.typesafe_key)
            ? text(`I understand that email text is sent to TypeSafe and, for some emails, ${chosenAi ?? "my AI"}, as described above.`, `我明白郵件內容會按上述方式傳送至 TypeSafe，部分郵件亦會傳送至${chosenAi ? ` ${chosenAi}` : "我選擇的 AI"}。`)
            : text(`I understand that email text is sent to ${chosenAi ?? "my AI"}, as described above.`, `我明白郵件內容會按上述方式傳送至${chosenAi ? ` ${chosenAi}` : "我選擇的 AI"}。`)}</label>}
        </section>
        <section className="mail-settings-block">
          <h3>{text("Group analysis", "群組分析")}</h3>
          <label>{text("Processing", "處理方式")}<select value={editor.input.processing_mode} onChange={e => setEditor({ ...editor, consent: false, input: { ...editor.input, processing_mode: e.target.value } })}>
            <option value="assistant">{text("Same AI as the assistant", "與 AI 助理相同的 AI")}</option>
            <option value="local">{text("Ollama on this Mac", "此 Mac 的 Ollama")}</option>
            <option value="openai">OpenAI</option>
            <option value="bedrock">{text("Your AWS Bedrock account", "自備 AWS Bedrock 帳戶")}</option>
          </select></label>
          {editor.input.processing_mode === "assistant" ? <>
            <p className="mail-hint">{text("Uses the AI chosen in Settings → Your AI, with its saved key.", "使用在「設定 → 你的 AI」選擇的 AI 及其已儲存的密鑰。")}</p>
            <label className="mail-consent"><input type="checkbox" required checked={editor.consent} onChange={e => setEditor({ ...editor, consent: e.target.checked })} />{text("I understand that analysis sends the group's emails, instructions and active memories to that AI (unless it is Ollama on this Mac).", "我明白分析時，群組郵件、指示及有效記憶會傳送至該 AI（如為此 Mac 的 Ollama 則不會離開此 Mac）。")}</label>
          </> : editor.input.processing_mode === "local" ? <>
            <label>{text("Ollama model name", "Ollama 模型名稱")}<input value={editor.input.local_model} onChange={e => setEditor({ ...editor, input: { ...editor.input, local_model: e.target.value } })} placeholder={text("An installed model, e.g. qwen3:8b", "已安裝的模型，例如 qwen3:8b")} /></label>
            <p className="mail-hint">{text("Install and start Ollama in Settings → Local AI first.", "請先在設定的「本機 AI」安裝並啟動 Ollama。")}</p>
          </> : editor.input.processing_mode === "openai" ? <>
            <label>{text("OpenAI model", "OpenAI 模型")}<input required value={editor.input.openai_model} onChange={e => setEditor({ ...editor, input: { ...editor.input, openai_model: e.target.value } })} placeholder={models?.defaults.openai} /></label>
            <p className="mail-hint">{openAiPriceHint(editor.input.openai_model.trim() || models?.defaults.openai || "")}</p>
            <label className="mail-consent"><input type="checkbox" required checked={editor.consent} onChange={e => setEditor({ ...editor, consent: e.target.checked })} />{text("I understand that analysis sends the group's emails, instructions and active memories to OpenAI.", "我明白分析時，群組郵件、指示及有效記憶會傳送至 OpenAI。")}</label>
          </> : <>
            <BedrockModelPicker place="mail" value={editor.input.bedrock_model} stored={workspace?.settings.bedrock_model ?? ""} fallback={models?.defaults.bedrock ?? ""} listState={bedrockList.list} onReload={bedrockList.reload} onChange={model => setEditor({ ...editor, input: { ...editor.input, bedrock_model: model } })} />
            <p className="mail-hint">{text("Add your AWS credentials and region in Settings → Models first. Your AWS account pays.", "請先在設定的「模型」頁面填入 AWS 憑證及地區；費用由你的 AWS 帳戶支付。")}</p>
            <label className="mail-consent"><input type="checkbox" required checked={editor.consent} onChange={e => setEditor({ ...editor, consent: e.target.checked })} />{text("I understand that analysis sends the group's emails, instructions and active memories to AWS Bedrock.", "我明白分析時，群組郵件、指示及有效記憶會傳送至 AWS Bedrock。")}</label>
          </>}
        </section>
      </>}
      {editor.type === "delete" && <><h2>{text("Confirm deletion", "確認刪除")}</h2><p className="mail-preserve">{editor.name}</p><p>{editor.kind === "account" ? text("This deletes the account, its local emails, their group memberships, AI results and memories sourced from them. Your Gmail is not affected.", "將刪除帳戶、其本機郵件、相關群組連結、AI 整理結果，以及以這些郵件為來源的記憶。Gmail 中的原信不受影響。") : editor.kind === "group" ? text("This deletes the group, its instructions and memories. Emails are kept.", "將刪除群組、指示及記憶；郵件仍然保留。") : text("A deleted memory cannot be restored.", "記憶刪除後無法還原。")}</p></>}
      {error && <p role="alert" className="mail-alert">{error}</p>}
      <footer className="mail-actions">
        <button type="button" onClick={() => setEditor(null)}>{editor.type === "account" ? text("Close", "關閉") : text("Cancel", "取消")}</button>
        {editor.type !== "account" && <button type="submit" className={editor.type === "delete" ? "mail-danger" : "mail-primary"} disabled={(editor.type === "group" && !editor.name.trim()) || (editor.type === "memory" && !editor.input.content.trim()) || (editor.type === "settings" && editor.input.processing_mode === "bedrock" && !modelChosen(editor.input.bedrock_model))}>{busy ? text("Working…", "處理中…") : editor.type === "delete" ? text("Delete", "刪除") : text("Save", "儲存")}</button>}
      </footer>
    </fieldset></form></dialog>}
  </div>;
}
