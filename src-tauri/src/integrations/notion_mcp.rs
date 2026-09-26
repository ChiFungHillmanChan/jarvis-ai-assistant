//! Notion's official hosted MCP connection. Credentials never enter the renderer.
use crate::{db::Database, security::keychain::KeychainStore};
use oauth2::{CsrfToken, PkceCodeChallenge};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const ORIGIN: &str = "https://mcp.notion.com";
const ENDPOINT: &str = "https://mcp.notion.com/mcp";
const SERVICE: &str = "com.jarvis.notion";
const ACCOUNT: &str = "mcp_oauth";
const METHOD_KEY: &str = "notion_connection_method";
const MAX_RESPONSE: usize = 2 * 1024 * 1024;
static TOKEN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static FLOW: LazyLock<Mutex<Option<(String, CancellationToken)>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Serialize)]
pub struct NotionConnectionStatus {
    pub status: String,
    pub identity: Option<String>,
    pub method: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct TokenBundle {
    client_id: String,
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    workspace_id: String,
    identity: String,
}

fn client() -> Result<Client, String> {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent("JARVIS-Desktop/0.3.0")
        .build()
        .map_err(|_| "無法建立 Notion 安全連線。".into())
}

async fn body(mut response: reqwest::Response) -> Result<String, String> {
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 | 403 => ACCESS_DENIED.into(),
            429 => "Notion 請求過於頻繁，請稍後再試。".into(),
            _ => format!(
                "Notion 服務暫時無法完成請求（HTTP {}）。",
                response.status().as_u16()
            ),
        });
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "無法接收 Notion 回應。")?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err("Notion 回應超過大小限制，請縮小查詢範圍。".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| "Notion 回應不是有效文字。".into())
}

async fn response_json(response: reqwest::Response) -> Result<Value, String> {
    serde_json::from_str(&body(response).await?).map_err(|_| "Notion 回應格式無效。".into())
}

fn parse_callback(request: &str, state: &str, host: &str) -> Option<Result<String, String>> {
    let mut parts = request.lines().next()?.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    if !target.starts_with("/notion/callback?") {
        return None;
    }
    let hosts: Vec<_> = request
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
        .collect();
    if hosts.len() != 1 || hosts[0].1.trim() != host {
        return None;
    }
    let url = Url::parse(&format!("http://{host}{target}")).ok()?;
    if url.path() != "/notion/callback" {
        return None;
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    let field = |name: &str| -> Option<String> {
        let found: Vec<_> = pairs.iter().filter(|(key, _)| key == name).collect();
        if found.len() == 1 {
            Some(found[0].1.to_string())
        } else {
            None
        }
    };
    let actual = field("state")?;
    if actual.len() != state.len()
        || actual
            .bytes()
            .zip(state.bytes())
            .fold(0, |a, (x, y)| a | (x ^ y))
            != 0
    {
        return None;
    }
    if field("error").is_some() {
        return Some(Err("Notion 授權已取消或被拒絕。".into()));
    }
    let code = field("code")?;
    if code.is_empty() || code.len() > 4096 {
        return None;
    }
    Some(Ok(code))
}

fn rpc_result(raw: &str, id: u64) -> Result<Value, String> {
    rpc_result_optional(raw, id)?.ok_or_else(|| "Notion 未回傳相符的操作結果。".into())
}

fn rpc_result_optional(raw: &str, id: u64) -> Result<Option<Value>, String> {
    let mut messages = Vec::new();
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        messages.push(value);
    } else {
        // Streamable HTTP can return SSE even for a single request. Only the
        // matching JSON-RPC response is accepted; events and notifications are not results.
        for event in raw.replace("\r\n", "\n").split("\n\n") {
            let data = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                .collect::<Vec<_>>()
                .join("\n");
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                messages.push(value);
            }
        }
    }
    for value in messages {
        if value["jsonrpc"] != "2.0" || value["id"].as_u64() != Some(id) {
            continue;
        }
        if value.get("error").is_some() {
            return Err("Notion 無法執行此操作，請確認工作區權限及可用功能。".into());
        }
        return value
            .get("result")
            .cloned()
            .map(Some)
            .ok_or_else(|| "Notion 缺少操作結果。".into());
    }
    Ok(None)
}

async fn rpc_response(mut response: reqwest::Response, id: u64) -> Result<Value, String> {
    if !response.status().is_success() {
        return rpc_result(&body(response).await?, id);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "無法接收 Notion MCP 回應。")?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err("Notion MCP 回應超過大小限制。".into());
        }
        bytes.extend_from_slice(&chunk);
        if let Ok(raw) = std::str::from_utf8(&bytes) {
            if let Some(result) = rpc_result_optional(raw, id)? {
                return Ok(result);
            }
        }
    }
    rpc_result(
        std::str::from_utf8(&bytes).map_err(|_| "Notion MCP 回應不是有效文字。")?,
        id,
    )
}

fn allowed_tool(name: &str) -> bool {
    matches!(
        name,
        "notion-search" | "notion-ai-search" | "notion-fetch" | "notion-list-recent-pages"
    )
}

struct McpClient {
    http: Client,
    endpoint: String,
    token: String,
    session: Option<String>,
    id: u64,
    protocol: String,
    tools: HashMap<String, Value>,
}

