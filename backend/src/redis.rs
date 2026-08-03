//! Redis-backed cross-replica coordination for the API tier.
//!
//! Two concerns live here:
//!   1. A distributed fixed-window HTTP admission limiter shared by every
//!      `gitdebt-api` replica. Redis keeps one counter per client key per
//!      window, so N replicas enforce ONE ceiling instead of N.
//!   2. A cache-invalidation pub/sub bus: a replica that rebuilds a user
//!      aggregate publishes the evicted keys so every other replica drops
//!      the same entries from its local moka caches.
//!
//! Redis is an availability dependency, never a correctness one: every
//! runtime failure FAILS OPEN (the request is admitted, the eviction is
//! skipped) with a throttled warning and a counter surfaced on `/metrics`.
//! "Failure" includes a stalled server: every command carries a client-side
//! response timeout (at the connection-manager level and again as an outer
//! `tokio::time::timeout`), so a TCP-connected-but-unresponsive Redis
//! degrades to bounded fail-open instead of hanging admitted requests.
//! Debug builds run without Redis at all via the in-process fallback
//! backend, which applies the same window math per replica.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use ::redis::aio::{ConnectionManager, ConnectionManagerConfig};

/// Pub/sub channel for cross-replica local-cache eviction.
pub const INVALIDATION_CHANNEL: &str = "gitdebt:invalidate";

/// Reconnect cadence while the initial Redis connection is unavailable.
const ESTABLISH_RETRY: Duration = Duration::from_secs(5);
/// Connection-manager-level ceiling on one command round-trip. Without it
/// (redis defaults to no response timeout) a TCP-connected-but-stalled Redis
/// blocks every admitted request instead of failing open.
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(500);
/// Ceiling on each (re)connection attempt inside the manager.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
/// Outer `tokio::time::timeout` bound on limiter checks and publishes:
/// belt over the manager-level response timeout so no request path can
/// ever await Redis past this bound. Elapsed == fail open.
const COMMAND_TIMEOUT: Duration = Duration::from_millis(750);
/// Throttle for fail-open warnings so an outage logs once a minute, not
/// once per request.
const FAIL_OPEN_WARN_INTERVAL_SECS: u64 = 60;

static FAIL_OPEN_TOTAL: AtomicU64 = AtomicU64::new(0);
static LAST_FAIL_OPEN_WARN: AtomicU64 = AtomicU64::new(0);

/// Total admissions granted because Redis was unavailable or errored.
/// Surfaced on `/metrics` as the outage signal for the shared limiter.
pub fn limiter_fail_open_total() -> u64 {
    FAIL_OPEN_TOTAL.load(Ordering::Relaxed)
}

fn note_fail_open(context: &'static str, detail: &str) {
    FAIL_OPEN_TOTAL.fetch_add(1, Ordering::Relaxed);
    let now = epoch_seconds();
    let last = LAST_FAIL_OPEN_WARN.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= FAIL_OPEN_WARN_INTERVAL_SECS
        && LAST_FAIL_OPEN_WARN
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        tracing::warn!(context, detail, "redis unavailable; failing open");
    }
}

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Connection-manager wrapper with reconnect. `connect` validates the URL
/// eagerly (a typo is a config error) but treats an unreachable server as a
/// runtime condition: commands fail open while a background task keeps
/// retrying the initial connection. Once established, the manager reconnects
/// internally after any drop.
pub struct RedisHandle {
    client: ::redis::Client,
    manager: tokio::sync::RwLock<Option<ConnectionManager>>,
}

impl RedisHandle {
    pub fn connect(url: &str) -> Result<Arc<Self>> {
        let client = ::redis::Client::open(url).context("REDIS_URL is not a valid redis:// URL")?;
        let handle = Arc::new(Self {
            client,
            manager: tokio::sync::RwLock::new(None),
        });
        let establishing = handle.clone();
        tokio::spawn(async move {
            loop {
                let config = ConnectionManagerConfig::new()
                    .set_response_timeout(Some(RESPONSE_TIMEOUT))
                    .set_connection_timeout(Some(CONNECT_TIMEOUT));
                match establishing
                    .client
                    .get_connection_manager_with_config(config)
                    .await
                {
                    Ok(manager) => {
                        *establishing.manager.write().await = Some(manager);
                        tracing::info!("redis connected");
                        return;
                    }
                    Err(error) => {
                        note_fail_open("connect", &error.to_string());
                        tokio::time::sleep(ESTABLISH_RETRY).await;
                    }
                }
            }
        });
        Ok(handle)
    }

