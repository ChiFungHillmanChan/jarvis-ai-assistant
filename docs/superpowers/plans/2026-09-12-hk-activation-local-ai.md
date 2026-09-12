# JARVIS 香港版啟用、本機 AI 及廣東話實作方案

> **最新文字規範（2026-09-12）：**使用者已改為要求繁體書面語。本文僅更新語體，原規劃日期、設計契約、功能狀態及測試結果均保留；後續介面、AI 中文回覆與交接以[現行文案手冊](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/product/2026-09-12-cantonese-copybook.md)為準，不以本紀錄代表後續版本已驗收或已發行。

> **後續執行 agent 指引：**按獨立工作逐項實作及審核，可使用 `superpowers:subagent-driven-development` 或 `superpowers:executing-plans`。2026-09-12 使用者已批准開始實作，並新增左上角視窗按鈕要求。未勾選的 checkbox 仍屬待辦；請依文件末段的實作紀錄，區分已完成、已驗證及外部依賴。

**目標：**透過一個設定入口，完成電郵工作區所需的 AI、Gmail 及首個附有來源的結果，提供無須 API key 的本機模式，並有完整香港繁體中文操作指引。

**架構：**保留 Assistant 與新 Mail 的儲存／憑證邊界，新增共用設定狀態及導引。第一版集中於現有 OpenAI／Bedrock／Ollama，按地區及實測結果選用；本機操作由 Rust 控制，前端只呈現狀態。JARVIS 代管 AI 是另一個設有先決條件的子項目，不納入第一版本機流程。

**技術：**Tauri v2、Rust／tokio、SQLite／refinery、React 18、TypeScript、Vite、現有 Node frontend tests。

**設計依據：**[產品及成本報告](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/2026-09-12-hk-launch-affordable-ai-report.md)、[逐頁繁體書面語文案](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/product/2026-09-12-cantonese-copybook.md)。所有擬議介面、型別及檔名均屬本方案契約，不代表 repo 已存在相應實作。

## 共通限制

- 基準為 2026-09-12 repo `main`／`6570acf`／`0.2.0`；開始實作前重新核對 status，保留現有改動。
- 最新 migration 檔案為 V24；只有確實需要 schema 變更時才新增 migration，開始前重新確認下一編號。不修改已套用的 SQL。
- 保留已選 provider／model／Gmail／Keychain；不因調整導引而自動切換既有使用者模式。
- 不輸出 `.env`、Keychain、OAuth callback 或真實郵件；測試使用合成資料及隔離 DB。
- Mail 連接繼續使用 `gmail.readonly`，不因統一 UI 而合併 token／資料表或擴大 scope。
- 本機文字分析不會自動 fallback 至雲端；Assistant 原有 fallback 須在設定中清楚交代，兩者並非同一機制。
- 原信、使用者自訂標題、品牌／模型 ID 保留原文；UI、文件及程式新增文案不使用 emoji。
- 成功儲存 key、HTTP endpoint 有回應、模型下載 EOF 或解析到音訊 bytes，均不等於功能已可用。
- UI 沿用現有暗底／cyan／字體；不額外重做產品視覺或無關整合。
- 真正一鍵啟動的目標是「Ollama 已完成 OS 安裝後」；首次安裝仍保留必要 OS 互動及下載等候。
- 收費、Google 公開驗證、政策確認、公司批准、公證／公開發佈各自有證據要求，本方案並不代表已取得所需許可。

## 工作拆分及次序

| 子項目 | 可獨立交付的結果 | 依賴 | 粗略工程估計，尚未排期 |
|---|---|---|---|
| A. 準確設定狀態及模型目錄 | 現有頁面不再誤報 ready，key 及模型均有驗證狀態 | 無 | 1–2 個專注工作日 |
| B. 核心廣東話 | shell、設定、Mail、Connections 及導引可切換語言 | A 的錯誤碼契約 | 2–3 日 |
| C. 本機啟動流程 | 無須 terminal，精確下載／測試／套用 | A | 2–4 日 |
| D. Mail 首次體驗 | 一個入口直達第一份附有引用的分析 | A、B；本機路徑依賴 C | 2–3 日 |
| E. 全頁面及香港格式 | 其餘頁面、時區、結果語氣及無障礙 | B | 2–4 日 |
| F. 發行及 pilot 驗收 | 乾淨安裝／重開／失敗恢復／真實適用地區證據 | 上述準備使用的子項目 | 2–3 日，外部審核另計 |
| G. 免 key 代管 AI | 有帳戶、額度、成本及資料契約的付費路徑 | pilot 證明 key 是主要阻力；供應商／政策通過 | 另作架構估算，不納入四週保證 |

