# JARVIS 香港公司產品改善及平價 AI 報告

JARVIS 最值得優先改善嘅，係令一位唔識 API 嘅香港公司用戶，由開 App 到睇到第一份有原信引用嘅客戶跟進摘要，中間唔使理解三間模型供應商、兩套 Google 連接同本機服務指令。**短期以統一設定入口、完整廣東話介面、可選本機 Qwen 做基礎；面向一般公司嘅長期方案係有限 AI 用量連服務一齊收費，將供應商設定留畀 JARVIS 處理。**

本報告資料核對截至 2026-09-12。程式審核基準係產品 repo `main`、HEAD `6570acf`、版本 `0.2.0`；開始審核時工作目錄乾淨，migration 檔案最高為 V24。呢啲係 repo 狀態，唔代表已安裝 App 或真實資料庫已升級。2026-09-10 嘅 Gmail／GPT 實測另有歷史報告，今次冇重跑真實服務。下文清楚分開「現有能力」「官方資料」「建議／估算」；新增功能均未實作。

配套文件：[逐頁廣東話文案](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/product/2026-09-12-cantonese-copybook.md)、[分階段實作方案](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/superpowers/plans/2026-09-12-hk-activation-local-ai.md)。

## 1. 四個要求嘅具體答案

| 要求 | 建議做法 | 完成後應有嘅體驗 |
|---|---|---|
| 一個入口設定所有必要功能 | 用電郵工作區做首次設定主線，按實際 readiness 顯示下一步；其他 integrations 按需要先連 | 已有 key／Gmail 就保留；用戶唔使揀三個 API，再估邊個設定影響邊頁 |
| 一撳開本機 AI，盡量平 | 沿用 Ollama，按機型只推薦一個經測試嘅 Qwen；補安裝引導、啟動、下載、驗證及套用 | 安裝後可一撳啟動；冇 API key；首次下載大小、進度及剩餘步驟清楚可見 |
| 詳細廣東話版本 | 介面語言、AI 回覆語氣、語音辨識同朗讀聲線分開設定；全流程都有香港用語 | 按鈕、錯誤、權限、更新、空白狀態、無障礙標籤都一致，唔只係 AI 識答廣東話 |
| 賣畀香港公司 | 先服務本身用 Mac + Gmail 嘅小公司客戶負責人，賣可核對嘅跟進成果 | 知道傳咩資料、邊度處理、幾多錢、錯咗點核對、唔用時點撤走 |

「一鍵」應指一個清楚入口同自動處理可代辦嘅設定，唔係零授權。Google 登入、公司管理員批准、macOS 安裝／咪高峰權限及付款，仍要由適當人士確認。下載 3.4GB 模型亦唔會瞬間完成；產品應量度主動操作時間同下載等候時間。

## 2. 現有產品邊度令用戶混亂

| 現有狀態，來自程式審核 | 對用戶嘅影響 | 修正優先度 |
|---|---|---|
| Onboarding 先展示 Gemini／OpenAI／Claude key，再連舊 Assistant Google 路徑 | 以為全部必填；完成導引後新電郵工作區仍可能未連接 | P0 |
| 新 `mail_*` 工作區同 Assistant 嘅模型路由、Google token 分開 | 「已連接」唔等於所有功能都 ready；設定頁需要顯示用途 | P0 |
| Local AI 提供複製 `brew install ollama`／`ollama serve`；下載假設服務已經運行 | 非技術用戶要自己開終端機；唔係一鍵啟動 | P0 |
| 本機模型加入 Assistant chain，唔會自動設定 Email 模式／模型 | 模型下載完，電郵仍可能用另一個供應商或未設定 | P0 |
| `Language` 控制 AI 回覆同 STT，唔會切換全 App UI | 用戶揀廣東話後仲見大量英文，感覺設定冇用 | P0 |
| 新 EmailPage 多數已有廣東話，shell、導引、設定及工具頁仍大量英文 | 信任及易用性未連貫 | P0 |
| 模型目錄有大小錯標、下載完成及已安裝判定可疑 | 可能誤下載過大模型，或者顯示未驗證嘅成功 | P0 |
| 郵件／分組／指示／記憶持久化，分析結果仍係頁面狀態 | 重開要再分析，增加費用及重複工作 | P1 |
| 文字分析、本機 LLM、STT、TTS 係不同部分；部分語音播放路徑未完成 | 唔可以將「有 local AI」宣傳成完整離線廣東話語音 | P1 |

