//! Repo-history analysis worker pool.
//!
//! Pulls jobs from `repo_analysis_queue`, opens or clones the repo via
//! `repo_history`, walks new commits, applies aggregates via
//! `repo_stats::apply_commits`, runs eviction. One worker per pool by
//! default — clones are disk-heavy and parallel I/O thrashes the cache;
//! analysis throughput is bounded by git CLI subprocesses, not by what
//! tokio can multiplex.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
pub const INTERACTIVE_PRIORITY: i64 = 1_000_000_000_000;
pub const CURRENT_ANALYSIS_REVISION: i32 = 3;

#[derive(Clone)]
pub struct AnalysisCtx {
    pub db: Db,
    pub storage: Arc<RepoStorage>,
    pub github: Arc<GithubClient>,
    pub gh_app: Option<Arc<crate::auth::GithubAppConfig>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Enqueued,
    AlreadyActive,
    Fresh,
    AtCapacity,
}

#[derive(Debug, Clone)]
struct AnalysisJob {
    repo: String,
    requested_by_user_id: Option<i64>,
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

/// **The** definition of "this repository's analysis is done and current",
/// as one reusable predicate. `$1` repo · `$2` freshness cutoff · `$3`
/// required analysis revision.
///
/// Three conditions, all of them about the *analysis*:
///   * `last_analyzed_at >= cutoff` — the walk ran recently enough;
///   * `analysis_revision >= $3` — it ran under the current algorithm;
///   * `last_analyzed_sha = head_sha` — it reached the head it observed
///     (`repo_stats::write_commits_at_head` writes both columns in the same
///     statement, so any other pairing means the run did not finish).
///
/// Deliberately absent: author login/avatar enrichment. That is
/// presentation-only metadata resolved best-effort against the GitHub API,
/// and some authors' commit emails can never resolve to a login. Gating
/// readiness on it made every such repository permanently "not analyzed":
/// the profile poll re-enqueued it every few seconds, each run applied zero
/// commits and completed, and the queue never drained. Enrichment now
/// converges on its own in [`sweep_author_enrichment`] and can neither
/// re-open an analysis nor hold one open.
const ANALYSIS_IS_CURRENT_SQL: &str = "SELECT EXISTS( \
     SELECT 1 FROM repo_history history \
     WHERE history.repo = $1 \
       AND history.last_analyzed_at >= $2 \
       AND history.analysis_revision >= $3 \
       AND history.last_analyzed_sha IS NOT NULL \
       AND history.head_sha = history.last_analyzed_sha \
 )";

pub async fn enqueue(db: &Db, repo: &str) -> Result<EnqueueOutcome> {
    enqueue_prioritized(db, repo, 0, None).await
}

/// Raise an existing queue row's priority and requester without disturbing
/// a claimed job's lease or a pending job's place in line.
async fn bump_active_job(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    repo: &str,
    priority: i64,
    requested_by_user_id: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "UPDATE repo_analysis_queue SET \
            priority = GREATEST(priority, $2), \
            requested_by_user_id = COALESCE($3, requested_by_user_id), \
            next_attempt_at = CASE WHEN status = 'pending' THEN NOW() ELSE next_attempt_at END, \
            updated_at = NOW() \
         WHERE repo = $1",
    )
    .bind(repo)
    .bind(priority.max(0))
    .bind(requested_by_user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Read-only form of [`ANALYSIS_IS_CURRENT_SQL`] for callers that need the
/// same answer outside an enqueue transaction (tests, readiness surfaces).
pub async fn analysis_is_current(db: &Db, repo: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(ANALYSIS_IS_CURRENT_SQL)
        .bind(repo)
        .bind(Utc::now() - analysis_freshness())
        .bind(CURRENT_ANALYSIS_REVISION)
        .fetch_one(&db.pool)
        .await?)
}

/// Queue repository health work with a durable priority. An authenticated
/// report view uses [`INTERACTIVE_PRIORITY`] and carries only the requesting
/// user's database id; OAuth plaintext is never written to a queue row.
pub async fn enqueue_prioritized(
    db: &Db,
    repo: &str,
    priority: i64,
    requested_by_user_id: Option<i64>,
) -> Result<EnqueueOutcome> {
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
    // A worker holds the lease on an in-progress job; never touch its row
    // beyond the priority bump, and never judge its freshness — the run it
    // is doing right now is what makes the row current.
    let in_progress = status.as_deref() == Some("in_progress");
    if in_progress {
        bump_active_job(&mut tx, repo, priority, requested_by_user_id).await?;
        tx.commit().await?;
        return Ok(EnqueueOutcome::AlreadyActive);
    }

    // Freshness is checked *before* honoring an existing `pending` row, not
    // after. A queued job for a repository whose analysis is already done
    // and current is a no-op run: the worker would clone, find HEAD
    // unchanged, apply zero commits and delete the row. Recognizing it here
    // drains that row immediately instead, which is what stops a polling
    // profile page from re-arming the same no-op every few seconds.
    let fresh: bool = sqlx::query_scalar(ANALYSIS_IS_CURRENT_SQL)
        .bind(repo)
        .bind(now - analysis_freshness())
        .bind(CURRENT_ANALYSIS_REVISION)
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

    if status.as_deref() == Some("pending") {
        bump_active_job(&mut tx, repo, priority, requested_by_user_id).await?;
        tx.commit().await?;
        return Ok(EnqueueOutcome::AlreadyActive);
    }

    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM repo_analysis_queue \
         WHERE status IN ('pending', 'in_progress')",
    )
    .fetch_one(&mut *tx)
    .await?;
    if active >= max_pending_analyses() && priority < INTERACTIVE_PRIORITY {
        tx.commit().await?;
        return Ok(EnqueueOutcome::AtCapacity);
    }

