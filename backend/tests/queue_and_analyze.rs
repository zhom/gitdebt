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
use gitdebt::{
    analyzer,
    cache::{ArchiveStarEvent, Cache},
    db::Db,
    queue, repo_analysis,
    repo_history::{CommitInfo, RepoStorage},
    repo_stats,
};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;

static SCHEMA_READY: OnceLock<()> = OnceLock::new();
static SCHEMA_LOCK: Mutex<()> = Mutex::const_new(());
static STAR_CLAIM_LOCK: Mutex<()> = Mutex::const_new(());
const CURRENT_ANALYSIS_REVISION: i32 = 3;

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
    let _ = sqlx::query("DELETE FROM repo_author_stats WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_file_stats WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_commit_days WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_todo_deltas WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_lines WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_star_arrivals WHERE repo LIKE $1")
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
    let _claim_guard = STAR_CLAIM_LOCK.lock().await;
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

    // Fresh claims survive a rolling replica startup; an expired lease is
    // recoverable.
    assert_eq!(queue::reset_inflight_on_startup(&db).await.unwrap(), 0);
    sqlx::query(
        "UPDATE star_fetch_queue SET claimed_at = NOW() - INTERVAL '16 minutes' WHERE repo = $1",
    )
    .bind(&a)
    .execute(&db.pool)
    .await
    .unwrap();
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
async fn transient_star_failures_stay_retryable_with_a_durable_delay() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-retry/";
    cleanup(&db, prefix).await;
    let repo = format!("{prefix}repo");

    queue::enqueue(&db, &repo, 0).await.unwrap();
    queue::fail(&db, &repo, "temporary provider failure")
        .await
        .unwrap();
    let row: (String, i64, bool) = sqlx::query_as(
        "SELECT status, attempts, next_attempt_at > NOW() \
         FROM star_fetch_queue WHERE repo = $1",
    )
    .bind(&repo)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row, ("pending".to_string(), 1, true));
    assert!(queue::is_active(&db, &repo).await.unwrap());
    assert!(queue::is_retrying(&db, &repo).await.unwrap());

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn archive_batch_claim_collects_jobs_across_repository_creation_dates() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let _claim_guard = STAR_CLAIM_LOCK.lock().await;
    let prefix = "gitdebt-test-archive-batch/";
    cleanup(&db, prefix).await;
    let old = format!("{prefix}old");
    let new = format!("{prefix}new");

    sqlx::query(
        "INSERT INTO repos (repo, created_at) VALUES \
            ($1, '2012-01-01T00:00:00Z'), ($2, '2025-01-01T00:00:00Z')",
    )
    .bind(&old)
    .bind(&new)
    .execute(&db.pool)
    .await
    .unwrap();
    // Use a test-local high priority and an exact limit so concurrent queue
    // tests cannot have their cold jobs swept into this global batch claim.
    queue::enqueue(&db, &old, 10_000).await.unwrap();
    queue::enqueue(&db, &new, 10_000).await.unwrap();

    let claimed = queue::claim_many(&db, "archive-test", 2).await.unwrap();
    let claimed_repos = claimed
        .into_iter()
        .map(|job| job.repo)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(claimed_repos, std::collections::HashSet::from([old, new]));

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
async fn readonly_cold_repo_is_pending_without_enqueuing() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-readonly/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    let rate = std::sync::Arc::new(
        gitdebt::rate_limit::RateLimitTracker::load(db.clone())
            .await
            .unwrap(),
    );
    let github = std::sync::Arc::new(gitdebt::github::GithubClient::new(None, rate).unwrap());
    let ctx = analyzer::AnalyzerCtx { github, cache };
    let owner = "gitdebt-test-readonly";
    let repo = "x";
    let full = format!("{owner}/{repo}");

    let result = analyzer::analyze_repo_readonly(owner, repo, &ctx)
        .await
        .expect("readonly analyze succeeds");
    assert!(result.pending);
    assert!(result.history.is_empty());
    assert!(
        !queue::is_active(&db, &full).await.unwrap(),
        "static snapshot reads must not create queue work"
    );

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn repo_analysis_enqueue_is_freshness_bounded_and_old_dead_jobs_revive() {
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
        repo_analysis::EnqueueOutcome::Enqueued
    );
    let revived: (String, i32) =
        sqlx::query_as("SELECT status, attempts FROM repo_analysis_queue WHERE repo = $1")
            .bind(&active)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(revived, ("pending".to_string(), 0));

    let missing = format!("{prefix}missing");
    sqlx::query(
        "INSERT INTO repos (repo, missing) VALUES ($1, TRUE) \
         ON CONFLICT (repo) DO UPDATE SET missing = TRUE",
    )
    .bind(&missing)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO repo_analysis_queue (repo, status, enqueued_at) \
         VALUES ($1, 'dead', NOW())",
    )
    .bind(&missing)
    .execute(&db.pool)
    .await
    .unwrap();
    repo_analysis::revive_retryable_on_startup(&db)
        .await
        .unwrap();
    let missing_status: String =
        sqlx::query_scalar("SELECT status FROM repo_analysis_queue WHERE repo = $1")
            .bind(&missing)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(missing_status, "dead", "404 tombstones stay terminal");

    let fresh = format!("{prefix}fresh");
    sqlx::query(
        "INSERT INTO repo_history \
            (repo, last_analyzed_sha, last_analyzed_at, analysis_revision) \
         VALUES ($1, 'abc', NOW(), $2)",
    )
    .bind(&fresh)
    .bind(CURRENT_ANALYSIS_REVISION)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        repo_analysis::enqueue(&db, &fresh).await.unwrap(),
        repo_analysis::EnqueueOutcome::Fresh
    );

    let unresolved = format!("{prefix}unresolved");
    sqlx::query(
        "INSERT INTO repo_history \
            (repo, last_analyzed_sha, last_analyzed_at, analysis_revision) \
         VALUES ($1, 'def', NOW(), $2)",
    )
    .bind(&unresolved)
    .bind(CURRENT_ANALYSIS_REVISION)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO repo_author_stats \
            (repo, author_email, avatar_url, commits, first_commit_at, last_commit_at) \
         VALUES ($1, 'author@example.com', \
                 'https://www.gravatar.com/avatar/example', 1, NOW(), NOW())",
    )
    .bind(&unresolved)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        repo_analysis::enqueue(&db, &unresolved).await.unwrap(),
        repo_analysis::EnqueueOutcome::Enqueued,
        "fresh commits with an unattempted author mapping must retry enrichment"
    );

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn repo_analysis_enqueue_many_skips_settled_jobs_and_bounds_new_work() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-analysis-batch/";
    cleanup(&db, prefix).await;

    let fresh = format!("{prefix}fresh");
    sqlx::query(
        "INSERT INTO repo_history \
            (repo, last_analyzed_sha, last_analyzed_at, analysis_revision) \
         VALUES ($1, 'abc', NOW(), $2)",
    )
    .bind(&fresh)
    .bind(CURRENT_ANALYSIS_REVISION)
    .execute(&db.pool)
    .await
    .unwrap();
    let active = format!("{prefix}active");
    repo_analysis::enqueue(&db, &active).await.unwrap();
    let cold_a = format!("{prefix}cold-a");
    let cold_b = format!("{prefix}cold-b");
    let cold_c = format!("{prefix}cold-c");
    let repos = vec![
        fresh,
        active,
        cold_a.clone(),
        cold_b.clone(),
        cold_c.clone(),
    ];

    let added = repo_analysis::enqueue_many(&db, &repos, 2).await.unwrap();
    assert_eq!(added, 2);
    assert!(analysis_queue_row_exists(&db, &cold_a).await);
    assert!(analysis_queue_row_exists(&db, &cold_b).await);
    assert!(!analysis_queue_row_exists(&db, &cold_c).await);

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn repo_analysis_startup_recovers_only_stale_leases() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-analysis-lease/";
    cleanup(&db, prefix).await;
    let fresh = format!("{prefix}fresh");
    let stale = format!("{prefix}stale");
    sqlx::query(
        "INSERT INTO repo_analysis_queue \
            (repo, status, enqueued_at, claimed_at, worker_id) VALUES \
            ($1, 'in_progress', NOW(), NOW(), 'old-fresh'), \
            ($2, 'in_progress', NOW(), NOW() - INTERVAL '3 minutes', 'old-stale')",
    )
    .bind(&fresh)
    .bind(&stale)
    .execute(&db.pool)
    .await
    .unwrap();

    assert_eq!(
        repo_analysis::reset_inflight_on_startup(&db).await.unwrap(),
        1
    );
    let statuses: Vec<(String, String)> = sqlx::query_as(
        "SELECT repo, status FROM repo_analysis_queue \
         WHERE repo LIKE $1 ORDER BY repo",
    )
    .bind(format!("{prefix}%"))
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        statuses,
        vec![
            (fresh, "in_progress".to_string()),
            (stale, "pending".to_string()),
        ]
    );

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn repo_analysis_commit_and_merge_head_advance_atomically() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-analysis-head/";
    cleanup(&db, prefix).await;
    let repo = format!("{prefix}repo");
    let committed_at = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let commit = CommitInfo {
        sha: "1111111111111111111111111111111111111111".to_string(),
        author_email: "author@example.com".to_string(),
        author_name: "Author".to_string(),
        committed_at,
        committed_day: committed_at.date_naive(),
        message_first_line: "change".to_string(),
        is_fix: false,
        paths_changed: vec!["src/lib.rs".to_string()],
        todo_added: 0,
        todo_removed: 0,
    };
    let merge_head = "2222222222222222222222222222222222222222";

    repo_stats::apply_commits_at_head(&db, &repo, &[commit], merge_head)
        .await
        .unwrap();

    let state: (Option<String>, Option<String>, i64) = sqlx::query_as(
        "SELECT last_analyzed_sha, head_sha, total_commits \
         FROM repo_history WHERE repo = $1",
    )
    .bind(&repo)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(state.0.as_deref(), Some(merge_head));
    assert_eq!(state.1.as_deref(), Some(merge_head));
    assert_eq!(state.2, 1);
    let file_commits: i64 = sqlx::query_scalar(
        "SELECT commits FROM repo_file_stats WHERE repo = $1 AND path = 'src/lib.rs'",
    )
    .bind(&repo)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(file_commits, 1);

    let merge_only_head = "3333333333333333333333333333333333333333";
    repo_stats::apply_commits_at_head(&db, &repo, &[], merge_only_head)
        .await
        .unwrap();
    let merge_only_state: (Option<String>, i64) =
        sqlx::query_as("SELECT last_analyzed_sha, total_commits FROM repo_history WHERE repo = $1")
            .bind(&repo)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(merge_only_state.0.as_deref(), Some(merge_only_head));
    assert_eq!(merge_only_state.1, 1);

    let replacement_head = "4444444444444444444444444444444444444444";
    let replacement = CommitInfo {
        sha: replacement_head.to_string(),
        author_email: "replacement@example.com".to_string(),
        author_name: "Replacement".to_string(),
        committed_at,
        committed_day: committed_at.date_naive(),
        message_first_line: "replacement window".to_string(),
        is_fix: true,
        paths_changed: vec!["src/new.rs".to_string()],
        todo_added: 1,
        todo_removed: 0,
    };
    repo_stats::replace_commits_at_head(&db, &repo, &[replacement], replacement_head, 1_567)
        .await
        .unwrap();
    let replaced: (Option<String>, i64) =
        sqlx::query_as("SELECT last_analyzed_sha, total_commits FROM repo_history WHERE repo = $1")
            .bind(&repo)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(replaced.0.as_deref(), Some(replacement_head));
    assert_eq!(replaced.1, 1_567, "reachable count includes merge commits");
    let paths: Vec<String> =
        sqlx::query_scalar("SELECT path FROM repo_file_stats WHERE repo = $1 ORDER BY path")
            .bind(&repo)
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert_eq!(paths, vec!["src/new.rs"]);

    cleanup(&db, prefix).await;
}

