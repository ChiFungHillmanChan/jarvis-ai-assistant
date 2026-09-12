# JARVIS 香港版啟用、本機 AI 及廣東話實作方案

> **畀之後執行嘅 agent：**按獨立工作逐項實作及審核，可用 `superpowers:subagent-driven-development` 或 `superpowers:executing-plans`。2026-09-12 用戶已批准開始實作，並新增左上角視窗按鈕要求。checkbox 未勾選仍係待辦；以文件尾嘅實作紀錄分清已完成、已驗證同外部依賴。

**目標：**由一個設定入口完成電郵工作區所需嘅 AI、Gmail 及首個有來源結果，提供唔使 API key 嘅本機模式，並有完整香港繁體中文操作指引。

**架構：**保留 Assistant 同新 Mail 嘅儲存／憑證邊界，新增共用設定狀態及導引。第一版集中現有 OpenAI／Bedrock／Ollama，按地區及實測結果選用；本機操作由 Rust 控制，前端只呈現狀態。JARVIS 代管 AI 係另一個有先決條件嘅子項目，唔混入第一版本機流程。

**技術：**Tauri v2、Rust／tokio、SQLite／refinery、React 18、TypeScript、Vite、現有 Node frontend tests。

**設計依據：**[產品及成本報告](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/2026-09-12-hk-launch-affordable-ai-report.md)、[逐頁廣東話文案](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/product/2026-09-12-cantonese-copybook.md)。所有擬議介面、型別同檔名只係本方案契約，唔代表 repo 已存在。

## 共通限制

- 基準為 2026-09-12 repo `main`／`6570acf`／`0.2.0`；開工再核對 status，保留現有改動。
- 最新 migration 檔案為 V24；只有真係需要 schema 變更先加 migration，開工再確認下一編號。唔改已套用 SQL。
- 保留已選 provider／model／Gmail／Keychain；唔為改導引自動切換既有用戶模式。
- 唔 dump `.env`、Keychain、OAuth callback、真實郵件；測試用合成資料及隔離 DB。
- Mail 連接繼續用 `gmail.readonly`，唔為統一 UI 合併 token／資料表或擴大 scope。
- 本機文字分析唔自動 fallback 雲端；Assistant 原有 fallback 喺設定清楚交代，唔當同一機制。
- 原信、用戶自訂標題、品牌／模型 ID 保留原文；UI、文件及程式新增文案唔用 emoji。
- 設定 key 成功、HTTP endpoint 有回應、模型下載 EOF、解析到音訊 bytes，都唔等於功能已可用。
- UI 沿用現有暗底／cyan／字體；唔順手重做產品視覺或無關整合。
- 真正一鍵啟動目標係「Ollama 已完成 OS 安裝後」；首次安裝保留必要 OS 互動及下載等候。
- 收費、Google 公開驗證、政策確認、公司批准、公證／公開發佈各自有證據要求，唔把本方案當已完成許可。

## 工作拆分同先後

| 子項目 | 可獨立交付嘅結果 | 依賴 | 粗略工程估計，未排期 |
|---|---|---|---|
| A. 誠實設定狀態及模型目錄 | 現有頁面唔再誤報 ready，key 及模型有驗證狀態 | 無 | 1–2個專注工作日 |
| B. 核心廣東話 | shell、設定、Mail、Connections及導引可切換 | A嘅錯誤碼契約 | 2–3日 |
| C. 本機啟動流程 | 唔使terminal，精確下載／測試／套用 | A | 2–4日 |
| D. Mail 首次體驗 | 一個入口直達第一份有引用分析 | A、B；本機路徑依賴C | 2–3日 |
| E. 全頁面及香港格式 | 其餘頁面、時區、結果語氣同無障礙 | B | 2–4日 |
| F. 發行及pilot驗收 | 乾淨安裝／重開／失敗恢復／真實適用地區證據 | 上述準備使用嘅子項目 | 2–3日，外部審核另計 |
| G. 免key代管AI | 有帳戶、額度、成本及資料契約嘅付費路徑 | pilot證明key係主要阻力；供應商／政策通過 | 另作架構估算，唔納入四週保證 |

四週係驗證節奏，唔代表上表所有項目四週內必須做晒。每週只有15–20小時時，先交A、核心B、C／D最小垂直流程，再用pilot決定E同G深度。每完成一個可驗收結果先擴範圍。

