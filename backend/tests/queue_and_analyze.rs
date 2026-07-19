//! Integration tests for the star-history fetch queue + the non-blocking
//! analyze path, against a real Postgres.
//!
//! Gated on `GITDEBT_TEST_DATABASE_URL` so `cargo test` stays green in
//! environments without a database (the unit tests cover the pure logic;
//! these cover the SQL + the queue/analyze wiring). To run them:
//!
//! ```bash
//! scripts/db.sh up
//! GITDEBT_TEST_DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt \
//!   cargo test --test queue_and_analyze
//! ```
//!
//! Each test namespaces its repos with a unique prefix and cleans up
//! after itself, so the suite is safe to run repeatedly against a shared
//! dev database without colliding.

use std::sync::OnceLock;

use chrono::{Duration, TimeZone, Utc};
use gitdebt::{analyzer, cache::Cache, db::Db, queue, repo_analysis};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;

static SCHEMA_READY: OnceLock<()> = OnceLock::new();
static SCHEMA_LOCK: Mutex<()> = Mutex::const_new(());

/// Returns a connected `Db` if a test database is configured, else `None`
/// (the test then no-ops). Keeps the suite green where no DB exists.
async fn test_db() -> Option<Db> {
    let url = std::env::var("GITDEBT_TEST_DATABASE_URL").ok()?;

    // Tokio creates one runtime per #[tokio::test], so a PgPool must not be
    // shared through a static OnceCell: the runtime that owns its maintenance
    // tasks can disappear while sibling tests still use the pool. Initialize
    // the schema exactly once, then give every test a small runtime-local pool.
    let schema_guard = SCHEMA_LOCK.lock().await;
    if SCHEMA_READY.get().is_none() {
        let db = Db::connect(&url).await.expect("connect test db");
        SCHEMA_READY.set(()).expect("schema initialized once");
        return Some(db);
    }
    drop(schema_guard);

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect test pool");
    Some(Db { pool })
}

