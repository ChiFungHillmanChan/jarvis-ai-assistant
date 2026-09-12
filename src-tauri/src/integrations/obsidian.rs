use super::obsidian_vault::{self, VaultGrant};
use crate::db::Database;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

pub const DEFAULT_OBSIDIAN_URL: &str = "https://127.0.0.1:27124";
const MAX_RESPONSE_BYTES: usize = 2 * 1_048_576;
const VAULT_PREF: &str = "obsidian_vault_grant";
const MODE_PREF: &str = "obsidian_connection_mode";
const GENERATION_PREF: &str = "obsidian_connection_generation";

pub type ConnectionGeneration = Option<String>;
pub fn capture_generation(db: &Database) -> Result<ConnectionGeneration, String> {
    preference(db, GENERATION_PREF)
}
pub fn check_generation(db: &Database, expected: &ConnectionGeneration) -> Result<(), String> {
    if &capture_generation(db)? != expected {
        return Err("Obsidian 連接已變更，已捨棄先前的讀取結果，請重試。".to_string());
    }
    Ok(())
}
async fn with_generation<T>(
    db: &Database,
    expected: &ConnectionGeneration,
    operation: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    check_generation(db, expected)?;
    let result = operation.await;
    check_generation(db, expected)?;
    result
}
fn renew_generation(transaction: &rusqlite::Transaction<'_>) -> Result<(), String> {
    transaction.execute("INSERT INTO user_preferences (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=?2", [GENERATION_PREF, &uuid::Uuid::new_v4().to_string()])
        .map_err(|_| "未能更新 Obsidian 授權狀態。")?;
    Ok(())
}
pub fn invalidate_reads(db: &Database) -> Result<(), String> {
    let mut conn = db.conn.lock().map_err(|_| "未能更新 Obsidian 授權狀態。")?;
    let transaction = conn
        .transaction()
        .map_err(|_| "未能更新 Obsidian 授權狀態。")?;
    renew_generation(&transaction)?;
    transaction
        .commit()
        .map_err(|_| "未能更新 Obsidian 授權狀態。".to_string())
}

fn preference(db: &Database, key: &str) -> Result<Option<String>, String> {
    use rusqlite::OptionalExtension;
    db.conn
        .lock()
        .map_err(|_| "未能讀取 Obsidian 連接設定。")?
        .query_row(
            "SELECT value FROM user_preferences WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "未能讀取 Obsidian 連接設定。".to_string())
}

pub fn resolve_base_url(db: &Database) -> String {
    preference(db, "obsidian_base_url")
        .ok()
        .flatten()
        .filter(|url| !url.trim().is_empty())
        .map(|url| url.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_OBSIDIAN_URL.to_string())
}

/// localhost is pinned to its numeric loopback address, avoiding DNS resolution.
/// Credentials must never leave this device, including through an HTTP redirect.
pub fn validated_base_url(base_url: &str) -> Result<Url, String> {
    let mut url = Url::parse(base_url).map_err(|_| "Obsidian 端點網址無效。")?;
    let allowed_host = match url.host_str() {
        Some("localhost") => {
            url.set_host(Some("127.0.0.1"))
                .map_err(|_| "Obsidian 端點網址無效。")?;
            true
        }
        Some(host) => host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    };
    if !allowed_host
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.port() == Some(0)
    {
        return Err(
            "Obsidian Local REST API 只接受本機 loopback 端點，且不可包含帳密、路徑或查詢參數。"
                .to_string(),
        );
    }
    Ok(url)
}

/// Self-signed certificates are allowed only after validating a loopback URL.
static OBSIDIAN_HTTP: LazyLock<Result<Client, String>> = LazyLock::new(|| {
    Client::builder()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "未能建立 Obsidian 本機連接。".to_string())
});