## 追加：macOS 視窗按鈕

- [x] 左上角由左至右：關閉（紅色）、最小化（黃色）、放大／還原（綠色）。
- [ ] 保留真正 Tauri close／minimize／toggleMaximize 動作、無障礙標籤同非拖曳區域；喺隔離原生 App 驗次序同動作。

## A. 設定狀態同假成功修正

**現有檔案：**[settings commands](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/settings.rs)、[system_check](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/system_check.rs)、[model_pull](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/model_pull.rs)、[Settings](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx)、[typed commands](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/lib/commands.ts)、[Mail wrappers](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/lib/mail.ts)。

**擬新增：**`/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/lib/setup.ts`（前端狀態契約）、`/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/setup.rs`（只讀狀態／主動合成測試入口）。新增 IPC 同時更新 commands module、lib handler 註冊及 typed wrapper。

前端契約草案：

```ts
export type SetupScope = "mail" | "assistant" | "stt" | "tts";
export type Readiness =
  | "not_configured" | "saved_unverified" | "checking"
  | "ready" | "action_required" | "temporarily_unavailable";

export interface CapabilityStatus {
  scope: SetupScope;
  state: Readiness;
  provider: string | null;
  model: string | null;
  checkedAt: string | null;
  errorCode: string | null;
}
```

`get_setup_status()` 只讀憑證有冇／設定／上次驗證資料，唔將憑證值回傳，亦唔自動發收費 request。`test_ai_connection(scope: SetupScope)` 由用戶撳「測試」先執行，使用不含郵件嘅短合成輸入；provider/model/region/憑證版本改變即令舊驗證失效。上次成功唔係服務永久可用，顯示時間，實際使用仍處理失敗。

- [ ] 先寫模型匹配測試：已安裝 `qwen2.5:7b`，對 `qwen2.5:14b` 必須 false；只有exact tag或同一經核實digest先true。
- [ ] 移除family substring判定；修正Llama標示，候選目錄保存tag、包大小、量化、來源、最後核對日；唔自動下載新模型。
- [ ] 先寫下載fixture：一段progress後EOF、stream內error、HTTP非2xx、無效JSON、取消全部唔可以emit complete。
- [ ] 要求明確`status: success`；完成後重新查exact模型。把「下載成功」同「模型測試成功」分開，唔篡改下載層語意。
- [ ] 補有key但無額度、401、429、未知模型、timeout測試；對外只回穩定錯誤code，唔回raw response body。
- [ ] 設定頁顯示「已儲存，未測試」；同時列Mail／Assistant／STT／TTS各自狀態，唔以GPT成功暗示Gemini STT成功。

測試案例示意，作為之後測試設計輸入：

```text
case: key_saved_but_not_probed
input: credential_present=true, probe_record=none
expected: saved_unverified; no outbound request; no secret in IPC

case: download_ended_early
input: status="downloading", completed=1024, total=8192, then EOF
expected: interrupted; zero completion events; selected model unchanged

case: model_family_is_not_exact_match
input: installed=["qwen2.5:7b"], requested="qwen2.5:14b"
expected: not_installed
```

**驗收：**可喺現有Settings使用，未重做wizard都受惠；測試網路錯誤唔改工作中設定。現有系統檢查可能只靠PATH，Finder啟動要另測已安裝Ollama App嘅偵測。

## B. 核心廣東話同集中翻譯

**現有檔案：**[App](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/App.tsx)、[Sidebar](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/Sidebar.tsx)、[workspace model](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/model.ts)、[Connections](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/ConnectionsPage.tsx)、Settings、OnboardingWizard、EmailPage。

**擬新增：**`/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/i18n/` 下 `en.ts`、`zh-HK.ts`、`index.tsx`；每份只負責字典／React context／格式化入口。先用輕量typed catalog，唔為兩個locale引入大型翻譯後端。

