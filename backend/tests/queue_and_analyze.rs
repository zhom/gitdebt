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
    github::RepoMetadata,
    queue, repo_analysis,
    repo_history::{CommitInfo, RepoStorage},
    repo_stats,
};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;

static SCHEMA_READY: OnceLock<()> = OnceLock::new();
static SCHEMA_LOCK: Mutex<()> = Mutex::const_new(());
static STAR_CLAIM_LOCK: Mutex<()> = Mutex::const_new(());
const CURRENT_ANALYSIS_REVISION: i32 = 5;

fn metadata(id: u64, stars: u64, forks: u64) -> RepoMetadata {
    RepoMetadata {
        id: Some(id),
        stargazers_count: stars,
        forks_count: forks,
        ..RepoMetadata::default()
    }
}

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
        let db = gitdebt::test_db::connect(&url)
            .await
            .expect("connect test db");
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
    let _ = sqlx::query("DELETE FROM repo_author_commit_days WHERE repo LIKE $1")
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
async fn catalog_bootstrap_only_enqueues_cold_or_stale_approximate_histories() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-catalog/";
    cleanup(&db, prefix).await;

    let cold = format!("{prefix}cold");
    let exact = format!("{prefix}exact");
    let approximate = format!("{prefix}approximate");
    let missing = format!("{prefix}missing");
    sqlx::query(
        "INSERT INTO repos \
            (repo, metadata_fetched_at, history_complete, history_source, \
             stargazers_fetched_at, archive_fetched_at, missing) \
         VALUES \
            ($1, NOW(), TRUE, 'github_api', NOW() - INTERVAL '1 year', NULL, FALSE), \
            ($2, NOW(), TRUE, 'gh_archive', NULL, NOW() - INTERVAL '7 hours', FALSE), \
            ($3, NOW(), FALSE, NULL, NULL, NULL, TRUE)",
    )
    .bind(&exact)
    .bind(&approximate)
    .bind(&missing)
    .execute(&db.pool)
    .await
    .unwrap();

    let repos = vec![
        cold.clone(),
        exact.clone(),
        approximate.clone(),
        missing.clone(),
    ];
    assert_eq!(
        queue::enqueue_cold_or_stale_many(&db, &repos, 0)
            .await
            .unwrap(),
        2
    );
    queue::enqueue(&db, &exact, 100).await.unwrap();
    let queued: Vec<String> =
        sqlx::query_scalar("SELECT repo FROM star_fetch_queue WHERE repo LIKE $1 ORDER BY repo")
            .bind(format!("{prefix}%"))
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert_eq!(queued, vec![approximate, cold]);

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
    seed_current_analysis(&db, &fresh, "abc").await;
    assert_eq!(
        repo_analysis::enqueue(&db, &fresh).await.unwrap(),
        repo_analysis::EnqueueOutcome::Fresh
    );

    // Analysis stopped short of the head it observed: the run did not
    // finish, so it is not current and must be re-enqueued.
    let mid = format!("{prefix}mid-analysis");
    sqlx::query(
        "INSERT INTO repo_history \
            (repo, last_analyzed_sha, head_sha, last_analyzed_at, analysis_revision) \
         VALUES ($1, 'ghi', 'jkl', NOW(), $2)",
    )
    .bind(&mid)
    .bind(CURRENT_ANALYSIS_REVISION)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        repo_analysis::enqueue(&db, &mid).await.unwrap(),
        repo_analysis::EnqueueOutcome::Enqueued,
        "an analysis that never reached its observed head is not current"
    );

    // A superseded algorithm revision is likewise not current.
    let stale_revision = format!("{prefix}stale-revision");
    sqlx::query(
        "INSERT INTO repo_history \
            (repo, last_analyzed_sha, head_sha, last_analyzed_at, analysis_revision) \
         VALUES ($1, 'mno', 'mno', NOW(), $2)",
    )
    .bind(&stale_revision)
    .bind(CURRENT_ANALYSIS_REVISION - 1)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        repo_analysis::enqueue(&db, &stale_revision).await.unwrap(),
        repo_analysis::EnqueueOutcome::Enqueued
    );

    cleanup(&db, prefix).await;
}

