# JARVIS 香港繁體中文逐頁文案手冊

日期：2026-09-12。用途：供設計、工程及香港試用者逐項覆核；呢份係文案及行為規格，未有改動 App。本次只讀程式、冇讀真實郵件或憑證。`zh-HK` 係介面地區格式；廣東話回覆仍用既有 `yue` preference。兩者唔可以混為一個設定。

## 1. 用法與狀態標記

- **現有替換**：現有 UI／行為可以用呢段文案取代。唔表示已套用；工程仍要抽出字串、接翻譯 catalog、測試畫面。
- **待實作**：依賴新增 UI、原生 command、能力檢查或新資料欄位。完成行為同驗收前不可展示為已可用。
- **條件顯示**：只有已取得相應證據先顯示，例如連線測試成功、模型精確存在、雲端授權已完成。讀取失敗必須顯示未知，唔當成未連接或成功。
- **歷史／非主入口**：程式仍存在但目前 AppShell 未接入；先避免喺首批商業使用者流程重新露出內部工具。

第 3 節按頁提供產品文案；每頁狀態同待實作例外以第 2 節為準。第 4 至 8 節提供現有細項、錯誤碼、佔位變量同驗收要求，工程可以逐項轉成 message key。建議 catalog key 以 `common.* / onboarding.* / settings.* / mail.* / calendar.* / automation.* / voice.* / errors.*` 分組；key 唔用翻譯後文字。

### 香港公司語氣

按鈕簡短清楚，用「儲存」「新增」「連接」「重試」。說明可以自然用「你」「喺」「嘅」「撳」，唔用「您」「的」「在」模仿普通話口語，亦唔把每句強行寫成中英夾雜。必要技術名稱保留英文；advanced settings 可顯示 API key、模型 ID、endpoint。

內部整理預設自然廣東話：「客戶要你星期五前確認交貨日，運費仲未講明。」對外文件按用戶選擇正式繁體中文：「請於星期五前確認交貨日期，並補充運費安排。」原本英文客戶信可以保留英文草稿。**介面語言、分析語氣、外發文稿語言、聽寫語言及朗讀聲線各自設定，唔自動互相覆蓋。**「正式繁體中文」同外發語言選項屬待實作。

原信、合約、客戶公司名、人名、型號、金額、單位、法律條款、引用文字同來源 quote 保留原文；翻譯另標「參考翻譯」，唔覆寫 source。`[郵件 #123]` 引用 ID 唔翻譯／重新編號。禁止憑空補上缺失嘅期限、幣種或承諾；無證據用「原信未有講明」「待確認」。

### 香港日期與時區

顯示建議：「2026年9月12日（星期六）14:30 香港時間」。儲存 instant 時保留 offset／UTC；日期-only（例如有效至 2026-09-12）保持日期語義。跨境例子：「2026年9月12日 14:30 香港時間／07:30 倫敦時間」；冬令時間 offset 要按事件日期計算，唔固定相差七個鐘。

目前大部分功能跟 macOS 本地時區；香港語言唔代表將所有時間改成香港時區。**工作時區選擇屬待實作**；完成前文案用「以下時間以 {timezoneLabel} 顯示」，必須從實際 formatter 讀取值。金額用 `HK$1,280`、`US$120`；只得 `$120` 而無來源幣種，顯示「$120（幣種待確認）」。唔擅自換算；如將來提供估算要顯示匯率時間。

## 2. 頁面覆蓋與可套用狀態

| 頁面 | 程式來源 | 狀態／界線 |
|---|---|---|
| 全局導航、分頁、搜尋 | [Sidebar.tsx:4](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/Sidebar.tsx:4) | 現有替換；自動標題翻譯需處理已保存 tab title，保留用戶自訂名 |
| App 狀態、快捷搜尋 | [App.tsx:42](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/App.tsx:42) | 現有替換 |
| 分頁資料模型 | [model.ts:3](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/model.ts:3) | 現有替換；固定英文 title 切語言後唔會自己更新 |
| 初次設定 | [OnboardingWizard.tsx:99](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/OnboardingWizard.tsx:99) | 現有三步可以翻譯；第 3.2／3.3 節新 Mail 主流程屬待實作 |
| 本機 AI | [LocalAiSetup.tsx:84](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/LocalAiSetup.tsx:84) | PULL／取消／檢查已有；一鍵安裝啟動、resume、最小推論、自動選 Mail 模型待實作 |
| Settings 模型／本機 endpoints | [Settings.tsx:580](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:580) | 現有替換；cloud API key 測試待實作，endpoint health test 已有 |
| Settings API keys | [Settings.tsx:703](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:703) | Save／Remove 已有；官方申請連結、儲存並測試、可用額度驗證待實作 |
| Settings Google | [Settings.tsx:745](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:745) | 現有替換；不可把 Assistant Google 當 Mail 帳戶 |
| Settings Notion／GitHub／Obsidian | [Settings.tsx:790](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:790) | 現有替換；新增服務連線測試待實作 |
| Settings AWS | [Settings.tsx:840](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:840) | 位於 Models 頁內，唔係獨立 sidebar 分頁；地區／可用模型實測待完成 |
| Settings 語言／語音 | [Settings.tsx:889](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:889) | 現有回覆語言／STT／聲線／wake word可翻；UI locale、試咪／試聽、聽寫覆核待實作 |
| Settings 助手／外觀 | [Settings.tsx:985](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:985) | 現有替換；資料及私隱分頁未有 |
| Mail 信箱／同步 | [EmailPage.tsx:95](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/EmailPage.tsx:95) | 現有替換／改善；示範引導同群組模板待實作 |
| Mail 群組／記憶／分析設定 | [EmailPage.tsx:121](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/EmailPage.tsx:121) | 現有替換；已確認背景係顯示名稱建議，唔改記憶權限 |
| 工作總覽 | [AutomationOverview.tsx:49](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/AutomationOverview.tsx:49) | 現有替換；跨全部信箱今日待辦整合未有，唔用假數據 |
| Assistant | [AssistantPanel.tsx:25](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/AssistantPanel.tsx:25) | 現有替換 |
| 工具結果及回覆草稿 | [ResultWorkspace.tsx:25](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/ResultWorkspace.tsx:25) | 現有替換；Mail 群組分析唔等於 Assistant draft_email |
| 日曆 | [CalendarPage.tsx:63](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/CalendarPage.tsx:63) | 現有替換；獨立工作時區待實作 |
| GitHub | [GitHubPage.tsx:17](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/GitHubPage.tsx:17) | 現有替換 |
| Notion | [NotionPage.tsx:15](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/NotionPage.tsx:15) | 現有替換；正文同步唔係現有能力 |
| 自動工作 | [CronDashboard.tsx:69](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/CronDashboard.tsx:69) | 現有替換；自然語言排程要與後端實際預覽一致 |
| 帳戶連接 | [ConnectionsPage.tsx:55](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/ConnectionsPage.tsx:55) | 現有替換 |
| 模型下載 toast | [ModelPullToast.tsx:9](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/ModelPullToast.tsx:9) | 現有替換；收到 complete event未必足以驗證模型可用 |
| 更新 | [UpdateButton.tsx:66](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/UpdateButton.tsx:66) | 現有自動檢查、下載安裝及重開；獨立手動檢查頁待實作 |
| 錯誤邊界 | [ErrorBoundary.tsx:33](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/ErrorBoundary.tsx:33) | 現有替換；結構化診斷匯出待實作 |
| 舊 Dashboard | [Dashboard.tsx:11](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Dashboard.tsx:11) | 歷史／非主入口；目前 AppShell home 用 AutomationOverview |
| Launch／Beta 內部控制頁 | [LaunchPage.tsx:87](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/LaunchPage.tsx:87) | 歷史／非主入口；目前 AppShell 未列 Launch 路由，唔當客戶可用付費頁 |
| Google callback | [google.rs:213](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/auth/google.rs:213) | 原生 HTML 替換；OAuth code 到達唔等於 token exchange同帳戶建立完成 |