- [ ] 新增`ui_locale` preference，值只接受`en`／`zh-HK`；現有`language`繼續用作Assistant輸出，唔偷改其意思。
- [ ] 以OS建議初值並讓用戶揀；已儲存選擇優先。`document.documentElement.lang`跟UI更新。
- [ ] 定義key parity及變量一致性測試；漏翻譯開發時報錯，production安全fallback有明確規則，唔顯示raw key。
- [ ] 將A錯誤code映射至字典；插值用純文字，唔將翻譯字串直接注入HTML。
- [ ] 依copybook替換核心頁面可見字串、ARIA、tooltip、空態、載入／失敗、對話框及通知。
- [ ] 預設分頁標題儲存識別資料後依locale呈現；用戶自訂標題原文保留。舊rendered title資料先辨別系統預設，唔用全文取代損害自訂名。
- [ ] 測中文IME `isComposing`，Enter選字唔誤送；鍵盤焦點同按鈕disabled狀態保留。

建議frontend測試檔：`/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/tests/i18n.test.ts`。最少檢查：同key集、同插值變量；重開保存locale；自訂title不翻譯；系統title可翻；`ui_locale`唔改`language`／provider。

**驗收：**純廣東話使用者可以完成核心工作；800px左右窄視窗同放大文字時，長錯誤訊息唔遮主要按鈕。翻譯修改唔需要逐句鏡像測試，測行為及漏key即可。

## C. 本機 AI 啟動及模型準備

**現有檔案：**system_check、model_pull、LocalAiSetup、[ModelPullToast](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/ModelPullToast.tsx)、[mail analysis](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/mail/analysis.rs)、[capabilities](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/capabilities/default.json)。

**擬新增：**`/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/local_setup.rs`，只負責探測／啟動／job狀態，重用現有下載與分析邏輯；唔重寫一般Assistant router。

前端操作契約草案：

```ts
export type LocalSetupStage =
  | "checking" | "needs_install" | "starting" | "downloading"
  | "verifying_model" | "testing" | "ready"
  | "cancelled" | "failed";

export interface LocalSetupProgress {
  jobId: string;
  stage: LocalSetupStage;
  model: string;
  completedBytes: number | null;
  totalBytes: number | null;
  errorCode: string | null;
}
```

`prepare_local_ai({ scope: "mail" | "assistant", model: string })` 只接受經批准目錄嘅exact tag，回`jobId`；job係單一active下載，重複撳唔起第二個。`cancel_local_setup(jobId)` 只取消相應job；進度事件要以jobId配對，舊job唔可以覆蓋新job狀態。

- [ ] 寫探測fixture：OS不合、Apple Silicon／Intel、App存在但PATH冇CLI、服務未開、port被其他程式佔用、磁碟不足、模型已存在。
- [ ] 顯示實際OS／總記憶體／可用磁碟；RAM建議用report作起点，再以測試數據調整。未知硬件結果唔當足夠。
- [ ] 未安裝時開固定官方下載頁，保留`needs_install`；回到App後可重試偵測。唔以remote shell安裝腳本一口氣帶過授權。
- [ ] 已安裝未啟動時透過受控路徑開Ollama App，或經明確驗證嘅executable＋argv起服務；唔提供任意shell／模型控制執行。輪詢timeout後回`server_not_ready`。
- [ ] 已有Ollama服務就重用，唔重複start；識別服務但仍需確認model metadata，唔將任意localhost HTTP當可信Ollama。
- [ ] 每個下載layer按digest維護bytes；校驗階段可顯示「校驗中」，唔用假百分比。取消後重新開始同tag可利用官方pull續傳；未有可靠pause語意就只提供取消／重試。
- [ ] 使用A完成檢查，再沿Mail現有guard拒絕cloud／remote alias／非GGUF／不明context metadata。loopback無proxy／redirect限制保留。
- [ ] 用一組合成香港短信做非空、引用及內容測試；Qwen3.5若有thinking或回應格式差異，adapter明確處理並做contract fixture，唔靠字串trim掩蓋錯誤。
- [ ] 先測完成，先保存所選scope嘅model及mode；成功訊息只講已變更嘅用途。若保存失敗顯示「模型可用，但工作區未完成設定」，唔退回重新下載。
- [ ] OOM／timeout保留已下載模型；推薦較細模型或減少資料，唔自動切cloud、刪其他模型或改既有Ollama全局設定。

**Context契約：**目前Mail最多50封、正文／記憶有字元上限；本機檢查按UTF-8長度保守估計。硬件目錄建議4K時，必須先選少量短信令prompt可容納，保留來源ID及未讀提示。分批／摘要會改變事實準確度，另作P1；第一版唔為勉強成功而截掉否定句或超上限靜默continue。

