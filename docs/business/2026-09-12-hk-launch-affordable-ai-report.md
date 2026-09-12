# JARVIS 香港公司產品改善及平價 AI 報告

> **最新文字規範（2026-09-12）：**使用者已改為要求繁體書面語。本文僅更新語體，原研究日期、價格、事實、來源及歷史實作狀態保留；後續介面與 AI 中文回覆以[現行文案手冊](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/product/2026-09-12-cantonese-copybook.md)為準，不以本紀錄代表後續版本已驗收或已發行。

JARVIS 最值得優先改善的，是讓不熟悉 API 的香港公司使用者，從開啟 App 到取得第一份附有原信引用的客戶跟進摘要，都無須理解三個模型供應商、兩套 Google 連接及本機服務指令。**短期以統一設定入口、完整廣東話介面及可選的本機 Qwen 為基礎；面向一般公司的長期方案，是將有限 AI 用量與服務一併收費，由 JARVIS 處理供應商設定。**

本報告資料核對截至 2026-09-12。程式審核基準為產品 repo `main`、HEAD `6570acf`、版本 `0.2.0`；開始審核時工作目錄乾淨，migration 檔案最高為 V24。這些是 repo 狀態，不代表已安裝 App 或真實資料庫已升級。2026-09-10 的 Gmail／GPT 實測另有歷史報告，本次沒有重跑真實服務。下文區分「現有能力」「官方資料」「建議／估算」；新增功能在當時均未實作。

配套文件：[逐頁繁體書面語文案](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/product/2026-09-12-cantonese-copybook.md)、[分階段實作方案](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/superpowers/plans/2026-09-12-hk-activation-local-ai.md)。

## 1. 四個要求的具體答案

| 要求 | 建議做法 | 完成後應有的體驗 |
|---|---|---|
| 一個入口設定所有必要功能 | 以電郵工作區作為首次設定主線，按實際 readiness 顯示下一步；其他 integrations 按需要連接 | 保留已有 key／Gmail；使用者無須選擇三個 API，再猜測各項設定影響哪些頁面 |
| 一鍵啟動本機 AI，盡量降低成本 | 沿用 Ollama，按機型只推薦一個經測試的 Qwen；補上安裝引導、啟動、下載、驗證及套用 | 安裝後可一鍵啟動；無須 API key；首次下載大小、進度及剩餘步驟清楚可見 |
| 詳細廣東話版本 | 介面語言、AI 回覆語氣、語音辨識及朗讀聲線分開設定；全流程採用香港用語 | 按鈕、錯誤、權限、更新、空白狀態及無障礙標籤一致，而非僅有 AI 能以廣東話回答 |
| 向香港公司銷售 | 先服務本身使用 Mac + Gmail 的小公司客戶負責人，提供可核對的跟進成果 | 清楚知道傳送資料、處理地點、費用、出錯後的核對方法及停止使用時的退出方式 |

「一鍵」應指一個清楚的入口，以及自動處理可代辦的設定，並非零授權。Google 登入、公司管理員批准、macOS 安裝／麥克風權限及付款，仍需由適當人士確認。下載 3.4GB 模型亦不會瞬間完成；產品應分別量度主動操作時間及下載等候時間。

## 2. 現有產品容易造成混淆的地方

| 現有狀態，來自程式審核 | 對使用者的影響 | 修正優先度 |
|---|---|---|
| Onboarding 先展示 Gemini／OpenAI／Claude key，再連舊 Assistant Google 路徑 | 以為全部必填；完成導引後新電郵工作區仍可能未連接 | P0 |
| 新 `mail_*` 工作區與 Assistant 的模型路由、Google token 分開 | 「已連接」不等於所有功能都 ready；設定頁需顯示用途 | P0 |
| Local AI 提供複製 `brew install ollama`／`ollama serve`；下載流程假設服務已經運行 | 非技術使用者需要自行開啟終端機；尚非一鍵啟動 | P0 |
| 本機模型加入 Assistant chain，不會自動設定 Email 模式／模型 | 模型下載完成後，電郵仍可能使用另一供應商或尚未設定 | P0 |
| `Language` 控制 AI 回覆與 STT，不會切換全 App UI | 使用者選擇廣東話後仍看見大量英文，容易認為設定無效 | P0 |
| 新 EmailPage 多數已有廣東話，shell、導引、設定及工具頁仍大量英文 | 信任及易用性未連貫 | P0 |
| 模型目錄有大小錯標，下載完成及已安裝的判定存疑 | 可能誤下載過大模型，或顯示未經驗證的成功狀態 | P0 |
| 郵件／分組／指示／記憶持久保存，分析結果仍屬頁面狀態 | 重開後需要再次分析，增加費用與重複工作 | P1 |
| 文字分析、本機 LLM、STT、TTS 屬不同部分；部分語音播放路徑未完成 | 不能將「有 local AI」宣傳為完整離線廣東話語音 | P1 |

