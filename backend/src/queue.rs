//! Persistent star-history fetch queue.
//!
//! The browser extension fires `/api/ext/ping` (and the website fires
//! `/analyze`) on every `github.com/owner/repo` a user opens. Those read
//! paths must be non-blocking and budget-safe, so the expensive part —
//! paginating a repo's full stargazer timeline against GitHub's API — is
//! moved off the request and onto this queue. The background worker
//! (`worker.rs`) drains it under the shared `RateLimitTracker`, so the
//! queue *physically cannot* exceed the GitHub budget no matter how many
//! repos are enqueued.
//!
//! Backed by the `star_fetch_queue` table. Mirrors the proven shape of
//! `repo_analysis` (Postgres-backed, `FOR UPDATE SKIP LOCKED` claim) but
//! is a distinct queue because the workload is GitHub-API-bound, not
//! disk/CPU-bound.
//!
//! Invariants:
//!   * **Dedup.** A repo already `pending` or `in_progress` is never
//!     re-enqueued to a second row (PK is the slug). Re-enqueuing a
//!     repo that finished (row gone) starts a fresh job.
//!   * **Priority.** Popularity-first (`view_count` snapshot at enqueue),
//!     then FIFO (`enqueued_at`). Hot repos drain first under a tight
//!     budget.
//!   * **Completeness.** This queue never writes the stargazer cache
//!     directly. The worker does, and only flips `stargazers_complete`
//!     inside the committed write transaction (see `cache.rs`).

use anyhow::Result;
use chrono::Utc;
use sqlx::Row;

use crate::db::Db;

/// Maximum number of attempts before a transiently-failing job is parked in
/// the terminal `dead` status. Without a cap, a repo that fails every claim
/// (e.g. a transient error that's actually permanent) would retry forever,
/// burning the GitHub budget on every pass. 5 attempts with the worker's
/// exponential backoff is enough to ride out a genuine blip.
pub const MAX_ATTEMPTS: i64 = 5;

/// Whether the job should be parked `dead` after this failure, given the
/// attempt count *prior* to the failure. Mirrors the `attempts + 1 >= MAX`
/// guard in [`fail`]'s SQL so the boundary is unit-testable without a DB.
pub fn should_park_dead(attempts_before: i64) -> bool {
    attempts_before + 1 >= MAX_ATTEMPTS
}

/// A claimed job and its resumable backfill cursor.
#[derive(Debug, Clone)]
pub struct Job {
    pub repo: String,
    pub partial: bool,
    pub next_page: u32,
}

/// Enqueue a repo for a star-history fetch at the given priority.
///
/// Dedup: if a row already exists and is `in_progress`, it's left alone
/// (don't yank a job out from under a worker). If it's `pending` we only
/// bump its priority upward (a freshly-hot repo should jump the line) and
/// never reset `partial`/`attempts`. A repo with no row (never enqueued,
/// or finished and the row was deleted) gets a fresh `pending` row.
///
/// `priority` is the caller's popularity snapshot (typically the repo's
/// `view_count`); higher drains first.
pub async fn enqueue(db: &Db, repo: &str, priority: i64) -> Result<()> {
    let now = Utc::now();
    // Dedup + terminal-state protection. A `dead` row is TERMINAL: it was
    // parked either because it exhausted its attempts, is a 404 tombstone, or
    // is `restricted` (durable 403 / stargazer restriction). The extension
    // re-fires enqueue on every page view, so if this upsert flipped `dead`
    // back to `pending` the worker would re-attempt the repo forever, burning
    // the shared GitHub budget on each view — exactly what `fail`/`mark_dead`
    // exist to prevent. Only `in_progress` and `dead` are preserved; anything
    // else (a `pending` row, or a fresh insert) resolves to `pending`.
    sqlx::query(
        "INSERT INTO star_fetch_queue (repo, status, priority, enqueued_at) \
         VALUES ($1, 'pending', $2, $3) \
         ON CONFLICT (repo) DO UPDATE SET \
            priority = GREATEST(star_fetch_queue.priority, EXCLUDED.priority), \
            status = CASE WHEN star_fetch_queue.status IN ('in_progress', 'dead') \
                          THEN star_fetch_queue.status ELSE 'pending' END",
    )
    .bind(repo)
    .bind(priority)
    .bind(now)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// True iff the repo currently has a `pending` or `in_progress` row.
/// Lets the analyze/ping paths report "already queued" without a second
/// enqueue.
pub async fn is_active(db: &Db, repo: &str) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM star_fetch_queue \
         WHERE repo = $1 AND status IN ('pending', 'in_progress')",
    )
    .bind(repo)
    .fetch_one(&db.pool)
    .await?;
    Ok(n > 0)
}