**驗收矩陣：**8／16／24GB至少有代表性真機或明確標記未測配置；服務冷／熱啟動、下載中斷、取消、重開、同family唔同tag、失效metadata、本機斷網分析全部有證據。記錄模型digest、Ollama版本、context、峰值記憶體、首token／總時間同語意分數；未量度唔寫「秒回」。

## D. 電郵優先嘅首次設定

**現有檔案：**OnboardingWizard、App、EmailPage、Connections、Mail wrappers、[Mail commands](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/mail.rs)、[Mail sync](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/mail/sync.rs)。

- [ ] 第一步用B語言，再揀本機／進階自備帳戶；代管未通過G前唔顯示可購買卡。
- [ ] 只呈現所選provider必要欄位；API key不顯示原值，提供固定官方入口同單一儲存／測試動作。地區不可用時解釋並提供合適路徑，唔教VPN。
- [ ] 使用新Mail連接command，唔再以`googleConnect()`當核心Mail啟用；Assistant／Calendar權限留到使用時另連。
- [ ] 設定由後端實況推導；只持久化導引進度等非秘密資料，重開須重新核對可用性。已正常Gmail／GPT用戶直接顯示現況，唔要求再次登入。
- [ ] Google返回後提供首批同步；顯示實際成功數、部分失敗、下一頁狀態。唔假裝完成全信箱備份。
- [ ] 引導手動選2–5封，建立第一個群組並加一條整理指示；郵件加入群組唔更改Gmail標籤。
- [ ] 分析前顯示scope、provider／model、包含及未納入範圍。雲端傳送資訊及同意保留；本機不外傳。
- [ ] 分析完成要求有有效來源引用，展示「開原信核對」；空輸出／incomplete／無效來源均當失敗，唔以裝飾提示補成成功。
- [ ] 「略過」只跳步，唔把缺失功能標ready；回來繼續由缺失狀態開始。

首個有用結果事件契約建議：`setup_started`、`provider_verified`、`mail_connected`、`first_sync_completed`、`first_analysis_succeeded`、`first_source_opened`。只有明確披露及適當選擇後先傳telemetry；本機先記去識別統計亦可。event唔含郵件、完整地址、群組名或key。

**測試：**新profile、已有Gmail但無model、已有model但Gmail未連、只連Assistant Google、OAuth取消／timeout／公司阻擋、key無效、成功後重開、分析失敗重試。用既有native QA隔離帳戶證明Tauri IPC及DB真實運作，唔以browser mock代替。

## E. 全頁面、香港格式同結果可信度

**現有檔案：**[CalendarPage](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/CalendarPage.tsx)、[CronDashboard](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/CronDashboard.tsx)、[AssistantPanel](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/AssistantPanel.tsx)、[ResultWorkspace](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/ResultWorkspace.tsx)、[GitHubPage](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/GitHubPage.tsx)、[NotionPage](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/NotionPage.tsx)、voice／scheduler相關模組。

- [ ] 依copybook完成其餘既有頁面及更新／原生callback文案；唔因文案新增未完成動作。
- [ ] 抽出locale格式化；新增`workspace_timezone`時預設保留系統時區，讓用戶明確選`Asia/Hong_Kong`或其他IANA時區。
- [ ] 顯示時區設定同排程執行時區分開處理；先顯示實際現況，只有scheduler真正按指定timezone計算時先允許保存「公司時區排程」。
- [ ] 日期-only欄位保持日期語意，唔用UTC午夜轉成本地前一日；DST及跨午夜用香港／倫敦測。
- [ ] 明示來源貨幣；`$`無幣種唔猜。UI翻譯唔改原信／citation ID／用戶自訂名稱。
- [ ] 內部摘要可用廣東話，外發草稿可用英文／正式繁體中文；兩者分開偏好。Mail冇寄信工具，草稿文案唔寫「已寄出」。
- [ ] 保存分析列P1獨立DB migration：保存model、範圍、source ids、生成時間及context指紋；信件／指示／記憶改變標過時，唔沿用舊結果作最新承諾。

**語音獨立gate：**文字版唔被語音阻住。先驗本機Whisper load、Cantonese錄音、中英夾雜、macOS已安裝聲線、播放及停止。Gemini STT需要另一憑證同地區資格；有GPT key時唔報語音ready。CosyVoice／MiniMax未真正完成播放前，唔寫低成本完整語音可用。