### 第 3 節待實作例外清單

- 3.2、3.3 全節：新首次設定、示範資料、代管雲端、用途選擇、工作時區。
- 3.4：「開啟官方申請頁」「儲存並測試」及逐項 key test 狀態；現在只可用「儲存」「已儲存／有密鑰，未驗證」。
- 3.5：合併到 onboarding 同 first-sync 引導待實作；連 Gmail、sync、本機副本能力已存在。
- 3.6：一鍵準備、啟動引擎、暫停／resume、磁碟檢查、合成分析測試及自動套用選擇待實作；現有只有手動檢查、複製指令、PULL／取消、+CHAIN。
- 3.7：跨信箱「今日要跟進」待實作；工作總覽嘅自動工作結果已有。
- 3.8：群組模板、結構化「限期／負責人／金額」固定欄位待實作；現有分析係帶引用文字。
- 3.9：「已確認背景」係替換顯示名稱提案；backend 仍保持 `mail_memories`。
- 3.13：其他服務嘅新連線測試按鈕待實作。
- 3.14：介面／外發文稿語言、工作時區、資料及私隱頁待實作；現有 Language 只控制 response／STT。
- 3.15：試咪、試聽、語音資料流模式、聽寫覆核、系統設定深連結待實作；不要以文案掩蓋實際 Gemini 優先路由。
- 3.16：「檢查更新」獨立手動入口及「複製診斷資料」待實作；現有 update 按鈕一撳會下載、安裝及重新啟動。

## 3. 逐頁主要文案


以下係可落地文案草案。「未實作」內容只可喺完成相應功能後展示。文字用自然香港繁體中文，說明可用「你／喺／嘅／撳」，按鈕用短而清楚動詞；保留 API key、Gmail、OpenAI 等必要名稱。正式發客戶電郵另有英文／書面中文選項。

### 3.1 全局導航

| 原名／場景 | 建議 |
|---|---|
| Automation（home） | 工作總覽 |
| Mail | 電郵工作區 |
| Calendar | 日曆 |
| Notes | 筆記來源 |
| GitHub | GitHub |
| Automations（cron） | 自動工作 |
| Connections | 帳戶連接 |
| Settings | 設定 |
| New task | 新增工作 |
| Recent tasks | 最近工作 |
| Search and commands | 搜尋及指令 |
| Ready | 可以開始 |
| Assistant working | 助手處理緊，你可以繼續睇其他頁面 |
| Opening workspace… | 開緊工作區… |
| Restoring your workspace… | 還原緊上次工作區… |
| 新結果提示 | 有新結果 |
| 隱藏／顯示 sidebar | 收起側邊欄／顯示側邊欄 |
| Search placeholder | 搵工作區、工作或設定… |
| 無結果 | 暫時搵唔到，試吓另一個關鍵字。 |

### 3.2 初次設定：歡迎頁（新流程，未實作）

標題：「幫你整理信箱，知道下一步做乜」

說明：「先揀處理方式，再連接 Gmail。我哋會用示範資料測試 AI，之後先由你揀要分析嘅郵件。」

欄位：「介面語言」「工作時區」。建議預選語言由 OS／使用者確認；時區顯示具體值，可選「香港時間（Asia/Hong_Kong）」或跟系統。

按鈕：「開始設定」「先用示範資料」。第二個按鈕只可喺真實 demo dataset 同無真實外部動作路徑實作後展示。

### 3.3 初次設定：AI 方式（新流程，未實作）

- 卡「簡單雲端方案」：只喺已提供受地區支援、計費及限額管控嘅 JARVIS 代管服務後顯示「毋須自己申請 API key」。目前未具備，唔可先寫成可用。
- 卡「用自己嘅 AI 帳戶」：說明「你要提供 API key，AI 用量由供應商向你收費。JARVIS 會幫你測試連接。」按鈕「設定 API key」。
- 卡「用呢部 Mac 運行 AI」：說明「首次要下載模型；之後文字分析可喺本機進行。速度同可處理郵件量視乎呢部 Mac。」按鈕「檢查呢部 Mac」。
- 通用：「你可以之後再改。每次轉用雲端處理前，會列明傳送嘅資料。」

### 3.4 API key 頁

標題：「連接你嘅 AI 帳戶」

說明：「API key 係供應商用嚟核對帳戶同計用量嘅憑證。ChatGPT 等聊天訂閱同 API 帳單可能分開；請喺供應商帳戶確認。」個別供應商 billing 句需要官方來源支持。

欄位：「供應商」「API key」；按鈕「開啟官方申請頁」「儲存並測試」「移除已儲存密鑰」。申請頁由官方型錄固定，唔由模型回覆生成。

狀態分開：「未設定」「已儲存，未測試」「測試緊…」「已通過連接測試」「密鑰無效」「帳戶未有可用額度」「呢個模型暫時用唔到」「暫時連唔到供應商」。

儲存說明：「密鑰儲存喺 macOS Keychain，唔會再顯示喺呢個欄位。留空會保留原有設定。」

移除確認：「移除已儲存嘅 {provider} API key？之後相關 AI 分析可能無法使用。」唔喺 status 顯示 key 前綴尾碼，除非有具體需要。

### 3.5 Gmail 連接頁

標題：「連接你嘅 Gmail」

說明：「將你揀嘅信箱加入呢部 Mac。每個帳戶會分開授權，可以之後再加其他信箱。」

readonly 說明：「呢個電郵工作區只讀取郵件，唔會替你寄信、刪信或改 Gmail 標籤。」

資料說明：「郵件文字副本會儲存喺呢部 Mac。附件暫時未下載。分析時會按你揀嘅 AI 方式處理。」

按鈕：「連接 Gmail」「遲啲先連」。授權進行：「請喺瀏覽器完成 Google 授權，再返嚟 JARVIS。」

公司限制：「公司可能要 IT 管理員批准呢個應用程式。你可以保留目前設定，批准後再試。」唔應教一般客戶繞過警告。

過期：「今次登入等候時間已過。撳『重新連接』開始新一次授權。」

首批同步：「已連接。先同步最近最多 50 封郵件。」按鈕「同步第一批郵件」。成功：「已下載 {count} 封，可以開始整理；仲未係完整信箱備份。」