主要檔案：[OnboardingWizard](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/OnboardingWizard.tsx)、[Settings 語言設定](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:893)、[LocalAiSetup](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/LocalAiSetup.tsx)、[EmailPage](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/EmailPage.tsx)、[本機檢查](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/system_check.rs)、[模型下載](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/model_pull.rs)。程式審核發現不等於已重現的原生測試結果。

仍可展示的能力包括：在同一部 Mac 集中多個 Gmail、手動跨信箱分組、每組指示、使用者確認的來源記憶，以及可開啟原信的分析引用。新分析路徑沒有寄信、付款或日曆工具，不應描述為已自動完成所有行政工作、團隊共用信箱、完整備份或跨裝置同步。

## 3. 減少客戶自行申請 API 密鑰的負擔

第一頁只問「希望如何使用 AI」：

| 路徑 | 客戶需要做甚麼 | JARVIS 負責甚麼 | 推出次序 |
|---|---|---|---|
| 本機 AI | 確認下載及所需安裝；連接 Gmail | 檢查機型、啟動服務、下載、驗證並選擇適合的工作量 | 優先完成 |
| 自備 AI 帳戶（進階） | 只為所選供應商填一個 key／由公司管理員配置 AWS | 官方開通連結、費用解釋、憑證測試、模式一致性、錯誤下一步 | 保留既有用戶路徑 |
| JARVIS 包含 AI 用量 | 登入 JARVIS、接受服務範圍及資料流、連接 Gmail | 代管推論服務、收費、額度、支援及供應商資格 | 完成需求／政策／成本驗證後推出 |

第三條路徑最接近一般非技術公司所需的「無須處理 API」產品。**不能將 founder 的 API key 放入桌面 binary**；必須由受控後端保存供應商憑證，以短期 App session 驗證使用者，再由伺服器執行額度及權限檢查。只有使用者要求分析時才傳送所需郵件內容，預設日誌不記錄正文。此後端當時尚未成為產品能力，會增加資料處理責任，不應只為新增一個按鈕而草率建立服務。

| 設定／憑證 | 正常應由誰處理 |
|---|---|
| Google OAuth project、client 設定、品牌及公開驗證 | JARVIS 開發／營運者；一般客戶無須建立 Google Cloud project |
| Gmail 登入及授權 | 信箱用戶；受管帳戶可能要公司管理員批准 |
| 包含用量方案的模型 API key／AWS role | JARVIS 後端；不交予每位終端使用者 |
| BYOK key／公司 AWS | 選擇進階模式的使用者／公司 IT；App 透過 Keychain 保存憑證 |
| 本機模型 | 無須模型 API key；首次需要安裝／下載 |
| 麥克風／語音 | 真正使用語音時才要求；文字電郵流程不應被語音設定阻擋 |
| GitHub／Notion／其他工具 | 第一次使用該工具時才連接，不列為核心電郵啟用門檻 |

導引建議順序：語言 → 選擇處理方式 → 驗證 AI → 連接電郵工作區 Gmail → 同步首批 → 以兩三封郵件建立第一個項目群組 → 整理下一步 → 開啟原信核對。尚未願意連接真實信箱時，提供清楚標示的合成示範。只有實際請求成功且產生可用結果時，才顯示「可以開始使用」；僅儲存 key 應顯示「已儲存，未測試」。

## 4. 香港地區是選擇供應商的必要條件

