//! OAuth 2.0 for installed apps: loopback redirect + PKCE (RFC 8252/7636).

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::token_store::TokenStore;
use crate::{DriveError, Result, SCOPES};

const GOOGLE_AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_REVOKE_URI: &str = "https://oauth2.googleapis.com/revoke";

/// The "Desktop app" OAuth client from Google Cloud console. For installed
/// apps the client secret is not confidential, but Google still requires it
/// at the token endpoint.
#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub auth_uri: String,
    pub token_uri: String,
    pub revoke_uri: String,
}

#[derive(Deserialize)]
struct InstalledJson {
    installed: InstalledInner,
}

#[derive(Deserialize)]
struct InstalledInner {
    client_id: String,
    client_secret: Option<String>,
    auth_uri: Option<String>,
    token_uri: Option<String>,
}

impl OAuthClient {
    pub fn new(client_id: impl Into<String>, client_secret: Option<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret,
            auth_uri: GOOGLE_AUTH_URI.into(),
            token_uri: GOOGLE_TOKEN_URI.into(),
            revoke_uri: GOOGLE_REVOKE_URI.into(),
        }
    }

    /// Parse the JSON file Google Cloud console offers for download
    /// (`{"installed": {...}}`).
    pub fn from_installed_json(bytes: &[u8]) -> Result<Self> {
        let j: InstalledJson = serde_json::from_slice(bytes).map_err(|e| {
            DriveError::NotConfigured(format!("not a Desktop-app OAuth client JSON: {e}"))
        })?;
        let mut c = Self::new(j.installed.client_id, j.installed.client_secret);
        if let Some(a) = j.installed.auth_uri {
            c.auth_uri = a;
        }
        if let Some(t) = j.installed.token_uri {
            c.token_uri = t;
        }
        Ok(c)
    }
}

fn random_urlsafe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

/// A sign-in in progress: open `auth_url` in the system browser, then await
/// [`PendingAuth::finish`].
pub struct PendingAuth {
    pub auth_url: String,
    listener: TcpListener,
    verifier: String,
    state: String,
    redirect_uri: String,
}

