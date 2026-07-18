//! GitHub OAuth (user-to-server) login flow + session cookies.
//!
//! Start sets a CSRF cookie and redirects to GitHub; callback verifies the
//! state, exchanges the code, stores the user, and signs a session cookie.

use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Duration, Utc};
use cookie::time;
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::Row;
use thiserror::Error;
use url::Url;

use crate::api::ApiState;
use crate::crypto::Crypto;
use crate::db::Db;

const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_URL: &str = "https://api.github.com/user";
const SESSION_COOKIE: &str = "session";
const CSRF_COOKIE: &str = "oauth_csrf";
const SESSION_TTL_DAYS: i64 = 30;

/// Configuration loaded from env. Optional — without these, the auth
/// routes return 503 instead of crashing the server, so the app still
/// runs in environments that haven't registered a GitHub App yet.
#[derive(Clone)]
pub struct GithubAppConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub session_secret: Vec<u8>,
    /// Whether to mark cookies `Secure`. True in production (HTTPS),
    /// false in local dev so cookies travel over plain HTTP.
    pub cookie_secure: bool,
    /// HMAC secret for verifying inbound webhooks. Optional but expected
    /// in any real deployment.
    pub webhook_secret: Option<Vec<u8>>,
    /// Encrypts/decrypts user OAuth tokens at rest. Required when the
    /// GitHub App is configured — main.rs refuses to start otherwise.
    pub crypto: Crypto,
}

impl GithubAppConfig {
    /// Load from env. Returns `Ok(None)` when the GitHub App isn't
    /// configured (auth routes 503). Returns `Err` when partially
    /// configured but missing critical secrets — better to crash on
    /// boot than to start serving traffic with a broken auth flow.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(client_id) = std::env::var("GITHUB_APP_CLIENT_ID").ok() else {
            return Ok(None);
        };
        if client_id.trim().is_empty() {
            return Ok(None);
        }
        let client_secret = std::env::var("GITHUB_APP_CLIENT_SECRET").map_err(|_| {
            anyhow::anyhow!("GITHUB_APP_CLIENT_ID set but GITHUB_APP_CLIENT_SECRET missing")
        })?;
        if client_secret.is_empty() {
            return Err(anyhow::anyhow!(
                "GITHUB_APP_CLIENT_SECRET must not be empty"
            ));
        }
        let redirect_uri = std::env::var("OAUTH_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:8787/auth/github/callback".to_string());
        let redirect_url = Url::parse(&redirect_uri)
            .map_err(|e| anyhow::anyhow!("OAUTH_REDIRECT_URI is invalid: {e}"))?;
        if !matches!(redirect_url.scheme(), "http" | "https")
            || redirect_url.host_str().is_none()
            || redirect_url.fragment().is_some()
        {
            return Err(anyhow::anyhow!(
                "OAUTH_REDIRECT_URI must be an absolute http(s) URL without a fragment"
            ));
        }
        // SESSION_SECRET handling diverges by build profile:
        //   - debug builds: an ephemeral per-process key is generated
        //     (sessions die on restart, fine for `cargo run`).
        //   - release builds: the env var is mandatory. A silent
        //     fallback in production logs out every user on every
        //     redeploy and masks a misconfigured deploy. Better to
        //     refuse to start.
        let session_secret = match std::env::var("SESSION_SECRET").ok() {
            Some(s) if s.len() >= 32 => s.into_bytes(),
            Some(_) => {
                return Err(anyhow::anyhow!(
                    "SESSION_SECRET must contain at least 32 bytes"
                ));
            }
            _ => {
                if cfg!(debug_assertions) {
                    tracing::warn!(
                        "SESSION_SECRET unset; generating an ephemeral key \
                         (sessions won't survive restart). Debug build only."
                    );
                    let mut buf = vec![0u8; 64];
                    rand::rng().fill_bytes(&mut buf);
                    buf
                } else {
                    return Err(anyhow::anyhow!(
                        "GitHub App is configured but SESSION_SECRET is unset. \
                         Generate with `openssl rand -hex 64` and set in env. \
                         Refusing to start — without it, every redeploy would \
                         silently log every user out and mask misconfiguration."
                    ));
                }
            }
        };
        let cookie_secure = std::env::var("COOKIE_SECURE")
            .ok()
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if redirect_url.scheme() == "https" && !cookie_secure {
            return Err(anyhow::anyhow!(
                "COOKIE_SECURE=1 is required with an HTTPS OAuth redirect"
            ));
        }
        let webhook_secret = std::env::var("GITHUB_WEBHOOK_SECRET")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.into_bytes());
        let crypto = Crypto::from_env()?.ok_or_else(|| {
            anyhow::anyhow!(
                "GitHub App configured but TOKEN_ENCRYPTION_KEY is unset. \
                 Generate with `openssl rand -base64 32` and set in env. \
                 Refusing to start — without it, OAuth tokens would land in the \
                 database in plaintext."
            )
        })?;
        Ok(Some(Self {
            client_id,
            client_secret,
            redirect_uri,
            session_secret,
            cookie_secure,
            webhook_secret,
            crypto,
        }))
    }
}