impl McpClient {
    async fn connect(token: &str) -> Result<Self, String> {
        Self::connect_to(token, ENDPOINT).await
    }

    // Only the production constant above and local protocol fixtures use this
    // private constructor. No renderer command accepts a remote MCP URL.
    async fn connect_to(token: &str, endpoint: &str) -> Result<Self, String> {
        let mut connection = Self {
            http: client()?,
            endpoint: endpoint.into(),
            token: token.into(),
            session: None,
            id: 0,
            protocol: "2025-06-18".into(),
            tools: HashMap::new(),
        };
        let initialized = connection.rpc("initialize", json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"JARVIS","version":"0.3.0"}})).await?;
        let protocol = initialized["protocolVersion"]
            .as_str()
            .ok_or("Notion 未提供 MCP 協定版本。")?;
        if !matches!(
            protocol,
            "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25" | "2026-07-28"
        ) {
            return Err("Notion 使用尚未支援的 MCP 協定版本，請更新 JARVIS。".into());
        }
        connection.protocol = protocol.into();
        let request =
            connection.request(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        let reply = request
            .send()
            .await
            .map_err(|_| "無法完成 Notion 連線初始化。")?;
        body(reply).await?;
        let mut cursor: Option<String> = None;
        for _ in 0..4 {
            let params = cursor
                .as_ref()
                .map(|cursor| json!({"cursor":cursor}))
                .unwrap_or(json!({}));
            let listed = connection.rpc("tools/list", params).await?;
            let tools = listed["tools"]
                .as_array()
                .ok_or("Notion 未提供可用工具清單。")?;
            for tool in tools {
                if let Some(name) = tool["name"].as_str().filter(|name| allowed_tool(name)) {
                    connection
                        .tools
                        .insert(name.into(), tool["inputSchema"].clone());
                }
            }
            cursor = listed["nextCursor"]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        if !connection.tools.contains_key("notion-fetch") {
            return Err("此 Notion 連接未開放讀取工具，請檢查工作區設定。".into());
        }
        Ok(connection)
    }

    fn request(&self, payload: Value) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.token)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", &self.protocol)
            .json(&payload);
        if let Some(session) = &self.session {
            request = request.header("Mcp-Session-Id", session);
        }
        request
    }

    async fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.id += 1;
        let reply = self
            .request(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| "無法連線至 Notion，請檢查網絡後重試。")?;
        if let Some(session) = reply
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
        {
            if session.len() <= 1024 {
                self.session = Some(session.into());
            }
        }
        rpc_response(reply, self.id).await
    }

    async fn call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        if !allowed_tool(name) {
            return Err("此 Notion 操作不在 JARVIS 的允許範圍內。".into());
        }
        if !self.tools.contains_key(name) {
            return Err("此 Notion 工作區尚未開放所需工具。".into());
        }
        let reply = self
            .rpc("tools/call", json!({"name":name,"arguments":arguments}))
            .await?;
        if reply["isError"].as_bool() == Some(true) {
            return Err("Notion 無法完成操作。請確認授權、目標頁面及工作區方案。".into());
        }
        tool_payload(&reply)
    }
}