    sqlx::query(
        "INSERT INTO repo_analysis_queue \
            (repo, status, phase, priority, requested_by_user_id, enqueued_at, updated_at) \
         VALUES ($1, 'pending', 'queued', $2, $3, $4, $4) \
         ON CONFLICT (repo) DO UPDATE SET \
            status = CASE WHEN repo_analysis_queue.status = 'in_progress' \
                          THEN 'in_progress' ELSE 'pending' END, \
            phase = CASE WHEN repo_analysis_queue.status = 'in_progress' \
                         THEN repo_analysis_queue.phase ELSE 'queued' END, \
            priority = GREATEST(repo_analysis_queue.priority, EXCLUDED.priority), \
            requested_by_user_id = COALESCE(EXCLUDED.requested_by_user_id, repo_analysis_queue.requested_by_user_id), \
            attempts = CASE WHEN repo_analysis_queue.status = 'dead' \
                            THEN 0 ELSE repo_analysis_queue.attempts END, \
            next_attempt_at = CASE WHEN repo_analysis_queue.status = 'in_progress' \
                                   THEN repo_analysis_queue.next_attempt_at ELSE NOW() END, \
            total_units = CASE WHEN repo_analysis_queue.status = 'in_progress' \
                               THEN repo_analysis_queue.total_units ELSE NULL END, \
            completed_units = CASE WHEN repo_analysis_queue.status = 'in_progress' \
                                   THEN repo_analysis_queue.completed_units ELSE 0 END, \
            updated_at = NOW(), \
            last_error = CASE WHEN repo_analysis_queue.status = 'dead' \
                              THEN NULL ELSE repo_analysis_queue.last_error END",
    )
    .bind(repo)
    .bind(priority.max(0))
    .bind(requested_by_user_id)
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
        "UPDATE repo_analysis_queue SET status = 'pending', phase = 'queued', \
         worker_id = NULL, claimed_at = NULL, started_at = NULL, updated_at = NOW() \
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
            phase = 'queued', next_attempt_at = NOW(), worker_id = NULL, \
            claimed_at = NULL, started_at = NULL, updated_at = NOW() \
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
    // Author enrichment rides along with the analysis pool but never on its
    // critical path: presentation-only metadata converges here, on its own
    // bounded schedule, so it can neither gate analysis readiness nor keep a
    // job in the queue.
    spawn_author_enrichment_sweep(ctx.clone());
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
        tracing::info!(repo = %job.repo, interactive = job.requested_by_user_id.is_some(), "analysis run started");
        let heartbeat_stop =
            spawn_lease_heartbeat(ctx.db.clone(), job.repo.clone(), worker_id.clone());
        let outcome = process(&job, &ctx).await;
        let _ = heartbeat_stop.send(true);
        match outcome {
            Ok(commits_applied) => {
                tracing::info!(repo = %job.repo, commits_applied, "analysis run complete");
                if let Err(e) = complete(&ctx.db, &job.repo).await {
                    tracing::warn!(repo = %job.repo, error = %e, "queue complete failed");
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
                tracing::warn!(repo = %job.repo, error = %msg, "analysis run failed");
                if let Err(e2) = fail(&ctx.db, &job.repo, &msg).await {
                    tracing::warn!(repo = %job.repo, error = %e2, "queue fail failed");
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
                        "UPDATE repo_analysis_queue SET claimed_at = NOW(), updated_at = NOW() \
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

async fn claim_one(db: &Db, worker_id: &str) -> Result<Option<AnalysisJob>> {
    let now = Utc::now();
    let row = sqlx::query(
        "UPDATE repo_analysis_queue \
         SET status = 'in_progress', phase = 'cloning', worker_id = $1, \
             claimed_at = $2, started_at = $2, updated_at = $2, \
             total_units = NULL, completed_units = 0 \
         WHERE repo = ( \
            SELECT repo FROM repo_analysis_queue \
            WHERE (status = 'pending' AND next_attempt_at <= NOW()) \
               OR (status = 'in_progress' AND claimed_at < NOW() - INTERVAL '2 minutes') \
            ORDER BY priority DESC, enqueued_at FOR UPDATE SKIP LOCKED LIMIT 1 \
         ) \
         RETURNING repo, requested_by_user_id",
    )
    .bind(worker_id)
    .bind(now)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(|row| AnalysisJob {
        repo: row.try_get::<String, _>("repo").unwrap_or_default(),
        requested_by_user_id: row.try_get("requested_by_user_id").ok().flatten(),
    }))
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
            phase = 'retrying', \
            updated_at = NOW(), \
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

async fn process(job: &AnalysisJob, ctx: &AnalysisCtx) -> Result<usize> {
    let repo = job.repo.as_str();
    let started = Instant::now();
    // A queue row is not proof that a repository is public: it may be stale,
    // manually inserted, or have been requested with an OAuth token that can
    // see private data. Require a recent metadata response through the
    // public-only GitHub decoder before any clone is opened. Private, deleted,
    // and renamed repositories all become the same non-readable tombstone.
    let cache = crate::cache::Cache::new(ctx.db.clone());
    if !cache
        .repo_metadata_fresh_within(repo, chrono::Duration::hours(1))
        .await?
    {
        let (owner, name) = repo
            .split_once('/')
            .context("analysis queue contained an invalid repository slug")?;
        let github = user_scoped_github(job, ctx).await;
        match github.repo_metadata(owner, name).await? {
            Some(metadata) => {
                cache
                    .put_repo_metadata(
                        repo,
                        metadata.id,
                        metadata.stargazers_count,
                        metadata.forks_count,
                        metadata.created_at,
                    )
                    .await?;
            }
            None => {
                cache.mark_repo_missing(repo).await?;
                tracing::info!(repo, "repository analysis skipped because it is not public");
                return Ok(0);
            }
        }
    }
    // Pull the cursor and algorithm revision together. Old bounded analyses
    // advanced their cursor to HEAD after sampling only a small recent window;
    // those rows must be atomically rebuilt rather than incremented.
    let (last_sha, was_truncated, analysis_revision): (Option<String>, bool, i32) = sqlx::query_as(
        "SELECT last_analyzed_sha, analysis_truncated, analysis_revision \
             FROM repo_history WHERE repo = $1",
    )
    .bind(repo)
    .fetch_optional(&ctx.db.pool)
    .await?
    .unwrap_or((None, false, 0));

    let handle = repo_history::open_or_clone(&ctx.storage, repo, last_sha.as_deref())
        .await
        .context("open_or_clone")?;
    // Update bookkeeping (clone_path + size + last_visited_at).
    let size = repo_history::clone_size_bytes(&handle.path);
    repo_stats::record_clone(&ctx.db, repo, &handle.path, size).await?;

    // An empty GitHub repository is a successful zero-result analysis, not a
    // transient git failure. Persist the empty aggregate so request paths can
    // render immediately and the durable queue does not retry forever.
    if handle.is_empty() {
        update_work_progress(&ctx.db, repo, "saving_history", Some(0), 0).await?;
        repo_stats::replace_commits_at_head(&ctx.db, repo, &[], &handle.head_sha, 0).await?;
        code_count::save(&ctx.db, repo, &[]).await?;
        repo_stats::record_analysis_details(
            &ctx.db,
            repo,
            i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
            0,
            false,
        )
        .await?;
        return Ok(0);
    }

    if Some(handle.head_sha.as_str()) == last_sha.as_deref()
        && analysis_revision >= CURRENT_ANALYSIS_REVISION
    {
        // Nothing to walk: commit aggregates and line counts are already at
        // this head under the current revision. Return immediately so the
        // caller dequeues the job. Author enrichment is explicitly NOT run
        // here — it is presentation-only metadata owned by
        // [`sweep_author_enrichment`], and doing GitHub work on this path is
        // what let a repeatedly re-enqueued no-op job burn budget forever.
        return Ok(0);
    }

    let limit = repo_history::analysis_commit_limit();
    let mut replace = analysis_revision < CURRENT_ANALYSIS_REVISION
        || was_truncated
        || last_sha.as_deref() == Some(repo_history::EMPTY_REPOSITORY_HEAD);
    let mut plan = repo_history::plan_recent_commits(
        &handle,
        if replace { None } else { last_sha.as_deref() },
        limit,
    )
    .await?;
    // More than one window landed since our cursor. Rebuild from HEAD so the
    // stored window is coherent instead of appending an arbitrary slice.
    if plan.truncated && !replace {
        replace = true;
        plan = repo_history::plan_recent_commits(&handle, None, limit).await?;
    }
    let reachable_commits = repo_history::reachable_commit_count(&handle).await?;
    update_work_progress(&ctx.db, repo, "scanning_history", Some(plan.shas.len()), 0).await?;
    let mut commits = Vec::with_capacity(plan.shas.len());
    for batch in plan.shas.chunks(repo_history::METADATA_BATCH_COMMITS) {
        commits.extend(repo_history::walk_commit_metadata_batch(&handle, batch).await?);
        update_work_progress(
            &ctx.db,
            repo,
            "scanning_history",
            Some(plan.shas.len()),
            commits.len(),
        )
        .await?;
    }
    let todo_start = plan
        .shas
        .len()
        .saturating_sub(repo_history::TODO_PATCH_COMMIT_LIMIT);
    let todo_shas = &plan.shas[todo_start..];
    let mut todo_by_sha = HashMap::with_capacity(todo_shas.len());
    if !todo_shas.is_empty() {
        update_work_progress(&ctx.db, repo, "scanning_todos", Some(todo_shas.len()), 0).await?;
        let mut todo_done = 0;
        for batch in todo_shas.chunks(repo_history::LOG_BATCH_COMMITS) {
            let patch_commits = repo_history::walk_commit_batch(&handle, batch).await?;
            todo_done += patch_commits.len();
            todo_by_sha.extend(
                patch_commits
                    .into_iter()
                    .map(|commit| (commit.sha, (commit.todo_added, commit.todo_removed))),
            );
            update_work_progress(
                &ctx.db,
                repo,
                "scanning_todos",
                Some(todo_shas.len()),
                todo_done,
            )
            .await?;
        }
    }
    for commit in &mut commits {
        if let Some((added, removed)) = todo_by_sha.get(&commit.sha) {
            commit.todo_added = *added;
            commit.todo_removed = *removed;
        }
    }
    let n = commits.len();
    update_work_progress(&ctx.db, repo, "saving_history", Some(n), n).await?;
    if replace {
        repo_stats::replace_commits_at_head(
            &ctx.db,
            repo,
            &commits,
            &handle.head_sha,
            reachable_commits,
        )
        .await?;
    } else {
        repo_stats::apply_commits_at_head_with_total(
            &ctx.db,
            repo,
            &commits,
            &handle.head_sha,
            reachable_commits,
        )
        .await?;
    }

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
    update_work_progress(&ctx.db, repo, "finishing", Some(n), n).await?;
    let github = user_scoped_github(job, ctx).await;
    let enrich = run_author_enrichment(&ctx.db, Some(handle.path.as_path()), repo, &github);
    let line_counts = async {
        if let Err(e) = run_line_counts(&ctx.db, &handle, repo).await {
            tracing::warn!(repo, error = %e, "line counts failed");
        }
    };
    tokio::join!(enrich, line_counts);
    repo_stats::record_analysis_details(
        &ctx.db,
        repo,
        i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
        n,
        plan.truncated,
    )
    .await?;
    Ok(n)
}

async fn user_scoped_github(job: &AnalysisJob, ctx: &AnalysisCtx) -> Arc<GithubClient> {
    let Some(user_id) = job.requested_by_user_id else {
        return ctx.github.clone();
    };
    let Some(config) = ctx.gh_app.as_deref() else {
        return ctx.github.clone();
    };
    match crate::auth::user_access_token(&ctx.db, config, user_id).await {
        Ok(Some(token)) => match ctx.github.for_user_token(&token) {
            Ok(client) => Arc::new(client),
            Err(error) => {
                tracing::warn!(user_id, %error, "user OAuth client construction failed");
                ctx.github.clone()
            }
        },
        Ok(None) => ctx.github.clone(),
        Err(error) => {
            tracing::warn!(user_id, %error, "user OAuth token unavailable");
            ctx.github.clone()
        }
    }
}

async fn update_work_progress(
    db: &Db,
    repo: &str,
    phase: &str,
    total_units: Option<usize>,
    completed_units: usize,
) -> Result<()> {
    sqlx::query(
        "UPDATE repo_analysis_queue SET phase = $1, total_units = $2, \
         completed_units = $3, updated_at = NOW() \
         WHERE repo = $4 AND status = 'in_progress'",
    )
    .bind(phase)
    .bind(total_units.map(|value| i64::try_from(value).unwrap_or(i64::MAX)))
    .bind(i64::try_from(completed_units).unwrap_or(i64::MAX))
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

async fn run_line_counts(db: &Db, handle: &RepoHandle, repo: &str) -> Result<()> {
    let (file_census, tree_files) = code_count::language_file_census(&handle.path).await?;
    if tree_files > code_count::exact_line_count_max_files() {
        code_count::save(db, repo, &file_census).await?;
        tracing::info!(
            repo,
            tree_files,
            languages = file_census.len(),
            "large repository language file census updated"
        );
        return Ok(());
    }

    match tokio::time::timeout(
        code_count::exact_line_count_timeout(),
        code_count::count_lines(&handle.path),
    )
    .await
    {
        Ok(Ok(counts)) => {
            code_count::save(db, repo, &counts).await?;
            tracing::info!(repo, languages = counts.len(), "line counts updated");
        }
        Ok(Err(error)) => {
            tracing::warn!(repo, %error, "exact line count failed; using file census");
            code_count::save(db, repo, &file_census).await?;
        }
        Err(_) => {
            tracing::warn!(
                repo,
                tree_files,
                "exact line count timed out; using file census"
            );
            code_count::save(db, repo, &file_census).await?;
        }
    }
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
const AUTHOR_ENRICH_MAX_PER_RUN: i64 = 24;
const AUTHOR_ENRICH_TIMEOUT: Duration = Duration::from_secs(15);

/// Deferral stamp for rows a pass *selected* but could not genuinely
/// attempt — its wall-clock budget lapsed, the shared GitHub budget was
/// empty, or the bare clone needed to sample a commit is not on this
/// replica's disk.
///
/// Such rows are still stamped, because the stamp is the only thing that
/// makes a pass terminate: an unstamped row is re-selected by the very next
/// pass, which is precisely the tight loop this design replaces. They are
/// stamped [`AUTHOR_ENRICH_DEFER`] *before* the TTL edge instead of at
/// `now`, so a genuinely transient failure retries in hours rather than a
/// month while still being skipped for long enough to be economical.
const AUTHOR_ENRICH_DEFER: chrono::Duration = chrono::Duration::hours(6);

/// Parallelism for one enrichment pass. Each author costs 1× `git log`
/// (local, cheap) + exactly 1× GitHub API call (`commit_author`, which
/// already returns login+avatar). The GitHub side is rate-limit-bucket
/// bound; concurrency here only reclaims TCP RTT. 6 is a sweet spot —
/// small enough that a chromium-class repo (~3000 unresolved authors)
/// doesn't pile up acquire wakeups, large enough that wall-clock drops
/// ~6× versus serial.
const AUTHOR_ENRICH_CONCURRENCY: usize = 6;

/// Author identity is presentation-only metadata. A depleted shared GitHub
/// budget must never keep a durable repository-analysis worker occupied
/// until the hourly reset, so every best-effort pass gets a small fixed
/// wall-clock budget. Never returns an error: analysis completion does not
/// depend on this.
async fn run_author_enrichment(
    db: &Db,
    repo_path: Option<&std::path::Path>,
    repo: &str,
    github: &Arc<GithubClient>,
) {
    let deadline = Instant::now() + AUTHOR_ENRICH_TIMEOUT;
    if let Err(error) = enrich_author_batch(db, repo_path, repo, github, deadline).await {
        tracing::warn!(repo, %error, "author-login enrichment failed");
    }
}

/// One bounded author-enrichment pass over a single repository.
///
/// Selects up to [`AUTHOR_ENRICH_MAX_PER_RUN`] rows whose `github_login` is
/// null (or whose avatar is still a gravatar fallback) and whose last
/// attempt has lapsed, then resolves each against the GitHub API using a
/// commit sampled from the local bare clone.
///
/// **Convergence contract:** every row this pass selects leaves it with a
/// non-null, in-TTL `enrich_attempted_at`. Resolved rows and durable misses
/// are stamped `now` by [`resolve_one_author`]; everything else — deadline
/// lapsed, GitHub budget empty, clone unavailable, transient API error — is
/// swept up by the closing statement at the [`AUTHOR_ENRICH_DEFER`] stamp.
/// That statement is unconditional and outside any cancellation, so a pass
/// can never leave the same row eligible for the very next pass.
///
/// Returns the number of rows attempted.
async fn enrich_author_batch(
    db: &Db,
    repo_path: Option<&std::path::Path>,
    repo: &str,
    github: &Arc<GithubClient>,
    deadline: Instant,
) -> Result<usize> {
    // Negative-cache cutoff: only consider rows not attempted recently.
    // (This also subsumes the "only enrich the new batch" optimization —
    // an already-attempted author from a prior batch is skipped until its
    // TTL lapses, so steady-state runs only touch genuinely-new authors.)
    let now = Utc::now();
    let cutoff = now - AUTHOR_ENRICH_TTL;
    let selected: Vec<String> = sqlx::query_scalar(
        "SELECT author_email FROM repo_author_stats \
         WHERE repo = $1 \
           AND (github_login IS NULL OR avatar_url LIKE 'https://www.gravatar.com/%') \
           AND (enrich_attempted_at IS NULL OR enrich_attempted_at < $2) \
         ORDER BY commits DESC, author_email \
         LIMIT $3",
    )
    .bind(repo)
    .bind(cutoff)
    .bind(AUTHOR_ENRICH_MAX_PER_RUN)
    .fetch_all(&db.pool)
    .await?;

    if selected.is_empty() {
        return Ok(0);
    }

    // A clone this replica does not hold cannot yield a commit to sample.
    // Skip straight to the deferral stamp rather than cloning a repository
    // for presentation metadata.
    let usable_clone = repo_path.filter(|path| path.is_dir());
    if let Some(path) = usable_clone {
        let (owner, name) = match repo.split_once('/') {
            Some((owner, name)) => (owner.to_string(), name.to_string()),
            None => (repo.to_string(), String::new()),
        };
        use futures::stream::{self, StreamExt};
        stream::iter(selected.clone())
            .for_each_concurrent(AUTHOR_ENRICH_CONCURRENCY, |email| {
                let owner = owner.clone();
                let name = name.clone();
                let repo = repo.to_string();
                let db = db.clone();
                let github = github.clone();
                let path = path.to_path_buf();
                async move {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() || !github.has_budget().await {
                        return;
                    }
                    // Per-author timeout as well as the pass deadline: a
                    // single rate-limit `acquire` can otherwise sleep past
                    // the budget and starve every remaining row.
                    match tokio::time::timeout(
                        remaining,
                        resolve_one_author(&db, &path, &repo, &owner, &name, &email, &github),
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            tracing::warn!(email, %error, "author enrichment failed")
                        }
                        Err(_) => {}
                    }
                }
            })
            .await;
    }

    // Terminating stamp. Only touches rows this pass left un-stamped: a
    // resolved row (or a durable miss) already carries `now`, which is
    // neither NULL nor older than the cutoff.
    let deferred = now - AUTHOR_ENRICH_TTL + AUTHOR_ENRICH_DEFER;
    sqlx::query(
        "UPDATE repo_author_stats SET enrich_attempted_at = $1 \
         WHERE repo = $2 AND author_email = ANY($3) \
           AND (enrich_attempted_at IS NULL OR enrich_attempted_at < $4)",
    )
    .bind(deferred)
    .bind(repo)
    .bind(&selected)
    .bind(cutoff)
    .execute(&db.pool)
    .await?;
    Ok(selected.len())
}

/// Cadence for the background author-enrichment sweep: one pass at startup,
/// then every [`AUTHOR_ENRICH_SWEEP_INTERVAL`].
const AUTHOR_ENRICH_SWEEP_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// Wall-clock budget for one sweep pass across all of its repositories.
const AUTHOR_ENRICH_SWEEP_BUDGET: Duration = Duration::from_secs(90);
/// Repositories one sweep pass may touch.
const AUTHOR_ENRICH_SWEEP_REPOS: i64 = 8;

/// What one [`sweep_author_enrichment`] pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AuthorEnrichmentSweep {
    pub repos: usize,
    pub rows_attempted: usize,
}

/// One bounded pass of the background author-enrichment sweep.
///
/// Author identity is presentation-only metadata that must converge on its
/// own schedule: it can neither gate analysis readiness nor cause a
/// repository to be re-analyzed. This sweep is the only thing that retries
/// it, and it is bounded three ways — repositories per pass, rows per
/// repository, and a shared wall-clock deadline. Every row it selects is
/// stamped (see [`enrich_author_batch`]), so successive passes strictly
/// shrink the backlog instead of re-picking the same unresolvable authors.
pub async fn sweep_author_enrichment(ctx: &AnalysisCtx) -> Result<AuthorEnrichmentSweep> {
    sweep_author_enrichment_until(ctx, Instant::now() + AUTHOR_ENRICH_SWEEP_BUDGET).await
}

/// [`sweep_author_enrichment`] with an explicit deadline (tests).
pub async fn sweep_author_enrichment_until(
    ctx: &AnalysisCtx,
    deadline: Instant,
) -> Result<AuthorEnrichmentSweep> {
    let cutoff = Utc::now() - AUTHOR_ENRICH_TTL;
    let candidates: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT history.repo, history.clone_path FROM repo_history history \
         WHERE EXISTS ( \
             SELECT 1 FROM repo_author_stats author \
             WHERE author.repo = history.repo \
               AND (author.github_login IS NULL \
                    OR author.avatar_url LIKE 'https://www.gravatar.com/%') \
               AND (author.enrich_attempted_at IS NULL \
                    OR author.enrich_attempted_at < $1) \
         ) \
         ORDER BY history.last_analyzed_at DESC NULLS LAST, history.repo \
         LIMIT $2",
    )
    .bind(cutoff)
    .bind(AUTHOR_ENRICH_SWEEP_REPOS)
    .fetch_all(&ctx.db.pool)
    .await?;

    let mut sweep = AuthorEnrichmentSweep::default();
    for (repo, clone_path) in candidates {
        let path = clone_path.map(std::path::PathBuf::from);
        // Note the deadline is NOT checked before the call: a lapsed
        // deadline still runs the batch, which skips every API call and
        // takes the deferral stamp. Bailing out early instead would leave
        // the rows unstamped and re-selected by the next pass.
        match enrich_author_batch(&ctx.db, path.as_deref(), &repo, &ctx.github, deadline).await {
            Ok(0) => {}
            Ok(rows) => {
                sweep.repos += 1;
                sweep.rows_attempted += rows;
            }
            Err(error) => tracing::warn!(%repo, %error, "author enrichment sweep pass failed"),
        }
    }
    Ok(sweep)
}

/// Spawn the periodic author-enrichment sweep (startup + every
/// [`AUTHOR_ENRICH_SWEEP_INTERVAL`]). Safe in every replica: passes are
/// idempotent and each stamps the rows it selected, so overlapping sweeps
/// converge rather than fight.
pub fn spawn_author_enrichment_sweep(ctx: AnalysisCtx) {
    tokio::spawn(async move {
        loop {
            match sweep_author_enrichment(&ctx).await {
                Ok(sweep) if sweep.rows_attempted == 0 => {}
                Ok(sweep) => tracing::info!(
                    repos = sweep.repos,
                    rows = sweep.rows_attempted,
                    "author-enrichment sweep pass complete"
                ),
                Err(error) => tracing::warn!(%error, "author-enrichment sweep failed"),
            }
            sleep(AUTHOR_ENRICH_SWEEP_INTERVAL).await;
        }
    });
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
        // No sampleable commit (shouldn't happen for a row that exists).
        // Leave it to the caller's deferral stamp so a later pass retries
        // once a commit is reachable, without re-picking it immediately.
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
            // A transient API error (rate limit, 5xx). Don't stamp `now` —
            // the caller's terminating statement gives this row the shorter
            // [`AUTHOR_ENRICH_DEFER`] stamp instead, so it retries in hours
            // rather than being negative-cached for the full TTL, and still
            // cannot be re-picked by the immediately following pass.
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
