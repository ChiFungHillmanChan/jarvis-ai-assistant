use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken,
    PkceCodeChallenge, RedirectUrl, Scope, TokenUrl,
    AuthorizationCode, TokenResponse, RefreshToken, reqwest::async_http_client,
};
use std::sync::Mutex;
use tokio::sync::oneshot;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub struct GoogleAuth {
    client_id: String,
    client_secret: String,
    pub access_token: Mutex<Option<String>>,
    pub refresh_token: Mutex<Option<String>>,
}

impl GoogleAuth {
    pub fn new() -> Option<Self> {
        let client_id = std::env::var("GOOGLE_CLIENT_ID").ok()?;
        let client_secret = std::env::var("GOOGLE_CLIENT_SECRET").ok()?;
        if client_id.is_empty() || client_secret.is_empty() { return None; }
        Some(GoogleAuth {
            client_id, client_secret,
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

    fn build_client(&self, redirect_port: u16) -> Result<BasicClient, String> {
        let client = BasicClient::new(
            ClientId::new(self.client_id.clone()),
            Some(ClientSecret::new(self.client_secret.clone())),
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

        let (tx, rx) = oneshot::channel::<String>();
        let tx = std::sync::Mutex::new(Some(tx));

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                if let Ok(n) = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
                    let request = String::from_utf8_lossy(&buf[..n]);
                    if let Some(code) = extract_code(&request) {
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>JARVIS</h1><p>Authentication successful. You can close this tab.</p></body></html>";
                        let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send(code);
                        }
                    }
                }
            }
        });

        let code = tokio::time::timeout(std::time::Duration::from_secs(120), rx)
            .await.map_err(|_| "OAuth timeout: no response within 120 seconds".to_string())?
            .map_err(|_| "OAuth channel closed".to_string())?;

        let token_result = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client)
            .await.map_err(|e| format!("Token exchange failed: {}", e))?;

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
        let token_result = client
            .exchange_refresh_token(&RefreshToken::new(refresh))
            .request_async(async_http_client)
            .await.map_err(|e| format!("Token refresh failed: {}", e))?;

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

    pub fn is_authenticated(&self) -> bool {
        self.access_token.lock().unwrap().is_some()
    }

    pub fn load_from_db(&self, db: &crate::db::Database) {
        let conn = db.conn.lock().unwrap();
        if let Ok(token) = conn.query_row(
            "SELECT value FROM user_preferences WHERE key = 'google_refresh_token'",
            [], |row| row.get::<_, String>(0),
        ) {
            *self.refresh_token.lock().unwrap() = Some(token);
        }
    }

    pub fn save_to_db(&self, db: &crate::db::Database) {
        if let Some(ref token) = *self.refresh_token.lock().unwrap() {
            let conn = db.conn.lock().unwrap();
            let _ = conn.execute(
                "INSERT INTO user_preferences (key, value, updated_at) VALUES ('google_refresh_token', ?1, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
                rusqlite::params![token],
            );
        }
    }
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
