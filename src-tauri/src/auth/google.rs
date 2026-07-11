use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken,
    PkceCodeChallenge, RedirectUrl, Scope, TokenUrl,
    AuthorizationCode, TokenResponse, RefreshToken, reqwest::async_http_client,
};
use std::sync::Mutex;
use tokio::sync::oneshot;

use crate::security::keychain::KeychainStore;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

// The Google refresh token is a long-lived credential and lives in the macOS
// Keychain, never in plaintext SQLite. The DB keeps only this non-secret marker
// so existence checks keep working.
const REFRESH_KEYCHAIN_SERVICE: &str = "com.jarvis.google";
const REFRESH_KEYCHAIN_ACCOUNT: &str = "refresh_token";
const REFRESH_KEYCHAIN_MARKER: &str = "__keychain__";

pub struct GoogleAuth {
    client_id: String,
    client_secret: String,
    pub access_token: Mutex<Option<String>>,
    pub refresh_token: Mutex<Option<String>>,
}

/// Resolve a credential by checking the runtime env var first, then falling back to
/// a value baked in at compile time via `option_env!`. Empty strings count as missing.
fn resolve_credential(env_key: &str, compile_time: Option<&'static str>) -> Option<String> {
    std::env::var(env_key)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| compile_time.map(str::to_string).filter(|v| !v.is_empty()))
}

impl GoogleAuth {
    /// Construct from credentials. Reads `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET`
    /// from the runtime env, falling back to values embedded at compile time. Returns
    /// `None` only if no client_id is available — packaged release builds inject these
    /// via the build environment so end users never need an .env file.
    pub fn new() -> Option<Self> {
        let client_id = resolve_credential(
            "GOOGLE_CLIENT_ID",
            option_env!("GOOGLE_CLIENT_ID"),
        )?;
        let client_secret = resolve_credential(
            "GOOGLE_CLIENT_SECRET",
            option_env!("GOOGLE_CLIENT_SECRET"),
        )
        .unwrap_or_default();
        Some(GoogleAuth {
            client_id,
            client_secret,
            access_token: Mutex::new(None),
            refresh_token: Mutex::new(None),
        })
    }

    pub fn new_empty() -> Self {
        GoogleAuth {
            client_id: String::new(),
            client_secret: String::new(),
            access_token: Mutex::new(None),
            refresh_token: Mutex::new(None),
        }
    }

    pub fn has_credentials(&self) -> bool {
        !self.client_id.is_empty()
    }

    fn build_client(&self, redirect_port: u16) -> Result<BasicClient, String> {
        if self.client_id.is_empty() {
            return Err(
                "Google sign-in is not configured for this build. Please contact support."
                    .to_string(),
            );
        }
        let secret = if self.client_secret.is_empty() {
            None
        } else {
            Some(ClientSecret::new(self.client_secret.clone()))
        };
        let client = BasicClient::new(
            ClientId::new(self.client_id.clone()),
            secret,
            AuthUrl::new(AUTH_URL.to_string()).map_err(|e| e.to_string())?,
            Some(TokenUrl::new(TOKEN_URL.to_string()).map_err(|e| e.to_string())?),
        )
        .set_redirect_uri(
            RedirectUrl::new(format!("http://127.0.0.1:{}", redirect_port))
                .map_err(|e| e.to_string())?,
        );
        Ok(client)
    }

    pub async fn start_auth_flow(&self, scopes: Vec<String>) -> Result<(), String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await.map_err(|e| format!("Failed to bind listener: {}", e))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();

