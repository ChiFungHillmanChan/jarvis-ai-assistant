# JARVIS 付款、方案及下載版設定報告

核對日期：2026-09-23。付款後端已部署至 AWS sandbox，官網英中付款返回頁已先經預覽驗證，再部署至 `jarvis-automation.com`。原生隔離 QA App 已完成 Stripe 測試付款及自動更新 Pro。公開 v0.4.1 安裝檔並不包含本輪修正；正式收費尚未啟用。首次語言設定、郵件排版、本機儲存及安裝說明見 [產品審查報告](2026-09-23-review-and-installation.md)。

## 已確認的產品決定

- 使用者在 App 按「升級」，由瀏覽器開啟 Stripe Checkout 安全付款頁。返回 App 後重新向伺服器確認方案。
- 目前採用 BYOK。使用者提供自己的 AI API 密鑰，AI 供應商另收用量費；JARVIS 訂閱不包含 AI 額度。
- 先使用 Hillmanchan／Hillman Projects 的 Stripe 帳戶，未來再改用 JARVIS 帳戶。
- 正式價格、Free／Pro 功能差異及 Team 是否推出尚未確定。歷史價格及測試產品並非正式上線批准。

## 現有帳戶與設定

Chrome 已核對 Hillman Projects 帳戶 `acct_1RRkHZJCG4qg3rNc`。測試模式內已有 JARVIS Pro HK$148／月、JARVIS Team HK$298／月。Pro 的價格 ID 為 `price_1TUsAuJCG4qg3rNcZVpOopcD`，與原有程式相符。這些金額只是沿用測試資料，沒有設定 Stripe Entitlements 功能。

Stripe CLI 原先指向另一業務 starlingmind。使用者已完成 Stripe 身分驗證，現在獨立 `jarvis-hillman` 設定已登入上述 Hillman 帳戶。CLI 憑證到期日為 2026-12-22，僅用作管理操作，不會把這種短期 CLI 憑證當成正式後端密鑰。本報告不包含 API 密鑰或登入權杖。

AWS 的 `Jarvis-sandbox-Payment` 已部署，狀態為 `UPDATE_COMPLETE`。公開 API 根網址為 `https://k2jn2wsgee.execute-api.ap-southeast-1.amazonaws.com`；Webhook 為該網址加 `/stripe/webhook`。原有 API 密鑰曾回覆 HTTP 401、`api_key_expired`；使用者明確批准後，已建立受限測試密鑰並安全存入 `jarvis/sandbox/stripe`，保留既有 `webhookSecret`。新密鑰已通過實際 Stripe API 權限檢查及原生 App 付款流程。

已在該帳戶的測試模式完成以下設定：

- 核實 Pro 與 Team 價格均為 active、HKD 月繳；Team ID 為 `price_1TUsAwJCG4qg3rNcPY3YJiOB`。
- 更新既有 Webhook `we_1TqLBwJCG4qg3rNcGq9RLsej` 至後端支援的 10 項事件，保留相同接收網址。此 endpoint 的 `api_version` 為 null，沿用帳戶版本；後端使用固定 API 版本重新讀取當前物件。完整事件格式仍需實際驗收。
- 建立 Portal configuration `bpc_1UIu72JCG4qg3rNcRFPWhof9`，名稱為 `JARVIS sandbox`：支援帳單紀錄、更新付款方式及期末取消；未開啟方案切換。Stripe 回傳 `is_default=true`；JARVIS 後端將明確指定此 ID。
- 已建立 `JARVIS sandbox backend` 受限測試密鑰：Customers／Checkout Sessions／Customer Portal 可寫，Products／Prices／Subscriptions／Invoices 只讀。密鑰沒有寫入程式庫、文件或 App；傳送只經本機臨時表格及記憶體交給指定 AWS secret，臨時服務已關閉。
- 首次部署遇到 API Gateway Stage 早於新 Portal route 更新的依賴次序問題。已恢復 stack，加入明確 route 依賴及回歸測試，再成功部署；沒有更換 Auth／Data 資源。

## 審查發現與修正範圍

