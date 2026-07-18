//! GitHub webhook receiver. Required by GitHub when registering an App
//! (the App config form refuses to save without a webhook URL), and the
//! receiver verifies inbound payloads against `GITHUB_WEBHOOK_SECRET`
//! using HMAC-SHA256 (`X-Hub-Signature-256`).
//!
//! v0 only handles `installation` events (created / deleted / suspended)
//! to maintain the `installations` table. Other events are accepted with
//! 200 so GitHub doesn't retry them, but otherwise ignored.

use anyhow::Result;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::Sha256;

use crate::api::ApiState;
use crate::db::Db;

pub fn router() -> Router<ApiState> {
    Router::new().route("/webhooks/github", post(receive))
}

async fn receive(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(cfg) = state.gh_app.as_ref() else {
        // Webhook endpoint exists but no secret configured — reject so
        // GitHub surfaces it during the App's "send test delivery" flow
        // rather than silently swallowing.
        return (StatusCode::SERVICE_UNAVAILABLE, "webhook not configured").into_response();
    };
    let Some(secret) = cfg.webhook_secret.as_deref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "GITHUB_WEBHOOK_SECRET unset",
        )
            .into_response();
    };

    if !verify_signature(secret, &body, headers.get("x-hub-signature-256")) {
        tracing::warn!("webhook signature verification failed");
        return (StatusCode::UNAUTHORIZED, "bad signature").into_response();
    }

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let result = match event {
        "ping" => Ok(()),
        "installation" | "installation_repositories" => {
            handle_installation_event(state.analyzer.cache.db(), &body).await
        }
        // Other events get 200 — we don't process them yet, but returning
        // a non-2xx makes GitHub queue retries we don't want.
        _ => Ok(()),
    };

    match result {
        Ok(()) => (StatusCode::OK, "ok").into_response(),
        Err(e) => {
            tracing::error!(error = %e, event, "webhook handler failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "handler error").into_response()
        }
    }
}

#[derive(Deserialize)]
struct InstallationEvent {
    action: String,
    installation: InstallationPayload,
}

#[derive(Deserialize)]
#[allow(dead_code)] // suspended_at kept for parity with GitHub's payload shape
struct InstallationPayload {
    id: i64,
    account: AccountPayload,
    #[serde(default)]
    repository_selection: Option<String>,
    #[serde(default)]
    suspended_at: Option<String>,
}

#[derive(Deserialize)]
struct AccountPayload {
    login: String,
    id: i64,
    #[serde(rename = "type", default)]
    account_type: Option<String>,
}

async fn handle_installation_event(db: &Db, body: &[u8]) -> Result<()> {
    let event: InstallationEvent = serde_json::from_slice(body)?;
    let now = Utc::now();
    match event.action.as_str() {
        "created" | "new_permissions_accepted" | "added" => {
            sqlx::query(
                "INSERT INTO installations (id, account_login, account_id, account_type, \
                                            repository_selection, suspended, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, FALSE, $6, $6) \
                 ON CONFLICT (id) DO UPDATE SET \
                    account_login = EXCLUDED.account_login, \
                    account_id = EXCLUDED.account_id, \
                    account_type = EXCLUDED.account_type, \
                    repository_selection = EXCLUDED.repository_selection, \
                    suspended = FALSE, \
                    updated_at = EXCLUDED.updated_at",
            )
            .bind(event.installation.id)
            .bind(&event.installation.account.login)
            .bind(event.installation.account.id)
            .bind(&event.installation.account.account_type)
            .bind(&event.installation.repository_selection)
            .bind(now)
            .execute(&db.pool)
            .await?;
            tracing::info!(
                installation_id = event.installation.id,
                account = %event.installation.account.login,
                "installation created"
            );
        }
        "deleted" | "removed" => {
            sqlx::query("DELETE FROM installations WHERE id = $1")
                .bind(event.installation.id)
                .execute(&db.pool)
                .await?;
            tracing::info!(
                installation_id = event.installation.id,
                "installation deleted"
            );
        }
        "suspend" => {
            sqlx::query("UPDATE installations SET suspended = TRUE, updated_at = $2 WHERE id = $1")
                .bind(event.installation.id)
                .bind(now)
                .execute(&db.pool)
                .await?;
        }
        "unsuspend" => {
            sqlx::query(
                "UPDATE installations SET suspended = FALSE, updated_at = $2 WHERE id = $1",
            )
            .bind(event.installation.id)
            .bind(now)
            .execute(&db.pool)
            .await?;
        }
        other => {
            tracing::debug!(action = other, "installation event ignored");
        }
    }
    Ok(())
}

/// Verify GitHub's `X-Hub-Signature-256` header against the raw body.
/// Format: `sha256=<hex_digest>`. Constant-time compare via `verify_slice`.
fn verify_signature(secret: &[u8], body: &[u8], header: Option<&axum::http::HeaderValue>) -> bool {
    let Some(header) = header else {
        return false;
    };
    let Ok(header_str) = header.to_str() else {
        return false;
    };
    let Some(hex_part) = header_str.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(provided) = hex::decode(hex_part) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(body);
    mac.verify_slice(&provided).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn rejects_missing_header() {
        assert!(!verify_signature(b"secret", b"body", None));
    }

    #[test]
    fn rejects_wrong_prefix() {
        let h = HeaderValue::from_static("sha1=abc");
        assert!(!verify_signature(b"secret", b"body", Some(&h)));
    }

    #[test]
    fn rejects_tampered_body() {
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(b"original");
        let sig = hex::encode(mac.finalize().into_bytes());
        let header = HeaderValue::from_str(&format!("sha256={sig}")).unwrap();
        assert!(!verify_signature(b"secret", b"tampered", Some(&header)));
    }

    #[test]
    fn accepts_correct_signature() {
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(b"original");
        let sig = hex::encode(mac.finalize().into_bytes());
        let header = HeaderValue::from_str(&format!("sha256={sig}")).unwrap();
        assert!(verify_signature(b"secret", b"original", Some(&header)));
    }
}
