import { useUiLocale } from '../i18n';
import { useState } from 'react';
import { ArrowLeft, ExternalLink, Copy, FileText } from 'lucide-react';
import { useWorkspace, useWorkspaceValue } from './WorkspaceContext';
import { validResult, resultTitle, providerUrl, type ToolResult } from './results';
import { openUrl } from '../lib/commands';
import ReadableContent from '../components/content/ReadableContent';

export default function ResultWorkspace() {
  const { text: uiText } = useUiLocale();
  const [payload] = useWorkspaceValue<ToolResult | null>('toolResult', null);
  const { dispatch, open } = useWorkspace();
  const [error, setError] = useState('');
  const [selected, setSelected] = useWorkspaceValue('selectedResult', 0);
  const [draft, setDraft] = useWorkspaceValue('localDraft', typeof payload?.result.body === 'string' ? payload.result.body : '');
  const [copied, setCopied] = useState(false);
  if (!validResult(payload)) return <div className="workspace-empty">{uiText("This result is no longer available. Open its task to review the conversation.", "此結果已無法使用。請開啟所屬工作查看對話。")}</div>;
  const p = payload, r = p.result;
  const url = providerUrl(p);
  const provider = p.target.view === 'email' ? 'Gmail' : p.target.view === 'notion' ? 'Notion' : p.target.view === 'calendar' ? 'Google Calendar' : 'GitHub';
  const rows = (Array.isArray(r.events) ? r.events : Array.isArray(r.items) ? r.items : []).filter((v): v is Record<string, unknown> => !!v && typeof v === 'object');
  const row = rows[selected] || rows[0];
  const text = (value: unknown, fallback = '') => typeof value === 'string' ? value : fallback;
  const date = (value: unknown) => { const s = text(value); const d = new Date(s); return isNaN(d.getTime()) ? s : d.toLocaleString(); };
  function back() { dispatch({ type: 'open', view: 'chat', id: `task:${p.session_id}`, sessionId: p.session_id, title: 'Assistant' }); }
  return <div className="result-workspace">
    <header className="workspace-heading"><div><button className="text-button" onClick={back}><ArrowLeft size={14} />{uiText('Back to task', '返回工作')}</button><h1>{resultTitle(p)}</h1><p>{provider} · {uiText("Retrieved for this task", "本次工作取得的內容")}</p></div>{url && <button className="workspace-button" onClick={() => void openUrl(url).catch(e => setError(String(e)))}>{uiText("Open", "開啟")} {provider}<ExternalLink size={15} /></button>}</header>
    {error && <p className="workspace-error" role="alert">{error}</p>}
    {p.target.view === 'email' && <div className="workspace-notice">{uiText("Uses the Google account connected to Assistant. Mail workspace accounts are separate. Check the account when opening Gmail.", "使用已連接至助理的 Google 帳戶。郵件工作區帳戶需另行管理；開啟 Gmail 時請核對帳戶。")} </div>}
    {!!r.state && <p className="result-confirmation">{r.state === 'saved_to_gmail' ? uiText('Draft saved to Gmail. It has not been sent.', '草稿已儲存至 Gmail，尚未寄出。') : r.state === 'created' ? (p.target.view === 'calendar' ? uiText('Personal event created.', '已建立個人活動。') : p.target.view === 'github' ? uiText('Issue created.', '已建立議題。') : uiText('Page created.', '已建立頁面。')) : uiText('Update saved.', '已儲存修改。')}{p.target.view === 'calendar' && r.state === 'created' ? uiText(' Add invitees in Google Calendar if needed.', ' 如有需要，可在 Google Calendar 加入受邀者。') : ''}</p>}
    {p.tool_name === 'draft_email' ? <div className="result-document"><dl><dt>{uiText('To', '收件人')}</dt><dd>{text(r.to)}</dd><dt>{uiText('Subject', '主旨')}</dt><dd>{text(r.subject)}</dd></dl><label htmlFor="reply-draft">{uiText('Working copy', '工作副本')}</label><textarea id="reply-draft" value={draft} onChange={e => { setDraft(e.target.value); setCopied(false); }} /><div className="result-document-footer"><small>{uiText("Edits are saved locally in this workspace. Copy them into Gmail to update the saved draft.", "修改會儲存於此本機工作區。如要更新 Gmail 草稿，請將內容複製至 Gmail。")} </small><button className="workspace-button" onClick={() => void navigator.clipboard.writeText(draft).then(() => setCopied(true)).catch(e => setError(String(e)))}><Copy size={14} />{copied ? uiText('Copied', '已複製') : uiText('Copy text', '複製文字')}</button></div></div>
      : typeof r.content === 'string' ? <article className="result-document"><ReadableContent text={r.content} format={p.tool_name === 'read_email' ? 'plain' : 'markdown'} /></article>
      : (Array.isArray(r.events) || Array.isArray(r.items)) ? <div className="result-browser"><div className="result-list">{!rows.length && <p className="workspace-empty">{uiText('No results for this request.', '此要求沒有結果。')}</p>}{rows.map((item, i) => <button key={i} className={selected === i ? 'is-selected' : ''} onClick={() => setSelected(i)}><strong>{text(item.summary || item.title, uiText('Untitled', '未命名'))}</strong><small>{text(item.repo) || date(item.start_time)}</small><span>{text(item.state || item.status)}</span></button>)}</div><article className="result-detail">{row ? <><FileText size={22} /><h2>{text(row.summary || row.title)}</h2><dl>{!!row.start_time && <><dt>{uiText('Starts', '開始')}</dt><dd>{date(row.start_time)}</dd><dt>{uiText('Ends', '結束')}</dt><dd>{date(row.end_time)}</dd></>}{!!row.repo && <><dt>{uiText('Repository', '儲存庫')}</dt><dd>{text(row.repo)}</dd></>}{!!row.location && <><dt>{uiText('Location', '地點')}</dt><dd>{text(row.location)}</dd></>}</dl>{typeof row.description === 'string' && <ReadableContent text={row.description} format={p.target.view === 'calendar' ? 'plain' : 'markdown'} />}{!row.description && <p>{uiText("Open the workspace to sync and inspect the latest details.", "開啟工作區以同步並查看最新詳情。")} </p>}<button className="workspace-button" onClick={() => open(p.target.view)}>{uiText("Open", "開啟")} {provider} {uiText("workspace", "工作區")}</button></> : <p>{uiText('Select a result to see its details.', '選取結果查看詳情。')}</p>}</article></div>
      : <div className="result-document"><h2>{text(r.title, resultTitle(p))}</h2>{!!r.start && <p>{date(r.start)} – {date(r.end)}</p>}<button className="workspace-button" onClick={() => open(p.target.view)}>{uiText('Open workspace', '開啟工作區')}</button></div>}
  </div>;
}