/// Count of jobs not yet finished (`pending` + `in_progress`). Surfaced
/// as the `queued` field on the analyze response so the frontend can show
/// queue depth / progress. `dead` rows are excluded — they're terminal and
/// not "queued" in any meaningful sense.
pub async fn pending_count(db: &Db) -> Result<i64> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM star_fetch_queue WHERE status IN ('pending', 'in_progress')",
    )
    .fetch_one(&db.pool)
    .await?;
    Ok(n)
}

/// Count of currently-`pending` jobs (excludes `in_progress` and `dead`).
/// Used to enforce the global enqueue ceiling so a script spamming pings
/// can't grow the queue unbounded.
pub async fn pending_only_count(db: &Db) -> Result<i64> {
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM star_fetch_queue WHERE status = 'pending'")
            .fetch_one(&db.pool)
            .await?;
    Ok(n)
}

/// True iff the repo has an active job that's a `partial` continuation —
/// i.e. it exceeded the per-attempt page cap and is being backfilled in
/// chunks. Surfaced as the `backfilling` flag on the analyze response.
pub async fn is_backfilling(db: &Db, repo: &str) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM star_fetch_queue \
         WHERE repo = $1 AND partial = TRUE AND status IN ('pending', 'in_progress')",
    )
    .bind(repo)
    .fetch_one(&db.pool)
    .await?;
    Ok(n > 0)
}

/// Whether a repo's star-history job reached a terminal failure. Restricted
/// responses and exhausted transient retries both stop polling; 404s are
/// reported separately through the repo tombstone.
pub async fn history_unavailable(db: &Db, repo: &str) -> Result<bool> {
    let unavailable: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM star_fetch_queue \
         WHERE repo = $1 AND status = 'dead')",
    )
    .bind(repo)
    .fetch_one(&db.pool)
    .await?;
    Ok(unavailable)
}

/// On startup, requeue only expired `in_progress` claims. A rolling deploy can
/// briefly run old and new replicas against the same database; resetting every
/// live claim would create duplicate writers. Fresh claims self-recover through
/// the same 15-minute lease predicate in the claim functions.
pub async fn reset_inflight_on_startup(db: &Db) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE star_fetch_queue SET status = 'pending', worker_id = NULL, claimed_at = NULL \
         WHERE status = 'in_progress' \
           AND (claimed_at IS NULL OR claimed_at < NOW() - INTERVAL '15 minutes')",
    )
    .execute(&db.pool)
    .await?;
    Ok(res.rows_affected())
}

/// Atomically claim the highest-priority pending job for `worker_id`.
/// `FOR UPDATE SKIP LOCKED` lets multiple workers claim distinct rows
/// without blocking each other. Ordered popularity-first then FIFO.
pub async fn claim_one(db: &Db, worker_id: &str) -> Result<Option<Job>> {
    let now = Utc::now();
    let row = sqlx::query(
        "UPDATE star_fetch_queue \
         SET status = 'in_progress', worker_id = $1, claimed_at = $2 \
         WHERE repo = ( \
            SELECT repo FROM star_fetch_queue \
            WHERE status = 'pending' \
               OR (status = 'in_progress' AND claimed_at < NOW() - INTERVAL '15 minutes') \
            ORDER BY priority DESC, enqueued_at \
            FOR UPDATE SKIP LOCKED LIMIT 1 \
         ) \
         RETURNING repo, partial, next_page",
    )
    .bind(worker_id)
    .bind(now)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(|r| Job {
        repo: r.try_get::<String, _>("repo").unwrap_or_default(),
        partial: r.try_get::<bool, _>("partial").unwrap_or(false),
        next_page: r
            .try_get::<i64, _>("next_page")
            .unwrap_or(1)
            .clamp(1, u32::MAX as i64) as u32,
    }))
}