/// Seed the exact state `repo_stats::write_commits_at_head` +
/// `record_analysis_details` leave behind for a completed, current run.
async fn seed_current_analysis(db: &Db, repo: &str, sha: &str) {
    sqlx::query(
        "INSERT INTO repo_history \
            (repo, last_analyzed_sha, head_sha, last_analyzed_at, analysis_revision) \
         VALUES ($1, $2, $2, NOW(), $3) \
         ON CONFLICT (repo) DO UPDATE SET \
            last_analyzed_sha = EXCLUDED.last_analyzed_sha, \
            head_sha = EXCLUDED.head_sha, \
            last_analyzed_at = EXCLUDED.last_analyzed_at, \
            analysis_revision = EXCLUDED.analysis_revision",
    )
    .bind(repo)
    .bind(sha)
    .bind(CURRENT_ANALYSIS_REVISION)
    .execute(&db.pool)
    .await
    .unwrap();
}

async fn seed_unresolved_author(db: &Db, repo: &str, email: &str) {
    sqlx::query(
        "INSERT INTO repo_author_stats \
            (repo, author_email, avatar_url, commits, first_commit_at, last_commit_at) \
         VALUES ($1, $2, 'https://www.gravatar.com/avatar/example', 1, NOW(), NOW()) \
         ON CONFLICT (repo, author_email) DO UPDATE SET enrich_attempted_at = NULL",
    )
    .bind(repo)
    .bind(email)
    .execute(&db.pool)
    .await
    .unwrap();
}

async fn unstamped_author_count(db: &Db, repo: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM repo_author_stats \
         WHERE repo = $1 AND enrich_attempted_at IS NULL",
    )
    .bind(repo)
    .fetch_one(&db.pool)
    .await
    .unwrap()
}

/// The 12-hour "Analyzing 7 repositories" loop, as a regression test.
///
/// Author login/avatar enrichment is best-effort GitHub metadata, and some
/// commit emails can never resolve to a login. When readiness required
/// every such row to be enriched, the repositories holding them could never
/// become ready: the profile poll re-enqueued them every few seconds, each
/// run applied zero commits and completed, and the queue never drained.
///
/// A repository whose analysis is done and current is ready, unenriched
/// authors or not — `enqueue` answers `Fresh`, deletes any leftover queue
/// row, and the readiness counts include it.
#[tokio::test]
async fn unenriched_authors_never_block_analysis_readiness() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-enrich-gate/";
    cleanup(&db, prefix).await;

    let repo = format!("{prefix}stuck");
    seed_current_analysis(&db, &repo, "head1").await;
    for i in 0..5 {
        seed_unresolved_author(&db, &repo, &format!("ghost-{i}@example.com")).await;
    }
    assert_eq!(unstamped_author_count(&db, &repo).await, 5);

    assert!(
        repo_analysis::analysis_is_current(&db, &repo)
            .await
            .unwrap(),
        "a current analysis is current regardless of author enrichment"
    );
    assert_eq!(
        repo_analysis::enqueue(&db, &repo).await.unwrap(),
        repo_analysis::EnqueueOutcome::Fresh,
        "unenriched authors must not re-open a finished analysis"
    );
    assert!(
        !analysis_queue_row_exists(&db, &repo).await,
        "a Fresh enqueue drains the queue instead of scheduling a no-op run"
    );

    // The profile poll's repeat enqueues stay no-ops — this is the loop.
    for _ in 0..3 {
        assert_eq!(
            repo_analysis::enqueue(&db, &repo).await.unwrap(),
            repo_analysis::EnqueueOutcome::Fresh
        );
    }
    assert!(!analysis_queue_row_exists(&db, &repo).await);

    // A stale leftover queue row is cleared by the same path, so a queue
    // that was already stuck drains as soon as this ships.
    sqlx::query(
        "INSERT INTO repo_analysis_queue (repo, status, enqueued_at) \
         VALUES ($1, 'pending', NOW())",
    )
    .bind(&repo)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        repo_analysis::enqueue(&db, &repo).await.unwrap(),
        repo_analysis::EnqueueOutcome::Fresh
    );
    assert!(!analysis_queue_row_exists(&db, &repo).await);

    // The profile card's own count over this same state is asserted where
    // that SQL lives (`api.rs::user_card_sql_aggregates_without_ambiguous_columns`),
    // rather than mirrored here where it could silently drift.
    cleanup(&db, prefix).await;
}

