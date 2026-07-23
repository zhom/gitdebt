//! Shared process bootstrap for the `gitdebt-api` and `gitdebt-worker`
//! binaries: env/tracing init, the Postgres + GitHub client stack both
//! processes need, the common shutdown signal, and the session-advisory-lock
//! leader election used by singleton worker coordinators.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::postgres::PgConnectOptions;
use sqlx::{Connection, PgConnection};
use tracing_subscriber::{EnvFilter, fmt};

use crate::auth::GithubAppConfig;
use crate::cache::Cache;
use crate::db::Db;
use crate::github::GithubClient;
use crate::rate_limit::RateLimitTracker;

/// Everything both binaries need before doing anything useful.
pub struct Services {
    pub db: Db,
    pub cache: Cache,
    pub github: Arc<GithubClient>,
    pub gh_app: Option<GithubAppConfig>,
    /// Kept for consumers that need dedicated (non-pool) connections,
    /// e.g. session-level advisory-lock leader election.
    pub database_url: String,
}

/// Load `.env` and install the tracing subscriber. Call first in `main`.
pub fn init_process() {
    let _ = dotenvy::dotenv();
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
}

/// Connect the shared service stack: GITHUB_TOKEN policy check, Postgres
/// (schema applied), the persistent GitHub budget tracker, the GitHub
/// client, and the optional GitHub App config.
pub async fn connect_services() -> Result<Services> {
    let token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    if token.is_none() {
        if cfg!(debug_assertions) {
            tracing::warn!("GITHUB_TOKEN not set; unauthenticated requests are limited to 60/hour");
        } else {
            anyhow::bail!("GITHUB_TOKEN must be set in release deployments");
        }
    }

    let database_url = std::env::var("DATABASE_URL").context(
        "DATABASE_URL must be set (postgres://user:pass@host:port/db). \
                  Run `scripts/db.sh up` to start a local Postgres in Docker.",
    )?;
    let db = Db::connect(&database_url).await?;
    tracing::info!("postgres connected; schema applied");
    let cache = Cache::new(db.clone());

    let rate = Arc::new(RateLimitTracker::load(db.clone()).await?);
    let github = Arc::new(GithubClient::new(token.as_deref(), rate)?);
    let gh_app =
        GithubAppConfig::from_env().context("GitHub App config invalid; refusing to start")?;
    if gh_app.is_some() {
        tracing::info!("GitHub App OAuth configured (tokens encrypted at rest)");
    } else {
        tracing::warn!(
            "GitHub App not configured (set GITHUB_APP_CLIENT_ID, GITHUB_APP_CLIENT_SECRET, \
             GITHUB_WEBHOOK_SECRET, SESSION_SECRET, TOKEN_ENCRYPTION_KEY); \
             /auth/* and /webhooks/github will 503"
        );
    }

    Ok(Services {
        db,
        cache,
        github,
        gh_app,
        database_url,
    })
}

/// Bind address from `PORT` (falling back to `default_port`) and
/// `BIND_LOCAL`. `0.0.0.0` in container deployments; localhost-only when
/// `BIND_LOCAL=1` keeps dev safe.
pub fn bind_addr(default_port: u16) -> SocketAddr {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(default_port);
    let host: [u8; 4] = if std::env::var("BIND_LOCAL").ok().as_deref() == Some("1") {
        [127, 0, 0, 1]
    } else {
        [0, 0, 0, 0]
    };
    SocketAddr::from((host, port))
}

/// Resolve on Ctrl+C or SIGTERM (Docker stop / redeploy) so in-flight
/// requests finish instead of dropping connections.
pub async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = async {
        signal::ctrl_c().await.expect("install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => tracing::info!("ctrl+c received; shutting down"),
        _ = terminate => tracing::info!("SIGTERM received; shutting down"),
    }
}

/// How often a non-leader re-contends for the advisory lock.
const LEADER_CONTEND_INTERVAL: Duration = Duration::from_secs(60);
/// How often the leader's lock connection is pinged to detect loss.
const LEADER_WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);
/// Client-side ceiling on one watchdog ping (and on closing the lock
/// connection). Without it, a half-open TCP connection blocks the ping for
/// the kernel retransmit timeout (~15-25 min) while Postgres has already
/// released the session lock and another replica has become leader — i.e.
/// two concurrent leaders. An unanswered ping within this bound is treated
/// as lost leadership.
const LEADER_PING_TIMEOUT: Duration = Duration::from_secs(10);
/// Pause before re-contending after leadership was lost or released.
const LEADER_REJOIN_DELAY: Duration = Duration::from_secs(5);

