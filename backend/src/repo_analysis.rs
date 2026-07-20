//! Repo-history analysis worker pool.
//!
//! Pulls jobs from `repo_analysis_queue`, opens or clones the repo via
//! `repo_history`, walks new commits, applies aggregates via
//! `repo_stats::apply_commits`, runs eviction. One worker per pool by
//! default — clones are disk-heavy and parallel I/O thrashes the cache;
//! analysis throughput is bounded by git CLI subprocesses, not by what
//! tokio can multiplex.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::Row;
use tokio::sync::watch;
use tokio::time::sleep;

use crate::code_count;
use crate::db::Db;
use crate::github::GithubClient;
use crate::repo_history::{self, RepoHandle, RepoStorage};
use crate::repo_stats;

const DEFAULT_MAX_PENDING_ANALYSES: i64 = 500;
const DEFAULT_ANALYSIS_FRESH_HOURS: i64 = 24;
const ENQUEUE_LOCK_ID: i64 = 6_794_738_132_977;

#[derive(Clone)]
pub struct AnalysisCtx {
    pub db: Db,
    pub storage: Arc<RepoStorage>,
    pub github: Arc<GithubClient>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Enqueued,
    AlreadyActive,
    Fresh,
    AtCapacity,
}

fn max_pending_analyses() -> i64 {
    std::env::var("MAX_PENDING_ANALYSES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_PENDING_ANALYSES)
}

fn analysis_freshness() -> chrono::Duration {
    let hours = std::env::var("REPO_ANALYSIS_FRESH_HOURS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &i64| *value > 0)
        .unwrap_or(DEFAULT_ANALYSIS_FRESH_HOURS);
    chrono::Duration::hours(hours)
}

pub async fn enqueue(db: &Db, repo: &str) -> Result<EnqueueOutcome> {
    let now = Utc::now();
    let mut tx = db.pool.begin().await?;
    // A process-wide count check is not a hard ceiling under concurrency.
    // This short transaction-scoped advisory lock serializes only enqueues.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(ENQUEUE_LOCK_ID)
        .execute(&mut *tx)
        .await?;

    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM repo_analysis_queue WHERE repo = $1")
            .bind(repo)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some("pending" | "in_progress") = status.as_deref() {
        tx.commit().await?;
        return Ok(EnqueueOutcome::AlreadyActive);
    }

    let fresh: bool = sqlx::query_scalar(
        "SELECT EXISTS( \
            SELECT 1 FROM repo_history history \
            WHERE history.repo = $1 \
              AND history.last_analyzed_at >= $2 \
              AND NOT EXISTS ( \
                  SELECT 1 FROM repo_author_stats author \
                  WHERE author.repo = history.repo \
                    AND (author.github_login IS NULL \
                         OR author.avatar_url LIKE 'https://www.gravatar.com/%') \
                    AND author.enrich_attempted_at IS NULL \
              ) \
         )",
    )
    .bind(repo)
    .bind(now - analysis_freshness())
    .fetch_one(&mut *tx)
    .await?;
    if fresh {
        sqlx::query("DELETE FROM repo_analysis_queue WHERE repo = $1")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(EnqueueOutcome::Fresh);
    }

    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM repo_analysis_queue \
         WHERE status IN ('pending', 'in_progress')",
    )
    .fetch_one(&mut *tx)
    .await?;
    if active >= max_pending_analyses() {
        tx.commit().await?;
        return Ok(EnqueueOutcome::AtCapacity);
    }

    sqlx::query(
        "INSERT INTO repo_analysis_queue (repo, status, enqueued_at) \
         VALUES ($1, 'pending', $2) \
         ON CONFLICT (repo) DO UPDATE SET \
            status = CASE WHEN repo_analysis_queue.status = 'in_progress' \
                          THEN 'in_progress' ELSE 'pending' END, \
            attempts = CASE WHEN repo_analysis_queue.status = 'dead' \
                            THEN 0 ELSE repo_analysis_queue.attempts END, \
            next_attempt_at = CASE WHEN repo_analysis_queue.status = 'in_progress' \
                                   THEN repo_analysis_queue.next_attempt_at ELSE NOW() END, \
            last_error = CASE WHEN repo_analysis_queue.status = 'dead' \
                              THEN NULL ELSE repo_analysis_queue.last_error END",
    )
    .bind(repo)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(EnqueueOutcome::Enqueued)
}

/// Enqueue up to `max_new` repositories, preserving input order.
///
/// Fresh and already-active jobs do not consume the limit. This lets discovery
/// surfaces offer a bounded batch to the workers without cloning synchronously
/// or letting one request monopolize the global queue.
pub async fn enqueue_many(db: &Db, repos: &[String], max_new: usize) -> Result<usize> {
    if max_new == 0 {
        return Ok(0);
    }
    let mut enqueued = 0usize;
    for repo in repos {
        match enqueue(db, repo).await? {
            EnqueueOutcome::Enqueued => {
                enqueued += 1;
                if enqueued >= max_new {
                    break;
                }
            }
            EnqueueOutcome::AtCapacity => break,
            EnqueueOutcome::AlreadyActive | EnqueueOutcome::Fresh => {}
        }
    }
    Ok(enqueued)
}

pub async fn reset_inflight_on_startup(db: &Db) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE repo_analysis_queue SET status = 'pending', worker_id = NULL, claimed_at = NULL \
         WHERE status = 'in_progress' \
           AND (claimed_at IS NULL OR claimed_at < NOW() - INTERVAL '2 minutes')",
    )
    .execute(&db.pool)
    .await?;
    Ok(res.rows_affected())
}