        let client = self.build_client(port)?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let mut auth_request = client.authorize_url(CsrfToken::new_random);
        for scope in &scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }
        auth_request = auth_request.add_extra_param("access_type", "offline");
        auth_request = auth_request.add_extra_param("prompt", "consent");

        let (auth_url, _csrf_token) = auth_request.set_pkce_challenge(pkce_challenge).url();

        open::that(auth_url.to_string()).map_err(|e| format!("Failed to open browser: {}", e))?;
        log::info!("Opened browser for Google OAuth at port {}", port);

        let (tx, rx) = oneshot::channel::<Result<String, String>>();
        let tx = std::sync::Mutex::new(Some(tx));

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                if let Ok(n) = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
                    let request = String::from_utf8_lossy(&buf[..n]);
                    if let Some(err) = extract_error(&request) {
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>JARVIS</h1><p>Authentication failed. You can close this tab.</p></body></html>";
                        let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send(Err(err));
                        }
                    } else if let Some(code) = extract_code(&request) {
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>JARVIS</h1><p>Authentication successful. You can close this tab.</p></body></html>";
                        let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send(Ok(code));
                        }
                    }
                }
            }
        });

        let code = tokio::time::timeout(std::time::Duration::from_secs(180), rx)
            .await.map_err(|_| "OAuth timeout: no response within 3 minutes. Please try again.".to_string())?
            .map_err(|_| "OAuth cancelled".to_string())?
            .map_err(|e| format!("Google OAuth denied: {}", e))?;

        let exchange_future = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client);
        let token_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            exchange_future,
        )
        .await
        .map_err(|_| {
            "Google did not respond within 30 seconds while exchanging the authorization code. \
             Check your network and try again."
                .to_string()
        })?
        .map_err(|e| format!("Token exchange failed: {}", e))?;

        let access = token_result.access_token().secret().clone();
        let refresh = token_result.refresh_token().map(|t| t.secret().clone());

        *self.access_token.lock().unwrap() = Some(access);
        *self.refresh_token.lock().unwrap() = refresh;

        log::info!("Google OAuth completed successfully");
        Ok(())
    }

    pub async fn refresh_access_token(&self) -> Result<(), String> {
        let refresh = self.refresh_token.lock().unwrap().clone()
            .ok_or("No refresh token available")?;
        let client = self.build_client(0)?;
        let refresh_token = RefreshToken::new(refresh);
        let refresh_future = client
            .exchange_refresh_token(&refresh_token)
            .request_async(async_http_client);
        let token_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            refresh_future,
        )
        .await
        .map_err(|_| "Google did not respond within 30 seconds while refreshing the access token.".to_string())?
        .map_err(|e| format!("Token refresh failed: {}", e))?;

        *self.access_token.lock().unwrap() = Some(token_result.access_token().secret().clone());
        if let Some(new_refresh) = token_result.refresh_token() {
            *self.refresh_token.lock().unwrap() = Some(new_refresh.secret().clone());
        }
        log::info!("Google access token refreshed");
        Ok(())
    }

    pub fn get_access_token(&self) -> Option<String> {
        self.access_token.lock().unwrap().clone()
    }

    pub fn has_refresh_token(&self) -> bool {
        self.refresh_token.lock().unwrap().is_some()
    }

    pub fn is_authenticated(&self) -> bool {
        self.access_token.lock().unwrap().is_some()
    }

    /// Returns true if we have either a live access token or a stored refresh token.
    pub fn is_linked(&self) -> bool {
        self.is_authenticated() || self.has_refresh_token()
    }

    /// Ensures an access token is available, refreshing from the stored refresh
    /// token if needed. Returns the access token or an error.
    pub async fn ensure_access_token(&self) -> Result<String, String> {
        if let Some(token) = self.get_access_token() {
            return Ok(token);
        }
        if !self.has_refresh_token() {
            return Err("Not authenticated with Google".to_string());
        }
        self.refresh_access_token().await?;
        self.get_access_token()
            .ok_or_else(|| "Token refresh succeeded but no token available".to_string())
    }

    pub fn load_from_db(&self, db: &crate::db::Database) {
        // Preferred source: the Keychain.
        if let Ok(token) = KeychainStore::get(REFRESH_KEYCHAIN_SERVICE, REFRESH_KEYCHAIN_ACCOUNT) {
            if !token.is_empty() {
                *self.refresh_token.lock().unwrap() = Some(token);
                return;
            }
        }

        // Backward compatibility: older builds stored the refresh token in
        // plaintext in user_preferences. Read the DB row (tight lock scope, no
        // Keychain FFI while holding it); if it's a real token, adopt it and
        // migrate it into the Keychain, replacing the plaintext with a marker so
        // the credential no longer sits on disk. Existing users stay logged in.
        let db_value: Option<String> = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = 'google_refresh_token'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
        };
        if let Some(token) = db_value {
            if token != REFRESH_KEYCHAIN_MARKER && !token.is_empty() {
                *self.refresh_token.lock().unwrap() = Some(token.clone());
                if KeychainStore::put(
                    REFRESH_KEYCHAIN_SERVICE,
                    REFRESH_KEYCHAIN_ACCOUNT,
                    &token,
                )
                .is_ok()
                {
                    let conn = db.conn.lock().unwrap();
                    let _ = conn.execute(
                        "UPDATE user_preferences SET value = ?1, updated_at = datetime('now') WHERE key = 'google_refresh_token'",
                        rusqlite::params![REFRESH_KEYCHAIN_MARKER],
                    );
                }
            }
        }
    }

    pub fn save_to_db(&self, db: &crate::db::Database) {
        // Clone the secret out and release the lock before any Keychain FFI.
        let token = self.refresh_token.lock().unwrap().clone();
        if let Some(token) = token {
            if KeychainStore::put(REFRESH_KEYCHAIN_SERVICE, REFRESH_KEYCHAIN_ACCOUNT, &token).is_ok()
            {
                let conn = db.conn.lock().unwrap();
                let _ = conn.execute(
                    "INSERT INTO user_preferences (key, value, updated_at) VALUES ('google_refresh_token', ?1, datetime('now'))
                     ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
                    rusqlite::params![REFRESH_KEYCHAIN_MARKER],
                );
            }
        }
    }
}

fn extract_error(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for param in query.split('&') {
        let mut parts = param.splitn(2, '=');
        if parts.next()? == "error" {
            return parts.next().map(|s| s.replace('+', " ").to_string());
        }
    }
    None
}

fn extract_code(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for param in query.split('&') {
        let mut parts = param.splitn(2, '=');
        if parts.next()? == "code" {
            return parts.next().map(|s| s.to_string());
        }
    }
    None
}