pub fn router() -> Router<ApiState> {
    Router::new()
        .route("/auth/github/start", get(login_start))
        .route("/auth/github/callback", get(login_callback))
        .route("/auth/logout", post(logout))
        .route("/api/me", get(me))
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("GitHub App not configured (set GITHUB_APP_CLIENT_ID etc)")]
    NotConfigured,
    #[error("missing CSRF state cookie")]
    CsrfMissing,
    #[error("CSRF mismatch")]
    CsrfMismatch,
    #[error("missing oauth code")]
    MissingCode,
    #[error("github oauth error: {0}")]
    Github(String),
    #[error("token exchange failed: {0}")]
    TokenExchange(String),
    #[error("user fetch failed: {0}")]
    UserFetch(String),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AuthError::NotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "authentication unavailable",
            ),
            AuthError::CsrfMissing => (StatusCode::BAD_REQUEST, "login state cookie missing"),
            AuthError::CsrfMismatch => (StatusCode::BAD_REQUEST, "login state mismatch"),
            AuthError::MissingCode => (StatusCode::BAD_REQUEST, "authorization code missing"),
            AuthError::Github(_) => (StatusCode::BAD_REQUEST, "GitHub authorization failed"),
            AuthError::TokenExchange(_) | AuthError::UserFetch(_) => {
                (StatusCode::BAD_GATEWAY, "GitHub authentication failed")
            }
            AuthError::Sqlx(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
        };
        tracing::warn!(error = %self, "auth error");
        (status, message).into_response()
    }
}

async fn login_start(
    State(state): State<ApiState>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AuthError> {
    let cfg = state.gh_app.as_ref().ok_or(AuthError::NotConfigured)?;
    let csrf = random_hex(32);
    let csrf_cookie = build_cookie(CSRF_COOKIE, csrf.clone(), cfg.cookie_secure)
        .max_age(time::Duration::minutes(10))
        .build();

    let mut auth = Url::parse(GITHUB_AUTHORIZE_URL).expect("static url");
    auth.query_pairs_mut()
        .append_pair("client_id", &cfg.client_id)
        .append_pair("redirect_uri", &cfg.redirect_uri)
        .append_pair("state", &csrf);

    Ok((jar.add(csrf_cookie), Redirect::temporary(auth.as_str())))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)] // token_type kept for parity with GitHub's response shape