/// Claim a bounded batch for a GH Archive query. A single BigQuery scan can
/// serve many repositories, so the archive coordinator uses this instead of
/// multiplying identical corpus scans across `WORKER_COUNT` tasks.
pub async fn claim_many(db: &Db, worker_id: &str, limit: usize) -> Result<Vec<Job>> {
    let limit = i64::try_from(limit.clamp(1, 1_000)).unwrap_or(1_000);
    let rows = sqlx::query(
        "WITH selected AS ( \
            SELECT repo FROM star_fetch_queue \
            WHERE status = 'pending' \
               OR (status = 'in_progress' AND claimed_at < NOW() - INTERVAL '15 minutes') \
            ORDER BY priority DESC, enqueued_at \
            FOR UPDATE SKIP LOCKED LIMIT $1 \
         ) \
         UPDATE star_fetch_queue AS queue \
         SET status = 'in_progress', worker_id = $2, claimed_at = $3 \
         FROM selected WHERE queue.repo = selected.repo \
         RETURNING queue.repo, queue.partial, queue.next_page",
    )
    .bind(limit)
    .bind(worker_id)
    .bind(Utc::now())
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Job {
            repo: row.try_get::<String, _>("repo").unwrap_or_default(),
            partial: row.try_get::<bool, _>("partial").unwrap_or(false),
            next_page: row
                .try_get::<i64, _>("next_page")
                .unwrap_or(1)
                .clamp(1, u32::MAX as i64) as u32,
        })
        .collect())
}

/// Release a successfully advanced archive backfill for its next date window.
/// Unlike a transient failure, progress does not consume the retry budget.
pub async fn requeue_archive_window(db: &Db, repo: &str) -> Result<()> {
    sqlx::query(
        "UPDATE star_fetch_queue SET partial = TRUE, worker_id = NULL, \
            claimed_at = NULL, status = 'pending', last_error = NULL \
         WHERE repo = $1",
    )
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Re-open only jobs parked due to the retired GitHub stargazer-list
/// restriction. Generic exhausted failures and 404 tombstones remain terminal.
pub async fn revive_restricted_for_archive(db: &Db) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE star_fetch_queue SET status = 'pending', attempts = 0, \
            partial = FALSE, next_page = 1, worker_id = NULL, claimed_at = NULL \
         WHERE status = 'dead' AND last_error LIKE $1",
    )
    .bind(format!("{RESTRICTED_MARKER}%"))
    .execute(&db.pool)
    .await?;
    Ok(result.rows_affected())
}

