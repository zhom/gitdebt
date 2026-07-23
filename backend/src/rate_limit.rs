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

/// Re-read the persisted `api_quota` row when the local view is older than
/// this. The api and worker processes (and any extra replicas) share tokens;
/// without the periodic re-read each process spends from its own in-memory
/// copy and only header reconciliation catches the drift.
const REFRESH_AFTER_SECS: i64 = 15;

#[derive(Debug, Clone)]
struct State {
    remaining: i64,
    limit_total: i64,
    reset_at: i64,
    /// `reset_at` came directly from GitHub's own response headers, so it
    /// is exact for this token. Windows adopted from the persisted row (or
    /// fabricated locally) are estimates only: the next real GitHub
    /// response must be allowed to re-anchor both `reset_at` and
    /// `remaining`, otherwise one replica's guess outlives the truth.
    authoritative_reset: bool,
    /// The current window is backed by real evidence — GitHub headers or
    /// the shared `api_quota` row — rather than fabricated locally (fresh
    /// seed, local window rollover). Fabricated state must never be
    /// persisted: a blind write would clobber the real shared budget for
    /// every replica using the same token.
    window_known: bool,
    /// Last time this state was synchronized against authoritative data
    /// (the persisted row, or GitHub's own response headers).
    refreshed_at: i64,
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
        let now = epoch_seconds();
        for row in rows {
            let source: String = row.try_get("source")?;
            let state = State {
                remaining: row.try_get("remaining")?,
                limit_total: row.try_get("limit_total")?,
                reset_at: row.try_get("reset_at")?,
                // The row is real shared evidence, but its reset may itself
                // descend from another replica's estimate; the next GitHub
                // response re-anchors it.
                authoritative_reset: false,
                window_known: true,
                refreshed_at: now,
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
        let now = epoch_seconds();
        // In-memory fallback only. A fabricated full budget must never be
        // written over the shared `api_quota` row: another replica may
        // already have spent most of the real window, and clobbering the
        // row would hand every replica a fake 5000.
        let seed = State {
            remaining: 5000,
            limit_total: 5000,
            reset_at: now + 3600,
            authoritative_reset: false,
            window_known: false,
            // If the seed-row read below fails, force a persisted-row
            // consultation on first acquire instead of trusting the guess.
            refreshed_at: now - REFRESH_AFTER_SECS,
        };
        let state = match seed_row_if_absent(&self.db, source, &seed).await {
            Ok(Some(persisted)) => State {
                remaining: persisted.remaining.max(0),
                limit_total: persisted.limit_total,
                reset_at: persisted.reset_at,
                // Shared evidence, not header truth: let the next real
                // GitHub response re-anchor reset and remaining.
                authoritative_reset: false,
                window_known: true,
                refreshed_at: now,
            },
            Ok(None) => seed,
            Err(e) => {
                tracing::warn!(error = %e, source, "rate-limit seed load failed");
                seed
            }
        };
        let arc = Arc::new(Mutex::new(state));
        write.insert(source.to_string(), arc.clone());
        arc
    }

    /// Reserve one request, waiting for the current window to reset when
    /// only the protected headroom remains.
    ///
    /// Before deciding to spend (or sleep), a stale local view is refreshed
    /// from the persisted `api_quota` row: replicas sharing a token each
    /// persist their own spend, so the MIN of local and persisted remaining
    /// for the same reset window is the safe cross-replica estimate.
    pub async fn acquire(&self, source: &str) {
        let state = self.get_or_load(source).await;
        loop {
            let now = epoch_seconds();
            let mut s = state.lock().await;
            if now.saturating_sub(s.refreshed_at) >= REFRESH_AFTER_SECS {
                match load_persisted(&self.db, source).await {
                    Ok(Some(persisted)) => merge_persisted(&mut s, &persisted),
                    Ok(None) => {}
                    Err(e) => {
                        // A refresh failure only skips the cross-replica
                        // reconciliation; the local budget still applies.
                        tracing::warn!(error = %e, source, "rate-limit budget refresh failed");
                    }
                }
                s.refreshed_at = now;
            }
            match reserve_request(&mut s, now) {
                Ok(()) => {
                    // A reservation against a fabricated window (fresh seed
                    // that never reached the row, or a local rollover) stays
                    // local: persistence is deferred until real GitHub
                    // headers or the shared row anchor the window.
                    if s.window_known
                        && let Err(e) = persist(&self.db, source, &s).await
                    {
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
            let now = epoch_seconds();
            reconcile_headers(&mut s, h, now);
            // GitHub's own headers are authoritative for the whole token
            // (they already account for every replica's spend), so this
            // counts as a fresh cross-replica synchronization.
            s.refreshed_at = now;
        }
        if s.window_known
            && let Err(e) = persist(&self.db, source, &s).await
        {
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
                s.window_known = true;
                tracing::warn!(
                    source,
                    retry_after_secs = retry_after,
                    "secondary rate limit hit (Retry-After); pausing"
                );
            } else if let Some(reset_at) = h.get("x-ratelimit-reset").and_then(parse_i64) {
                s.reset_at = reset_at;
                s.authoritative_reset = true;
                s.window_known = true;
            }
        }
        // Without a real reset header a fabricated window stays local; the
        // exhaustion still pauses this replica, but its guessed reset must
        // not overwrite the shared row.
        if s.window_known
            && let Err(e) = persist(&self.db, source, &s).await
        {
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
        // Local rollover: GitHub's true window boundary is unknown, so this
        // reset is fabricated. Mark the window as such — callers must not
        // persist it, and the next real response re-anchors it.
        state.remaining = state.limit_total;
        state.reset_at = now + 3600;
        state.authoritative_reset = false;
        state.window_known = false;
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
        if let Some(reset_at) = reported_reset
            && reset_at >= state.reset_at
        {
            state.reset_at = reset_at;
            state.window_known = true;
        }
        return;
    }

    match reported_reset {
        Some(reset_at) if !state.authoritative_reset || reset_at > state.reset_at => {
            state.reset_at = reset_at;
            state.authoritative_reset = true;
            state.window_known = true;
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

/// The persisted view of a source's budget (another replica may have
/// written it more recently than this process).
#[derive(Debug, Clone, Copy)]
struct PersistedQuota {
    remaining: i64,
    limit_total: i64,
    reset_at: i64,
}

async fn load_persisted(db: &Db, source: &str) -> Result<Option<PersistedQuota>> {
    let row =
        sqlx::query("SELECT remaining, limit_total, reset_at FROM api_quota WHERE source = $1")
            .bind(source)
            .fetch_optional(&db.pool)
            .await?;
    Ok(match row {
        Some(row) => Some(PersistedQuota {
            remaining: row.try_get("remaining")?,
            limit_total: row.try_get("limit_total")?,
            reset_at: row.try_get("reset_at")?,
        }),
        None => None,
    })
}

/// Fold the persisted row into the local state:
/// - same reset window → take the MIN remaining (each replica's counter
///   only ever under-reports its OWN spend, so the smaller number is the
///   safer shared estimate);
/// - persisted window is newer → another replica moved on to a later
///   window; adopt it as the working estimate, but NOT as authoritative:
///   the row's reset may itself be a guess, and only GitHub's own headers
///   are allowed to pin a window. The next real response re-anchors both
///   `reset_at` and `remaining`;
/// - persisted window is older → stale last-writer row; ignore.
fn merge_persisted(state: &mut State, persisted: &PersistedQuota) {
    if persisted.reset_at == state.reset_at {
        state.remaining = state.remaining.min(persisted.remaining.max(0));
        state.window_known = true;
    } else if persisted.reset_at > state.reset_at {
        state.reset_at = persisted.reset_at;
        state.limit_total = persisted.limit_total;
        state.remaining = persisted.remaining.max(0);
        state.authoritative_reset = false;
        state.window_known = true;
    }
}

/// Seed the shared row for a source only when none exists yet, then read
/// back whichever row actually won. A fresh replica must adopt the shared
/// budget another process may already have spent from — never clobber it
/// with a fabricated full window.
async fn seed_row_if_absent(db: &Db, source: &str, seed: &State) -> Result<Option<PersistedQuota>> {
    sqlx::query(
        "INSERT INTO api_quota (source, remaining, limit_total, reset_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT(source) DO NOTHING",
    )
    .bind(source)
    .bind(seed.remaining)
    .bind(seed.limit_total)
    .bind(seed.reset_at)
    .bind(Utc::now())
    .execute(&db.pool)
    .await?;
    load_persisted(db, source).await
}

/// Blind last-writer-wins upsert. Callers must only persist states whose
/// window is real shared evidence (`window_known`), never a local guess.
async fn persist(db: &Db, source: &str, state: &State) -> Result<()> {
    debug_assert!(
        state.window_known,
        "fabricated rate-limit state must never be persisted"
    );
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
            window_known: true,
            refreshed_at: 0,
        }
    }

    /// Connect to the test database when configured; DB-backed tests no-op
    /// otherwise so `cargo test` stays green without Postgres.
    async fn test_db() -> Option<Db> {
        crate::test_db::shared().await
    }

    async fn clear_source(db: &Db, source: &str) {
        sqlx::query("DELETE FROM api_quota WHERE source = $1")
            .bind(source)
            .execute(&db.pool)
            .await
            .expect("clear api_quota test row");
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
        // The rolled-over reset is fabricated; callers must not persist it.
        assert!(!value.window_known);
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

    /// Cross-replica refresh: for the SAME reset window the persisted row
    /// can only lower the local estimate (another replica spent from the
    /// shared token), never raise it.
    #[test]
    fn refresh_takes_min_of_local_and_persisted_for_same_window() {
        let mut value = state(100, 1_000);
        merge_persisted(
            &mut value,
            &PersistedQuota {
                remaining: 40,
                limit_total: 5_000,
                reset_at: 1_000,
            },
        );
        assert_eq!(value.remaining, 40);

        // A HIGHER persisted number (this replica is the bigger spender,
        // or the row lags) must not inflate the local budget.
        merge_persisted(
            &mut value,
            &PersistedQuota {
                remaining: 4_000,
                limit_total: 5_000,
                reset_at: 1_000,
            },
        );
        assert_eq!(value.remaining, 40);

        // Defensive: a negative persisted remaining clamps to zero.
        merge_persisted(
            &mut value,
            &PersistedQuota {
                remaining: -5,
                limit_total: 5_000,
                reset_at: 1_000,
            },
        );
        assert_eq!(value.remaining, 0);
    }

    #[test]
    fn refresh_adopts_a_newer_persisted_window() {
        let mut value = state(3, 1_000);
        merge_persisted(
            &mut value,
            &PersistedQuota {
                remaining: 4_999,
                limit_total: 5_000,
                reset_at: 2_000,
            },
        );
        assert_eq!(value.remaining, 4_999);
        assert_eq!(value.reset_at, 2_000);
        // Adopted from the row, not from GitHub headers: usable and
        // persistable, but NOT authoritative — the row's reset may itself
        // be another replica's estimate.
        assert!(!value.authoritative_reset);
        assert!(value.window_known);
    }

    /// Windows adopted from the shared row are estimates: the next real
    /// GitHub response must re-anchor both reset and remaining, even when
    /// the true reset is EARLIER than the adopted one. (Regression: an
    /// adopted-later window became authoritative, which turned every
    /// subsequent real header into a no-op and froze fabricated state
    /// across all replicas.)
    #[test]
    fn adopted_newer_window_is_re_anchored_by_the_next_header() {
        let mut value = state(3, 1_000);
        merge_persisted(
            &mut value,
            &PersistedQuota {
                remaining: 4_999,
                limit_total: 5_000,
                reset_at: 2_000,
            },
        );
        assert_eq!(value.reset_at, 2_000);
        assert!(!value.authoritative_reset);

        reconcile_headers(&mut value, &headers(120, 1_500), 100);
        assert_eq!(value.reset_at, 1_500);
        assert_eq!(value.remaining, 120);
        assert!(value.authoritative_reset);
        assert!(value.window_known);
    }

    #[test]
    fn refresh_ignores_a_stale_persisted_window() {
        let mut value = state(100, 2_000);
        merge_persisted(
            &mut value,
            &PersistedQuota {
                remaining: 1,
                limit_total: 5_000,
                reset_at: 1_000,
            },
        );
        assert_eq!(value.remaining, 100);
        assert_eq!(value.reset_at, 2_000);
    }

    /// A brand-new replica discovering a source must adopt the shared row,
    /// never clobber it with a fabricated full budget. (Regression: the
    /// seed's blind upsert overwrote the real budget with 5000/now+3600
    /// before the forced refresh could read the row back.)
    #[tokio::test]
    async fn fresh_replica_seed_does_not_clobber_a_populated_row() {
        let Some(db) = test_db().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let source = format!("github:test:seed-{}", std::process::id());
        clear_source(&db, &source).await;
        let reset_at = epoch_seconds() + 1_800;
        sqlx::query(
            "INSERT INTO api_quota (source, remaining, limit_total, reset_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&source)
        .bind(123i64)
        .bind(5_000i64)
        .bind(reset_at)
        .bind(Utc::now())
        .execute(&db.pool)
        .await
        .expect("insert populated row");

        let tracker = RateLimitTracker {
            db: db.clone(),
            sources: Arc::new(RwLock::new(HashMap::new())),
        };
        {
            let state = tracker.get_or_load(&source).await;
            let s = state.lock().await;
            assert_eq!(s.remaining, 123, "seed must adopt the shared budget");
            assert_eq!(s.reset_at, reset_at);
            assert!(s.window_known);
            // Adopted from the row, not from headers: the next real GitHub
            // response must be able to re-anchor it.
            assert!(!s.authoritative_reset);
        }
        let row = load_persisted(&db, &source)
            .await
            .expect("read row")
            .expect("row still exists");
        assert_eq!(row.remaining, 123, "seed must not clobber the shared row");
        assert_eq!(row.reset_at, reset_at);
        clear_source(&db, &source).await;
    }

    /// An expired window rolled over locally is only a guess about GitHub's
    /// next window: `acquire` must reserve against it in memory without
    /// persisting the fabricated reset. Only a real GitHub response anchors
    /// the window and makes the state persistable again.
    #[tokio::test]
    async fn fabricated_rollover_is_never_persisted() {
        let Some(db) = test_db().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let source = format!("github:test:rollover-{}", std::process::id());
        clear_source(&db, &source).await;
        let now = epoch_seconds();
        let expired = State {
            remaining: 60,
            limit_total: 5_000,
            reset_at: now - 10,
            authoritative_reset: true,
            window_known: true,
            // Fresh enough that acquire() skips the cross-replica refresh
            // and exercises the local rollover path directly.
            refreshed_at: now,
        };
        let mut map = HashMap::new();
        map.insert(source.clone(), Arc::new(Mutex::new(expired)));
        let tracker = RateLimitTracker {
            db: db.clone(),
            sources: Arc::new(RwLock::new(map)),
        };

        tracker.acquire(&source).await;
        assert!(
            load_persisted(&db, &source)
                .await
                .expect("read row")
                .is_none(),
            "fabricated rollover state must not reach the shared row"
        );
        {
            let state = tracker.get_or_load(&source).await;
            let s = state.lock().await;
            assert_eq!(s.remaining, 4_999);
            assert!(!s.window_known);
            assert!(!s.authoritative_reset);
        }

        // The next real GitHub response re-anchors the window (even to an
        // earlier true reset) and only then is the state persisted.
        let true_reset = now + 1_200;
        tracker
            .record_response(&source, Some(&headers(4_998, true_reset)))
            .await;
        let row = load_persisted(&db, &source)
            .await
            .expect("read row")
            .expect("row persisted after real headers");
        assert_eq!(row.remaining, 4_998);
        assert_eq!(row.reset_at, true_reset);
        clear_source(&db, &source).await;
    }
}