/// Session-level advisory-lock leader election for singleton coordinators
/// (GH Archive BigQuery coordinator, hourly follower). Any number of worker
/// replicas may call this; exactly one holds the lock and runs the task
/// built by `make_task`. The lock lives on a dedicated non-pool connection:
/// dropping/closing that connection — including a Postgres restart — releases
/// it server-side, at which point the watchdog ping fails (or times out
/// client-side, see [`LEADER_PING_TIMEOUT`]), the task is aborted, and this
/// replica re-contends alongside the others.
pub fn spawn_leader<F, Fut>(database_url: String, lock_id: i64, name: &'static str, make_task: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        // Server-side guard matching LEADER_PING_TIMEOUT: an orphaned
        // watchdog query cannot linger on the session. The client-side
        // timeout in `leader_ping_ok` is the actual half-open-TCP defense.
        let options = match database_url.parse::<PgConnectOptions>() {
            Ok(options) => options.options([("statement_timeout", "10s")]),
            Err(error) => {
                tracing::error!(
                    coordinator = name, %error,
                    "leader: invalid database URL; coordinator disabled"
                );
                return;
            }
        };
        loop {
            let mut conn = match PgConnection::connect_with(&options).await {
                Ok(conn) => conn,
                Err(error) => {
                    tracing::warn!(coordinator = name, %error, "leader: connect failed");
                    tokio::time::sleep(LEADER_CONTEND_INTERVAL).await;
                    continue;
                }
            };
            let acquired = match try_advisory_lock(&mut conn, lock_id).await {
                Ok(acquired) => acquired,
                Err(error) => {
                    tracing::warn!(coordinator = name, %error, "leader: lock attempt failed");
                    let _ = conn.close().await;
                    tokio::time::sleep(LEADER_CONTEND_INTERVAL).await;
                    continue;
                }
            };
            if !acquired {
                let _ = conn.close().await;
                tokio::time::sleep(LEADER_CONTEND_INTERVAL).await;
                continue;
            }
            tracing::info!(coordinator = name, "leadership acquired");
            let task = tokio::spawn(make_task());
            loop {
                tokio::time::sleep(LEADER_WATCHDOG_INTERVAL).await;
                if task.is_finished() {
                    break;
                }
                if !leader_ping_ok(&mut conn, name, LEADER_PING_TIMEOUT).await {
                    break;
                }
            }
            task.abort();
            let _ = task.await;
            // Closing the session releases the advisory lock; if the
            // connection already died, Postgres released it server-side.
            // Bounded so a half-open socket cannot wedge re-contention: an
            // elapsed timeout drops the close future and with it the
            // connection, closing the socket non-gracefully.
            let _ = tokio::time::timeout(LEADER_PING_TIMEOUT, conn.close()).await;
            tracing::warn!(coordinator = name, "leadership released; re-contending");
            tokio::time::sleep(LEADER_REJOIN_DELAY).await;
        }
    });
}

/// One watchdog probe on the leader's dedicated lock connection. `false`
/// means leadership must be treated as lost: the ping errored, or — on a
/// half-open TCP connection — did not answer within `ping_timeout`, in
/// which case Postgres may already have released the session lock to
/// another replica. The connection must not be reused after `false` (the
/// cancelled ping leaves the protocol stream mid-message).
async fn leader_ping_ok(
    conn: &mut PgConnection,
    name: &'static str,
    ping_timeout: Duration,
) -> bool {
    match tokio::time::timeout(ping_timeout, sqlx::query("SELECT 1").execute(conn)).await {
        Ok(Ok(_)) => true,
        Ok(Err(error)) => {
            tracing::warn!(coordinator = name, %error, "leader: lock connection lost");
            false
        }
        Err(_) => {
            tracing::warn!(
                coordinator = name,
                timeout = ?ping_timeout,
                "leader: lock connection unresponsive; treating leadership as lost"
            );
            false
        }
    }
}