fn tool_payload(reply: &Value) -> Result<Value, String> {
    if let Some(structured) = reply.get("structuredContent").filter(|v| v.is_object()) {
        return Ok(structured.clone());
    }
    let text = reply["content"]
        .as_array()
        .ok_or("Notion 未提供可讀取的內容。")?
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return Err("Notion 回傳空白內容。".into());
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

pub fn method(db: &Database) -> Result<Option<String>, String> {
    use rusqlite::OptionalExtension;
    db.conn
        .lock()
        .map_err(|_| "無法讀取 Notion 連接設定。")?
        .query_row(
            "SELECT value FROM user_preferences WHERE key = ?1",
            [METHOD_KEY],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "無法讀取 Notion 連接設定。".into())
}

pub fn generation(db: &Database) -> Result<Option<String>, String> {
    use rusqlite::OptionalExtension;
    db.conn
        .lock()
        .map_err(|_| "無法讀取 Notion 連接狀態。")?
        .query_row(
            "SELECT value FROM user_preferences WHERE key='notion_connection_generation'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "無法讀取 Notion 連接狀態。".into())
}

pub(super) fn ensure_generation(db: &Database, expected: &Option<String>) -> Result<(), String> {
    if &generation(db)? != expected {
        return Err("Notion 連接已變更，已停止傳回先前連接的內容。".into());
    }
    Ok(())
}

pub fn set_method(db: &Database, method: &str, clear_pages: bool) -> Result<(), String> {
    let mut conn = db.conn.lock().map_err(|_| "無法儲存 Notion 連接設定。")?;
    let tx = conn
        .transaction()
        .map_err(|_| "無法儲存 Notion 連接設定。")?;
    tx.execute("INSERT INTO user_preferences(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [METHOD_KEY, method])
        .map_err(|_| "無法儲存 Notion 連接設定。")?;
    tx.execute("INSERT INTO user_preferences(key,value) VALUES('notion_connection_generation',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [uuid::Uuid::new_v4().to_string()])
        .map_err(|_| "無法儲存 Notion 連接設定。")?;
    if clear_pages {
        tx.execute("DELETE FROM notion_pages", [])
            .map_err(|_| "無法重設 Notion 頁面索引。")?;
    }
    tx.commit().map_err(|_| "無法儲存 Notion 連接設定。".into())
}

fn load_bundle() -> Result<Option<TokenBundle>, String> {
    match KeychainStore::get(SERVICE, ACCOUNT) {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|_| "Notion 授權資料無效，請重新連接。".into()),
        Err(error) if KeychainStore::is_not_found(&error) => Ok(None),
        Err(_) => Err("無法從鑰匙圈讀取 Notion 授權，請解鎖鑰匙圈後重試。".into()),
    }
}

fn store_bundle(bundle: &TokenBundle) -> Result<(), String> {
    let raw = serde_json::to_string(bundle).map_err(|_| "無法準備 Notion 授權資料。")?;
    KeychainStore::put(SERVICE, ACCOUNT, &raw)
        .map_err(|_| "無法將 Notion 授權安全儲存至鑰匙圈。".into())
}

fn apply_tokens(bundle: &mut TokenBundle, value: &Value) -> Result<(), String> {
    let access = value["access_token"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 16384 && !v.chars().any(char::is_whitespace))
        .ok_or("Notion 未提供有效的存取權杖。")?;
    if value["token_type"]
        .as_str()
        .map(|v| !v.eq_ignore_ascii_case("bearer"))
        .unwrap_or(false)
    {
        return Err("Notion 權杖類型尚未支援。".into());
    }
    let expires = value["expires_in"]
        .as_i64()
        .filter(|v| *v > 0 && *v <= 31_536_000)
        .ok_or("Notion 未提供有效的授權有效期。")?;
    let refresh = value["refresh_token"]
        .as_str()
        .filter(|v| !v.is_empty())
        .unwrap_or(&bundle.refresh_token);
    if refresh.is_empty() || refresh.len() > 16384 {
        return Err("Notion 未提供有效的更新權杖。".into());
    }
    bundle.refresh_token = refresh.into();
    bundle.access_token = access.into();
    bundle.expires_at = chrono::Utc::now().timestamp() + expires;
    Ok(())
}

/// Only a refresh token Notion rejected means the grant is gone. A rate limit
/// or an outage leaves the stored bundle valid, and the next call retries.
fn refresh_failure(status: u16) -> String {
    match status {
        400 | 401 | 403 => "Notion 授權已失效，請重新連接。".into(),
        429 => "Notion 請求過於頻繁，請稍後再試。".into(),
        _ => format!("Notion 暫時無法更新授權（HTTP {status}），請稍後再試。"),
    }
}

/// `force` refreshes even before the stored expiry: Notion can reject an
/// access token early, and one refresh is cheaper than a reconnect.
async fn access_token(db: &Database, force: bool) -> Result<String, String> {
    let _guard = TOKEN_LOCK.lock().await;
    if method(db)?.as_deref() != Some("mcp") {
        return Err("Notion OAuth 連接已解除。".into());
    }
    let mut bundle = load_bundle()?.ok_or("尚未連接 Notion，請先完成授權。")?;
    if force || bundle.expires_at <= chrono::Utc::now().timestamp() + 60 {
        let response = client()?
            .post(format!("{ORIGIN}/token"))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", bundle.client_id.as_str()),
                ("refresh_token", bundle.refresh_token.as_str()),
                ("resource", ENDPOINT),
            ])
            .send()
            .await
            .map_err(|_| "無法更新 Notion 授權，請檢查網絡後重試。")?;
        if !response.status().is_success() {
            return Err(refresh_failure(response.status().as_u16()));
        }
        apply_tokens(&mut bundle, &response_json(response).await?)?;
        store_bundle(&bundle)?;
    }
    Ok(bundle.access_token)
}

/// What `body` returns for a 401/403 from the MCP endpoint.
const ACCESS_DENIED: &str = "Notion 拒絕存取。請重新授權，或確認工作區權限。";

/// Connect with the stored token. If Notion rejects it before its stated
/// expiry, refresh once and retry before the user is told to reconnect.
async fn open(db: &Database) -> Result<McpClient, String> {
    match McpClient::connect(&access_token(db, false).await?).await {
        Err(error) if error == ACCESS_DENIED => {
            McpClient::connect(&access_token(db, true).await?).await
        }
        connected => connected,
    }
}

fn self_identity(value: &Value) -> Result<(String, String), String> {
    let info = &value["self"];
    let id = info["workspace"]["id"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("Notion 未能確認已連接的工作區。")?;
    let name = info["workspace"]["name"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("Notion 未能確認工作區名稱。")?;
    let user = info["user"]["name"].as_str().filter(|v| !v.is_empty());
    if name.chars().count() > 200 || user.map(|v| v.chars().count() > 200).unwrap_or(false) {
        return Err("Notion 身分資料超過長度限制。".into());
    }
    Ok((
        id.into(),
        user.map(|user| format!("{name} · {user}"))
            .unwrap_or_else(|| name.into()),
    ))
}

pub async fn connection_status(db: &Database) -> Result<NotionConnectionStatus, String> {
    let selected = method(db)?;
    let initial_generation = generation(db)?;
    let mut status = NotionConnectionStatus {
        status: "not_connected".into(),
        identity: None,
        method: selected.clone(),
        message: None,
    };
    if selected.as_deref() == Some("disconnected") {
        status.method = None;
        return Ok(status);
    }
    if !matches!(selected.as_deref(), None | Some("token") | Some("mcp")) {
        return Err("Notion 連接設定無效，請重新連接。".into());
    }
    if selected.as_deref() != Some("mcp") {
        if crate::integration_keys::resolve("notion_api_key").is_some() {
            status.status = "configured".into();
            status.method = Some("token".into());
            status.message = Some("已儲存進階 API 權杖；尚未驗證連接。".into());
        }
        return Ok(status);
    }
    status.status = "configured".into();
    let checked = async {
        let mut connection = open(db).await?;
        self_identity(
            &connection
                .call("notion-fetch", json!({"id":"self"}))
                .await?,
        )
    }
    .await;
    match checked {
        Ok((_, identity)) => {
            status.status = "connected".into();
            status.identity = Some(identity);
        }
        Err(error) => status.message = Some(error),
    }
    if generation(db)? != initial_generation {
        return Err("Notion 連接已變更，請重新檢查狀態。".into());
    }
    Ok(status)
}

struct FlowGuard(String);
impl Drop for FlowGuard {
    fn drop(&mut self) {
        if let Ok(mut pending) = FLOW.lock() {
            if pending
                .as_ref()
                .map(|(id, _)| id == &self.0)
                .unwrap_or(false)
            {
                *pending = None;
            }
        }
    }
}

pub fn cancel_connect() -> Result<(), String> {
    if let Some((_, cancel)) = FLOW.lock().map_err(|_| "無法取消 Notion 連接。")?.as_ref() {
        cancel.cancel();
    }
    Ok(())
}

pub async fn connect(db: &Database) -> Result<NotionConnectionStatus, String> {
    if cfg!(feature = "native-qa") {
        return Err("隔離測試模式不會啟動真實 Notion 授權。".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    {
        let mut pending = FLOW.lock().map_err(|_| "無法開始 Notion 連接。")?;
        if pending.is_some() {
            return Err("Notion 授權正在進行，請先完成或取消。".into());
        }
        *pending = Some((id.clone(), cancel.clone()));
    }
    let _guard = FlowGuard(id);
    tokio::select! {
        _ = cancel.cancelled() => Err("Notion 授權已取消。".into()),
        result = tokio::time::timeout(Duration::from_secs(180), connect_inner(db, &cancel)) => result.map_err(|_| "Notion 授權已逾時，請重新連接。".to_string())?,
    }
}

async fn connect_inner(
    db: &Database,
    cancel: &CancellationToken,
) -> Result<NotionConnectionStatus, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "無法建立 Notion 本機授權回呼。")?;
    let host = listener
        .local_addr()
        .map_err(|_| "無法讀取本機授權位址。")?
        .to_string();
    let redirect = format!("http://{host}/notion/callback");
    let http = client()?;
    let metadata = response_json(
        http.get(format!("{ORIGIN}/.well-known/oauth-authorization-server"))
            .send()
            .await
            .map_err(|_| "無法取得 Notion 授權設定。")?,
    )
    .await?;
    if metadata["issuer"] != ORIGIN
        || metadata["authorization_endpoint"] != format!("{ORIGIN}/authorize")
        || metadata["token_endpoint"] != format!("{ORIGIN}/token")
        || metadata["registration_endpoint"] != format!("{ORIGIN}/register")
        || !metadata["code_challenge_methods_supported"]
            .as_array()
            .map(|v| v.contains(&json!("S256")))
            .unwrap_or(false)
        || !metadata["token_endpoint_auth_methods_supported"]
            .as_array()
            .map(|v| v.contains(&json!("none")))
            .unwrap_or(false)
    {
        return Err("Notion 授權設定已變更，請更新 JARVIS 後重試。".into());
    }
    let registration = response_json(http.post(format!("{ORIGIN}/register"))
        .json(&json!({"client_name":"JARVIS Desktop", "redirect_uris":[redirect], "grant_types":["authorization_code","refresh_token"], "response_types":["code"], "token_endpoint_auth_method":"none"}))
        .send().await.map_err(|_| "無法登記 Notion 桌面連接。")?).await?;
    let client_id = registration["client_id"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 2048)
        .ok_or("Notion 未提供客戶端識別碼。")?;
    let state = CsrfToken::new_random();
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut url =
        Url::parse(&format!("{ORIGIN}/authorize")).map_err(|_| "Notion 授權網址無效。")?;
    url.query_pairs_mut().extend_pairs(&[
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", &redirect),
        ("scope", "default"),
        ("state", state.secret()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("resource", ENDPOINT),
    ]);
    open::that(url.as_str()).map_err(|_| "無法開啟瀏覽器進行 Notion 授權。")?;
    let code = loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|_| "Notion 本機授權回呼失敗。")?;
        let mut request = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(3), async {
            let mut chunk = [0u8; 1024];
            while request.len() < 8192 {
                let count = stream.read(&mut chunk).await?;
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|v| v == b"\r\n\r\n") {
                    break;
                }
            }
            Ok::<(), std::io::Error>(())
        })
        .await;
        if !matches!(read, Ok(Ok(()))) {
            continue;
        }
        let result = std::str::from_utf8(&request)
            .ok()
            .and_then(|request| parse_callback(request, state.secret(), &host));
        let message = if result.is_some() {
            "已收到授權回應。請返回 JARVIS 查看連接結果。"
        } else {
            "此授權回應無效，請返回 JARVIS 重新連接。"
        };
        let response = format!("HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n<!doctype html><html lang=\"zh-Hant\"><title>JARVIS</title><body><h1>JARVIS</h1><p>{message}</p></body></html>", if result.is_some() { "200 OK" } else { "400 Bad Request" });
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            stream.write_all(response.as_bytes()),
        )
        .await;
        if let Some(result) = result {
            break result?;
        }
    };
    let tokens = response_json(
        http.post(format!("{ORIGIN}/token"))
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client_id),
                ("code", &code),
                ("redirect_uri", &redirect),
                ("code_verifier", verifier.secret()),
                ("resource", ENDPOINT),
            ])
            .send()
            .await
            .map_err(|_| "無法完成 Notion 授權交換。")?,
    )
    .await?;
    let mut bundle = TokenBundle {
        client_id: client_id.into(),
        access_token: String::new(),
        refresh_token: String::new(),
        expires_at: 0,
        workspace_id: String::new(),
        identity: String::new(),
    };
    apply_tokens(&mut bundle, &tokens)?;
    let mut connection = McpClient::connect(&bundle.access_token).await?;
    let (workspace_id, identity) = self_identity(
        &connection
            .call("notion-fetch", json!({"id":"self"}))
            .await?,
    )?;
    bundle.workspace_id = workspace_id;
    bundle.identity = identity.clone();
    let _token_guard = TOKEN_LOCK.lock().await;
    if cancel.is_cancelled() {
        return Err("Notion 授權已取消。".into());
    }
    let previous = load_bundle()?;
    store_bundle(&bundle)?;
    // A fresh grant may be for another user even within the same workspace.
    // Refreshing the index avoids retaining pages the new grant cannot access.
    if let Err(error) = set_method(db, "mcp", true) {
        if let Some(previous) = previous {
            let _ = store_bundle(&previous);
        } else {
            let _ = KeychainStore::delete(SERVICE, ACCOUNT);
        }
        return Err(error);
    }
    Ok(NotionConnectionStatus {
        status: "connected".into(),
        identity: Some(identity),
        method: Some("mcp".into()),
        message: None,
    })
}

