//! Persistent, per-source GitHub rate-limit tracker.
//!
//! Each token (env PAT, GitHub App installation token, individual user
//! OAuth token) has its own 5k–15k/hr budget on GitHub's side. We mirror
//! that per-source: a `source` string (token-hash with a kind prefix like
//! `github:user:abc123…`) keys an in-memory `State`, persisted to the
//! `api_quota` table. App restarts re-load every known source's state
//! instead of re-discovering it via failed calls.
//!
//! Sources we use today:
//!   - `github:default:<hash>` — the env PAT used by background workers
//!   - `github:user:<hash>` — a logged-in user's OAuth access token
//!   - `github:anonymous`    — unauthenticated requests (60/hr per IP)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use chrono::Utc;
use reqwest::header::HeaderMap;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio::sync::{Mutex, RwLock};
use tokio::time::sleep;

use crate::db::Db;

/// Always keep this much headroom before refusing to make a request. The
/// app should never burn the last 50 calls per source — we want some
/// budget left for background freshness checks and ad-hoc curl debugging.
const RESERVE: i64 = 50;

#[derive(Debug, Clone)]
struct State {
    remaining: i64,
    limit_total: i64,
    reset_at: i64,
    authoritative_reset: bool,
}

#[derive(Clone)]
pub struct RateLimitTracker {
    db: Db,
    /// One mutex per source so high-volume sources don't serialize
    /// through a global lock. The outer RwLock guards the map itself.
    sources: Arc<RwLock<HashMap<String, Arc<Mutex<State>>>>>,
}

impl RateLimitTracker {
    /// Eagerly load every known source from `api_quota`. Sources discovered
    /// at runtime (e.g., a fresh user OAuth token) are added lazily on first
    /// use via `get_or_load`.
    pub async fn load(db: Db) -> Result<Self> {
        let rows = sqlx::query("SELECT source, remaining, limit_total, reset_at FROM api_quota")
            .fetch_all(&db.pool)
            .await?;
        let mut map: HashMap<String, Arc<Mutex<State>>> = HashMap::with_capacity(rows.len());
        for row in rows {
            let source: String = row.try_get("source")?;
            let state = State {
                remaining: row.try_get("remaining")?,
                limit_total: row.try_get("limit_total")?,
                reset_at: row.try_get("reset_at")?,
                authoritative_reset: true,
            };
            map.insert(source, Arc::new(Mutex::new(state)));
        }
        tracing::info!(sources_loaded = map.len(), "rate-limit tracker loaded");
        Ok(Self {
            db,
            sources: Arc::new(RwLock::new(map)),
        })
    }