四週是驗證節奏，不代表上表所有項目均須在四週內完成。每週只有 15–20 小時時，先交付 A、核心 B、C／D 最小完整流程，再由 pilot 決定 E 及 G 的深度。每完成一個可驗收結果，才擴大範圍。

## 追加：macOS 視窗按鈕

- [x] 左上角由左至右：關閉（紅色）、最小化（黃色）、放大／還原（綠色）。
- [ ] 保留真正 Tauri close／minimize／toggleMaximize 動作、無障礙標籤及非拖曳區域；在隔離原生 App 驗證次序及動作。

## A. 設定狀態及誤報成功修正

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

`get_setup_status()` 只讀取憑證是否存在、設定及上次驗證資料，不回傳憑證值，亦不自動發出收費 request。`test_ai_connection(scope: SetupScope)` 只在使用者按下「測試」後執行，使用不含郵件的簡短合成輸入；provider／model／region／憑證版本改變時，舊驗證即失效。上次成功不代表服務永久可用，須顯示時間，實際使用時仍要處理失敗。

- [ ] 先寫模型匹配測試：已安裝 `qwen2.5:7b` 時，對 `qwen2.5:14b` 必須回傳 false；只有 exact tag 或同一經核實 digest 才回傳 true。
- [ ] 移除 family substring 判定；修正 Llama 標示，候選目錄保存 tag、套件大小、量化、來源及最後核對日期；不自動下載新模型。
- [ ] 先寫下載 fixture：一段 progress 後 EOF、stream 內 error、HTTP 非 2xx、無效 JSON 及取消，全部不得 emit complete。
- [ ] 要求明確 `status: success`；完成後重新檢查 exact 模型。區分「下載成功」與「模型測試成功」，不改變下載層語意。
- [ ] 補充有 key 但沒有額度、401、429、未知模型及 timeout 測試；對外只回傳穩定錯誤 code，不回傳 raw response body。
- [ ] 設定頁顯示「已儲存，未測試」；同時列出 Mail／Assistant／STT／TTS 各自狀態，不以 GPT 成功暗示 Gemini STT 成功。

測試案例示意，供後續設計測試時參考：

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

**驗收：**可在現有 Settings 使用，無須等待 wizard 重做；測試網絡錯誤不改動正在使用的設定。現有系統檢查可能只依賴 PATH，須另外測試從 Finder 啟動時能否偵測已安裝的 Ollama App。

## B. 核心廣東話及集中翻譯

**現有檔案：**[App](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/App.tsx)、[Sidebar](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/Sidebar.tsx)、[workspace model](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/model.ts)、[Connections](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/ConnectionsPage.tsx)、Settings、OnboardingWizard、EmailPage。

**擬新增：**`/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/i18n/` 下的 `en.ts`、`zh-HK.ts`、`index.tsx`；分別負責字典／React context／格式化入口。先使用輕量 typed catalog，不為兩個 locale 引入大型翻譯後端。

- [ ] 新增 `ui_locale` preference，值只接受 `en`／`zh-HK`；現有 `language` 繼續用作 Assistant 輸出，不擅自改變其含義。
- [ ] 以 OS 建議初值並讓使用者選擇；已儲存的選擇優先。`document.documentElement.lang` 隨 UI 更新。
- [ ] 定義 key parity 及變量一致性測試；開發時遇到缺少翻譯須報錯，production 安全 fallback 須有明確規則，不顯示 raw key。
- [ ] 將 A 的錯誤 code 映射至字典；插值使用純文字，不將翻譯字串直接注入 HTML。
- [ ] 依 copybook 替換核心頁面的可見字串、ARIA、tooltip、空白狀態、載入／失敗、對話框及通知。
- [ ] 預設分頁標題儲存識別資料後，依 locale 呈現；使用者自訂標題保留原文。舊 rendered title 資料須先辨別系統預設，不以全文取代損害自訂名稱。
- [ ] 測試中文 IME `isComposing`，按 Enter 選字時不可誤送；保留鍵盤焦點及按鈕 disabled 狀態。