### 3.6 本機 AI 頁

標題：「喺呢部 Mac 運行 AI」

硬件：「記憶體：{ramGb} GB」「可用磁碟：{free} GB」「建議模式：{modeLabel}」。顯示呢啲值前要有真實檢測。

分步：「檢查運行環境」「安裝或開啟本機引擎」「下載建議模型」「測試廣東話分析」「完成設定」。

按鈕：「準備本機 AI」「暫停下載」「繼續下載」「取消下載」「重試」「試用另一個模型」。未支援 resume 前唔顯示「繼續下載」。

確認說明：「將下載約 {size} GB。下載期間需要上網；模型分析會喺呢部 Mac 進行。」

完成：「已通過本機分析測試。電郵工作區已選用 {model}。」只有電郵工作區嘅模型設定已成功儲存並通過測試先咁寫；如果只加入一般助手，就顯示「已加入助手服務次序」，唔聲稱 Mail 已可用。

硬件不足：「呢部 Mac 暫時未適合建議模型。你可以揀較細模型處理少量郵件，或者選雲端方式。」

離線說明：「本機文字分析唔會自動轉雲端。Gmail 同步、模型下載及你另外開啟嘅雲端語音功能仍然需要網絡。」

### 3.7 工作總覽

標題：「今日要跟進嘅工作」屬新產品方向，要有真實整合結果先用；現有功能安全文案係「工作總覽」。

說明：「睇返已啟用嘅自動工作、最近結果同下一次執行時間。」

按鈕：「交工作俾 JARVIS」「用語音輸入」「管理自動工作」。空態：「未有啟用中嘅自動工作。你可以先去電郵工作區整理第一組郵件。」

狀態：「執行中」「已暫停」「上次執行失敗」「查看結果」「下次：{dateTime}」「上次結果」。

### 3.8 電郵工作區

保留現有「全部信箱」「同步最近郵件」「載入更舊郵件」「群組」「個人指示」「來源郵件」「斷開連接，保留郵件」等清楚用詞。

改善主文案：「集中多個 Gmail，按客戶或項目整理。每項分析都可以睇返來源。」

新增引導空態：「連接 Gmail，再揀幾封相關郵件建立第一個群組。」

搜尋：「搜尋已下載郵件」。補充「只搜尋呢部 Mac 已下載嘅內容」，避免以為搜尋完整 Gmail。

群組模板建議：「客戶跟進」「報價及合約」「供應商訂單」「申請及文件限期」；模板係預填指示，唔代表自動分類已實作。

分析按鈕：「整理重點同下一步」。分析範圍：「今次分析 {included} 封；另有 {omitted} 封未納入。」

結果欄位：「要你跟進」「限期」「負責人」「金額」「待確認」「原信依據」。冇證據就顯示「原信未有講明」。

### 3.9 群組及已確認背景

標題：「群組設定」；欄位「群組名稱」「整理指示」。placeholder：「例如：用廣東話列出我要覆嘅問題、回覆限期同下一步。唔清楚嘅地方標做待確認。」

現有「群組記憶」可改顯示為「已確認背景」，補充：「你確認嘅事實、偏好同承諾，會用喺之後嘅分析。AI 唔會自行加入。」

欄位：「內容」「類型：事實／偏好／承諾」「來源郵件（可選）」「有效至（可選）」「狀態：有效／已完成」。

來源失效：「來源郵件已移出群組，呢條背景暫時唔會加入分析。」

按鈕：「儲存背景」「查看來源」「標記已完成」。刪除：「刪除後無法還原；來源郵件會保留。」

### 3.10 助手及結果頁

標題：「有咩想交俾 JARVIS？」；placeholder「例如：幫我睇今日嘅會議安排」。

推薦問題：「睇今日日曆」「搵要回覆嘅郵件」。第二項現時用 Assistant Google cache，要附「使用助手已連接嘅 Google 帳戶；電郵工作區信箱另行管理。」

Think harder：「深入分析」，附「可能需要較長時間同較多 AI 用量」。

結果頁：「返回工作」「來源：{provider}」「查看原始資料」「複製文字」「已複製」。

草稿：「回覆草稿」「收件人」「主旨」「本機編輯版本」；說明：「呢度嘅修改只會儲存喺本機。複製到 Gmail，先會更新 Gmail 草稿。」；禁止把「草稿完成」寫成「已寄出」。

### 3.11 日曆

標題：「你嘅日程」；說明「以下時間以 {timezoneLabel} 顯示。同步後先會見到最新安排。」

按鈕：「新增活動」「同步日曆」「今日」。空態：「呢日暫時冇已同步活動。」

詳情：「活動詳情」「時間」「地點」「狀態」「參與者」「備註」「全日」。

新增：「新增個人日曆活動」「活動名稱」「開始時間」「結束時間」「建立活動」。

錯誤：「結束時間要遲過開始時間。」成功：「已喺 Google Calendar 建立個人活動。未有發出邀請；如有需要，請喺 Google Calendar 加入參與者。」

### 3.12 自動工作

標題：「排程同執行紀錄」；說明「排程使用 {timezoneLabel}。你可以查看每次執行嘅實際結果。」

欄位：「名稱」「執行時間」「下次執行」「狀態」「工作指示」「最近執行」。

按鈕：「建立自動工作」「暫停」「啟用」「重新整理」。

狀態：「已啟用」「已暫停」「執行中」「已完成」「執行失敗」「未有執行紀錄」。

排程確認：「將於每個工作日 09:00（香港時間）執行。接落嚟三次：{dates}。」只可當計算實際吻合先顯示；唔把 cron 字串當普通客戶唯一介面。

### 3.13 帳戶連接

標題：「管理帳戶連接」；說明「睇返呢部 Mac 已連接嘅帳戶同服務。」

分區：「電郵工作區嘅 Gmail」「助手及日曆嘅 Google」「其他服務」。

狀態：「已連接」「已斷開，保留本機副本」「未同步」「上次同步：{dateTime}」「暫時讀唔到連接狀態」。

說明：「喺電郵工作區加入 Gmail，唔會自動連接助手及日曆。」；按鈕「管理信箱」「管理助手及日曆」「設定其他服務」。

「儲存 token 唔代表已經成功取得服務權限。請執行連接測試。」需新增測試後先顯示 test 按鈕。

### 3.14 設定

分區：「AI 處理方式」「API key」「助手及日曆」「AWS」「語言及語音」「助手偏好」「外觀」「資料及私隱」。最後一項係建議新增。

Advanced provider chain：「進階：後備 AI 服務次序」；說明「一般助手可能按呢個次序轉用後備服務。電郵分析使用獨立設定。」

語言拆分：「介面語言」「助手回覆語言」「對外文稿語言」「語音辨識語言」「朗讀聲線」「工作時區」。

選項：「香港繁體中文」「English」「廣東話」「正式繁體中文」「按原文語言」「中英夾雜」。具體邊組適用要按資料模型設計，唔一次加十個無法執行嘅設定。

### 3.15 語音

