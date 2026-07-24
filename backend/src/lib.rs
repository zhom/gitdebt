pub mod aggregate;
pub mod analyzer;
pub mod animated_gif;
pub mod api;
pub mod archive_hourly_db;
pub mod archive_worker;
pub mod auth;
pub mod badge;
pub mod bootstrap;
pub mod brand;
pub mod cache;
pub mod cards;
pub mod catalog;
pub mod chart;
pub mod code_count;
pub mod crypto;
pub mod db;
pub mod export;
pub mod gh_archive;
pub mod gh_archive_hourly;
pub mod github;
pub mod leaderboard;
pub mod og;
pub mod progress;
pub mod queue;
pub mod raster;
pub mod rate_limit;
pub mod redis;
pub mod repo_analysis;
pub mod repo_charts;
pub mod repo_endpoints;
pub mod repo_history;
pub mod repo_stats;
pub mod streak;
pub mod texture;
pub mod theme;
pub mod usage;
pub mod webhook;
pub mod worker;

/// Postgres handles for DB-backed tests, shared by the unit tests in this
/// crate and by the integration suites in `backend/tests/`.
///
/// Each `#[tokio::test]` runs on its own runtime, so a pool cannot be shared
/// between tests (sqlx ties pool background tasks to the creating runtime).
/// Every test therefore opens its own small pool, but only the first one
/// applies the schema: re-running the idempotent DDL while other tests hold
/// row locks deadlocks in Postgres.
#[doc(hidden)]
pub mod test_db {
    use crate::db::Db;

    /// Connections per test pool. Test threads run in parallel, so these sum
    /// against Postgres' connection limit.
    const TEST_POOL_SIZE: u32 = 2;

    static SCHEMA_APPLIED: tokio::sync::Mutex<bool> = tokio::sync::Mutex::const_new(false);

    /// A pool on the test database, applying the schema on first use.
    pub async fn connect(database_url: &str) -> anyhow::Result<Db> {
        {
            let mut applied = SCHEMA_APPLIED.lock().await;
            if !*applied {
                Db::connect_with_pool_size(database_url, TEST_POOL_SIZE).await?;
                *applied = true;
            }
        }
        Db::connect_pool_only(database_url, TEST_POOL_SIZE).await
    }

    /// The test database, or `None` when `GITDEBT_TEST_DATABASE_URL` is unset
    /// so DB-backed tests can skip instead of failing.
    pub async fn shared() -> Option<Db> {
        let url = std::env::var("GITDEBT_TEST_DATABASE_URL").ok()?;
        Some(connect(&url).await.expect("connect test db"))
    }
}
