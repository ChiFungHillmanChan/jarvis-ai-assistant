# 原生電郵工作區驗收紀錄

> **最新文字規範（2026-09-12）：**使用者已改為要求繁體書面語。本文原有口語、功能狀態及測試結果保留作歷史紀錄；新增介面、AI 中文回覆與交接採用[現行文案手冊](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/product/2026-09-12-cantonese-copybook.md)，不以本紀錄代表後續版本已驗收或已發行。

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

## 2026-09-12：v0.3.0 發行驗收

用戶明確要求測試、commit、push、部署及新版本發行。本節係 v0.3.0 嘅新驗證，唔以之前版本嘅成功代替本輪結果。功能來源已提交為 `b0854d7`；最終發行包含本輪原生驗收搵到嘅斷線副本分析修正。

### 自動測試及真實服務

| 檢查 | 結果／實際範圍 |
|---|---|
| Rust suite | 371 通過、0 失敗、8 ignored；HTTP fixtures 喺允許 loopback 嘅環境執行 |
| Frontend suite | 19 通過；斷線分析門檻回歸先重現失敗，再修正通過 |
| TypeScript／Vite | 通過 |
| GitHub CI | 功能提交 run `34716529668` 同最終源碼 `1de57cc` 嘅 run `34717493041` 均成功，包含 build、frontend、Rust 及 advisory clippy |
| 真實 Gemini | 明確執行模型供應清單檢查成功；唔等同今輪測試所有聊天／語音流程 |
| 真實 GPT | `gpt-5.6-luna` 經 production analysis entry point 分析合成郵件，1 項通過、4.17 秒；日期、HK$1,000、毋須付款／開會及來源正確 |
| 真實 Ollama 9B | `qwen3.5:9b` 嘅 production analysis 同設定 synthetic probe 兩項通過，共 11.59 秒；核對兩封合成郵件，來源 #101／#102，未納入 0 封 |
| 2B 限制 | `qwen3.5:2b` 設定 probe 可通過，但普通群組分析仍可能輸出不合格式引用而被拒絕；冇聲稱同 9B 同等可靠 |

真實模型測試只使用合成文字。OpenAI／Gemini 開發憑證只喺受保護測試子程序環境使用，冇寫入文件或安裝包。Ollama 0.34.0 使用獨立暫存模型目錄、loopback 11434 及 `OLLAMA_NO_CLOUD=true`；測試下載 2B／9B，冇改用戶原有模型目錄或正式電郵 AI 選擇。

### 實測發現及修正

1. Qwen3.5 細模型預設 thinking 可能用盡設定測試輸出額度，導致最終內容空白。只對批准嘅 exact 2B／4B／9B tags 設定 `think:false`，保留其他模型協議。
2. 合成 probe 有時返回完整 JSON code fence。只容許單一完整外層 fence，內部仍做嚴格 JSON、未知欄位及五項事實驗證；多餘文字、錯誤日期／金額／引用仍失敗。
3. Probe 指示明確規定 citation 係帶方括號嘅字串，唔係陣列。正常群組分析引用驗證冇放寬。
4. 導引原先要求 Gmail 即時連線，令斷線後保留嘅本機郵件無法分析。已移除連線計數依賴，仍要求本次 AI 測試成功及已揀本機郵件。讀取、群組成員及後端 context 隔離冇改。

### 原生測試及發行狀態

隔離 QA 使用 120 封合成郵件、兩個斷線 `example.test` 信箱、記憶體 Keychain。初次重建後操作工具讀到空白視窗，連已安裝 v0.2.0 操作亦逾時。重置原生工具後，同一測試程序可正常顯示廣東話介面；Web Inspector 顯示 CSS／DOM 已載入，Console 冇見到啟動錯誤。冇將未證實假設當產品 bug；臨時 Inspector 及 devUrl 診斷改動已移除，冇納入發行源碼。

最終重建後已完成以下真正原生操作，唔係直接改 SQLite 或 mock IPC：