pub async fn disconnect(db: &Database) -> Result<(), String> {
    cancel_connect()?;
    let _guard = TOKEN_LOCK.lock().await;
    // Mark disconnected first: a denied Keychain deletion must not revive the
    // token or silently fall back to a pre-existing environment/API credential.
    set_method(db, "disconnected", true)?;
    match KeychainStore::delete(SERVICE, ACCOUNT) {
        Ok(()) => Ok(()),
        Err(error) if KeychainStore::is_not_found(&error) => Ok(()),
        Err(_) => {
            Err("已解除 Notion 連接，但無法刪除鑰匙圈中的授權資料。請解鎖鑰匙圈後重試。".into())
        }
    }
}

pub async fn save_rest_token(db: &Database, token: &str) -> Result<(), String> {
    cancel_connect()?;
    let _guard = TOKEN_LOCK.lock().await;
    let selected = if token.trim().is_empty() {
        "disconnected"
    } else {
        "token"
    };
    // Fail closed before replacing a Keychain credential: SQLite and Keychain
    // cannot share a transaction. A later DB failure must leave the replacement
    // disabled, rather than let it inherit the old connection's generation.
    set_method(db, "disconnected", true)?;
    // Preserve the legacy Keychain item as a distinct advanced connection.
    crate::integration_keys::store(db, "notion_api_key", token)
        .map_err(|_| "無法將 Notion API 權杖儲存至鑰匙圈。")?;
    set_method(db, selected, false)
}