/// The background author-enrichment sweep must converge on its own.
///
/// It runs with no local clone here (and, in CI, no GitHub budget), which
/// is the case that used to make no progress at all: rows were selected,
/// nothing could be resolved, nothing was stamped, and the next pass picked
/// the identical rows. The sweep now stamps every row it selects, so the
/// first pass drains the backlog and the second finds nothing.
#[tokio::test]
async fn author_enrichment_sweep_stamps_attempted_rows_and_terminates() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-enrich-sweep/";
    cleanup(&db, prefix).await;

    let repo = format!("{prefix}unresolvable");
    seed_current_analysis(&db, &repo, "head1").await;
    // A clone path this replica does not hold: no commit can be sampled.
    sqlx::query("UPDATE repo_history SET clone_path = $2 WHERE repo = $1")
        .bind(&repo)
        .bind("/nonexistent/gitdebt-test-enrich-sweep")
        .execute(&db.pool)
        .await
        .unwrap();
    for i in 0..4 {
        seed_unresolved_author(&db, &repo, &format!("ghost-{i}@example.com")).await;
    }
    assert_eq!(unstamped_author_count(&db, &repo).await, 4);

    let rate = std::sync::Arc::new(
        gitdebt::rate_limit::RateLimitTracker::load(db.clone())
            .await
            .unwrap(),
    );
    let ctx = repo_analysis::AnalysisCtx {
        db: db.clone(),
        storage: std::sync::Arc::new(RepoStorage::from_env()),
        github: std::sync::Arc::new(gitdebt::github::GithubClient::new(None, rate).unwrap()),
        gh_app: None,
    };

    // One pass covers at most AUTHOR_ENRICH_SWEEP_REPOS repositories, and
    // the shared test database may hold other fixtures' backlogs; a handful
    // of passes is still a hard bound, which is the point.
    let mut attempted = 0usize;
    for _ in 0..3 {
        let pass = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            repo_analysis::sweep_author_enrichment_until(
                &ctx,
                std::time::Instant::now() + std::time::Duration::from_secs(5),
            ),
        )
        .await
        .expect("the sweep is wall-clock bounded")
        .expect("sweep pass");
        attempted += pass.rows_attempted;
        if unstamped_author_count(&db, &repo).await == 0 {
            break;
        }
    }
    assert!(attempted >= 4, "the sweep must attempt this repo's rows");
    assert_eq!(
        unstamped_author_count(&db, &repo).await,
        0,
        "every selected row leaves the pass stamped, even with nothing resolvable"
    );

    // The stamp is deliberately short of `now` so a transient failure
    // retries in hours — but it is far enough inside the TTL that the very
    // next pass cannot re-pick the same rows. That is what terminates.
    let reselected: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM repo_author_stats \
         WHERE repo = $1 \
           AND (enrich_attempted_at IS NULL \
                OR enrich_attempted_at < NOW() - INTERVAL '30 days')",
    )
    .bind(&repo)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(reselected, 0, "no row is eligible for the following pass");

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
    seed_current_analysis(&db, &fresh, "abc").await;
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
        file_changes: vec![gitdebt::repo_history::FileChange {
            path: "src/lib.rs".to_string(),
            lines_added: 12,
            lines_deleted: 3,
            binary: false,
        }],
        lines_added: 12,
        lines_deleted: 3,
        binary_files: 0,
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
        file_changes: vec![gitdebt::repo_history::FileChange {
            path: "src/new.rs".to_string(),
            lines_added: 8,
            lines_deleted: 2,
            binary: false,
        }],
        lines_added: 8,
        lines_deleted: 2,
        binary_files: 0,
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
    let author_days: Vec<(String, i64)> = sqlx::query_as(
        "SELECT author_email, commits FROM repo_author_commit_days \
         WHERE repo = $1 ORDER BY author_email",
    )
    .bind(&repo)
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        author_days,
        vec![("replacement@example.com".to_string(), 1)],
        "atomic replacement must remove stale per-author streak days"
    );

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
    cache
        .put_repo_metadata(&full, &metadata(101, 5, 0))
        .await
        .unwrap();
    cache.put_repo_stargazers(&full, &items).await.unwrap();
    assert!(cache.repo_stargazers_complete(&full).await.unwrap());
    assert!(
        cache
            .repo_stargazers_fresh_within(&full, Duration::hours(6))
            .await
            .unwrap()
    );
    sqlx::query(
        "UPDATE repos SET stargazers_fetched_at = NOW() - INTERVAL '1 year' WHERE repo = $1",
    )
    .bind(&full)
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(
        cache
            .repo_stargazers_fresh_within(&full, Duration::hours(6))
            .await
            .unwrap(),
        "completed exact snapshots never age out"
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
    assert_eq!(res.history.len(), 1, "same-day stars aggregate in SQL");
    assert_eq!(res.history[0].stars, 5);
    let insights = res
        .star_history_insights
        .expect("complete Postgres history derives report insights");
    assert_eq!(
        insights.largest_day.map(|record| record.stars_gained),
        Some(5)
    );
    assert!(
        insights
            .milestones
            .iter()
            .all(|milestone| milestone.reached_at.is_none())
    );

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
    cache
        .put_repo_metadata(&full, &metadata(102, 3, 0))
        .await
        .unwrap();
    cache.put_repo_stargazers(&full, &initial).await.unwrap();

    // Incremental append of two newer rows.
    let tail: Vec<(i64, _)> = (3..5)
        .map(|i| (i + 1, base + Duration::seconds(i)))
        .collect();
    cache.append_repo_stargazers(&full, &tail, 5).await.unwrap();

    let got = cache.get_repo_stargazers(&full).await.unwrap().unwrap();
    assert_eq!(got.len(), 5, "appended tail is present and complete");
    let chart_series = cache.get_repo_star_series(&full).await.unwrap().unwrap();
    assert_eq!(
        chart_series.len(),
        1,
        "same-day events must aggregate before common read paths"
    );
    assert_eq!(chart_series[0].stars, 5);
    assert_eq!(cache.get_repo_star_count(&full).await.unwrap(), Some(5));

    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn completed_history_is_hidden_until_public_metadata_is_recorded() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-public-proof/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    let full = format!("{prefix}legacy");
    let at = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    cache.put_repo_stargazers(&full, &[(1, at)]).await.unwrap();

    assert!(
        cache.get_repo_stargazers(&full).await.unwrap().is_none(),
        "legacy history without public metadata must never reach readers"
    );
    assert!(
        cache.get_repo_star_series(&full).await.unwrap().is_none(),
        "daily chart reads must enforce the same completeness/public gate"
    );
    assert!(!cache.repo_stargazers_complete(&full).await.unwrap());
    assert_eq!(cache.get_repo_star_count(&full).await.unwrap(), None);

    cache
        .put_repo_metadata(&full, &metadata(104, 1, 0))
        .await
        .unwrap();
    assert_eq!(
        cache.get_repo_stargazers(&full).await.unwrap().unwrap(),
        vec![at]
    );
    assert!(cache.repo_stargazers_complete(&full).await.unwrap());
    assert_eq!(cache.get_repo_star_count(&full).await.unwrap(), Some(1));

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
    cache
        .put_repo_metadata(&full, &metadata(103, 1, 0))
        .await
        .unwrap();
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
        .put_repo_metadata(&full, &metadata(42, 99, 3))
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

/// A repository whose HEAD has not moved must still count as freshly
/// analyzed.
///
/// The worker returns early when the stored cursor already equals HEAD, and
/// nothing on that path used to record that the run happened. Freshness is
/// read from `last_analyzed_at`, so such a repository could never become
/// `Fresh` again: every view re-queued it, the worker fetched the remote,
/// rediscovered the same head, and returned — forever, for the majority of
/// tracked repositories, which are the ones that do not change daily.
#[tokio::test]
async fn confirming_an_unchanged_head_keeps_the_analysis_fresh() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-noop-analysis/";
    cleanup(&db, prefix).await;
    let repo = format!("{prefix}unchanged");

    // A completed analysis that has since aged past the freshness window.
    sqlx::query(
        "INSERT INTO repo_history \
            (repo, last_analyzed_sha, last_analyzed_at, head_sha, analysis_revision) \
         VALUES ($1, 'a1b2c3', NOW() - INTERVAL '30 days', 'a1b2c3', $2)",
    )
    .bind(&repo)
    .bind(repo_analysis::CURRENT_ANALYSIS_REVISION)
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(
        !repo_analysis::analysis_is_current(&db, &repo)
            .await
            .unwrap(),
        "a 30-day-old analysis is outside the freshness window"
    );

    // What the worker does when it finds HEAD unchanged.
    repo_stats::touch_analyzed_at(&db, &repo).await.unwrap();

    assert!(
        repo_analysis::analysis_is_current(&db, &repo)
            .await
            .unwrap(),
        "confirming the stored head must refresh the analysis window"
    );
    assert_eq!(
        repo_analysis::enqueue(&db, &repo).await.unwrap(),
        repo_analysis::EnqueueOutcome::Fresh,
        "and the next view must not re-queue the same no-op run"
    );

    cleanup(&db, prefix).await;
}

