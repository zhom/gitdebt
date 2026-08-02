//! Repo-history analysis worker pool.
//!
//! Pulls jobs from `repo_analysis_queue`, opens or clones the repo via
//! `repo_history`, walks new commits, applies aggregates via
//! `repo_stats::apply_commits`, runs eviction.
//!
//! Two scheduling rules shape the pool, both of them consequences of the same
//! fact: an analysis covers a repository's complete history, so a single job
//! can legitimately run for a very long time.
//!
//!   * **Capacity is reserved, not merely ordered.** Part of the pool claims
//!     only rows at or above [`VISITOR_PRIORITY_FLOOR`], and no more than
//!     [`catalog_concurrency_cap`] rows below it may run at once — otherwise a
//!     few hundred queued catalog repositories hold every slot and a visitor's
//!     top-priority row waits behind all of them regardless of its priority.
//!   * **Silence is what is bounded.** A run is killed when it stops making
//!     progress, not for taking long: killing a working job on a flat wall
//!     clock guarantees a large repository never finishes, because each
//!     attempt dies at the same point and the next starts over. There is a
//!     second, far wider ceiling at [`ABSOLUTE_JOB_CEILING`], set several
//!     stall windows past the worst measured extrapolation so that it can only
//!     ever catch a run that is beating steadily while making no real headway
//!     — a loop, not a large repository.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Rows this replica lets accumulate in `pending` + `in_progress` before it
/// starts answering `AtCapacity`.
///
/// Sized as a drain time, not as a memory bound: [`ANALYSIS_WORKERS`] run at
/// once and a catalog-sized repository takes a couple of minutes on this host
/// (apache/kafka's 17,985 commits measured 93 s end to end), so 200 rows is
/// roughly two hours of queue — the longest backlog still worth reporting to a
/// visitor as an ETA. The previous 500 was three times that, and a queue that
/// deep is indistinguishable from a stuck one.
const MAX_PENDING_ANALYSES: i64 = 200;
/// How far above the ordinary ceiling interactive work may push the queue.
const INTERACTIVE_CAPACITY_FACTOR: i64 = 4;
/// How long a completed analysis is treated as current before a view may
/// re-enqueue it.
///
/// A full-history walk is the most expensive thing this host does, and a day
/// of new commits moves no repo-health chart visibly, so re-analysis is a
/// daily job rather than an hourly one. It also sets the catalog's steady-state
/// load: ~117 curated repositories expiring once a day is ~5 re-analyses an
/// hour, which the single catalog slot (see [`catalog_concurrency_cap`])
/// absorbs while leaving the pool to visitors.
const ANALYSIS_FRESH_HOURS: i64 = 24;
const ENQUEUE_LOCK_ID: i64 = 6_794_738_132_977;
/// Priority for work a person is waiting on right now: exactly one repository
/// per request, from a surface that is rendering its report.
pub const INTERACTIVE_PRIORITY: i64 = 1_000_000_000_000;
/// Priority for bulk warm-up (sign-in discovery, profile aggregate builds).
/// Above popularity-driven catalog work, below anything with a live viewer —
/// warming a login's whole starred list must never outrank the report the
/// next visitor is actually watching.
pub const WARM_PRIORITY: i64 = 1_000_000;
/// The band boundary between "a person is waiting on this" and "backfill".
///
/// Sorting alone is not reservation: when every worker slot holds a
/// twenty-minute priority-0 catalog job, a visitor's row sorts first and still
/// waits for a slot to free. [`reserved_visitor_workers`] keeps part of the
/// pool for rows at or above this floor, and [`catalog_concurrency_cap`]
/// bounds how much of the rest the band below it may hold at once.
///
/// Deliberately far below [`WARM_PRIORITY`] and well above 0: every
/// visitor-driven enqueue must land above it. A surface whose priority is a
/// raw popularity counter (a view count, which is 0 for a repository nobody
/// has opened yet) must add this as a *base*, or its cold repositories land in
/// the catalog band and queue behind the whole backfill.
pub const VISITOR_PRIORITY_FLOOR: i64 = 1_000;
/// Every visitor-driven band sits above the floor, and the catalog band (0)
/// below it. If that ever stopped holding, reserving workers for visitors
/// would silently reserve them for the backfill instead.
const _: () = assert!(
    INTERACTIVE_PRIORITY > VISITOR_PRIORITY_FLOOR
        && WARM_PRIORITY > VISITOR_PRIORITY_FLOOR
        && VISITOR_PRIORITY_FLOOR > 0
);
// v4 adds per-author/day buckets for truthful profile commit streaks. v6 is
// the first revision whose aggregates describe a repository's complete
// history rather than a sampled window of its newest commits, so every row
// written by an earlier one is rebuilt from the first commit instead of being
// appended to — the two are not the same measurement.
pub const CURRENT_ANALYSIS_REVISION: i32 = 6;

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
    /// Consecutive runs killed for making no progress, recovered from the
    /// queue row's `last_error`. Decides when the row is parked instead of
    /// retried.
    prior_stalls: u32,
    /// The most units any previous attempt on this repository completed before
    /// it was killed. A run that beats it is progress *across* attempts — a
    /// large repository grinding forward, not a stuck one — and resets
    /// `prior_stalls`.
    best_units: u64,
}

pub(crate) fn analysis_freshness() -> chrono::Duration {
    chrono::Duration::hours(ANALYSIS_FRESH_HOURS)
}

/// The predicate half of [`ANALYSIS_IS_CURRENT_SQL`], over a `repo_history`
/// row aliased `history`, with `$2` freshness cutoff and `$3` required
/// revision. A macro so [`enqueue_backfill`]'s candidate scan correlates the
/// *same* three conditions to its own slug list instead of carrying a second
/// copy that can drift from this one.
macro_rules! analysis_is_current_predicate {
    () => {
        "history.last_analyzed_at >= $2 \
         AND history.analysis_revision >= $3 \
         AND history.last_analyzed_sha IS NOT NULL \
         AND history.head_sha = history.last_analyzed_sha"
    };
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
const ANALYSIS_IS_CURRENT_SQL: &str = concat!(
    "SELECT EXISTS( SELECT 1 FROM repo_history history WHERE history.repo = $1 AND ",
    analysis_is_current_predicate!(),
    " )"
);

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
    // Interactive work jumps the ordinary ceiling — someone is watching — but
    // not an unlimited one. Without the outer bound the ceiling constrained
    // anonymous traffic only, and signed-in bursts could grow the queue past
    // any drain time worth reporting as an ETA.
    let ceiling = if priority >= INTERACTIVE_PRIORITY {
        MAX_PENDING_ANALYSES.saturating_mul(INTERACTIVE_CAPACITY_FACTOR)
    } else {
        MAX_PENDING_ANALYSES
    };
    if active >= ceiling {
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
                              THEN SUBSTRING(repo_analysis_queue.last_error FROM $5) \
                              ELSE repo_analysis_queue.last_error END",
    )
    .bind(repo)
    .bind(priority.max(0))
    .bind(requested_by_user_id)
    .bind(now)
    // Un-parking a row clears its error — except for the `stall:N/U` record,
    // which is not a diagnosis of the last run but the queue's only durable
    // memory of how far this repository has ever got. Dropping it would let a
    // genuinely wedged repository start its stall ladder over on every visit,
    // and would forget the high-water mark that tells a slow-but-advancing
    // repository apart from a stuck one. A run that finishes deletes the row
    // outright, so the memory never outlives the problem.
    .bind(format!("{STALL_MARKER}[0-9]+/[0-9]+"))
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

/// Offer background backfill work — the curated catalog — to the queue.
///
/// Three differences from [`enqueue_many`], all of them about work nobody is
/// waiting on:
///   * **bounded**: at most `max_new` rows per pass, so a bootstrap cannot
///     consume the global [`MAX_PENDING_ANALYSES`] ceiling and turn every
///     visitor's enqueue into `AtCapacity`;
///   * **never resurrects**: a repository that already has a queue row in any
///     state, including a parked one, is skipped. `enqueue_prioritized` clears
///     a `dead` row's attempts and error on purpose — that is the right
///     answer for a person asking for a repository, and the wrong one for a
///     bootstrap that runs on every crash-loop restart;
///   * **coldest first**: never-analyzed repositories precede stale ones, so
///     a truncated pass still adds the repositories with nothing to show.
///
/// Returns how many rows it added.
pub async fn enqueue_backfill(db: &Db, repos: &[String], max_new: usize) -> Result<usize> {
    if repos.is_empty() || max_new == 0 {
        return Ok(0);
    }
    let candidates: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT slug FROM UNNEST($1::TEXT[]) AS slug \
         WHERE NOT EXISTS (SELECT 1 FROM repo_analysis_queue queued WHERE queued.repo = slug) \
           AND NOT EXISTS (SELECT 1 FROM repos cached \
                           WHERE cached.repo = slug AND cached.missing = TRUE) \
           AND NOT EXISTS (SELECT 1 FROM repo_history history \
                           WHERE history.repo = slug AND ",
        analysis_is_current_predicate!(),
        ") \
         ORDER BY (SELECT history.last_analyzed_at FROM repo_history history \
                   WHERE history.repo = slug) ASC NULLS FIRST, slug \
         LIMIT $4"
    ))
    .bind(repos)
    .bind(Utc::now() - analysis_freshness())
    .bind(CURRENT_ANALYSIS_REVISION)
    .bind(i64::try_from(max_new).unwrap_or(i64::MAX))
    .fetch_all(&db.pool)
    .await?;

    let mut enqueued = 0usize;
    for repo in candidates {
        // Priority 0: the catalog is a warm-up, and it is the one band the
        // reserved workers and the concurrency cap are defined against.
        match enqueue_prioritized(db, &repo, 0, None).await? {
            EnqueueOutcome::Enqueued => enqueued += 1,
            EnqueueOutcome::AtCapacity => break,
            EnqueueOutcome::AlreadyActive | EnqueueOutcome::Fresh => {}
        }
    }
    Ok(enqueued)
}

/// Repositories one catalog backfill pass may add, and the gap between passes.
///
/// The pair is chosen as a *rate*, and the rate has to sit below what the
/// catalog band can retire, or the drip is just a slower flood: sub-floor rows
/// run [`catalog_concurrency_cap`] at a time — one slot — and a catalog-sized
/// repository takes two to five minutes on this host, so that slot retires
/// roughly 12–30 rows an hour. Two rows every fifteen minutes is 8 an hour:
/// strictly below the drain, so the pending catalog depth converges to zero
/// instead of the 495-row backlog that filled the queue and starved visitor
/// work. Offering the ~117 curated repositories therefore takes most of a day,
/// which is the correct priority for warm-up nobody is waiting on.
const CATALOG_BACKFILL_BATCH: usize = 2;
const CATALOG_BACKFILL_INTERVAL: Duration = Duration::from_secs(900);