建議 frontend 測試檔：`/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/tests/i18n.test.ts`。至少檢查：相同 key 集、相同插值變量；重開後保存 locale；自訂 title 不翻譯；系統 title 可翻譯；`ui_locale` 不改變 `language`／provider。

**驗收：**以廣東話為主要語言的使用者可以完成核心工作；約 800px 的窄視窗及放大文字時，長錯誤訊息不遮擋主要按鈕。翻譯修改無須逐句編寫鏡像測試，驗證行為及缺少 key 即可。

## C. 本機 AI 啟動及模型準備

**現有檔案：**system_check、model_pull、LocalAiSetup、[ModelPullToast](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/ModelPullToast.tsx)、[mail analysis](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/mail/analysis.rs)、[capabilities](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/capabilities/default.json)。

**擬新增：**`/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/local_setup.rs`，只負責探測／啟動／job 狀態，重用現有下載與分析邏輯；不重寫一般 Assistant router。

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

`prepare_local_ai({ scope: "mail" | "assistant", model: string })` 只接受經批准目錄內的 exact tag，回傳 `jobId`；同一時間只容許一個 active 下載 job，重複按下不會啟動第二個。`cancel_local_setup(jobId)` 只取消相應 job；進度事件須以 jobId 配對，舊 job 不可覆蓋新 job 狀態。

- [ ] 編寫探測 fixture：OS 不符、Apple Silicon／Intel、App 存在但 PATH 沒有 CLI、服務未啟動、port 被其他程式佔用、磁碟不足及模型已存在。
- [ ] 顯示實際 OS／總記憶體／可用磁碟；RAM 建議以報告為起點，再依測試數據調整。未知硬件結果不視為資源充足。
- [ ] 未安裝時開啟固定官方下載頁，保留 `needs_install`；回到 App 後可重新偵測。不以 remote shell 安裝腳本略過授權。
- [ ] 已安裝但未啟動時，透過受控路徑開啟 Ollama App，或經明確驗證的 executable＋argv 啟動服務；不提供任意 shell／模型控制執行。輪詢 timeout 後回傳 `server_not_ready`。
- [ ] 已有 Ollama 服務則重用，不重複 start；識別服務後仍須確認 model metadata，不將任意 localhost HTTP 視為可信 Ollama。
- [ ] 每個下載 layer 按 digest 維護 bytes；校驗階段可顯示「校驗中」，不使用虛構百分比。取消後以相同 tag 重新開始，可利用官方 pull 續傳；未有可靠 pause 語意時只提供取消／重試。
- [ ] 使用 A 的完成檢查，再沿用 Mail 現有 guard 拒絕 cloud／remote alias／非 GGUF／不明 context metadata。保留 loopback 不允許 proxy／redirect 的限制。
- [ ] 以一組合成香港短郵件進行非空、引用及內容測試；Qwen3.5 若有 thinking 或回應格式差異，adapter 須明確處理並建立 contract fixture，不以字串 trim 掩蓋錯誤。
- [ ] 測試完成後，才保存所選 scope 的 model 及 mode；成功訊息只說明已變更的用途。保存失敗時顯示「模型可用，但工作區尚未完成設定」，不返回重新下載。
- [ ] OOM／timeout 時保留已下載模型；建議較小模型或減少資料，不自動切換 cloud、刪除其他模型或修改既有 Ollama 全局設定。

**Context 契約：**目前 Mail 最多 50 封，正文／記憶設有字元上限；本機檢查按 UTF-8 長度保守估計。硬件目錄建議 4K 時，須先選少量短郵件，使 prompt 可容納，並保留來源 ID 及未讀提示。分批／摘要會影響事實準確度，另列為 P1；第一版不為勉強成功而截去否定句，或在超出上限時靜默 continue。

**驗收矩陣：**8／16／24GB 至少有代表性真機，或明確標記尚未測試的配置；服務冷／熱啟動、下載中斷、取消、重開、同一 family 的不同 tag、失效 metadata 及本機斷網分析，均須有證據。記錄模型 digest、Ollama 版本、context、峰值記憶體、首 token／總時間及語意分數；未量度前不聲稱「秒回」。

## D. 電郵優先的首次設定

**現有檔案：**OnboardingWizard、App、EmailPage、Connections、Mail wrappers、[Mail commands](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/mail.rs)、[Mail sync](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/mail/sync.rs)。