主要檔案：[OnboardingWizard](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/OnboardingWizard.tsx)、[Settings 語言設定](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:893)、[LocalAiSetup](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/components/LocalAiSetup.tsx)、[EmailPage](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/EmailPage.tsx)、[本機檢查](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/system_check.rs)、[模型下載](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/commands/model_pull.rs)。程式審核發現唔等於已重現嘅原生測試結果。

仍可用作展示嘅能力係：同一部 Mac 集中多個 Gmail、手動跨信箱分組、每組指示、用戶確認嘅來源記憶，以及可開返原信嘅分析引用。新分析路徑冇寄信、付款或日曆工具。唔將呢啲講成已經自動完成所有行政、團隊共同信箱、完整備份或跨裝置同步。

## 3. 唔好再叫客戶自己搵一堆 key

第一頁只問「想點用 AI」：

| 路徑 | 客戶要做乜 | JARVIS 要負責乜 | 推出次序 |
|---|---|---|---|
| 本機 AI | 確認下載及所需安裝；連 Gmail | 檢查機型、啟動服務、下載、驗證、揀適合工作量 | 先做好 |
| 自備 AI 帳戶（進階） | 只為所選供應商填一個 key／由公司管理員配置 AWS | 官方開通連結、費用解釋、憑證測試、模式一致性、錯誤下一步 | 保留既有用戶路徑 |
| JARVIS 包 AI 用量 | 登入 JARVIS、接受服務範圍及資料流、連 Gmail | 代管推論服務、收費、額度、支援及供應商資格 | 完成需求／政策／成本驗證後推出 |

第三條先係一般非技術公司最接近「唔使理 API」嘅產品。**唔可以將 founder 嘅 API key 放入桌面 binary**；要由受控後端保存供應商憑證，用短期 App session 驗證用戶，伺服器執行額度及權限檢查。郵件只喺用戶要求分析時傳所需內容；預設日誌唔記正文。呢個後端目前未係產品能力，會增加資料處理責任，唔應只為加個按鈕就草率起服務。

| 設定／憑證 | 正常應由邊個處理 |
|---|---|
| Google OAuth project、client 設定、品牌及公開驗證 | JARVIS 開發／營運者；一般客戶唔使建立 Google Cloud project |
| Gmail 登入及授權 | 信箱用戶；受管帳戶可能要公司管理員批准 |
| 包用量方案嘅模型 API key／AWS role | JARVIS 後端；唔交畀每個終端用戶 |
| BYOK key／公司 AWS | 選擇進階模式嘅用戶／公司 IT；App 經 Keychain 保存憑證 |
| 本機模型 | 唔需要模型 API key；首次需安裝／下載 |
| 咪高峰／語音 | 真正使用語音先要求；文字電郵流程唔應被語音設定阻住 |
| GitHub／Notion／其他工具 | 第一次使用該工具先連接，唔放入核心電郵啟用門檻 |

導引建議順序：語言 → 選處理方式 → 驗證 AI → 連電郵工作區 Gmail → 同步首批 → 用兩三封信建立第一個項目群組 → 整理下一步 → 開原信核對。未願意連真信時提供清楚標示嘅合成示範。只有實際請求成功、有可用結果先叫「可以開始使用」；只儲存 key 應寫「已儲存，未測試」。

## 4. 香港地區係選供應商嘅硬條件

