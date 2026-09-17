import { useEffect, useRef, useState } from "react";
import * as mail from "../lib/mail";
import type { MailAnalysis, MailMemory, MailMemoryInput, MailMessage, MailPage, MailSettings, MailWorkspace } from "../lib/mail";
import { useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./email-workspace.css";

type Editor = { type: "group"; id?: number; name: string; instructions: string } | { type: "memory"; input: MailMemoryInput } | { type: "settings"; input: MailSettings; consent: boolean } | { type: "delete"; kind: "account" | "group" | "memory"; id: string | number; name: string };
const date = (value: string | null) => value ? new Date(value).toLocaleString("zh-HK") : "未同步";

export default function EmailPage({ onSetup }: { onSetup?: () => void }) {
  const [workspace, setWorkspace] = useState<MailWorkspace | null>(null);
  const [accountId, setAccountId] = useWorkspaceValue("mailAccountId", "");
  const [groupId, setGroupId] = useWorkspaceValue<number | undefined>("mailGroupId", undefined);
  const [query, setQuery] = useWorkspaceValue("mailSearch", "");
  const [offset, setOffset] = useWorkspaceValue("mailOffset", 0);
  const [page, setPage] = useState<MailPage>({ messages: [], total: 0 });
  const [selected, setSelected] = useState<number[]>([]);
  const [targetGroup, setTargetGroup] = useState("");
  const [messageId, setMessageId] = useWorkspaceValue<number | null>("mailMessageId", null);
  const [message, setMessage] = useState<MailMessage | null>(null);
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
  const dialog = useRef<HTMLDialogElement>(null);
  const group = workspace?.groups.find(item => item.id === groupId);
  const account = workspace?.accounts.find(item => item.id === accountId);

  useEffect(() => {
    let current = true;
    mail.mailWorkspace().then(data => { if (current) setWorkspace(data); }).catch(e => { if (current) setError(`載入工作區失敗：${String(e)}`); });
    return () => { current = false; };
  }, [revision]);
  useEffect(() => {
    let current = true;
    setLoading(true); setSelected([]); setPage({ messages: [], total: 0 });
    const timer = window.setTimeout(() => {
      mail.mailListMessages({ accountId: accountId || undefined, groupId, query, offset }).then(data => {
        if (current) { setPage(data); if (offset > 0 && offset >= data.total) setOffset(0); }
      }).catch(e => { if (current) setError(`未能載入郵件：${String(e)}`); }).finally(() => { if (current) setLoading(false); });
    }, 200);
    return () => { current = false; window.clearTimeout(timer); };
  }, [accountId, groupId, query, offset, revision]);
  useEffect(() => {
    let current = true; setMessage(null); setReading(messageId !== null);
    if (messageId !== null) mail.mailGetMessage(messageId).then(data => { if (current) setMessage(data); }).catch(e => { if (current) setError(`未能開啟郵件：${String(e)}`); }).finally(() => { if (current) setReading(false); });
    return () => { current = false; };
  }, [messageId, revision]);
  useEffect(() => {
    let current = true; setMemories([]); setMemoryLoading(groupId !== undefined);
    if (groupId !== undefined) mail.mailListMemories(groupId).then(data => { if (current) setMemories(data); }).catch(e => { if (current) setError(`載入記憶失敗：${String(e)}`); }).finally(() => { if (current) setMemoryLoading(false); });
    return () => { current = false; };
  }, [groupId, revision]);
  useEffect(() => { if (editor && dialog.current && !dialog.current.open) dialog.current.showModal(); }, [editor]);

  async function run(label: string, action: () => Promise<void>, refresh = true) {
    if (operation.current) return;
    operation.current = true; setBusy(label); setError(""); setNotice("");
    try { await action(); if (refresh) { setRevision(n => n + 1); setSelected([]); setAnalysis(null); } }
    catch (e) { setError(`${label}失敗：${String(e)}`); }
    finally { operation.current = false; setBusy(""); }
  }
  function chooseGroup(id?: number) { setGroupId(id); setOffset(0); setSelected([]); setMessageId(null); }
  function editMemory(item?: MailMemory) {
    if (!group) return;
    setEditor({ type: "memory", input: item ? { ...item } : { id: null, group_id: group.id, content: "", kind: "fact", source_message_id: null, expires_at: null, status: "active" } });
  }
  async function saveEditor() {
    if (!editor) return;
    const draft = editor;
    await run("儲存", async () => {
      if (draft.type === "group") await mail.mailSaveGroup(draft.name.trim(), draft.instructions, draft.id);
      if (draft.type === "memory") await mail.mailSaveMemory({ ...draft.input, content: draft.input.content.trim() });
      if (draft.type === "settings") await mail.mailSaveSettings(draft.input);
      if (draft.type === "delete") {
        if (draft.kind === "account") { await mail.mailRemoveAccount(String(draft.id)); if (accountId === draft.id) setAccountId(""); }
        if (draft.kind === "group") { await mail.mailDeleteGroup(Number(draft.id)); if (groupId === draft.id) chooseGroup(); if (targetGroup === String(draft.id)) setTargetGroup(""); }
        if (draft.kind === "memory") await mail.mailDeleteMemory(Number(draft.id));
      }
      setEditor(null); setNotice(draft.type === "delete" ? "已刪除。" : "已儲存。");
    });
  }
  async function sync(older: boolean, restart = false) {
    if (!account) return;
    await run("同步郵件", async () => { const result = restart ? await mail.mailRestartSync(account.id) : await mail.mailSyncAccount(account.id, older); setNotice(`已儲存 ${result.saved} 封郵件。${result.has_more ? "仍有更舊的郵件可供載入。" : result.warning ? "同步未完成。" : "本次沒有更多郵件。"}${result.warning ? ` ${result.warning} 請再按「${restart ? "由最新郵件重新掃描" : older ? "載入更舊郵件" : "同步最近郵件"}」重試。` : ""}`); });
  }

  return <div className="mail-workspace">
    <header className="mail-heading"><div><h1>電郵工作區</h1><p>集中管理多個信箱的本機副本，透過群組整理郵件與記憶。</p></div>{onSetup && <button disabled={!!busy} onClick={onSetup}>設定導引</button>}<button disabled={!!busy || !workspace} onClick={() => workspace && setEditor({ type: "settings", input: { ...workspace.settings }, consent: workspace.settings.processing_mode !== "local" })}>分析設定</button></header>
    {error && <div role="alert" className="mail-alert">{error}<button onClick={() => { setError(""); setRevision(n => n + 1); }}>重新載入</button></div>}
    <div role="status" className="mail-status">{busy ? `${busy}中，請稍等…` : notice || "郵件可讀文字儲存於此 Mac；部分格式可能只能取得摘要，附件尚未下載。"}</div>
    <fieldset disabled={!!busy} className={`mail-layout${messageId !== null ? " has-message" : ""}`} aria-busy={!!busy}>
      <aside className="mail-sidebar">
        <h2>信箱</h2>
        <button className={`mail-nav ${!accountId ? "is-active" : ""}`} onClick={() => { setAccountId(""); setOffset(0); setMessageId(null); }}>全部信箱 <span>{workspace?.accounts.reduce((sum, item) => sum + item.message_count, 0) ?? 0}</span></button>
        {workspace?.accounts.map(item => <button key={item.id} className={`mail-nav ${accountId === item.id ? "is-active" : ""}`} onClick={() => { setAccountId(item.id); setOffset(0); setMessageId(null); }}><span className="mail-truncate">{item.email}<small>{item.connected ? "已連接" : "已斷開・保留本機副本"}</small></span><span>{item.message_count}</span></button>)}
        <button className="mail-wide" onClick={() => run("連接 Gmail", async () => { const connected = await mail.mailConnectAccount(); setAccountId(connected.id); setOffset(0); setNotice("已連接。請按「同步最近郵件」下載首批郵件。"); })}>連接 Gmail 帳戶</button>
        <p className="mail-hint">每個 Gmail 帳戶需分別授權；此處的連接與日曆設定互相獨立。</p>
        <div className="mail-section-title"><h2>群組</h2><button onClick={() => setEditor({ type: "group", name: "", instructions: "" })}>新增</button></div>
        <button className={`mail-nav ${groupId === undefined ? "is-active" : ""}`} onClick={() => chooseGroup()}>全部郵件</button>
        {workspace?.groups.map(item => <button key={item.id} className={`mail-nav ${item.id === groupId ? "is-active" : ""}`} onClick={() => chooseGroup(item.id)}><span className="mail-truncate">{item.name}</span><span>{item.message_count}</span></button>)}
        {account && <section className="mail-account"><h2>信箱同步</h2><p className="mail-hint">上次同步：{date(account.last_synced)}</p><button disabled={!account.connected} onClick={() => sync(false)}>同步最近郵件</button><button disabled={!account.connected || !account.next_page_token} onClick={() => sync(true)}>載入更舊郵件</button><p className="mail-hint">每次最多同步 50 封，不代表已備份整個信箱。</p><button disabled={!account.connected} onClick={() => sync(false, true)}>由最新郵件重新掃描</button><p className="mail-hint">保留本機郵件，並重設較舊郵件的載入位置；適合補回大量新郵件。</p>{account.connected && <button onClick={() => run("斷開帳戶", async () => { await mail.mailDisconnectAccount(account.id); setNotice("已斷開；本機郵件仍然保留。"); })}>斷開連接，保留郵件</button>}<button className="mail-danger" onClick={() => setEditor({ type: "delete", kind: "account", id: account.id, name: account.email })}>刪除帳戶及本機郵件</button></section>}
      </aside>
      <section className="mail-list-panel" aria-label="本機郵件">
        <label className="mail-search">搜尋本機郵件<input type="search" value={query} placeholder="主旨、寄件人或內容" onChange={e => { setQuery(e.target.value); setOffset(0); setMessageId(null); }} /></label>
        <div className="mail-selection"><label><input type="checkbox" checked={page.messages.length > 0 && selected.length === page.messages.length} disabled={!page.messages.length || loading} onChange={e => setSelected(e.target.checked ? page.messages.map(item => item.id) : [])} />全選本頁</label><span>已選 {selected.length} 封</span></div>
        <div className="mail-membership"><select aria-label="選擇要加入的群組" value={targetGroup} onChange={e => setTargetGroup(e.target.value)}><option value="">選擇群組</option>{workspace?.groups.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select><button disabled={!selected.length || !targetGroup || loading} onClick={() => run("加入群組", async () => { await mail.mailSetGroupMembers(Number(targetGroup), selected, true); setNotice("已加入群組。"); })}>加入</button><button disabled={!selected.length || !group || loading} onClick={() => group && run("移出群組", async () => { await mail.mailSetGroupMembers(group.id, selected, false); setNotice("已移出此群組，郵件仍然保留。"); })}>移出此群組</button></div>
        <div className="mail-message-list" aria-busy={loading}>
          {loading ? <p className="mail-empty">正在載入郵件…</p> : page.messages.length === 0 ? <p className="mail-empty">沒有符合條件的本機郵件。請調整搜尋或篩選條件，或選擇一個信箱進行同步。</p> : page.messages.map(item => <div key={item.id} className={`mail-row ${messageId === item.id ? "is-active" : ""}`}><input aria-label={`選取 ${item.subject || "無主旨"}`} type="checkbox" checked={selected.includes(item.id)} onChange={e => setSelected(ids => e.target.checked ? [...ids, item.id] : ids.filter(id => id !== item.id))} /><button className="mail-message-button" onClick={() => setMessageId(item.id)}><span className="mail-row-meta"><strong>{item.sender}</strong><time>{new Date(item.received_at).toLocaleDateString("zh-HK")}</time></span><span className="mail-subject">{!item.is_read && <i aria-label="未讀" />}{item.subject || "（無主旨）"}</span><span className="mail-snippet">{item.snippet}</span><small>{item.account_email}</small></button></div>)}
        </div><footer className="mail-pagination"><button disabled={offset === 0 || loading} onClick={() => setOffset(n => Math.max(0, n - 50))}>上一頁</button><span>{page.total ? `${offset + 1}–${Math.min(offset + 50, page.total)}` : "0"} / {page.total}</span><button disabled={offset + 50 >= page.total || loading} onClick={() => setOffset(n => n + 50)}>下一頁</button></footer>
      </section>
      <section className="mail-detail">
        {reading ? <p className="mail-empty">正在開啟郵件…</p> : message ? <article className="mail-reader"><div className="mail-section-title"><h2>郵件</h2><button onClick={() => setMessageId(null)}>收起</button></div><h3>{message.subject || "（無主旨）"}</h3><p>{message.sender}</p><p className="mail-hint">{message.account_email}<br />{date(message.received_at)}</p><pre>{message.body || message.snippet || "此郵件沒有可顯示的文字。"}</pre></article> : <p className="mail-empty">選取一封郵件，即可在此查看本機內容。</p>}
        {group ? <section className="mail-group-detail"><div className="mail-section-title"><h2>{group.name}</h2><button onClick={() => setEditor({ type: "group", id: group.id, name: group.name, instructions: group.instructions })}>編輯群組</button></div><p className="mail-hint">群組共有 {group.message_count} 封；分析範圍涵蓋整個群組，不受左側信箱篩選影響。最多納入最近 50 封郵件，本機模型另有容量限制。</p><h3>個人指示</h3><p className="mail-preserve">{group.instructions || "尚未設定指示。可填寫此群組郵件的整理、回應及跟進方式。"}</p><div className="mail-section-title"><h3>群組記憶</h3><button onClick={() => editMemory()}>新增記憶</button></div>{memoryLoading ? <p className="mail-hint">正在載入記憶…</p> : !memories.length && <p className="mail-hint">記錄已確認的事實、偏好及承諾。分析不會自動新增記憶。</p>}{memories.map(item => <div key={item.id} className="mail-memory"><small>{({ fact: "事實", preference: "偏好", commitment: "承諾" } as Record<string, string>)[item.kind] || item.kind} · {item.status === "done" ? "已完成" : "進行中"}{item.expires_at ? ` · 到期 ${item.expires_at}` : ""}</small><p>{item.content}</p><div className="mail-actions">{item.source_message_id && <button onClick={() => setMessageId(item.source_message_id)}>來源 #{item.source_message_id}</button>}<button onClick={() => editMemory(item)}>編輯</button><button className="mail-danger" onClick={() => setEditor({ type: "delete", kind: "memory", id: item.id, name: item.content })}>刪除</button></div></div>)}<div className="mail-analysis"><h3>群組分析</h3><p className="mail-hint">{workspace?.settings.processing_mode === "openai" ? "使用你的 OpenAI API；此群組的郵件內容、指示及有效記憶會傳送至 OpenAI。" : workspace?.settings.processing_mode === "bedrock" ? "使用你的 AWS Bedrock；此群組的郵件內容、指示及有效記憶會傳送至 AWS。" : "僅使用本機 Ollama；失敗時不會自動改用雲端。"}</p><button className="mail-primary" onClick={() => run("分析群組", async () => { const result = await mail.mailAnalyzeGroup(group.id); setAnalysis({ groupId: group.id, result }); }, false)}>分析此群組</button>{analysis?.groupId === group.id && <div className="mail-analysis-result"><p className="mail-hint">{analysis.result.mode === "openai" ? "OpenAI GPT" : analysis.result.mode === "bedrock" ? "AWS Bedrock" : "本機 Ollama"} / {analysis.result.model}</p><p>未納入郵件：{analysis.result.omitted_messages} 封</p><pre>{analysis.result.text}</pre><div className="mail-actions">{analysis.result.source_ids.map(id => <button key={id} onClick={() => setMessageId(id)}>來源 #{id}</button>)}</div></div>}</div><button className="mail-danger" onClick={() => setEditor({ type: "delete", kind: "group", id: group.id, name: group.name })}>刪除群組</button></section> : <p className="mail-empty">選取一個群組，以管理指示、記憶及分析。</p>}
      </section>
    </fieldset>
    {editor && <dialog ref={dialog} className="mail-dialog" onCancel={e => { e.preventDefault(); }}><form onSubmit={e => { e.preventDefault(); void saveEditor(); }}><fieldset disabled={!!busy}>
      {editor.type === "group" && <><h2>{editor.id ? "編輯群組" : "新增群組"}</h2><label>群組名稱<input autoFocus required maxLength={100} value={editor.name} onChange={e => setEditor({ ...editor, name: e.target.value })} /></label><label>個人指示<textarea rows={6} maxLength={8000} value={editor.instructions} onChange={e => setEditor({ ...editor, instructions: e.target.value })} placeholder="例如：以繁體書面語整理，列出需要我回覆的問題及截止日期。" /></label></>}
      {editor.type === "memory" && <><h2>{editor.input.id ? "編輯記憶" : "新增記憶"}</h2><label>內容<textarea autoFocus required maxLength={2000} rows={4} value={editor.input.content} onChange={e => setEditor({ ...editor, input: { ...editor.input, content: e.target.value } })} /></label><label>種類<select value={editor.input.kind} onChange={e => setEditor({ ...editor, input: { ...editor.input, kind: e.target.value } })}><option value="fact">事實</option><option value="preference">偏好</option><option value="commitment">承諾</option></select></label><label>狀態<select value={editor.input.status} onChange={e => setEditor({ ...editor, input: { ...editor.input, status: e.target.value } })}><option value="active">進行中</option><option value="done">已完成</option></select></label><label>來源郵件編號（可留空，必須屬於此群組）<input type="number" min={1} step={1} value={editor.input.source_message_id ?? ""} onChange={e => setEditor({ ...editor, input: { ...editor.input, source_message_id: e.target.value ? Number(e.target.value) : null } })} /></label><label>到期日（可留空）<input type="date" value={editor.input.expires_at ?? ""} onChange={e => setEditor({ ...editor, input: { ...editor.input, expires_at: e.target.value || null } })} /></label><p className="mail-hint">已完成或已到期的記憶不會納入分析。</p></>}
      {editor.type === "settings" && <>
        <h2>電郵分析設定</h2>
        <label>處理方式<select value={editor.input.processing_mode} onChange={e => setEditor({ ...editor, consent: false, input: { ...editor.input, processing_mode: e.target.value } })}>
          <option value="local">本機 Ollama</option>
          <option value="openai">OpenAI GPT</option>
          <option value="bedrock">自備 AWS Bedrock 帳戶</option>
        </select></label>
        {editor.input.processing_mode === "local" ? <>
          <label>Ollama 模型名稱<input required value={editor.input.local_model} onChange={e => setEditor({ ...editor, input: { ...editor.input, local_model: e.target.value } })} placeholder="輸入已安裝的模型名稱" /></label>
          <p className="mail-hint">請先在設定的「本機 AI」中安裝並啟動 Ollama，再填入已下載的模型名稱。工作區僅連接此 Mac 上的 Ollama。</p>
        </> : editor.input.processing_mode === "openai" ? <>
          <label>GPT 模型名稱<input required value={editor.input.openai_model} onChange={e => setEditor({ ...editor, input: { ...editor.input, openai_model: e.target.value } })} placeholder="gpt-5.6-luna" /></label>
          <p className="mail-hint">使用設定頁儲存的 OpenAI API 密鑰；分析費用由你的 OpenAI 帳戶支付。郵件副本仍保留於本機，分析不會自動轉用其他服務。</p>
          <label className="mail-consent"><input type="checkbox" required checked={editor.consent} onChange={e => setEditor({ ...editor, consent: e.target.checked })} />我明白執行分析時，群組郵件內容、個人指示及有效記憶會傳送至 OpenAI。</label>
        </> : <>
          <label>Bedrock 模型 ID<input required value={editor.input.bedrock_model} onChange={e => setEditor({ ...editor, input: { ...editor.input, bedrock_model: e.target.value } })} /></label>
          <p className="mail-hint">請先在設定的「模型」頁面內，於 AWS 區塊填入自己的憑證及地區，或使用設定導引。分析費用由你的 AWS 帳戶支付。</p>
          <label className="mail-consent"><input type="checkbox" required checked={editor.consent} onChange={e => setEditor({ ...editor, consent: e.target.checked })} />我明白執行分析時，群組郵件內容、個人指示及有效記憶會傳送至 AWS Bedrock。</label>
        </>}
      </>}
      {editor.type === "delete" && <><h2>確認刪除</h2><p className="mail-preserve">{editor.name}</p><p>{editor.kind === "account" ? "將刪除帳戶、本機郵件、相關群組連結，以及以此帳戶郵件為來源的記憶；其他自行新增的記憶會保留。Gmail 中的原始郵件不受影響。" : editor.kind === "group" ? "將刪除群組、指示及記憶；本機郵件仍然保留。" : "此記憶刪除後無法還原。"}</p></>}
      {error && <p role="alert" className="mail-alert">{error}</p>}<footer className="mail-actions"><button type="button" onClick={() => setEditor(null)}>取消</button><button type="submit" className={editor.type === "delete" ? "mail-danger" : "mail-primary"} disabled={(editor.type === "group" && !editor.name.trim()) || (editor.type === "memory" && !editor.input.content.trim())}>{busy ? "處理中…" : editor.type === "delete" ? "確認刪除" : "儲存"}</button></footer>
    </fieldset></form></dialog>}
  </div>;
}