    /// Cheap clone of the shared multiplexed connection, `None` until the
    /// first connection has been established.
    pub(crate) async fn manager(&self) -> Option<ConnectionManager> {
        self.manager.read().await.clone()
    }

    /// A dedicated pub/sub connection (the multiplexed manager cannot
    /// subscribe). Callers own reconnect.
    pub async fn pubsub(&self) -> Result<::redis::aio::PubSub> {
        Ok(self.client.get_async_pubsub().await?)
    }
}

/// One route class's fixed-window budget. The window is sized so the windowed
/// ceiling preserves the previous token-bucket ceilings: a 10-second window
/// admits `rate*10 + burst` requests, i.e. the old sustained rate plus the
/// old burst allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowLimit {
    /// Stable name; part of the Redis key, so renaming resets counters.
    pub name: &'static str,
    pub window_secs: u64,
    pub max_per_window: u64,
}

impl WindowLimit {
    pub const fn per_second(name: &'static str, rate: u64, burst: u64) -> Self {
        Self {
            name,
            window_secs: 10,
            max_per_window: rate * 10 + burst,
        }
    }
}

/// Start of the fixed window containing `now_secs`.
fn window_start(now_secs: u64, window_secs: u64) -> u64 {
    now_secs - now_secs % window_secs
}

/// Seconds until the current window rolls over (never zero: a client told
/// to retry immediately would hammer the boundary).
fn retry_after_secs(now_secs: u64, window_secs: u64) -> u64 {
    (window_secs - now_secs % window_secs).max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny { retry_after_secs: u64 },
}

/// In-process fallback: one shared window boundary, per-key counters,
/// swept by clearing the map on every rollover.
struct MemoryWindows {
    window_start: u64,
    counts: HashMap<String, u64>,
}

fn check_memory(
    state: &mut MemoryWindows,
    key: &str,
    now_secs: u64,
    limit: &WindowLimit,
) -> Decision {
    let start = window_start(now_secs, limit.window_secs);
    if state.window_start != start {
        state.window_start = start;
        state.counts.clear();
    }
    let count = state.counts.entry(key.to_string()).or_insert(0);
    *count += 1;
    if *count > limit.max_per_window {
        Decision::Deny {
            retry_after_secs: retry_after_secs(now_secs, limit.window_secs),
        }
    } else {
        Decision::Allow
    }
}

enum LimiterBackend {
    Redis(Arc<RedisHandle>),
    Memory(std::sync::Mutex<MemoryWindows>),
}

/// Distributed fixed-window admission limiter for one route class.
pub struct HttpLimiter {
    limit: WindowLimit,
    backend: LimiterBackend,
}

impl HttpLimiter {
    pub fn shared(limit: WindowLimit, redis: Option<Arc<RedisHandle>>) -> Arc<Self> {
        let backend = match redis {
            Some(handle) => LimiterBackend::Redis(handle),
            None => LimiterBackend::Memory(std::sync::Mutex::new(MemoryWindows {
                window_start: 0,
                counts: HashMap::new(),
            })),
        };
        Arc::new(Self { limit, backend })
    }

    /// Count one request for `client_key` and decide admission. Redis
    /// errors — including a command that does not answer within
    /// [`COMMAND_TIMEOUT`] — always admit (fail open); see the module docs.
    pub async fn check(&self, client_key: &str) -> Decision {
        let now = epoch_seconds();
        match &self.backend {
            LimiterBackend::Memory(state) => {
                let mut state = state.lock().expect("limiter mutex never poisoned");
                check_memory(&mut state, client_key, now, &self.limit)
            }
            LimiterBackend::Redis(handle) => {
                let Some(mut manager) = handle.manager().await else {
                    note_fail_open(self.limit.name, "no connection");
                    return Decision::Allow;
                };
                let start = window_start(now, self.limit.window_secs);
                let key = format!("gitdebt:rl:{}:{start}:{client_key}", self.limit.name);
                let mut pipe = ::redis::pipe();
                pipe.atomic()
                    .incr(&key, 1u64)
                    .expire(&key, self.limit.window_secs as i64 + 1)
                    .ignore();
                let query = pipe.query_async::<(u64,)>(&mut manager);
                match tokio::time::timeout(COMMAND_TIMEOUT, query).await {
                    Ok(Ok((count,))) if count > self.limit.max_per_window => Decision::Deny {
                        retry_after_secs: retry_after_secs(now, self.limit.window_secs),
                    },
                    Ok(Ok(_)) => Decision::Allow,
                    Ok(Err(error)) => {
                        note_fail_open(self.limit.name, &error.to_string());
                        Decision::Allow
                    }
                    Err(_) => {
                        note_fail_open(self.limit.name, "command timed out");
                        Decision::Allow
                    }
                }
            }
        }
    }
}