fn client() -> Result<&'static Client, String> {
    OBSIDIAN_HTTP.as_ref().map_err(Clone::clone)
}
fn endpoint(base_url: &str, suffix: &str) -> Result<Url, String> {
    validated_base_url(base_url)?
        .join(suffix)
        .map_err(|_| "Obsidian 端點網址無效。".to_string())
}
fn note_url(base_url: &str, path: &str) -> Result<Url, String> {
    let parts = obsidian_vault::note_components(path)?;
    let encoded = parts
        .into_iter()
        .map(|part| urlencoding::encode(part).into_owned())
        .collect::<Vec<_>>()
        .join("/");
    endpoint(base_url, &format!("vault/{encoded}"))
}
fn transport_error(_: reqwest::Error) -> String {
    "未能連接 Obsidian。請確認應用程式及 Local REST API 外掛已啟動。".to_string()
}
async fn response_bytes(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(format!(
            "Obsidian 連接失敗（HTTP {}）。請檢查外掛及 API key。",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("Obsidian 回傳內容超過處理上限。".to_string());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if bytes.len() + chunk.len() > limit {
            return Err("Obsidian 回傳內容超過處理上限。".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ObsidianNote {
    pub path: String,
    pub content: Option<String>,
}
#[derive(Deserialize)]
struct SearchResult {
    filename: Option<String>,
    path: Option<String>,
    matches: Option<Vec<SearchMatch>>,
}
#[derive(Deserialize)]
struct SearchMatch {
    context: Option<String>,
}

/// The plugin's documented simple-search API uses query parameters, not JSON.
pub async fn search_vault(
    base_url: &str,
    api_key: &str,
    query: &str,
) -> Result<Vec<ObsidianNote>, String> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 500 {
        return Err("請輸入 1 至 500 個字元的搜尋內容。".to_string());
    }
    let mut url = endpoint(base_url, "search/simple/")?;
    url.query_pairs_mut()
        .append_pair("query", query)
        .append_pair("contextLength", "100");
    let response = client()?
        .post(url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(transport_error)?;
    let results: Vec<SearchResult> =
        serde_json::from_slice(&response_bytes(response, MAX_RESPONSE_BYTES).await?)
            .map_err(|_| "Obsidian 搜尋回應格式無效。".to_string())?;
    Ok(results
        .into_iter()
        .filter_map(|result| {
            let path = result.path.or(result.filename)?;
            obsidian_vault::note_components(&path).ok()?;
            let content = result
                .matches
                .and_then(|matches| matches.into_iter().next())
                .and_then(|m| m.context)
                .map(|text| text.chars().take(500).collect());
            Some(ObsidianNote { path, content })
        })
        .take(50)
        .collect())
}

pub async fn get_note(base_url: &str, api_key: &str, path: &str) -> Result<String, String> {
    let response = client()?
        .get(note_url(base_url, path)?)
        .bearer_auth(api_key)
        .header("Accept", "text/markdown")
        .send()
        .await
        .map_err(transport_error)?;
    String::from_utf8(response_bytes(response, obsidian_vault::MAX_NOTE_BYTES).await?)
        .map_err(|_| "Obsidian 筆記不是有效的 UTF-8 文字。".to_string())
}

pub async fn save_note(
    base_url: &str,
    api_key: &str,
    path: &str,
    content: &str,
) -> Result<(), String> {
    if content.len() > obsidian_vault::MAX_NOTE_BYTES {
        return Err("筆記超過 1 MiB，請縮小內容後再試。".to_string());
    }
    let response = client()?
        .put(note_url(base_url, path)?)
        .bearer_auth(api_key)
        .header("Content-Type", "text/markdown")
        .body(content.to_string())
        .send()
        .await
        .map_err(transport_error)?;
    response_bytes(response, MAX_RESPONSE_BYTES).await?;
    Ok(())
}

pub async fn list_files(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let response = client()?
        .get(endpoint(base_url, "vault/")?)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(transport_error)?;
    let body: serde_json::Value =
        serde_json::from_slice(&response_bytes(response, MAX_RESPONSE_BYTES).await?)
            .map_err(|_| "Obsidian 檔案清單格式無效。".to_string())?;
    let files = body["files"]
        .as_array()
        .ok_or("Obsidian 檔案清單格式無效。")?;
    Ok(files
        .iter()
        .filter_map(|file| file.as_str())
        .filter(|path| obsidian_vault::note_components(path).is_ok())
        .take(10_000)
        .map(str::to_string)
        .collect())
}

pub enum Connection {
    Vault(VaultGrant),
    Rest { base_url: String, key: String },
    Disconnected,
}

pub async fn verify_rest(base_url: &str, api_key: &str) -> Result<(), String> {
    let response = client()?
        .get(endpoint(base_url, "")?)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(transport_error)?;
    let body: serde_json::Value = serde_json::from_slice(&response_bytes(response, 65_536).await?)
        .map_err(|_| "Obsidian 連接驗證回應格式無效。".to_string())?;
    if body["authenticated"] != true || body["service"].as_str() != Some("Obsidian Local REST API")
    {
        return Err("Obsidian 外掛未確認授權，請檢查 API key。".to_string());
    }
    Ok(())
}

pub fn connection(db: &Database) -> Result<Connection, String> {
    let mode = preference(db, MODE_PREF)?;
    match mode.as_deref() {
        Some("disconnected") => Ok(Connection::Disconnected),
        Some("vault") => {
            let stored = preference(db, VAULT_PREF)?
                .ok_or("Obsidian 資料庫授權已失效，請重新選取資料夾。")?;
            let grant = serde_json::from_str(&stored)
                .map_err(|_| "Obsidian 資料庫授權已失效，請重新選取資料夾。")?;
            Ok(Connection::Vault(grant))
        }
        Some("rest") | None | Some("") => {
            match crate::integration_keys::resolve("obsidian_api_key") {
                Some(key) => Ok(Connection::Rest {
                    base_url: resolve_base_url(db),
                    key,
                }),
                None => Ok(Connection::Disconnected),
            }
        }
        _ => Err("Obsidian 連接設定無效，請重新連接。".to_string()),
    }
}

pub fn store_vault(db: &Database, grant: &VaultGrant) -> Result<(), String> {
    grant.verify()?;
    let stored = serde_json::to_string(grant).map_err(|_| "未能儲存 Obsidian 授權。")?;
    let mut conn = db.conn.lock().map_err(|_| "未能儲存 Obsidian 授權。")?;
    let transaction = conn.transaction().map_err(|_| "未能儲存 Obsidian 授權。")?;
    for (key, value) in [(VAULT_PREF, stored.as_str()), (MODE_PREF, "vault")] {
        transaction.execute("INSERT INTO user_preferences (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=?2", [key, value])
            .map_err(|_| "未能儲存 Obsidian 授權。")?;
    }
    renew_generation(&transaction)?;
    transaction
        .commit()
        .map_err(|_| "未能儲存 Obsidian 授權。".to_string())
}

pub fn select_rest_mode(db: &Database) -> Result<(), String> {
    let mut conn = db.conn.lock().map_err(|_| "未能儲存 Obsidian 連接設定。")?;
    let transaction = conn
        .transaction()
        .map_err(|_| "未能儲存 Obsidian 連接設定。")?;
    transaction.execute("INSERT INTO user_preferences (key,value) VALUES (?1,'rest') ON CONFLICT(key) DO UPDATE SET value='rest'", [MODE_PREF])
        .map_err(|_| "未能儲存 Obsidian 連接設定。")?;
    transaction
        .execute("DELETE FROM user_preferences WHERE key=?1", [VAULT_PREF])
        .map_err(|_| "未能更新 Obsidian 連接設定。")?;
    renew_generation(&transaction)?;
    transaction
        .commit()
        .map_err(|_| "未能儲存 Obsidian 連接設定。".to_string())
}

pub fn disconnect(db: &Database) -> Result<(), String> {
    // Disable first, including development environment credentials. Removing a
    // stored key alone would otherwise silently fall back to OBSIDIAN_API_KEY.
    {
        let mut conn = db.conn.lock().map_err(|_| "未能中斷 Obsidian 連接。")?;
        let transaction = conn.transaction().map_err(|_| "未能中斷 Obsidian 連接。")?;
        transaction.execute("INSERT INTO user_preferences (key,value) VALUES (?1,'disconnected') ON CONFLICT(key) DO UPDATE SET value='disconnected'", [MODE_PREF])
            .map_err(|_| "未能中斷 Obsidian 連接。")?;
        transaction
            .execute("DELETE FROM user_preferences WHERE key=?1", [VAULT_PREF])
            .map_err(|_| "未能撤銷資料庫權限。")?;
        renew_generation(&transaction)?;
        transaction
            .commit()
            .map_err(|_| "未能中斷 Obsidian 連接。")?;
    }
    crate::integration_keys::store(db, "obsidian_api_key", "")
}

fn not_connected() -> String {
    "尚未連接 Obsidian，請先選取資料庫資料夾。".to_string()
}

pub async fn search_connected(db: &Database, query: &str) -> Result<Vec<ObsidianNote>, String> {
    let generation = capture_generation(db)?;
    let connection = connection(db)?;
    with_generation(db, &generation, async {
        match connection {
            Connection::Vault(grant) => {
                let query = query.to_string();
                tokio::task::spawn_blocking(move || obsidian_vault::search_notes(&grant, &query))
                    .await
                    .map_err(|_| "未能搜尋 Obsidian 筆記。")?
            }
            Connection::Rest { base_url, key } => search_vault(&base_url, &key, query).await,
            Connection::Disconnected => Err(not_connected()),
        }
    })
    .await
}
pub async fn read_connected(db: &Database, path: &str) -> Result<String, String> {
    let generation = capture_generation(db)?;
    let connection = connection(db)?;
    with_generation(db, &generation, async {
        match connection {
            Connection::Vault(grant) => {
                let path = path.to_string();
                tokio::task::spawn_blocking(move || obsidian_vault::read_note(&grant, &path))
                    .await
                    .map_err(|_| "未能讀取 Obsidian 筆記。")?
            }
            Connection::Rest { base_url, key } => get_note(&base_url, &key, path).await,
            Connection::Disconnected => Err(not_connected()),
        }
    })
    .await
}
pub async fn list_connected(db: &Database) -> Result<Vec<String>, String> {
    let generation = capture_generation(db)?;
    let connection = connection(db)?;
    with_generation(db, &generation, async {
        match connection {
            Connection::Vault(grant) => {
                tokio::task::spawn_blocking(move || obsidian_vault::list_notes(&grant))
                    .await
                    .map_err(|_| "未能列出 Obsidian 筆記。")?
            }
            Connection::Rest { base_url, key } => list_files(&base_url, &key).await,
            Connection::Disconnected => Err(not_connected()),
        }
    })
    .await
}
pub async fn save_connected(db: &Database, path: &str, content: &str) -> Result<(), String> {
    match connection(db)? {
        Connection::Vault(_) => {
            Err("本機資料庫連接只授予讀取權限。請在 Obsidian 內編輯筆記。".to_string())
        }
        Connection::Rest { base_url, key } => save_note(&base_url, &key, path, content).await,
        Connection::Disconnected => Err(not_connected()),
    }
}

pub fn open_uri(db: &Database) -> Result<String, String> {
    // Opening Obsidian never depends on an API key and remains available when
    // the plugin is stopped or the user has revoked JARVIS access.
    if preference(db, MODE_PREF)?.as_deref() == Some("vault") {
        let stored = preference(db, VAULT_PREF)?.ok_or("請重新選取 Obsidian 資料庫。")?;
        let grant: VaultGrant =
            serde_json::from_str(&stored).map_err(|_| "請重新選取 Obsidian 資料庫。")?;
        grant.verify()?;
        Ok(format!(
            "obsidian://open?vault={}",
            urlencoding::encode(&grant.name())
        ))
    } else {
        Ok("obsidian://choose-vault".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    /// In-memory, not a file under /tmp: a file fixture leaks a database per
    /// test run because nothing deletes it, and the connection is all these
    /// tests need.
    fn temp_db() -> crate::db::Database {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE user_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .expect("create prefs table");
        crate::db::Database {
            conn: Mutex::new(conn),
        }
    }

    fn set_base_url(db: &crate::db::Database, value: &str) {
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO user_preferences (key, value) VALUES ('obsidian_base_url', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                rusqlite::params![value],
            )
            .unwrap();
    }

    #[test]
    fn absent_or_blank_override_falls_back_to_the_plugin_default() {
        let db = temp_db();
        assert_eq!(resolve_base_url(&db), DEFAULT_OBSIDIAN_URL);
        set_base_url(&db, "   ");
        assert_eq!(resolve_base_url(&db), DEFAULT_OBSIDIAN_URL);
    }

    /// F042: a user who moved the plugin off its default port needs a way in.
    #[test]
    fn a_configured_override_wins_and_loses_its_trailing_slash() {
        let db = temp_db();
        set_base_url(&db, "http://127.0.0.1:27123/");
        // A trailing slash would produce `//vault/`, which the plugin 404s.
        assert_eq!(resolve_base_url(&db), "http://127.0.0.1:27123");
    }

    #[test]
    fn endpoints_are_loopback_only_and_note_paths_cannot_change_the_request() {
        for endpoint in [
            "https://example.com",
            "https://127.0.0.1.evil.test",
            "https://user:pass@127.0.0.1",
            "https://127.0.0.1/path",
            "https://127.0.0.1?secret=x",
            "https://127.0.0.1#fragment",
            "file:///tmp/note",
            "http://192.168.1.5:27123",
            "http://[::]:27123",
        ] {
            assert!(validated_base_url(endpoint).is_err(), "accepted {endpoint}");
        }
        assert!(validated_base_url("http://[::1]:27123").is_ok());
        assert_eq!(
            validated_base_url("https://localhost:27124")
                .unwrap()
                .host_str(),
            Some("127.0.0.1")
        );
        let url = note_url(DEFAULT_OBSIDIAN_URL, "folder/a?b#c%2F.md").unwrap();
        assert_eq!(url.path(), "/vault/folder/a%3Fb%23c%252F.md");
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        assert!(note_url(DEFAULT_OBSIDIAN_URL, "../secret.md").is_err());
    }

    async fn fixture(
        body: &str,
        status: &str,
        extra_headers: &str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}", body.len());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") && request.len() < 16_384 {
                let count = socket.read(&mut buffer).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
            }
            socket.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        (format!("http://{address}"), task)
    }

    #[tokio::test]
    async fn rest_search_uses_documented_query_and_context_response() {
        let body = r#"[{"filename":"meeting.md","matches":[{"context":"香港公司","match":{"start":0,"end":2}}]},{"filename":".obsidian/hidden.md","matches":[]}]"#;
        let (base_url, task) = fixture(body, "200 OK", "").await;
        let notes = search_vault(&base_url, "synthetic-key", "香港 & 公司")
            .await
            .unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content.as_deref(), Some("香港公司"));
        let request = task.await.unwrap();
        let request_target = request.lines().next().unwrap().split(' ').nth(1).unwrap();
        let parsed = Url::parse(&format!("{base_url}{request_target}")).unwrap();
        assert_eq!(parsed.path(), "/search/simple/");
        assert_eq!(
            parsed
                .query_pairs()
                .find(|(name, _)| name == "query")
                .unwrap()
                .1,
            "香港 & 公司"
        );
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer synthetic-key"));
    }

    #[tokio::test]
    async fn redirect_is_rejected_without_forwarding_credentials() {
        let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (base_url, task) = fixture(
            "",
            "302 Found",
            &format!(
                "Location: http://{}/vault/\r\n",
                destination.local_addr().unwrap()
            ),
        )
        .await;
        let error = list_files(&base_url, "synthetic-private-key")
            .await
            .unwrap_err();
        assert!(error.contains("302"));
        assert!(!error.contains("synthetic-private-key"));
        task.await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), destination.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rest_status_requires_the_plugin_to_confirm_authentication() {
        for (body, expected) in [
            (
                r#"{"authenticated":true,"service":"Obsidian Local REST API"}"#,
                true,
            ),
            (
                r#"{"authenticated":false,"service":"Obsidian Local REST API"}"#,
                false,
            ),
            (
                r#"{"authenticated":true,"service":"unrelated service"}"#,
                false,
            ),
        ] {
            let (base_url, task) = fixture(body, "200 OK", "").await;
            assert_eq!(
                verify_rest(&base_url, "synthetic-key").await.is_ok(),
                expected
            );
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn vault_mode_is_read_only_and_disconnect_revokes_access() {
        let root = std::env::temp_dir().join(format!("jarvis-vault-mode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".obsidian")).unwrap();
        std::fs::write(root.join("note.md"), "approved fixture").unwrap();
        let db = temp_db();
        let grant = VaultGrant::authorize(&root).unwrap();
        store_vault(&db, &grant).unwrap();
        assert_eq!(
            read_connected(&db, "note.md").await.unwrap(),
            "approved fixture"
        );
        assert!(save_connected(&db, "note.md", "changed").await.is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("note.md")).unwrap(),
            "approved fixture"
        );
        let uri = Url::parse(&open_uri(&db).unwrap()).unwrap();
        assert_eq!(
            uri.query_pairs().find(|(key, _)| key == "vault").unwrap().1,
            grant.name()
        );
        disconnect(&db).unwrap();
        assert!(preference(&db, VAULT_PREF).unwrap().is_none());
        assert!(matches!(
            connection(&db).unwrap(),
            super::Connection::Disconnected
        ));
        assert!(read_connected(&db, "note.md").await.is_err());
        assert_eq!(open_uri(&db).unwrap(), "obsidian://choose-vault");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_completed_old_rest_read_is_discarded_after_disconnect_or_reconnect() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for disconnecting in [true, false] {
            let db = std::sync::Arc::new(temp_db());
            select_rest_mode(&db).unwrap();
            let generation = capture_generation(&db).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let (started, request_started) = tokio::sync::oneshot::channel();
            let (release, response_ready) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                socket.read(&mut [0; 2048]).await.unwrap();
                let _ = started.send(());
                response_ready.await.unwrap();
                let body = "previous connection private note";
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/markdown\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            });
            let read_db = db.clone();
            let reading = tokio::spawn(async move {
                with_generation(
                    &read_db,
                    &generation,
                    get_note(&base, "synthetic-key", "note.md"),
                )
                .await
            });
            request_started.await.unwrap();
            if disconnecting {
                disconnect(&db).unwrap();
            } else {
                select_rest_mode(&db).unwrap();
            }
            release.send(()).unwrap();
            let error = reading.await.unwrap().unwrap_err();
            assert!(error.contains("連接已變更"));
            assert!(!error.contains("previous connection private note"));
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn a_blocking_old_vault_read_cannot_return_after_selecting_another_vault() {
        let root = std::env::temp_dir().join(format!("jarvis-vault-race-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("old/.obsidian")).unwrap();
        std::fs::create_dir_all(root.join("new/.obsidian")).unwrap();
        std::fs::write(root.join("old/note.md"), "old private note").unwrap();
        std::fs::write(root.join("new/note.md"), "new note").unwrap();
        let db = std::sync::Arc::new(temp_db());
        let old = VaultGrant::authorize(&root.join("old")).unwrap();
        let new = VaultGrant::authorize(&root.join("new")).unwrap();
        store_vault(&db, &old).unwrap();
        let generation = capture_generation(&db).unwrap();
        let (started, waiting) = tokio::sync::oneshot::channel();
        let (release, continuation) = std::sync::mpsc::channel();
        let read_db = db.clone();
        let reading = tokio::spawn(async move {
            with_generation(&read_db, &generation, async move {
                tokio::task::spawn_blocking(move || {
                    let _ = started.send(());
                    continuation.recv().unwrap();
                    obsidian_vault::read_note(&old, "note.md")
                })
                .await
                .unwrap()
            })
            .await
        });
        waiting.await.unwrap();
        store_vault(&db, &new).unwrap();
        release.send(()).unwrap();
        let error = reading.await.unwrap().unwrap_err();
        assert!(error.contains("連接已變更"));
        assert!(!error.contains("old private note"));
        assert_eq!(read_connected(&db, "note.md").await.unwrap(), "new note");
        std::fs::remove_dir_all(root).unwrap();
    }
}