fn page_id(value: &str) -> Result<String, String> {
    uuid::Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| "Notion 頁面識別碼無效。".into())
}

fn notion_url(value: &str) -> bool {
    Url::parse(value)
        .map(|url| {
            let host = url.host_str().unwrap_or_default();
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && (host == "notion.so"
                    || host.ends_with(".notion.so")
                    || host == "notion.com"
                    || host.ends_with(".notion.com"))
        })
        .unwrap_or(false)
}

/// Recent-page rows name a page only by URL, ending in the 32 hex digits of
/// its id: `.../p/<id>` or `.../Some-Title-<id>`.
fn id_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let segment = parsed.path_segments()?.next_back()?;
    let hex = segment.get(segment.len().checked_sub(32)?..)?;
    hex.bytes().all(|b| b.is_ascii_hexdigit()).then(|| hex.to_string())
}

fn search_results(value: &Value) -> Result<Vec<super::notion::NotionPage>, String> {
    let rows = value["results"]
        .as_array()
        .ok_or("Notion 搜尋結果格式無效。")?;
    let mut pages = Vec::new();
    for row in rows.iter().take(50) {
        if let Some(kind) = row["object"].as_str().or_else(|| row["type"].as_str()) {
            if !matches!(kind, "page" | "notion_page") {
                continue;
            }
        }
        let Some(raw_id) = row["id"]
            .as_str()
            .map(str::to_string)
            .or_else(|| row["url"].as_str().and_then(id_from_url))
        else {
            continue;
        };
        let Ok(id) = page_id(&raw_id) else {
            continue;
        };
        let url = row["url"].as_str().filter(|url| notion_url(url));
        // Hosted search may include connected services. Never import those
        // into the Notion integration or offer their IDs as writable parents.
        if row["url"].is_string() && url.is_none() {
            continue;
        }
        let title = row["title"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or("未命名頁面");
        pages.push(super::notion::NotionPage {
            notion_id: id.clone(),
            title: title.chars().take(1000).collect(),
            url: Some(
                url.map(str::to_string)
                    .unwrap_or_else(|| format!("https://www.notion.so/{}", id.replace('-', ""))),
            ),
            parent_type: None,
            parent_title: None,
            last_edited: row["last_edited_time"]
                .as_str()
                .or_else(|| row["timestamp"].as_str())
                .map(str::to_string),
            content_snippet: None,
        });
    }
    Ok(pages)
}

pub async fn search_pages(
    db: &Database,
    query: Option<&str>,
) -> Result<Vec<super::notion::NotionPage>, String> {
    let generation = generation(db)?;
    let query = query.unwrap_or("").trim();
    if query.chars().count() > 1000 {
        return Err("Notion 搜尋文字過長，請縮短後重試。".into());
    }
    let mut connection = open(db).await?;
    // The hosted search refuses an empty query below the Business plan, so
    // "everything" means the pages the user has been working in.
    let results = if query.is_empty() {
        search_results(
            &connection
                .call("notion-list-recent-pages", json!({"limit": 50}))
                .await?,
        )?
    } else {
        search_connected(&mut connection, query).await?
    };
    ensure_generation(db, &generation)?;
    Ok(results)
}

async fn search_connected(
    connection: &mut McpClient,
    query: &str,
) -> Result<Vec<super::notion::NotionPage>, String> {
    let own = connection
        .call("notion-fetch", json!({"id":"self"}))
        .await?;
    self_identity(&own)?;
    let tool = if own["self"]["current_tool_access"]["ai_search"]["status"] == "available" {
        "notion-ai-search"
    } else {
        "notion-search"
    };
    let mut args = json!({"query":query});
    // Older workspace search schemas expose an internal/user selector. Only
    // send it when the official tool explicitly supports the internal value.
    if connection
        .tools
        .get(tool)
        .and_then(|schema| schema["properties"]["query_type"]["enum"].as_array())
        .map(|values| values.contains(&json!("internal")))
        .unwrap_or(false)
    {
        args["query_type"] = json!("internal");
    }
    search_results(&connection.call(tool, args).await?)
}

pub async fn get_page_content(db: &Database, id: &str) -> Result<String, String> {
    let generation = generation(db)?;
    let id = page_id(id)?;
    let mut connection = open(db).await?;
    let value = connection.call("notion-fetch", json!({"id":id})).await?;
    ensure_generation(db, &generation)?;
    if let Some(text) = value
        .as_str()
        .or_else(|| value["text"].as_str())
        .or_else(|| value["content"].as_str())
        .or_else(|| value["markdown"].as_str())
    {
        return Ok(text.into());
    }
    Err("Notion 未提供可讀取的頁面正文。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_only_accepts_its_own_state_path_and_host() {
        let request = "GET /notion/callback?state=expected&code=a%2Fb HTTP/1.1\r\nHost: 127.0.0.1:5555\r\n\r\n";
        assert_eq!(
            parse_callback(request, "expected", "127.0.0.1:5555"),
            Some(Ok("a/b".into()))
        );
        assert!(parse_callback(
            &request.replace("state=expected", "state=forged"),
            "expected",
            "127.0.0.1:5555"
        )
        .is_none());
        assert!(parse_callback(
            &request.replace("/notion/callback", "/unrelated"),
            "expected",
            "127.0.0.1:5555"
        )
        .is_none());
        assert!(parse_callback(request, "expected", "evil.example").is_none());
        assert!(parse_callback(
            &request.replace("code=a%2Fb", "code=first&code=second"),
            "expected",
            "127.0.0.1:5555"
        )
        .is_none());
    }

    #[test]
    fn rpc_reads_json_and_sse_but_rejects_wrong_ids_and_remote_errors() {
        let raw = r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}"#;
        assert_eq!(rpc_result(raw, 7).unwrap()["ok"], true);
        assert_eq!(
            rpc_result(&format!("event: message\ndata: {raw}\n\n"), 7).unwrap()["ok"],
            true
        );
        assert!(rpc_result(raw, 8).is_err());
        let err = rpc_result(
            r#"{"jsonrpc":"2.0","id":7,"error":{"message":"sensitive-remote-body"}}"#,
            7,
        )
        .unwrap_err();
        assert!(!err.contains("sensitive-remote-body"));
    }

    /// notion-list-recent-pages rows carry a URL but no id. Sync used to send
    /// an empty search instead, which Notion refuses below the Business plan,
    /// so every scheduled sync failed and notion_pages stayed empty.
    #[test]
    fn recent_page_rows_take_their_id_from_the_url() {
        let pages = search_results(&json!({"results":[
            {"type":"page","url":"https://app.notion.com/p/3c4e2f1a78bf814f8132c68d1ade8945?pvs=204","title":"Edwin 個案檔案"},
            {"type":"page","url":"https://www.notion.so/Weekly-Report-30de2f1a78bf80f38975dfb1423f77f7","title":"Weekly Report"},
            {"type":"database","url":"https://app.notion.com/p/28fe2f1a78bf80cfb9fdca62e8ca5892","title":"會議記錄"},
            {"type":"page","url":"https://app.notion.com/p/not-an-id","title":"壞連結"},
            {"type":"page","url":"https://slack.com/p/3c4e2f1a78bf814f8132c68d1ade8945","title":"Slack"}
        ],"nextCursor":"offset:5"}))
        .unwrap();
        let ids: Vec<&str> = pages.iter().map(|p| p.notion_id.as_str()).collect();
        assert_eq!(ids, ["3c4e2f1a-78bf-814f-8132-c68d1ade8945", "30de2f1a-78bf-80f3-8975-dfb1423f77f7"]);
        assert_eq!(pages[0].title, "Edwin 個案檔案");
    }

    /// Only a refresh token Notion actually rejected means reconnecting. A rate
    /// limit or outage used to say "reconnect" too, and reconnecting wipes the
    /// synced pages for nothing.
    #[test]
    fn only_a_rejected_refresh_token_asks_for_a_reconnect() {
        for status in [400, 401, 403] {
            assert!(refresh_failure(status).contains("重新連接"), "{status}");
        }
        for status in [429, 500, 502, 503] {
            assert!(!refresh_failure(status).contains("重新連接"), "{status}");
        }
    }

    #[test]
    fn mcp_exposes_only_the_existing_notion_operations() {
        assert!(allowed_tool("notion-search"));
        assert!(allowed_tool("notion-fetch"));
        assert!(allowed_tool("notion-list-recent-pages"));
        assert!(!allowed_tool("notion-create-pages"));
        assert!(!allowed_tool("notion-create-comment"));
        assert!(!allowed_tool("notion-update-page"));
        assert!(!allowed_tool("notion-create-file-upload"));
    }

    #[test]
    fn disconnected_mode_does_not_restore_an_existing_rest_key() {
        let db = test_db();
        crate::integration_keys::store(&db, "notion_api_key", "fixture-legacy-token").unwrap();
        set_method(&db, "disconnected", false).unwrap();
        assert!(!super::super::notion::is_configured(&db).unwrap());
        assert_eq!(method(&db).unwrap().as_deref(), Some("disconnected"));
        crate::integration_keys::store(&db, "notion_api_key", "").unwrap();
    }

    #[test]
    fn token_rotation_keeps_a_refresh_token_only_when_the_provider_omits_one() {
        let mut bundle = TokenBundle {
            client_id: "fixture-client".into(),
            access_token: "old-access".into(),
            refresh_token: "old-refresh".into(),
            expires_at: 0,
            workspace_id: "fixture".into(),
            identity: "測試工作區".into(),
        };
        apply_tokens(&mut bundle, &json!({"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600,"token_type":"Bearer"})).unwrap();
        assert_eq!(bundle.refresh_token, "new-refresh");
        apply_tokens(
            &mut bundle,
            &json!({"access_token":"third-access","expires_in":3600}),
        )
        .unwrap();
        assert_eq!(bundle.refresh_token, "new-refresh");
        assert!(apply_tokens(
            &mut bundle,
            &json!({"access_token":"bad\nheader","expires_in":3600})
        )
        .is_err());
        assert!(apply_tokens(&mut bundle, &json!({"access_token":"good","expires_in":0})).is_err());
    }

    #[test]
    fn search_results_exclude_connected_apps_and_non_page_objects() {
        let id = "b9e0a9b0-1649-459d-b871-8b6f1f235f41";
        let pages = search_results(&json!({"results":[
            {"id":id,"title":"工作筆記","url":format!("https://www.notion.so/{id}"),"type":"page"},
            {"id":id,"title":"Slack","url":"https://slack.com/a","type":"page"},
            {"id":id,"title":"資料庫","type":"database"},
            {"id":"malformed","title":"無效識別碼"}
        ]}))
        .unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].title, "工作筆記");
        assert!(!notion_url("https://notion.so.attacker.example/page"));
        assert!(!notion_url("https://token@notion.so/page"));
    }

    #[test]
    fn identity_requires_a_real_self_workspace_result() {
        assert!(self_identity(&json!({"result":"ok"})).is_err());
        let (_, identity) = self_identity(&json!({"self":{"workspace":{"id":"workspace","name":"測試公司"},"user":{"name":"測試使用者"}}})).unwrap();
        assert_eq!(identity, "測試公司 · 測試使用者");
    }

    #[tokio::test]
    async fn mcp_mode_cannot_create_pages_even_when_the_parent_was_synced() {
        let db = test_db();
        set_method(&db, "mcp", false).unwrap();
        let error = super::super::notion::create_page_connected(
            &db,
            "b9e0a9b0-1649-459d-b871-8b6f1f235f41",
            "標題",
            "內容",
        )
        .await
        .unwrap_err();
        assert!(error.contains("僅支援搜尋及讀取"));
    }

    #[tokio::test]
    async fn disconnect_removes_mcp_credentials_and_invalidates_previous_operations() {
        let db = test_db();
        set_method(&db, "mcp", false).unwrap();
        let old_generation = generation(&db).unwrap();
        let bundle = TokenBundle {
            client_id: "fixture-client".into(),
            access_token: "fixture-access".into(),
            refresh_token: "fixture-refresh".into(),
            expires_at: chrono::Utc::now().timestamp() + 3600,
            workspace_id: "fixture".into(),
            identity: "測試公司".into(),
        };
        store_bundle(&bundle).unwrap();
        assert_eq!(
            load_bundle().unwrap().unwrap().access_token,
            "fixture-access"
        );
        disconnect(&db).await.unwrap();
        assert!(load_bundle().unwrap().is_none());
        assert!(ensure_generation(&db, &old_generation).is_err());
        assert!(access_token(&db, false).await.unwrap_err().contains("已解除"));
        let status = connection_status(&db).await.unwrap();
        assert_eq!(status.status, "not_connected");
        assert!(status.identity.is_none());
    }

    #[test]
    fn replacing_a_connection_invalidates_older_reads_even_in_the_same_mode() {
        let db = test_db();
        set_method(&db, "mcp", false).unwrap();
        let old_generation = generation(&db).unwrap();
        set_method(&db, "mcp", true).unwrap();
        assert!(ensure_generation(&db, &old_generation).is_err());
        assert!(ensure_generation(&db, &generation(&db).unwrap()).is_ok());
    }

    #[tokio::test]
    async fn a_failed_rest_mode_commit_cannot_keep_the_previous_connection_active() {
        let db = test_db();
        set_method(&db, "token", false).unwrap();
        let original = generation(&db).unwrap();
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_token_mode BEFORE UPDATE ON user_preferences WHEN NEW.key='notion_connection_method' AND NEW.value='token' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(save_rest_token(&db, "fixture-replacement-token")
            .await
            .is_err());
        assert_eq!(method(&db).unwrap().as_deref(), Some("disconnected"));
        assert!(ensure_generation(&db, &original).is_err());
        crate::integration_keys::store(&db, "notion_api_key", "").unwrap();
    }

    #[tokio::test]
    async fn http_fixture_preserves_mcp_session_and_never_exposes_write_tools() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let (release, mut hold_open) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for index in 0..6 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0u8; 1024];
                    let count = stream.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .filter_map(|l| l.split_once(':'))
                            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(request
                    .to_lowercase()
                    .contains("authorization: bearer fixture-access"));
                if index > 0 {
                    assert!(request
                        .to_lowercase()
                        .contains("mcp-session-id: fixture-session"));
                }
                requests.push(request);
                let (status, extra, payload) = match index {
                    0 => ("200 OK", "Mcp-Session-Id: fixture-session\r\n", json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}}}}).to_string()),
                    1 => ("202 Accepted", "", String::new()),
                    2 => ("200 OK", "", json!({"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"notion-search"},{"name":"notion-ai-search"},{"name":"notion-fetch"},{"name":"notion-create-pages"}]}}).to_string()),
                    3 => ("200 OK", "", json!({"jsonrpc":"2.0","id":3,"result":{"structuredContent":{"self":{"workspace":{"id":"fixture","name":"測試公司"},"current_tool_access":{"ai_search":{"status":"available"}}}}}}).to_string()),
                    4 => ("200 OK", "", json!({"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"{\"results\":[]}"}]}}).to_string()),
                    _ => ("200 OK", "", json!({"jsonrpc":"2.0","id":5,"result":{"content":[{"type":"text","text":"測試頁面正文"}]}}).to_string()),
                };
                if index == 4 {
                    // Keep the stream open until the caller has received the
                    // matching event. Waiting for EOF would time out this test.
                    let data = format!("event: message\ndata: {payload}\n\n");
                    let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{data}\r\n", data.len());
                    stream.write_all(reply.as_bytes()).await.unwrap();
                    let _ = (&mut hold_open).await;
                    continue;
                }
                let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
                stream.write_all(reply.as_bytes()).await.unwrap();
            }
            requests
        });
        let mut connection = McpClient::connect_to("fixture-access", &endpoint)
            .await
            .unwrap();
        assert!(connection
            .call("notion-create-pages", json!({}))
            .await
            .is_err());
        let found = tokio::time::timeout(
            Duration::from_secs(2),
            search_connected(&mut connection, "報告"),
        )
        .await;
        let _ = release.send(());
        assert!(found.unwrap().unwrap().is_empty());
        let read = connection
            .call(
                "notion-fetch",
                json!({"id":"b9e0a9b0-1649-459d-b871-8b6f1f235f41"}),
            )
            .await
            .unwrap();
        assert_eq!(read, "測試頁面正文");
        let requests = server.await.unwrap();
        assert!(requests[0].contains("initialize"));
        assert!(requests[1].contains("notifications/initialized"));
        assert!(requests[2].contains("tools/list"));
        assert!(requests[3].contains("self"));
        assert!(requests[4].contains("notion-ai-search"));
        assert!(requests[5].contains("notion-fetch"));
        assert!(!requests.iter().any(|r| r.contains("notion-create-pages")));
    }

    fn test_db() -> Database {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE user_preferences(key TEXT PRIMARY KEY,value TEXT,updated_at TEXT); CREATE TABLE notion_pages(id INTEGER PRIMARY KEY,notion_id TEXT,title TEXT,url TEXT,parent_type TEXT,parent_title TEXT,last_edited TEXT,content_snippet TEXT,synced_at TEXT);").unwrap();
        Database {
            conn: Mutex::new(conn),
        }
    }
}