/// Local-cache keys evicted by one replica that every other replica must
/// evict too. Field names are the wire format on [`INVALIDATION_CHANNEL`].
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Invalidation {
    /// Keys into `ApiState::user_agg_cache` (lowercased logins).
    #[serde(default)]
    pub user_agg: Vec<String>,
    /// Keys into `ApiState::analyze_cache` (e.g. `user:<login>`).
    #[serde(default)]
    pub analyze: Vec<String>,
}

/// Fire-and-forget publish; never blocks or fails the calling request.
/// The publishing replica already evicted locally, so a lost message only
/// leaves OTHER replicas stale until their TTL — the pre-Redis behavior.
pub fn publish_invalidation(handle: &Arc<RedisHandle>, invalidation: Invalidation) {
    let handle = handle.clone();
    tokio::spawn(async move {
        let Some(mut manager) = handle.manager().await else {
            return;
        };
        let Ok(payload) = serde_json::to_string(&invalidation) else {
            return;
        };
        let publish = async {
            ::redis::cmd("PUBLISH")
                .arg(INVALIDATION_CHANNEL)
                .arg(&payload)
                .query_async::<i64>(&mut manager)
                .await
        };
        match tokio::time::timeout(COMMAND_TIMEOUT, publish).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "cache invalidation publish failed"),
            Err(_) => tracing::warn!("cache invalidation publish timed out"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowed_limits_preserve_governor_ceilings() {
        // rate*10 + burst over a 10s window keeps the old per-second rate
        // and the old burst allowance.
        let analyze = WindowLimit::per_second("analyze", 2, 20);
        assert_eq!((analyze.window_secs, analyze.max_per_window), (10, 40));
        let images = WindowLimit::per_second("images", 10, 60);
        assert_eq!(images.max_per_window, 160);
        let ext = WindowLimit::per_second("ext", 1, 10);
        assert_eq!(ext.max_per_window, 20);
        let mutating = WindowLimit::per_second("mutating", 1, 5);
        assert_eq!(mutating.max_per_window, 15);
    }

    #[test]
    fn window_math_is_aligned_and_retry_is_never_zero() {
        assert_eq!(window_start(1_000, 10), 1_000);
        assert_eq!(window_start(1_009, 10), 1_000);
        assert_eq!(window_start(1_010, 10), 1_010);
        assert_eq!(retry_after_secs(1_003, 10), 7);
        // At an exact boundary the caller is in a fresh window; the
        // clamped floor keeps the header sane.
        assert_eq!(retry_after_secs(1_010, 10), 10);
        assert_eq!(retry_after_secs(1_009, 10), 1);
    }

    #[test]
    fn memory_backend_counts_per_key_and_denies_over_ceiling() {
        let limit = WindowLimit {
            name: "test",
            window_secs: 10,
            max_per_window: 3,
        };
        let mut state = MemoryWindows {
            window_start: 0,
            counts: HashMap::new(),
        };
        for _ in 0..3 {
            assert_eq!(check_memory(&mut state, "a", 100, &limit), Decision::Allow);
        }
        assert_eq!(
            check_memory(&mut state, "a", 103, &limit),
            Decision::Deny {
                retry_after_secs: 7
            }
        );
        // Independent keys have independent budgets.
        assert_eq!(check_memory(&mut state, "b", 103, &limit), Decision::Allow);
    }

    #[test]
    fn memory_backend_sweeps_on_window_rollover() {
        let limit = WindowLimit {
            name: "test",
            window_secs: 10,
            max_per_window: 1,
        };
        let mut state = MemoryWindows {
            window_start: 0,
            counts: HashMap::new(),
        };
        assert_eq!(check_memory(&mut state, "a", 100, &limit), Decision::Allow);
        assert!(matches!(
            check_memory(&mut state, "a", 105, &limit),
            Decision::Deny { .. }
        ));
        // Next window: counters were swept, the key is fresh again.
        assert_eq!(check_memory(&mut state, "a", 110, &limit), Decision::Allow);
        assert_eq!(state.counts.len(), 1, "rollover clears stale keys");
    }

    /// A dead Redis (valid URL, nothing listening) must admit every request
    /// and count the fail-opens — never 5xx, never block.
    #[tokio::test]
    async fn redis_backend_fails_open_on_dead_server() {
        let handle = RedisHandle::connect("redis://127.0.0.1:1").unwrap();
        let limiter = HttpLimiter::shared(
            WindowLimit {
                name: "test-dead",
                window_secs: 10,
                max_per_window: 1,
            },
            Some(handle),
        );
        let before = limiter_fail_open_total();
        for _ in 0..5 {
            assert_eq!(limiter.check("203.0.113.9").await, Decision::Allow);
        }
        assert!(
            limiter_fail_open_total() >= before + 5,
            "each admitted-on-error request increments the fail-open counter"
        );
    }

    /// A Redis that completes the connection handshake but then stalls —
    /// TCP connected, never answers another command — must fail open within
    /// the client-side bound instead of hanging every admitted request.
    /// The fake server answers the redis-rs connection-setup pipeline (two
    /// `CLIENT SETINFO` commands) so the manager establishes, then reads
    /// and discards forever without responding.
    #[tokio::test]
    async fn redis_backend_fails_open_fast_when_server_stalls() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                std::thread::spawn(move || {
                    let mut buf = [0u8; 1024];
                    // Complete the setup handshake...
                    if stream.read(&mut buf).is_ok() {
                        let _ = stream.write_all(b"+OK\r\n+OK\r\n");
                        let _ = stream.flush();
                    }
                    // ...then stall: keep the socket open, never respond.
                    while let Ok(n) = stream.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                    }
                });
            }
        });

        let handle = RedisHandle::connect(&format!("redis://127.0.0.1:{port}")).unwrap();
        let mut connected = false;
        for _ in 0..100 {
            if handle.manager().await.is_some() {
                connected = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            connected,
            "the handshake against the stalling server must establish a manager"
        );

        let limiter = HttpLimiter::shared(
            WindowLimit {
                name: "test-stall",
                window_secs: 10,
                max_per_window: 1,
            },
            Some(handle),
        );
        let before = limiter_fail_open_total();
        let start = std::time::Instant::now();
        assert_eq!(limiter.check("198.51.100.99").await, Decision::Allow);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(1_500),
            "a stalled redis must fail open within the bound, took {elapsed:?}"
        );
        assert!(
            limiter_fail_open_total() > before,
            "the stalled command counts as a fail-open"
        );
    }

    /// Real-Redis path: counters are shared per key per window and the
    /// ceiling holds. Gated on GITDEBT_TEST_REDIS_URL, mirroring the
    /// Postgres-backed test convention.
    #[tokio::test]
    async fn redis_backend_enforces_window_ceiling() {
        let Ok(url) = std::env::var("GITDEBT_TEST_REDIS_URL") else {
            eprintln!("skipping: set GITDEBT_TEST_REDIS_URL to run");
            return;
        };
        let handle = RedisHandle::connect(&url).unwrap();
        // Wait for the background connect (bounded).
        let mut connected = false;
        for _ in 0..50 {
            if handle.manager().await.is_some() {
                connected = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(connected, "test redis at {url} must be reachable");

        // Unique limiter name per run so reruns never inherit counters; a
        // wide window keeps the test inside one window.
        let name: &'static str =
            Box::leak(format!("test-{}-{}", std::process::id(), epoch_seconds()).into_boxed_str());
        let limiter = HttpLimiter::shared(
            WindowLimit {
                name,
                window_secs: 3_600,
                max_per_window: 3,
            },
            Some(handle),
        );
        for _ in 0..3 {
            assert_eq!(limiter.check("198.51.100.7").await, Decision::Allow);
        }
        assert!(matches!(
            limiter.check("198.51.100.7").await,
            Decision::Deny { .. }
        ));
        // Another key is unaffected.
        assert_eq!(limiter.check("198.51.100.8").await, Decision::Allow);
    }
}