| 操作 | 結果 |
|---|---|
| 準備並用於電郵 | 真實 9B 合成測試及套用成功，畫面顯示 `local / qwen3.5:9b` 同測試通過 |
| 重開準備頁 | 保留模型及香港繁體；測試成功狀態冇跨程序沿用，須重新測試 |
| 三步導引 | 沿用兩個已斷線信箱嘅 120 封本機郵件，揀 #120／#119，輸入群組名稱及廣東話指示 |
| 建立群組並分析 | 成功返回 9B 分析，來源 #120／#119、未納入 0 封；模型識別為人工測試資料，唔當作真實交易 |
| 開原信 | 「原信 #120」打開正確主旨及正文；內文 script 標記只以文字顯示 |
| 完成及退出重開 | 群組 2 封、指示、目前分頁、模型同香港繁體全部保留 |
| SQLite 補充核對 | QA 2 帳戶／120 郵件／1 群組／0 記憶；`integrity_check=ok`、外鍵零違規 |

生成摘要仍可能偏書面中文、夾英文或模型自加格式，唔代表完整廣東話語氣品質已達標。呢次合成案例驗收亦唔代替香港公司 20 組評估。

版本 metadata 已統一為 0.3.0。開始驗收時，已為 `/Applications/JARVIS.app` v0.2.0 同正式 DB 建立備份；SQLite 用 backup API 建一致副本。正式 DB 當時 1 帳戶、50 郵件、1 群組、1 記憶，`processing_mode=openai`，完整性正常。備份目錄：`~/Library/Application Support/jarvis/backups/before-v0.3.0-20260912T200510Z`。

稍後原生核對 `/Applications/JARVIS.app` 已顯示 v0.3.0，正常載入新設定導引；當次 binary SHA-256 與剛編譯嘅候選相同。導引沿用 `openai / gpt-5.6-luna`，冇重新儲存密鑰、改用本機模型或重做 Gmail 授權；正式 DB 再查仍為上述計數，完整性正常。呢個係本機啟動結果，唔等於 Apple 公證或公開更新已完成。

最後一般 DMG 封裝喺 Finder 外觀步驟失敗；卸載本次暫存卷後，以官方 `tauri bundle --ci --target aarch64-apple-darwin` 重用相同 binary 成功產生 Developer ID 簽署 App／DMG／updater tarball／signature。CI 模式略過 Finder 背景及圖示座標排版，唔改 App 功能。隔離 Ollama server 同所屬 runner 已全部停止，11434 已釋放，暫存測試模型及報告保留供重現。

### 最終發行檔獨立核對

| 檔案 | Bytes | SHA-256 |
|---|---:|---|
| `JARVIS_0.3.0_aarch64.dmg` | 17,542,163 | `18ca236c6a62bed86d59e46381c532afd9a01ac5687577284ae428c66423f8e0` |
| `JARVIS.app.tar.gz` | 15,580,494 | `ba3a93a95b8bc22af853ef895fe83d83e4d1ae42ffe996a6053990110aebc44f` |
| `JARVIS.app.tar.gz.sig` | 404 | `04fbc334222c47393ea2ddb3fd7a355aa73fc450d8dae192c7b8ed6417dd3c29` |

檔案喺 `src-tauri/target/aarch64-apple-darwin/release/bundle/` 下嘅 `dmg/` 同 `macos/`。已用 Tauri 設定內嘅公鑰實際驗證最新 updater 簽名，唔係只檢查 `.sig` 非空；DMG `hdiutil verify` 通過，唯讀掛載核對後已卸載。

DMG、tarball 同最終 bundle 內 App 都係 `com.jarvis.app / 0.3.0`，binary SHA-256 同為 `5db984c3f697639407b378948d38228a43cb79247ce6cd35ea897d4a9d39b4a4`。三者同已安裝 App strict codesign 驗證通過；已原生驗收嘅安裝版本同最終 bundle 完整 CodeDirectory hash 均為 `075e2768795f9cbea423f09cc8dfbc04befc7491261556552028905d1366d333`，只因重新簽署時間而有 binary 檔案 hash 差異。

