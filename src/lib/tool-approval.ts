// The sentence on the approval card (src/components/ToolApproval.tsx). It
// lives here so the wording for each action can be tested without a window.

export interface ApprovalRequest { approval_id: string; tool_name: string; detail: string; status: string | null }

type Text = (en: string, zh: string) => string;

export function describeApproval({ tool_name, detail, status }: ApprovalRequest, text: Text): string {
  switch (tool_name) {
    case 'remember': return text(`Save to memory: ${detail}`, `儲存到記憶：${detail}`);
    case 'archive_email': return text(`Archive the email ${detail}`, `封存郵件 ${detail}`);
    case 'delete_event': return text(`Delete the calendar event ${detail}`, `刪除日曆活動 ${detail}`);
    case 'update_event': return text(`Change the calendar event ${detail}`, `更改日曆活動 ${detail}`);
    case 'forget': return text(`Delete from memory: ${detail}`, `從記憶中刪除：${detail}`);
    case 'create_notion_page': return text(`Create the Notion page ${detail}`, `建立 Notion 頁面 ${detail}`);
    case 'create_github_issue': return text(`Open the GitHub issue ${detail}`, `建立 GitHub issue ${detail}`);
    case 'open_file': return text(`Open the file ${detail}`, `開啟檔案 ${detail}`);
    case 'open_app': return text(`Open the app ${detail}`, `開啟應用程式 ${detail}`);
    case 'update_follow_up':
      if (status === 'done') return text(`Mark the follow-up ${detail} as done`, `將跟進事項 ${detail} 標為完成`);
      if (status === 'dismissed') return text(`Dismiss the follow-up ${detail}`, `略過跟進事項 ${detail}`);
      return text(`Reopen the follow-up ${detail}`, `重新開啟跟進事項 ${detail}`);
    default: return `${tool_name}: ${detail}`;
  }
}