- [ ] 第一步使用 B 的語言設定，再選擇本機／進階自備帳戶；代管通過 G 前不顯示可購買卡片。
- [ ] 只呈現所選 provider 的必要欄位；API key 不顯示原值，提供固定官方入口及單一儲存／測試動作。地區不可用時說明原因並提供合適路徑，不教授 VPN 繞過方法。
- [ ] 使用新 Mail 連接 command，不再以 `googleConnect()` 作為核心 Mail 啟用；Assistant／Calendar 權限保留至實際使用時另行連接。
- [ ] 設定由後端實況推導；只持久化導引進度等非秘密資料，重開時須重新核對可用性。已有正常 Gmail／GPT 的使用者直接查看現況，無須再次登入。
- [ ] 從 Google 返回後提供首批同步；顯示實際成功數、部分失敗及下一頁狀態，不宣稱已完成全信箱備份。
- [ ] 引導手動選擇 2–5 封郵件，建立第一個群組並新增一條整理指示；將郵件加入群組不會更改 Gmail 標籤。
- [ ] 分析前顯示 scope、provider／model、包含及未納入的範圍。保留雲端傳送資訊及同意流程；本機不外傳。
- [ ] 分析完成須有有效來源引用，展示「開啟原信核對」；空輸出／incomplete／無效來源均視為失敗，不以裝飾提示補成成功。
- [ ] 「略過」只跳過該步驟，不將缺少的功能標記為 ready；返回後仍由缺少設定的狀態開始。

首個有用結果的事件契約建議：`setup_started`、`provider_verified`、`mail_connected`、`first_sync_completed`、`first_analysis_succeeded`、`first_source_opened`。只有明確披露並提供適當選擇後，才傳送 telemetry；亦可先在本機記錄去識別統計。event 不包含郵件、完整地址、群組名稱或 key。

**測試：**新 profile、已有 Gmail 但沒有 model、已有 model 但 Gmail 未連接、只連接 Assistant Google、OAuth 取消／timeout／公司阻擋、key 無效、成功後重開、分析失敗重試。使用既有 native QA 隔離帳戶，證明 Tauri IPC 及 DB 真正運作，不以 browser mock 取代。

## E. 全頁面、香港格式及結果可信度

**現有檔案：**[CalendarPage](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/CalendarPage.tsx)、[CronDashboard](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/CronDashboard.tsx)、[AssistantPanel](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/AssistantPanel.tsx)、[ResultWorkspace](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/workspace/ResultWorkspace.tsx)、[GitHubPage](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/GitHubPage.tsx)、[NotionPage](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/NotionPage.tsx)、voice／scheduler相關模組。

- [ ] 依 copybook 完成其餘既有頁面及更新／原生 callback 文案；不因文案而新增尚未完成的動作。
- [ ] 抽出 locale 格式化；新增 `workspace_timezone` 時，預設保留系統時區，讓使用者明確選擇 `Asia/Hong_Kong` 或其他 IANA 時區。
- [ ] 顯示時區設定與排程執行時區分開處理；先顯示實際現況，只有 scheduler 真正按指定 timezone 計算時，才容許保存「公司時區排程」。
- [ ] 日期-only 欄位保持日期語意，不以 UTC 午夜轉換成本地前一日；DST 及跨午夜使用香港／倫敦測試。
- [ ] 明示來源貨幣；`$` 沒有幣種時不作猜測。UI 翻譯不改動原信／citation ID／使用者自訂名稱。
- [ ] 原方案允許內部摘要使用廣東話，外發草稿使用英文／正式繁體中文，並分開設定偏好；後續中文輸出依最新書面語規範。Mail 沒有寄信工具，草稿文案不可聲稱「已寄出」。
- [ ] 保存分析列為 P1 獨立 DB migration：保存 model、範圍、source ids、生成時間及 context 指紋；信件／指示／記憶改變時標記為過時，不沿用舊結果作為最新承諾。

**語音獨立 gate：**文字版不受語音進度阻擋。先驗證本機 Whisper load、Cantonese 錄音、中英夾雜、macOS 已安裝聲線、播放及停止。Gemini STT 需要另一憑證及地區資格；有 GPT key 時不可將語音標記為 ready。CosyVoice／MiniMax 真正完成播放前，不宣稱低成本完整語音可用。

