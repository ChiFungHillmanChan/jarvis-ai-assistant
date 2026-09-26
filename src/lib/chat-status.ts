import type { Text } from './mail-presentation';

/** The backend reports progress in English (ai/*.rs, tools::tool_status_label). */
const ZH: Record<string, string> = {
  'Understanding your request...': '正在理解你的要求…',
  'Transcribing your request...': '正在轉換語音…',
  'Reviewing earlier conversations...': '正在查閱之前的對話…',
  'Reviewing tool results...': '正在核對工具結果…',
  'Composing response...': '正在撰寫回覆…',
  'Retrying with next provider...': '正在改用下一個 AI…',
  'Searching emails...': '正在搜尋郵件…',
  'Reading email...': '正在閱讀郵件…',
  'Drafting email...': '正在起草郵件…',
  'Archiving email...': '正在封存郵件…',
  'Checking calendar...': '正在查看日曆…',
  'Creating event...': '正在建立活動…',
  'Updating event...': '正在更新活動…',
  'Deleting event...': '正在刪除活動…',
  'Searching Notion...': '正在搜尋 Notion…',
  'Reading Notion page...': '正在閱讀 Notion 頁面…',
  'Creating Notion page...': '正在建立 Notion 頁面…',
  'Checking GitHub...': '正在查看 GitHub…',
  'Creating GitHub issue...': '正在建立 GitHub issue…',
  'Searching notes...': '正在搜尋筆記…',
  'Reading note...': '正在閱讀筆記…',
  'Opening app...': '正在開啟 App…',
  'Running command...': '正在執行指令…',
  'Finding files...': '正在尋找檔案…',
  'Opening file...': '正在開啟檔案…',
  'Creating task...': '正在建立待辦…',
  'Checking tasks...': '正在查看待辦…',
  'Checking follow-ups...': '正在查看跟進事項…',
  'Updating follow-up...': '正在更新跟進事項…',
  'Checking your inbox...': '正在查看收件匣…',
  'Checking system...': '正在查看系統…',
  'Sending notification...': '正在發出通知…',
  'Rendering chart...': '正在繪製圖表…',
  'Showing result...': '正在顯示結果…',
  'Saving to memory...': '正在記住…',
  'Removing from memory...': '正在刪除記憶…',
  'Checking memory...': '正在查看記憶…',
  'Processing...': '處理中…',
};

/** A progress line as a plain action ("Searching emails: Gold Badge"), never a log prefix. */
export function chatStatusText(status: string, text: Text, detail?: string | null): string {
  const action = /^(?:Planning|Running): (.+)$/.exec(status)?.[1] ?? status;
  const english = action.replace(/\.\.\.$/, '…');
  const label = text(english, ZH[action] ?? english);
  return detail ? `${label.replace(/…$/, '')}${text(': ', '：')}${detail}` : label;
}