封裝未發現 `.env`、資料庫、私匙檔或 QA seed marker；常見 AI／AWS key 格式掃描零命中。呢個係有限模式及檔案檢查，唔等同全面秘密資料審計。上述 DMG hash 係公證 staple 前；成功 staple 後須重新計算，唔重用舊值。

Apple 公證提交被自動審批拒絕，要求用戶明確批准將產品 DMG 上傳 Apple；已發出針對性確認問題。雖然 Desktop OAuth 公開 client 設定與已發行 v0.2.0 相同、亦確認冇 AI key／私人郵件／DB／Keychain，仍唔繞過審批。公證、S3 上架、網站更新 manifest 及 GitHub release 必須以之後實際完成記錄為準；本段唔代表已公開發行。最後讀取正式 `https://jarvis-automation.com/latest.json` 仍為 v0.2.0；官網原有未提交改動保持原狀。

## 2026-09-12：用戶批准後嘅正式發行

用戶回覆「allowed. you can do it for me please」，明確批准之前提出嘅 Apple 公證提交，並要求繼續完成部署及 release。以下更新取代上一節當時嘅公證待批狀態。

- Apple 公證 submission：`92cd90d8-c2a3-4dcb-ad9b-c060ce89b2f7`，2026-09-12T20:41:15.770Z 提交，結果 **Accepted**。
- DMG `stapler staple`／`stapler validate` 通過。最終 bundle 同 `/Applications/JARVIS.app` 經 `spctl --assess --type execute` 均返回 **accepted / Notarized Developer ID**。
- 公證後正式 DMG：17,543,853 bytes，SHA-256 `8d24bb683be1acd1a393d7fc736da0396be659dd0f2ccd5370a11e344ed742dd`。前表嘅 DMG hash 只係 staple 前快照；tarball 同 updater signature 冇改動。
- 已上載三個版本化檔案到既有 `jarvis-releases` bucket；逐個用公開 HTTPS 網址 GET 下載，實際 bytes／SHA-256 與本機公證後檔案完全吻合。再用公開下載嘅 tarball 同 `.sig`、配 App 公鑰驗證成功。
- `JARVIS_latest_aarch64.dmg` 固定下載別名已更新。冇改 bucket 權限或加入其他私人資料。
- 官網只修改 `public/latest.json`，提交 `c472c1f5480d78d7f892bd313311265445e5c0ad`，CI/CD run `34717975716`；原本網站嘅其他未提交改動保留。

官網 CI/CD `34717975716` 嘅 Verify 同 production deployment 全部成功，Vercel deployment `dpl_2dAxQFy22a4wXrsfWug8Fft2EUFN` 為 Ready。正式 `https://jarvis-automation.com/latest.json` 返回 HTTP 200／v0.3.0，同已提交 manifest 逐 byte 相同：2,361 bytes，SHA-256 `615303a034c0a5daaf0eb2633903f5700e7b01a2b50b9847485285138df17e22`。英中下載頁 HTTP 200；固定 DMG 下載亦重新 GET 核對，與公證後檔案相同。S3 manifest 副本已同步。

Git tag `v0.3.0` 指向已測試及 CI 通過嘅 `14348d95fa3a55c143e9bc7e92fd259a2f26398f`（主分支 CI `34717741978` 成功）。[GitHub v0.3.0 release](https://github.com/ChiFungHillmanChan/jarvis-ai-assistant/releases/tag/v0.3.0) 已於 **2026-09-12T20:48:01Z** 正式發布並設為 Latest，唔係 draft／prerelease。DMG、updater tarball、`.sig` 同 `SHA256SUMS.txt` 四個附件全部 uploaded；GitHub 回報嘅 bytes 同 SHA-256 digest 逐項同本機發行檔吻合。

正式下載：[v0.3.0 DMG](https://jarvis-releases.s3.ap-east-1.amazonaws.com/JARVIS_0.3.0_aarch64.dmg)。本輪已完成測試、源碼 commit／push、Apple 公證、S3 上架、官網部署、updater manifest 及 GitHub release。公證唔等於 Google OAuth 公開驗證、公司試點或所有硬件準確度驗收；2B／完整廣東話語氣等限制保持原有說明。