struct GithubTokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: String,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
    refresh_token_expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct GithubUserResponse {
    id: i64,
    login: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

async fn login_callback(
    State(state): State<ApiState>,
    Query(q): Query<CallbackQuery>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AuthError> {
    let cfg = state.gh_app.as_ref().ok_or(AuthError::NotConfigured)?;

    if let Some(err) = q.error {
        let detail = q.error_description.unwrap_or_default();
        return Err(AuthError::Github(format!("{err}: {detail}")));
    }
    let code = q.code.ok_or(AuthError::MissingCode)?;
    let received_state = q.state.unwrap_or_default();
    let csrf_cookie = jar.get(CSRF_COOKIE).ok_or(AuthError::CsrfMissing)?;
    if !constant_time_eq(csrf_cookie.value().as_bytes(), received_state.as_bytes()) {
        return Err(AuthError::CsrfMismatch);
    }

    // Fresh reqwest client — the auth flow shouldn't piggy-back on the
    // GithubClient's rate-limited path; this isn't a rate-counted call
    // and it's also against github.com (not api.github.com).
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AuthError::TokenExchange(e.to_string()))?;
    let token: GithubTokenResponse = http
        .post(GITHUB_TOKEN_URL)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", cfg.redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|e| AuthError::TokenExchange(e.to_string()))?
        .error_for_status()
        .map_err(|e| AuthError::TokenExchange(e.to_string()))?
        .json()
        .await
        .map_err(|e| AuthError::TokenExchange(e.to_string()))?;

    let user: GithubUserResponse = http
        .get(GITHUB_USER_URL)
        .bearer_auth(&token.access_token)
        .header("User-Agent", "gitdebt/0.1")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AuthError::UserFetch(e.to_string()))?
        .error_for_status()
        .map_err(|e| AuthError::UserFetch(e.to_string()))?
        .json()
        .await
        .map_err(|e| AuthError::UserFetch(e.to_string()))?;

    let now = Utc::now();
    let token_exp = token.expires_in.map(|s| now + Duration::seconds(s));
    let refresh_exp = token
        .refresh_token_expires_in
        .map(|s| now + Duration::seconds(s));

    // Tokens go in encrypted. The DB sees only ciphertext; decryption
    // happens at use-time in `github_client_for_user`.
    let access_enc = cfg
        .crypto
        .encrypt(&token.access_token)
        .map_err(|e| AuthError::TokenExchange(format!("encrypt access_token: {e}")))?;
    let refresh_enc = match token.refresh_token.as_deref() {
        Some(rt) => Some(
            cfg.crypto
                .encrypt(rt)
                .map_err(|e| AuthError::TokenExchange(format!("encrypt refresh_token: {e}")))?,
        ),
        None => None,
    };

    sqlx::query(
        "INSERT INTO app_users (id, login, name, avatar_url, email, access_token, \
                                refresh_token, token_expires_at, refresh_token_expires_at, \
                                created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10) \
         ON CONFLICT (id) DO UPDATE SET \
            login = EXCLUDED.login, \
            name = EXCLUDED.name, \
            avatar_url = EXCLUDED.avatar_url, \
            email = EXCLUDED.email, \
            access_token = EXCLUDED.access_token, \
            refresh_token = EXCLUDED.refresh_token, \
            token_expires_at = EXCLUDED.token_expires_at, \
            refresh_token_expires_at = EXCLUDED.refresh_token_expires_at, \
            updated_at = EXCLUDED.updated_at",
    )
    .bind(user.id)
    .bind(&user.login)
    .bind(&user.name)
    .bind(&user.avatar_url)
    .bind(&user.email)
    .bind(&access_enc)
    .bind(&refresh_enc)
    .bind(token_exp)
    .bind(refresh_exp)
    .bind(now)
    .execute(&state.analyzer.cache.db().pool)
    .await?;

    let session_value = sign_session(
        &cfg.session_secret,
        user.id,
        now + Duration::days(SESSION_TTL_DAYS),
    );
    let session_cookie = build_cookie(SESSION_COOKIE, session_value, cfg.cookie_secure)
        .max_age(time::Duration::days(SESSION_TTL_DAYS))
        .build();

    // Clear the CSRF cookie now that we've used it.
    let jar = jar
        .remove(removal_cookie(CSRF_COOKIE, cfg.cookie_secure))
        .add(session_cookie);
    Ok((jar, Redirect::to(&state.frontend_origin)))
}

async fn logout(State(state): State<ApiState>, jar: CookieJar) -> (CookieJar, Redirect) {
    let secure = state
        .gh_app
        .as_ref()
        .is_some_and(|config| config.cookie_secure);
    let jar = jar.remove(removal_cookie(SESSION_COOKIE, secure));
    (jar, Redirect::to(&state.frontend_origin))
}

#[derive(Serialize)]
struct MeResponse {
    id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
}