/// A job that can never succeed must stop consuming queue capacity, and must
/// stay stopped across restarts.
///
/// Every failure used to return the row to `pending` with at most an hour of
/// backoff, so a repository that cannot be cloned retried forever while still
/// counting against the ceiling that admits new work. Startup revival exists
/// for rows parked by releases that had no terminal state, so it must not
/// resurrect the rows this release parks deliberately.
#[tokio::test]
async fn permanently_failing_analyses_are_parked_and_stay_parked() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-analysis-terminal/";
    cleanup(&db, prefix).await;
    let repo = format!("{prefix}hopeless");

    repo_analysis::enqueue(&db, &repo).await.unwrap();
    sqlx::query(
        "UPDATE repo_analysis_queue \
         SET status = 'dead', attempts = 8, last_error = $2 \
         WHERE repo = $1",
    )
    .bind(&repo)
    .bind(format!("{} clone failed", repo_analysis::TERMINAL_MARKER))
    .execute(&db.pool)
    .await
    .unwrap();

    repo_analysis::revive_retryable_on_startup(&db)
        .await
        .unwrap();
    let status: String =
        sqlx::query_scalar("SELECT status FROM repo_analysis_queue WHERE repo = $1")
            .bind(&repo)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(
        status, "dead",
        "a deliberately parked job must survive a restart"
    );

    // A row parked by an older release carries no marker and is still revived.
    let legacy = format!("{prefix}legacy");
    sqlx::query(
        "INSERT INTO repo_analysis_queue (repo, status, enqueued_at, last_error) \
         VALUES ($1, 'dead', NOW(), 'clone failed')",
    )
    .bind(&legacy)
    .execute(&db.pool)
    .await
    .unwrap();
    repo_analysis::revive_retryable_on_startup(&db)
        .await
        .unwrap();
    let legacy_status: String =
        sqlx::query_scalar("SELECT status FROM repo_analysis_queue WHERE repo = $1")
            .bind(&legacy)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(legacy_status, "pending");

    cleanup(&db, prefix).await;
}
