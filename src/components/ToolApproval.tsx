import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useUiLocale } from '../i18n';
import { showToast } from './Toast';

interface ApprovalRequest { approval_id: string; tool_name: string; detail: string; status: string | null }

type Text = (en: string, zh: string) => string;

function describe({ tool_name, detail, status }: ApprovalRequest, text: Text): string {
  switch (tool_name) {
    case 'remember': return text(`Save to memory: ${detail}`, `儲存到記憶：${detail}`);
    case 'archive_email': return text(`Archive the email ${detail}`, `封存郵件 ${detail}`);
    case 'delete_event': return text(`Delete the calendar event ${detail}`, `刪除日曆活動 ${detail}`);
    case 'update_follow_up':
      if (status === 'done') return text(`Mark the follow-up ${detail} as done`, `將跟進事項 ${detail} 標為完成`);
      if (status === 'dismissed') return text(`Dismiss the follow-up ${detail}`, `略過跟進事項 ${detail}`);
      return text(`Reopen the follow-up ${detail}`, `重新開啟跟進事項 ${detail}`);
    default: return `${tool_name}: ${detail}`;
  }
}

/**
 * The user's click before the assistant changes something (see
 * src-tauri/src/tool_approval.rs). An email can ask the model to archive mail,
 * delete events or save a memory, so each such action waits here. Not
 * answering within two minutes means no.
 */
export default function ToolApproval() {
  const { text } = useUiLocale();
  const [requests, setRequests] = useState<ApprovalRequest[]>([]);

  useEffect(() => {
    const opened = listen<ApprovalRequest>('tool-approval-request', ({ payload }) => setRequests(list => [...list, payload]));
    const closed = listen<{ approval_id: string }>('tool-approval-closed', ({ payload }) => setRequests(list => list.filter(r => r.approval_id !== payload.approval_id)));
    return () => { void opened.then(stop => stop()); void closed.then(stop => stop()); };
  }, []);

  const current = requests[0];
  if (!current) return null;

  async function answer(approved: boolean) {
    setRequests(list => list.filter(r => r.approval_id !== current.approval_id));
    try { await invoke('resolve_tool_approval', { approvalId: current.approval_id, approved }); }
    catch { showToast(text('This request expired. Ask JARVIS again.', '這項請求已逾時，請再向 JARVIS 提出。'), 'error'); }
  }

  return <div className="update-toast tool-approval" role="alertdialog" aria-labelledby="tool-approval-title" aria-describedby="tool-approval-detail">
    <span className="update-toast-dot" aria-hidden="true" />
    <div className="update-toast-body">
      <strong id="tool-approval-title">{text('JARVIS asks before making this change', 'JARVIS 需要你批准這項變更')}</strong>
      <span id="tool-approval-detail" className="tool-approval-detail">{describe(current, text)}</span>
      <div className="tool-approval-actions">
        <button type="button" className="update-toast-action" onClick={() => void answer(true)}>{text('Allow', '批准')}</button>
        <button type="button" className="tool-approval-decline" onClick={() => void answer(false)}>{text("Don't allow", '不批准')}</button>
      </div>
      {requests.length > 1 && <small className="tool-approval-more">{text(`${requests.length - 1} more waiting`, `另有 ${requests.length - 1} 項等待批准`)}</small>}
    </div>
  </div>;
}