**20 組香港合成案例的標註要求：**每組列出正確金額／幣種、已確認限期、暫定／否定／未知、最新版本及可引用來源。四種回覆語氣分開測試；關鍵事實出錯即禁止將該 model 設為預設，不以總平均分掩蓋。

## F. 驗收、公開發佈及付費pilot

先按變更執行相關測試，再執行現有 CI 層級。以下是後續實作驗證命令；原文件交付當時沒有執行產品 suite：

```sh
cd /Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant
npm run test:frontend
npm run build
cargo test --offline --manifest-path src-tauri/Cargo.toml --quiet
git diff --check
python3 native-mail-qa.py
```

離線 Rust 測試依賴已有 cache；缺少 cache 不視為 regression。native QA 須 review 當時 launcher 的隔離策略並重新 build；不使用舊 binary，亦不執行全部 `--ignored` 真實服務測試。需要新測試入口時，確保不載入正式 DB／Keychain／`.env`。

- [ ] 自動測試：scope 隔離、憑證不洩漏、精確 model 匹配、下載終態、job 取消、語言持久化、舊 title 相容及日期邊界。
- [ ] 真正原生：全新／已有 profile、核心 wizard、部分失敗重試、兩個合成信箱、來源記憶、重開、取消／恢復。
- [ ] 真實適用環境：香港網絡及公司帳戶按已授權範圍驗證供應商／region／模型／計費；單次 HTTP 成功不能取代條款資格。
- [ ] Google 對照：readonly 用途、OAuth 品牌／網域／privacy／terms、公開驗證、雲端資料傳送 assessment、撤權／保留／刪除。Testing 不包裝為可無限公開招募。
- [ ] 公司資料包：一頁資料流、安裝及移除、員工用法、供應商及支援範圍；如實交代現有 SQLite 保護限制。
- [ ] 5 位新測試者中有 4 位，在 AI 服務已備妥時於 10 分鐘內完成首個引用結果；模型下載與公司批准時間另行報告，不從數據中隱藏。
- [ ] 付費 pilot 只在技術及資料條件滿足後開始；3 間公司每間一個流程；追蹤實際重用、人工核對時間、支援分鐘及續費，不將一次性費用計入 MRR。
- [ ] 發行版本／簽署／公證／updater／DB 相容性獨立核對；`scripts/release.sh` 有外部副作用，不作普通測試執行。

**停止擴招條件：**關鍵數字或限期錯誤斷言、未知來源引用、資料外傳目的地不符選擇、更新損壞資料、地區／公司政策未滿足。先修相應問題及重測；合成示範可繼續。

## G. 免 key 代管 AI：條件式第二階段

只有 pilot 顯示足夠付費需求、API 開通是主要流失原因，而且可滿足香港供應商資格及資料政策時，才另外啟動此子項目。本節界定下一份服務設計的範圍，並非部署指令或已完成設施。

最小資料流：`Mac → 已驗證JARVIS session → 限額檢查 → 已批准provider/model → 返回有引用結果`。Mail 原信仍在本機保存；後端收到分析所需 payload，不宣稱「郵件從不離機」。

| 元件 | 必要契約／驗收 |
|---|---|
| 身份 | JARVIS 登入與 Gmail OAuth 分開；每次請求驗證短期 token、subject 及有效期；使用者 A 不可查詢／消耗 B 的額度 |
| Secret | provider key／AWS role 只保存在後端；桌面 binary、前端及 log 均不含共用 secret；可撤銷及輪換 |
| API | 每次 job 帶 client request ID；同一 ID 重試不重複扣除額度；input／output 大小及 timeout 設有上限 |
| 資料 | provider 目的地 allowlist；日誌只記錄去識別 job、token、費率及錯誤碼；定義在途／暫存／刪除及供應商保留 |
| 計費 | 伺服器 entitlement 與月額度；先預留最大可能成本，完成後結算；失敗／重試規則清楚；跨並發不超賣額度 |
| 品質 | provider 專屬 adapter 及 20 組香港案例通過；不自動跨供應商移交私人資料 |
| 退出 | 取消續費、停止雲端 request、撤銷 session；本機資料刪除與訂閱狀態分開，支援使用者匯出及退出 |
| 維運 | 每戶／全服務成本上限、告警、緊急停用及服務中斷文案；credits 只列為減免，不視為永續零成本 |