**20組香港合成案例嘅標註要求：**每組列正確金額／幣種、已確認限期、暫定／否定／未知、最新版本及可引用來源。四種回覆語氣分開測；關鍵事實出錯就擋該model作預設，唔靠總平均分掩蓋。

## F. 驗收、公開發佈及付費pilot

先按變更執行相關測試，再跑現有CI層級。以下係日後實作驗證命令，今輪文件交付冇跑產品suite：

```sh
cd /Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant
npm run test:frontend
npm run build
cargo test --offline --manifest-path src-tauri/Cargo.toml --quiet
git diff --check
python3 native-mail-qa.py
```

離線Rust測試依賴已有cache；缺cache唔當regression。native QA要review當時launcher隔離策略、重新build；唔用舊binary或全部`--ignored`真實服務測試。需新測試入口時確保唔載入正式DB／Keychain／`.env`。

- [ ] 自動測試：scope隔離、憑證不洩漏、精確model匹配、下載終態、job取消、語言持久化、舊title相容、日期邊界。
- [ ] 真正原生：全新／已有profile、核心wizard、部分失敗重試、兩個合成信箱、來源記憶、重開、取消／恢復。
- [ ] 真實適用環境：香港網絡及公司帳戶按已授權範圍驗證供應商／region／模型／計費；單次HTTP成功唔代替條款資格。
- [ ] Google對照：readonly用途、OAuth品牌／網域／privacy／terms、公開驗證、雲端資料傳送assessment、撤權／保留／刪除。Testing唔包裝為可無限公開招募。
- [ ] 公司資料包：一頁資料流、安裝撤走、員工用法、供應商及支援範圍；現有SQLite保護限制如實交代。
- [ ] 5位新測試者中4位在AI服務已備妥時10分鐘內完成首個引用結果；模型下載與公司批准時間另報，唔從數據中隱藏。
- [ ] 付費pilot只喺技術及資料條件滿足後開始；3間公司每間一個流程；追蹤實際重用、人工核對時間、支援分鐘同續費，唔將一次性費用算MRR。
- [ ] 發行版本／簽署／公證／updater／DB相容性獨立核對；`scripts/release.sh`有外部副作用，唔作普通測試執行。

**停止擴招條件：**關鍵數字或限期錯誤斷言、未知來源引用、資料外傳目的地不符選擇、更新損壞資料、地區／公司政策未滿足。先修相應問題及重測；合成示範可繼續。

## G. 免 key 代管 AI：條件式第二階段

只有pilot顯示足夠付費需求、API開通係主要流失原因，而且香港供應商資格及資料政策可滿足，先另起呢個子項目。呢度係下一份服務設計嘅明確邊界，唔係部署指令或已完成設施。

最小資料流：`Mac → 已驗證JARVIS session → 限額檢查 → 已批准provider/model → 返回有引用結果`。Mail原信仍本機保存；後端收到分析所需payload，唔聲稱「郵件從不離機」。

| 元件 | 必要契約／驗收 |
|---|---|
| 身份 | JARVIS登入與Gmail OAuth分開；每請求驗短期token、subject、有效期；用戶A唔可查／消耗B額度 |
| Secret | provider key／AWS role只喺後端；桌面binary、前端、log均冇共用secret；能撤銷及輪換 |
| API | 每次job帶client request ID；同一ID重試唔重複扣額度；input／output大小及timeout有上限 |
| 資料 | provider目的地allowlist；日志只記去識別job、token、費率、錯誤碼；定義在途／暫存／刪除及供應商保留 |
| 計費 | 伺服器entitlement與月額度；先預留最大可能成本，完成後結算；失敗／重試規則清楚；跨並發唔超賣額度 |
| 品質 | provider專屬adapter及20組香港案例通過；唔自動跨供應商移交私人資料 |
| 退出 | 取消續費、停止雲端request、撤session；本機資料刪除與訂閱狀態分開，支援用戶匯出退出 |
| 維運 | 每戶／全服務成本上限、告警、緊急停用、服務中斷文案；credits只列減免，唔當永續零成本 |

