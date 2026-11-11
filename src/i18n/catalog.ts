export type UiLocale = 'en' | 'zh-HK';

const messages = {
  navAutomation: ['Automation', '工作總覽'], navMail: ['Mail', '電郵工作區'], navCalendar: ['Calendar', '日曆'], navNotes: ['Notes', '筆記'], navGitHub: ['GitHub', 'GitHub'], navAutomations: ['Automations', '自動排程'], navConnections: ['Connections', '連接服務'], navSettings: ['Settings', '設定'], navAssistant: ['Assistant', 'AI 助理'], navResult: ['Result', '結果'],
  appNavigation: ['App navigation', 'JARVIS 主導覽'], hideSidebar: ['Hide sidebar', '收起側邊欄'], showSidebar: ['Show sidebar', '顯示側邊欄'], newTask: ['New task', '新增工作'], searchCommands: ['Search and commands', '搜尋與指令'], recentTasks: ['Recent tasks', '最近工作'], tasksAppear: ['Tasks you start will appear here.', '已開始的工作會顯示於此。'], newResult: ['New result', '有新結果'], openWork: ['Open work', '已開啟的工作區'], pinned: ['pinned', '已固定'], pinHint: ['double-click to pin', '按兩下固定分頁'], closeTab: ['Close {title}', '關閉「{title}」分頁'], newTaskTab: ['New task tab', '新增工作分頁'],
  deleteConversation: ['Delete conversation', '刪除對話'], deleteConversationConfirm: ['Delete this conversation?', '刪除此對話？'], deleteConversationHint: ['Its messages are removed from this Mac. Drafts or events it created are kept.', '對話內容會從此 Mac 刪除；已建立的草稿或活動會保留。'], deleteConfirm: ['Delete', '刪除'], deleteCancel: ['Cancel', '取消'], conversationDeleted: ['Conversation deleted.', '已刪除對話。'], deleteWhileBusy: ['Wait for the assistant to finish before deleting.', '請等待助理完成目前工作後再刪除。'],
  uiLanguage: ['Interface language', '介面語言'], uiLanguageHelp: ['Changes JARVIS menus and controls only. Assistant replies and speech recognition have separate settings.', '僅變更 JARVIS 選單及操作介面。助理回覆與語音辨識語言需另行設定。'], english: ['English', '英文'], cantonese: ['繁體中文（香港）', '繁體中文（香港）'],
  apiKeys: ['API keys', 'API 密鑰'], keyAvailable: ['Key saved; connection not verified', '已儲存密鑰，尚未驗證連接'], keyHelp: ['Keys are stored in macOS Keychain. Saving a key does not verify provider access or available credit.', '密鑰儲存於 macOS 鑰匙圈。儲存密鑰不代表已驗證供應商連接或可用額度。'],
  save: ['Save', '儲存'], saved: ['Saved', '已儲存'], clear: ['Remove', '移除'], loading: ['Loading…', '載入中…'], retry: ['Retry', '重試'], refresh: ['Refresh', '重新整理'], refreshing: ['Refreshing…', '重新整理中…'], syncing: ['Syncing…', '同步中…'], noRun: ['Never', '從未執行'], localTime: ['Local time', '本機時間'], statusActive: ['Active', '運行中'], statusPaused: ['Paused', '已暫停'], statusFailed: ['Failed', '失敗'], statusRunning: ['Running', '執行中'], statusEnabled: ['Enabled', '已啟用'],
  countEvents: ['{count} events', '{count} 個活動'], countSynced: ['{count} synced', '已同步 {count} 項'], countTotal: ['{count} total', '共 {count} 項'],
  accountMenu: ['Account menu', '帳戶選單'], displayName: ['Display name', '顯示名稱'], displayNameHelp: ['Shown at the bottom of the sidebar. Stored on this Mac only.', '顯示於側邊欄底部。只儲存在此 Mac。'], displayNamePlaceholder: ['Your name', '你的名稱'],
  accountSection: ['Account', '帳戶'], accountSignedOut: ['Sign in to view your plan and manage billing.', '登入後可查看方案及管理訂閱。'], accountUnavailable: ['Accounts are not available in this build.', '此版本未提供帳戶功能。'],
  signIn: ['Sign in', '登入'], signOut: ['Sign out', '登出'], signInGoogle: ['Continue with Google', '使用 Google 繼續'], signInEmail: ['Sign in with email', '使用電郵登入'], signingIn: ['Finish signing in in your browser, then return to JARVIS.', '請在瀏覽器完成登入，再返回 JARVIS。'], signedInAs: ['Signed in as {email}', '已登入：{email}'], sessionEnded: ['Your session has ended. Please sign in again.', '登入已失效，請重新登入。'],
  planLabel: ['Plan: {plan}', '方案：{plan}'], tierFree: ['Free', '免費版'], tierPro: ['Pro', 'Pro'], tierTeam: ['Team', 'Team'], tierUnknown: ['Unknown (JARVIS could not be reached)', '未知（無法連接 JARVIS）'],
  upgradeOpened: ['Checkout opened in your browser. JARVIS will check your plan after payment.', '已在瀏覽器開啟付款頁面。JARVIS 會在付款後檢查方案。'],
  manageBilling: ['Manage billing', '管理訂閱'], billingOpened: ['Billing opened in your browser. Return to JARVIS after making changes.', '已在瀏覽器開啟訂閱管理頁面。完成變更後請返回 JARVIS。'],
  billingByok: ['JARVIS uses your own AI provider keys (BYOK). A subscription does not include AI credits; your provider bills AI usage separately.', 'JARVIS 採用自備 AI 密鑰（BYOK）。訂閱不包含 AI 額度；AI 用量由你選用的供應商另行收費。'],
  billingBrowserFailed: ['The browser could not be opened. Use the button below to try again.', '未能開啟瀏覽器，請按下方按鈕重試。'], billingBusy: ['Your billing request is still being processed. Please try again shortly.', '訂閱要求仍在處理中，請稍後重試。'],
  paymentChecking: ['Checking your plan for up to two minutes…', '正在檢查方案，最多等候兩分鐘…'], paymentConfirmed: ['Your paid plan is confirmed and up to date.', '已確認付費方案，帳戶資料已更新。'], paymentUnconfirmed: ['Payment has not been confirmed. If you completed payment, choose Refresh shortly. You do not need to pay again.', '尚未確認付款。如已完成付款，請稍後按「重新整理」，無須再次付款。'],
  reopenCheckout: ['Reopen checkout', '重新開啟付款頁面'], reopenBilling: ['Reopen billing', '重新開啟訂閱管理'],
  gateSignInTitle: ['Sign in to start using JARVIS', '登入以開始使用 JARVIS'], gateSignInBody: ['Your account holds your plan. Your mail, notes and settings stay on this Mac.', '帳戶只用作管理你的方案。你的郵件、筆記及設定仍然只儲存在此 Mac。'],
  trialTitle: ['Start your {days}-day free trial', '開始 {days} 日免費試用'], trialTerms: ['Free for {days} days, then {price}. Cancel any time before day {days} and you pay nothing. Stripe asks for a card now so your plan continues without interruption.', '首 {days} 日免費，之後{price}。在第 {days} 日前隨時取消，不會收取任何費用。Stripe 會先登記信用卡，讓方案在試用期後繼續使用而不中斷。'], trialStart: ['Start free trial', '開始免費試用'],
  subscribeTitle: ['Subscribe to JARVIS Pro', '訂閱 JARVIS Pro'], subscribeTerms: ['JARVIS Pro is {price}. Cancel any time from Manage billing.', '收費為{price}，可隨時在「管理訂閱」取消。'], subscribe: ['Subscribe', '訂閱'],
  codeHave: ['I have a code', '我有優惠碼'], codeLabel: ['Code', '優惠碼'], codeApply: ['Apply code', '使用優惠碼'], codeInvalid: ['Code not recognised. Check the spelling, or ask the person who gave it to you.', '無法識別此優惠碼。請檢查拼寫，或向提供優惠碼的人查詢。'],
  gateOfflineTitle: ['Cannot confirm your plan', '未能確認你的方案'], gateOfflineBody: ['JARVIS could not reach its server and has no recent confirmation of your plan. Check your connection and try again.', 'JARVIS 未能連接伺服器，亦沒有近期的方案確認紀錄。請檢查網絡連線後再試。'],
  gateWaitingTitle: ['Finish checkout in your browser', '請在瀏覽器完成付款程序'], gateBack: ['Back', '返回'],
  planTrialEnds: ['Trial ends {date}', '試用期於 {date} 結束'], planRenews: ['Renews {date}', '將於 {date} 續期'], planEnds: ['Ends {date}', '將於 {date} 結束'],
  updateReady: ['JARVIS {version} is ready', 'JARVIS {version} 已準備好'], restartToUpdate: ['Restart to update', '重新啟動以更新'], restartingUpdate: ['Restarting…', '正在重新啟動…'], updateFailed: ['Update failed', '更新失敗'], updateReopen: ['Quit and reopen JARVIS to finish updating', '請結束並重新開啟 JARVIS 以完成更新'], dismissUpdate: ['Hide until JARVIS next opens', '暫時隱藏，下次開啟 JARVIS 時再提示'],
} as const;
export type MessageKey = keyof typeof messages;
export const catalog = {
  en: Object.fromEntries(Object.entries(messages).map(([key, pair]) => [key, pair[0]])) as { [K in MessageKey]: (typeof messages)[K][0] },
  'zh-HK': Object.fromEntries(Object.entries(messages).map(([key, pair]) => [key, pair[1]])) as { [K in MessageKey]: (typeof messages)[K][1] },
};
export function interpolate(template: string, params: Record<string, string | number> = {}): string {
  return template.replace(/\{(\w+)\}/g, (match, key: string) => Object.prototype.hasOwnProperty.call(params, key) ? String(params[key]) : match);
}
export function suggestedUiLocale(languages: readonly string[]): UiLocale {
  const first = languages[0]?.toLowerCase() ?? '';
  return first === 'zh-hk' || first === 'yue-hk' || first === 'zh-hant-hk' ? 'zh-HK' : 'en';
}
export function normalizeUiLocale(value: unknown, fallback: UiLocale = 'en'): UiLocale {
  return value === 'en' || value === 'zh-HK' ? value : fallback;
}
export function translate(locale: UiLocale, key: MessageKey, params?: Record<string, string | number>): string {
  return interpolate(catalog[locale][key], params);
}