    async fn get_or_load(&self, source: &str) -> Arc<Mutex<State>> {
        // Fast path: already in the map.
        {
            let read = self.sources.read().await;
            if let Some(s) = read.get(source) {
                return s.clone();
            }
        }
        // Slow path: take the write lock and double-check (another caller
        // may have inserted concurrently while we were waiting).
        let mut write = self.sources.write().await;
        if let Some(s) = write.get(source) {
            return s.clone();
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let state = State {
            remaining: 5000,
            limit_total: 5000,
            reset_at: now + 3600,
            authoritative_reset: false,
        };
        let arc = Arc::new(Mutex::new(state.clone()));
        write.insert(source.to_string(), arc.clone());
        if let Err(e) = persist(&self.db, source, &state).await {
            tracing::warn!(error = %e, source, "rate-limit seed persist failed");
        }
        arc
    }

    /// Reserve one request, waiting for the current window to reset when
    /// only the protected headroom remains.
    pub async fn acquire(&self, source: &str) {
        let state = self.get_or_load(source).await;
        loop {
            let now = epoch_seconds();
            let mut s = state.lock().await;
            match reserve_request(&mut s, now) {
                Ok(()) => {
                    if let Err(e) = persist(&self.db, source, &s).await {
                        tracing::warn!(error = %e, source, "rate-limit reservation persist failed");
                    }
                    return;
                }
                Err(reset_at) => {
                    let remaining = s.remaining;
                    drop(s);
                    let wait_secs = (reset_at - now + 1).max(1) as u64;
                    tracing::warn!(
                        source,
                        remaining,
                        wait_secs,
                        "rate budget under reserve; waiting for reset"
                    );
                    sleep(Duration::from_secs(wait_secs.min(60))).await;
                }
            }
        }
    }

    /// Reconcile a reservation with GitHub's response headers. Responses
    /// from the same window may arrive out of order, so they can only lower
    /// the local remaining count.
    pub async fn record_response(&self, source: &str, headers: Option<&HeaderMap>) {
        let state = self.get_or_load(source).await;
        let mut s = state.lock().await;
        if let Some(h) = headers {
            reconcile_headers(&mut s, h, epoch_seconds());
        }
        if let Err(e) = persist(&self.db, source, &s).await {
            tracing::warn!(error = %e, source, "rate-limit persist failed");
        }
    }

    /// Force a wait period for `source`. Honors:
    /// - `Retry-After` (seconds) — secondary/abuse rate limit; takes precedence
    /// - `x-ratelimit-reset` (epoch) — primary rate limit
    pub async fn mark_exhausted(&self, source: &str, headers: Option<&HeaderMap>) {
        let state = self.get_or_load(source).await;
        let mut s = state.lock().await;
        s.remaining = 0;
        if let Some(h) = headers {
            if let Some(retry_after) = h.get("retry-after").and_then(parse_i64) {
                let now = epoch_seconds();
                s.reset_at = now + retry_after.max(1);
                s.authoritative_reset = true;
                tracing::warn!(
                    source,
                    retry_after_secs = retry_after,
                    "secondary rate limit hit (Retry-After); pausing"
                );
            } else if let Some(reset_at) = h.get("x-ratelimit-reset").and_then(parse_i64) {
                s.reset_at = reset_at;
                s.authoritative_reset = true;
            }
        }
        if let Err(e) = persist(&self.db, source, &s).await {
            tracing::warn!(error = %e, source, "rate-limit persist (exhausted) failed");
        }
    }

    pub async fn snapshot(&self, source: &str) -> (i64, i64, i64) {
        let state = self.get_or_load(source).await;
        let s = state.lock().await;
        (s.remaining, s.limit_total, s.reset_at)
    }
}

fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn reserve_request(state: &mut State, now: i64) -> Result<(), i64> {
    if now >= state.reset_at {
        state.remaining = state.limit_total;
        state.reset_at = now + 3600;
        state.authoritative_reset = false;
    }
    if state.remaining <= RESERVE {
        return Err(state.reset_at);
    }
    state.remaining -= 1;
    Ok(())
}

fn reconcile_headers(state: &mut State, headers: &HeaderMap, now: i64) {
    if let Some(limit) = headers.get("x-ratelimit-limit").and_then(parse_i64) {
        state.limit_total = limit;
    }
    let reported_remaining = headers.get("x-ratelimit-remaining").and_then(parse_i64);
    let reported_reset = headers.get("x-ratelimit-reset").and_then(parse_i64);

    if state.remaining == 0 && now < state.reset_at {
        if let Some(reset_at) = reported_reset {
            state.reset_at = state.reset_at.max(reset_at);
        }
        return;
    }

    match reported_reset {
        Some(reset_at) if !state.authoritative_reset || reset_at > state.reset_at => {
            state.reset_at = reset_at;
            state.authoritative_reset = true;
            if let Some(remaining) = reported_remaining {
                state.remaining = remaining.max(0);
            }
        }
        Some(reset_at) if reset_at == state.reset_at => {
            if let Some(remaining) = reported_remaining {
                state.remaining = state.remaining.min(remaining.max(0));
            }
        }
        Some(_) => {}
        None => {
            if let Some(remaining) = reported_remaining {
                state.remaining = state.remaining.min(remaining.max(0));
            }
        }
    }
}

fn parse_i64(v: &reqwest::header::HeaderValue) -> Option<i64> {
    v.to_str().ok()?.parse().ok()
}

async fn persist(db: &Db, source: &str, state: &State) -> Result<()> {
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO api_quota (source, remaining, limit_total, reset_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT(source) DO UPDATE SET \
            remaining = EXCLUDED.remaining, \
            limit_total = EXCLUDED.limit_total, \
            reset_at = EXCLUDED.reset_at, \
            updated_at = EXCLUDED.updated_at",
    )
    .bind(source)
    .bind(state.remaining)
    .bind(state.limit_total)
    .bind(state.reset_at)
    .bind(now)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Stable source key for a token. Hashes the token so we never log it
/// or persist it directly. The `kind` prefix lets us distinguish PAT,
/// installation, and per-user buckets at a glance in the `api_quota` table.
pub fn source_for_token(kind: &str, token: Option<&str>) -> String {
    match token {
        Some(t) => {
            let mut h = Sha256::new();
            h.update(t.as_bytes());
            let hex = hex::encode(h.finalize());
            format!("github:{kind}:{}", &hex[..16])
        }
        None => "github:anonymous".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderValue};

    use super::*;

    fn state(remaining: i64, reset_at: i64) -> State {
        State {
            remaining,
            limit_total: 5_000,
            reset_at,
            authoritative_reset: true,
        }
    }

    fn headers(remaining: i64, reset_at: i64) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-ratelimit-remaining",
            HeaderValue::from_str(&remaining.to_string()).unwrap(),
        );
        headers.insert(
            "x-ratelimit-reset",
            HeaderValue::from_str(&reset_at.to_string()).unwrap(),
        );
        headers
    }

    #[test]
    fn reservations_are_atomic_against_the_headroom() {
        let mut value = state(52, 1_000);
        assert_eq!(reserve_request(&mut value, 100), Ok(()));
        assert_eq!(value.remaining, 51);
        assert_eq!(reserve_request(&mut value, 100), Ok(()));
        assert_eq!(value.remaining, RESERVE);
        assert_eq!(reserve_request(&mut value, 100), Err(1_000));
    }

    #[test]
    fn expired_window_resets_then_reserves() {
        let mut value = state(0, 100);
        assert_eq!(reserve_request(&mut value, 100), Ok(()));
        assert_eq!(value.remaining, 4_999);
        assert!(!value.authoritative_reset);
    }

    #[test]
    fn out_of_order_headers_cannot_raise_same_window_budget() {
        let mut value = state(52, 1_000);
        reconcile_headers(&mut value, &headers(59, 1_000), 100);
        assert_eq!(value.remaining, 52);
        reconcile_headers(&mut value, &headers(50, 1_000), 100);
        assert_eq!(value.remaining, 50);
        reconcile_headers(&mut value, &headers(55, 1_000), 100);
        assert_eq!(value.remaining, 50);
    }

    #[test]
    fn newer_window_replaces_the_old_budget() {
        let mut value = state(52, 1_000);
        reconcile_headers(&mut value, &headers(4_999, 2_000), 100);
        assert_eq!(value.remaining, 4_999);
        assert_eq!(value.reset_at, 2_000);
    }

    #[test]
    fn in_flight_response_cannot_clear_exhaustion() {
        let mut value = state(0, 1_000);
        reconcile_headers(&mut value, &headers(4_999, 2_000), 100);
        assert_eq!(value.remaining, 0);
        assert_eq!(value.reset_at, 2_000);
    }
}