唔喺第一版加S3郵件備份、團隊共同資料、向量資料庫、AgentCore或自建GPU集群，除非有獨立需求證據。先確認受支持嘅模型真正比本機／BYOK更易用，避免以大後端解決一個未驗證嘅定價問題。

## 文件及交接完成標準

- [ ] 更新產品使用指引時明示已實作／待實作／已實測日期；AGENTS舊V23及無frontend tests描述要跟實際變更核對，唔盲抄。
- [ ] 官網改動另喺`jarvis-official-website` repo處理，並對照真正能力；今輪只提供可用文案，冇改網站。
- [ ] 每個子項目交付時列檔案、測試證據、未驗證硬件／供應商、費率查核日期及剩餘限制。
- [ ] 完成標準係「用戶可以完成具體流程而有證據」，唔係只改按鈕文字、只保存設定、或只見模型下載完成。


## 2026-09-12 實作及驗收紀錄

用戶批准後已喺 `feat/hk-activation-local-ai` 開始落地；改動保留喺工作目錄，未 commit、push 或公開發佈。以下係第一輪可交付流程嘅實況；上文未勾選項目仍按原有分階段安排處理。

| 範圍 | 今輪已實作 | 驗證範圍／限制 |
|---|---|---|
| 視窗按鈕 | 紅色關閉、黃色最小化、綠色放大／還原；英文及香港繁體標籤 | 原生畫面確認次序及顏色，綠色放大／還原；紅／黃接既有 Tauri 動作。原有關閉係隱藏視窗，唔係退出背景 App |
| 設定入口 | 頂欄及電郵頁「設定導引」；選 AI、Gmail 唯讀連接／首批同步、選最多 5 封、群組指示、分析及原信核對 | 原生 QA 驗證頁面、導覽、合成郵件顯示、未有 AI 時禁止分析、略過後返回 Mail；冇重做正式 Gmail 授權 |
| AI 狀態 | 只讀 Mail 設定狀態、明確手動合成測試；日期／貨幣／金額／付款 boolean／引用精確核對 | 冇憑證會提示，唔顯示成功；未知 Keychain 讀取錯誤與未設定分開。測試成功狀態只喺今次程序有效，唔代表語音或 Assistant 已可用 |
| 安全套用 | 已測候選＋預期舊設定以原子 SQL 比對更新；有其他頁面改動就拒絕覆蓋 | Rust 測試覆蓋未驗證拒絕、舊設定衝突、成功保存；原有 GPT／AWS 模型欄位保留 |
| 本機 AI | 官方安裝入口、macOS 14／RAM／磁碟探測、啟動已安裝 Ollama、下載、合成測試、套用 Mail；可取消、精確 job ID 過濾 | 8／16／24+GB 分別建議 2B／4B／9B；已安裝 exact tag 略過下載；今輪冇啟動真實 Ollama 或下載模型，完整冷啟動／下載／語意表現仍需真機模型驗收 |
| 假完成修正 | 精確 tag、修正模型大小、只有 success 終態加模型存在核對先 complete；Qwen 小模型 context 上限 | 異常 EOF／錯誤／錯 tag／取消等測試；大型郵件群組會明示容量不足，冇靜默截去內容或自動切雲端 |
| 廣東話介面 | `ui_locale`、輕量字典、設定及工作區主要頁面、導引、按鈕、下載、更新文案；介面語言喺「外觀」 | 同回覆及 STT 分開；重開保存；原信、自訂標題及識別碼保留。供應商原始錯誤／部分工具動態描述仍可能係英文；未宣稱完整廣東話語音 |

### 最新自動測試

- `npm run test:frontend`：19 項通過，包括字典變量、預設／自訂標題、job 事件隔離、取消／失敗清理同啟用門檻。
- `npm run build`：TypeScript 同 Vite 通過。
- `cargo test --offline --manifest-path src-tauri/Cargo.toml --quiet`：369 項通過、0 失敗、7 ignored；真實服務項目未執行。
- `git diff --check`：通過。
- 最初 Rust fixture 遇到沙箱 loopback 權限限制；授權只容許隔離本機 HTTP fixture 後，全套通過。唔係供應商外部連接驗收。

### 原生 QA 證據

用 `python3 native-mail-qa.py` 重新 build `JARVIS Native QA`（debug-only、獨立 identifier、合成 120 封郵件、記憶體 Keychain），再經真正 macOS 視窗／React／Tauri IPC 操作。冇用 browser mock 冒充原生測試。