async fn analysis_queue_row_exists(db: &Db, repo: &str) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM repo_analysis_queue WHERE repo = $1)",
    )
    .bind(repo)
    .fetch_one(&db.pool)
    .await
    .unwrap()
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
async fn archive_windows_are_hidden_until_final_commit() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-archive/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    let full = format!("{prefix}history");
    cache
        .put_repo_metadata(&full, Some(42), 99, 3, None)
        .await
        .unwrap();
    let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let start = chrono::NaiveDate::from_ymd_opt(2011, 2, 12).unwrap();
    let next = chrono::NaiveDate::from_ymd_opt(2011, 3, 1).unwrap();
    cache
        .commit_archive_backfill_window(
            &full,
            start,
            next,
            &[ArchiveStarEvent {
                source_event_id: Some("archive-1".to_string()),
                starred_at: base,
            }],
            false,
        )
        .await
        .unwrap();
    assert!(
        cache.get_repo_stargazers(&full).await.unwrap().is_none(),
        "partial archive history must remain invisible"
    );

    let done = chrono::NaiveDate::from_ymd_opt(2011, 4, 1).unwrap();
    cache
        .commit_archive_backfill_window(
            &full,
            next,
            done,
            &[ArchiveStarEvent {
                source_event_id: Some("archive-2".to_string()),
                starred_at: base + Duration::seconds(1),
            }],
            true,
        )
        .await
        .unwrap();
    let history = cache.get_repo_stargazers(&full).await.unwrap().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(
        cache.get_repo_star_count(&full).await.unwrap(),
        Some(99),
        "archive event count must not replace GitHub's current star total"
    );
    let summary = cache.get_repo_summary(&full).await.unwrap().unwrap();
    assert_eq!(summary.history_source.as_deref(), Some("gh_archive"));
    assert_eq!(summary.history_observed_count, Some(2));

    cleanup(&db, prefix).await;
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

#[tokio::test]
async fn clone_quota_sum_decodes_as_bigint() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-quota/";
    cleanup(&db, prefix).await;

    let clone_root = tempfile::tempdir().expect("temp clone root");
    let repo = format!("{prefix}repo");
    repo_stats::record_clone(&db, &repo, clone_root.path(), 4096)
        .await
        .expect("record clone bytes");
    let storage = RepoStorage {
        root: clone_root.path().to_path_buf(),
        quota_bytes: 1024 * 1024 * 1024 * 1024,
        high_watermark_pct: 80,
    };
    assert_eq!(
        repo_stats::evict_to_quota(&db, &storage)
            .await
            .expect("SUM(bigint) must decode after its explicit cast"),
        0
    );

    cleanup(&db, prefix).await;
}