第一版不新增 S3 郵件備份、團隊共同資料、向量資料庫、AgentCore 或自建 GPU 集群，除非有獨立需求證據。先確認受支援的模型確實比本機／BYOK 更易用，避免以大型後端解決尚未驗證的定價問題。

## 文件及交接完成標準

- [ ] 更新產品使用指引時，明示已實作／待實作／已實測日期；AGENTS 舊 V23 及沒有 frontend tests 的描述須與實際變更核對，不直接照抄。
- [ ] 官網改動另於 `jarvis-official-website` repo 處理，並對照實際能力；原文件交付只提供可用文案，沒有修改網站。
- [ ] 每個子項目交付時，列出檔案、測試證據、尚未驗證的硬件／供應商、費率查核日期及剩餘限制。
- [ ] 完成標準是「使用者可以完成具體流程，並有證據」，不能僅修改按鈕文字、保存設定或看見模型下載完成。


## 2026-09-12 實作及驗收紀錄

使用者批准後，已在 `feat/hk-activation-local-ai` 開始實作；當時改動保留在工作目錄，尚未 commit、push 或公開發佈。以下是第一輪可交付流程的實況；上文未勾選項目仍按原有分階段安排處理。

| 範圍 | 當時已實作 | 驗證範圍／限制 |
|---|---|---|
| 視窗按鈕 | 紅色關閉、黃色最小化、綠色放大／還原；英文及香港繁體標籤 | 原生畫面確認次序、顏色及綠色放大／還原；紅／黃連接既有 Tauri 動作。原有關閉會隱藏視窗，不會退出背景 App |
| 設定入口 | 頂欄及電郵頁「設定導引」；選擇 AI、Gmail 唯讀連接／首批同步、選擇最多 5 封、群組指示、分析及原信核對 | 原生 QA 驗證頁面、導覽、合成郵件顯示、沒有 AI 時禁止分析、略過後返回 Mail；沒有重做正式 Gmail 授權 |
| AI 狀態 | 只讀 Mail 設定狀態、明確手動合成測試；日期／貨幣／金額／付款 boolean／引用精確核對 | 沒有憑證時會提示，不顯示成功；未知 Keychain 讀取錯誤與未設定分開。測試成功狀態只在當次程序有效，不代表語音或 Assistant 已可用 |
| 安全套用 | 已測候選與預期舊設定以原子 SQL 比對更新；其他頁面有改動時拒絕覆蓋 | Rust 測試覆蓋未驗證拒絕、舊設定衝突及成功保存；保留原有 GPT／AWS 模型欄位 |
| 本機 AI | 官方安裝入口、macOS 14／RAM／磁碟探測、啟動已安裝 Ollama、下載、合成測試、套用 Mail；可取消、精確 job ID 過濾 | 8／16／24+GB 分別建議 2B／4B／9B；已安裝 exact tag 時略過下載；當時沒有啟動真實 Ollama 或下載模型，完整冷啟動／下載／語意表現仍需真機模型驗收 |
| 誤報完成修正 | 精確 tag、修正模型大小、只有 success 終態加模型存在核對才 complete；Qwen 小模型 context 上限 | 異常 EOF／錯誤／錯 tag／取消等測試；大型郵件群組會明示容量不足，沒有靜默截去內容或自動切換雲端 |
| 廣東話介面 | `ui_locale`、輕量字典、設定及工作區主要頁面、導引、按鈕、下載、更新文案；介面語言設於「外觀」 | 與回覆及 STT 分開；重開後保存；原信、自訂標題及識別碼保留。供應商原始錯誤／部分工具動態描述仍可能是英文；未宣稱完整廣東話語音 |

### 最新自動測試

- `npm run test:frontend`：19 項通過，包括字典變量、預設／自訂標題、job 事件隔離、取消／失敗清理及啟用門檻。
- `npm run build`：TypeScript 及 Vite 通過。
- `cargo test --offline --manifest-path src-tauri/Cargo.toml --quiet`：369 項通過、0 失敗、7 ignored；真實服務項目未執行。
- `git diff --check`：通過。
- 最初 Rust fixture 遇到沙箱 loopback 權限限制；授權只容許隔離本機 HTTP fixture 後，全套通過。此結果並非供應商外部連接驗收。

### 原生 QA 證據

