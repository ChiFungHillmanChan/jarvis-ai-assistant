//! GitHub device authorization. Secrets never cross IPC or enter SQLite.
use crate::{db::Database, security::keychain::KeychainStore};
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, LazyLock, Mutex, OnceLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

const SERVICE: &str = "com.jarvis.github.oauth";
const ACCOUNT: &str = "device_token_bundle";
const DEVICE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const VERIFY_URL: &str = "https://github.com/login/device";
/// JARVIS's own OAuth App (owner ChiFungHillmanChan, Device Flow on, token
/// expiry off). A Client ID is public by design -- the device flow sends no
/// secret -- so it ships in the binary and every build can connect without
/// the user registering an app of their own.
const DEFAULT_CLIENT_ID: &str = "Ov23liZ7BoeqHE3fpKrn";
static HTTP: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .user_agent("JARVIS-Desktop")
        .build()
        .expect("GitHub HTTP client")
});
static FLOW: Mutex<Option<Arc<PendingFlow>>> = Mutex::new(None);
static CREDENTIAL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize, Clone)]
pub struct ConnectionStatus {
    pub status: &'static str,
    pub identity: Option<String>,
    pub method: Option<String>,
    pub message: Option<String>,
    pub oauth_ready: bool,
}
#[derive(Serialize)]
pub struct DevicePrompt {
    pub flow_id: String,
    pub user_code: String,
    pub verification_uri: &'static str,
    pub expires_in: u64,
}
struct PendingFlow {
    id: String,
    client_id: String,
    created_at: tokio::time::Instant,
    device: OnceLock<DeviceGrant>,
    cancelled: AtomicBool,
    polling: AtomicBool,
    wake: Notify,
}
struct DeviceGrant {
    code: String,
    interval: u64,
    deadline: tokio::time::Instant,
}
#[derive(Clone, Copy)]
struct Endpoints<'a> {
    device: &'a str,
    token: &'a str,
    user: &'a str,
}
const ENDPOINTS: Endpoints<'static> = Endpoints {
    device: DEVICE_URL,
    token: TOKEN_URL,
    user: "https://api.github.com/user",
};
#[derive(Clone, Copy)]
struct Storage<'a> {
    service: &'a str,
    account: &'a str,
}
const STORAGE: Storage<'static> = Storage {
    service: SERVICE,
    account: ACCOUNT,
};
#[derive(Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum StoredConnection {
    Oauth { bundle: TokenBundle },
    Manual { access_token: String, login: String },
    Disconnected,
}
#[derive(Serialize, Deserialize)]
struct TokenBundle {
    client_id: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    login: String,
}
#[derive(Debug, PartialEq)]
enum PollReply {
    Pending(u64),
    Token(String),
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn nonempty(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 8192)
        .map(str::to_owned)
}
fn provider_error(value: &Value) -> String {
    match value["error"].as_str() {
        Some("access_denied") => "GitHub 授權已取消。",
        Some("expired_token" | "token_expired") => "GitHub 驗證碼已過期，請重新連接。",
        Some("device_flow_disabled") => {
            "JARVIS 的 GitHub 應用程式尚未啟用 Device Flow，請先完成應用程式設定。"
        }
        Some("incorrect_client_credentials") => "GitHub Client ID 無效，請檢查 JARVIS 的授權設定。",
        Some("bad_refresh_token") => "GitHub 授權已失效，請重新連接。",
        _ => "未能完成 GitHub 授權，請稍後再試。",
    }
    .into()
}
fn poll_reply(value: &Value, interval: u64) -> Result<PollReply, String> {
    match value["error"].as_str() {
        Some("authorization_pending") => Ok(PollReply::Pending(interval)),
        Some("slow_down") => {
            let next = interval
                .saturating_add(5)
                .max(value["interval"].as_u64().unwrap_or(0));
            if next > 1800 {
                return Err("GitHub 要求延後授權，請稍後重新連接。".into());
            }
            Ok(PollReply::Pending(next))
        }
        Some(_) => Err(provider_error(value)),
        None if value["token_type"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("bearer")) =>
        {
            nonempty(value, "access_token")
                .map(PollReply::Token)
                .ok_or_else(|| "GitHub 未提供有效的存取權杖。".into())
        }
        _ => Err("GitHub 回傳的授權格式無效。".into()),
    }
}
fn validate_device_reply(value: &Value) -> Result<(), String> {
    let valid_code = value["user_code"].as_str().is_some_and(|s| {
        s.len() == 9
            && s.as_bytes()[4] == b'-'
            && s.bytes()
                .enumerate()
                .all(|(i, c)| i == 4 || c.is_ascii_uppercase() || c.is_ascii_digit())
    });
    if value["verification_uri"].as_str() != Some(VERIFY_URL)
        || !valid_code
        || nonempty(value, "device_code").is_none()
        || !value["expires_in"]
            .as_u64()
            .is_some_and(|v| (1..=1800).contains(&v))
        || !value["interval"]
            .as_u64()
            .is_some_and(|v| (1..=1800).contains(&v))
    {
        return Err(if value.get("error").is_some() {
            provider_error(value)
        } else {
            "GitHub 回傳的裝置授權資料無效。".into()
        });
    }
    Ok(())
}
/// Only a 401 means the grant is gone. Rate limits and outages clear on
/// their own, and a 403 is as often a secondary rate limit as a missing scope.
fn http_failure(status: u16) -> String {
    match status {
        401 => "GitHub 授權已失效，請重新連接。".into(),
        403 => "GitHub 拒絕此要求（HTTP 403），可能是請求過於頻繁或權限不足，請稍後再試。".into(),
        429 | 500..=599 => format!("GitHub 暫時無法使用（HTTP {status}），請稍後再試。"),
        _ => format!("GitHub 服務回應 HTTP {status}，請檢查連接。"),
    }
}
async fn json_response(mut response: Response) -> Result<Value, String> {
    if !response.status().is_success() {
        return Err(http_failure(response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "未能讀取 GitHub 回應。")?
    {
        if bytes.len() + chunk.len() > 65536 {
            return Err("GitHub 回應超出大小上限。".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "GitHub 回應格式無效。".into())
}
async fn post(url: &str, form: &[(&str, &str)]) -> Result<Value, String> {
    json_response(
        HTTP.post(url)
            .header("Accept", "application/json")
            .form(form)
            .send()
            .await
            .map_err(|_| "未能連接 GitHub，請檢查網絡後再試。")?,
    )
    .await
}
async fn identity_at(token: &str, user_url: &str) -> Result<String, String> {
    let response = HTTP
        .get(user_url)
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "未能驗證 GitHub 帳戶，請檢查網絡後再試。")?;
    let value = json_response(response).await?;
    value["login"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 100
                && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
        .map(str::to_owned)
        .ok_or_else(|| "GitHub 未提供有效的帳戶身分。".into())
}
fn read_connection(storage: Storage<'_>) -> Result<Option<StoredConnection>, String> {
    decode_connection(KeychainStore::get(storage.service, storage.account))
}
fn decode_connection(value: Result<String, String>) -> Result<Option<StoredConnection>, String> {
    let value = match value {
        Ok(value) => value,
        Err(error) if KeychainStore::is_not_found(&error) => return Ok(None),
        Err(_) => return Err("未能讀取 GitHub 鑰匙圈授權，請檢查 macOS 權限後再試。".into()),
    };
    if value.len() > 32_768 {
        return Err("GitHub 已儲存的授權格式無效，請重新連接。".into());
    }
    let stored: StoredConnection =
        serde_json::from_str(&value).map_err(|_| "GitHub 已儲存的授權格式無效，請重新連接。")?;
    let valid = match &stored {
        StoredConnection::Oauth { bundle } => {
            valid_token(&bundle.access_token)
                && valid_login(&bundle.login)
                && valid_client_id(&bundle.client_id)
                && bundle
                    .refresh_token
                    .as_ref()
                    .is_none_or(|token| valid_token(token))
        }
        StoredConnection::Manual {
            access_token,
            login,
        } => valid_token(access_token) && valid_login(login),
        StoredConnection::Disconnected => true,
    };
    if !valid {
        return Err("GitHub 已儲存的授權格式無效，請重新連接。".into());
    }
    Ok(Some(stored))
}
fn valid_token(token: &str) -> bool {
    !token.trim().is_empty() && token.len() <= 8192 && !token.chars().any(char::is_control)
}
fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 100
        && login
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
}
fn save_connection(storage: Storage<'_>, connection: &StoredConnection) -> Result<(), String> {
    let value = serde_json::to_string(connection).map_err(|_| "未能準備 GitHub 憑證。")?;
    KeychainStore::put(storage.service, storage.account, &value)
        .map_err(|_| "未能將 GitHub 授權儲存至鑰匙圈。".to_string())
}
fn clear_cached_account(db: &Database) -> Result<(), String> {
    db.conn
        .lock()
        .map_err(|_| "未能更新 GitHub 快取。")?
        .execute("DELETE FROM github_items", [])
        .map_err(|_| "未能清除舊 GitHub 快取；授權尚未變更，請稍後重試。")?;
    Ok(())
}
fn replace_connection(
    db: &Database,
    storage: Storage<'_>,
    connection: &StoredConnection,
) -> Result<(), String> {
    // Caller holds CREDENTIAL_LOCK. Losing a cache on Keychain failure is safe;
    // changing credentials while retaining another account's destinations is not.
    clear_cached_account(db)?;
    save_connection(storage, connection)
}
fn client_id(db: &Database) -> Option<String> {
    std::env::var("JARVIS_GITHUB_CLIENT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            option_env!("JARVIS_GITHUB_CLIENT_ID")
                .map(str::to_owned)
                .filter(|v| !v.trim().is_empty())
        })
        .or_else(|| {
            db.conn
                .lock()
                .ok()?
                .query_row(
                    "SELECT value FROM user_preferences WHERE key='github_oauth_client_id'",
                    [],
                    |r| r.get::<_, String>(0),
                )
                .ok()
        })
        .or_else(|| Some(DEFAULT_CLIENT_ID.to_string()))
        .filter(|v| valid_client_id(v))
}
fn valid_client_id(id: &str) -> bool {
    (10..=100).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
pub fn configure_client(db: &Database, id: &str) -> Result<(), String> {
    let id = id.trim();
    if !valid_client_id(id) {
        return Err(
            "請輸入 GitHub 應用程式的公開 Client ID；請勿輸入 Client Secret 或個人存取權杖。"
                .into(),
        );
    }
    if id.starts_with("ghp_") || id.starts_with("gho_") || id.starts_with("github_pat_") {
        return Err("此欄位只接受公開 Client ID，不接受存取權杖。".into());
    }
    db.conn.lock().map_err(|_|"未能讀取設定。")?.execute("INSERT INTO user_preferences(key,value) VALUES('github_oauth_client_id',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[id]).map_err(|_|"未能儲存 GitHub Client ID。")?;
    Ok(())
}
fn cancel_pending() -> Result<(), String> {
    if let Some(flow) = FLOW.lock().map_err(|_| "未能取消 GitHub 授權。")?.take() {
        flow.cancelled.store(true, Ordering::SeqCst);
        flow.wake.notify_one();
    }
    Ok(())
}
pub fn cancel() -> Result<(), String> {
    cancel_pending()
}
fn ensure_active(flow: &PendingFlow) -> Result<(), String> {
    if flow.cancelled.load(Ordering::SeqCst) {
        Err("GitHub 授權已取消。".into())
    } else if flow
        .device
        .get()
        .is_some_and(|device| tokio::time::Instant::now() >= device.deadline)
    {
        Err("GitHub 驗證碼已過期，請重新連接。".into())
    } else {
        Ok(())
    }
}
fn retire_flow(flow: &PendingFlow) -> Result<(), String> {
    let mut slot = FLOW.lock().map_err(|_| "未能更新 GitHub 授權狀態。")?;
    if slot.as_ref().is_some_and(|active| active.id == flow.id) {
        *slot = None;
    }
    Ok(())
}
async fn begin_using(client_id: String, endpoints: Endpoints<'_>) -> Result<DevicePrompt, String> {
    let flow = Arc::new(PendingFlow {
        id: uuid::Uuid::new_v4().to_string(),
        client_id,
        created_at: tokio::time::Instant::now(),
        device: OnceLock::new(),
        cancelled: AtomicBool::new(false),
        polling: AtomicBool::new(false),
        wake: Notify::new(),
    });
    // Register BEFORE the HTTP request. Cancel and newer begin requests must
    // invalidate a request even while GitHub has not returned a device code.
    {
        let mut slot = FLOW.lock().map_err(|_| "未能開始 GitHub 授權。")?;
        if let Some(old) = slot.take() {
            old.cancelled.store(true, Ordering::SeqCst);
            old.wake.notify_one();
        }
        *slot = Some(flow.clone());
    }
    let result = async {
        let form = [
            ("client_id", flow.client_id.as_str()),
            ("scope", "repo read:user"),
        ];
        let value = tokio::select! {
            _ = flow.wake.notified() => return Err("GitHub 授權已取消。".to_string()),
            result = post(endpoints.device, &form) => result?,
        };
        validate_device_reply(&value)?;
        let slot = FLOW.lock().map_err(|_| "未能開始 GitHub 授權。")?;
        ensure_active(&flow)?;
        if !slot.as_ref().is_some_and(|active| active.id == flow.id) {
            return Err("GitHub 授權已取消。".into());
        }
        let expires_in = value["expires_in"].as_u64().unwrap();
        flow.device
            .set(DeviceGrant {
                code: nonempty(&value, "device_code").unwrap(),
                interval: value["interval"].as_u64().unwrap(),
                deadline: flow.created_at + Duration::from_secs(expires_in),
            })
            .map_err(|_| "GitHub 授權狀態無效。")?;
        ensure_active(&flow)?;
        Ok(DevicePrompt {
            flow_id: flow.id.clone(),
            user_code: nonempty(&value, "user_code").unwrap(),
            verification_uri: VERIFY_URL,
            expires_in,
        })
    }
    .await;
    if result.is_err() {
        retire_flow(&flow)?;
    }
    result
}
pub async fn begin(db: &Database) -> Result<DevicePrompt, String> {
    let client_id=client_id(db).ok_or("GitHub 一鍵授權尚未啟用：請先設定 JARVIS 專用的公開 Client ID，並在 GitHub 啟用 Device Flow。")?;
    begin_using(client_id, ENDPOINTS).await
}
pub async fn complete(db: &Database, flow_id: &str) -> Result<ConnectionStatus, String> {
    let flow = FLOW
        .lock()
        .map_err(|_| "未能讀取 GitHub 授權狀態。")?
        .as_ref()
        .filter(|f| f.id == flow_id)
        .cloned()
        .ok_or("GitHub 授權已失效，請重新連接。")?;
    if flow.polling.swap(true, Ordering::SeqCst) {
        return Err("GitHub 授權正在進行。".into());
    }
    let result = complete_flow_using(db, &flow, ENDPOINTS, STORAGE).await;
    retire_flow(&flow)?;
    result
}
fn expires_at(value: &Value, required: bool) -> Result<Option<u64>, String> {
    match value.get("expires_in") {
        None if !required => Ok(None),
        Some(value)
            if value
                .as_u64()
                .is_some_and(|seconds| seconds > 0 && seconds <= 31_536_000) =>
        {
            Ok(Some(now().saturating_add(value.as_u64().unwrap())))
        }
        _ => Err("GitHub 回傳的授權有效期限無效，請重新連接。".into()),
    }
}
async fn complete_flow_using(
    db: &Database,
    flow: &PendingFlow,
    endpoints: Endpoints<'_>,
    storage: Storage<'_>,
) -> Result<ConnectionStatus, String> {
    let device = flow.device.get().ok_or("GitHub 裝置驗證碼尚未準備完成。")?;
    let mut interval = device.interval;
    loop {
        ensure_active(flow)?;
        tokio::select! {
            _=flow.wake.notified()=>{ ensure_active(flow)?; },
            _=tokio::time::sleep_until((tokio::time::Instant::now()+Duration::from_secs(interval)).min(device.deadline))=>{},
        }
        ensure_active(flow)?;
        let form = [
            ("client_id", flow.client_id.as_str()),
            ("device_code", device.code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];
        let value = tokio::select! {
            _=flow.wake.notified()=>return Err("GitHub 授權已取消。".into()),
            result=post(endpoints.token,&form)=>result?,
        };
        match poll_reply(&value, interval)? {
            PollReply::Pending(next) => interval = next,
            PollReply::Token(access_token) => {
                let login = tokio::select! {
                    _=flow.wake.notified()=>return Err("GitHub 授權已取消。".into()),
                    result=identity_at(&access_token,endpoints.user)=>result?,
                };
                let bundle = TokenBundle {
                    client_id: flow.client_id.clone(),
                    access_token,
                    refresh_token: nonempty(&value, "refresh_token"),
                    expires_at: expires_at(&value, false)?,
                    login: login.clone(),
                };
                let _guard = CREDENTIAL_LOCK.lock().await;
                let slot = FLOW.lock().map_err(|_| "未能完成 GitHub 授權。")?;
                ensure_active(flow)?;
                if !slot.as_ref().is_some_and(|f| f.id == flow.id) {
                    return Err("GitHub 授權已取消。".into());
                }
                replace_connection(db, storage, &StoredConnection::Oauth { bundle })?;
                return Ok(ConnectionStatus {
                    status: "connected",
                    identity: Some(login),
                    method: Some("oauth".into()),
                    message: Some("已完成授權。請同步 GitHub 以載入項目。".into()),
                    oauth_ready: true,
                });
            }
        }
    }
}
struct Credentials {
    token: String,
    method: &'static str,
    login: Option<String>,
}
async fn credentials_using(
    storage: Storage<'_>,
    endpoints: Endpoints<'_>,
    legacy: impl FnOnce() -> Option<String>,
) -> Result<Credentials, String> {
    let stored = read_connection(storage)?;
    let mut bundle = match stored {
        Some(StoredConnection::Disconnected) => {
            return Err("GitHub 尚未連接。請在「連接服務」完成授權。".into())
        }
        Some(StoredConnection::Manual {
            access_token,
            login,
        }) => {
            return Ok(Credentials {
                token: access_token,
                method: "token",
                login: Some(login),
            })
        }
        Some(StoredConnection::Oauth { bundle }) => bundle,
        None => {
            return legacy()
                .filter(|token| valid_token(token))
                .map(|token| Credentials {
                    token,
                    method: "token",
                    login: None,
                })
                .ok_or_else(|| "GitHub 尚未連接。請在「連接服務」完成授權。".into())
        }
    };
    if bundle
        .expires_at
        .is_none_or(|expiry| expiry > now().saturating_add(60))
    {
        return Ok(Credentials {
            token: bundle.access_token,
            method: "oauth",
            login: Some(bundle.login),
        });
    }
    let refresh = bundle
        .refresh_token
        .as_deref()
        .ok_or("GitHub 授權已到期，請重新連接。")?;
    let value = post(
        endpoints.token,
        &[
            ("client_id", &bundle.client_id),
            ("refresh_token", refresh),
            ("grant_type", "refresh_token"),
        ],
    )
    .await?;
    let PollReply::Token(token) = poll_reply(&value, 5)? else {
        return Err("未能更新 GitHub 授權，請重新連接。".into());
    };
    let login = identity_at(&token, endpoints.user).await?;
    if !login.eq_ignore_ascii_case(&bundle.login) {
        return Err("GitHub 授權的帳戶不一致，請重新連接。".into());
    }
    let expiration = expires_at(&value, true)?;
    let next_refresh =
        nonempty(&value, "refresh_token").ok_or("GitHub 未提供有效的更新權杖，請重新連接。")?;
    bundle.access_token = token.clone();
    bundle.refresh_token = Some(next_refresh);
    bundle.expires_at = expiration;
    bundle.login = login.clone();
    save_connection(storage, &StoredConnection::Oauth { bundle })?;
    Ok(Credentials {
        token,
        method: "oauth",
        login: Some(login),
    })
}
/// Keep this guard for the entire provider operation, including local cache
/// writes. A reconnect or disconnect waits for the current operation to finish.
pub struct GitHubSession {
    credentials: Credentials,
    _guard: tokio::sync::MutexGuard<'static, ()>,
}
impl GitHubSession {
    pub fn token(&self) -> &str {
        &self.credentials.token
    }
}
pub async fn session() -> Result<GitHubSession, String> {
    let guard = CREDENTIAL_LOCK.lock().await;
    let credentials = credentials_using(STORAGE, ENDPOINTS, || {
        crate::integration_keys::resolve("github_token")
    })
    .await?;
    Ok(GitHubSession {
        credentials,
        _guard: guard,
    })
}
pub async fn access_token() -> Result<String, String> {
    Ok(session().await?.token().to_string())
}
pub async fn sync_connected(db: &Database) -> Result<super::github::AssignedItemsFetch, String> {
    let session = session().await?;
    let fetched = super::github::fetch_assigned_items_detailed(session.token()).await?;
    super::github::save_to_db(db, &fetched)?;
    Ok(fetched)
}
pub async fn status(db: &Database) -> Result<ConnectionStatus, String> {
    let _guard = CREDENTIAL_LOCK.lock().await;
    let stored = read_connection(STORAGE)?;
    let method = match &stored {
        Some(StoredConnection::Disconnected) => None,
        Some(StoredConnection::Oauth { .. }) => Some("oauth"),
        Some(StoredConnection::Manual { .. }) => Some("token"),
        None if crate::integration_keys::resolve("github_token").is_some() => Some("token"),
        None => None,
    };
    let mut status = ConnectionStatus {
        status: if method.is_some() {
            "configured"
        } else {
            "not_connected"
        },
        identity: None,
        method: method.map(str::to_string),
        message: None,
        oauth_ready: client_id(db).is_some(),
    };
    if method.is_some() {
        match credentials_using(STORAGE, ENDPOINTS, || {
            crate::integration_keys::resolve("github_token")
        })
        .await
        {
            Ok(credentials) => match identity_at(&credentials.token, ENDPOINTS.user).await {
                Ok(login)
                    if credentials
                        .login
                        .as_ref()
                        .is_none_or(|expected| expected.eq_ignore_ascii_case(&login)) =>
                {
                    status.status = "connected";
                    status.identity = Some(login);
                    status.method = Some(credentials.method.into());
                }
                Ok(_) => status.message = Some("GitHub 授權的帳戶不一致，請重新連接。".into()),
                Err(error) => status.message = Some(error),
            },
            Err(error) => status.message = Some(error),
        }
    }
    Ok(status)
}
pub async fn disconnect(db: &Database) -> Result<(), String> {
    cancel_pending()?;
    let _guard = CREDENTIAL_LOCK.lock().await;
    // A non-secret tombstone also disables legacy/environment credentials.
    // Overwriting the one item is atomic and removes its OAuth/manual secrets.
    save_connection(STORAGE, &StoredConnection::Disconnected)?;
    if let Err(error) = KeychainStore::delete(crate::ai::KEY_SERVICE, "github_token") {
        if !KeychainStore::is_not_found(&error) {
            return Err("GitHub 已中斷連接，但未能移除舊鑰匙圈權杖；請檢查 macOS 權限。".into());
        }
    }
    crate::integration_keys::store(db, "github_token", "")
        .map_err(|_| "GitHub 已中斷連接，但未能清除舊憑證標記。")?;
    Ok(())
}
async fn save_manual_using(
    db: &Database,
    token: &str,
    storage: Storage<'_>,
    endpoints: Endpoints<'_>,
) -> Result<(), String> {
    let token = token.trim();
    if !valid_token(token) {
        return Err("請輸入有效的 GitHub 存取權杖。".into());
    }
    let login = identity_at(token, endpoints.user).await?;
    replace_connection(
        db,
        storage,
        &StoredConnection::Manual {
            access_token: token.to_string(),
            login,
        },
    )
}
pub async fn save_manual(db: &Database, token: &str) -> Result<(), String> {
    cancel_pending()?;
    let _guard = CREDENTIAL_LOCK.lock().await;
    save_manual_using(db, token, STORAGE, ENDPOINTS).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pending_and_slow_down_follow_server_polling_limits() {
        assert_eq!(
            poll_reply(&json!({"error":"authorization_pending"}), 5).unwrap(),
            PollReply::Pending(5)
        );
        assert_eq!(
            poll_reply(&json!({"error":"slow_down","interval":20}), 5).unwrap(),
            PollReply::Pending(20)
        );
        assert_eq!(
            poll_reply(&json!({"error":"slow_down"}), 20).unwrap(),
            PollReply::Pending(25)
        );
    }

    #[test]
    fn access_token_requires_bearer_and_never_echoes_provider_secrets() {
        assert_eq!(
            poll_reply(&json!({"access_token":"fixture","token_type":"bearer"}), 5).unwrap(),
            PollReply::Token("fixture".into())
        );
        for reply in [
            json!({"access_token":"","token_type":"bearer"}),
            json!({"access_token":"secret","token_type":"mac"}),
            json!({"error":"access_denied","error_description":"secret"}),
        ] {
            let error = poll_reply(&reply, 5).unwrap_err();
            assert!(!error.contains("secret"));
        }
    }

    #[test]
    fn device_response_cannot_redirect_user_to_an_untrusted_login() {
        let valid = json!({"device_code":"fixture-device","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5});
        assert!(validate_device_reply(&valid).is_ok());
        for (key, value) in [
            ("verification_uri", json!("https://example.test/login")),
            ("user_code", json!("<script>")),
            ("expires_in", json!(0)),
            ("interval", json!(0)),
            ("device_code", json!("")),
        ] {
            let mut reply = valid.clone();
            reply[key] = value;
            assert!(validate_device_reply(&reply).is_err(), "{key}");
        }
    }

    struct TestStorage {
        account: String,
    }
    impl TestStorage {
        fn new() -> Self {
            Self {
                account: uuid::Uuid::new_v4().to_string(),
            }
        }
        fn storage(&self) -> Storage<'_> {
            Storage {
                service: "com.jarvis.test.github-auth",
                account: &self.account,
            }
        }
    }
    impl Drop for TestStorage {
        fn drop(&mut self) {
            let storage = self.storage();
            let _ = KeychainStore::delete(storage.service, storage.account);
        }
    }
    fn temp_db() -> Database {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE github_items (id INTEGER PRIMARY KEY, repo TEXT); CREATE TABLE user_preferences (key TEXT PRIMARY KEY, value TEXT, updated_at TEXT);").unwrap();
        Database {
            conn: Mutex::new(connection),
        }
    }
    fn oauth_bundle() -> TokenBundle {
        TokenBundle {
            client_id: "fixture-client-id".into(),
            access_token: "expired-access".into(),
            refresh_token: Some("old-refresh".into()),
            expires_at: Some(now() - 10),
            login: "fixture-user".into(),
        }
    }
    fn manual(token: &str, login: &str) -> StoredConnection {
        StoredConnection::Manual {
            access_token: token.into(),
            login: login.into(),
        }
    }
    async fn http_fixture(
        replies: Vec<(u16, String, String)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, body, extra_headers) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 2048];
                loop {
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&request[..index]);
                        let body_length = header
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|length| length.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= index + 4 + body_length {
                            break;
                        }
                    }
                    assert!(request.len() < 32_768);
                }
                requests.push(String::from_utf8(request).unwrap());
                let response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes()).await;
            }
            requests
        });
        (format!("http://{address}"), task)
    }
    fn response(value: Value) -> (u16, String, String) {
        (200, value.to_string(), String::new())
    }

    /// With no Client ID anywhere the Connect button stayed disabled and the
    /// only way in was registering your own GitHub app.
    #[test]
    fn jarvis_ships_with_its_own_client_id() {
        assert!(valid_client_id(DEFAULT_CLIENT_ID));
        assert!(client_id(&temp_db()).is_some());
    }

    /// A GitHub outage or rate limit is not a revoked grant; telling the user
    /// to re-authorise sends them round a reconnect that fixes nothing.
    #[test]
    fn only_an_unauthorised_response_asks_for_a_reconnect() {
        assert!(http_failure(401).contains("重新連接"));
        for status in [429, 500, 502, 503] {
            let message = http_failure(status);
            assert!(!message.contains("重新"), "{status}: {message}");
            assert!(message.contains("稍後"), "{status}: {message}");
        }
    }

    #[test]
    fn keychain_faults_are_not_treated_as_missing_credentials() {
        assert!(decode_connection(Err("keychain get: interaction denied".into())).is_err());
        assert!(decode_connection(Err(
            "keychain get: The specified item could not be found in the keychain.".into()
        ))
        .unwrap()
        .is_none());
        assert!(decode_connection(Ok("{invalid-json}".into())).is_err());
        assert!(decode_connection(Ok(
            r#"{"method":"manual","access_token":"","login":"user"}"#.into()
        ))
        .is_err());
    }

    #[tokio::test]
    async fn explicit_manual_and_disconnected_states_never_use_a_legacy_account() {
        let fixture = TestStorage::new();
        save_connection(fixture.storage(), &manual("chosen-token", "chosen-user")).unwrap();
        let credentials = credentials_using(fixture.storage(), ENDPOINTS, || {
            panic!("legacy/env credential must not be read")
        })
        .await
        .unwrap();
        assert_eq!(credentials.token, "chosen-token");
        assert_eq!(credentials.login.as_deref(), Some("chosen-user"));
        save_connection(fixture.storage(), &StoredConnection::Disconnected).unwrap();
        assert!(credentials_using(fixture.storage(), ENDPOINTS, || panic!(
            "disconnect must block legacy/env fallback"
        ))
        .await
        .is_err());
        let stored =
            KeychainStore::get(fixture.storage().service, fixture.storage().account).unwrap();
        assert!(!stored.contains("chosen-token"));
        KeychainStore::put(
            fixture.storage().service,
            fixture.storage().account,
            "corrupt",
        )
        .unwrap();
        assert!(credentials_using(fixture.storage(), ENDPOINTS, || panic!(
            "corrupt state must fail closed"
        ))
        .await
        .is_err());
    }

    #[tokio::test]
    async fn absent_new_storage_preserves_legacy_credentials_only_until_explicit_selection() {
        let fixture = TestStorage::new();
        let credentials =
            credentials_using(fixture.storage(), ENDPOINTS, || Some("legacy-token".into()))
                .await
                .unwrap();
        assert_eq!(credentials.token, "legacy-token");
        assert_eq!(credentials.method, "token");
    }

    #[tokio::test]
    async fn refresh_rotates_keychain_tokens_and_preserves_the_account() {
        let fixture = TestStorage::new();
        save_connection(
            fixture.storage(),
            &StoredConnection::Oauth {
                bundle: oauth_bundle(),
            },
        )
        .unwrap();
        let (base, server) = http_fixture(vec![
            response(json!({"access_token":"fresh-access","token_type":"bearer","refresh_token":"fresh-refresh","expires_in":28800})),
            response(json!({"login":"fixture-user"})),
        ]).await;
        let token_url = format!("{base}/token");
        let user_url = format!("{base}/user");
        let endpoints = Endpoints {
            token: &token_url,
            user: &user_url,
            ..ENDPOINTS
        };
        let credentials = credentials_using(fixture.storage(), endpoints, || {
            panic!("OAuth must not fall back")
        })
        .await
        .unwrap();
        assert_eq!(credentials.token, "fresh-access");
        let Some(StoredConnection::Oauth { bundle }) = read_connection(fixture.storage()).unwrap()
        else {
            panic!("OAuth bundle missing")
        };
        assert_eq!(bundle.refresh_token.as_deref(), Some("fresh-refresh"));
        assert!(bundle.expires_at.unwrap() > now());
        let requests = server.await.unwrap();
        assert!(requests[0].contains("grant_type=refresh_token"));
        assert!(requests[0].contains("refresh_token=old-refresh"));
        assert!(requests[1]
            .to_lowercase()
            .contains("authorization: bearer fresh-access"));
    }

    #[tokio::test]
    async fn refresh_with_another_account_or_missing_expiry_never_replaces_credentials() {
        for (login, expiry) in [("another-user", Some(28800)), ("fixture-user", None)] {
            let fixture = TestStorage::new();
            save_connection(
                fixture.storage(),
                &StoredConnection::Oauth {
                    bundle: oauth_bundle(),
                },
            )
            .unwrap();
            let before =
                KeychainStore::get(fixture.storage().service, fixture.storage().account).unwrap();
            let mut token = json!({"access_token":"wrong-access","token_type":"bearer","refresh_token":"next-refresh"});
            if let Some(expiry) = expiry {
                token["expires_in"] = json!(expiry);
            }
            let (base, server) =
                http_fixture(vec![response(token), response(json!({"login":login}))]).await;
            let token_url = format!("{base}/token");
            let user_url = format!("{base}/user");
            let endpoints = Endpoints {
                token: &token_url,
                user: &user_url,
                ..ENDPOINTS
            };
            assert!(credentials_using(fixture.storage(), endpoints, || panic!(
                "refresh failure must not use legacy"
            ))
            .await
            .is_err());
            assert_eq!(
                KeychainStore::get(fixture.storage().service, fixture.storage().account).unwrap(),
                before
            );
            server.await.unwrap();
        }
    }

    #[test]
    fn failed_cache_clear_does_not_change_the_account() {
        let fixture = TestStorage::new();
        save_connection(
            fixture.storage(),
            &manual("previous-token", "previous-user"),
        )
        .unwrap();
        let db = temp_db();
        db.conn
            .lock()
            .unwrap()
            .execute("DROP TABLE github_items", [])
            .unwrap();
        assert!(
            replace_connection(&db, fixture.storage(), &manual("new-token", "new-user")).is_err()
        );
        let Some(StoredConnection::Manual { login, .. }) =
            read_connection(fixture.storage()).unwrap()
        else {
            panic!("manual connection missing")
        };
        assert_eq!(login, "previous-user");
    }

    #[tokio::test]
    async fn account_switch_waits_for_the_operation_and_removes_its_old_cache() {
        let fixture = TestStorage::new();
        let db = Arc::new(temp_db());
        let session = GitHubSession {
            credentials: Credentials {
                token: "previous-token".into(),
                method: "token",
                login: Some("previous-user".into()),
            },
            _guard: CREDENTIAL_LOCK.lock().await,
        };
        let db_for_switch = db.clone();
        let account = fixture.account.clone();
        let switch = tokio::spawn(async move {
            let _guard = CREDENTIAL_LOCK.lock().await;
            replace_connection(
                &db_for_switch,
                Storage {
                    service: "com.jarvis.test.github-auth",
                    account: &account,
                },
                &manual("new-token", "new-user"),
            )
        });
        tokio::task::yield_now().await;
        assert!(!switch.is_finished());
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO github_items(repo) VALUES ('previous/repo')",
                [],
            )
            .unwrap();
        drop(session);
        switch.await.unwrap().unwrap();
        let count: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM github_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    static FLOW_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    #[tokio::test]
    async fn completed_device_authorization_saves_verified_account_only_in_keychain() {
        let _serial = FLOW_TEST_LOCK.lock().await;
        cancel_pending().unwrap();
        let fixture = TestStorage::new();
        let db = temp_db();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO github_items(repo) VALUES ('previous/repo')",
                [],
            )
            .unwrap();
        let (base, server) = http_fixture(vec![
            response(json!({"device_code":"fixture-device","user_code":"ABCD-EFGH","verification_uri":VERIFY_URL,"expires_in":900,"interval":1})),
            response(json!({"access_token":"verified-oauth-token","token_type":"bearer"})),
            response(json!({"login":"verified-user"})),
        ]).await;
        let device = format!("{base}/device");
        let token = format!("{base}/token");
        let user = format!("{base}/user");
        let endpoints = Endpoints {
            device: &device,
            token: &token,
            user: &user,
        };
        let prompt = begin_using("fixture-client-id".into(), endpoints)
            .await
            .unwrap();
        let flow = FLOW.lock().unwrap().as_ref().unwrap().clone();
        assert_eq!(prompt.flow_id, flow.id);
        let status = complete_flow_using(&db, &flow, endpoints, fixture.storage())
            .await
            .unwrap();
        assert_eq!(status.status, "connected");
        assert_eq!(status.identity.as_deref(), Some("verified-user"));
        let Some(StoredConnection::Oauth { bundle }) = read_connection(fixture.storage()).unwrap()
        else {
            panic!("OAuth credentials missing")
        };
        assert_eq!(bundle.access_token, "verified-oauth-token");
        let db = db.conn.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM github_items", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM user_preferences", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(db);
        retire_flow(&flow).unwrap();
        let requests = server.await.unwrap();
        assert!(requests[0].contains("scope=repo+read%3Auser"));
        assert!(requests[1].contains("device_code=fixture-device"));
    }

    #[tokio::test]
    async fn manual_save_verifies_the_submitted_token_before_replacing_old_account() {
        let fixture = TestStorage::new();
        let db = temp_db();
        let (base, server) = http_fixture(vec![response(json!({"login":"chosen-user"}))]).await;
        let endpoints = Endpoints {
            user: &base,
            ..ENDPOINTS
        };
        save_manual_using(&db, "  chosen-token  ", fixture.storage(), endpoints)
            .await
            .unwrap();
        let credentials = credentials_using(fixture.storage(), ENDPOINTS, || {
            Some("other-environment-account".into())
        })
        .await
        .unwrap();
        assert_eq!(credentials.token, "chosen-token");
        assert_eq!(credentials.login.as_deref(), Some("chosen-user"));
        assert!(server.await.unwrap()[0]
            .to_lowercase()
            .contains("authorization: bearer chosen-token"));
        let (base, server) = http_fixture(vec![(
            401,
            "untrusted-provider-error".into(),
            String::new(),
        )])
        .await;
        assert!(save_manual_using(
            &db,
            "invalid-token",
            fixture.storage(),
            Endpoints {
                user: &base,
                ..ENDPOINTS
            }
        )
        .await
        .is_err());
        assert_eq!(
            credentials_using(fixture.storage(), ENDPOINTS, || None)
                .await
                .unwrap()
                .token,
            "chosen-token"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancelling_during_initial_device_request_cannot_restore_the_flow() {
        use tokio::io::AsyncReadExt;
        let _serial = FLOW_TEST_LOCK.lock().await;
        cancel_pending().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let device = format!("http://{}/device", listener.local_addr().unwrap());
        let (seen, waiting) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 2048];
            socket.read(&mut buffer).await.unwrap();
            let _ = seen.send(());
            std::future::pending::<()>().await;
        });
        let beginning = tokio::spawn(async move {
            begin_using(
                "fixture-client-id".into(),
                Endpoints {
                    device: &device,
                    ..ENDPOINTS
                },
            )
            .await
        });
        waiting.await.unwrap();
        cancel_pending().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), beginning)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        assert!(FLOW.lock().unwrap().is_none());
        server.abort();
    }

    #[tokio::test]
    async fn newer_begin_supersedes_an_earlier_request_still_in_flight() {
        use tokio::io::AsyncReadExt;
        let _serial = FLOW_TEST_LOCK.lock().await;
        cancel_pending().unwrap();
        let first_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let first_device = format!("http://{}/device", first_listener.local_addr().unwrap());
        let (seen, waiting) = tokio::sync::oneshot::channel();
        let first_server = tokio::spawn(async move {
            let (mut socket, _) = first_listener.accept().await.unwrap();
            socket.read(&mut [0; 2048]).await.unwrap();
            let _ = seen.send(());
            std::future::pending::<()>().await;
        });
        let first = tokio::spawn(async move {
            begin_using(
                "fixture-client-id".into(),
                Endpoints {
                    device: &first_device,
                    ..ENDPOINTS
                },
            )
            .await
        });
        waiting.await.unwrap();
        let (second_device, second_server) = http_fixture(vec![response(json!({"device_code":"new-device-code","user_code":"ABCD-EFGH","verification_uri":VERIFY_URL,"expires_in":900,"interval":5}))]).await;
        let second = begin_using(
            "fixture-client-id".into(),
            Endpoints {
                device: &second_device,
                ..ENDPOINTS
            },
        )
        .await
        .unwrap();
        assert!(first.await.unwrap().is_err());
        assert_eq!(FLOW.lock().unwrap().as_ref().unwrap().id, second.flow_id);
        cancel_pending().unwrap();
        first_server.abort();
        second_server.await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_token_poll_cannot_commit_a_late_token() {
        use tokio::io::AsyncReadExt;
        let _serial = FLOW_TEST_LOCK.lock().await;
        cancel_pending().unwrap();
        let fixture = TestStorage::new();
        let db = Arc::new(temp_db());
        let flow = Arc::new(PendingFlow {
            id: uuid::Uuid::new_v4().to_string(),
            client_id: "fixture-client-id".into(),
            created_at: tokio::time::Instant::now(),
            device: OnceLock::new(),
            cancelled: AtomicBool::new(false),
            polling: AtomicBool::new(false),
            wake: Notify::new(),
        });
        flow.device
            .set(DeviceGrant {
                code: "device-code".into(),
                interval: 0,
                deadline: tokio::time::Instant::now() + Duration::from_secs(30),
            })
            .ok()
            .unwrap();
        *FLOW.lock().unwrap() = Some(flow.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let token_url = format!("http://{}/token", listener.local_addr().unwrap());
        let (seen, waiting) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket.read(&mut [0; 2048]).await.unwrap();
            let _ = seen.send(());
            std::future::pending::<()>().await;
        });
        let account = fixture.account.clone();
        let completing = tokio::spawn(async move {
            complete_flow_using(
                &db,
                &flow,
                Endpoints {
                    token: &token_url,
                    ..ENDPOINTS
                },
                Storage {
                    service: "com.jarvis.test.github-auth",
                    account: &account,
                },
            )
            .await
        });
        waiting.await.unwrap();
        cancel_pending().unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(1), completing)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(read_connection(fixture.storage()).unwrap().is_none());
        server.abort();
    }

    #[tokio::test]
    async fn network_redirect_and_oversized_provider_responses_are_rejected() {
        let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (base, server) = http_fixture(vec![(
            302,
            String::new(),
            format!(
                "Location: http://{}/stolen\r\n",
                destination.local_addr().unwrap()
            ),
        )])
        .await;
        let error = post(&base, &[("refresh_token", "fixture-sensitive")])
            .await
            .unwrap_err();
        assert!(error.contains("302"));
        assert!(!error.contains("fixture-sensitive"));
        server.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), destination.accept())
                .await
                .is_err()
        );
        let (base, server) = http_fixture(vec![(200, "a".repeat(65_537), String::new())]).await;
        assert!(post(&base, &[]).await.unwrap_err().contains("上限"));
        server.await.unwrap();
        assert!(poll_reply(&json!({"error":"slow_down","interval":u64::MAX}), 5).is_err());
    }
}
