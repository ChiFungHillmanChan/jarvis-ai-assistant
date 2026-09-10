import { useState } from 'react';
import { ArrowLeft, ExternalLink, Copy, FileText } from 'lucide-react';
import { useWorkspace, useWorkspaceValue } from './WorkspaceContext';
import { validResult, resultTitle, providerUrl, type ToolResult } from './results';
import { openUrl } from '../lib/commands';
import MessageRenderer from '../components/chat/MessageRenderer';

export default function ResultWorkspace() {
  const [payload] = useWorkspaceValue<ToolResult | null>('toolResult', null);
  const { dispatch, open } = useWorkspace();
  const [error, setError] = useState('');
  const [selected, setSelected] = useWorkspaceValue('selectedResult', 0);
  const [draft, setDraft] = useWorkspaceValue('localDraft', typeof payload?.result.body === 'string' ? payload.result.body : '');
  const [copied, setCopied] = useState(false);
  if (!validResult(payload)) return <div className="workspace-empty">This result is no longer available. Open its task to review the conversation.</div>;
  const p = payload, r = p.result;
  const url = providerUrl(p);
  const provider = p.target.view === 'email' ? 'Gmail' : p.target.view === 'notion' ? 'Notion' : p.target.view === 'calendar' ? 'Google Calendar' : 'GitHub';
  const rows = (Array.isArray(r.events) ? r.events : Array.isArray(r.items) ? r.items : []).filter((v): v is Record<string, unknown> => !!v && typeof v === 'object');
  const row = rows[selected] || rows[0];
  const text = (value: unknown, fallback = '') => typeof value === 'string' ? value : fallback;
  const date = (value: unknown) => { const s = text(value); const d = new Date(s); return isNaN(d.getTime()) ? s : d.toLocaleString(); };
  function back() { dispatch({ type: 'open', view: 'chat', id: `task:${p.session_id}`, sessionId: p.session_id, title: 'Assistant' }); }
  return <div className="result-workspace">
    <header className="workspace-heading"><div><button className="text-button" onClick={back}><ArrowLeft size={14} />Back to task</button><h1>{resultTitle(p)}</h1><p>{provider} · Retrieved for this task</p></div>{url && <button className="workspace-button" onClick={() => void openUrl(url).catch(e => setError(String(e)))}>Open {provider}<ExternalLink size={15} /></button>}</header>
    {error && <p className="workspace-error" role="alert">{error}</p>}
    {p.target.view === 'email' && <div className="workspace-notice">Uses the Google account connected to Assistant. Mail workspace accounts are separate. Check the account when opening Gmail.</div>}
    {!!r.state && <p className="result-confirmation">{r.state === 'saved_to_gmail' ? 'Draft saved to Gmail. It has not been sent.' : r.state === 'created' ? `${p.target.view === 'calendar' ? 'Personal event' : p.target.view === 'github' ? 'Issue' : 'Page'} created.` : 'Update saved.'}{p.target.view === 'calendar' && r.state === 'created' ? ' Add invitees in Google Calendar if needed.' : ''}</p>}
    {p.tool_name === 'draft_email' ? <div className="result-document"><dl><dt>To</dt><dd>{text(r.to)}</dd><dt>Subject</dt><dd>{text(r.subject)}</dd></dl><label htmlFor="reply-draft">Working copy</label><textarea id="reply-draft" value={draft} onChange={e => { setDraft(e.target.value); setCopied(false); }} /><div className="result-document-footer"><small>Edits are saved locally in this workspace. Copy them into Gmail to update the saved draft.</small><button className="workspace-button" onClick={() => void navigator.clipboard.writeText(draft).then(() => setCopied(true)).catch(e => setError(String(e)))}><Copy size={14} />{copied ? 'Copied' : 'Copy text'}</button></div></div>
      : typeof r.content === 'string' ? <article className="result-document"><MessageRenderer content={r.content} /></article>
      : (Array.isArray(r.events) || Array.isArray(r.items)) ? <div className="result-browser"><div className="result-list">{!rows.length && <p className="workspace-empty">No results for this request.</p>}{rows.map((item, i) => <button key={i} className={selected === i ? 'is-selected' : ''} onClick={() => setSelected(i)}><strong>{text(item.summary || item.title, 'Untitled')}</strong><small>{text(item.repo) || date(item.start_time)}</small><span>{text(item.state || item.status)}</span></button>)}</div><article className="result-detail">{row ? <><FileText size={22} /><h2>{text(row.summary || row.title)}</h2><dl>{!!row.start_time && <><dt>Starts</dt><dd>{date(row.start_time)}</dd><dt>Ends</dt><dd>{date(row.end_time)}</dd></>}{!!row.repo && <><dt>Repository</dt><dd>{text(row.repo)}</dd></>}{!!row.location && <><dt>Location</dt><dd>{text(row.location)}</dd></>}</dl>{typeof row.description === 'string' && <MessageRenderer content={row.description} />}{!row.description && <p>Open the workspace to sync and inspect the latest details.</p>}<button className="workspace-button" onClick={() => open(p.target.view)}>Open {provider} workspace</button></> : <p>Select a result to see its details.</p>}</article></div>
      : <div className="result-document"><h2>{text(r.title, resultTitle(p))}</h2>{!!r.start && <p>{date(r.start)} – {date(r.end)}</p>}<button className="workspace-button" onClick={() => open(p.target.view)}>Open workspace</button></div>}
  </div>;
}