標題：「語音輸入及朗讀」。

按鈕：「試咪」「試聽廣東話」「下載離線辨識模型」「停止朗讀」。

提醒：「按 ⌘⇧J 開始或結束語音輸入。」；模式「使用本機辨識」「使用雲端辨識」，模式要與實際 router 一致。

麥克風錯誤：「JARVIS 未有麥克風權限。請喺 macOS 系統設定允許使用咪。」

未聽清：「未聽清楚，請再講一次，或者直接輸入文字。」

聲線缺失：「呢部 Mac 仲未安裝廣東話聲線。你可以先用文字結果，或者開啟系統聲線設定下載。」

Wake word：「實驗功能：語音喚醒」「聽到『Hey Jarvis』後開始語音輸入」。唔把普通話或英語 fallback 包装成廣東話成功。

### 3.16 GitHub、筆記、更新及錯誤

GitHub：「待處理嘅 Issue 同程式審查」「同步 GitHub」「項目詳情」「儲存庫」「作者」「更新時間」「喺 GitHub 開啟」。

Notion：「Notion 筆記來源」「同步 Notion」「頁面」「來源詳情」「最後修改」「喺 Notion 開啟」。保留限制：「目前同步咗標題同來源資料，未包含頁面正文。」

更新：「檢查更新」「有新版本 {version}」「下載更新」「重新啟動以完成更新」「暫時無法檢查，請稍後再試」。

一般錯誤：「未能完成 {operation}。你嘅已儲存資料仍然保留。」只喺實際 transaction 行為保證冇資料破壞時使用後一句。按鈕「重試」「查看技術詳情」「複製診斷資料」；診斷匯出應先遮蓋敏感內容。


## 4. 現有畫面逐項替換表

呢節全部係**現有替換**，除非該行明確標「待實作」。作用係填補主要文案以外嘅 controls、空態同說明，唔需要為翻譯加新權限。

### 4.1 現有三步 Onboarding 過渡版本

新流程未落地前，可以先翻譯原有三步，並去除假成功。來源：[OnboardingWizard.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/OnboardingWizard.tsx:95)。

| 建議 message key | 現有位置 | 替換文案 |
|---|---|---|
| onboarding.step | STEP {step} / 3 | 第 {step} 步，共 {totalSteps} 步 |
| onboarding.providers.title | Cloud AI providers | 設定雲端 AI |
| onboarding.providers.body | 三個 keys 說明 | 如果你已有 Gemini、OpenAI 或 Claude API key，可以喺呢度加入。一般助手會按已啟用嘅服務次序處理；電郵分析使用獨立設定。你亦可以先跳過。 |
| onboarding.providers.gemini | Gemini recommended placeholder | Gemini API key |
| onboarding.skip | Skip | 暫時跳過 |
| onboarding.next | Next | 下一步 |
| onboarding.google.title | Connect Google | 連接助手及日曆嘅 Google 帳戶 |
| onboarding.google.body | Connect Gmail and Calendar… | 呢個連接供助手電郵工具、日曆同簡報使用。電郵工作區嘅信箱要另外加入。 |
| onboarding.google.scopeTitle | JARVIS will request access to | 呢個連接會要求以下 Google 權限： |
| onboarding.google.scopeMail | Read your Gmail… | 讀取 Gmail 郵件及標籤 |
| onboarding.google.scopeWrite | Send, archive, and label… | 管理 Gmail 郵件及草稿；JARVIS 唔會透過助手直接寄信 |
| onboarding.google.scopeCalendar | Read and create… | 讀取及建立日曆活動 |
| onboarding.google.scopeProfile | 目前漏列 profile，補充文案 | 讀取個人檔案名稱，供 JARVIS 顯示稱呼 |
| onboarding.google.privacy | Content processed… | 相關郵件或日曆內容會交俾所用 AI 服務處理。JARVIS 冇將副本儲存喺自家伺服器。 |
| onboarding.legal | By continuing… | 繼續即表示你同意《私隱政策》同《服務條款》。 |
| onboarding.google.connect | Connect Google | 連接 Google |
| onboarding.google.waiting | Opening Google… | 請喺瀏覽器完成 Google 授權，再返嚟 JARVIS。 |
| onboarding.google.failed | Couldn't connect to Google | 未能連接 Google |
| onboarding.finish.title | Ready | 初步設定已儲存 |
| onboarding.finish.body | Setup complete… | 你可以進入工作區。AI 同帳戶連接仍要按實際測試結果確認；未設定嘅項目可以稍後喺設定補上。 |
| onboarding.finish.start | Start | 進入工作區 |

權限說明要再對照 Google consent screen 同實際 scope，唔因「唔寄信」而隱藏 `gmail.modify` 包含嘅寬權限。正式新 Mail onboarding 應獨立用 readonly 文案。條款 URL 要由已核實官網設定提供，唔原樣沿用未核對域名。

### 4.2 Settings：九個現有分頁

來源：[Settings.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:547)。現有分頁係 Models、API keys、Assistant Google、Notion、GitHub、Obsidian、Voice、Assistant、Appearance；AWS 喺 Models 內。

| key／原文 | 替換文案 |
|---|---|
| settings.title | 設定 |
| settings.description | 設定 JARVIS 喺呢部 Mac 嘅運作方式。 |
| settings.models | AI 模型 |
| settings.keys | API key |
| settings.google | 助手及日曆 |
| settings.notion | Notion |
| settings.github | GitHub |
| settings.obsidian | Obsidian |
| settings.voice | 語言及語音 |
| settings.assistant | 助手偏好 |
| settings.appearance | 外觀 |
| Settings categories | 設定分類 |

**AI 模型及 endpoints**，來源：[Settings.tsx:580](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:580)。

| 原文／場景 | 替換文案 |
|---|---|
| These providers handle Assistant… | 呢度設定一般助手使用嘅 AI。電郵分析使用獨立設定，但會共用呢度儲存嘅 API key。 |
| Mail analysis settings | 開啟電郵分析設定 |
| Provider priority | 助手 AI 服務次序 |
| JARVIS tries enabled providers… | 一般助手會按次序嘗試已啟用嘅服務。用箭嘴調整先後。 |
| Move {label} up／down | 將 {providerLabel} 移前／移後 |
| Enable {label} | 啟用 {providerLabel} |
| Remove from priority | 從服務次序移除 {providerLabel} |
| No providers configured | 未有設定 AI 服務。你可以加入雲端服務或本機模型。 |
| + {provider} | 加入 {provider} |
| Default provider | 預設 AI 服務 |
| Used when no provider chain… | 未設定服務次序時使用。 |
| Local AI setup and endpoints | 本機 AI 設定及連接 |
| Optional. Connect a local server… | 只喺你想用本機 AI 時，先連接本機服務或下載模型。 |
| Local model endpoints | 本機模型連接 |
| Connect to Ollama, vLLM… | 連接 Ollama、vLLM 或相容服務。 |
| Available models | 服務提供嘅模型 |
| {count} models, {latency}ms | {modelCount} 個模型，連接延遲 {latencyMs} 毫秒 |
| Unreachable | 暫時連唔到服務 |
| Test connection | 檢查服務連接 |
| Add to priority | 加入助手服務次序 |
| Name (optional) | 連接名稱（可選） |
| Endpoint URL | 服務網址 |
| Add endpoint | 新增連接 |
| tools／prompt-tools／chat | 原生工具呼叫／提示詞工具呼叫／文字對話 |
| local／default／unknown | 本機／預設／未有模型資料 |