使用 `python3 native-mail-qa.py` 重新 build `JARVIS Native QA`（debug-only、獨立 identifier、合成 120 封郵件、記憶體 Keychain），再透過真正 macOS 視窗／React／Tauri IPC 操作。沒有使用 browser mock 代替原生測試。

- 已確認紅／黃／綠按鈕、英文→香港繁體切換、模型／RAM／磁碟顯示、三步導引及返回電郵。
- 合成環境沒有 OpenAI key，測試後顯示缺少憑證；QA DB 仍為 `processing_mode=local`、`local_model` 空白、120 封、0 群組，`integrity_check=ok`。失敗測試沒有改為 GPT。
- QA DB 保存 `ui_locale=zh-HK`；當時最新建置沿用同一隔離目錄重開後，原生畫面確認保留香港繁體及最新導覽文案。再次測試缺少 OpenAI 憑證，畫面仍顯示目前模型「未設定」；進入第二步後舊錯誤已清除。驗收後已退出 QA App。
- UI 實測發現「失敗候選顯示為目前模型」，已改用實際儲存設定；切換步驟亦會清除上一頁錯誤提示。
- 原生 QA build 有測試專用 Keychain helper dead-code warnings，沒有建置失敗；bundle 只有 ad-hoc 簽署，尚未公證。

### 當時尚未完成的階段

E 的完整時區偏好／結果持久化 migration／獨立語音驗收、F 的多款硬件及真實供應商測試／Google 公開驗證／公司試點／正式簽署公證，以及 G 免 key 代管付費服務，當時仍未完成。該輪沒有覆蓋 `/Applications/JARVIS.app`、讀寫正式郵件庫或改動現有 GPT key／Gmail 授權，亦沒有修改官網。

後續先補充本機模型的合成 end-to-end 真機驗收及公司允許的雲端測試，再決定正式安裝／發行。單一 JSON 合成測試成功只是最低連接及格式門檻，不代表 20 組香港案例或真實公司準確度已達標。

## 2026-09-12 後續：v0.3.0 發行準備

使用者其後明確要求測試、commit、push、deploy 及 release；上一節「未提交／未啟動本機模型」是當時快照，並非後續狀態。功能提交 `b0854d7` 已推送至 `feat/hk-activation-local-ai`，GitHub CI run `34716529668` 通過。詳細證據及後續發行狀態以根目錄 `NATIVE_MAIL_TEST_REPORT.md` 最新章節為準。

- [x] Rust 371 通過、frontend 19 通過、TypeScript／Vite build 通過。
- [x] 合成真實 GPT 測試、Gemini 模型供應檢查通過。
- [x] 隔離 Ollama 9B 通過 production analysis 及 setup probe 兩項；已修正 Qwen thinking 輸出額度、單一 JSON fence 容錯及 probe citation 指示。
- [x] 2B 的引用失敗限制已如實加入 UI 及驗證報告，沒有放寬正常引用檢查。
- [x] 真正原生「準備→測試→套用」已顯示成功；同一 QA 目錄重開後保留 `qwen3.5:9b`／香港繁體，成功驗證狀態按程序重新檢查。
- [x] 原生測試發現 Gmail 斷線時，導引錯誤停用本機副本分析；先重現回歸失敗，再移除即時連線門檻，保留 AI ready 及選信要求。
- [x] 六個版本 metadata 已統一為 0.3.0，繁體更新說明已準備；正式 DB／v0.2.0 App 已建立一致備份。
- [x] 使用者其後明確批准將 DMG 上傳至 Apple。公證 Accepted、DMG staple／Gatekeeper 通過，正式 App 原生啟動正常；S3、官網 v0.3.0 manifest、Git tag 及 GitHub Latest release 已完成，詳見 `NATIVE_MAIL_TEST_REPORT.md` 最新章節。

該輪下載只使用暫存模型目錄，與正式 GPT／Gmail 分開；沒有轉換使用者現有工作區的處理方式。公司 pilot、完整 Google 公開驗證、香港網絡／多硬件測試及代管免 key 服務仍未完成，不列為 v0.3.0 已交付能力。

公開發行於 2026-09-12T20:48:01Z 完成：[JARVIS v0.3.0](https://github.com/ChiFungHillmanChan/jarvis-ai-assistant/releases/tag/v0.3.0)。官網只提交 `public/latest.json`（`c472c1f`），CI/CD `34717975716` 通過，正式 manifest 與已驗證版本完全相符；官網其餘未提交改動保留。