/// Mark a job done by deleting its row. The stargazer cache was already
/// committed complete by the worker; the queue row is just bookkeeping.
pub async fn complete(db: &Db, repo: &str) -> Result<()> {
    sqlx::query("DELETE FROM star_fetch_queue WHERE repo = $1")
        .bind(repo)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// Re-queue a job that hit the per-attempt page cap before finishing the
/// whole stargazer list. Successful chunks do not consume the transient
/// failure budget: the cursor advances and the row returns to `pending`.
pub async fn requeue_partial(db: &Db, repo: &str, next_page: u32) -> Result<()> {
    sqlx::query(
        "UPDATE star_fetch_queue SET partial = TRUE, \
            next_page = $1, worker_id = NULL, claimed_at = NULL, status = 'pending' \
         WHERE repo = $2",
    )
    .bind(i64::from(next_page.max(1)))
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Record a transient failure: bump `attempts`, store the error, and
/// return the row to `pending` so a later claim retries it (the worker
/// applies exponential backoff between attempts). `partial` is preserved
/// so a continuation that errors stays a continuation.
///
/// Once `attempts` reaches [`MAX_ATTEMPTS`] the row is parked in the
/// terminal `dead` status instead of `pending`, so a permanently-failing
/// repo stops consuming the GitHub budget. `claim_one` only selects
/// `pending` rows, so `dead` rows are never picked up again (they're kept
/// for debugging / the metrics surface). Returns `true` iff the job was
/// parked dead.
pub async fn fail(db: &Db, repo: &str, err: &str) -> Result<bool> {
    let new_status: String = sqlx::query_scalar(
        "UPDATE star_fetch_queue SET \
            attempts = attempts + 1, \
            last_error = $1, \
            worker_id = NULL, \
            claimed_at = NULL, \
            status = CASE WHEN attempts + 1 >= $2 THEN 'dead' ELSE 'pending' END \
         WHERE repo = $3 \
         RETURNING status",
    )
    .bind(err)
    .bind(MAX_ATTEMPTS)
    .bind(repo)
    .fetch_optional(&db.pool)
    .await?
    .unwrap_or_else(|| "pending".to_string());
    Ok(new_status == "dead")
}

/// Park a job in the terminal `dead` status immediately, without bumping
/// attempts toward the cap. Used for *permanent* failures the worker can
/// recognize up front — chiefly GitHub `NotFound` (private/deleted/typo'd
/// repo): retrying can never succeed, so we stop now rather than burning
/// five attempts. The caller also tombstones the repo (`mark_repo_missing`)
/// so the enqueue paths short-circuit.
pub async fn mark_dead(db: &Db, repo: &str, err: &str) -> Result<()> {
    sqlx::query(
        "UPDATE star_fetch_queue SET status = 'dead', last_error = $1, \
            worker_id = NULL, claimed_at = NULL \
         WHERE repo = $2",
    )
    .bind(err)
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Sentinel `last_error` prefix for a repo parked because GitHub is serving
/// an empty or forbidden stargazer response (the 2026-06-30 restriction, or
/// a durable 403). The repo is **not** missing — it exists, we just can't
/// read its stargazers — so it is parked `dead` WITHOUT a `repos.missing`
/// tombstone. Callers/readers (the API surface) can distinguish this from a
/// genuine 404 by matching this prefix on a `dead` row whose `repos.missing`
/// is `FALSE`, and present it as "restricted, not missing".
pub const RESTRICTED_MARKER: &str = "restricted:";

/// Park a repo `dead` because its stargazer list is unavailable (empty-200
/// or durable 403). Same terminal effect as [`mark_dead`] — the job stops
/// consuming GitHub budget on every view — but stamps the [`RESTRICTED_MARKER`]
/// so the state reads as "restricted" rather than "failed/missing". The
/// caller must NOT tombstone the repo as `missing`.
pub async fn mark_restricted(db: &Db, repo: &str, detail: &str) -> Result<()> {
    let err = format!("{RESTRICTED_MARKER} {detail}");
    mark_dead(db, repo, &err).await
}

/// True iff a `last_error` string denotes the restricted park (see
/// [`RESTRICTED_MARKER`]). Pure so the classification is unit-testable.
pub fn is_restricted_error(last_error: &str) -> bool {
    last_error.starts_with(RESTRICTED_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn park_dead_at_cap() {
        // Attempts 0..=3 (→ 1..=4 after the bump) stay pending; the 5th
        // failure (attempts_before = 4 → 5) crosses MAX_ATTEMPTS and parks.
        assert!(!should_park_dead(0));
        assert!(!should_park_dead(1));
        assert!(!should_park_dead(2));
        assert!(!should_park_dead(3));
        assert!(should_park_dead(4));
        assert!(should_park_dead(5));
        assert!(should_park_dead(100));
    }

    #[test]
    fn max_attempts_is_five() {
        assert_eq!(MAX_ATTEMPTS, 5);
    }

    #[test]
    fn restricted_marker_roundtrips() {
        // A restricted park stamps a recognizable, prefix-matchable error so
        // the API can tell "restricted, not missing" from a plain failure.
        let err = format!("{RESTRICTED_MARKER} stargazers unavailable (403)");
        assert!(is_restricted_error(&err));
        // A generic failure is NOT classified restricted.
        assert!(!is_restricted_error("http error: connection reset"));
        assert!(!is_restricted_error("repo not found: o/r"));
    }
}