「檢查服務連接」只係現有 health／模型清單檢查，唔改寫成「已通過電郵分析測試」。`{latencyMs}` 係 health request 延遲，唔係模型回覆時間。

**API key**，來源：[Settings.tsx:703](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:703)。

| 原文／場景 | 替換文案 |
|---|---|
| Provider API keys | AI 服務 API key |
| Keys are stored… | 密鑰儲存喺 macOS Keychain，並用作連接相應服務。 |
| Key available | 有可用嘅密鑰設定，尚未驗證連線 |
| Replace saved key | 輸入新密鑰以取代原有密鑰 |
| Save／Remove key | 儲存／移除密鑰 |
| Saved keys are never displayed… | 已儲存嘅密鑰唔會再顯示。留空會保留原有設定；輸入新密鑰會取代舊密鑰。 |
| No key was entered… | 未有輸入密鑰，所以冇更改設定。輸入新密鑰再儲存，或者撳「移除密鑰」。 |

現有 status 亦可能源於環境變數，唔應一概顯示「已存入 Keychain」。條件顯示「已儲存」只跟剛完成嘅儲存操作；全局 status 用「有密鑰設定」。

**助手及日曆 Google**，來源：[Settings.tsx:745](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:745)。

| 原文／場景 | 替換文案 |
|---|---|
| Google for Assistant and Calendar | 助手及日曆嘅 Google 帳戶 |
| Connect Google for Assistant… | 連接後可使用助手電郵工具、日曆同簡報。電郵工作區使用另一套 Gmail 連接。 |
| Checking Google connection… | 檢查緊 Google 連接… |
| Status is unavailable | 暫時讀唔到 Google 連接狀態，請重試。 |
| Connected as {name} | 已連接：{displayName} |
| Connected／Connecting… | 已連接／連接緊… |
| Switch account | 更換帳戶 |
| Disconnect | 斷開連接 |
| 更換確認 | 更換助手及日曆嘅 Google 帳戶？會撤銷目前連接，並刪除呢部 Mac 嘅助手郵件快取、日曆資料同簡報。電郵工作區嘅副本會保留。 |
| 斷開確認 | 斷開助手及日曆嘅 Google 連接？會撤銷存取權，並刪除相關本機助手郵件快取、日曆資料同簡報。你可以之後再連接；電郵工作區副本會保留。 |

**Notion／GitHub／Obsidian**，來源：[Settings.tsx:790](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:790)。

| 原文／場景 | 替換文案 |
|---|---|
| Enter your Notion integration token | 輸入 Notion 整合權杖（token）。只有分享俾呢個整合嘅頁面先可以存取。 |
| Enter your GitHub personal access token | 輸入 GitHub 個人存取權杖（token）。 |
| Notion／GitHub token saved | 權杖已儲存，尚未驗證服務權限。 |
| Connect to Obsidian vault… | 透過 Obsidian 嘅 Local REST API 外掛連接筆記庫。 |
| API key from Obsidian… | 輸入 Local REST API 外掛提供嘅 API key |
| Install Local REST API… | 請先喺 Obsidian 安裝並啟用 Local REST API 外掛，再複製 API key 到呢度。 |
| Endpoint | 服務網址 |
| Leave blank for plugin default… | 留空會使用外掛預設 HTTPS 連接。只有你改過連接埠或明確啟用 HTTP 服務，先需要改呢度。 |
| Save／Saved | 儲存／已儲存 |

**AWS Bedrock 區塊**，來源：[Settings.tsx:840](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:840)。

| 原文／場景 | 替換文案 |
|---|---|
| 區塊說明 | 使用你自己嘅 AWS 帳戶進行 Bedrock 推論。能否使用視乎地區、模型權限同帳戶設定；費用由你嘅 AWS 帳戶支付。 |
| Currently configured for | 目前設定地區：{region} |
| Access Key ID | 存取金鑰 ID（Access Key ID） |
| Secret Access Key | 秘密存取金鑰（Secret Access Key） |
| Region | AWS 地區 |
| Singapore／Tokyo／US East | 新加坡（ap-southeast-1）／東京（ap-northeast-1）／美國東部（us-east-1） |
| Save／Remove key | 儲存 AWS 憑證／移除 AWS 憑證 |

冇實測就唔加「香港最快」「建議」「低延遲」；地區唔一定等於所有 inference processing 地區，要按模型／inference profile 真正路由說明。AWS credits 適用與否唔由文案保證。

**語言、語音及喚醒**，來源：[Settings.tsx:889](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:889)。

| 原文／場景 | 替換文案 |
|---|---|
| Language | 回覆及辨識語言 |
| Controls the assistant… | 設定助手回覆同語音辨識語言。呢個設定暫時唔會改介面語言。 |
| Response language | 助手回覆語言 |
| Speech-to-text language | 語音辨識語言 |
| Auto-detect／Bilingual | 自動辨識／廣東話同英文夾雜 |
| Speech／Enable text-to-speech | 語音朗讀／開啟語音朗讀 |
| TTS Voice | 英文朗讀聲線 |
| Cantonese voice | 廣東話朗讀聲線 |
| Speech-to-text ready -- cloud… | 已有 Gemini 語音辨識設定，本機 Whisper 模型可作後備。實際可用性要按辨識結果確認。 |
| Speech-to-text ready -- local… | 本機 Whisper 模型已下載，辨識效果要按實際測試確認。 |
| Speech-to-text needs… | 語音輸入需要 Gemini API key，或者本機 Whisper 模型。你可以先用文字輸入。 |
| Wake word／Enable wake word | 語音喚醒／啟用語音喚醒 |
| Experimental… | 實驗功能：喺本機辨識「Hey Jarvis」，聽到後開始語音輸入。 |
| Download model／Download again | 下載辨識模型／重新下載辨識模型 |
| Model ready | 模型已下載 |
| Downloading… | 下載緊… |
| 模型檔案損壞提示 | 如果模型載入持續失敗，可以重新下載取代原有檔案。 |
| wake privacy | 喚醒詞會喺本機辨識。喚醒後，如有 Gemini API key，完整語音指令會先交俾 Gemini 辨識；失敗後先嘗試本機 Whisper。 |

**助手偏好及外觀**，來源：[Settings.tsx:985](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:985)。