async fn me(State(state): State<ApiState>, jar: CookieJar) -> Result<Response, AuthError> {
    let Some(cfg) = state.gh_app.as_ref() else {
        return Ok(no_store((StatusCode::UNAUTHORIZED).into_response()));
    };
    let Some(user_id) = current_user_id(cfg, &jar) else {
        return Ok(no_store((StatusCode::UNAUTHORIZED).into_response()));
    };
    let row = sqlx::query("SELECT id, login, name, avatar_url, email FROM app_users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.analyzer.cache.db().pool)
        .await?;
    match row {
        Some(row) => {
            let me = MeResponse {
                id: row.try_get("id").map_err(AuthError::Sqlx)?,
                login: row.try_get("login").map_err(AuthError::Sqlx)?,
                name: row.try_get("name").ok(),
                avatar_url: row.try_get("avatar_url").ok(),
                email: row.try_get("email").ok(),
            };
            Ok(no_store(Json(me).into_response()))
        }
        None => Ok(no_store((StatusCode::UNAUTHORIZED).into_response())),
    }
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
}

/// Decode the session cookie and return the GitHub user id if valid and
/// not expired. Constant-time signature compare; any tampering returns
/// None rather than leaking which part of the token was wrong.
pub fn current_user_id(cfg: &GithubAppConfig, jar: &CookieJar) -> Option<i64> {
    let cookie = jar.get(SESSION_COOKIE)?;
    verify_session(&cfg.session_secret, cookie.value())
}

// Session cookie format:
//   "<user_id>.<expiry_unix>.<hmac_sha256_hex>"
// HMAC is computed over "<user_id>.<expiry_unix>" with `session_secret`.

fn sign_session(secret: &[u8], user_id: i64, expires_at: DateTime<Utc>) -> String {
    let payload = format!("{user_id}.{}", expires_at.timestamp());
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    let sig = mac.finalize().into_bytes();
    format!("{payload}.{}", hex::encode(sig))
}

fn verify_session(secret: &[u8], value: &str) -> Option<i64> {
    let mut parts = value.splitn(3, '.');
    let user_id_str = parts.next()?;
    let exp_str = parts.next()?;
    let sig_hex = parts.next()?;
    let user_id: i64 = user_id_str.parse().ok()?;
    let exp: i64 = exp_str.parse().ok()?;
    let sig = hex::decode(sig_hex).ok()?;
    let payload = format!("{user_id}.{exp}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    // verify_slice does constant-time comparison.
    if mac.verify_slice(&sig).is_err() {
        return None;
    }
    if Utc::now().timestamp() > exp {
        return None;
    }
    Some(user_id)
}

fn build_cookie<'a>(name: &'static str, value: String, secure: bool) -> cookie::CookieBuilder<'a> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
}

fn removal_cookie(name: &'static str, secure: bool) -> Cookie<'static> {
    build_cookie(name, String::new(), secure)
        .max_age(time::Duration::ZERO)
        .expires(time::OffsetDateTime::UNIX_EPOCH)
        .build()
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Return a GithubClient configured with the given user's access token.
/// Decrypts the stored ciphertext at use-time. Used when handling a
/// request from a logged-in user — calls debit *their* 5k/hr GitHub
/// bucket instead of the app's default token.
pub async fn github_client_for_user(
    db: &Db,
    user_id: i64,
    rate: Arc<crate::rate_limit::RateLimitTracker>,
    crypto: &Crypto,
) -> Result<Option<crate::github::GithubClient>> {
    let blob: Option<String> =
        sqlx::query_scalar("SELECT access_token FROM app_users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&db.pool)
            .await?;
    let Some(blob) = blob else { return Ok(None) };
    let token = crypto.decrypt(&blob)?;
    let client = crate::github::GithubClient::for_user_token(&token, rate)?;
    Ok(Some(client))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[test]
    fn session_signatures_reject_tampering() {
        let secret = b"a sufficiently long test secret";
        let value = sign_session(secret, 42, Utc::now() + Duration::minutes(5));
        assert_eq!(verify_session(secret, &value), Some(42));
        assert_eq!(verify_session(b"another secret", &value), None);
    }

    #[test]
    fn removal_cookie_matches_original_path() {
        let cookie = removal_cookie(SESSION_COOKIE, true);
        assert_eq!(cookie.path(), Some("/"));
        assert!(cookie.secure().unwrap_or(false));
        assert_eq!(cookie.max_age(), Some(time::Duration::ZERO));
    }

    #[tokio::test]
    async fn upstream_errors_do_not_leak_details() {
        let response = AuthError::TokenExchange("client_secret=do-not-leak".into()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"GitHub authentication failed");
    }
}