/// Drip the curated catalog into the analysis queue forever, a bounded batch
/// at a time.
///
/// Replaces the boot-time "offer all ~117 repositories at once": that filled
/// the queue with hundreds of priority-0 rows before the pool even existed,
/// and on a crash-looping or frequently-redeployed service it re-filled it on
/// every start. Safe in every replica — each pass skips repositories that
/// already have a row, so overlapping replicas converge rather than multiply.
pub fn spawn_catalog_backfill(db: Db, repos: Vec<String>) {
    if repos.is_empty() {
        return;
    }
    tokio::spawn(async move {
        loop {
            match enqueue_backfill(&db, &repos, CATALOG_BACKFILL_BATCH).await {
                Ok(0) => {}
                Ok(added) => tracing::info!(
                    added,
                    catalog = repos.len(),
                    "curated catalog backfill offered a batch"
                ),
                Err(error) => tracing::warn!(%error, "curated catalog backfill pass failed"),
            }
            sleep(CATALOG_BACKFILL_INTERVAL).await;
        }
    });
}

/// Identity carried by every job this *process* claims:
/// `ra:{host}:{pid}:{boot}:`. Unique to one process — two processes cannot
/// share a pid on a host at the same millisecond — which is what lets
/// [`release_pool_claims`] hand back this pool's rows and nobody else's.
fn pool_worker_prefix() -> &'static str {
    static PREFIX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PREFIX.get_or_init(|| {
        format!(
            "{}{}:",
            replica_worker_prefix(),
            Utc::now().timestamp_millis()
        )
    })
}

/// The same identity minus the boot timestamp: `ra:{host}:{pid}:`, which every
/// *incarnation* of this replica shares. Under Docker a restarted container
/// keeps its hostname and its pid 1, so this is how a process recognizes rows
/// its own predecessor abandoned.
fn replica_worker_prefix() -> &'static str {
    static PREFIX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PREFIX.get_or_init(|| format!("ra:{}:{}:", worker_host(), std::process::id()))
}

fn worker_host() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Reclaim analysis jobs no live worker is running, once, before this
/// process's own pool exists.
///
/// A row qualifies on one condition only: **the lease expired.** Nothing has
/// heartbeated `claimed_at` for longer than the claim path's steal window, so
/// the row is free by the same rule a peer would use.
///
/// Deliberately keyed on the lease alone, never on `{host}:{pid}`.
///
/// Reclaiming rows that merely *look* like this process's own is unsafe: a
/// container started with an explicit hostname (a compose `hostname:` key, a
/// service-name hostname) runs as pid 1, so two replicas of the same service
/// produce the identical identity. Replica B booting would then reclaim
/// replica A's live, freshly-heartbeated rows; both would analyze the same
/// repository, and A's `complete()` would delete the row B was running under.
/// That is a corruption, traded for reclaiming ghosts a couple of minutes
/// sooner — and the ghosts are already covered twice over, by
/// [`release_pool_claims`] on an orderly shutdown (which is what a redeploy
/// sends) and by the lease rule for everything else.
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

/// Hand back every job this process still holds, on the way out.
///
/// A redeploy is the one case where the process knows something the queue
/// cannot infer: its workers are about to stop existing. Waiting for the lease
/// to expire instead left the incoming pool's first two minutes of accounting
/// describing jobs nobody was running. Scoped to [`pool_worker_prefix`], so it
/// is exactly this process's rows however many replicas are up.
pub async fn release_pool_claims(db: &Db) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE repo_analysis_queue SET status = 'pending', phase = 'queued', \
         worker_id = NULL, claimed_at = NULL, started_at = NULL, updated_at = NOW() \
         WHERE status = 'in_progress' AND starts_with(worker_id, $1)",
    )
    .bind(pool_worker_prefix())
    .execute(&db.pool)
    .await?;
    Ok(res.rows_affected())
}

/// Revive jobs parked after a fixed number of transient clone/process
/// failures. Rows carrying [`TERMINAL_MARKER`] stay parked — reviving them on
/// every restart would undo the ceiling and let permanently-failing
/// repositories reoccupy the queue's capacity after each deploy.
///
/// Rows parked by [`MAX_ANALYSIS_STALLS`] deliberately do *not* carry
/// that marker, so a deploy re-arms them: a stall is a diagnosis of one
/// process, and the release being deployed is often exactly what fixes it.
/// Their `last_error` keeps the `stall:N/U` record, so a repository that goes
/// on stalling at the same point is parked again after a bounded number of
/// attempts instead of getting a free ladder on every restart.
pub async fn revive_retryable_on_startup(db: &Db) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE repo_analysis_queue SET status = 'pending', attempts = 0, \
            phase = 'queued', next_attempt_at = NOW(), worker_id = NULL, \
            claimed_at = NULL, started_at = NULL, updated_at = NOW() \
         WHERE status = 'dead' \
           AND (last_error IS NULL OR last_error NOT LIKE $1) \
           AND NOT EXISTS ( \
               SELECT 1 FROM repos \
               WHERE repos.repo = repo_analysis_queue.repo AND repos.missing = TRUE \
           )",
    )
    .bind(format!("{TERMINAL_MARKER}%"))
    .execute(&db.pool)
    .await?;
    Ok(res.rows_affected())
}

/// Analyses this replica runs at once. **The** single definition of the pool
/// size: the worker binary sizes its pool with it, and `repo_history` divides
/// the host's cores by it for `pack.threads` and for the commit walk's
/// subprocess fan-out.
///
/// **Three, because the box has 12 vCPU and does not belong to gitdebt.**
/// Every per-analysis parallelism on this host is
/// `repo_history::GIT_CORE_BUDGET / pool`, so this number chooses both how
/// much CPU the pool asks for in total and how that ask is *shaped*:
///
///   * at 8 (the previous production value) each clone indexed its pack with
///     `8 / 8 = 1` thread and each walk ran one `git log`, so eight analyses
///     crawled in lockstep: apache/kafka's 17,985 commits landed in 93 s while
///     microsoft/vscode (161k commits), postgres/postgres (64k) and rabbitmq
///     (62k) all ran past twenty minutes without finishing and torvalds/linux
///     never got a slot at all. The clones were finished and static on disk
///     throughout — the contention was CPU, disk and Postgres, not network;
///   * at 3, `repo_history`'s `GIT_CORE_BUDGET` of 8 — the host's 12 vCPU less
///     the 4 left to Postgres and the co-tenant services — buys `8 / 3 = 2`
///     index-pack threads and a 2-way walk fan-out per analysis, against
///     `8 / 8 = 1` of each at the old pool of eight. Three concurrent clones
///     share the measured 32 MiB/s instead of eight, and three concurrent
///     aggregate transactions hit a Postgres that lives on this same box and
///     was already logging slow statements every second or two.
///
/// The trade is deliberate and is *not* a constant-appetite reshuffle: total
/// git parallelism falls from 8 x 1 to 3 x 2 = 6 cores, while per-repository
/// parallelism doubles. Lowering the total is the point — it is what leaves
/// Postgres and the co-tenants room — and doubling the per-repository share is
/// what makes vscode, postgres and rabbitmq finish instead of crawl.
///
/// A repository that finishes is worth more than a repository that started,
/// and finishing is what frees the slot: three fat workers retire more work
/// per hour than eight starved ones even though fewer jobs are in flight.
/// Three also leaves genuine headroom for the co-tenant services, the star
/// pool and the API, which eight did not.
///
/// It is *not* the number a queue ETA divides by — part of the pool serves
/// only visitor-driven work, so a row in the catalog band drains at
/// [`general_analysis_workers`] per wave, not at this rate.
const ANALYSIS_WORKERS: usize = 3;

pub fn configured_analysis_workers() -> usize {
    ANALYSIS_WORKERS
}

/// Share of the pool held back for rows at or above
/// [`VISITOR_PRIORITY_FLOOR`]: one in three, so exactly one of
/// [`ANALYSIS_WORKERS`] is reserved and two stay general.
///
/// A third is the smallest reservation that survives the pool getting
/// smaller. The shape matters more than the ratio: at a pool of 3 this is one
/// worker that a waiting visitor always finds free, and two that the catalog
/// may compete for — while at the old pool of 8 the same fraction reserved 2
/// and left 6, which is why a burst of catalog work could still swallow the
/// queue.
///
/// Always leaves at least one worker able to claim any band, so a pool of one
/// reserves nothing: a lone worker that only ever claimed visitor work would
/// let the catalog starve completely. Reserved workers also fall back to
/// general claims after a few empty polls (see [`RESERVED_FALLBACK_POLLS`]),
/// so the reservation costs idle capacity for seconds, not for a job.
pub fn reserved_visitor_workers(pool: usize) -> usize {
    (pool / RESERVED_VISITOR_SHARE_DIVISOR)
        .max(1)
        .min(pool.saturating_sub(1))
}

const RESERVED_VISITOR_SHARE_DIVISOR: usize = 3;

/// Workers that will claim any band, including catalog backfill. **This** is
/// the divisor for a queue-position ETA in the catalog band: dividing a
/// position by [`configured_analysis_workers`] would promise a drain rate the
/// reserved workers do not contribute to.
pub fn general_analysis_workers() -> usize {
    let pool = configured_analysis_workers();
    pool.saturating_sub(reserved_visitor_workers(pool)).max(1)
}

/// Ceiling on rows *below* [`VISITOR_PRIORITY_FLOOR`] running at once, across
/// every replica. Enforced inside the claim itself, so 495 queued catalog rows
/// can never hold more than this many slots no matter how many workers go
/// looking for work.
///
/// Half the general pool, which at [`ANALYSIS_WORKERS`] = 3 is **one**: the
/// catalog gets a single slot and two of the three always remain available to
/// somebody waiting on a report. That is the headroom the old shape lacked —
/// 8 workers with a cap of 3 still let backfill hold three quarters of the
/// visitor-reachable pool at once. Never zero, which would not throttle the
/// catalog but stop it forever.
pub fn catalog_concurrency_cap() -> i64 {
    catalog_cap_for(general_analysis_workers())
}

fn catalog_cap_for(general_workers: usize) -> i64 {
    i64::try_from((general_workers / 2).max(1)).unwrap_or(i64::MAX)
}

/// Empty visitor-only polls a reserved worker tolerates before it also claims
/// general work. The reservation must not become idleness while the queue has
/// work: three one-second polls is long enough that a reserved worker is
/// still free the moment a visitor arrives, short enough that it never sits
/// out a quiet minute.
const RESERVED_FALLBACK_POLLS: u32 = 3;

pub fn spawn_pool(ctx: AnalysisCtx, count: usize) {
    // Author enrichment rides along with the analysis pool but never on its
    // critical path: presentation-only metadata converges here, on its own
    // bounded schedule, so it can neither gate analysis readiness nor keep a
    // job in the queue.
    spawn_author_enrichment_sweep(ctx.clone());
    let reserved = reserved_visitor_workers(count);
    for i in 0..count {
        let ctx = ctx.clone();
        // Every id carries this process's identity, which is what lets a
        // shutdown hand its own rows back and a restart recognize its
        // predecessor's (see [`reset_inflight_on_startup`]).
        let id = format!("{}{i}", pool_worker_prefix());
        let lane = if i < reserved {
            Lane::Visitor
        } else {
            Lane::General
        };
        tokio::spawn(async move {
            run_worker(id, ctx, lane).await;
        });
    }
    tracing::info!(
        workers = count,
        reserved_visitor = reserved,
        catalog_concurrency_cap = catalog_concurrency_cap(),
        "repo-analysis pool lanes configured"
    );
}

/// Which bands a worker claims from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    /// Claims any band.
    General,
    /// Claims only at or above [`VISITOR_PRIORITY_FLOOR`], until
    /// [`RESERVED_FALLBACK_POLLS`] consecutive polls find nothing there.
    Visitor,
}