| 問題 | 影響 | 本輪處理 |
| --- | --- | --- |
| 發行設定缺少 Cognito domain／client ID | 下載版可能完全沒有可用帳戶入口 | 發行腳本在建置、改版本及上傳前，檢查三項公開設定及網址格式 |
| 付款完成後只能手動重新整理 | App 可能仍顯示 Free | 返回前景立即更新，Checkout 後每 5 秒查詢，最多 2 分鐘；只相信伺服器方案 |
| `/me` 快取沒有期限，更新失敗仍保留舊付費資料 | 取消方案或網絡失敗後可能繼續顯示已確認 Pro | 加入 60 秒快取期限、並行請求保護及失敗清除；保留登入但標示方案未確認 |
| 付費用戶沒有管理訂閱入口 | 無法從 App 管理續費或取消 | 新增經登入驗證的 `/portal` 與 App「管理訂閱」入口 |
| 瀏覽器開啟失敗仍回報成功 | 用戶不知道下一步 | 回傳實際開啟結果並提供重開按鈕；只接受指定 Stripe HTTPS 網址 |
| 重複點擊、跨方案未完成 Checkout | 可能產生重複 customer 或付款工作階段 | 後端以每用戶鎖、持久保存的付款嘗試及 Stripe 冪等處理；切換前處理舊工作階段 |
| Webhook 使用通知內的舊訂閱快照 | 延遲、重複通知可能覆蓋新方案 | 驗證簽章後重新讀取 Stripe 當前訂閱，核對 customer／用戶關係 |
| 測試 Price ID 寫死 | 換帳戶或正式收費容易用錯價格 | 價格改為部署設定；正式環境不能靜默沿用 sandbox ID |
| 官網成功／取消網址是 404 | 付款後沒有清楚返回指引 | 新增英中返回頁；按 App 語言選擇網址，成功頁仍要求 App 確認方案 |
| 登出或換帳戶時舊請求仍可能完成 | 舊付款連結、登入憑證或方案可能回寫到新會話 | 前後端使用會話代數核對，登出先失效；開啟瀏覽器前再次核對登入狀態 |
| Stripe 託管付款及管理頁未指定語言 | 中文 App 開啟英文付款頁 | 新 session 明確傳入 `en`／`zh-HK`；保留舊付款嘗試的原始參數以維持冪等性 |

本輪沒有擅自設定付費功能限制。現有 tier 原本只是顯示 Free／Pro／Team；在產品功能差異落實及驗收前，不應向顧客宣稱 Pro 已解鎖指定功能。原有 Team 測試產品亦不代表團隊協作、席位或共享信箱已完成。

## App 實際付款流程

1. 下載版在建置時帶入公開的 Cognito 登入設定和付款 API 網址。一般使用者不用編輯 `.env`。
2. 用戶在「設定 → 帳戶」登入 JARVIS。Gmail 授權與 JARVIS 訂閱登入是分開的；郵件仍保存在本機工作區。
3. App 把驗證過的登入權杖和所選方案送至 JARVIS 後端。價格及 Stripe customer 由伺服器決定，App 不持有 Stripe 商戶密鑰。
4. 後端建立 Stripe Checkout，App 開啟瀏覽器。付款資料在 Stripe 頁面填寫。
5. Stripe Webhook 通知後端；後端驗證簽章並核對當前訂閱，再更新用戶方案。瀏覽器返回成功頁本身不會授予 Pro。
6. App 返回前景及短暫輪詢時查詢 `/me`。未能確認時顯示待確認，不會把付款頁返回當成付款成功。
7. 重啟 App 後，使用儲存在 macOS 鑰匙圈的登入憑證重新查詢方案。訂閱以 JARVIS 帳戶識別，不靠本機開關保存。

這個架構可用於已下載的 App。實際可用仍取決於該版本已包含帳戶功能、部署網址／Cognito 設定正確、Stripe 密鑰有效、Webhook 已接通，以及上線驗收完成。本輪單元測試不等於正式安裝檔付款成功。

## 正式推出前的接續設定

Hillman CLI 登入、sandbox 密鑰、十項 Webhook 事件、JARVIS 專用 Portal、測試價格對應、Payment stack 及官網返回頁均已設定。不要重複建立另一套密鑰或產品，也不要將 sandbox 公開設定直接當成正式發行設定。

1. 決定正式售價、幣別、月付／年付、Free／Pro 權益及是否推出 Team。現時尚未有可對顧客承諾的付費功能限制。
2. 實作並驗收權益控制；頁面清楚說明 BYOK 不含 AI 用量。
3. 核對 Hillman 正式商戶資料。測試 Checkout 顯示舊商戶名稱 `personal portfolio`，產品描述仍聲稱「All 5 integrations, unlimited voice, automation」；正式收費前要按實際功能更新，不能照搬。共享帳戶名稱可能影響其他業務，本輪沒有擅自更改全帳戶品牌。
4. 在正確商戶的正式模式建立獨立產品、價格、Webhook、受限密鑰和 Portal，部署相應環境。完成失敗付款、延遲／重送、重複點擊、重新訂閱及不同登入狀態的整合驗收；單元測試覆蓋不等於每個情境都已在 Stripe 實際操作。
5. 將正式公開登入設定放入發行環境：`JARVIS_PAYMENT_API`、`JARVIS_COGNITO_DOMAIN`、`JARVIS_COGNITO_CLIENT_ID`。一般用戶不用編輯 `.env`。
6. 建置高於 0.4.1 的版本，Developer ID 簽署及 Apple 公證；驗收正式鑰匙圈登入恢復、全新安裝及舊版更新，再發布 DMG／updater。隔離 QA 使用記憶體憑證，不能代替正式鑰匙圈驗收。

