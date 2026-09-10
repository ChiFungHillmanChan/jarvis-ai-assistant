# 原生電郵工作區驗收紀錄

日期：2026-09-10。測試版：JARVIS v0.1.3，`feat/local-email-workspace` 工作目錄。

呢輪使用真正打包嘅 macOS `.app`、`tauri://localhost`、正式 React 頁面、Tauri IPC 同 Rust/SQLite 實作。冇替換 `invoke`，亦冇用瀏覽器假後端。先用兩個合成信箱／120 封合成郵件做隔離 QA，再按用戶授權，以一個真實 Gmail 同 OpenAI API 做正式安裝驗收；呢啲係功能測試，唔係客戶市場驗證。

最新自動測試：**345 項 Rust 測試通過、0 失敗、7 項 ignored**；另外明確執行嘅真實 OpenAI 測試 **1 項通過**。`npm run build`、原生 debug `.app` 打包及 `git diff --check` 通過。前一輪原生 App 完成兩次啟動／正常退出，第二次包括重啟持久化同修正後 HUD 驗證。

## 隔離方法

執行 `python3 native-mail-qa.py` 會編譯 **JARVIS Native QA**，App ID 係 `com.jarvis.nativeqa`。只可用 debug build；資料存喺有標記嘅系統臨時子目錄。登入憑證只放程序記憶體，唔讀 macOS login Keychain；唔載入 `.env`，launcher 過濾繼承環境變數，Google 登入停用。測試時唔會新增真實 Gmail 授權或者 AWS 推論費用。

第一次啟動建立兩個已斷開嘅測試信箱，各有 60 封郵件。正常本機副本功能仍然可用。重用 launcher 印出嘅目錄：

```sh
python3 native-mail-qa.py --reuse '/完整測試目錄' --no-build
```

唔好直接雙擊 QA `.app`：佢需要 launcher 提供隔離目錄。修改程式後應移除 `--no-build` 重新編譯。

## 已執行嘅原生操作

| 操作 | 已觀察結果 |
|---|---|
| 載入本機郵件 | 兩個信箱共 120 封，首頁顯示 1–50 / 120 |
| 原生分頁 | 依次顯示 1–50、51–100、101–120；最後一頁不能再往下 |
| 建群組及廣東話指示 | 原生表單儲存成功，SQLite 內容完全吻合 |
| 跨信箱加入同一群組 | 工作信箱 #120 同私人信箱 #119 同組，群組共兩封 |
| 記憶來源驗證 | #118 唔屬於群組而被拒；改成 #120 後成功，失敗嘅提交冇留下多餘記憶 |
| 來源閱讀 | 開到 #120 正文；郵件內 `<script>` 顯示為純文字 |
| 本文搜尋 | `native-body-119` 搜到私人信箱唯一一封 |
| 帳戶／群組／搜尋交集 | 再揀工作信箱後顯示零封，群組總數仍然係兩封 |
| 未設定模型 | 顯示模型名稱錯誤，UI 可以繼續操作 |
| 本機 Ollama 不可用 | 顯示「未有傳送郵件內容」嘅 preflight 錯誤 |
| AWS 同意 | 未勾同意時，macOS WebKit 表單阻止儲存；SQLite 仍為 local 模式 |
| 完全退出 | Cmd+Q 正常結束，程序 exit code 0；群組、來源記憶同設定已寫入磁碟 |
| 重開同一資料庫 | 群組、廣東話指示、#119/#120 成員、來源記憶同 local 模型設定保留 |
| 刪除測試工作信箱 | 只剩私人信箱 60 封；#120 來源記憶刪除，無來源嘅自行確認記憶保留，群組仍有 #119 |
| 刪除群組 | 群組／成員／記憶清除，私人信箱 60 封仍然保留；再以 SQLite 斷言確認 |
| 全域 HUD 錯誤收尾 | 原先無 API key 會卡住 Thinking；修正後重啟，同樣服務失敗但 HUD 正常消失 |
| SQLite 完整性 | `integrity_check = ok`，`foreign_key_check` 零違規 |

## 自動測試

儲存模組 21 項通過，包括真實檔案 SQLite 測試：V21 升最新 schema、V22 升 V23、OpenAI 設定保存／重開、舊 IPC 相容、關閉重開、120 封分頁與搜尋、帳戶隔離、交易 rollback、記憶來源／到期／上限及廣東話長度。分析模組最新 35 項通過、2 項 ignored。較早嘅 Ollama HTTP 測試中，8 個測試涵蓋 24 個傳輸情境；使用真正 reqwest 配臨時 loopback server，驗證 tags/show/chat 次序、模型 metadata、容量、redirect、錯誤回覆及唔傳郵件嘅拒絕路徑。

新增測試先重現一個 bug：空白模型回覆加 `done_reason=length` 會被截斷提示包裝成成功。修正後，空白回覆正確返回失敗。