impl PendingAuth {
    pub async fn begin(client: &OAuthClient) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}");
        let verifier = random_urlsafe(48);
        let state = random_urlsafe(16);
        let mut url = url::Url::parse(&client.auth_uri)
            .map_err(|e| DriveError::NotConfigured(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("client_id", &client.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", &SCOPES.join(" "))
            .append_pair("code_challenge", &pkce_challenge(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");
        Ok(Self { auth_url: url.into(), listener, verifier, state, redirect_uri })
    }

    /// Wait for the browser redirect (up to `timeout`), then exchange the
    /// code. The refresh token is saved to `store`.
    pub async fn finish(
        self,
        client: &OAuthClient,
        http: &reqwest::Client,
        store: &dyn TokenStore,
        timeout: Duration,
    ) -> Result<Auth> {
        let code = tokio::time::timeout(timeout, self.wait_for_code())
            .await
            .map_err(|_| DriveError::AuthFailed("timed out waiting for the browser".into()))??;
        let mut form = vec![
            ("code", code.as_str()),
            ("client_id", client.client_id.as_str()),
            ("code_verifier", self.verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", self.redirect_uri.as_str()),
        ];
        if let Some(s) = &client.client_secret {
            form.push(("client_secret", s));
        }
        let resp = http.post(&client.token_uri).form(&form).send().await?;
        let tokens = parse_token_response(resp).await?;
        let refresh = tokens
            .refresh_token
            .clone()
            .ok_or_else(|| DriveError::AuthFailed("Google did not return a refresh token".into()))?;
        store.save(&refresh)?;
        let auth = Auth::new(client.clone(), http.clone());
        auth.set_access(tokens.access_token, tokens.expires_in).await;
        Ok(auth)
    }

    async fn wait_for_code(&self) -> Result<String> {
        loop {
            let (mut sock, _) = self.listener.accept().await?;
            let mut buf = vec![0u8; 8192];
            let mut n = 0;
            // Read until the end of the request line/headers (or buffer full).
            while n < buf.len() {
                let r = sock.read(&mut buf[n..]).await?;
                if r == 0 {
                    break;
                }
                n += r;
                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&buf[..n]);
            let target = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
            let url = url::Url::parse(&format!("http://127.0.0.1{target}"))
                .map_err(|e| DriveError::AuthFailed(e.to_string()))?;
            let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            if params.is_empty() {
                // favicon.ico and friends.
                let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            }
            let result = if let Some(err) = params.get("error") {
                Err(DriveError::AuthFailed(err.clone()))
            } else if params.get("state") != Some(&self.state) {
                Err(DriveError::AuthFailed("state mismatch".into()))
            } else if let Some(code) = params.get("code") {
                Ok(code.clone())
            } else {
                Err(DriveError::AuthFailed("no code in redirect".into()))
            };
            let page = if result.is_ok() { SUCCESS_PAGE } else { FAILURE_PAGE };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                page.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(page.as_bytes()).await;
            let _ = sock.shutdown().await;
            return result;
        }
    }
}

const SUCCESS_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>BiblioChad</title>
<style>body{font:18px system-ui;background:#14110f;color:#f3ead8;display:grid;place-items:center;height:100vh;margin:0}
b{color:#f5b942}</style><div><h1>Connected. <b>Chad</b> has the keys.</h1><p>You can close this tab and get back to reading.</p></div>"#;

const FAILURE_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>BiblioChad</title>
<style>body{font:18px system-ui;background:#14110f;color:#f3ead8;display:grid;place-items:center;height:100vh;margin:0}</style>
<div><h1>Sign-in did not complete.</h1><p>Close this tab and try Connect Google Drive again.</p></div>"#;

async fn parse_token_response(resp: reqwest::Response) -> Result<TokenResponse> {
    let status = resp.status();
    let body = resp.bytes().await?;
    if status.is_success() {
        return serde_json::from_slice(&body).map_err(|e| DriveError::Invalid(e.to_string()));
    }
    match serde_json::from_slice::<TokenError>(&body) {
        Ok(e) if e.error == "invalid_grant" => Err(DriveError::ReauthRequired),
        Ok(e) => Err(DriveError::AuthFailed(format!(
            "{}{}",
            e.error,
            e.error_description.map(|d| format!(": {d}")).unwrap_or_default()
        ))),
        Err(_) => Err(DriveError::Api {
            status: status.as_u16(),
            message: String::from_utf8_lossy(&body).chars().take(300).collect(),
        }),
    }
}

/// Source of access tokens. Clone-cheap; refreshes on demand.
#[derive(Clone)]
pub struct Auth {
    inner: Arc<AuthInner>,
}

struct AuthInner {
    client: Option<OAuthClient>,
    http: reqwest::Client,
    access: Mutex<Option<(String, Instant)>>,
}

impl Auth {
    pub fn new(client: OAuthClient, http: reqwest::Client) -> Self {
        Self { inner: Arc::new(AuthInner { client: Some(client), http, access: Mutex::new(None) }) }
    }

    /// A fixed token that never refreshes (tests, tooling).
    pub fn fixed(token: &str) -> Self {
        Self {
            inner: Arc::new(AuthInner {
                client: None,
                http: reqwest::Client::new(),
                access: Mutex::new(Some((token.into(), Instant::now() + Duration::from_secs(10 * 365 * 86400)))),
            }),
        }
    }

    async fn set_access(&self, token: String, expires_in: Option<u64>) {
        let ttl = Duration::from_secs(expires_in.unwrap_or(3600).saturating_sub(60).max(30));
        *self.inner.access.lock().await = Some((token, Instant::now() + ttl));
    }

    /// Forget the cached access token (e.g. after a 401).
    pub async fn invalidate(&self) {
        if self.inner.client.is_some() {
            *self.inner.access.lock().await = None;
        }
    }

    /// A valid access token, refreshing with the stored refresh token if
    /// needed. A revoked/expired refresh token is removed from `store` and
    /// reported as [`DriveError::ReauthRequired`].
    pub async fn access_token(&self, store: &dyn TokenStore) -> Result<String> {
        let mut guard = self.inner.access.lock().await;
        if let Some((t, exp)) = guard.as_ref() {
            if Instant::now() < *exp {
                return Ok(t.clone());
            }
        }
        let client = self.inner.client.as_ref().ok_or(DriveError::ReauthRequired)?;
        let refresh = store.load()?.ok_or(DriveError::NotConnected)?;
        let mut form = vec![
            ("client_id", client.client_id.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ];
        if let Some(s) = &client.client_secret {
            form.push(("client_secret", s));
        }
        let resp = self.inner.http.post(&client.token_uri).form(&form).send().await?;
        match parse_token_response(resp).await {
            Ok(t) => {
                if let Some(r) = &t.refresh_token {
                    store.save(r)?;
                }
                let ttl = Duration::from_secs(t.expires_in.unwrap_or(3600).saturating_sub(60).max(30));
                *guard = Some((t.access_token.clone(), Instant::now() + ttl));
                Ok(t.access_token)
            }
            Err(DriveError::ReauthRequired) => {
                store.clear()?;
                Err(DriveError::ReauthRequired)
            }
            Err(e) => Err(e),
        }
    }

    /// Revoke the refresh token at Google and clear it locally.
    pub async fn revoke(&self, store: &dyn TokenStore) -> Result<()> {
        if let (Some(client), Some(t)) = (self.inner.client.as_ref(), store.load()?) {
            // Best effort: a failure here must not block a local wipe.
            let _ = self.inner.http.post(&client.revoke_uri).form(&[("token", t.as_str())]).send().await;
        }
        store.clear()?;
        *self.inner.access.lock().await = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_store::MemoryStore;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(server: &MockServer) -> OAuthClient {
        let mut c = OAuthClient::new("cid", Some("secret".into()));
        c.token_uri = format!("{}/token", server.uri());
        c.revoke_uri = format!("{}/revoke", server.uri());
        c
    }

    #[test]
    fn parses_installed_json_and_pkce() {
        let c = OAuthClient::from_installed_json(
            br#"{"installed":{"client_id":"abc.apps.googleusercontent.com","client_secret":"s","token_uri":"https://oauth2.googleapis.com/token"}}"#,
        )
        .unwrap();
        assert_eq!(c.client_id, "abc.apps.googleusercontent.com");
        assert!(OAuthClient::from_installed_json(br#"{"web":{}}"#).is_err());
        // BASE64URL(SHA256(verifier)) without padding.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mJ92K9JDOyUbz6vtHw4Yj5cWwQ8vW4"),
            "07Zw9MRzeYj_DBwangYSWXoic3EJMbE1SFRAlu0zCEA"
        );
    }

    #[tokio::test]
    async fn loopback_flow_exchanges_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=the-code"))
            .and(body_string_contains("code_verifier="))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at1", "expires_in": 3600, "refresh_token": "rt1"
            })))
            .mount(&server)
            .await;
        let client = client_for(&server);
        let pending = PendingAuth::begin(&client).await.unwrap();
        let auth_url = url::Url::parse(&pending.auth_url).unwrap();
        let q: std::collections::HashMap<_, _> = auth_url.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(q["scope"].contains("drive.readonly") && q["scope"].contains("drive.appdata"));
        let redirect = q["redirect_uri"].clone();
        let state = q["state"].clone();

        // Simulate the browser: a favicon request, then the real redirect.
        let browser = tokio::spawn(async move {
            let http = reqwest::Client::new();
            let _ = http.get(format!("{redirect}/favicon.ico")).send().await;
            http.get(format!("{redirect}/?state={state}&code=the-code")).send().await.unwrap().text().await.unwrap()
        });
        let store = MemoryStore::default();
        let auth = pending
            .finish(&client, &reqwest::Client::new(), &store, Duration::from_secs(10))
            .await
            .unwrap();
        assert!(browser.await.unwrap().contains("Connected"));
        assert_eq!(store.load().unwrap().as_deref(), Some("rt1"));
        assert_eq!(auth.access_token(&store).await.unwrap(), "at1");
    }

    #[tokio::test]
    async fn rejects_state_mismatch() {
        let server = MockServer::start().await;
        let client = client_for(&server);
        let pending = PendingAuth::begin(&client).await.unwrap();
        let q: std::collections::HashMap<_, _> =
            url::Url::parse(&pending.auth_url).unwrap().query_pairs().into_owned().collect();
        let redirect = q["redirect_uri"].clone();
        tokio::spawn(async move {
            let _ = reqwest::get(format!("{redirect}/?state=evil&code=x")).await;
        });
        let r = pending
            .finish(&client, &reqwest::Client::new(), &MemoryStore::default(), Duration::from_secs(10))
            .await;
        assert!(matches!(r, Err(DriveError::AuthFailed(_))));
    }

    #[tokio::test]
    async fn refresh_and_invalid_grant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("refresh_token=good"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fresh", "expires_in": 3600
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("refresh_token=revoked"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant", "error_description": "Token has been expired or revoked."
            })))
            .mount(&server)
            .await;
        let client = client_for(&server);
        let auth = Auth::new(client.clone(), reqwest::Client::new());
        let good = MemoryStore::with("good");
        assert_eq!(auth.access_token(&good).await.unwrap(), "fresh");

        let auth = Auth::new(client.clone(), reqwest::Client::new());
        let bad = MemoryStore::with("revoked");
        assert!(matches!(auth.access_token(&bad).await, Err(DriveError::ReauthRequired)));
        assert_eq!(bad.load().unwrap(), None, "dead token is cleared");

        let none = MemoryStore::default();
        assert!(matches!(auth.access_token(&none).await, Err(DriveError::NotConnected)));
    }
}