/// Revive jobs parked by older releases after a fixed number of transient
/// clone/process failures. New releases keep those failures pending with a
/// durable backoff, so this is a one-way startup repair.
pub async fn revive_retryable_on_startup(db: &Db) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE repo_analysis_queue SET status = 'pending', attempts = 0, \
            next_attempt_at = NOW(), worker_id = NULL, claimed_at = NULL \
         WHERE status = 'dead' \
           AND NOT EXISTS ( \
               SELECT 1 FROM repos \
               WHERE repos.repo = repo_analysis_queue.repo AND repos.missing = TRUE \
           )",
    )
    .execute(&db.pool)
    .await?;
    Ok(res.rows_affected())
}

pub fn spawn_pool(ctx: AnalysisCtx, count: usize) {
    let pool_id = format!("{}-{}", std::process::id(), Utc::now().timestamp_millis());
    for i in 0..count {
        let ctx = ctx.clone();
        let id = format!("ra-{pool_id}-{i}");
        tokio::spawn(async move {
            run_worker(id, ctx).await;
        });
    }
}

/// Run the disk-quota eviction pass every Nth completed job rather than
/// after each one. The pass sorts every clone row by a bytes×idle score
/// under disk pressure — cheap when well under quota, but pure overhead on
/// the worker's critical path when run after every job. Amortizing it
/// keeps the worker draining jobs; now that clones are blobless +
/// single-branch (#1/#2) disk pressure accumulates far more slowly, so a
/// periodic sweep is plenty. A partial overshoot between sweeps is bounded
/// by how many clones N jobs can add.
const EVICT_EVERY_N_JOBS: u64 = 16;

async fn run_worker(worker_id: String, ctx: AnalysisCtx) {
    tracing::info!(worker_id, "repo-analysis worker started");
    let idle = Duration::from_secs(5);
    let mut jobs_since_evict: u64 = 0;
    loop {
        let job = match claim_one(&ctx.db, &worker_id).await {
            Ok(Some(repo)) => repo,
            Ok(None) => {
                sleep(idle).await;
                continue;
            }
            Err(e) => {
                tracing::error!(error = %e, "claim failed");
                sleep(idle).await;
                continue;
            }
        };
        let heartbeat_stop = spawn_lease_heartbeat(ctx.db.clone(), job.clone(), worker_id.clone());
        let outcome = process(&job, &ctx).await;
        let _ = heartbeat_stop.send(true);
        match outcome {
            Ok(commits_applied) => {
                tracing::info!(repo = %job, commits_applied, "analysis run complete");
                if let Err(e) = complete(&ctx.db, &job).await {
                    tracing::warn!(repo = %job, error = %e, "queue complete failed");
                }
                // Eviction off the per-job critical path: only sweep every
                // EVICT_EVERY_N_JOBS completions.
                jobs_since_evict += 1;
                if jobs_since_evict >= EVICT_EVERY_N_JOBS {
                    jobs_since_evict = 0;
                    if let Err(e) = repo_stats::evict_to_quota(&ctx.db, &ctx.storage).await {
                        tracing::warn!(error = %e, "eviction pass failed");
                    }
                }
            }
            Err(e) => {
                let msg = compact_error(&e);
                tracing::warn!(repo = %job, error = %msg, "analysis run failed");
                if let Err(e2) = fail(&ctx.db, &job, &msg).await {
                    tracing::warn!(repo = %job, error = %e2, "queue fail failed");
                }
                sleep(Duration::from_secs(30)).await;
            }
        }
    }
}