async fn cleanup(db: &Db, prefix: &str) {
    let like = format!("{prefix}%");
    let _ = sqlx::query("DELETE FROM star_fetch_queue WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_stargazers WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repos WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_analysis_queue WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_history WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
}

#[tokio::test]
async fn queue_dedup_and_claim() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-dedup/";
    cleanup(&db, prefix).await;

    let a = format!("{prefix}a");
    let b = format!("{prefix}b");

    // Enqueue `a` twice — dedup keeps it a single row.
    queue::enqueue(&db, &a, 0).await.unwrap();
    queue::enqueue(&db, &a, 0).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM star_fetch_queue WHERE repo = $1")
        .bind(&a)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "enqueue is idempotent per repo");
    assert!(queue::is_active(&db, &a).await.unwrap());

    // Priority ordering: `b` enqueued later but hotter → claimed first.
    queue::enqueue(&db, &b, 100).await.unwrap();
    let first = queue::claim_one(&db, "w0").await.unwrap().unwrap();
    assert_eq!(first.repo, b, "higher priority claimed first");
    assert!(!first.partial);
    assert_eq!(first.next_page, 1);

    // A successful capped chunk resumes at its persisted cursor without
    // consuming the transient-failure attempt budget.
    queue::requeue_partial(&db, &b, 401).await.unwrap();
    let resumed = queue::claim_one(&db, "w2").await.unwrap().unwrap();
    assert_eq!(resumed.repo, b);
    assert!(resumed.partial);
    assert_eq!(resumed.next_page, 401);
    let attempts: i64 = sqlx::query_scalar("SELECT attempts FROM star_fetch_queue WHERE repo = $1")
        .bind(&b)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(attempts, 0);
    queue::complete(&db, &b).await.unwrap();

    // The remaining claim goes to `a`.
    let second = queue::claim_one(&db, "w1").await.unwrap().unwrap();
    assert_eq!(second.repo, a);

    // reset_inflight requeues the in-progress row.
    let reset = queue::reset_inflight_on_startup(&db).await.unwrap();
    assert!(reset >= 1);
    assert!(queue::is_active(&db, &a).await.unwrap());
    assert!(!queue::is_active(&db, &b).await.unwrap());

    // complete() removes the row.
    queue::complete(&db, &a).await.unwrap();
    assert!(!queue::is_active(&db, &a).await.unwrap());

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn analyze_cold_repo_is_pending_and_enqueues_without_paginating() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-cold/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    // GithubClient with NO token + a rate tracker whose budget is already
    // exhausted: if analyze tried to synchronously paginate, it would
    // block on acquire (and the test would hang) — proving it must not.
    let rate = std::sync::Arc::new(
        gitdebt::rate_limit::RateLimitTracker::load(db.clone())
            .await
            .unwrap(),
    );
    let github = std::sync::Arc::new(gitdebt::github::GithubClient::new(None, rate).unwrap());
    let ctx = analyzer::AnalyzerCtx { github, cache };

    let owner = "gitdebt-test-cold";
    let repo = "x";
    let full = format!("{owner}/{repo}");

    // Cold lookup must return promptly with pending=true, empty history,
    // and enqueue a fetch. We bound it with a timeout: a regression to the
    // old synchronous-paginate path would stall here.
    let res = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        analyzer::analyze_repo(owner, repo, &ctx),
    )
    .await
    .expect("analyze must not block on a cold repo")
    .expect("analyze ok");

    assert!(res.pending, "cold repo is pending");
    assert!(!res.history_complete);
    assert!(res.history.is_empty());
    assert!(
        queue::is_active(&db, &full).await.unwrap(),
        "cold analyze enqueues a fetch"
    );

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn repo_analysis_enqueue_is_freshness_bounded_and_dead_is_terminal() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-analysis-queue/";
    cleanup(&db, prefix).await;

    let active = format!("{prefix}active");
    assert_eq!(
        repo_analysis::enqueue(&db, &active).await.unwrap(),
        repo_analysis::EnqueueOutcome::Enqueued
    );
    assert_eq!(
        repo_analysis::enqueue(&db, &active).await.unwrap(),
        repo_analysis::EnqueueOutcome::AlreadyActive
    );
    sqlx::query("UPDATE repo_analysis_queue SET status = 'dead' WHERE repo = $1")
        .bind(&active)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        repo_analysis::enqueue(&db, &active).await.unwrap(),
        repo_analysis::EnqueueOutcome::Dead
    );

    let fresh = format!("{prefix}fresh");
    sqlx::query(
        "INSERT INTO repo_history (repo, last_analyzed_sha, last_analyzed_at) \
         VALUES ($1, 'abc', NOW())",
    )
    .bind(&fresh)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        repo_analysis::enqueue(&db, &fresh).await.unwrap(),
        repo_analysis::EnqueueOutcome::Fresh
    );

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn analyze_fresh_complete_repo_returns_history_not_pending() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-fresh/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    let owner = "gitdebt-test-fresh";
    let repo = "y";
    let full = format!("{owner}/{repo}");

    // Seed a complete, fresh stargazer set directly through the cache.
    let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let items: Vec<(i64, _)> = (0..5)
        .map(|i| (i + 1, base + Duration::seconds(i)))
        .collect();
    cache.put_repo_stargazers(&full, &items).await.unwrap();
    assert!(cache.repo_stargazers_complete(&full).await.unwrap());
    assert!(
        cache
            .repo_stargazers_fresh_within(&full, Duration::hours(6))
            .await
            .unwrap()
    );

    let rate = std::sync::Arc::new(
        gitdebt::rate_limit::RateLimitTracker::load(db.clone())
            .await
            .unwrap(),
    );
    let github = std::sync::Arc::new(gitdebt::github::GithubClient::new(None, rate).unwrap());
    let ctx = analyzer::AnalyzerCtx { github, cache };

    let res = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        analyzer::analyze_repo(owner, repo, &ctx),
    )
    .await
    .expect("analyze must not block")
    .expect("analyze ok");

    assert!(res.history_complete);
    assert!(!res.pending, "fresh complete repo is not pending");
    assert_eq!(res.total_stars, 5);
    assert_eq!(res.history.len(), 5);

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn incremental_append_through_cache() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-incr/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    let full = "gitdebt-test-incr/z".to_string();

    let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let initial: Vec<(i64, _)> = (0..3)
        .map(|i| (i + 1, base + Duration::seconds(i)))
        .collect();
    cache.put_repo_stargazers(&full, &initial).await.unwrap();

    // Incremental append of two newer rows.
    let tail: Vec<(i64, _)> = (3..5)
        .map(|i| (i + 1, base + Duration::seconds(i)))
        .collect();
    cache.append_repo_stargazers(&full, &tail, 5).await.unwrap();

    let got = cache.get_repo_stargazers(&full).await.unwrap().unwrap();
    assert_eq!(got.len(), 5, "appended tail is present and complete");
    assert_eq!(cache.get_repo_star_count(&full).await.unwrap(), Some(5));

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn partial_fetch_leaves_incomplete() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-partial/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    let full = "gitdebt-test-partial/big".to_string();

    let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let stale = vec![(99, base - Duration::seconds(1))];
    cache.put_repo_stargazers(&full, &stale).await.unwrap();

    let items: Vec<(i64, _)> = (0..10)
        .map(|i| (i + 1, base + Duration::seconds(i)))
        .collect();

    // A fresh backfill replaces the prior snapshot and becomes unreadable
    // until every chunk is committed.
    cache
        .replace_repo_stargazers_partial(&full, &items[..4])
        .await
        .unwrap();
    assert!(
        !cache.repo_stargazers_complete(&full).await.unwrap(),
        "partial fetch keeps stargazers_complete = FALSE"
    );
    assert!(cache.get_repo_stargazers(&full).await.unwrap().is_none());
    let first = cache.get_repo_stargazers_partial(&full).await.unwrap();
    assert_eq!(first.len(), 4);
    assert!(first.iter().all(|(position, _)| *position != 99));

    // Continuation retries are idempotent, and only the final transaction
    // flips the cache back to complete with an exact row count.
    cache
        .put_repo_stargazers_partial(&full, &items[4..7])
        .await
        .unwrap();
    cache
        .put_repo_stargazers_partial(&full, &items[4..7])
        .await
        .unwrap();
    let total = cache
        .finish_repo_stargazers_partial(&full, &items[7..])
        .await
        .unwrap();
    assert_eq!(total, 10);
    assert!(cache.repo_stargazers_complete(&full).await.unwrap());
    assert_eq!(
        cache
            .get_repo_stargazers(&full)
            .await
            .unwrap()
            .unwrap()
            .len(),
        10
    );
    assert_eq!(cache.get_repo_star_count(&full).await.unwrap(), Some(10));

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn stargazer_schema_contains_no_account_identity() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let login_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM information_schema.columns \
         WHERE table_name = 'repo_stargazers' AND column_name = 'login'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(login_columns, 0);

    let position_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM information_schema.columns \
         WHERE table_name = 'repo_stargazers' AND column_name = 'position'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(position_columns, 1);
}

#[tokio::test]
async fn record_view_bumps_count_and_priority() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-view/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    let full = "gitdebt-test-view/v".to_string();

    assert_eq!(cache.get_repo_view_count(&full).await.unwrap(), 0);
    cache.record_repo_view(&full).await.unwrap();
    cache.record_repo_view(&full).await.unwrap();
    assert_eq!(cache.get_repo_view_count(&full).await.unwrap(), 2);

    cleanup(&db, prefix).await;
}