/// The floor a worker's next claim uses.
///
/// A reserved worker relaxes to the general floor only after its own band has
/// been empty for several polls, and tightens again the moment it claims
/// anything — so reserving capacity never means idling next to work that is
/// waiting, and a worker that took a fallback job is back on visitor duty as
/// soon as that job ends.
fn claim_min_priority(lane: Lane, empty_polls: u32) -> i64 {
    match lane {
        Lane::Visitor if empty_polls < RESERVED_FALLBACK_POLLS => VISITOR_PRIORITY_FLOOR,
        _ => 0,
    }
}

async fn run_worker(worker_id: String, ctx: AnalysisCtx, lane: Lane) {
    tracing::info!(worker_id, ?lane, "repo-analysis worker started");
    // Interactive requests should not wait behind a five-second polling nap.
    // One indexed claim per idle worker per second keeps wake-up latency low
    // without turning an empty queue into a busy loop.
    let idle = Duration::from_secs(1);
    let catalog_cap = catalog_concurrency_cap();
    let mut empty_polls: u32 = 0;
    loop {
        let min_priority = claim_min_priority(lane, empty_polls);
        let job = match claim_one(&ctx.db, &worker_id, min_priority, catalog_cap).await {
            Ok(Some(repo)) => {
                empty_polls = 0;
                repo
            }
            Ok(None) => {
                empty_polls = empty_polls.saturating_add(1);
                sleep(idle).await;
                continue;
            }
            Err(e) => {
                tracing::error!(error = %e, "claim failed");
                empty_polls = empty_polls.saturating_add(1);
                sleep(idle).await;
                continue;
            }
        };
        tracing::info!(
            repo = %job.repo,
            interactive = job.requested_by_user_id.is_some(),
            prior_stalls = job.prior_stalls,
            best_units = job.best_units,
            "analysis run started"
        );
        let heartbeat_stop =
            spawn_lease_heartbeat(ctx.db.clone(), job.repo.clone(), worker_id.clone());
        let outcome = run_until_stalled(&job, &ctx).await;
        let _ = heartbeat_stop.send(true);
        match outcome {
            Ok(commits_applied) => {
                tracing::info!(repo = %job.repo, commits_applied, "analysis run complete");
                if let Err(e) = complete(&ctx.db, &job.repo).await {
                    tracing::warn!(repo = %job.repo, error = %e, "queue complete failed");
                }
                // After every completion, not amortized over a batch of them.
                // Clones carry complete history now, so one job can add
                // gigabytes; disk is the only ceiling an analysis has left,
                // and the overshoot an amortized sweep tolerated is measured
                // in whole repositories. `evict_to_quota` opens with a single
                // aggregate and returns immediately when the volume is under
                // its watermark, so the common case costs one query.
                if let Err(e) = repo_stats::evict_to_quota(&ctx.db, &ctx.storage).await {
                    tracing::warn!(error = %e, "eviction pass failed");
                }
            }
            Err(failure) => {
                tracing::warn!(
                    repo = %job.repo,
                    error = %failure.message,
                    park = failure.park,
                    "analysis run failed"
                );
                if let Err(e2) = fail(&ctx.db, &job.repo, &failure).await {
                    tracing::warn!(repo = %job.repo, error = %e2, "queue fail failed");
                }
                // No sleep here: `fail` already parked this row behind a
                // durable `next_attempt_at`, so the worker cannot re-claim it.
                // Sleeping instead idled the whole slot, which turned a broad
                // upstream outage into a throughput collapse across the pool.
            }
        }
    }
}

/// Silence — no phase transition and no progress write — that a run may go
/// through before the worker declares it stuck and kills it.
///
/// This is *not* a job budget. Analyzing complete history is the product, so a
/// repository with hundreds of thousands of commits is allowed to take as long
/// as it honestly takes; what is never allowed is a run that has stopped doing
/// anything. Killing a working job on a flat wall clock is worse than useless
/// on a large repository: every attempt dies at the same point and the next
/// one starts over, so the analysis is guaranteed never to land.
///
/// It is therefore a backstop, not a budget: `repo_history` already bounds
/// every git phase individually, and each of those lapses into an ordinary
/// error. This only catches a hang none of them cover.
///
/// **What it has to clear.** The clone and the fetch used to be the answer,
/// and they were the reason this value was wrong: a 6 GB transfer is a single
/// silent await that can legitimately outlast any window an operator would
/// accept for a wedged job, so no setting of this number was both safe and
/// useful. They no longer count — [`process`] hands `open_or_clone` a
/// [`repo_history::Progress`] callback, so a transfer that is moving beats and
/// only a transfer that has stopped goes quiet. What remains is:
///
///   * `saving_history` — the one aggregate-plus-cursor transaction. It is
///     deliberately unbounded and deliberately silent: splitting it to publish
///     progress would split the delta from the cursor advance, which is the
///     invariant incremental analysis rests on. Measured worst case is minutes
///     even at a million commits.
///   * `finishing` — the HEAD-tree line count, bounded at ~300 s by
///     `code_count`'s own tree-listing and exact-count timeouts.
///
/// One hour of *total silence* is therefore an order of magnitude above the
/// only phase that can honestly produce it, and is unambiguous evidence of a
/// hang. It is also twelve times `repo_history`'s own longest per-chunk
/// ceiling (300 s for a walk chunk), so a phase that is merely slow on a
/// loaded co-tenant box lapses into an ordinary, retryable error long before
/// it looks stuck. The cost of the headroom is that a genuinely wedged run
/// holds one of three workers for an hour before [`MAX_ANALYSIS_STALLS`]
/// starts counting; the pool reserves visitor capacity and caps catalog
/// concurrency precisely so that this cannot become a queue-wide outage.
const ANALYSIS_STALL_SECONDS: u64 = 3_600;
const ANALYSIS_STALL_WINDOW: Duration = Duration::from_secs(ANALYSIS_STALL_SECONDS);

/// How often the guard samples the heartbeat. Only the resolution of the
/// stall verdict, so it is coarse on purpose.
const STALL_POLL_INTERVAL: Duration = Duration::from_secs(15);

/// Consecutive stalls, without ever getting further, before the row is parked.
/// A parked row is re-armed by the next deploy (see
/// [`revive_retryable_on_startup`]) and by any explicit request, so this ends
/// a wasteful loop rather than a repository's chances. Three is what a wedged
/// repository costs this pool before it is put down: 3 × the stall window ≈ 3
/// hours of one worker, spread over an escalating backoff.
const MAX_ANALYSIS_STALLS: u32 = 3;

/// Absolute wall-clock ceiling on one run: twelve hours.
///
/// A runaway detector, deliberately not a size limit. The honest way to read
/// any value here is **any repository whose first analysis cannot finish
/// inside this window will never be analyzed at all** — a run is not resumable
/// past the cursor its last *successful* run committed, so every attempt
/// restarts the same walk and dies at the same point. So the number has to be
/// derived from the largest repository that must still land, not from what an
/// operator would like a job to cost.
///
/// **The arithmetic, on this host.** apache/kafka's 17,985 commits completed
/// in 93 s — ~190 commits/s end to end at a pool of 8. torvalds/linux is
/// 1.46M commits, so the same rate extrapolates to ~2.1 hours, and its 6.1 GB
/// clone adds ~190 s of transfer at the measured 32 MiB/s plus an index-pack
/// that now gets `GIT_CORE_BUDGET / 3 = 2` threads instead of 1. Even tripling
/// the whole estimate for a fully contended box lands near 6 hours, and 12
/// leaves a 2× margin on top of *that*. Nothing anyone points at this deployment reaches
/// it while making progress.
///
/// It is coherent with [`ANALYSIS_STALL_WINDOW`] by being a multiple of it: a
/// run that goes quiet is always killed by the stall guard first (at 1 hour,
/// with the stall ladder that eventually parks the row), so this ceiling can
/// only ever fire on a run that is *beating* and still going after half a day
/// — a loop, not a big repository. That is why a run that trips it is reported
/// as an ordinary transient failure and counts toward neither
/// [`MAX_ANALYSIS_STALLS`] nor [`TERMINAL_MARKER`]: the repository did nothing
/// wrong, and the next deploy must be able to bring it back.
const ABSOLUTE_JOB_CEILING_SECONDS: u64 = 12 * 3_600;
const ABSOLUTE_JOB_CEILING: Duration = Duration::from_secs(ABSOLUTE_JOB_CEILING_SECONDS);
/// The ceiling must stay several stall windows wide, or it would start
/// pre-empting the stall guard — killing wedged runs *without* recording a
/// stall, so the ladder that eventually parks them would never advance.
const _: () = assert!(ABSOLUTE_JOB_CEILING_SECONDS >= 4 * ANALYSIS_STALL_SECONDS);

/// Evidence that a run is still doing something, shared between [`process`]
/// and the guard watching it.
#[derive(Debug)]
struct Heartbeat {
    /// Bumped by every phase transition and every progress write. Only the
    /// fact that it changes matters, never its value.
    beats: AtomicU64,
    /// High-water mark of units this run has completed. Monotonic on purpose:
    /// phases restart their own counters, and what the durable stall record
    /// needs is how far the run got overall.
    units: AtomicU64,
    /// The phase the run is in, so a stall names where it happened.
    phase: std::sync::Mutex<&'static str>,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            beats: AtomicU64::new(0),
            units: AtomicU64::new(0),
            phase: std::sync::Mutex::new("starting"),
        }
    }
}

impl Heartbeat {
    /// Record that the run entered a phase. Phases that report no units — the
    /// metadata lookup, the clone, the commit plan — beat through here, which
    /// is what stops the guard from mistaking a long clone for a stuck job.
    fn phase(&self, phase: &'static str) {
        *self.phase.lock().unwrap_or_else(|error| error.into_inner()) = phase;
        self.beats.fetch_add(1, Ordering::Relaxed);
    }

    fn progress(&self, phase: &'static str, completed: usize) {
        self.units.fetch_max(completed as u64, Ordering::Relaxed);
        self.phase(phase);
    }

    fn beats(&self) -> u64 {
        self.beats.load(Ordering::Relaxed)
    }

    fn units(&self) -> u64 {
        self.units.load(Ordering::Relaxed)
    }