## 未來改用 JARVIS Stripe 帳戶

未有正式付費用戶時，可以先在新帳戶建立產品、價格、Webhook 和 Portal，切換伺服器設定，再完整重測。不要把 Hillman 的 customer／subscription ID 當成新帳戶既有資料。

已有正式訂閱時，必須先安排資料及訂閱遷移，維持原本帳單週期，處理舊訂閱，避免重複收費。Stripe 的帳戶資料複製不會自動複製訂閱、發票及交易；訂閱需要另行重建／遷移。[Stripe 官方資料複製界線](https://support.stripe.com/questions/data-that-can-be-copied-between-stripe-accounts?locale=en-GB)、[訂閱重建說明](https://support.stripe.com/questions/recreate-subscriptions-and-plans-after-moving-customer-data-to-a-new-stripe-account?locale=en-GB)。

## 驗證及未完成項目

| 驗證 | 結果及界線 |
| --- | --- |
| 前端完整測試 | 84 通過，包括帳戶競態、付款狀態、發行設定、語言及郵件排版 |
| Rust 完整測試 | 471 通過、0 失敗、8 略過；包括 Cognito 32 項及付款 10 項 |
| 付款後端 | 41 通過，TypeScript 建置通過；包含冪等恢復、Webhook 競態及 CDK route 依賴 |
| 官網 | 136 測試及 27 路由檢查通過；預覽與正式四個英中返回頁均 HTTP 200、noindex；無語言前綴的 307 轉向保留 query |
| 已部署 API | 匿名 `/me`、`/checkout`、`/portal` 全部 401；Webhook 無／錯誤簽章全部 400；JWT scope、限流及 Lambda 配置已核對 |
| 受限測試密鑰 | 14 項實際 API 操作回傳 200，包括建立／終止 Checkout 及指定 Portal；獨立權限測試 customer 已清除 |
| 原生 Stripe 付款 | 隔離 QA App 登入，開啟 Checkout，官方測試卡付款後自動由 Free 轉 Pro；官網中文成功頁正常 |
| 伺服器方案 | Stripe `livemode=false`、Checkout `complete/paid`；DynamoDB customer／subscription 對應正確、tier=pro、status=active；本次 Webhook 日誌沒有錯誤 |
| Customer Portal | 由 App 開啟，顯示已付款帳單；實際期末取消成功，取消日前仍保持 Pro |
| Stripe 頁面語言 | 修正已部署，重新由 App 開啟 Portal 實際顯示繁體中文；首次付款發生於修正前，新的中文 Checkout session 尚待另行實測 |
| Webhook 重送 | 多個通知並行時出現 `409 billing_busy`，由 Stripe 重試；手動重送 Checkout 完成及訂閱建立事件均回傳 HTTP 200。唯讀核對仍只有一筆測試訂閱，期末取消設定不變 |
| App 重啟 | 關閉隔離 QA App 後，以同一帳戶重新登入，重新取得 Pro；QA 的 MemoryKeychain 不持久保存憑證，這不是正式鑰匙圈自動登入驗收 |

測試付款使用 Stripe 官方 Sandbox 測試卡，沒有實際扣款。[Stripe 測試說明](https://docs.stripe.com/testing)。測試使用既有 JARVIS 登入帳戶連到 sandbox，合成卡及姓名；沒有重新授權 Gmail 或傳送私人郵件。

正式權益／價格、正式商戶設定、正式版本鑰匙圈與安裝更新驗收、新 DMG 發布仍未完成。不能把本輪成功的原生 QA 流程描述為公開 v0.4.1 或正式收費已全面通過，亦不能保證沒有任何 bug。

測試訂閱已設定 2026-10-23 期末取消。立即取消以驗證 Free 降級的操作，曾因缺少該項明確授權而被自動審核拒絕，已向使用者提出確認；確認前不會執行。Stripe Sandbox 沒有實際扣款。