| 原文／場景 | 替換文案 |
|---|---|
| Briefing | 每日簡報 |
| Speak morning briefing… | 開啟 JARVIS 時朗讀每日簡報（每日一次） |
| Colour theme | 色彩模式 |
| Choose a light or dark… | 選擇淺色、深色，或者跟隨 Mac 設定。 |
| Light／Dark／System | 淺色／深色／跟隨系統 |
| A bright workspace／A darker workspace | 較明亮嘅工作區／較暗嘅工作區 |
| Desktop wallpaper | 桌面背景模式 |
| Full app as wallpaper… | 將 JARVIS 放喺其他視窗同桌面圖示後面，作為動態桌面背景。 |
| Enable wallpaper mode | 啟用桌面背景模式 |
| Toggling… | 切換緊… |
| Wallpaper active／inactive | 桌面背景模式已啟用／桌面背景模式未啟用 |
| 操作說明 | 啟用後仍可操作 JARVIS，直到你將佢送到背景。之後可按選單列圖示或 ⌘⇧W 開返；語音喚醒已啟用時亦可講「Hey Jarvis」。關閉視窗會將 JARVIS 送回背景。 |

### 4.3 模型下載現有版本

來源：[LocalAiSetup.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/LocalAiSetup.tsx:140)、[ModelPullToast.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/ModelPullToast.tsx:9)。

| 原文／場景 | 可直接替換文案 |
|---|---|
| LOCAL AI SETUP | 本機 AI 設定 |
| RAM | {ramGb} GB 記憶體 |
| No GPU acceleration | 未偵測到 GPU 加速 |
| Up to {params} models | 初步模型大小建議：{parameterSize}（未包含實際上下文用量） |
| REFRESH／COPY | 重新檢查／複製指令 |
| Ollama installed | 已找到 Ollama |
| Ollama running | Ollama 服務有回應 |
| Models available | 服務提供模型清單 |
| Install Ollama | 安裝 Ollama |
| Start Ollama | 啟動 Ollama |
| RECOMMENDED MODELS | 可考慮嘅模型 |
| PULL | 下載模型 |
| READY | 偵測到相關模型；請核對型號 |
| + CHAIN | 加入助手服務次序 |
| Pulling {model}… {percent}% | 下載緊 {model}：{percent}% |
| verifying | 核對緊模型檔案… |
| CANCEL | 取消下載 |
| CONNECT | 連接服務 |
| Click REFRESH… | 撳「重新檢查」確認本機服務狀態。 |

上表 READY 用保守過渡文案，因為現有 family substring 判定有誤標風險；修好精確 tag／digest及最小推論之後先升級為「已通過分析測試」。同樣 `complete` event 目前只表示 stream 結束，唔作可用證據。來源：[system_check.rs:100](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/system_check.rs:100)、[model_pull.rs:175](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/model_pull.rs:175)。

### 4.4 Mail 表單、刪除及分析錯誤

來源：[EmailPage.tsx:125](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/EmailPage.tsx:125)。現有大部分文案可保留；唔為全 App 翻譯改已通過嘅資料行為。

| 場景 | 文案 |
|---|---|
| 同步範圍 | 每次最多同步 50 封，唔代表已備份成個信箱。 |
| 分析範圍 | 分析以成個群組作範圍，唔受左邊信箱篩選影響；實際納入郵件受分析上限限制。 |
| 刪除帳戶確認 | 刪除 {accountLabel} 同本機郵件？相關群組連結，以及以呢個帳戶郵件為來源嘅背景記憶亦會刪除。其他自行加入嘅背景會保留。Gmail 原信唔受影響。 |
| 刪除群組確認 | 刪除「{groupName}」？群組指示同背景記憶亦會刪除；本機郵件會保留。 |
| 刪除記憶確認 | 刪除呢條背景記憶？刪除後無法還原；原信會保留。 |
| 副作用按鈕 | 確認刪除／取消 |
| Processing mode | 分析方式 |
| Local model | 本機 Ollama 模型名稱 |
| OpenAI model | OpenAI 模型名稱 |
| Bedrock model | Bedrock 模型 ID |
| OpenAI consent | 我明白撳分析時，群組郵件、個人指示同有效背景記憶會傳送到 OpenAI。 |
| Bedrock consent | 我明白撳分析時，群組郵件、個人指示同有效背景記憶會傳送到 AWS Bedrock。 |
| 雲端費用 | 分析用量由你嘅 {provider} 帳戶支付。郵件副本留喺本機；失敗唔會自動轉用其他分析服務。 |
| 來源問題 | 今次結果未能通過來源核對，暫時唔顯示為有效分析。請重試，或減少郵件數量。 |
| 無模型回覆 | AI 今次冇傳回可顯示嘅結果，請重試。 |
| 上下文超量 | 呢個模型暫時處理唔到今次資料量。請用較少郵件建立群組，或者揀支援較大內容量嘅模型。 |

錯誤顯示後唔把舊結果改稱最新結果；如保留上次結果，要顯示「上次成功結果：{dateTime}」。呢個時間標記需要資料欄位時屬待實作。

### 4.5 Launch／Beta 內部頁

來源：[LaunchPage.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/LaunchPage.tsx:87)。**歷史／非主入口**。呢頁唔應作現成售價、正式 team 功能或 AWS 已驗證能力嘅證據。

| 原文／場景 | 文案及限制 |
|---|---|
| AWS STARTUP LAUNCH | 內部試用驗證 |
| Beta Control Plane | 試用管理 |
| PRIVATE BETA | 邀請制試用 |
| AWS READINESS | AWS 設定完整度 |
| Bedrock launch checks complete | 已填妥 {completedChecks}／{totalChecks} 項設定，唔代表已通過真實推論測試 |
| AWS credentials／region／model／chain | AWS 憑證／Bedrock 地區／Bedrock 模型／助手服務次序 |
| READY／TODO | 已有設定／待設定 |
| Set AWS_BEDROCK_REGION | 到 AI 模型設定選擇 AWS 地區 |
| Set AWS_BEDROCK_MODEL_ID | 到相關模型設定檢查 Bedrock 模型 ID |
| COMMERCIAL SIGNAL | 試用及定價研究 |
| Pro／Team price | 定價假設（待驗證）：{planName}，HK${amount}／{billingUnit} |
| Audit logs and admin policy | 團隊功能研究中，暫未提供管理政策或審計紀錄 |
| WAITLIST／FEEDBACK | 本機已記錄試用者／本機已記錄意見 |
| BETA ACCESS | 記錄試用者 |
| Name／Email | 稱呼／電郵地址 |
| Founder／Operator／Developer／Team lead | 創辦人／營運人員／開發者／團隊主管 |
| What should JARVIS save time… | 你最想 JARVIS 先幫你慳邊一部分時間？ |
| SAVE BETA TESTER | 儲存試用者資料 |
| Beta tester saved… | 試用者資料已儲存喺本機。提交意見時可以使用同一個電郵地址。 |
| TEST FEEDBACK | 試用意見 |
| Activation／AWS setup／Workflow reliability／Pricing | 首次設定／AWS 設定／工作流程可靠程度／定價 |
| Rating | 評分（1 至 5 分） |
| Feedback placeholder | 有邊度用唔到、幫你慳咗幾多時間，或者仲欠咩先值得付費？ |
| SAVE FEEDBACK | 儲存意見 |
| Feedback saved… | 意見已儲存喺本機，供產品驗證使用。 |

