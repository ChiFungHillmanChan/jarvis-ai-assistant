# GitHub、Notion、Obsidian 連接指引

更新日期：2026-09-12。本文件採用繁體書面語，取代先前介面的口語文案規範。

本輪目標是讓使用者從「連接服務」或設定頁開始連接，並在同一位置查看狀態、開啟服務及中斷連接。第一次登入仍須由使用者在供應商頁面確認；按鈕不會代替使用者同意授權。

## 使用方式

| 服務 | 主要按鈕 | 連接後可用功能 | 首次所需操作 |
|---|---|---|---|
| GitHub | 連接 GitHub | 同步指派項目及審查請求、沿用現有聊天工具 | 在 GitHub 輸入畫面上的驗證碼並確認權限 |
| Notion | 連接 Notion | 搜尋、讀取頁面及同步頁面標題與來源資料 | 在 Notion 官方頁面選擇工作區並確認授權 |
| Obsidian | 選取資料庫並允許存取 | 搜尋及讀取本機 Markdown 筆記 | 在 macOS 資料夾選擇視窗指定現有資料庫 |

GitHub 和 Notion 工作頁也有連接入口。「開啟 GitHub」、「開啟 Notion」及「開啟 Obsidian」可以直接前往服務；開啟應用程式與授權 JARVIS 是不同操作。

### GitHub：產品開發者只需預先設定一次

GitHub 的 Device Flow 需要專屬應用程式的公開 Client ID。JARVIS 不會借用其他工具的 Client ID、讀取 GitHub CLI 的登入權杖，或將 Client Secret 加入桌面程式。

1. 登入持有 JARVIS 應用程式的 GitHub 帳戶，前往 [New OAuth App](https://github.com/settings/applications/new)。
2. 填寫應用程式名稱 `JARVIS` 及公司網站。登記表要求的 callback URL 可使用 `http://127.0.0.1`；本輪 Device Flow 不使用此回呼交換授權碼。
3. 在該應用程式設定啟用 **Device Flow**。
4. 將公開 **Client ID** 填入 JARVIS 顯示的授權設定欄位。不要填 Client Secret 或個人存取權杖。
5. 正式建置時，以 `JARVIS_GITHUB_CLIENT_ID` 環境變數提供同一公開識別碼，讓發行版預先包含設定。下載 App 的使用者便只需按「連接 GitHub」。

```sh
JARVIS_GITHUB_CLIENT_ID='你的公開ClientID' npm run tauri build
```

本輪 OAuth 申請 `repo read:user`，涵蓋個人資料，以及公開和私人儲存庫的廣泛讀寫權限，不能標示為唯讀。公司 GitHub 組織可能需要管理員批准或 SSO 授權。若未來銷售對象要求逐一選取儲存庫及更細緻的權限，可另行改用 GitHub App；這不是目前已完成的功能。

JARVIS 會顯示驗證碼、開啟官方裝置授權頁，並等待確認；支援取消、逾時、GitHub 要求降低輪詢頻率，以及過期權杖更新。憑證保存在 macOS 鑰匙圈。切換帳戶後需要重新同步，以免混入先前帳戶的項目。

### Notion：使用官方授權，毋須自行取得 API key

JARVIS 透過 Notion 官方 MCP 服務完成公開客戶端登記、PKCE 授權及權杖更新。使用者不需要建立傳統 REST integration，亦不需要複製 API key。

Notion 授權本身可能涵蓋與登入使用者相同的工作區存取權；本輪 JARVIS 的新 MCP 連接僅開放搜尋及讀取，不提供新增、修改或刪除頁面的 MCP 操作。供應商的授權範圍與 JARVIS 實際提供的操作需要分別說明。

同步最多保留一次搜尋回傳的 50 個頁面項目，不是完整工作區備份。頁面標題及來源資料可在 Notion 頁查看；聊天讀取頁面時才取得正文。實際工具可用性與搜尋能力受 Notion 工作區方案及管理員設定影響。

原有 REST 權杖設定保留在「進階」選項。REST 權杖與 MCP 權杖分開儲存及使用，兩者不能混用。

### Obsidian：本機資料夾授權，不是 OAuth

選取資料庫根目錄後，JARVIS 驗證其中存在 `.obsidian` 資料夾，並限定讀取該根目錄內的 Markdown 筆記。隱藏資料夾、設定檔、越界符號連結及不合條件的檔案不在存取範圍內。此模式拒絕寫入，不需要 Local REST API 外掛。

每篇筆記上限為 1 MiB；每次掃描最多 10,000 個項目、搜尋最多讀取 16 MiB 文字，結果最多 50 項。資料庫移動或替換後需要重新選取。

「開啟 Obsidian」使用官方 URI。若有多個同名資料庫，Obsidian 的名稱選擇可能有歧義；JARVIS 的讀取授權仍綁定使用者實際選定的資料夾。未安裝 Obsidian 時會顯示開啟失敗，使用者需要先安裝應用程式。

原有 Local REST API 模式保留於「進階」設定，需要外掛 API key，且只接受本機端點。

## 狀態與資料處理

- 「尚未連接」表示沒有啟用的連接。
- 「已儲存 · 尚未驗證」不等於服務已可用；可按「檢查連接」取得結果。
- 「已驗證連接」僅代表當次連接或授權檢查成功，不保證每個儲存庫、頁面或模型都可使用。
- 中斷連接會停止 JARVIS 的後續存取；供應商帳戶中的授權可另外撤銷。原始 Obsidian 筆記會保留；Notion 的本機頁面索引會清除，以避免保留上一帳戶的頁面資料。GitHub 中斷連接時保留本機項目，重新授權或更換權杖時會清除舊項目，再由使用者重新同步。
- 使用雲端 AI 處理筆記或頁面時，所需內容會傳送至所選 AI 供應商。本機 Obsidian 授權不等於 AI 推論離線。
- 現有 GPT 模型、OpenAI API key、Gmail 帳戶及正式郵件資料不應因本輪更新而重設。

## 驗收範圍

程式、合成 HTTP 回應及隔離本機資料庫測試，與真實帳戶授權驗收是不同證據。具體結果見[本輪驗收紀錄](2026-09-12-connections-verification.md)；沒有實際完成的 GitHub／Notion 使用者授權，不應寫成已成功登入。

GitHub 專用 Client ID 尚未提供時，一鍵授權仍有開發者設定步驟待完成。不要用其他應用程式的 Client ID 掩蓋此缺項。

## 官方依據

- [GitHub OAuth App 授權及 Device Flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps)
- [GitHub OAuth scopes](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/scopes-for-oauth-apps)
- [Notion 自訂 MCP client、PKCE 與權杖更新](https://developers.notion.com/guides/mcp/build-mcp-client)
- [Notion MCP 支援工具及方案差異](https://developers.notion.com/guides/mcp/mcp-supported-tools)
- [Notion MCP 存取權限](https://developers.notion.com/guides/mcp/mcp-security-best-practices)
- [Obsidian URI](https://obsidian.md/help/uri)
- [Obsidian 資料儲存方式](https://obsidian.md/help/Files+and+folders/How+Obsidian+stores+data)
- [Local REST API 外掛官方 OpenAPI](https://github.com/coddingtonbear/obsidian-local-rest-api/blob/main/docs/openapi.yaml)