OpenAI 官方 API 支援地區清單在查閱當日沒有列出香港，並指出在清單以外使用或提供服務可能導致停權。因此，不應將香港公司直接申請 OpenAI API 設為新客戶預設。這不否定合資格地區既有的 GPT 自用流程，也無須為撰寫報告重設已儲存憑證。[OpenAI 支援地區](https://developers.openai.com/api/docs/supported-countries)

Google AI Studio／Gemini Developer API 官方地區清單亦沒有列出香港。消費者 Gemini App 能否在香港使用，不等於 Developer API 的開通條件相同；Google 指向的企業雲端路徑是另一項需要驗證的服務。[Gemini API 支援地區](https://ai.google.dev/gemini-api/docs/available-regions)

GPT 模型雖已出現在 Bedrock，但 AWS 公布的 OpenAI Services Agreement（June 2026）第 14.13 條仍限制 Customer 與 End Users 的使用地區，其定義引用 OpenAI 支援清單。**不應將 Bedrock GPT、海外伺服器或 VPN 寫成香港商用的保證方案。** OpenAI 開放權重模型有另一套授權，亦不可與商業 GPT 混淆。[AWS 第三方模型條款](https://aws.amazon.com/legal/bedrock/third-party-models/)

對香港公司，首輪雲端驗證建議先考慮 AWS 自家 Nova，或授權與銷售地區清楚的開放權重服務，並核對實際 AWS 帳戶、模型、region、資料目的地及公司批准。Bedrock 支援某模型，不代表現有 JARVIS 使用的 API、憑證或推論區域已通過測試。Nova Micro 是文字模型，官方列有 Converse；尚無足夠證據證明其廣東話電郵品質達標，應先進行候選評測。[Nova Micro 模型卡](https://docs.aws.amazon.com/en_en/bedrock/latest/userguide/model-card-amazon-nova-micro.html)

Kimi／DeepSeek 可列為第二輪候選；正式開放前同樣需要核實帳戶地區、資料處理、商用條款、在售模型及實際 API。不能因介面相容 OpenAI 就只更改 URL；現有 OpenAI 郵件路徑固定使用官方 endpoint，新增供應商需獨立處理憑證、請求、錯誤及引用驗證。

## 5. 每月可降低至多少成本

以下為**純文字模型費估算，並非 JARVIS 售價或實際帳單**。未計 reasoning 額外輸出、重試、多輪對話、語音、附件辨識、稅項、付款費、支援及後端。所有輸入使用未命中 cache 的價格，不假設有免費額度。美元換港幣固定以 **US$1 = HK$7.80** 作規劃假設，並非即時匯率。

| 方案 | 每百萬輸入／輸出 token，美元 | 適用性及來源 |
|---|---:|---|
| 本機 Qwen | 雲端 token 費 $0 | 須已有合適電腦；電費、時間、儲存及支援另計 |
| Nova Micro | $0.035／$0.14 | AWS 2026-04-16 官方文章的列價用作估算；動態價表未取得對應地區數字，正式開通時需再核實，不視為鎖定現價。[AWS 成本資料](https://aws.amazon.com/blogs/machine-learning/cost-efficient-custom-text-to-sql-using-amazon-nova-micro-and-amazon-bedrock-on-demand-inference/) |
| DeepSeek Flash | 非繁忙 $0.15／$0.60；繁忙 $0.30／$1.20 | 當日直接定價頁；`deepseek-flash`，未接入 JARVIS、未完成香港帳戶驗證。[DeepSeek 定價](https://api-docs.deepseek.com/quick_start/pricing/) |
| GPT-5.6 Luna | $0.20／$1.20 | Standard、短 context；用於合資格地區比較，並非香港直連預設。[OpenAI 定價](https://developers.openai.com/api/docs/pricing) |
| Gemini 3.1 Flash-Lite | $0.25／$1.50 | Standard 文字；合資格 Developer API 地區比較，未接新電郵工作區。[Google 定價](https://ai.google.dev/gemini-api/docs/pricing) |
| Kimi API | 本次未核實確切數字 | 舊頁轉至新平台，動態價表未能完整讀取，因此不填入舊 K2.5 價格。[Kimi 官方定價](https://platform.kimi.ai/docs/pricing/chat) |

DeepSeek 繁忙時段為星期一至五 UTC 01:00–04:00、06:00–10:00，即香港 09:00–12:00、14:00–18:00，正好涵蓋常見上班時間。香港公司不應只按夜間低價估算；舊搜尋快照顯示的 V4-Flash 及舊價，已被直接頁面的 V4.1-Flash／`deepseek-flash` 資料取代。[DeepSeek 當日價表及時段](https://api-docs.deepseek.com/quick_start/pricing/)

成本公式：`次數 ×（每次輸入 token × 輸入單價 + 每次計費輸出 token × 輸出單價）÷ 1,000,000`。

| 每人每月工作量假設 | Nova Micro 參考價 | DeepSeek 非繁忙／繁忙 | GPT Luna | Gemini Flash-Lite |
|---|---:|---:|---:|---:|
| 200 次；每次 10K 輸入 + 1K 輸出 | HK$0.76 | HK$3.28／6.55 | HK$4.99 | HK$6.24 |
| 600 次；每次 10K + 1K | HK$2.29 | HK$9.83／19.66 | HK$14.98 | HK$18.72 |
| 1,000 次；每次 30K + 2K | HK$10.37 | HK$44.46／88.92 | HK$65.52 | HK$81.90 |

即使使用相同的低價模型，讀信次數與長度增加也可令成本上升十多倍。現有郵件流程設有字元上限，**字元不等於 token**；上表工作量為獨立估算，並非由 50 封同步上限直接換算。應記錄每次 provider/model、計費 token、費率版本、錯誤重試及工作區用途；本機只記錄延遲與記憶體，不虛構美元費用。

最低成本策略依次為：先縮小傳送資料、保存可重用分析、資料變更後才重算、限制輸出、按任務選模型，再考慮 cache／batch。新郵件或記憶變更應令舊分析失效或清楚標示過時，不能為節省費用而顯示過期承諾。免費 tier 只用於合適的示範；Google 定價列明 free／paid 的產品改善用途不同，不應為節省費用將真實公司資料送往尚未確認用途的免費服務。[Google 免費及付費資料用途](https://ai.google.dev/gemini-api/docs/pricing)

另一項更大的成本是支援：假設內部時間成本為 HK$200／小時，一次 30 分鐘設定支援已需 HK$100；縮短至 5 分鐘則約為 HK$16.67。這是工時計價假設，並非實際工資。因此，減少 API 設定通常比進一步節省數元 token 費更值得優先處理。初期不建議為低用量長期租用 GPU，也不建議客戶先購買高 RAM Mac。

### 現有 App 的最少設定路徑

已有正常 GPT／Gmail 的合資格自用環境可直接沿用，無須申請三個供應商。電郵工作區的「電郵分析設定」選擇 OpenAI GPT／已驗證模型後，會讀取 Settings 儲存的 OpenAI key；舊 Assistant 模型選擇不能取代此頁設定。只有尚無 key 的進階使用者，才需透過供應商官方帳戶建立，再親自填入 App；不應將 key 貼入對話。

香港雲端試點若先驗證 Nova Micro，應由公司 IT／開發者處理以下流程，不將這份 AWS 工程設定清單當作一般客戶的最終體驗：

1. 核對公司 AWS 帳戶、付款／credits 適用範圍及模型存取，先在支援的 region 測試合成文字；例如官方模型卡列出 `us-east-1` 可用 in-region，資料會傳至美國，須符合公司選擇。
2. 公司管理員配置僅允許所需推論的憑證，不使用 root key。現有 JARVIS 欄位為 Access Key ID＋Secret Access Key＋Region，並非 Bedrock bearer API key 輸入框，不可混用。
3. 在 Settings → Models 的 AWS Bedrock 區塊儲存上述資料。現有「Singapore recommended」文案不適用於所有模型，應先依實際模型卡選擇地區。
4. 在電郵工作區 → 電郵分析設定 → 自備 AWS Bedrock，填入對應模型 ID，例如 in-region `amazon.nova-micro-v1:0`；確認郵件外傳說明後才儲存。
5. 先使用合成／明確允許的非敏感案例核對金額、否定句、日期及引用，並記錄真實帳單。通過後才向 pilot 使用者開放；本輪未進行此項真實 AWS 驗收。

現有介面及資料型別佐證：[AWS 欄位](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:840)、[電郵分析設定](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/EmailPage.tsx:127)。模型／區域依據：[Nova Micro 官方模型卡](https://docs.aws.amazon.com/en_en/bedrock/latest/userguide/model-card-amazon-nova-micro.html)。

## 6. 低成本本機方案：Ollama + Qwen

Ollama 官方 macOS 要求為 Sonoma 14 或更新版本；Apple M 系列支援 CPU／GPU，Intel 為 CPU。首版安裝建議集中 Apple Silicon；Intel 應顯示可測試範圍與速度限制，不承諾相同體驗。[Ollama macOS 指引](https://docs.ollama.com/macos)

下表為待測試的工程建議，並非官方最低 RAM 或已量度的速度。下載包大小不等於運行 RAM；macOS、瀏覽器、context cache 及其他程式亦需要空間。

| 統一記憶體 | 第一個候選 | 查閱時下載包 | 建議起始 context／磁碟餘額 |
|---|---|---:|---|
| 8GB | `qwen3.5:2b`，Q8_0 | 2.7GB | 2K–4K／至少 8GB 可用空間；僅輕量短任務 |
| 16GB | `qwen3.5:4b`，Q4_K_M | 3.4GB | 4K／至少 10GB |
| 24GB | `qwen3.5:9b`，Q4_K_M；重視速度可測 4B | 6.6GB | 4K–8K／至少 15GB |
| 32GB | `qwen3.5:9b`，Q4_K_M | 6.6GB | 8K 起／至少 15GB |

官方 tag／大小：[2B](https://ollama.com/library/qwen3.5:2b)、[4B](https://ollama.com/library/qwen3.5:4b)、[9B](https://ollama.com/library/qwen3.5:9b)。Qwen 官方模型卡標示 Apache-2.0；出貨要記錄原模型授權、量化來源、digest 及已測試的 Ollama 版本。[Qwen 4B 模型卡](https://huggingface.co/Qwen/Qwen3.5-4B)

4B 是候選模型，不是廣東話品質保證。一般多語言 benchmark 不足以證明其能正確理解「唔使付款」「暫定」「改咗下星期」及跨信箱的新舊版本。小型模型應先處理簡短郵件／少量相關片段；目前 JARVIS 本機 context 路徑最多採用 32,768，並按 UTF-8 長度作保守檢查。若改為 4K，必須同步縮小分析內容，明示未讀範圍，不能只降低 `num_ctx` 後仍傳入所有郵件。[本機 context 程式](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/mail/analysis.rs:436)、[Ollama context 文件](https://docs.ollama.com/context-length)

**現階段手動驗證方法**：從官方下載頁安裝 Ollama 並開啟 Ollama App；服務已運行時不再啟動第二個。16GB Mac 可使用以下命令測試模型。這是供日後驗證的教學，本次沒有執行或下載。

```sh
ollama pull qwen3.5:4b
ollama run qwen3.5:4b
ollama ps
```

8GB 改用 `qwen3.5:2b`；24／32GB 可測試 `qwen3.5:9b`。若版本不支援模型，先評估官方升級。之後在 JARVIS **電郵工作區**選擇本機並填入相同 exact tag，先用合成短郵件測試引用，再重開驗證設定。僅在 Assistant 加入 chain 不等於完成這一步。[官方下載](https://ollama.com/download/mac)、[Ollama CLI](https://docs.ollama.com/cli)

日後一鍵流程應包括：偵測 → 必要安裝引導 → 啟動／重用 → 真實下載進度 → 驗證成功 → 合成廣東話試用 → 套用至明確工作區。需支援中斷重試、磁碟不足、模型不相容及記憶體不足；保留已成功步驟，不要求重新開始。首次 OS 安裝可保留官方互動，安裝後啟動應無須使用終端機。[Ollama pull API](https://raw.githubusercontent.com/ollama/ollama/main/docs/api.md)

本機模式只允許經驗證的本機模型及 loopback endpoint，不將 `:cloud` 或 remote alias 視為離線。`OLLAMA_NO_CLOUD=1` 是 Ollama 可用選項，但不會替 JARVIS 關閉 STT 或其他雲端路徑。離線文案只適用於「已下載內容的已驗證文字分析」；Gmail 授權、同步及首次模型下載仍需網絡。[Ollama FAQ](https://docs.ollama.com/faq)

模型目錄需先修正：`llama3.2:latest` 是 3B／2.0GB，而非 8B；`llama3.3:latest` 是 70B／43GB，而非 13B。發行時不應以易變的 `latest` 取代明確大小，再依手寫標籤估算 RAM。[Llama 3.2](https://ollama.com/library/llama3.2)、[Llama 3.3](https://ollama.com/library/llama3.3)

## 7. Kimi 為何不是一般 Mac 的首選

Kimi 是 App／模型系列／雲端 API 的品牌，並非單一小型模型。以 Kimi K2.5 為例，官方架構為 1T 總參數、約 32B active。active 只描述每個 token 啟用部分參數，不代表只需保存該部分權重。[Kimi K2.5 官方](https://github.com/MoonshotAI/Kimi-K2.5)

純算術：`1T × 4 bit ÷ 8` 約為 **500GB** 權重，未計量化額外資料、runtime 及 cache；這不是官方最低 RAM。Offload／分散部署可以改變所需機型，但不符合 8–32GB Mac「低成本且簡單」的目標。官方亦有較小的 Kimi-VL（16B total／3B active），因此不應泛稱所有 Kimi 均無法運行；本輪只是尚無合適、穩定的 Mac tag 及電郵驗收證據支持將其設為預設。[Kimi-VL 官方](https://github.com/MoonshotAI/Kimi-VL)

查閱當日，Ollama K2.5 頁面寫明此模型已於 2026-07-31 retired，不應複製舊文章的 `kimi-k2.5:cloud` 教學。本次未核實 Kimi 新平台的數字價表，正式 API 接入留待第二輪，不以過時價格作出「最便宜」結論。[Ollama Kimi 頁](https://ollama.com/library/kimi-k2.5)、[Kimi 定價](https://platform.kimi.ai/docs/pricing/chat)

## 8. 完整廣東話版本的要求

原方案以香港繁體中文作為介面，指引可採自然廣東話，對客電郵另設「香港商務書面語」選項；不應因 UI 使用廣東話，就強制所有正式客戶內容採用口語。偏好應拆分為 UI locale、回覆語氣、STT 語言及 TTS 聲線，沿用既有 preference 資料時保留舊設定。此段保留原研究方向，最新文字規範以文首說明及現行文案手冊為準。

| 範疇 | 必須包含的內容 |
|---|---|
| 導航／啟用 | 電郵工作區、AI 助理、已連接服務、開始設定、繼續設定、暫時略過 |
| 錯誤 | 說明問題、目前影響、下一步；例如「API key 已儲存，但服務未能使用。請檢查帳戶餘額後再測試。」 |
| 日期／時區 | 使用明確年月日及時區；公司可選 `Asia/Hong_Kong`，不因使用者說廣東話就擅改日曆時間 |
| 金額 | 保留 HKD／CNY／USD；來源只有 `$` 時不自行當作港幣；萬元、k 及小數需準確 |
| 事實／語氣 | 保留原信引用；區分已確認、待確認、暫定及毋須；不將推測寫成承諾 |
| 失敗及無障礙 | 表單標籤、ARIA、快捷鍵提示、通知、下載／更新、空白狀態同步翻譯 |
| 語音 | 中英夾雜、香港地名、人名及公司術語分開驗收；文字版本成功不代表語音已完成 |

最低驗收使用 20 組合成香港工作郵件，涵蓋否定句、改期、不同貨幣、舊記憶失效、兩個同名客戶、UK／HK 時差、模糊日期、附件缺失、來源引用及郵件注入。每組均有人工正確答案；關鍵金額／期限不容許錯誤斷言，資料不足需明示。小樣本全部通過只是 release gate，不等於真實世界零錯誤。

## 9. 香港公司願意付費的實際場景

工業貿易署 2026 年 3 月資料列有 357,077 間中小企，佔企業逾 98%。但這並非 JARVIS 的可服務市場，仍需按 Mac、Gmail、安裝權限、工作方式及預算篩選。資料頁底部仍有較舊的覆檢日期，本報告採用表內明示的統計期。[工業貿易署中小企統計](https://success.tid.gov.hk/tc_chi/aboutus/what_are_sme.html)

**首批 ICP 假設**：香港 2–15 人的創意／營銷／顧問公司中，使用 Mac + Gmail／Google Workspace、同時跟進多個客戶的老闆或 account lead。先提供「跨信箱整理客戶變更、待回覆事項及已確認背景」。公司規模、使用頻率與付費意願均需訪談，不視為行業普遍事實。

第一個兩分鐘示範：同一客戶的兩個信箱、三次交期變更 → 加入同一群組 → 加入「未正式落單唔當已承諾」指示 → 使用者確認一條來源記憶 → 產出三項下一步 → 開啟原信。展示減少遺漏跟進及重複閱讀，而非只展示 AI 聊天。

首輪訪談應詢問實例、設備、信箱、客戶跟進主要使用 email 還是 WhatsApp、誰批准 AI、目前處理方法及所需時間。本次沒有具代表性的官方數據證明香港公司使用 Gmail／M365 或 Mac／Windows 的比例，不應沿用舊市場文件的桌面市佔率作為公司採購比例。

先招募設備合適的 pilot，不要求公司更換系統。記錄 20 位合資格候選中，有多少僅因 Outlook／Windows 而流失；若至少 5 位有真實場景及預算，卻被相同缺口阻擋，再驗證擴展需求。這是內部決策門檻。M365 與 Windows 是兩項工作；Graph delegated `Mail.Read` 可作為未來個人信箱的起點，共用信箱及租戶管理員批准需另行處理。[Microsoft Graph 權限](https://learn.microsoft.com/en-us/graph/permissions-reference#mailread)、[Microsoft 使用者同意設定](https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/configure-user-consent)

## 10. 公司採購前需補足的信任與公開驗證

Gmail `readonly` 亦屬 restricted scope。Google 指出，將此類資料儲存／傳送至伺服器涉及 security assessment 要求；本機 App 或 BYOK 不會自動豁免雲端分析資料流。Testing 的 100 位 test users／7 日授權到期，以及未驗證 App lifetime 100 位新使用者上限，是不同限制；轉為 production 不等於完成驗證。[Gmail scopes](https://developers.google.com/workspace/gmail/api/auth/scopes)、[Google Audience 規則](https://support.google.com/cloud/answer/15549945)

Google 政策容許可見的生成式電郵摘要，但限制資料用途、轉交及訓練，並設有保留／cache／加密要求。需要逐項對照現有本機郵件副本、斷線保留、來源記憶及刪除行為，不自行斷言所有本機副本違規。SQLite 沒有應用層加密，Keychain 保護 key 不等於郵件庫已加密。最終評估範圍、CASA 費用與時程尚未核實，不編造固定報價。[Google Workspace 資料政策](https://developers.google.com/workspace/workspace-api-user-data-developer-policy)

PCPD 模範框架與僱員指引可轉化為採購材料：列明工具用途、最少資料、人手核對、權限、事故支援及退出方式。這是依指引設計，並非公署認證或法律合規保證。[PCPD AI 框架](https://www.pcpd.org.hk/tc_chi/news_events/media_statements/press_20240611.html)、[PCPD 僱員指引](https://www.pcpd.org.hk/tc_chi/news_events/media_statements/press_20250331.html)

每間 pilot 公司應收到四份簡短材料：一頁資料流、安裝及移除指引、員工可用／不可用案例，以及供應商與支援範圍。需列明斷開連接後保留哪些資料、刪除本機帳戶會清除甚麼、是否變更原 Gmail、模型在哪裡處理、保留多久及如何退出。重開、撤權、斷網及更新相容性均屬產品驗收範圍，不能只驗證一次成功摘要。

## 11. 定價與銷售實驗

以下為建議測試價，全部尚無成交或續費證明，並非本輪已開通的套餐：

| 階段 | 測試內容 | 價格／成本處理 |
|---|---|---|
| 首批 3 人／3 間公司，14 日 | 一個明確工作流程、30 分鐘設定、15 分鐘覆盤 | HK$300／人試點服務；本機無 API 費，或使用公司已批准且地區合資格的 BYOK。雲端另付費須事先說明。此收入不計入 MRR |
| 續費第一批 | 相同核心工作流程，限制支援範圍 | HK$98／人／月，本機／BYOK；記錄真實付款及拒絕原因 |
| 下一批可比客戶 | 相同功能與服務量，測試價格接受度 | HK$148／人／月；小樣本不聲稱統計顯著 |
| 包含 AI 的方案，另一輪 | 確認 key 是主要流失原因時再測試 | 可先測 HK$148／人／月並明示額度；以每月 200 次、每次10K輸入／1K計費輸出作成本設計起點，產品需處理超長請求，不宣稱無限 AI |

不同成本模式不應同時對同一客戶反覆改價。包含 AI 與 BYOK 的售價需分別記錄成本；模型／reasoning／token 長度改變會影響可提供次數。Quota 應設有伺服器硬上限、提前提醒及超額停止，不能僅為 UI 數字。

先提供 HKD 報價、清楚服務範圍、付款收據及取消方式。公司對賬需要時可評估發票／FPS 流程；正式收款、稅務與付款啟用另行處理。不出售尚未完成的 Team RBAC，也不將一次性 setup 收入計作 MRR。100 人 × HK$148 = HK$14,800 MRR 是目標算術，並非預測。

渠道先使用 founder 現有公司／HKSTP 人脈及定向示範，記錄各來源的合資格率、完成啟用率及續費。主要銷售句可寫為：「集中整理多個 Gmail 信箱的客戶郵件，記錄你已確認的背景，以廣東話整理下一步；每個重點均可返回原信核對。」下方明示目前適用 Mac + Gmail。

## 12. 四星期內先驗證一條可售流程

以每星期約 15–20 小時作規劃假設；工程細節見配套方案。Google 審核及客戶管理員批准是外部依賴，不保證四星期內完成。若尚未滿足真實資料使用條件，繼續合成測試，不稱為已商用。

| 時間 | 產物 | 決策門檻，全部為目標而非已達結果 |
|---|---|---|
| 第1週 | 20 人篩選、5 次工作流訪談；計時觀察現有設定；確定公司允許的 AI 路徑 | 至少5位設備／信箱／權限合適；找出最大3個啟用阻礙；政策缺口有負責人 |
| 第2週 | 統一啟用流程及核心廣東話；修正錯誤模型目錄；本機啟動最小流程；20組合成驗收 | 5位新測試者至少4位無須工程師代操作；已有可用AI服務時10分鐘內取得首份可核對摘要；模型下載另計 |
| 第3週 | 達到條件後啟動3個14日付費pilot，按需要分批；驗證斷網／重開／引用 | 至少2人連續3個工作日重用；關鍵數字／期限測試沒有錯誤斷言 |
| 第4週 | 完成足14日的pilot覆盤、續費、支援及成本報告；未滿14日者延後覆盤 | 3人最少2人實際續費；每人至少5個同類任務，中位處理時間減少目標20%，包括核對時間 |

以人工基準和難度相近的案例輪換先後順序，減少熟悉郵件造成的假進步。記錄首次有用結果時間、key／Google 啟用阻礙、每週使用日、修正次數、漏報、token 成本、支援分鐘及續費；預設 telemetry 不收集正文。

若啟用仍失敗，先修正設定；若使用後不再重用，先改善工作價值；若只有 key 造成阻礙，再提供包含 AI 的方案；若設備／信箱不合適，先驗證擴展平台需求。不應同時開展 Windows、Outlook、WhatsApp、S3、多人團隊及新語音引擎。

## 13. 證據範圍與來源索引

本輪原始交付為研究、文案及實作方案，沒有修改 App、API key、Gmail 授權、模型、正式資料庫或公開網站。程式結論來自只讀審核；硬件容量、價格試驗、工時及商業目標均已標示假設。未將舊 roadmap、founder 自用或單元測試數目當作市場驗證。

所有網頁查閱日為 2026-09-12；沒有發布日期的頁面僅按查閱日記錄。主要來源如下，具體用途已在正文對應段落引用：

| 發布者／資料 | 日期或版本 | 本報告用途 |
|---|---|---|
| [OpenAI API 支援地區](https://developers.openai.com/api/docs/supported-countries)、[定價](https://developers.openai.com/api/docs/pricing) | 查閱版本 | 香港直連限制；Luna 比較價 |
| [Google Gemini 地區](https://ai.google.dev/gemini-api/docs/available-regions)、[定價](https://ai.google.dev/gemini-api/docs/pricing) | 地區頁更新 2026-04-28；定價查閱版本 | Developer API 地區、Flash-Lite及免費資料用途 |
| [AWS 第三方模型條款](https://aws.amazon.com/legal/bedrock/third-party-models/) | OpenAI 部分 June 2026 | Bedrock 不會自動豁免模型地區限制 |
| [AWS Nova Micro 模型卡](https://docs.aws.amazon.com/en_en/bedrock/latest/userguide/model-card-amazon-nova-micro.html)、[成本文章](https://aws.amazon.com/blogs/machine-learning/cost-efficient-custom-text-to-sql-using-amazon-nova-micro-and-amazon-bedrock-on-demand-inference/) | 文章 2026-04-16；模型卡查閱版本 | 候選模型／API；有日期參考費率 |
| [DeepSeek 定價](https://api-docs.deepseek.com/quick_start/pricing/) | 直接頁面查閱版本 | Flash 價格及香港上班繁忙時段 |
| [Ollama macOS](https://docs.ollama.com/macos)、[CLI](https://docs.ollama.com/cli)、[FAQ](https://docs.ollama.com/faq)、[context](https://docs.ollama.com/context-length) | 查閱版本 | 安裝、啟動、離線邊界及context |
| [Qwen 2B](https://ollama.com/library/qwen3.5:2b)、[4B](https://ollama.com/library/qwen3.5:4b)、[9B](https://ollama.com/library/qwen3.5:9b)、[原模型卡](https://huggingface.co/Qwen/Qwen3.5-4B) | 查閱版本 | 包大小、量化及授權；並非機型實測 |
| [Kimi K2.5](https://github.com/MoonshotAI/Kimi-K2.5)、[Kimi-VL](https://github.com/MoonshotAI/Kimi-VL)、[Kimi 定價](https://platform.kimi.ai/docs/pricing/chat) | 查閱版本 | 參數區分及未核實價表 |
| [工業貿易署中小企資料](https://success.tid.gov.hk/tc_chi/aboutus/what_are_sme.html) | 表內統計期 2026-03 | 宏觀背景，並非可服務市場 |
| [Gmail scopes](https://developers.google.com/workspace/gmail/api/auth/scopes)、[Audience](https://support.google.com/cloud/answer/15549945)、[Workspace政策](https://developers.google.com/workspace/workspace-api-user-data-developer-policy) | scopes更新2026-09-10；政策更新2026-09-03 | OAuth／公開驗證／資料用途 |
| [PCPD 模範框架](https://www.pcpd.org.hk/tc_chi/news_events/media_statements/press_20240611.html)、[僱員指引](https://www.pcpd.org.hk/tc_chi/news_events/media_statements/press_20250331.html) | 2024-06-11／2025-03-31 | 公司管治及採購文件 |
| [Microsoft Graph](https://learn.microsoft.com/en-us/graph/permissions-reference#mailread)、[Entra 同意設定](https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/configure-user-consent) | 查閱版本 | 未來M365權限／公司批准 |

產品背景：[AGENTS.md](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/AGENTS.md)、[CLAUDE.md](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/CLAUDE.md)、[電郵工作區](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/EMAIL_WORKSPACE.md)、[歷史原生驗收](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/NATIVE_MAIL_TEST_REPORT.md)、[Business Roadmap](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/BUSINESS_ROADMAP.md)、[Mentorship Notes](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/MENTORSHIP_NOTES.md)、[Gold計劃](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/GOLD_BADGE_CYCLE_PLAN.md)、[舊香港市場策略](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/HK_MARKET_STRATEGY.md)。有日期衝突時以實際程式及今輪官方資料核對。