「儲存意見」唔寫成「已傳送俾 JARVIS 團隊」，因為呢條現有路徑記錄本機資料。任何對外提交要另實作同取得相應授權。

### 4.6 舊 Dashboard 及共用卡片

來源：[Dashboard.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Dashboard.tsx:11)、[Briefing.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/Briefing.tsx:68)、[CostPanel.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/CostPanel.tsx:83)。**Dashboard 係歷史／非主入口**，共用卡片如再次顯示仍要翻譯。

| 原文／場景 | 文案 |
|---|---|
| SYSTEM ERROR | 暫時開唔到總覽 |
| INITIALIZING… | 準備緊工作資料… |
| DAILY BRIEFING／SHOW BRIEFING | 每日簡報／顯示簡報 |
| GENERATING BRIEFING… | 整理緊今日簡報… |
| SPEAK／SPEAKING | 朗讀／朗讀緊… |
| DISMISS | 收起 |
| TASKS／pending | 工作／待處理 |
| EMAIL／unread／not connected | 郵件／未讀／未連接 |
| TIMELINE | 工作時間線 |
| AUTO-ARCHIVE SUGGESTION | 自動封存規則建議 |
| ENABLE／DISMISS | 啟用規則／略過 |
| INFERENCE COST · 30D | 最近 30 日 AI 推論費用估算 |
| Total／Per turn／Turns／Local share | 總計／平均每輪／對話輪數／本機處理比例 |
| 費用說明 | 呢度係程式按記錄用量計算嘅估算，唔係供應商最終帳單。 |

CostPanel 目前用 `$` 顯示 USD；翻譯時應明寫 `US$`，唔假設係港元。資料為空就顯示「未有估算資料」，唔當成 0。舊封存規則可能影響 Gmail，唔混入 readonly Mail 工作區而唔交代差別。

## 5. 使用者可見錯誤與下一步

以下係**待實作嘅結構化錯誤 catalog**，文案可以先採用，但真實錯誤分類要有後端證據。唔透過分析 raw 英文錯誤猜用戶帳戶身份；保留安全 error code 同 retryable 欄位。對 raw provider body、token、OAuth code、callback URL、原信正文一律唔直接輸出到錯誤畫面或診斷。

| 建議錯誤碼 | 顯示文案 | 下一步／按鈕 |
|---|---|---|
| AI_KEY_MISSING | 仲未設定 {provider} API key。 | 開啟 API key 設定 |
| AI_KEY_REJECTED | {provider} 未接受呢個密鑰。請確認密鑰仍有效。 | 更新密鑰／重新測試 |
| AI_QUOTA_UNAVAILABLE | {provider} 帳戶暫時冇可用額度，或者已到用量上限。 | 開啟官方帳單頁；唔自動充值 |
| AI_MODEL_UNAVAILABLE | 呢個帳戶暫時用唔到 {model}。 | 揀另一個已驗證模型 |
| AI_RATE_LIMITED | AI 服務暫時太忙，請稍後再試。 | 重試；有可信 retry-after 先顯示秒數 |
| AI_NETWORK | 暫時連唔到 {provider}。 | 檢查網絡，再試一次 |
| AI_RESPONSE_INVALID | 今次回覆未能通過結果核對。 | 重試／減少分析內容 |
| AI_TIMEOUT | 今次分析超過等候時間，未有有效結果。 | 重試；唔顯示假「已完成」 |
| OAUTH_BUILD_CONFIG | 呢個版本未有可用嘅 Google 登入設定。 | 聯絡支援；唔叫客戶填 .env |
| OAUTH_DENIED | 你未有授權呢次連接。 | 重新連接／稍後再做 |
| OAUTH_TIMEOUT | 今次 Google 授權等候時間已過。 | 重新連接，開新授權流程 |
| OAUTH_ADMIN_BLOCKED | 公司可能需要管理員先批准呢個應用程式。 | 查看管理員說明／保留設定稍後再試 |
| OAUTH_EXPIRED | Google 連接已失效，需要重新授權。 | 重新連接；保留本機資料 |
| MAIL_SYNC_PARTIAL | 已儲存 {savedCount} 封，仲有部分郵件未完成同步。 | 重試同步；保留已完成部分 |
| MAIL_EMPTY | 冇符合條件嘅已下載郵件。 | 清除搜尋／調整篩選／同步信箱 |
| LOCAL_ENGINE_MISSING | 呢部 Mac 仲未有可用嘅本機 AI 引擎。 | 準備本機 AI（待實作）／目前顯示安裝說明 |
| LOCAL_ENGINE_STOPPED | 本機 AI 引擎未有回應。 | 啟動引擎（待實作）／目前顯示啟動說明 |
| LOCAL_DISK_LOW | 下載模型前需要更多磁碟空間。 | 查看所需空間／揀較細模型；唔自動刪檔 |
| LOCAL_DOWNLOAD_FAILED | 模型下載未完成。 | 重試下載；唔標 READY |
| LOCAL_DOWNLOAD_CANCELLED | 已取消今次下載。 | 重新下載；只有有 resume 機制先顯示繼續 |
| LOCAL_MODEL_MISSING | 服務冇提供所選模型 {model}。 | 下載呢個模型／改用已安裝模型 |
| LOCAL_CONTEXT_TOO_SMALL | 呢個模型處理唔到今次內容量。 | 減少郵件／選較大上下文模型 |
| VOICE_PERMISSION | JARVIS 未有麥克風權限。 | 開啟 macOS 權限設定／改用文字 |
| VOICE_UNINTELLIGIBLE | 未聽清楚今次內容。 | 再講一次／輸入文字 |
| VOICE_MODEL_UNLOADABLE | 本機辨識模型未能載入。 | 重新下載／改用文字 |
| VOICE_YUE_UNAVAILABLE | 呢部 Mac 暫時未有可用嘅廣東話聲線。 | 安裝聲線／睇文字結果 |
| SETTINGS_SAVE_FAILED | 未能儲存今次設定。 | 重試；畫面唔應顯示已儲存 |
| STATUS_UNAVAILABLE | 暫時讀唔到狀態。 | 重新整理；唔當作已斷開 |
| UPDATE_FAILED | 更新未完成。你可以繼續用目前版本，再試一次。 | 重試更新；只有已知目前 App 可繼續先用呢句 |
| WORKSPACE_LOAD_FAILED | 暫時開唔到呢個工作區。 | 重新載入呢頁 |

Google callback HTML 嘅「Authentication successful」應先改成「已收到 Google 回應，請返去 JARVIS 完成連接。」只有 App 已完成 token exchange、驗證帳戶及儲存後，App 先顯示「已連接」。OAuth callback 收到 code 嘅時點喺[google.rs:223](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/auth/google.rs:223)。

## 6. 無障礙、輸入及通知文案

來源：[WorkspaceTabs.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/WorkspaceTabs.tsx:5)、[AssistantPanel.tsx](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/AssistantPanel.tsx:35)。