原生畫面另外揭露全域 AI 狀態列卡住嘅 bug：provider chain 最後失敗仍顯示重試，冇發出 idle。修正後只喺確實仲有下一個 provider 時顯示重試；終止錯誤同無語音播放嘅完成會清除 HUD，成功串流語音仍等音訊播完。3 個新增回歸測試先重現其中兩個缺 idle 錯誤，再全部通過；原生重開亦確認修復。

重跑自動測試：

```sh
cargo test --offline --manifest-path src-tauri/Cargo.toml --quiet
npm run build
git diff --check
```

HTTP 測試需要准許 bind 本機臨時連接埠；唔需要外部網絡。正常測試冇包含 `--ignored`，唔會刻意運行真實 Keychain／服務整合測試。

## GPT 接駁及真實模型驗證

用戶要求改用 GPT 後，新增獨立 OpenAI 分析路徑、介面選項及 V23 schema。原有選擇保留，唔會因升級自動轉雲端。OpenAI 使用官方 Responses API，指定 `store: false`、無工具、無供應商 fallback、禁止 redirects，最多 4,096 output tokens。一般測試唔載入真 API key。

新增 OpenAI HTTP 測試先重現 7 項失敗，修正後驗證 request schema／Authorization、多段文字、redirect、HTTP 失敗、未完成回覆、refusal、無效 JSON、空白回覆、缺 key、模型名稱及共用引用檢查。Keychain 讀取選擇另外 3 項測試通過，確認空環境設定唔會遮蓋已儲存 key、有效環境 key 唔會多讀 Keychain、錯誤唔洩漏憑證或原始細節。

項目已設定嘅 key 向 OpenAI 查模型清單返回 HTTP 200；`gpt-5.6-luna` 可用。明確執行 `real_openai_analyzes_synthetic_mail_through_production_entry_point`，透過正式 `analyze()` 同官方 endpoint，**1 項通過、4.91 秒**。兩封合成郵件／一條來源記憶嘅人工核對：

| 要求 | GPT 結果 |
|---|---|
| 2026-09-11 下午三點前回覆 | 正確 |
| 預算 HK$1,000 | 正確 |
| 只確認報價，毋須付款 | 正確 |
| 記憶指出毋須開會 | 正確，下一步採電郵回覆 |
| 來源及完整性 | 只用 #101／#102，未納入 0 封，completed 回覆 |

呢個係功能及單一案例質素驗證，唔代表所有郵件分析都會正確。GPT 回覆偏書面繁體中文；用戶群組指示可再要求口語廣東話。

轉用 GPT 前，曾以真實 Ollama 跑同一合成案例：0.5b 模型有錯誤付款理解及虛構引用，7b 案例內容正確。呢個發現促使加入所有模式共用嘅引用驗證：拒絕缺漏、未知編號、非正數、溢位及格式錯誤，仍容許有效舊記憶來源。引用驗證只確認格式同來源成員，唔能夠證明摘要事實正確。按用戶最新要求已停止 Ollama，並移除今次加到正式模型目錄嘅測試模型；原有 Ollama App 保留。

## 已安裝舊版嘅崩潰

macOS 報告 `jarvis-2026-09-10-192007.ips` 同 `jarvis-2026-09-10-192010.ips` 指向 `/Applications/JARVIS.app` v0.1.1，唔係新 QA bundle。已安裝 binary 包含 V1–V16 migration，但正式資料庫 migration metadata 已到 V19。舊版 `Database::new().expect(...)` 會將缺失 migration 錯誤變成啟動 panic，與報告堆疊一致。系統冇保留原始 Rust panic 文字，所以呢個係由版本、migration 同程式碼推導，唔係直接抄到錯誤訊息。

已將 `/Applications/JARVIS.app` 更新至 **v0.1.3**。本機 release build 成功，以 Developer ID 簽署；安裝後 `codesign --verify --deep --strict --verbose=2` 通過，binary SHA-256 同本次 build 一致。未做 Apple 公證或公開發佈。

更新前已用 SQLite backup 建立正式資料庫一致副本（schema V19、integrity check 通過），並保留舊 App。備份位置：`~/Library/Application Support/jarvis/backups/before-mail-workspace-20260910T185259Z`，包含 `jarvis.db`、`JARVIS-v0.1.1.app` 及更新時移走嘅 `JARVIS-original-install.app`。呢個係原狀備份，唔代表原本不相容嘅舊 App 可以正常啟動；唔應單獨降級 App。

首次嘗試啟動遇到 Mac 鎖定，用戶解鎖後已完成正式原生驗收：

| 正式安裝測試 | 結果 |
|---|---|
| 開啟 `/Applications/JARVIS.app` | 真正 `tauri://localhost` 桌面視窗正常載入，未再遇到啟動崩潰 |
| 正式 DB 升級 | V19 → V23 成功；正常只讀 SQLite 連線包含 WAL，`integrity_check=ok`，外鍵零違規 |
| GPT 同意表單 | 未勾同意按儲存，WebKit 顯示 `Select this checkbox` 並阻止提交 |
| 儲存設定 | 勾選後成功儲存 `openai / gpt-5.6-luna` |
| 完全退出 | Cmd+Q 後確認 JARVIS 程序已結束 |
| 重開持久化 | 再開正式 App，設定仍為 OpenAI GPT／gpt-5.6-luna；SQLite 與原生表單一致 |
| 全域 HUD | 啟動時短暫重試後消失，未卡住 Thinking |