OpenAI 官方 API 支援地區清單喺查閱當日冇列香港，並指出喺清單以外使用或提供服務可能導致停權。因此唔應將香港公司直接申請 OpenAI API 當新客預設。呢個結論唔否定合資格地區既有 GPT 自用流程，亦唔需要為寫報告重設已儲存憑證。[OpenAI 支援地區](https://developers.openai.com/api/docs/supported-countries)

Google AI Studio／Gemini Developer API 官方地區清單亦冇列香港。消費者 Gemini App 能否喺香港用，唔等於 Developer API 開通條件相同；Google 指向嘅企業雲端路線係另一項需驗證嘅服務。[Gemini API 支援地區](https://ai.google.dev/gemini-api/docs/available-regions)

GPT 模型雖已出現喺 Bedrock，但 AWS 公布嘅 OpenAI Services Agreement（June 2026）第 14.13 條仍限制 Customer 同 End Users 嘅使用地區，定義引用 OpenAI 支援清單。**唔將 Bedrock GPT、海外伺服器或 VPN 寫成香港商用保證方案。** OpenAI 開放權重模型有另一套授權，亦唔可同商業 GPT 混淆。[AWS 第三方模型條款](https://aws.amazon.com/legal/bedrock/third-party-models/)

對香港公司，首輪雲端驗證建議先考慮 AWS 自家 Nova 或授權及銷售地區清楚嘅開放權重服務，核對實際 AWS 帳戶、模型、region、資料目的地及公司批准。Bedrock 支援某個模型，唔代表現有 JARVIS 所用 API、憑證或推論區域已測通。Nova Micro 係文字模型，官方列有 Converse；未有足夠證據證明佢嘅廣東話電郵質素達標，應先做候選評測。[Nova Micro 模型卡](https://docs.aws.amazon.com/en_en/bedrock/latest/userguide/model-card-amazon-nova-micro.html)

Kimi／DeepSeek 可列第二輪候選；正式開放前同樣核實帳戶地區、資料處理、商用條款、當售模型同實際 API。唔因為介面相容 OpenAI 就只改 URL；現有 OpenAI 郵件路徑固定官方 endpoint，新增供應商要獨立處理憑證、請求、錯誤及引用驗證。

## 5. 每月究竟可以有幾平

以下係**純文字模型費估算，唔係 JARVIS 售價或實際帳單**。未計 reasoning 額外輸出、重試、多輪對話、語音、附件辨識、稅項、付款費、支援及後端。所有輸入用未命中 cache 價；唔假設有免費額度。美元換港幣用固定 **US$1 = HK$7.80** 作規劃假設，唔係即時匯價。

| 方案 | 每百萬輸入／輸出 token，美元 | 適用性及來源 |
|---|---:|---|
| 本機 Qwen | 雲端 token 費 $0 | 有合適現成電腦先成立；電費、時間、儲存同支援另計 |
| Nova Micro | $0.035／$0.14 | AWS 2026-04-16 官方文章列價作估算；動態價表未取得對應地區數字，正式開通再核實，唔當鎖定現價。[AWS 成本資料](https://aws.amazon.com/blogs/machine-learning/cost-efficient-custom-text-to-sql-using-amazon-nova-micro-and-amazon-bedrock-on-demand-inference/) |
| DeepSeek Flash | 非繁忙 $0.15／$0.60；繁忙 $0.30／$1.20 | 當日直接定價頁；`deepseek-flash`，未接入 JARVIS、未完成香港帳戶驗證。[DeepSeek 定價](https://api-docs.deepseek.com/quick_start/pricing/) |
| GPT-5.6 Luna | $0.20／$1.20 | Standard、短 context；合資格地區比較用，唔係香港直連預設。[OpenAI 定價](https://developers.openai.com/api/docs/pricing) |
| Gemini 3.1 Flash-Lite | $0.25／$1.50 | Standard 文字；合資格 Developer API 地區比較，未接新電郵工作區。[Google 定價](https://ai.google.dev/gemini-api/docs/pricing) |
| Kimi API | 今次未核實確切數字 | 舊頁轉至新平台，動態價表未能完整讀取；唔填舊 K2.5 價格。[Kimi 官方定價](https://platform.kimi.ai/docs/pricing/chat) |

DeepSeek 繁忙時段係星期一至五 UTC 01:00–04:00、06:00–10:00，即香港 09:00–12:00、14:00–18:00，正正覆蓋常見上班時間。香港公司唔應只用夜間平價估算；舊搜尋快照顯示嘅 V4-Flash 及舊價，已被直接頁面嘅 V4.1-Flash／`deepseek-flash` 資料取代。[DeepSeek 當日價表及時段](https://api-docs.deepseek.com/quick_start/pricing/)

成本公式：`次數 ×（每次輸入 token × 輸入單價 + 每次計費輸出 token × 輸出單價）÷ 1,000,000`。

| 每人每月工作量假設 | Nova Micro 參考價 | DeepSeek 非繁忙／繁忙 | GPT Luna | Gemini Flash-Lite |
|---|---:|---:|---:|---:|
| 200 次；每次 10K 輸入 + 1K 輸出 | HK$0.76 | HK$3.28／6.55 | HK$4.99 | HK$6.24 |
| 600 次；每次 10K + 1K | HK$2.29 | HK$9.83／19.66 | HK$14.98 | HK$18.72 |
| 1,000 次；每次 30K + 2K | HK$10.37 | HK$44.46／88.92 | HK$65.52 | HK$81.90 |

同一個便宜模型，讀信次數同長度增大都可以令成本升十幾倍。現有郵件流程有字元上限，**字元唔等於 token**；上表工作量係獨立估算，唔係由 50 封同步上限直接換算。應記錄每次 provider/model、計費 token、費率版本、錯誤重試及工作區用途；本機記錄延遲及記憶體，唔虛構美元費用。

最低成本策略依次係：先縮細送入資料、保存可重用分析、資料有改先重算、限制輸出、按任務選模型、再考慮 cache／batch。新郵件或記憶變更要令舊分析失效或清楚標示過時；唔好為慳錢顯示過期承諾。免費 tier 只作合適示範；Google 定價列明 free／paid 嘅產品改善用途不同，真實公司資料唔應為慳錢就直接送去未確認用途嘅免費服務。[Google 免費及付費資料用途](https://ai.google.dev/gemini-api/docs/pricing)

一個更大成本係支援：假設內部時間 HK$200／小時，一次 30 分鐘設定支援成本已 HK$100；壓到 5 分鐘就約 HK$16.67。呢個係工時計價假設，唔係實際工資。因此減少 API 設定通常比再慳幾蚊 token 更值得優先做。初期唔建議為低用量長開租用 GPU，亦唔建議客戶先買高 RAM Mac。

### 現有 App 嘅最少設定路徑

已經有正常 GPT／Gmail 嘅合資格自用環境，直接沿用現況，唔需要申請三個供應商。電郵工作區嘅「電郵分析設定」揀 OpenAI GPT／已驗證模型，會讀 Settings 儲存嘅 OpenAI key；舊 Assistant 模型選擇唔係呢頁嘅替代。只係真係未有 key 嘅進階用戶先經供應商官方帳戶建立，再親自填入 App，唔將 key 貼入對話。

香港雲端試點如果先驗 Nova Micro，由公司 IT／開發者處理以下路徑，唔把呢份 AWS 工程設定清單當普通客戶嘅最終體驗：

1. 核對公司 AWS 帳戶、付款／credits 適用範圍及模型存取，先喺有支援嘅 region 測合成文字；例如官方模型卡列 `us-east-1` 可用 in-region，資料會去美國，須符合公司選擇。
2. 公司管理員配置只容許所需推論嘅憑證；唔用 root key。現有 JARVIS 欄位係 Access Key ID＋Secret Access Key＋Region，唔係 Bedrock bearer API key 輸入框，唔可以將兩者亂貼。
3. 到 Settings → Models 內嘅 AWS Bedrock 區塊儲存上述資料。現有「Singapore recommended」文案唔適用所有模型，先依實際模型卡選地區。
4. 到電郵工作區 → 電郵分析設定 → 自己嘅 AWS Bedrock，填對應模型 ID，例如 in-region `amazon.nova-micro-v1:0`；睇清楚郵件外傳說明後先保存。
5. 先用合成／明確允許嘅非敏感案例核對金額、否定句、日期及引用，記錄真實帳單。通過後先向 pilot 用戶開放；今輪未做呢項真實 AWS 驗收。

現有介面及資料型別佐證：[AWS 欄位](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/Settings.tsx:840)、[電郵分析設定](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src/pages/EmailPage.tsx:127)。模型／區域依據：[Nova Micro 官方模型卡](https://docs.aws.amazon.com/en_en/bedrock/latest/userguide/model-card-amazon-nova-micro.html)。

## 6. 超平本機方案：Ollama + Qwen

Ollama 官方 macOS 要求係 Sonoma 14 或更新；Apple M 系列支援 CPU／GPU，Intel 為 CPU。首版安裝建議集中 Apple Silicon；Intel 顯示可測試及速度限制，唔承諾同樣體驗。[Ollama macOS 指引](https://docs.ollama.com/macos)

下表係待測試工程建議，唔係官方最低 RAM 或已量度速度。包大小唔等於運行 RAM；macOS、瀏覽器、context cache 同其他程式都要空間。

| 統一記憶體 | 第一個候選 | 查閱時下載包 | 建議起始 context／磁碟餘額 |
|---|---|---:|---|
| 8GB | `qwen3.5:2b`，Q8_0 | 2.7GB | 2K–4K／至少 8GB 可用空間；僅輕量短任務 |
| 16GB | `qwen3.5:4b`，Q4_K_M | 3.4GB | 4K／至少 10GB |
| 24GB | `qwen3.5:9b`，Q4_K_M；重視速度可測 4B | 6.6GB | 4K–8K／至少 15GB |
| 32GB | `qwen3.5:9b`，Q4_K_M | 6.6GB | 8K 起／至少 15GB |

官方 tag／大小：[2B](https://ollama.com/library/qwen3.5:2b)、[4B](https://ollama.com/library/qwen3.5:4b)、[9B](https://ollama.com/library/qwen3.5:9b)。Qwen 官方模型卡標示 Apache-2.0；出貨要記錄原模型授權、量化來源、digest 同測試過嘅 Ollama 版本。[Qwen 4B 模型卡](https://huggingface.co/Qwen/Qwen3.5-4B)

4B 係候選，唔係廣東話質素保證。一般多語言 benchmark 唔足以證明佢讀得啱「唔使付款」「暫定」「改咗下星期」及跨信箱新舊版本。小模型先用短信／少量相關片段；目前 JARVIS 本機 context 路徑最多採 32,768，並保守按 UTF-8 長度檢查。若改成 4K，必須同步縮細分析內容，明示未讀範圍，唔只降低 `num_ctx` 然後照塞所有郵件。[本機 context 程式](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/src-tauri/src/mail/analysis.rs:436)、[Ollama context 文件](https://docs.ollama.com/context-length)

**現階段手動驗證方法**：由官方下載頁安裝 Ollama，開啟 Ollama App；服務已運行就唔再開第二個。16GB Mac 可用下列命令測模型。呢啲係日後驗證教學，今次冇執行或下載。

```sh
ollama pull qwen3.5:4b
ollama run qwen3.5:4b
ollama ps
```

8GB 改用 `qwen3.5:2b`；24／32GB 可測 `qwen3.5:9b`。若版本唔支援模型，先評估官方升級。之後喺 JARVIS **電郵工作區**選本機並填相同 exact tag，先用合成短信測引用，再重開驗證設定。只喺 Assistant 加入 chain 唔等於完成呢一步。[官方下載](https://ollama.com/download/mac)、[Ollama CLI](https://docs.ollama.com/cli)

日後一鍵流程應做到：偵測 → 必要安裝引導 → 啟動／重用 → 下載真實進度 → 驗證成功 → 合成廣東話試用 → 套用到明確工作區。支援中斷重試、磁碟不足、模型不相容、記憶體不足；保留成功步驟，唔要求由頭做起。首次 OS 安裝可保留官方互動，安裝後啟動應唔使終端機。[Ollama pull API](https://raw.githubusercontent.com/ollama/ollama/main/docs/api.md)

本機模式只允許經驗證嘅本機模型及 loopback endpoint，唔將 `:cloud` 或 remote alias 當離線。`OLLAMA_NO_CLOUD=1` 係 Ollama 可用選項，但唔會替 JARVIS 關閉 STT 或其他雲端路徑。離線文案只適用於「已下載內容嘅已驗證文字分析」；Gmail 授權、同步、首次模型下載仍需要網絡。[Ollama FAQ](https://docs.ollama.com/faq)

模型目錄要先修正：`llama3.2:latest` 係 3B／2.0GB，唔係 8B；`llama3.3:latest` 係 70B／43GB，唔係 13B。出貨唔應用易變 `latest` 代替明確大小，再靠手寫標籤估 RAM。[Llama 3.2](https://ollama.com/library/llama3.2)、[Llama 3.3](https://ollama.com/library/llama3.3)

## 7. Kimi 點解唔係普通 Mac 嘅首選

Kimi 係 App／模型系列／雲端 API 嘅品牌，唔係單一細模型。以 Kimi K2.5 為例，官方架構係 1T 總參數、約 32B active。active 只描述每個 token 啟用部分參數，唔代表只需保存嗰部分權重。[Kimi K2.5 官方](https://github.com/MoonshotAI/Kimi-K2.5)

純算術：`1T × 4 bit ÷ 8` 約 **500GB** 權重，未計量化額外資料、runtime 同 cache；呢個唔係官方最低 RAM。Offload／分散部署可以改變所需機型，但唔符合 8–32GB Mac「超平簡單」目標。官方亦有較細嘅 Kimi-VL（16B total／3B active），所以唔應泛稱所有 Kimi 都跑唔到；只係今輪未有合適穩定 Mac tag 及電郵驗收支持選佢做預設。[Kimi-VL 官方](https://github.com/MoonshotAI/Kimi-VL)

查閱日 Ollama K2.5 頁寫明 2026-07-31 已 retired，唔應複製舊文章嘅 `kimi-k2.5:cloud` 教學。Kimi 新平台嘅數字價表今次未核實，正式 API 接入留第二輪，唔用過時價錢做「最平」結論。[Ollama Kimi 頁](https://ollama.com/library/kimi-k2.5)、[Kimi 定價](https://platform.kimi.ai/docs/pricing/chat)

## 8. 廣東話版本要做到乜先算完整

介面使用香港繁體中文，指引可以自然廣東話；對客電郵另有「香港商務書面語」選項。唔應因 UI 係廣東話，就強制所有正式對客內容都用口語。新偏好拆成 UI locale、回覆語氣、STT 語言、TTS 聲線；沿用既有 preference 資料時保留舊設定。

| 範疇 | 必須有嘅內容 |
|---|---|
| 導航／啟用 | 電郵工作區、AI 助理、已連接服務、開始設定、繼續設定、暫時略過 |
| 錯誤 | 說明問題、目前影響、下一步；例如「API key 已儲存，但服務未能使用。請檢查帳戶餘額後再測試。」 |
| 日期／時區 | 用明確年月日及時區；公司可選 `Asia/Hong_Kong`，唔因用戶講廣東話就擅改行事曆時間 |
| 金額 | 保留 HKD／CNY／USD；來源只有 `$` 就唔自行當港幣；萬元、k 同小數要準確 |
| 事實／語氣 | 保留原信引用；分清已確認、待確認、暫定、毋須；唔將推測寫成承諾 |
| 失敗及無障礙 | 表單標籤、ARIA、快捷鍵提示、通知、下載／更新、空白狀態同步翻譯 |
| 語音 | 中英夾雜、香港地名、人名、公司術語分開驗收；文字版本成功唔代表語音完成 |

最低驗收用 20 組合成香港工作郵件，覆蓋否定句、改期、不同貨幣、舊記憶失效、兩個同名客戶、UK／HK 時差、模糊日期、附件缺失、來源引用及郵件注入。每組有人工正確答案；關鍵金額／限期唔容許錯誤斷言，資料不足應明示。小樣本全通過只係 release gate，唔等於真實世界零錯誤。

## 9. 香港公司真正願意付錢嘅場景

工業貿易署 2026 年 3 月資料列有 357,077 間中小企，佔企業逾 98%。但呢個唔係 JARVIS 可服務市場；要再篩 Mac、Gmail、安裝權限、工作方式同預算。資料頁底部仍有較舊覆檢日期，本報告使用表內明示嘅統計期。[工業貿易署中小企統計](https://success.tid.gov.hk/tc_chi/aboutus/what_are_sme.html)

**首批 ICP 假設**：香港 2–15 人創意／營銷／顧問公司，自己用 Mac + Gmail／Google Workspace，同時跟幾個客戶嘅老闆或 account lead。先賣「跨信箱整理客戶改動、待覆事項同已確認背景」。公司大小、頻率及付費意願都要訪談，唔當行業普遍事實。

第一個兩分鐘示範：同一客戶兩個信箱、三次交期變更 → 放入一組 → 加「未正式落單唔當已承諾」指示 → 用戶確認一條來源記憶 → 產出三項下一步 → 點開原信。展示少漏跟及少重讀，唔只展示 AI 聊天。

首輪訪談問實例、設備、信箱、客戶跟進落喺 email 定 WhatsApp、邊個批 AI、而家點做同幾耐。今次冇代表性官方數據證明香港公司嘅 Gmail／M365 或 Mac／Windows 比例，唔沿用舊市場文件嘅桌面市佔率作公司採購比例。

先招現有設備合適嘅 pilot，唔叫公司換系統。記錄 20 位合資格候選中有幾多只因 Outlook／Windows流失；若至少 5 位有真場景同預算但被同一缺口擋住，再做擴展驗證。呢個係內部決策門檻。M365 同 Windows係兩項工作；Graph delegated `Mail.Read` 可以係未來個人信箱起點，共同信箱及租戶管理員批准要另外處理。[Microsoft Graph 權限](https://learn.microsoft.com/en-us/graph/permissions-reference#mailread)、[Microsoft 使用者同意設定](https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/configure-user-consent)

## 10. 公司採購前要補嘅信任及公開驗證

Gmail `readonly` 都係 restricted scope。Google 指出將呢類資料儲存／傳送至伺服器有 security assessment 要求；本機 App 或 BYOK 唔自動豁免雲端分析資料流。Testing 嘅 100 位 test users／7 日授權到期，同未驗證 App lifetime 100 位新用戶上限，係不同限制；轉 production 唔等於完成驗證。[Gmail scopes](https://developers.google.com/workspace/gmail/api/auth/scopes)、[Google Audience 規則](https://support.google.com/cloud/answer/15549945)

Google 政策容許可見嘅生成式電郵摘要，但限制資料用途、轉交及訓練，並有保留／cache／加密要求。需要逐項對照現有本機郵件副本、斷線保留、來源記憶同刪除行為；唔自行斷言所有本機副本違規。SQLite 冇應用層加密，Keychain 保護 key 唔等於郵件庫已加密。最終評估範圍、CASA 費用同時程未核實，唔編固定報價。[Google Workspace 資料政策](https://developers.google.com/workspace/workspace-api-user-data-developer-policy)

PCPD 模範框架同僱員指引可以轉成採購材料：列明工具用途、最少資料、人手核對、權限、事故支援及退出方式。呢個係依指引設計，唔係公署認證或法律合規保證。[PCPD AI 框架](https://www.pcpd.org.hk/tc_chi/news_events/media_statements/press_20240611.html)、[PCPD 僱員指引](https://www.pcpd.org.hk/tc_chi/news_events/media_statements/press_20250331.html)

每間 pilot 公司應收到四份短材料：一頁資料流、安裝及撤走指引、員工可用／不可用案例、供應商及支援範圍。寫清斷開連接保留乜、刪本機帳戶清乜、原 Gmail 會唔會改、模型喺邊處理、保留幾耐，以及點退出。重開、撤權、斷網、更新相容性都屬產品驗收，唔只驗一次成功摘要。

## 11. 定價同銷售實驗

以下係建議測試價，全部未有成交或續費證明，唔係今輪已開通嘅套餐：

| 階段 | 測試內容 | 價格／成本處理 |
|---|---|---|
| 首批 3 人／3 間公司，14 日 | 一個明確工作流程、30 分鐘設定、15 分鐘覆盤 | HK$300／人試點服務；本機無 API 費，或用公司已批且地區合資格嘅 BYOK。雲端另付要先寫明。呢筆唔計 MRR |
| 續費第一批 | 相同核心工作流程，限制支援範圍 | HK$98／人／月，本機／BYOK；記真付款同拒絕原因 |
| 下一批可比客戶 | 同功能同服務量，測價格接受度 | HK$148／人／月；小樣本唔聲稱統計顯著 |
| 包 AI 方案，另一輪 | 解決 key 係主要流失原因時測 | 可先測 HK$148／人／月及明示額度；以每月 200 次、每次10K輸入／1K計費輸出作成本設計起點，產品要處理超長請求，唔寫無限 AI |

唔同成本模式唔應同時對一位客戶反覆改價。包 AI 同 BYOK 售價需分開記錄成本；模型／reasoning／token 長度改變會影響可提供次數。Quota 應有伺服器硬上限、提前提醒同超額停止，唔只係 UI 數字。

先提供 HKD 報價、清楚服務範圍、付款收據及取消方式。公司對賬需要時可評估發票／FPS流程；正式收款、稅務同付款啟用另行處理。唔賣未完成嘅 Team RBAC，唔將一次性 setup 收入寫成 MRR。100 人 × HK$148 = HK$14,800 MRR 係目標算術，唔係預測。

渠道先用 founder 現有公司／HKSTP 人脈及定向示範；記錄每個來源嘅合資格率、完成啟用率同續費。主要銷售句可以係：「將幾個 Gmail 入面嘅客戶郵件放埋一齊，記低你確認過嘅背景，用廣東話整理下一步；每個重點都可以返原信核對。」下面明示目前適用 Mac + Gmail。

## 12. 四星期先驗證一條可售流程

以每星期約 15–20 小時作規劃假設；工程細節見配套方案。Google 審核及客戶管理員批准係外部依賴，唔保證四星期完成。若未達真資料條件，繼續合成測試，唔稱已商用。

| 時間 | 產物 | 決策門檻，全部係目標而非已達結果 |
|---|---|---|
| 第1週 | 20 人篩選、5 次工作流訪談；計時觀察現有設定；確定公司允許嘅 AI 路徑 | 至少5位設備／信箱／權限合適；找出最大3個啟用卡點；政策缺口有責任人 |
| 第2週 | 統一啟用流程及核心廣東話；修錯誤模型目錄；本機啟動最小流程；20組合成驗收 | 5位新測試者至少4位毋須工程師代操作；已有可用AI服務時10分鐘內出首份可核對摘要；模型下載另計 |
| 第3週 | 達條件後啟動3個14日付費pilot，按需要分批；驗斷網／重開／引用 | 至少2人連續3個工作日重用；關鍵數字／限期測試無錯誤斷言 |
| 第4週 | 完成足14日嘅pilot覆盤、續費、支援及成本報告；未滿14日嘅延後覆盤 | 3人最少2人實際續費；每人至少5個同類任務，中位處理時間減少目標20%，包括核對時間 |

用人工基準同難度相近案例輪換先後順序，減少因熟悉郵件而造成嘅假進步。記錄首次有用結果時間、key／Google卡點、每週使用日、修正次數、漏報、token成本、支援分鐘及續費；唔收正文作預設 telemetry。

如果啟用仍失敗，先修設定；如果用完唔重用，先修工作價值；如果只係 key 阻住，再做包 AI；如果設備／信箱唔合，先驗擴平台需求。唔一次過同時做 Windows、Outlook、WhatsApp、S3、多人團隊同新語音引擎。

## 13. 證據範圍同來源索引

今輪交付係研究、文案及實作方案，冇修改 App、API key、Gmail 授權、模型、正式資料庫或公開網站。程式結論來自只讀審核；硬件容量、價格試驗、工時同商業目標均有標示假設。冇將舊 roadmap、founder 自用或單元測試數目當市場驗證。

所有網頁查閱日為 2026-09-12；冇發布日期嘅頁面只按查閱日記錄。主要來源如下，精確用途已喺正文對應段落引用：

| 發布者／資料 | 日期或版本 | 本報告用途 |
|---|---|---|
| [OpenAI API 支援地區](https://developers.openai.com/api/docs/supported-countries)、[定價](https://developers.openai.com/api/docs/pricing) | 查閱版本 | 香港直連限制；Luna 比較價 |
| [Google Gemini 地區](https://ai.google.dev/gemini-api/docs/available-regions)、[定價](https://ai.google.dev/gemini-api/docs/pricing) | 地區頁更新 2026-04-28；定價查閱版本 | Developer API 地區、Flash-Lite及免費資料用途 |
| [AWS 第三方模型條款](https://aws.amazon.com/legal/bedrock/third-party-models/) | OpenAI 部分 June 2026 | Bedrock唔自動豁免模型地區限制 |
| [AWS Nova Micro 模型卡](https://docs.aws.amazon.com/en_en/bedrock/latest/userguide/model-card-amazon-nova-micro.html)、[成本文章](https://aws.amazon.com/blogs/machine-learning/cost-efficient-custom-text-to-sql-using-amazon-nova-micro-and-amazon-bedrock-on-demand-inference/) | 文章 2026-04-16；模型卡查閱版本 | 候選模型／API；有日期參考費率 |
| [DeepSeek 定價](https://api-docs.deepseek.com/quick_start/pricing/) | 直接頁面查閱版本 | Flash 價格及香港上班繁忙時段 |
| [Ollama macOS](https://docs.ollama.com/macos)、[CLI](https://docs.ollama.com/cli)、[FAQ](https://docs.ollama.com/faq)、[context](https://docs.ollama.com/context-length) | 查閱版本 | 安裝、啟動、離線邊界及context |
| [Qwen 2B](https://ollama.com/library/qwen3.5:2b)、[4B](https://ollama.com/library/qwen3.5:4b)、[9B](https://ollama.com/library/qwen3.5:9b)、[原模型卡](https://huggingface.co/Qwen/Qwen3.5-4B) | 查閱版本 | 包大小、量化及授權；唔係機型實測 |
| [Kimi K2.5](https://github.com/MoonshotAI/Kimi-K2.5)、[Kimi-VL](https://github.com/MoonshotAI/Kimi-VL)、[Kimi 定價](https://platform.kimi.ai/docs/pricing/chat) | 查閱版本 | 參數區分及未核實價表 |
| [工業貿易署中小企資料](https://success.tid.gov.hk/tc_chi/aboutus/what_are_sme.html) | 表內統計期 2026-03 | 宏觀背景，唔係可服務市場 |
| [Gmail scopes](https://developers.google.com/workspace/gmail/api/auth/scopes)、[Audience](https://support.google.com/cloud/answer/15549945)、[Workspace政策](https://developers.google.com/workspace/workspace-api-user-data-developer-policy) | scopes更新2026-09-10；政策更新2026-09-03 | OAuth／公開驗證／資料用途 |
| [PCPD 模範框架](https://www.pcpd.org.hk/tc_chi/news_events/media_statements/press_20240611.html)、[僱員指引](https://www.pcpd.org.hk/tc_chi/news_events/media_statements/press_20250331.html) | 2024-06-11／2025-03-31 | 公司管治及採購文件 |
| [Microsoft Graph](https://learn.microsoft.com/en-us/graph/permissions-reference#mailread)、[Entra 同意設定](https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/configure-user-consent) | 查閱版本 | 未來M365權限／公司批准 |

產品背景：[AGENTS.md](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/AGENTS.md)、[CLAUDE.md](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/CLAUDE.md)、[電郵工作區](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/EMAIL_WORKSPACE.md)、[歷史原生驗收](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/NATIVE_MAIL_TEST_REPORT.md)、[Business Roadmap](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/BUSINESS_ROADMAP.md)、[Mentorship Notes](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/MENTORSHIP_NOTES.md)、[Gold計劃](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/GOLD_BADGE_CYCLE_PLAN.md)、[舊香港市場策略](/Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant/docs/business/HK_MARKET_STRATEGY.md)。有日期衝突時以實際程式及今輪官方資料核對。