| 元件／label | 香港繁體中文 |
|---|---|
| App navigation | JARVIS 主導覽 |
| Search workspaces and tasks | 搜尋工作區同工作 |
| Close search | 關閉搜尋 |
| Tablist | 已開啟嘅工作區 |
| Close {tabTitle} | 關閉「{tabTitle}」分頁 |
| Pinned | 已固定 |
| Double-click to pin | 按兩下固定分頁 |
| New task tab | 新增工作分頁 |
| New result | 有新結果 |
| Message JARVIS | 輸入俾 JARVIS 嘅指示 |
| Send message | 送出指示 |
| Stop speaking | 停止朗讀 |
| Talk to JARVIS | 開始語音輸入 |
| End voice input | 結束語音輸入 |
| Start another task | 開始另一項工作 |
| Open conversation tab | 開啟完整對話分頁 |
| Close assistant | 收起助手 |
| Select {subject} | 選取郵件「{subject}」 |
| Choose a day | 選擇日期 |
| Progress | {operation}，已完成 {percent}% |
| Dismiss notification | 收起通知 |
| Dialog delete | 確認刪除 {entityLabel} |
| Current workspace | 目前工作區：{workspaceName} |

錯誤用 `role=alert`，一般進度用 `role=status`；更新百分比唔每個 event 都大聲播讀，按合理間距 announce。按鈕 disable 要有附近可理解嘅原因。dialog 打開時移焦點，關閉後返去原按鈕。placeholder 唔代替 label；只得 icon 嘅按鈕要有本地化 accessible name。中文 IME composing 時 Enter 唔會誤送出；現有 guard 要保留。翻譯唔應改快捷鍵語義。

## 7. 佔位變量與資料邊界

| 變量 | 定義／例子 | 顯示規則 |
|---|---|---|
| `{completedChecks}`／`{totalChecks}` | 已填妥設定項目數／總項目數 | 只反映配置完整度，唔代表真實推論成功 |
| `{step}`／`{totalSteps}` | 當前步數／真實總步數 | 同畫面流程一致，不能固定 3 後又加步驟 |
| `{provider}` | 已選供應商顯示名，如 OpenAI | 由固定 catalog 提供，唔由未可信郵件注入 |
| `{providerLabel}` | 供應商＋模型短名 | 包括可辨別實際用途嘅標籤 |
| `{modeLabel}` | 實際通過檢查嘅模式建議 | 清楚寫適用用途，冇硬件檢查唔作建議 |
| `{model}` | 實際模型完整 tag／ID | 唔用 family 名代替；唔寫假推論能力 |
| `{region}` | 保存及實際使用嘅 AWS region | 有 mismatch 顯示設定問題 |
| `{count}`／`{savedCount}` | 真實本次同步數量 | 0 同未知分開；唔用本機總數冒充本次新增 |
| `{included}`／`{omitted}` | 今次分析納入／未納入數量 | 後端回傳，唔用前端猜測 |
| `{groupName}` | 用戶群組名 | 原文保留、escape，唔翻譯 |
| `{accountLabel}` | 用戶所見信箱識別 | 只喺管理帳戶必要畫面顯示；診斷文件遮蓋 |
| `{displayName}` | 帳戶設定提供嘅稱呼 | 冇資料用「已連接」，唔捏造使用者名字 |
| `{dateTime}`／`{dates}` | 已按選定時區格式化嘅時間／清單 | 包含時區；跨日期唔省日期 |
| `{timezoneLabel}` | 例如「香港時間（Asia/Hong_Kong）」 | 用 formatter 真實時區，唔只看語言 |
| `{ramGb}`／`{free}` | 實際檢測 RAM／可用磁碟 | 區分 total／available；值不可用顯示「未能讀取」 |
| `{size}`／`{parameterSize}` | 已核實下載量／模型參數級別 | 唔直接沿用過期靜態表，近似值標「約」 |
| `{modelCount}`／`{latencyMs}` | health 回傳模型數／連接延遲 | 唔當成推論品質／回覆時間 |
| `{percent}` | 0–100 下載進度 | 總量未知用「下載緊…」，唔虛構百分比 |
| `{version}` | 已驗證更新 manifest 版本 | 下載完成唔等於 App 已重新開到新版本 |
| `{planName}`／`{amount}`／`{billingUnit}` | 定價研究名／金額／每月或每席 | Launch 只標假設；唔宣稱訂閱權限已可執行 |
| `{operation}`／`{entityLabel}` | 固定操作／項目類別 | 由 catalog提供，唔直接塞 raw SQL或backend錯誤 |
| `{tabTitle}`／`{workspaceName}`／`{subject}` | 使用者或來源內容 | 保留原文、正確 escape；空主旨顯示「冇主旨」 |

原文欄出現嘅 `{label}`／`{name}`／`{params}`／`{latency}` 係既有程式舊變量，套用時分別對應 `{providerLabel}`／`{displayName}`／`{parameterSize}`／`{latencyMs}`，唔建立另一套含義。

所有 source quote 原文保留。報告、模型回覆、通知都唔把「已引用」等同「內容一定正確」；引用只提供核對入口。

## 8. 工程套用與驗收清單

1. 新建集中 locale catalog；分清 `ui_locale` 與現有 `language`／`stt_language`。現有用戶設定優先保留，唔因香港版覆蓋已選 GPT、API key、Gmail 或時區。
2. 按第 2 節來源逐頁抽取所有標題、說明、controls、error、toast、aria-label、tooltip及原生 callback。用 message key 同型別化插值；不要只翻可見 `<h1>`。
3. 第 3 節待實作例外要有 feature gate，未有 command／狀態／資料欄位就唔顯示未存在按鈕。單純翻譯唔順便擴大 OAuth或工具權限。
4. 工作區自動分頁標題按 locale 重新解析，保留用戶自己改過嘅 title；舊 English persisted title 要有安全相容策略。來源：[model.ts:19](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/model.ts:19)。
5. 確認真實帳戶或供應商字串係原文資料，不通過翻譯字典；預設分類狀態如 `active / paused / failed` 要翻譯，底層 enum 保持。
6. 採用一致 formatter；香港／倫敦環境測跨午夜、全日活動、DST、截止日、排程未來三次。變 UI 語言唔變事件 instant，冇時區資訊唔靠 locale猜。
7. 乾淨 profile、保留已登入 profile、斷線、錯 key、無額度、Google 管理員阻擋、空信箱、部分同步、模型下載中止、缺聲線全部有下一步。測試用合成內容，唔用真實郵件做 smoke test。
8. 視窗變窄、字體放大、長廣東話提示、VoiceOver、鍵盤、中文 IME 都可操作；唔裁走「取消」「確認刪除」或設定影響。
9. 編譯檢查配合現有 Rust／frontend tests同隔離原生 QA；字串與模型結果驗收分開。翻譯 build通過唔代表廣東話ASR／LLM準確度通過。
10. 對外付費文案覆核：唔出現「全部一鍵完成」「所有郵件永不離機」「完全離線」「已完成團隊管理」「已通過香港法規」等未證明承諾。介紹要跟當次真正可用能力一致。

本手冊交付後仍需要實作同原生驗收。呢次只新增文件，冇聲稱香港版介面、代管 API 或一鍵本機引擎已完成。
