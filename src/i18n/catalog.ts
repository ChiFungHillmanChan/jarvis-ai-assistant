export type UiLocale = 'en' | 'zh-HK';

const messages = {
  navAutomation: ['Automation', '工作總覽'], navMail: ['Mail', '電郵工作區'], navCalendar: ['Calendar', '日曆'], navNotes: ['Notes', '筆記'], navGitHub: ['GitHub', 'GitHub'], navAutomations: ['Automations', '自動排程'], navConnections: ['Connections', '連接服務'], navSettings: ['Settings', '設定'], navAssistant: ['Assistant', 'AI 助理'], navResult: ['Result', '結果'],
  appNavigation: ['App navigation', 'JARVIS 主導覽'], hideSidebar: ['Hide sidebar', '收起側邊欄'], showSidebar: ['Show sidebar', '顯示側邊欄'], newTask: ['New task', '新增工作'], searchCommands: ['Search and commands', '搜尋同指令'], recentTasks: ['Recent tasks', '最近工作'], tasksAppear: ['Tasks you start will appear here.', '你開始嘅工作會顯示喺度。'], newResult: ['New result', '有新結果'], openWork: ['Open work', '已開啟嘅工作區'], pinned: ['pinned', '已固定'], pinHint: ['double-click to pin', '按兩下固定分頁'], closeTab: ['Close {title}', '關閉「{title}」分頁'], newTaskTab: ['New task tab', '新增工作分頁'],
  uiLanguage: ['Interface language', '介面語言'], uiLanguageHelp: ['Changes JARVIS menus and controls only. Assistant replies and speech recognition have separate settings.', '只改 JARVIS 選單同操作介面。助手回覆同語音辨識語言分開設定。'], english: ['English', '英文'], cantonese: ['繁體中文（香港）', '繁體中文（香港）'],
  apiKeys: ['API keys', 'API 密鑰'], keyAvailable: ['Key saved; connection not verified', '已儲存密鑰，尚未驗證連接'], keyHelp: ['Keys are stored in macOS Keychain. Saving a key does not verify provider access or available credit.', '密鑰儲存喺 macOS 鑰匙圈。儲存密鑰唔代表已驗證供應商連接或者可用額度。'],
  save: ['Save', '儲存'], saved: ['Saved', '已儲存'], clear: ['Remove', '移除'], loading: ['Loading…', '載入緊…'], retry: ['Retry', '重試'], refresh: ['Refresh', '重新整理'], refreshing: ['Refreshing…', '重新整理緊…'], syncing: ['Syncing…', '同步緊…'], noRun: ['Never', '從未執行'], localTime: ['Local time', '本機時間'], statusActive: ['Active', '運行中'], statusPaused: ['Paused', '已暫停'], statusFailed: ['Failed', '失敗'], statusRunning: ['Running', '執行緊'], statusEnabled: ['Enabled', '已啟用'],
  countEvents: ['{count} events', '{count} 個活動'], countSynced: ['{count} synced', '已同步 {count} 項'], countTotal: ['{count} total', '共 {count} 項'],
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
