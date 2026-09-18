export type UiLocale = 'en' | 'zh-HK';

const messages = {
  navAutomation: ['Automation', '工作總覽'], navMail: ['Mail', '電郵工作區'], navCalendar: ['Calendar', '日曆'], navNotes: ['Notes', '筆記'], navGitHub: ['GitHub', 'GitHub'], navAutomations: ['Automations', '自動排程'], navConnections: ['Connections', '連接服務'], navSettings: ['Settings', '設定'], navAssistant: ['Assistant', 'AI 助理'], navResult: ['Result', '結果'],
  appNavigation: ['App navigation', 'JARVIS 主導覽'], hideSidebar: ['Hide sidebar', '收起側邊欄'], showSidebar: ['Show sidebar', '顯示側邊欄'], newTask: ['New task', '新增工作'], searchCommands: ['Search and commands', '搜尋與指令'], recentTasks: ['Recent tasks', '最近工作'], tasksAppear: ['Tasks you start will appear here.', '已開始的工作會顯示於此。'], newResult: ['New result', '有新結果'], openWork: ['Open work', '已開啟的工作區'], pinned: ['pinned', '已固定'], pinHint: ['double-click to pin', '按兩下固定分頁'], closeTab: ['Close {title}', '關閉「{title}」分頁'], newTaskTab: ['New task tab', '新增工作分頁'],
  uiLanguage: ['Interface language', '介面語言'], uiLanguageHelp: ['Changes JARVIS menus and controls only. Assistant replies and speech recognition have separate settings.', '僅變更 JARVIS 選單及操作介面。助理回覆與語音辨識語言需另行設定。'], english: ['English', '英文'], cantonese: ['繁體中文（香港）', '繁體中文（香港）'],
  apiKeys: ['API keys', 'API 密鑰'], keyAvailable: ['Key saved; connection not verified', '已儲存密鑰，尚未驗證連接'], keyHelp: ['Keys are stored in macOS Keychain. Saving a key does not verify provider access or available credit.', '密鑰儲存於 macOS 鑰匙圈。儲存密鑰不代表已驗證供應商連接或可用額度。'],
  save: ['Save', '儲存'], saved: ['Saved', '已儲存'], clear: ['Remove', '移除'], loading: ['Loading…', '載入中…'], retry: ['Retry', '重試'], refresh: ['Refresh', '重新整理'], refreshing: ['Refreshing…', '重新整理中…'], syncing: ['Syncing…', '同步中…'], noRun: ['Never', '從未執行'], localTime: ['Local time', '本機時間'], statusActive: ['Active', '運行中'], statusPaused: ['Paused', '已暫停'], statusFailed: ['Failed', '失敗'], statusRunning: ['Running', '執行中'], statusEnabled: ['Enabled', '已啟用'],
  countEvents: ['{count} events', '{count} 個活動'], countSynced: ['{count} synced', '已同步 {count} 項'], countTotal: ['{count} total', '共 {count} 項'],
  accountMenu: ['Account menu', '帳戶選單'], displayName: ['Display name', '顯示名稱'], displayNameHelp: ['Shown at the bottom of the sidebar. Stored on this Mac only.', '顯示於側邊欄底部。只儲存在此 Mac。'], displayNamePlaceholder: ['Your name', '你的名稱'],
  accountSection: ['Account', '帳戶'], accountSignedOut: ['Sign in to use JARVIS Pro. Everything else works without an account.', '登入後即可使用 JARVIS Pro。其他功能無須帳戶亦可使用。'], accountUnavailable: ['Accounts are not available in this build.', '此版本未提供帳戶功能。'],
  signIn: ['Sign in', '登入'], signOut: ['Sign out', '登出'], signInGoogle: ['Continue with Google', '使用 Google 繼續'], signInEmail: ['Sign in with email', '使用電郵登入'], signingIn: ['Finish signing in in your browser, then return to JARVIS.', '請在瀏覽器完成登入，再返回 JARVIS。'], signedInAs: ['Signed in as {email}', '已登入：{email}'], sessionEnded: ['Your session has ended. Please sign in again.', '登入已失效，請重新登入。'],
  planLabel: ['Plan: {plan}', '方案：{plan}'], tierFree: ['Free', '免費版'], tierPro: ['Pro', 'Pro'], tierTeam: ['Team', 'Team'], tierUnknown: ['Unknown (JARVIS could not be reached)', '未知（無法連接 JARVIS）'],
  upgradePro: ['Upgrade to Pro', '升級至 Pro'], upgradeOpened: ['Checkout opened in your browser. Choose Refresh after paying.', '已在瀏覽器開啟付款頁面。付款後請按「重新整理」。'],
  updateAvailable: ['New version {version}', '有新版本 {version}'], downloadLatest: ['Download latest version', '下載最新版本'], updating: ['Updating {percent}%', '更新中 {percent}%'], updateFailed: ['Update failed', '更新失敗'], dismissUpdate: ['Dismiss until the next check', '暫時關閉，下次檢查再提示'],
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