fn spawn_lease_heartbeat(db: Db, repo: String, worker_id: String) -> watch::Sender<bool> {
    let (stop, mut stopped) = watch::channel(false);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = sleep(Duration::from_secs(30)) => {
                    if *stopped.borrow() {
                        break;
                    }
                    if let Err(error) = sqlx::query(
                        "UPDATE repo_analysis_queue SET claimed_at = NOW() \
                         WHERE repo = $1 AND status = 'in_progress' AND worker_id = $2",
                    )
                    .bind(&repo)
                    .bind(&worker_id)
                    .execute(&db.pool)
                    .await
                    {
                        tracing::warn!(%repo, %worker_id, %error, "analysis lease heartbeat failed");
                    }
                }
                changed = stopped.changed() => {
                    if changed.is_err() || *stopped.borrow() {
                        break;
                    }
                }
            }
        }
    });
    stop
}

async fn claim_one(db: &Db, worker_id: &str) -> Result<Option<String>> {
    let now = Utc::now();
    let row = sqlx::query(
        "UPDATE repo_analysis_queue \
         SET status = 'in_progress', worker_id = $1, claimed_at = $2 \
         WHERE repo = ( \
            SELECT repo FROM repo_analysis_queue \
            WHERE (status = 'pending' AND next_attempt_at <= NOW()) \
               OR (status = 'in_progress' AND claimed_at < NOW() - INTERVAL '2 minutes') \
            ORDER BY enqueued_at FOR UPDATE SKIP LOCKED LIMIT 1 \
         ) \
         RETURNING repo",
    )
    .bind(worker_id)
    .bind(now)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(|r| r.try_get::<String, _>("repo").unwrap_or_default()))
}

async fn complete(db: &Db, repo: &str) -> Result<()> {
    sqlx::query("DELETE FROM repo_analysis_queue WHERE repo = $1")
        .bind(repo)
        .execute(&db.pool)
        .await?;
    Ok(())
}