用戶已親自完成 Gmail OAuth／讀取授權，並自行喺設定保存 OpenAI API key；真實服務驗收結果見下節。

將項目 API key 永久存到 Keychain 並授權 JARVIS 讀取嘅動作被自動審批拒絕，原因係需要用戶明確授權憑證儲存及讀取權限。拒絕前命令未執行；之後只執行分開嘅 App 安裝。用戶選擇自行喺 JARVIS 設定填入，之後已確認 Keychain 存在 OpenAI 項目（只查存在與否，冇讀取密鑰內容）。Agent 冇匯入 API key，安裝包亦冇嵌入密鑰。先前真實 GPT 測試只喺測試子程序使用項目現有憑證。

## 未驗證範圍

第二個真實 Gmail 同時連接、大量歷史郵件完整遍歷、Google quota／撤權／長時間 token 到期恢復、AWS Bedrock 憑證／IAM／計費、Apple 公證及公開發佈仍未實測。跨信箱分組已用兩個合成帳戶驗證，唔等同兩個真實帳戶嘅 OAuth 驗收。

## Google 測試版授權提示

真實登入出現「Google hasn’t verified this app」。已核對瀏覽器 OAuth client 同本項目設定一致；新電郵連接程式只要求 `gmail.readonly`。Google 說明 Testing 用戶會見到提醒，需親自決定繼續授權；Testing 的相關授權及 refresh token 會喺七日後到期。[Google 官方說明](https://support.google.com/cloud/answer/15549945?hl=en)

呢個提示唔係 JARVIS 啟動崩潰；公開推出前仍需處理 Google OAuth 驗證要求。未改 Google Cloud 發佈狀態，亦冇代用戶接受警告或最終授權。

## 真實 Gmail → 本機群組 → GPT 原生驗收

用戶親自完成 Google 警告及授權，Agent 冇代按最終同意。先前一輪登入等候逾時，重新由 App 發起授權後成功。正式帳戶只作 metadata 統計，測試報告唔保存郵件正文、帳戶地址、API key 或 OAuth token。

| 測試 | 已觀察結果 |
|---|---|
| Gmail OAuth | 原生 App 顯示帳戶已連接 |
| 首次同步 | 一次儲存 50 封，提示仍有較舊郵件；唔係完整信箱備份 |
| 本機搜尋及閱讀 | 相關業務郵件可搜尋、可讀；測試兩封正文分別 4,021／2,734 字元 |
| 真實群組 | 建立「Gold Badge 跟進」，加入兩封相關業務郵件（本機 #1／#5） |
| 個人指示 | 保存廣東話分類指示，要求分清已確認／待回覆、引用來源及唔虛構確實日期 |
| 來源記憶 | 核對原信後保存一條 active 事實記憶，來源 #5，無到期日 |
| 真正 GPT IPC | 原生按「分析呢個群組」，顯示 `OpenAI GPT / gpt-5.6-luna`；未納入 0 封，來源 #1／#5 |
| 摘要重點核對 | 正確保留暫定日期嘅不確定性、評核形式，並將未獲答覆嘅用戶問題列為待確認 |
| 完全退出及重開 | 程序確實結束再啟動；50 封、兩個成員、指示及來源記憶全部保留 |
| 重啟後重新同步 | 毋須重新 Google 登入；再次同步成功，總數仍為 50，重複 provider ID 零個；group membership／來源記憶不變 |
| 重啟後 GPT | 用已儲存嘅 OpenAI key 再次原生分析成功，來源及未納入數字一致 |
| 引用閱讀 | 按分析結果「來源 #5」打開正確原信，收起後分析仍顯示 |
| 正式資料庫完整性 | V23、`integrity_check=ok`、`foreign_key_check` 零違規 |

實際 GPT 透過桌面 App 共分析兩次同一個兩封郵件群組，第二次專門驗證重啟同重新同步後仍可用。結果以繁體中文為主，部分措辭偏書面，唔代表所有個人語氣要求都已完美達成。分析結果目前留喺頁面記憶體；群組、指示、來源記憶及郵件會持久保存，重新開 App 後需要重新按分析。

原生群組下拉選單嘅自動化 AX 選項未有正常顯示；退出選單後以鍵盤首字選取，UI「已加入群組」及資料庫均確認成功。冇因此繞過 Rust IPC 或直接改寫真實資料庫。

今輪冇寄信、封存或刪除 Gmail 原信。最終保留已開啟嘅 JARVIS，停喺「Gold Badge 跟進」群組，供用戶查看分析。