    fn current_phase(&self) -> &'static str {
        *self.phase.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// Run one analysis, killing it only when it stops making progress.
///
/// Worker slots are the scarcest resource in the pool and the lease heartbeat
/// guarantees no peer will ever steal a job from a wedged one, so a wedged run
/// must be ended by this process or not at all. Dropping the returned future
/// is what ends it: `kill_on_drop` on the git commands reaps the subprocess.
async fn run_until_stalled(job: &AnalysisJob, ctx: &AnalysisCtx) -> Result<usize, Failure> {
    let beat = Heartbeat::default();
    let work = process(job, ctx, &beat);
    guard_progress(
        job,
        &beat,
        ANALYSIS_STALL_WINDOW,
        Some(ABSOLUTE_JOB_CEILING),
        work,
    )
    .await
}

/// The guard proper, over any work that reports through `beat`.
///
/// Split out from [`run_until_stalled`] so the budgets are arguments rather
/// than module constants. The property that matters — "a run that keeps
/// beating is never killed, however many windows it outlasts" — is about the
/// ratio of beat interval to window, not about hours, so a test can only state
/// it by supplying both.
async fn guard_progress<F>(
    job: &AnalysisJob,
    beat: &Heartbeat,
    patience: Duration,
    ceiling: Option<Duration>,
    work: F,
) -> Result<usize, Failure>
where
    F: std::future::Future<Output = Result<usize>>,
{
    let started = Instant::now();
    tokio::pin!(work);

    let mut last_beat = beat.beats();
    let mut silent_since = Instant::now();
    loop {
        tokio::select! {
            done = &mut work => {
                return done.map_err(|error| Failure::transient(&error));
            }
            _ = sleep(STALL_POLL_INTERVAL.min(patience)) => {
                let beats = beat.beats();
                if beats != last_beat {
                    last_beat = beats;
                    silent_since = Instant::now();
                }
                let silence = silent_since.elapsed();
                if silence >= patience {
                    return Err(Failure::stalled(job, beat, silence));
                }
                if ceiling.is_some_and(|ceiling| started.elapsed() >= ceiling) {
                    return Err(Failure::over_ceiling(beat, started.elapsed()));
                }
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

/// Claim the highest-priority runnable job this worker's lane may take.
///
/// `min_priority` is the lane floor: a reserved worker passes
/// [`VISITOR_PRIORITY_FLOOR`] and simply cannot see catalog rows, which is
/// what makes reservation different from ordering. `catalog_cap` bounds how
/// many rows *below* that floor may be in progress at once.
///
/// Both predicates live inside the existing `FOR UPDATE SKIP LOCKED`
/// subquery, so the claim stays exactly one atomic statement and multi-replica
/// claiming and the stale-lease steal path are untouched. The cap is a soft
/// one under concurrency — two workers can read the same count in the same
/// instant and both admit a catalog job — which is why it is expressed as
/// "about half the pool" rather than as a correctness invariant.
async fn claim_one(
    db: &Db,
    worker_id: &str,
    min_priority: i64,
    catalog_cap: i64,
) -> Result<Option<AnalysisJob>> {
    let now = Utc::now();
    let row = sqlx::query(
        "UPDATE repo_analysis_queue \
         SET status = 'in_progress', phase = 'cloning', worker_id = $1, \
             claimed_at = $2, started_at = $2, updated_at = $2, \
             total_units = NULL, completed_units = 0 \
         WHERE repo = ( \
            SELECT candidate.repo FROM repo_analysis_queue candidate \
            WHERE ((candidate.status = 'pending' AND candidate.next_attempt_at <= NOW()) \
                OR (candidate.status = 'in_progress' \
                    AND candidate.claimed_at < NOW() - INTERVAL '2 minutes')) \
              AND candidate.priority >= $3 \
              AND (candidate.priority >= $4 OR ( \
                    SELECT COUNT(*) FROM repo_analysis_queue running \
                    WHERE running.status = 'in_progress' \
                      AND running.priority < $4 \
                      AND running.claimed_at >= NOW() - INTERVAL '2 minutes' \
                  ) < $5) \
            ORDER BY candidate.priority DESC, candidate.enqueued_at \
            FOR UPDATE SKIP LOCKED LIMIT 1 \
         ) \
         RETURNING repo, requested_by_user_id, last_error",
    )
    .bind(worker_id)
    .bind(now)
    .bind(min_priority)
    .bind(VISITOR_PRIORITY_FLOOR)
    .bind(catalog_cap)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(|row| {
        let (prior_stalls, best_units) = stall_record(
            row.try_get::<Option<String>, _>("last_error")
                .ok()
                .flatten(),
        );
        AnalysisJob {
            repo: row.try_get::<String, _>("repo").unwrap_or_default(),
            requested_by_user_id: row.try_get("requested_by_user_id").ok().flatten(),
            prior_stalls,
            best_units,
        }
    }))
}

async fn complete(db: &Db, repo: &str) -> Result<()> {
    sqlx::query("DELETE FROM repo_analysis_queue WHERE repo = $1")
        .bind(repo)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// Transient failures a job may accumulate before it is parked terminally.
/// Without a ceiling, a repository that can never be cloned retries at the
/// one-hour floor forever while still counting against the queue's capacity
/// ceiling — enough of them and no new work can be admitted at all.
const MAX_ANALYSIS_ATTEMPTS: i64 = 8;

/// Marks a row parked by [`MAX_ANALYSIS_ATTEMPTS`]. Startup revival re-opens
/// jobs parked by older releases, which had no terminal state; rows carrying
/// this prefix were parked deliberately and must stay parked.
///
/// Which failures may write it is [`Failure::terminal`], not the attempt count
/// alone: reaching the ceiling on evidence this process manufactured — a stall
/// verdict, an operator's own wall-clock ceiling — is not evidence about the
/// repository, and the largest repositories are the ones that manufacture it.
pub const TERMINAL_MARKER: &str = "terminal:";

/// Prefix that records "this run was killed for making no progress": how many
/// times in a row, and the furthest any attempt on this repository has ever
/// got. It is the queue's only durable memory of a stall — `attempts` cannot
/// tell a half-hour silence from a two-second clone error — so it is
/// carried in the error text rather than in two new columns:
/// `stall:2/34000 analysis made no progress ...`.
const STALL_MARKER: &str = "stall:";

/// Recover `(consecutive stalls, furthest units reached)` from a queue row's
/// `last_error`. Any other error text means the last run failed some other way
/// and the record starts over — "consecutive" is the point.
fn stall_record(last_error: Option<String>) -> (u32, u64) {
    let Some(error) = last_error else {
        return (0, 0);
    };
    let error = error
        .trim()
        .strip_prefix(TERMINAL_MARKER)
        .unwrap_or(&error)
        .trim_start();
    let Some(record) = error
        .strip_prefix(STALL_MARKER)
        .and_then(|rest| rest.split_whitespace().next())
    else {
        return (0, 0);
    };
    let (count, units) = record.split_once('/').unwrap_or((record, "0"));
    (count.parse().unwrap_or(0), units.parse().unwrap_or(0))
}

/// A failed run, and what the queue should do about it.
struct Failure {
    message: String,
    /// Park the row now instead of scheduling another attempt.
    park: bool,
    /// May this failure, on the [`MAX_ANALYSIS_ATTEMPTS`] attempt, retire the
    /// repository permanently — that is, write [`TERMINAL_MARKER`], which
    /// [`revive_retryable_on_startup`] refuses to re-open?
    ///
    /// Only true for ordinary errors, where eight failures really do mean the
    /// repository cannot be analyzed. It is false for every failure this
    /// process inflicts on itself, and those are exactly the ones a very large
    /// repository accumulates:
    ///
    ///   * a stall has its own ladder ([`MAX_ANALYSIS_STALLS`]) whose contract
    ///     is that the next deploy re-arms the row — the release being
    ///     deployed is often what fixes the hang. Letting a stall also spend
    ///     the attempt ceiling silently broke that contract: a repository that
    ///     stalled with `attempts` already high was retired for good;
    ///   * an [`ABSOLUTE_JOB_CEILING`] lapse is a statement about this
    ///     deployment's own patience, not about the repository. Raising the
    ///     ceiling and redeploying must bring the row back;
    ///   * a `repo_history` wall-clock ceiling — clone, fetch, plan, count,
    ///     walk — is the same statement one layer down. It arrives as an
    ///     ordinary `Err` from `process`, so it is told apart by
    ///     [`repo_history::budget_lapsed`] rather than by its own constructor.
    ///
    /// All of them still reach `dead` and stop consuming worker slots; what
    /// they lose is the ability to make that permanent.
    terminal: bool,
}

impl Failure {
    /// An ordinary error: clone refused, git exited non-zero, Postgres blipped.
    /// These are cheap to discover and worth the full attempt ladder.
    ///
    /// Except when the error is one of `repo_history`'s own ceilings lapsing.
    /// That is not a discovery about the repository at all — it is this
    /// deployment saying it gave up early — and eight of them used to write
    /// [`TERMINAL_MARKER`], after which no amount of raising the ceiling and
    /// redeploying could ever bring the repository back. The largest
    /// repositories are precisely the ones that collect these.
    fn transient(error: &anyhow::Error) -> Self {
        Self {
            message: compact_error(&format!("{error:#}")),
            park: false,
            terminal: !(repo_history::budget_lapsed(error)
                || repo_history::local_clone_unusable(error)),
        }
    }

    /// The run stopped reporting progress and was killed.
    ///
    /// The consecutive count restarts whenever a run gets further than any
    /// previous one did, because that is a large repository grinding forward
    /// rather than a stuck one — and only the latter should ever be parked.
    fn stalled(job: &AnalysisJob, beat: &Heartbeat, silence: Duration) -> Self {
        let units = beat.units();
        let advanced = units > job.best_units;
        let consecutive = if advanced {
            1
        } else {
            job.prior_stalls.saturating_add(1)
        };
        Self {
            message: compact_error(&format!(
                "{STALL_MARKER}{consecutive}/{} analysis made no progress for {}s in phase \
                 '{}' after {units} units",
                job.best_units.max(units),
                silence.as_secs(),
                beat.current_phase(),
            )),
            park: consecutive >= MAX_ANALYSIS_STALLS,
            terminal: false,
        }
    }

    /// [`ABSOLUTE_JOB_CEILING`] lapsed while the run was still beating.
    /// Deliberately an ordinary transient failure: nothing about the
    /// repository is wrong, so it must not accumulate toward parking.
    fn over_ceiling(beat: &Heartbeat, elapsed: Duration) -> Self {
        Self {
            message: compact_error(&format!(
                "analysis exceeded the {}s runaway ceiling in phase '{}' after {} units",
                elapsed.as_secs(),
                beat.current_phase(),
                beat.units(),
            )),
            park: false,
            terminal: false,
        }
    }
}

/// Record a failed attempt: bump the backoff, and decide whether the row stops
/// being runnable.
///
/// `attempts` is always incremented, including for the failures this process
/// inflicts on itself — it is what drives `next_attempt_at`, so freezing it
/// would leave a repeatedly-failing repository retrying at the 30-second floor
/// forever. What [`Failure::terminal`] gates is narrower and separate: whether
/// crossing [`MAX_ANALYSIS_ATTEMPTS`] also writes [`TERMINAL_MARKER`], the one
/// thing startup revival will not undo.
async fn fail(db: &Db, repo: &str, failure: &Failure) -> Result<()> {
    sqlx::query(
        "UPDATE repo_analysis_queue SET \
            attempts = attempts + 1, \
            last_error = CASE WHEN attempts + 1 >= $3 AND $6 \
                              THEN $4 || ' ' || $1 ELSE $1 END, \
            worker_id = NULL, \
            claimed_at = NULL, \
            status = CASE WHEN attempts + 1 >= $3 OR $5 THEN 'dead' ELSE 'pending' END, \
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
    .bind(&failure.message)
    .bind(repo)
    .bind(MAX_ANALYSIS_ATTEMPTS)
    .bind(TERMINAL_MARKER)
    .bind(failure.park)
    .bind(failure.terminal)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// One-line, bounded error text for a queue row. `last_error` is parsed by
/// [`stall_record`] and matched by startup revival, so it must stay a single
/// control-character-free line.
fn compact_error(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(1_000)
        .collect()
}

/// Does the stored `repo_history` row, read on its own, already rule out an
/// incremental append?
///
/// Two reasons, both decidable before a plan exists — which is why they are
/// separate from the cursor check: this answer is what decides whether
/// `plan_commits` is even offered a cursor to validate.
///
///   * **no usable row.** Never analyzed, or analyzed by a revision older than
///     [`CURRENT_ANALYSIS_REVISION`] — those aggregates were built from a
///     sampled window rather than from the repository, so they are a different
///     measurement and cannot be added to.
///   * **the cursor is a placeholder.** A repository that was empty when it
///     was last seen stored [`repo_history::EMPTY_REPOSITORY_HEAD`], which is
///     not a commit at all.
///
/// `was_truncated` is deliberately *not* a reason: a row already at the
/// current revision covers every reachable commit, and its cursor is coherent
/// at the head the last run reached.
fn stored_row_needs_rebuild(analysis_revision: i32, last_sha: Option<&str>) -> bool {
    analysis_revision < CURRENT_ANALYSIS_REVISION
        || last_sha == Some(repo_history::EMPTY_REPOSITORY_HEAD)
}

/// Must this run replace every aggregate instead of adding to them?
///
/// [`stored_row_needs_rebuild`] plus the answer `plan_commits` gives about the
/// cursor, and that second term is the one that costs real money if it is
/// dropped. A force-push or a rebase of the default branch rewrites commits
/// that are already summed into every stored aggregate, and
/// `git rev-list <rewritten>..HEAD` does not fail on it: it exits 0 and prints
/// a perfectly plausible list of the commits unique to the new head. Appending
/// that lands fresh stats on top of the rewritten commits' surviving stats, so
/// commit counts, per-file churn and per-author totals drift upward
/// permanently, and nothing downstream can detect it because no record of the
/// old history survives. Rebases are routine, so this is a normal branch of a
/// steady-state run, not an exotic one.
///
/// One-way on purpose: a caller that had already decided to rebuild still
/// rebuilds. A usable cursor is permission to append, never an instruction to.
fn must_rebuild(analysis_revision: i32, last_sha: Option<&str>, cursor_rejected: bool) -> bool {
    stored_row_needs_rebuild(analysis_revision, last_sha) || cursor_rejected
}

/// Is the HEAD-derived half of the analysis — the language breakdown and the
/// repository-readiness flags — actually stored for this exact head?
///
/// Those describe the current tree rather than the history, so they are
/// recomputed in full by every run that walks anything, incremental or not
/// (see [`run_line_counts`]). The gap this closes is the *no-op* path: if the
/// run that last advanced the cursor had its line-count pass fail, it still
/// stamped the analysis revision, and every later run then matched
/// `head_sha == last_analyzed_sha` and returned early — so a repository whose
/// HEAD does not move would never acquire language data at all. One indexed
/// lookup makes the early return mean "everything this run would produce is
/// already stored", which is what it always claimed to mean.
async fn head_derived_data_is_at(db: &Db, repo: &str, head_sha: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM repo_readiness WHERE repo = $1 AND head_sha = $2)",
    )
    .bind(repo)
    .bind(head_sha)
    .fetch_one(&db.pool)
    .await?)
}

async fn process(job: &AnalysisJob, ctx: &AnalysisCtx, beat: &Heartbeat) -> Result<usize> {
    let repo = job.repo.as_str();
    let started = Instant::now();
    beat.phase("checking_visibility");
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
                cache.put_repo_metadata(repo, &metadata).await?;
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

    // The clone completes no units, so it can only report liveness. One beat
    // before the await was worthless: the await *is* the phase, and a 6.1 GB
    // transfer legitimately outlasts any stall window worth having, so the
    // guard read a working clone as a wedged job and killed it — every time,
    // at the same point, which is the shape that guarantees a large repository
    // is never analyzed at all. This callback is what makes the guard measure
    // silence instead of duration: `open_or_clone` beats it while bytes are
    // arriving and stops beating when they stop.
    beat.phase("cloning");
    let clone_is_alive = || beat.phase("cloning");
    let handle = repo_history::open_or_clone(
        &ctx.storage,
        repo,
        last_sha.as_deref(),
        Some(&clone_is_alive),
    )
    .await
    .context("open_or_clone")?;
    // Update bookkeeping (clone_path + size + last_visited_at).
    let size = repo_history::clone_size_bytes(&handle.path);
    repo_stats::record_clone(&ctx.db, repo, &handle.path, size).await?;

    // An empty GitHub repository is a successful zero-result analysis, not a
    // transient git failure. Persist the empty aggregate so request paths can
    // render immediately and the durable queue does not retry forever.
    if handle.is_empty() {
        update_work_progress(&ctx.db, beat, repo, "saving_history", Some(0), 0).await?;
        repo_stats::replace_commits_at_head(&ctx.db, repo, &[], &handle.head_sha, 0).await?;
        code_count::save(&ctx.db, repo, &[], true).await?;
        code_count::save_repository_readiness(
            &ctx.db,
            repo,
            &handle.head_sha,
            &code_count::RepositoryReadiness::default(),
        )
        .await?;
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

    // No cursor check guards this branch, and none is needed: `head_sha` came
    // from `rev-parse HEAD` on the clone this run just refreshed, so a cursor
    // equal to it necessarily still exists and is trivially its own ancestor.
    if Some(handle.head_sha.as_str()) == last_sha.as_deref()
        && analysis_revision >= CURRENT_ANALYSIS_REVISION
        && head_derived_data_is_at(&ctx.db, repo, &handle.head_sha).await?
    {
        // Nothing to walk: commit aggregates and line counts are already at
        // this head under the current revision. Stamp the run before
        // returning — `last_analyzed_at` is what [`ANALYSIS_IS_CURRENT_SQL`]
        // reads, so without it a repository whose HEAD never moves can never
        // become `Fresh` again and every view re-enqueues a job that fetches
        // the remote just to rediscover the same head. Author enrichment is
        // explicitly NOT run here — it is presentation-only metadata owned by
        // [`sweep_author_enrichment`], and doing GitHub work on this path is
        // what let a repeatedly re-enqueued no-op job burn budget forever.
        repo_stats::touch_analyzed_at(&ctx.db, repo).await?;
        return Ok(0);
    }

    // Offer the cursor only when the stored row itself does not already
    // disqualify it; `plan_commits` then decides whether the repository's own
    // history still supports it.
    let stale_row = stored_row_needs_rebuild(analysis_revision, last_sha.as_deref());
    beat.phase("planning_window");
    let planned =
        repo_history::plan_commits(&handle, if stale_row { None } else { last_sha.as_deref() })
            .await?;
    let replace = must_rebuild(
        analysis_revision,
        last_sha.as_deref(),
        planned.requires_full_rebuild(),
    );
    if let Some(reason) = planned.rejection() {
        // Warn, not info: this is the one path that discards a repository's
        // entire stored aggregate set, and the alternative — appending to
        // aggregates that still count rewritten commits — drifts silently and
        // forever. An operator looking at a commit-count discontinuity needs
        // to find this line.
        tracing::warn!(
            repo,
            last_analyzed_sha = last_sha.as_deref().unwrap_or_default(),
            head_sha = %handle.head_sha,
            reason = reason.as_str(),
            commits = planned.plan().shas.len(),
            "stored analysis cursor rejected; replacing aggregates with complete history"
        );
    }
    let plan = planned.into_plan();
    beat.phase("counting_commits");
    let reachable_commits = repo_history::reachable_commit_count(&handle).await?;
    update_work_progress(
        &ctx.db,
        beat,
        repo,
        "scanning_history",
        Some(plan.shas.len()),
        0,
    )
    .await?;
    // Every walked commit is folded into the accumulator and then dropped.
    // Holding the walk in a `Vec<CommitInfo>` until the write made peak memory
    // a function of the repository's AGE: linux's ~1.1M non-merge commits are
    // roughly a gigabyte of records before a single row is written, and the
    // aggregation maps were then built alongside them. `Aggregates` is keyed by
    // authors, files, days and file pairs — the repository's SHAPE — so it
    // stays bounded however long the history is, which is what lets the
    // analysis pool run several giants at once without the commit cap that
    // used to hide this.
    let mut aggregator = repo_stats::CommitAggregator::new();
    // The TODO scan is a separate, later phase (it reports its own progress and
    // its own share of the bar), and it can only amend the newest
    // `TODO_PATCH_COMMIT_LIMIT` commits. Those are held back from the
    // accumulator until it has run, so they are folded in with their final
    // values; the buffer is bounded by that constant, never by history length.
    // They are also the last commits in plan order, so pushing them last
    // preserves the accumulator's oldest-first contract exactly.
    let todo_start = plan
        .shas
        .len()
        .saturating_sub(repo_history::TODO_PATCH_COMMIT_LIMIT);
    let todo_shas = &plan.shas[todo_start..];
    let deferred_shas: HashSet<&str> = todo_shas.iter().map(String::as_str).collect();
    let mut deferred: Vec<repo_history::CommitInfo> = Vec::with_capacity(todo_shas.len());
    // Batched rather than one walk over the whole history: a batch boundary is
    // where this run publishes progress, and progress is now what keeps it
    // alive (see [`run_until_stalled`]) as well as what a waiting visitor
    // watches. One `git log` over 300,000 commits would report nothing for
    // minutes and read as silence.
    let mut walked_commits = 0usize;
    for batch in plan.shas.chunks(repo_history::METADATA_BATCH_COMMITS) {
        let walked = repo_history::walk_commit_metadata_batch(&handle, batch).await?;
        if walked.incomplete_objects {
            // A `--numstat` chunk fell back to a path-only walk because git
            // could not read an object, so every commit in it carries zero
            // lines added and zero lines deleted. This is the damaged-object
            // case ONLY: a chunk that merely lapsed its wall-clock ceiling is
            // retried in halves inside `repo_history` and, if that ladder is
            // spent, surfaces as a `BUDGET_MARKER` error that the queue
            // classifies non-terminal. Conflating the two is what used to
            // discard a walk that had already completed a million commits
            // because one chunk was slow on a co-tenant host.
            //
            // Nothing durable has happened yet — the whole walk precedes the
            // aggregate transaction — so abandoning the run costs one walk and
            // leaves the repository exactly as it was: coherent aggregates at
            // the older head, cursor unmoved, clone still on disk. The retry
            // re-walks the identical range from the identical start against a
            // now-warm clone.
            //
            // The alternative, persisting the zeros behind a durable "rebuild
            // me next time" marker, is the trade this codebase refuses
            // everywhere else. A zeroed churn value is indistinguishable from
            // a real one, so until the rebuild lands every health chart,
            // export and leaderboard states with full confidence that these
            // commits touched nothing — and a repository whose objects are
            // unreadable for a systemic reason would keep re-publishing that
            // claim on every pass. Truncated-but-confident is the one outcome
            // worse than falling back.
            // `git log --numstat` exits non-zero for a damaged pack, but also
            // for a child the OOM reaper killed on a loaded co-tenant host.
            // Neither is a fact about the repository, and both meet identical
            // bytes on the next attempt — so the clone goes, and the failure
            // stays revivable. Without the discard, retrying is theatre: eight
            // attempts read the same broken objects and retire a healthy
            // repository for good.
            repo_history::discard_clone(&handle.path).await;
            anyhow::bail!(
                "{}: numstat degraded to a path-only walk after {} of {} commits; \
                 refusing to persist zeroed churn, discarded the clone",
                repo_history::LOCAL_CLONE_MARKER,
                walked_commits,
                plan.shas.len()
            );
        }
        for commit in walked.commits {
            walked_commits += 1;
            if deferred_shas.contains(commit.sha.as_str()) {
                deferred.push(commit);
            } else {
                aggregator.push(&commit);
            }
        }
        update_work_progress(
            &ctx.db,
            beat,
            repo,
            "scanning_history",
            Some(plan.shas.len()),
            walked_commits,
        )
        .await?;
    }
    let mut todo_by_sha = HashMap::with_capacity(todo_shas.len());
    if !todo_shas.is_empty() {
        update_work_progress(
            &ctx.db,
            beat,
            repo,
            "scanning_todos",
            Some(todo_shas.len()),
            0,
        )
        .await?;
        let mut todo_done = 0;
        for batch in todo_shas.chunks(repo_history::LOG_BATCH_COMMITS) {
            // Never let this cost the walk. The TODO scan is one auxiliary
            // signal over the newest few commits and it runs AFTER the entire
            // history has been walked but BEFORE anything is written, so
            // propagating its failure would throw away hours of completed work
            // on a repository the size of linux — to avoid missing a debt
            // marker. The commits simply keep their zeroed markers, and the
            // shortfall is logged rather than persisted as a measurement.
            let patch_commits = match repo_history::walk_commit_batch(&handle, batch).await {
                Ok(commits) => commits,
                Err(error) => {
                    tracing::warn!(
                        repo,
                        %error,
                        scanned = todo_done,
                        of = todo_shas.len(),
                        "TODO/FIXME scan fell short; keeping the completed history walk"
                    );
                    break;
                }
            };
            todo_done += patch_commits.len();
            todo_by_sha.extend(
                patch_commits
                    .into_iter()
                    .map(|commit| (commit.sha, (commit.todo_added, commit.todo_removed))),
            );
            update_work_progress(
                &ctx.db,
                beat,
                repo,
                "scanning_todos",
                Some(todo_shas.len()),
                todo_done,
            )
            .await?;
        }
    }
    for commit in &mut deferred {
        if let Some((added, removed)) = todo_by_sha.get(&commit.sha) {
            commit.todo_added = *added;
            commit.todo_removed = *removed;
        }
    }
    aggregator.extend(deferred.iter());
    let aggregates = aggregator.finish();
    // The commit count now comes from the accumulator rather than from a Vec
    // that outlived the walk purely so it could be measured.
    let n = aggregates.commits_seen();
    update_work_progress(&ctx.db, beat, repo, "saving_history", Some(n), n).await?;
    // The one write that has to be atomic. Both of these functions apply the
    // delta and advance `last_analyzed_sha` to `handle.head_sha` inside a
    // single `repo_stats::write_aggregates_in_tx` transaction, which is what
    // makes a crash here harmless: the aggregates and the cursor roll back
    // together, so the retry re-applies the identical range `(last_sha, HEAD]`
    // from the identical start. Split into two transactions, a crash between
    // them would either double-count the range on retry or skip it forever,
    // and neither is detectable afterwards. It is also why the streaming walk
    // above flushes nothing: a per-batch write would have to advance the
    // cursor per batch to stay crash-safe, and a cursor ahead of a partially
    // applied run is exactly the state that is undetectable afterwards. This
    // phase is likewise the reason the save publishes no intermediate progress
    // and the stall window has to clear it (see
    // [`ANALYSIS_STALL_WINDOW`]) — chunking it to emit progress would
    // be chunking the transaction.
    if replace {
        repo_stats::replace_aggregates_at_head(
            &ctx.db,
            repo,
            &aggregates,
            &handle.head_sha,
            reachable_commits,
        )
        .await?;
    } else {
        repo_stats::apply_aggregates_at_head_with_total(
            &ctx.db,
            repo,
            &aggregates,
            &handle.head_sha,
            reachable_commits,
        )
        .await?;
    }

    // Language counting is the only post-pass on the completion path, and it
    // runs unconditionally — on the incremental path exactly as on the
    // rebuild. Line counts and readiness describe HEAD's *tree*, not the
    // history, so there is no delta to append and no cursor to respect: a run
    // that appended forty commits still has to re-read the tree those forty
    // commits produced. They are also local and cheap, which is why "recompute
    // in full, always" costs nothing worth optimizing away.
    //
    // Author/login enrichment is presentation metadata owned by the bounded
    // background sweep spawned with the pool. Waiting up to 15 seconds for
    // GitHub identity lookups here kept the queue row—and therefore the live
    // "analyzing" UI—open after every repository signal was already saved.
    update_work_progress(&ctx.db, beat, repo, "finishing", Some(n), n).await?;
    if let Err(e) = run_line_counts(&ctx.db, &handle, repo).await {
        tracing::warn!(repo, error = %e, "line counts failed");
    }
    // Re-measure: this run may have fetched, repacked, or run maintenance
    // since the size was taken, and disk is now the only thing bounding an
    // analysis. The quota accountant sums this column, so a stale value means
    // eviction never fires for exactly the repositories that fill the volume.
    repo_stats::record_clone(
        &ctx.db,
        repo,
        &handle.path,
        repo_history::clone_size_bytes(&handle.path),
    )
    .await?;
    // A rebuild covers every reachable commit, so it clears the flag a capped
    // release left behind; an incremental run only appends to the window it
    // inherited and so cannot change what that window covers.
    let scope_truncated = if replace {
        plan.truncated
    } else {
        was_truncated
    };
    repo_stats::record_analysis_details(
        &ctx.db,
        repo,
        i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
        n,
        scope_truncated,
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

/// Publish the run's progress to the queue row *and* to the stall guard.
///
/// Deliberately one call: the progress a visitor sees and the evidence that
/// keeps the run alive must be the same event, or a change to one silently
/// stops feeding the other.
async fn update_work_progress(
    db: &Db,
    beat: &Heartbeat,
    repo: &str,
    phase: &'static str,
    total_units: Option<usize>,
    completed_units: usize,
) -> Result<()> {
    beat.progress(phase, completed_units);
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

/// Persist the repository's language breakdown.
///
/// Two metrics can end up in `repo_lines`: exact line counts, or a file
/// census when the countable content is outside the hydration budget. They are
/// stored with a discriminator rather than being told apart by "all the line
/// columns are zero" — readers rendered that as a confident `0 lines of code`,
/// and profile aggregates summed file counts and line counts into one number.
async fn run_line_counts(db: &Db, handle: &RepoHandle, repo: &str) -> Result<()> {
    // Readiness, the census, and the exact count all describe HEAD's tree, so
    // list it once instead of paying three `git ls-tree -r` walks. The listing
    // stays outside the timeout below: readiness and the census are persisted
    // unconditionally, and a slow repository must not end up with neither.
    let blobs = code_count::head_blobs(&handle.path).await?;
    let readiness = code_count::readiness_from_blobs(&blobs);
    code_count::save_repository_readiness(db, repo, &handle.head_sha, &readiness).await?;
    let (file_census, tree_files) = code_count::census_from_blobs(&blobs);
    // The timeout is a backstop against a pathological local read, not the
    // thing that decides which metric is stored: `count_lines_for` returns
    // `None` by its own deterministic budget, so a repository does not flip
    // between metrics because one run happened to be slower than another.
    let exact = match tokio::time::timeout(
        code_count::exact_line_count_timeout(),
        code_count::count_lines_for(&handle.path, &blobs),
    )
    .await
    {
        Ok(Ok(counts)) => counts,
        Ok(Err(error)) => {
            tracing::warn!(repo, %error, "exact line count failed; using file census");
            None
        }
        Err(_) => {
            tracing::warn!(
                repo,
                tree_files,
                "exact line count timed out; using file census"
            );
            None
        }
    };
    match exact {
        Some(counts) if !counts.is_empty() => {
            let languages = counts.len();
            code_count::save(db, repo, &counts, true).await?;
            tracing::info!(repo, languages, "line counts updated");
        }
        _ => {
            code_count::save(db, repo, &file_census, false).await?;
            tracing::info!(
                repo,
                tree_files,
                languages = file_census.len(),
                "language file census updated"
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Claim tests mutate a table the whole suite shares, so they run one at
    /// a time within this process and restore every row they touch.
    static CLAIM_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn the_pool_split_always_leaves_a_worker_for_general_work() {
        // A pool that reserved every worker would starve the catalog
        // completely rather than merely deprioritizing it.
        for pool in 1..=32usize {
            assert!(reserved_visitor_workers(pool) < pool.max(2));
            assert!(pool - reserved_visitor_workers(pool) >= 1);
        }
        assert_eq!(reserved_visitor_workers(1), 0, "no sharing to do at all");
        assert_eq!(reserved_visitor_workers(2), 1);
        assert_eq!(reserved_visitor_workers(8), 2);
        assert_eq!(reserved_visitor_workers(32), 10);
        // The shape the production pool actually runs: one lane a waiting
        // visitor always finds free, two the catalog may compete for.
        assert_eq!(reserved_visitor_workers(ANALYSIS_WORKERS), 1);
        assert_eq!(general_analysis_workers(), 2);
    }

    /// A cap of zero would not throttle the catalog, it would stop it: no
    /// worker could ever admit a sub-floor row and the backfill would never
    /// run again. And a cap that reached the pool size would not be a cap at
    /// all — backfill could hold every slot, which is the starvation the
    /// reservation exists to prevent.
    #[test]
    fn the_catalog_cap_leaves_headroom_without_ever_wedging_at_zero() {
        for general in 1..=32usize {
            assert!(catalog_cap_for(general) >= 1);
            assert!(catalog_cap_for(general) <= general as i64);
        }
        assert_eq!(catalog_concurrency_cap(), 1);
        assert!(
            catalog_concurrency_cap() < configured_analysis_workers() as i64,
            "the catalog band must never be able to hold the whole pool"
        );
    }

    #[test]
    fn a_reserved_worker_falls_back_only_after_its_own_band_stays_empty() {
        assert_eq!(claim_min_priority(Lane::General, 0), 0);
        assert_eq!(claim_min_priority(Lane::General, 99), 0);
        for polls in 0..RESERVED_FALLBACK_POLLS {
            assert_eq!(
                claim_min_priority(Lane::Visitor, polls),
                VISITOR_PRIORITY_FLOOR,
                "a reserved worker must hold its lane while visitors may arrive"
            );
        }
        assert_eq!(
            claim_min_priority(Lane::Visitor, RESERVED_FALLBACK_POLLS),
            0,
            "and must never idle beside general work that is waiting"
        );
    }

    fn job_after(prior_stalls: u32, best_units: u64) -> AnalysisJob {
        AnalysisJob {
            repo: "owner/repo".into(),
            requested_by_user_id: None,
            prior_stalls,
            best_units,
        }
    }

    fn beat_at(phase: &'static str, units: usize) -> Heartbeat {
        let beat = Heartbeat::default();
        beat.progress(phase, units);
        beat
    }

    /// The whole point of the redesign: a repository that keeps getting
    /// further is not stuck, however long it takes, and must never be parked.
    #[test]
    fn a_run_that_gets_further_than_the_last_one_never_accumulates_a_stall() {
        let silence = Duration::from_secs(900);
        let mut job = job_after(0, 0);
        for units in [5_000usize, 34_000, 120_000] {
            let failure = Failure::stalled(&job, &beat_at("scanning_history", units), silence);
            assert!(
                !failure.park,
                "a repository advancing across attempts must keep its slot: {}",
                failure.message
            );
            let (consecutive, best) = stall_record(Some(failure.message));
            assert_eq!(consecutive, 1, "progress restarts the ladder");
            assert_eq!(best, units as u64);
            job = job_after(consecutive, best);
        }
    }

    /// The blocker this whole lane exists for. The guard's contract is that it
    /// kills silence and never duration, and before the clone took a liveness
    /// callback it could not tell the two apart: the transfer was one await,
    /// so a working 6.1 GB clone looked exactly like a hang and was killed at
    /// the window, every attempt, at the same point.
    ///
    /// The window is injected rather than slept through — that is why
    /// [`guard_progress`] takes it as an argument. What has to be reproduced
    /// is the *ratio*, not the seconds: work beating fifteen times per window
    /// survives six consecutive windows. Against the production 3,600 s window
    /// that is a beat every four minutes sustained for six hours, and
    /// `repo_history` promises one every five.
    #[tokio::test]
    async fn a_phase_that_keeps_beating_is_never_killed_however_long_it_runs() {
        let job = job_after(0, 0);
        let beat = Heartbeat::default();
        // Both margins matter. Fifteen beats per window is what makes a false
        // kill require the executor itself to stall for a whole window, not
        // merely to run late; six windows is what makes the test a statement
        // about duration rather than about one lucky interval.
        let patience = Duration::from_millis(300);
        let step = patience / 15;
        let beats = 90u32;

        let work = async {
            for _ in 0..beats {
                sleep(step).await;
                // No units: a clone completes none. Liveness only.
                beat.phase("cloning");
            }
            Ok::<usize, anyhow::Error>(7)
        };

        match guard_progress(&job, &beat, patience, None, work).await {
            Ok(applied) => assert_eq!(applied, 7),
            Err(failure) => panic!(
                "a run beating every {}ms was killed after {} windows of {}ms: {}",
                step.as_millis(),
                u128::from(beats) * step.as_millis() / patience.as_millis(),
                patience.as_millis(),
                failure.message
            ),
        }
    }

    /// And the other half of the contract: work that stops reporting really is
    /// killed, at the window, naming where it stopped and how far it got.
    #[tokio::test]
    async fn a_phase_that_goes_silent_is_killed_at_the_window() {
        let job = job_after(0, 0);
        let beat = Heartbeat::default();

        let work = async {
            beat.progress("scanning_history", 1_200);
            std::future::pending::<Result<usize>>().await
        };

        let failure = guard_progress(&job, &beat, Duration::from_millis(300), None, work)
            .await
            .expect_err("a wedged run must be ended by this process or not at all");
        assert!(
            failure.message.contains("scanning_history"),
            "the stall must name its phase: {}",
            failure.message
        );
        assert_eq!(stall_record(Some(failure.message)), (1, 1_200));
    }

    /// The dangerous trap, because it does not look like a failure:
    /// `git rev-list <rewritten-sha>..HEAD` exits 0 and prints a plausible
    /// commit list after a force-push or a rebase. Appending it lands fresh
    /// stats on top of the rewritten commits' surviving stats, and the drift
    /// is permanent and undetectable. Only `plan_commits`' cursor validation
    /// separates the two, and this is where its answer has to become
    /// `replace = true`.
    #[test]
    fn an_unusable_cursor_forces_a_full_rebuild() {
        let head = "0123456789abcdef0123456789abcdef01234567";

        assert!(
            !must_rebuild(CURRENT_ANALYSIS_REVISION, Some(head), false),
            "steady state must stay incremental — that is the whole point of the cursor"
        );
        assert!(
            must_rebuild(CURRENT_ANALYSIS_REVISION, Some(head), true),
            "a rejected cursor must rebuild, not append"
        );
        assert!(
            must_rebuild(CURRENT_ANALYSIS_REVISION + 3, Some(head), true),
            "and a future revision does not excuse it either"
        );

        // The reasons that already existed, kept honest alongside the new one.
        // These are the ones that must be visible *before* a plan exists,
        // because they decide whether the cursor is offered for validation.
        assert!(stored_row_needs_rebuild(0, None), "never analyzed");
        assert!(
            stored_row_needs_rebuild(CURRENT_ANALYSIS_REVISION - 1, Some(head)),
            "an older algorithm's aggregates are a different measurement"
        );
        assert!(
            stored_row_needs_rebuild(
                CURRENT_ANALYSIS_REVISION,
                Some(repo_history::EMPTY_REPOSITORY_HEAD)
            ),
            "the empty-repository placeholder is not a commit"
        );
        assert!(
            !stored_row_needs_rebuild(CURRENT_ANALYSIS_REVISION, Some(head)),
            "and an ordinary current row must not"
        );
    }

    /// Retiring a repository for good is reserved for evidence about the
    /// repository. The failures this process inflicts on itself — a stall, an
    /// operator's own wall-clock ceiling, and every `repo_history` budget —
    /// are exactly the ones a very large repository accumulates, and letting
    /// them spend the terminal attempt ceiling made the next deploy unable to
    /// re-arm the row, which is the recovery all of them are documented to
    /// rely on.
    #[test]
    fn only_an_ordinary_error_may_retire_a_repository_permanently() {
        let beat = beat_at("cloning", 0);
        assert!(Failure::transient(&anyhow::anyhow!("clone refused")).terminal);
        assert!(!Failure::over_ceiling(&beat, Duration::from_secs(3_600)).terminal);
        assert!(
            !Failure::stalled(&job_after(0, 0), &beat, Duration::from_secs(3_600)).terminal,
            "a stall is parked by its own ladder and re-armed by the next deploy"
        );
    }

    /// A `repo_history` ceiling lapsing reaches the queue as an ordinary
    /// `Err` from `process`, wrapped in whatever context the call site added.
    /// Eight of them used to write [`TERMINAL_MARKER`], after which
    /// [`revive_retryable_on_startup`] would never re-open the row again —
    /// raising the ceiling and redeploying could not bring the repository
    /// back. The classification therefore has to see through the context
    /// chain, not just the outermost message.
    #[test]
    fn a_self_imposed_budget_lapse_never_retires_a_repository() {
        let lapse = anyhow::anyhow!(
            "{}: clone did not finish in 3600s",
            repo_history::BUDGET_MARKER
        )
        .context("open_or_clone")
        .context("analysis run");
        let failure = Failure::transient(&lapse);
        assert!(
            !failure.terminal,
            "a ceiling this deployment set for itself says nothing about the repository"
        );
        assert!(
            !failure.park,
            "and it must keep taking ordinary attempts in the meantime"
        );
        assert!(
            !failure.message.starts_with(TERMINAL_MARKER),
            "so a deploy still re-arms the row: {}",
            failure.message
        );

        // The distinction is real: a genuine remote failure still spends the
        // ladder and still ends in a permanent retirement.
        assert!(
            Failure::transient(
                &anyhow::anyhow!("git clone failed: repository not found").context("open_or_clone")
            )
            .terminal
        );
    }

    /// A clone this host cannot read is a statement about this host's disk,
    /// not about the repository, so it must stay revivable — and unlike a
    /// ceiling lapse it cannot be fixed by redeploying with a bigger number,
    /// which is why the bail that raises it also discards the clone.
    #[test]
    fn an_unreadable_local_clone_never_retires_a_repository() {
        let unusable = anyhow::anyhow!(
            "{}: numstat degraded to a path-only walk after 1050000 of 1100000 commits",
            repo_history::LOCAL_CLONE_MARKER
        )
        .context("process");
        let failure = Failure::transient(&unusable);
        assert!(
            !failure.terminal,
            "an OOM-killed git child must not permanently retire torvalds/linux"
        );
        assert!(
            !failure.message.starts_with(TERMINAL_MARKER),
            "so a later attempt still re-arms the row: {}",
            failure.message
        );

        // Still distinct from a real answer about the repository.
        assert!(
            Failure::transient(
                &anyhow::anyhow!("git clone failed: repository not found").context("open_or_clone")
            )
            .terminal
        );
    }

    #[test]
    fn the_stall_record_survives_a_round_trip_through_last_error() {
        let silence = Duration::from_secs(900);
        let beat = beat_at("hydrating_window", 500);
        let first = Failure::stalled(&job_after(0, 500), &beat, silence);
        assert_eq!(stall_record(Some(first.message.clone())), (1, 500));
        assert!(
            first.message.contains("hydrating_window"),
            "the operator must be able to see where it stopped: {}",
            first.message
        );

        // An unrelated failure clears the record: "consecutive" is the point.
        let transient = Failure::transient(&anyhow::anyhow!("clone refused"));
        assert_eq!(stall_record(Some(transient.message)), (0, 0));
        assert!(!transient.park);
        assert_eq!(stall_record(None), (0, 0));
        // A row parked by the attempt ceiling still carries its record.
        assert_eq!(
            stall_record(Some(format!(
                "{TERMINAL_MARKER} {STALL_MARKER}3/9000 stuck"
            ))),
            (3, 9_000)
        );
        // The runaway ceiling is not the repository's fault and never counts.
        assert_eq!(
            stall_record(Some(
                Failure::over_ceiling(&beat, ABSOLUTE_JOB_CEILING).message
            )),
            (0, 0)
        );

        // Stuck at the same place, run after run: the count climbs and the
        // slot is eventually released for good — but the message never
        // carries TERMINAL_MARKER, so a deploy still re-arms the row.
        let mut job = job_after(0, 0);
        let mut parked_after = None;
        for attempt in 1..=MAX_ANALYSIS_STALLS {
            let failure = Failure::stalled(&job, &beat, silence);
            let (consecutive, best) = stall_record(Some(failure.message.clone()));
            assert_eq!(
                consecutive, attempt,
                "no progress must not reset the ladder"
            );
            assert!(!failure.message.starts_with(TERMINAL_MARKER));
            if failure.park {
                parked_after = Some(attempt);
                break;
            }
            job = job_after(consecutive, best);
        }
        assert_eq!(
            parked_after,
            Some(MAX_ANALYSIS_STALLS),
            "a repository stuck at the same point must stop reclaiming slots"
        );
    }

    async fn seed(db: &Db, repo: &str, priority: i64, status: &str, claimed: bool) {
        sqlx::query(
            "INSERT INTO repo_analysis_queue \
                (repo, status, phase, priority, enqueued_at, next_attempt_at, claimed_at) \
             VALUES ($1, $2, 'queued', $3, NOW(), NOW(), CASE WHEN $4 THEN NOW() END) \
             ON CONFLICT (repo) DO UPDATE SET \
                status = EXCLUDED.status, priority = EXCLUDED.priority, \
                claimed_at = EXCLUDED.claimed_at, next_attempt_at = EXCLUDED.next_attempt_at",
        )
        .bind(repo)
        .bind(status)
        .bind(priority)
        .bind(claimed)
        .execute(&db.pool)
        .await
        .expect("seed queue row");
    }

    async fn purge(db: &Db, prefix: &str) {
        sqlx::query("DELETE FROM repo_analysis_queue WHERE repo LIKE $1")
            .bind(format!("{prefix}%"))
            .execute(&db.pool)
            .await
            .expect("cleanup queue rows");
    }

    async fn statuses(db: &Db, prefix: &str) -> Vec<(String, String)> {
        sqlx::query_as("SELECT repo, status FROM repo_analysis_queue WHERE repo LIKE $1 ORDER BY 1")
            .bind(format!("{prefix}%"))
            .fetch_all(&db.pool)
            .await
            .expect("read queue rows")
    }

    async fn release(db: &Db, repos: &[String]) {
        sqlx::query(
            "UPDATE repo_analysis_queue SET status = 'pending', worker_id = NULL, \
             claimed_at = NULL, started_at = NULL WHERE repo = ANY($1)",
        )
        .bind(repos)
        .execute(&db.pool)
        .await
        .expect("release claimed rows");
    }

    /// Claim repeatedly through one lane, holding every row it takes so
    /// nothing is claimed twice, until `target` appears or `tries` run out.
    /// Bounded rather than draining: the test database is shared, so a lane
    /// may legitimately hand back rows this test never seeded.
    async fn claim_through(
        db: &Db,
        worker: &str,
        min_priority: i64,
        cap: i64,
        tries: usize,
        target: &str,
    ) -> Vec<String> {
        let mut held = Vec::new();
        for _ in 0..tries {
            match claim_one(db, worker, min_priority, cap)
                .await
                .expect("claim")
            {
                Some(job) => {
                    let found = job.repo == target;
                    held.push(job.repo);
                    if found {
                        break;
                    }
                }
                None => break,
            }
        }
        held
    }

    /// The production shape the reservation exists for: every slot is held by
    /// backfill work and a visitor arrives.
    #[tokio::test]
    async fn a_reserved_worker_claims_past_a_pool_full_of_catalog_work() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let _guard = CLAIM_LOCK.lock().await;
        let prefix = "gitdebt-test-lane/";
        let visitor = format!("{prefix}visitor");
        let catalog: Vec<String> = (0..3).map(|i| format!("{prefix}catalog-{i}")).collect();
        purge(&db, prefix).await;

        // One below the floor: still the catalog band, but ahead of any other
        // sub-floor row a shared test database happens to hold, so what these
        // assertions measure is the lane and not the fixture next door.
        let catalog_priority = VISITOR_PRIORITY_FLOOR - 1;
        for (index, repo) in catalog.iter().enumerate() {
            // The first two are already running, which is what makes every
            // remaining slot unavailable in production.
            let running = index < 2;
            let status = if running { "in_progress" } else { "pending" };
            seed(&db, repo, catalog_priority, status, running).await;
        }
        // Above any real row, for the same reason.
        seed(&db, &visitor, i64::MAX / 2, "pending", false).await;

        let claimed = claim_one(&db, "test-reserved", VISITOR_PRIORITY_FLOOR, 1)
            .await
            .expect("reserved claim")
            .expect("a visitor row is claimable while the catalog holds every slot")
            .repo;
        assert_eq!(
            claimed, visitor,
            "sorting alone would have made this row wait for a slot to free"
        );

        // The reserved lane cannot see the catalog band at all — that is the
        // difference between reserving capacity and ordering a queue.
        let reserved_held =
            claim_through(&db, "test-reserved", VISITOR_PRIORITY_FLOOR, 1, 8, "").await;
        assert!(
            !reserved_held.iter().any(|repo| repo.starts_with(prefix)),
            "a reserved worker claimed catalog work through its own lane: {reserved_held:?}"
        );

        // Two of this test's catalog rows are running, so a cap of two admits
        // no more of them even to a general worker.
        let capped = claim_through(&db, "test-capped", 0, 2, 4, &catalog[2]).await;
        assert!(
            !capped.contains(&catalog[2]),
            "the catalog concurrency cap must be enforced inside the claim itself"
        );

        // Fallback: the same reserved worker, once its own band has been empty
        // for RESERVED_FALLBACK_POLLS, claims the general band it was holding
        // capacity away from.
        let fallback_floor = claim_min_priority(Lane::Visitor, RESERVED_FALLBACK_POLLS);
        let fallback = claim_through(
            &db,
            "test-fallback",
            fallback_floor,
            i64::MAX,
            16,
            &catalog[2],
        )
        .await;
        assert!(
            fallback.contains(&catalog[2]),
            "a reserved worker must fall back rather than idle beside waiting work: {fallback:?}"
        );

        // Hand back every row this test claimed that it did not seed.
        let borrowed: Vec<String> = reserved_held
            .into_iter()
            .chain(capped)
            .chain(fallback)
            .filter(|repo| !repo.starts_with(prefix))
            .collect();
        release(&db, &borrowed).await;
        purge(&db, prefix).await;
    }

    /// A redeploy must not leave the incoming pool accounting for jobs nobody
    /// is running. Both rows here carry a *fresh* lease — the shape a SIGTERM
    /// produces, since the heartbeat ran seconds before the process died — so
    /// the two-minute lease rule reclaims neither, which is exactly why six of
    /// them once sat `in_progress` under a dead pool's `worker_id` for fifteen
    /// minutes, spending catalog-concurrency and queue-capacity budget until a
    /// worker happened to steal them.
    #[tokio::test]
    async fn a_shutdown_hands_back_this_pools_rows_and_only_this_pools_rows() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let _guard = CLAIM_LOCK.lock().await;
        let prefix = "gitdebt-test-release/";
        let mine = format!("{prefix}mine");
        let peer = format!("{prefix}peer");
        purge(&db, prefix).await;

        let seed_claimed = async |repo: &String, worker: String| {
            sqlx::query(
                "INSERT INTO repo_analysis_queue \
                    (repo, status, phase, priority, enqueued_at, claimed_at, worker_id) \
                 VALUES ($1, 'in_progress', 'cloning', 0, NOW(), NOW(), $2)",
            )
            .bind(repo)
            .bind(worker)
            .execute(&db.pool)
            .await
            .expect("seed claimed row");
        };
        seed_claimed(&mine, format!("{}0", pool_worker_prefix())).await;
        seed_claimed(&peer, "ra:another-host:1:1700000000000:0".to_string()).await;

        assert_eq!(
            release_pool_claims(&db).await.unwrap(),
            1,
            "a process may release its own claims and nobody else's"
        );
        assert_eq!(
            statuses(&db, prefix).await,
            vec![
                (mine.clone(), "pending".to_string()),
                (peer.clone(), "in_progress".to_string()),
            ]
        );

        // The SIGKILL path is the lease's job, not an identity check's: a row
        // whose heartbeat stopped is reclaimed once the steal window passes,
        // and a peer that is still beating is never touched no matter whose
        // hostname or pid it shares. Seeded here at both ages so the rule that
        // replaced the identity clause is the one under test.
        purge(&db, prefix).await;
        seed_claimed(&mine, format!("{}0", pool_worker_prefix())).await;
        sqlx::query(
            "UPDATE repo_analysis_queue SET claimed_at = NOW() - INTERVAL '5 minutes' \
             WHERE repo = $1",
        )
        .bind(&mine)
        .execute(&db.pool)
        .await
        .expect("age the abandoned lease");
        seed_claimed(&peer, "ra:another-host:1:1700000000000:0".to_string()).await;
        reset_inflight_on_startup(&db).await.expect("startup reset");
        assert_eq!(
            statuses(&db, prefix).await,
            vec![
                (mine, "pending".to_string()),
                (peer, "in_progress".to_string()),
            ],
            "an expired lease is reclaimed; a peer still inside its window is not"
        );

        purge(&db, prefix).await;
    }

    /// The catalog bootstrap runs on every process start, including a crash
    /// loop's, so it must stay bounded and must not re-arm parked rows.
    #[tokio::test]
    async fn catalog_backfill_is_bounded_and_never_resurrects_a_parked_row() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let prefix = "gitdebt-test-backfill/";
        let repos: Vec<String> = (0..6).map(|i| format!("{prefix}repo-{i}")).collect();
        purge(&db, prefix).await;

        assert_eq!(enqueue_backfill(&db, &repos, 2).await.unwrap(), 2);
        assert_eq!(
            statuses(&db, prefix).await.len(),
            2,
            "a bootstrap must not offer the whole catalog at once"
        );

        // Park one of the remaining repositories the way a repeated timeout
        // does, then confirm a later pass leaves it alone.
        let parked = repos.last().expect("seeded").clone();
        seed(&db, &parked, 0, "dead", false).await;
        let added = enqueue_backfill(&db, &repos, 10).await.unwrap();
        assert_eq!(added, 3, "only the untouched repositories are offered");
        assert_eq!(
            statuses(&db, prefix)
                .await
                .into_iter()
                .find(|(repo, _)| repo == &parked)
                .map(|(_, status)| status),
            Some("dead".to_string()),
            "backfill must never clear a parked row's attempts"
        );
        assert_eq!(
            enqueue_backfill(&db, &repos, 10).await.unwrap(),
            0,
            "and must add nothing once every repository has a row"
        );

        purge(&db, prefix).await;
    }

    /// Parking must not be amnesia: a re-requested repository is runnable
    /// again immediately, but the queue keeps the stall ladder and the
    /// high-water mark, so a wedged repository cannot restart its free
    /// attempts on every visit.
    #[tokio::test]
    async fn re_requesting_a_parked_repository_keeps_what_the_queue_learned() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let prefix = "gitdebt-test-parked/";
        let repo = format!("{prefix}stalls");
        let flaky = format!("{prefix}flaky");
        purge(&db, prefix).await;

        for (slug, error) in [
            (
                &repo,
                format!("{TERMINAL_MARKER} {STALL_MARKER}3/9000 made no progress"),
            ),
            (&flaky, "clone refused".to_string()),
        ] {
            sqlx::query(
                "INSERT INTO repo_analysis_queue \
                    (repo, status, phase, priority, enqueued_at, attempts, last_error) \
                 VALUES ($1, 'dead', 'retrying', 0, NOW(), 7, $2)",
            )
            .bind(slug)
            .bind(error)
            .execute(&db.pool)
            .await
            .expect("seed parked row");
            enqueue_prioritized(&db, slug, INTERACTIVE_PRIORITY, None)
                .await
                .expect("re-request");
        }

        let rows: Vec<(String, String, i32, Option<String>)> = sqlx::query_as(
            "SELECT repo, status, attempts, last_error FROM repo_analysis_queue \
             WHERE repo LIKE $1 ORDER BY 1",
        )
        .bind(format!("{prefix}%"))
        .fetch_all(&db.pool)
        .await
        .expect("read rows");
        for (slug, status, attempts, _) in &rows {
            assert_eq!(status, "pending", "{slug} must be runnable again");
            assert_eq!(*attempts, 0, "{slug} must get its attempts back");
        }
        let error_for = |slug: &str| {
            rows.iter()
                .find(|(repo, ..)| repo == slug)
                .and_then(|(.., error)| error.clone())
        };
        assert_eq!(
            error_for(&flaky),
            None,
            "an ordinary failure says nothing about the next run"
        );
        let kept = error_for(&repo);
        assert_eq!(kept.as_deref(), Some("stall:3/9000"));
        assert_eq!(stall_record(kept), (3, 9_000));

        purge(&db, prefix).await;
    }
}