async fn fail(db: &Db, repo: &str, err: &str) -> Result<()> {
    sqlx::query(
        "UPDATE repo_analysis_queue SET \
            attempts = attempts + 1, \
            last_error = $1, \
            worker_id = NULL, \
            claimed_at = NULL, \
            status = 'pending', \
            next_attempt_at = NOW() + CASE \
                WHEN attempts <= 0 THEN INTERVAL '30 seconds' \
                WHEN attempts = 1 THEN INTERVAL '1 minute' \
                WHEN attempts = 2 THEN INTERVAL '2 minutes' \
                WHEN attempts = 3 THEN INTERVAL '4 minutes' \
                WHEN attempts = 4 THEN INTERVAL '8 minutes' \
                WHEN attempts = 5 THEN INTERVAL '16 minutes' \
                WHEN attempts = 6 THEN INTERVAL '32 minutes' \
                ELSE INTERVAL '1 hour' \
            END \
         WHERE repo = $2",
    )
    .bind(err)
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

fn compact_error(error: &anyhow::Error) -> String {
    format!("{error:#}")
        .chars()
        .filter(|character| !character.is_control())
        .take(1_000)
        .collect()
}

async fn process(repo: &str, ctx: &AnalysisCtx) -> Result<usize> {
    // Pull last_analyzed_sha to drive incremental walking.
    let last_sha: Option<String> =
        sqlx::query_scalar("SELECT last_analyzed_sha FROM repo_history WHERE repo = $1")
            .bind(repo)
            .fetch_optional(&ctx.db.pool)
            .await?
            .flatten();

    let handle = repo_history::open_or_clone(&ctx.storage, repo, last_sha.as_deref())
        .await
        .context("open_or_clone")?;
    // Update bookkeeping (clone_path + size + last_visited_at).
    let size = repo_history::clone_size_bytes(&handle.path);
    repo_stats::record_clone(&ctx.db, repo, &handle.path, size).await?;

    if Some(handle.head_sha.as_str()) == last_sha.as_deref() {
        // Commit aggregates and line counts are unchanged, but author
        // enrichment is deliberately retried. It is TTL/negative-cache
        // guarded, and a transient GitHub failure (or a deployment that
        // predates enrichment) must not strand every `github_login` as NULL
        // until the repository happens to receive another commit.
        if let Err(e) = enrich_author_logins(&ctx.db, &handle, repo, &ctx.github).await {
            tracing::warn!(repo, error = %e, "author-login enrichment retry failed");
        }
        return Ok(0);
    }

    let commits = repo_history::walk_new_commits(&handle, last_sha.as_deref()).await?;
    let n = commits.len();
    repo_stats::apply_commits(&ctx.db, repo, &commits).await?;

    // Two independent post-passes, overlapped with `tokio::join!`:
    //   * author enrichment is GitHub-API-bound (network RTT, rate-limit
    //     acquire waits) and writes `repo_author_stats`;
    //   * line counting is CPU-bound (`spawn_blocking` walk) and writes
    //     `repo_lines`.
    // They touch disjoint tables and only read the (immutable) clone, so
    // running them concurrently overlaps the network wait with the CPU work
    // instead of serializing them. Each logs + swallows its own error so a
    // failure in one never aborts the other (matching the prior best-effort
    // behavior).
    let enrich = async {
        if let Err(e) = enrich_author_logins(&ctx.db, &handle, repo, &ctx.github).await {
            tracing::warn!(repo, error = %e, "author-login enrichment failed");
        }
    };
    let line_counts = async {
        if let Err(e) = run_line_counts(&ctx.db, &handle, repo).await {
            tracing::warn!(repo, error = %e, "line counts failed");
        }
    };
    tokio::join!(enrich, line_counts);
    Ok(n)
}

async fn run_line_counts(db: &Db, handle: &RepoHandle, repo: &str) -> Result<()> {
    let counts = code_count::count_lines(&handle.path).await?;
    code_count::save(db, repo, &counts).await?;
    tracing::info!(repo, languages = counts.len(), "line counts updated");
    Ok(())
}

/// How long an author-enrichment *attempt* is trusted before we retry it.
/// An author whose commit-email never maps to a GitHub login (the common
/// gravatar-fallback case) would otherwise be re-queried against the
/// GitHub API on every analysis run forever. We stamp `enrich_attempted_at`
/// on every attempt (resolved or not) and skip rows touched within this
/// window — so unresolvable authors cost one API call per TTL, not one per
/// run. 30 days is long enough to make the burn negligible while still
/// letting a since-created GitHub account eventually resolve.
const AUTHOR_ENRICH_TTL: chrono::Duration = chrono::Duration::days(30);

/// For every author row whose `github_login` is null (or whose avatar is
/// still a gravatar fallback) AND that hasn't been attempted within
/// [`AUTHOR_ENRICH_TTL`], pick one of their commits and ask GitHub who the
/// human behind the email is. Updates `repo_author_stats` in place.
/// Failures are logged and skipped — we never block analysis on this.
async fn enrich_author_logins(
    db: &Db,
    handle: &RepoHandle,
    repo: &str,
    github: &Arc<GithubClient>,
) -> Result<()> {
    // Negative-cache cutoff: only consider rows not attempted recently.
    // (This also subsumes the "only enrich the new batch" optimization —
    // an already-attempted author from a prior batch is skipped until its
    // TTL lapses, so steady-state runs only touch genuinely-new authors.)
    let cutoff = Utc::now() - AUTHOR_ENRICH_TTL;
    let unresolved: Vec<String> = sqlx::query_scalar(
        "SELECT author_email FROM repo_author_stats \
         WHERE repo = $1 \
           AND (github_login IS NULL OR avatar_url LIKE 'https://www.gravatar.com/%') \
           AND (enrich_attempted_at IS NULL OR enrich_attempted_at < $2)",
    )
    .bind(repo)
    .bind(cutoff)
    .fetch_all(&db.pool)
    .await?;

    if unresolved.is_empty() {
        return Ok(());
    }
    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    let owner = parts[0].to_string();
    let name = parts.get(1).copied().unwrap_or("").to_string();

    // Parallelize at AUTHOR_ENRICH_CONCURRENCY. Each author requires
    // 1× `git log` (local, cheap) + exactly 1× GitHub API call
    // (`commit_author`, which already returns login+avatar — the old
    // redundant `/users/{login}` follow-up is gone). The GitHub side is
    // rate-limit-bucket-bound; concurrency here only reclaims TCP RTT. 6 is
    // a sweet spot — small enough that a chromium-class repo (~3000
    // unresolved authors) doesn't pile up acquire wakeups; large enough
    // that wall-clock drops by ~6x vs serial.
    use futures::stream::{self, StreamExt};
    const AUTHOR_ENRICH_CONCURRENCY: usize = 6;

    stream::iter(unresolved)
        .for_each_concurrent(AUTHOR_ENRICH_CONCURRENCY, |email| {
            let owner = owner.clone();
            let name = name.clone();
            let repo = repo.to_string();
            let db = db.clone();
            let github = github.clone();
            let handle_path = handle.path.clone();
            async move {
                if let Err(e) =
                    resolve_one_author(&db, &handle_path, &repo, &owner, &name, &email, &github)
                        .await
                {
                    tracing::warn!(email, error = %e, "author enrichment failed");
                }
            }
        })
        .await;
    Ok(())
}

async fn resolve_one_author(
    db: &Db,
    handle_path: &std::path::Path,
    repo: &str,
    owner: &str,
    name: &str,
    email: &str,
    github: &Arc<GithubClient>,
) -> Result<()> {
    let Some(sha) = sample_commit_for_email_at(handle_path, email).await? else {
        // No sampleable commit (shouldn't happen for a row that exists) —
        // don't stamp; let a future run retry once a commit is reachable.
        return Ok(());
    };
    match github.commit_author(owner, name, &sha).await {
        Ok(Some(info)) => {
            if let Some(login) = info.login.as_ref() {
                // `commit_author` already carries the login + avatar — no
                // separate `/users/{login}` round-trip (that endpoint only
                // returned the same login again; `User` has no display name).
                // We set author_name to the login when it was previously
                // unset, preserving the old behavior without the extra call.
                let avatar_url = info.avatar_url.clone().or_else(|| {
                    info.id
                        .map(|id| format!("https://avatars.githubusercontent.com/u/{id}?s=80&v=4"))
                });
                sqlx::query(
                    "UPDATE repo_author_stats SET \
                        github_login = $1, \
                        avatar_url = COALESCE($2, avatar_url), \
                        author_name = COALESCE($3, author_name), \
                        enrich_attempted_at = $4 \
                     WHERE repo = $5 AND author_email = $6",
                )
                .bind(login)
                .bind(&avatar_url)
                .bind(login)
                .bind(Utc::now())
                .bind(repo)
                .bind(email)
                .execute(&db.pool)
                .await?;
                return Ok(());
            }
            // GitHub knows the commit but not a login behind the email —
            // stamp the attempt so we don't re-query this gravatar author
            // every run.
            tracing::debug!(email, "commit author has no login; negative-caching");
            stamp_enrich_attempt(db, repo, email).await?;
        }
        Ok(None) => {
            // The commit/repo wasn't resolvable to an author block — stamp
            // so we don't retry on every run.
            tracing::debug!(email, "commit author resolution returned None");
            stamp_enrich_attempt(db, repo, email).await?;
        }
        Err(e) => {
            // A transient API error (rate limit, 5xx) — do NOT stamp, so the
            // next run retries. Only durable "no login" outcomes are cached.
            tracing::warn!(email, error = %e, "commit_author API failed");
        }
    }
    Ok(())
}

/// Record that we attempted (and failed to resolve) an author's GitHub
/// login, so the negative cache skips re-querying it until the TTL lapses.
async fn stamp_enrich_attempt(db: &Db, repo: &str, email: &str) -> Result<()> {
    sqlx::query(
        "UPDATE repo_author_stats SET enrich_attempted_at = $1 \
         WHERE repo = $2 AND author_email = $3",
    )
    .bind(Utc::now())
    .bind(repo)
    .bind(email)
    .execute(&db.pool)
    .await?;
    Ok(())
}

async fn sample_commit_for_email_at(
    repo_path: &std::path::Path,
    email: &str,
) -> Result<Option<String>> {
    use tokio::process::Command;
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args([
            "log",
            &format!("--author={email}"),
            "--format=%H",
            "-1",
            "HEAD",
        ])
        .output()
        .await?;
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!sha.is_empty()).then_some(sha))
}