- 已見紅／黃／綠按鈕、英文→香港繁體切換、模型／RAM／磁碟顯示、三步導引及返回電郵。
- 合成環境冇 OpenAI key，測試後顯示缺少憑證；QA DB 仍係 `processing_mode=local`、`local_model` 空白、120 封、0 群組，`integrity_check=ok`。失敗測試冇改成 GPT。
- QA DB 保存 `ui_locale=zh-HK`；最新建置沿用同一隔離目錄重開後，原生畫面確認保留香港繁體及最新導覽文案。再次測試缺少 OpenAI 憑證，畫面仍顯示目前模型「未設定」；進入第二步後舊錯誤已清除。驗收後已退出 QA App。
- UI 實測捉到「失敗候選顯示為目前模型」，已改用實際儲存設定；切換步驟亦清除上一頁錯誤提示。
- 原生 QA build 有測試專用 Keychain helper dead-code warnings，冇建置失敗；bundle 只有 ad-hoc 簽署，未做公證。

### 尚未完成嘅階段

E 嘅完整時區偏好／結果持久化 migration／獨立語音驗收、F 嘅多款硬件及真實供應商測試／Google 公開驗證／公司試點／正式簽署公證，同 G 免 key 代管付費服務仍未完成。今輪冇覆蓋 `/Applications/JARVIS.app`，冇讀寫正式郵件庫或改現有 GPT key／Gmail 授權，亦冇改官網。

往後先補本機模型嘅合成 end-to-end 真機驗收及公司允許嘅雲端測試，再決定正式安裝／發行。單一 JSON 合成測試成功只係最低連接及格式門檻，唔代表 20 組香港案例或真實公司準確度已達標。

## 2026-09-12 後續：v0.3.0 發行準備

用戶其後明確要求測試、commit、push、deploy 同 release；上一節「未提交／未啟動本機模型」係當時快照，唔係後續狀態。功能提交 `b0854d7` 已推送 `feat/hk-activation-local-ai`，GitHub CI run `34716529668` 通過。詳細證據及之後發行狀態以根目錄 `NATIVE_MAIL_TEST_REPORT.md` 最新章節為準。

- [x] Rust 371 通過、frontend 19 通過、TypeScript／Vite build 通過。
- [x] 合成真實 GPT 測試、Gemini 模型供應檢查通過。
- [x] 隔離 Ollama 9B 經 production analysis 同 setup probe 兩項通過；已修正 Qwen thinking 輸出額度、單一 JSON fence 容錯及 probe citation 指示。
- [x] 2B 嘅引用失敗限制已如實加入 UI 及驗證報告，冇放寬正常引用檢查。
- [x] 真正原生「準備→測試→套用」已顯示成功；同一 QA 目錄重開保留 `qwen3.5:9b`／香港繁體，成功驗證狀態按程序重新檢查。
- [x] 原生測試發現 Gmail 斷線時導引錯誤停用本機副本分析；先重現回歸失敗，再移除即時連線門檻，保留 AI ready 同選信要求。
- [x] 六個版本 metadata 已統一為 0.3.0，繁體更新說明已準備；正式 DB／v0.2.0 App 已建立一致備份。
- [x] 用戶其後明確批准 DMG 上傳 Apple。公證 Accepted、DMG staple／Gatekeeper 通過，正式 App 原生啟動正常；S3、官網 v0.3.0 manifest、Git tag 同 GitHub Latest release 已完成，詳見 `NATIVE_MAIL_TEST_REPORT.md` 最新章節。

本輪下載只喺暫存模型目錄，同正式 GPT／Gmail 分開；冇轉換用戶現有工作區嘅處理方式。公司 pilot、完整 Google 公開驗證、香港網絡／多硬件測試同代管免 key 服務仍未完成，唔列為 v0.3.0 已交付能力。

公開發行於 2026-09-12T20:48:01Z 完成：[JARVIS v0.3.0](https://github.com/ChiFungHillmanChan/jarvis-ai-assistant/releases/tag/v0.3.0)。官網只提交 `public/latest.json`（`c472c1f`），CI/CD `34717975716` 通過，正式 manifest 與已驗證版本完全相符；官網其餘未提交改動保留。