/// `pg_try_advisory_lock` on a dedicated session connection.
pub(crate) async fn try_advisory_lock(conn: &mut PgConnection, lock_id: i64) -> Result<bool> {
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(lock_id)
        .fetch_one(conn)
        .await?;
    Ok(acquired)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

    use super::*;

    /// Forward bytes until `stall` flips, then keep both sockets open but
    /// forward nothing further: the peer sees a connected socket that never
    /// answers — a half-open TCP connection from its point of view.
    async fn copy_until_stalled(
        mut reader: OwnedReadHalf,
        mut writer: OwnedWriteHalf,
        stall: Arc<AtomicBool>,
    ) {
        let mut buf = [0u8; 8192];
        loop {
            let n = match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if stall.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if writer.write_all(&buf[..n]).await.is_err() {
                break;
            }
        }
    }

    /// A half-open leader lock connection (peer stops responding but the
    /// socket stays open) must fail the watchdog ping within the client-side
    /// bound — not block until the TCP retransmit timeout, during which
    /// Postgres releases the session lock and a second replica leads
    /// concurrently. Exercised through a stallable TCP proxy in front of the
    /// test Postgres.
    #[tokio::test]
    async fn leader_ping_times_out_on_stalled_connection() {
        let Ok(url) = std::env::var("GITDEBT_TEST_DATABASE_URL") else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let upstream_options: PgConnectOptions = url.parse().unwrap();
        let upstream_addr = format!(
            "{}:{}",
            upstream_options.get_host(),
            upstream_options.get_port()
        );

        let stall = Arc::new(AtomicBool::new(false));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = listener.local_addr().unwrap().port();
        let proxy_stall = stall.clone();
        tokio::spawn(async move {
            let (client, _) = listener.accept().await.unwrap();
            let upstream = tokio::net::TcpStream::connect(upstream_addr).await.unwrap();
            let (client_read, client_write) = client.into_split();
            let (upstream_read, upstream_write) = upstream.into_split();
            tokio::spawn(copy_until_stalled(
                client_read,
                upstream_write,
                proxy_stall.clone(),
            ));
            tokio::spawn(copy_until_stalled(upstream_read, client_write, proxy_stall));
        });

        let proxied = upstream_options.host("127.0.0.1").port(proxy_port);
        let mut conn = PgConnection::connect_with(&proxied).await.unwrap();
        assert!(
            leader_ping_ok(&mut conn, "test-proxy", Duration::from_secs(5)).await,
            "healthy connection through the proxy must pass the watchdog ping"
        );

        stall.store(true, Ordering::SeqCst);
        let start = std::time::Instant::now();
        assert!(
            !leader_ping_ok(&mut conn, "test-proxy", Duration::from_millis(500)).await,
            "a stalled connection must be treated as lost leadership"
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the watchdog ping must give up within the client-side bound, took {:?}",
            start.elapsed()
        );

        // The bounded close must not hang on the half-open socket either.
        let start = std::time::Instant::now();
        let _ = tokio::time::timeout(Duration::from_millis(500), conn.close()).await;
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "closing the dead lock connection must be bounded, took {:?}",
            start.elapsed()
        );
    }

    /// Two contenders on separate session connections: exactly one wins the
    /// advisory lock, and closing the winner's session frees it for the other.
    #[tokio::test]
    async fn advisory_leader_lock_is_mutually_exclusive() {
        let Ok(url) = std::env::var("GITDEBT_TEST_DATABASE_URL") else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        // Unique per test process so parallel CI runs cannot collide.
        let lock_id = 0x6769_7464_0000_0000_i64 | i64::from(std::process::id());
        let mut first = PgConnection::connect(&url).await.unwrap();
        let mut second = PgConnection::connect(&url).await.unwrap();

        assert!(try_advisory_lock(&mut first, lock_id).await.unwrap());
        assert!(
            !try_advisory_lock(&mut second, lock_id).await.unwrap(),
            "second contender must lose while the first session holds the lock"
        );

        first.close().await.unwrap();
        // The release is server-side on session close; poll briefly.
        let mut acquired = false;
        for _ in 0..50 {
            if try_advisory_lock(&mut second, lock_id).await.unwrap() {
                acquired = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(acquired, "lock must be reacquirable after the winner exits");
        second.close().await.unwrap();
    }
}
