//! Persistent star-history fetch queue.
//!
//! The browser extension fires `/api/ext/ping` (and the website fires
//! `/analyze`) on every `github.com/owner/repo` a user opens. Those read
//! paths must be non-blocking and budget-safe, so the GitHub part — reading a
//! repository's whole star history — is moved off the request and onto this
//! queue. The background worker (`worker.rs`) drains it under the shared
//! `RateLimitTracker`, so the queue *physically cannot* exceed the GitHub
//! budget no matter how many repos are enqueued.
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

/// Durable retry delay for a repository-specific transient failure.
///
/// Jobs never become terminal merely because an upstream service or the
/// network failed repeatedly. The delay grows from 30 seconds to one hour,
/// which bounds request spend without lying to readers that history can never
/// arrive.
pub fn retry_delay_seconds(attempts_before: i64) -> i64 {
    let shift = attempts_before.clamp(0, 7) as u32;
    (30_i64.saturating_mul(1_i64 << shift)).min(3_600)
}

/// A claimed job and its resumable backfill cursor.
#[derive(Debug, Clone)]
pub struct Job {
    pub repo: String,
    pub partial: bool,
    pub next_page: u32,
}

/// Queue state needed by one `/analyze` response, loaded in one round trip.
#[derive(Debug, Clone, Copy, Default)]
pub struct RepoQueueStatus {
    pub queued: i64,
    pub backfilling: bool,
    pub retrying: bool,
    /// The row is parked because GitHub will not serve this repository's
    /// stargazers to gitdebt, not because anything failed.
    ///
    /// Distinct from `retrying` and load-bearing for the caller: a restricted
    /// park is terminal until something changes outside gitdebt, so a client
    /// told "retrying" polls forever for a retry that is never scheduled.
    /// Since GitHub limited the stargazers endpoint to a repository's own
    /// admins and collaborators (July 2026), this is the ordinary state of
    /// every repository the configured credential does not administer — not
    /// an error worth reporting as one.
    pub restricted: bool,
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
///
/// A repository whose GitHub star history was read within
/// [`crate::analyzer::STARGAZER_REFRESH_TTL`] is not enqueued at all: some
/// callers (sign-in warm-up, the metadata backfill) offer repositories without
/// checking freshness first, and re-reading a history that cannot have moved
/// would spend budget for nothing.
pub async fn enqueue(db: &Db, repo: &str, priority: i64) -> Result<()> {
    let now = Utc::now();
    // A `dead` row is reserved for a confirmed permanent condition (a 404
    // tombstone, or a private repository parked restricted). Transient
    // failures stay pending with a durable `next_attempt_at`.
    sqlx::query(
        "INSERT INTO star_fetch_queue (repo, status, priority, enqueued_at) \
         SELECT $1, 'pending', $2, $3 \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM repos \
             WHERE repo = $1 AND history_complete = TRUE \
               AND history_source = 'github_history' \
               AND stargazers_fetched_at > $4 \
               AND metadata_fetched_at IS NOT NULL \
         ) \
         ON CONFLICT (repo) DO UPDATE SET \
            priority = GREATEST(star_fetch_queue.priority, EXCLUDED.priority), \
            status = CASE WHEN star_fetch_queue.status IN ('in_progress', 'dead') \
                          THEN star_fetch_queue.status ELSE 'pending' END",
    )
    .bind(repo)
    .bind(priority)
    .bind(now)
    .bind(now - crate::analyzer::STARGAZER_REFRESH_TTL)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Set-based deployment bootstrap for a trusted, already-validated list of
/// repository slugs. Only cold or stale histories are offered; fresh rows,
/// tombstones, and terminal queue rows are left untouched. One statement
/// handles the whole catalog, avoiding hundreds of request-path round trips
/// and public rate-limit checks during a deploy.
pub async fn enqueue_cold_or_stale_many(db: &Db, repos: &[String], priority: i64) -> Result<u64> {
    if repos.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query(
        "WITH requested(repo) AS ( \
             SELECT DISTINCT UNNEST($1::TEXT[]) \
         ), eligible AS ( \
             SELECT requested.repo \
             FROM requested \
             LEFT JOIN repos cached ON cached.repo = requested.repo \
             WHERE cached.repo IS NULL \
                OR (cached.missing = FALSE AND ( \
                    cached.history_complete = FALSE \
                    OR cached.history_source IS DISTINCT FROM 'github_history' \
                    OR cached.stargazers_fetched_at IS NULL \
                    OR cached.stargazers_fetched_at < NOW() - INTERVAL '6 hours' \
                )) \
         ) \
         INSERT INTO star_fetch_queue (repo, status, priority, enqueued_at) \
         SELECT repo, 'pending', $2, NOW() FROM eligible \
         ON CONFLICT (repo) DO UPDATE SET \
            priority = GREATEST(star_fetch_queue.priority, EXCLUDED.priority) \
         WHERE star_fetch_queue.status = 'pending' \
           AND star_fetch_queue.priority < EXCLUDED.priority",
    )
    .bind(repos)
    .bind(priority.max(0))
    .execute(&db.pool)
    .await?;
    Ok(result.rows_affected())
}

/// The background sweep's selection, hoisted so a test can read its clauses.
///
/// Two kinds of repository, in this order:
///
///  1. Published histories that are not GitHub's star history yet: a
///     stargazer-list snapshot frozen by the July 2026 restriction, or an
///     approximate archive or spliced series. Each is strictly improved by one
///     read, so they drain first, most-viewed first.
///  2. GitHub star histories last read before `$2`, oldest read first among
///     equally viewed repositories.
///
/// Only repositories with published history and public metadata: a cold
/// repository is read when somebody asks for it, not speculatively. A
/// repository holding a queue row of any status is skipped — pending and
/// in-progress work is already handled, and a `dead` park is a decision this
/// sweep must not reverse.
const STAR_HISTORY_SWEEP_SQL: &str = "WITH eligible AS ( \
         SELECT repo FROM repos \
         WHERE history_complete = TRUE \
           AND NOT missing \
           AND metadata_fetched_at IS NOT NULL \
           AND ( \
               history_source IS DISTINCT FROM 'github_history' \
               OR stargazers_fetched_at IS NULL \
               OR stargazers_fetched_at < $2 \
           ) \
           AND NOT EXISTS ( \
               SELECT 1 FROM star_fetch_queue q WHERE q.repo = repos.repo \
           ) \
         ORDER BY (history_source IS NOT DISTINCT FROM 'github_history'), \
                  view_count DESC, stargazers_fetched_at NULLS FIRST, repo \
         LIMIT $1 \
     ) \
     INSERT INTO star_fetch_queue (repo, status, priority, enqueued_at) \
     SELECT repo, 'pending', 0, NOW() FROM eligible \
     ON CONFLICT (repo) DO NOTHING \
     RETURNING repo";

/// One bounded pass of the background star-history sweep: see
/// [`STAR_HISTORY_SWEEP_SQL`] for what it selects. Priority 0, so anything a
/// visitor is waiting on drains first. Returns the repositories enqueued.
pub async fn enqueue_star_history_sweep(
    db: &Db,
    limit: i64,
    stale_before: chrono::DateTime<Utc>,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(STAR_HISTORY_SWEEP_SQL)
        .bind(limit.clamp(1, 5_000))
        .bind(stale_before)
        .fetch_all(&db.pool)
        .await?)
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
/// queue depth / progress.
pub async fn pending_count(db: &Db) -> Result<i64> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM star_fetch_queue WHERE status IN ('pending', 'in_progress')",
    )
    .fetch_one(&db.pool)
    .await?;
    Ok(n)
}

/// The one-round-trip projection behind [`RepoQueueStatus`], named so a test
/// can assert which statuses each subquery reads.
const REPO_STATUS_SQL: &str = "SELECT \
            (SELECT COUNT(*)::BIGINT FROM star_fetch_queue \
             WHERE status IN ('pending', 'in_progress')), \
            EXISTS(SELECT 1 FROM star_fetch_queue \
             WHERE repo = $1 AND partial = TRUE \
               AND status IN ('pending', 'in_progress')), \
            EXISTS(SELECT 1 FROM star_fetch_queue \
             WHERE repo = $1 AND status IN ('pending', 'in_progress') \
               AND (attempts > 0 OR last_error LIKE $2)), \
            EXISTS(SELECT 1 FROM star_fetch_queue \
             WHERE repo = $1 AND status = 'dead' AND last_error LIKE $3)";

pub async fn repo_status(db: &Db, repo: &str) -> Result<RepoQueueStatus> {
    let row: (i64, bool, bool, bool) = sqlx::query_as(REPO_STATUS_SQL)
        .bind(repo)
        .bind(format!("{PROVIDER_MARKER}%"))
        // The restricted park is `dead`, so it is deliberately NOT constrained to
        // the pending/in_progress statuses the three counters above use.
        .bind(format!("{RESTRICTED_MARKER}%"))
        .fetch_one(&db.pool)
        .await?;
    Ok(RepoQueueStatus {
        queued: row.0,
        backfilling: row.1,
        retrying: row.2,
        restricted: row.3,
    })
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

/// True when an active job has already failed at least once or was released
/// by the shared archive provider circuit breaker. Public responses use this
/// to distinguish a normal queue wait from a delayed retry without exposing
/// internal error text.
pub async fn is_retrying(db: &Db, repo: &str) -> Result<bool> {
    let retrying: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM star_fetch_queue \
         WHERE repo = $1 AND status IN ('pending', 'in_progress') \
           AND (attempts > 0 OR last_error LIKE $2))",
    )
    .bind(repo)
    .bind(format!("{PROVIDER_MARKER}%"))
    .fetch_one(&db.pool)
    .await?;
    Ok(retrying)
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
            WHERE (status = 'pending' AND next_attempt_at <= NOW()) \
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
    let limit = i64::try_from(limit.clamp(1, 5_000)).unwrap_or(5_000);
    let rows = sqlx::query(
        "WITH selected AS ( \
            SELECT repo FROM star_fetch_queue \
            WHERE (status = 'pending' AND next_attempt_at <= NOW()) \
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
            claimed_at = NULL, status = 'pending', attempts = 0, \
            next_attempt_at = NOW(), last_error = NULL \
         WHERE repo = $1",
    )
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Re-open every non-missing job parked `dead`, once per worker start.
///
/// Almost every such park was made against a source that no longer decides
/// anything: older releases parked repositories `restricted` when GitHub
/// closed the stargazer list, and parked whole batches after shared BigQuery
/// failures. GitHub's star history serves every public repository, so those
/// jobs can now succeed. A private repository simply parks again after one
/// metadata lookup; confirmed 404 tombstones remain terminal.
pub async fn revive_parked_on_startup(db: &Db) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE star_fetch_queue SET status = 'pending', attempts = 0, \
            partial = FALSE, next_page = 1, next_attempt_at = NOW(), \
            worker_id = NULL, claimed_at = NULL, last_error = NULL \
         WHERE status = 'dead' \
           AND NOT EXISTS ( \
               SELECT 1 FROM repos \
               WHERE repos.repo = star_fetch_queue.repo AND repos.missing = TRUE \
           )",
    )
    .execute(&db.pool)
    .await?;
    Ok(result.rows_affected())
}

/// Release a whole archive batch after a shared provider/query failure.
///
/// The failure is not repository-specific, so it must not consume per-repo
/// attempts. Keeping the next attempt durable prevents a restart from turning a
/// provider outage into a hot loop.
pub async fn release_archive_provider_error(
    db: &Db,
    repos: &[String],
    err: &str,
    delay_seconds: i64,
) -> Result<u64> {
    if repos.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query(
        "UPDATE star_fetch_queue SET status = 'pending', worker_id = NULL, \
            claimed_at = NULL, last_error = $1, \
            next_attempt_at = NOW() + $2 * INTERVAL '1 second' \
         WHERE repo = ANY($3)",
    )
    .bind(format!("{PROVIDER_MARKER} {err}"))
    .bind(delay_seconds.clamp(1, 3_600))
    .bind(repos)
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
            next_page = $1, worker_id = NULL, claimed_at = NULL, status = 'pending', \
            attempts = 0, next_attempt_at = NOW(), last_error = NULL \
         WHERE repo = $2",
    )
    .bind(i64::from(next_page.max(1)))
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Record a repository-specific transient failure and schedule a durable,
/// exponentially delayed retry. Only explicit permanent classifications use
/// [`mark_dead`].
pub async fn fail(db: &Db, repo: &str, err: &str) -> Result<()> {
    sqlx::query(
        "UPDATE star_fetch_queue SET \
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
pub const PROVIDER_MARKER: &str = "provider:";

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
    fn transient_retry_delay_is_bounded() {
        assert_eq!(retry_delay_seconds(0), 30);
        assert_eq!(retry_delay_seconds(1), 60);
        assert_eq!(retry_delay_seconds(6), 1_920);
        assert_eq!(retry_delay_seconds(7), 3_600);
        assert_eq!(retry_delay_seconds(100), 3_600);
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

    /// The restricted probe must look at `dead` rows, and no other counter may.
    ///
    /// The three original counters in `repo_status` all constrain themselves to
    /// `('pending', 'in_progress')`, because they describe work in flight. A
    /// restricted park is the opposite: `mark_restricted` writes `dead`, so a
    /// probe carrying the same status constraint would never see one and every
    /// restricted repository would keep reporting "queued" forever.
    #[test]
    fn restricted_probe_reads_parked_rows_not_in_flight_ones() {
        let sql = REPO_STATUS_SQL;
        let restricted_clause = sql
            .split("EXISTS(")
            .nth(3)
            .expect("repo_status has a fourth subquery for the restricted park");
        assert!(
            restricted_clause.contains("status = 'dead'"),
            "the restricted probe must read parked rows: {restricted_clause}"
        );
        assert!(
            !restricted_clause.contains("'in_progress'"),
            "a restricted park is never in flight: {restricted_clause}"
        );
        // And the in-flight counters must not have drifted onto dead rows.
        for (index, clause) in sql.split("EXISTS(").enumerate().skip(1).take(2) {
            assert!(
                clause.contains("'pending', 'in_progress'"),
                "subquery {index} describes work in flight: {clause}"
            );
        }
    }

    /// The sweep upgrades every non-GitHub history before it refreshes any
    /// GitHub one, never touches a repository nobody has published, and never
    /// overrides an existing queue row of any status.
    #[test]
    fn sweep_upgrades_first_and_respects_existing_rows() {
        let sql = STAR_HISTORY_SWEEP_SQL;
        for clause in [
            "history_complete = TRUE",
            "NOT missing",
            "metadata_fetched_at IS NOT NULL",
            "history_source IS DISTINCT FROM 'github_history'",
            "stargazers_fetched_at < $2",
            "NOT EXISTS",
            "ON CONFLICT (repo) DO NOTHING",
        ] {
            assert!(sql.contains(clause), "sweep is missing `{clause}`");
        }
        // FALSE sorts before TRUE: upgrades drain ahead of refreshes.
        assert!(sql.contains("ORDER BY (history_source IS NOT DISTINCT FROM 'github_history')"));
        let selection = sql
            .split("INSERT INTO")
            .next()
            .expect("the selection precedes the insert");
        assert!(
            !selection.contains("status"),
            "a queue row of any status must block the sweep"
        );
    }

    /// Against Postgres: the sweep offers upgrades and stale refreshes, and
    /// leaves fresh, cold, tombstoned and already-queued repositories alone.
    #[tokio::test]
    async fn sweep_offers_upgrades_and_stale_refreshes_only() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let prefix = format!("gitdebt-sweep-test-{}", std::process::id());
        let frozen = format!("{prefix}/frozen");
        let spliced = format!("{prefix}/spliced");
        let stale = format!("{prefix}/stale");
        let fresh = format!("{prefix}/fresh");
        let cold = format!("{prefix}/cold");
        let tombstoned = format!("{prefix}/tombstoned");
        let queued = format!("{prefix}/queued");
        let all = [
            frozen.clone(),
            spliced.clone(),
            stale.clone(),
            fresh.clone(),
            cold.clone(),
            tombstoned.clone(),
            queued.clone(),
        ];
        clear_migration_fixture(&db, &all).await;

        for (repo, source, complete, missing, read_hours_ago) in [
            (&frozen, Some("github_api"), true, false, None),
            (&spliced, Some("spliced"), true, false, None),
            (&stale, Some("github_history"), true, false, Some(48)),
            (&fresh, Some("github_history"), true, false, Some(1)),
            (&cold, None, false, false, None),
            (&tombstoned, Some("github_api"), true, true, None),
            (&queued, Some("github_api"), true, false, None),
        ] {
            sqlx::query(
                "INSERT INTO repos (repo, github_id, star_count, metadata_fetched_at, \
                    history_complete, history_source, missing, stargazers_fetched_at, \
                    history_splice_at, history_splice_position) \
                 VALUES ($1, 777, 10, NOW(), $2, $3, $4, \
                    NOW() - $5 * INTERVAL '1 hour', \
                    CASE WHEN $3 = 'spliced' THEN NOW() END, \
                    CASE WHEN $3 = 'spliced' THEN 1 END)",
            )
            .bind(repo)
            .bind(complete)
            .bind(source)
            .bind(missing)
            .bind(read_hours_ago.map(f64::from))
            .execute(&db.pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO star_fetch_queue (repo, status, priority, enqueued_at) \
             VALUES ($1, 'dead', 0, NOW())",
        )
        .bind(&queued)
        .execute(&db.pool)
        .await
        .unwrap();

        let swept =
            enqueue_star_history_sweep(&db, 5_000, Utc::now() - chrono::Duration::hours(24))
                .await
                .unwrap();
        // The sweep is global by design. Hand back every row it added outside
        // this fixture, or another suite sharing the database claims them.
        let outside: Vec<&String> = swept
            .iter()
            .filter(|repo| !repo.starts_with(&prefix))
            .collect();
        sqlx::query("DELETE FROM star_fetch_queue WHERE repo = ANY($1)")
            .bind(&outside)
            .execute(&db.pool)
            .await
            .unwrap();
        let mut offered: Vec<String> = sqlx::query_scalar(
            "SELECT repo FROM star_fetch_queue WHERE repo = ANY($1) AND status = 'pending'",
        )
        .bind(&all[..])
        .fetch_all(&db.pool)
        .await
        .unwrap();
        offered.sort();
        let mut expected = vec![frozen.clone(), spliced.clone(), stale.clone()];
        expected.sort();
        assert_eq!(offered, expected);

        // A fresh GitHub star history is never re-enqueued by an ordinary
        // offer either; a stale or legacy one is.
        sqlx::query("DELETE FROM star_fetch_queue WHERE repo = ANY($1)")
            .bind(&all[..])
            .execute(&db.pool)
            .await
            .unwrap();
        enqueue(&db, &fresh, 1).await.unwrap();
        enqueue(&db, &frozen, 1).await.unwrap();
        let offered: Vec<String> =
            sqlx::query_scalar("SELECT repo FROM star_fetch_queue WHERE repo = ANY($1)")
                .bind(&all[..])
                .fetch_all(&db.pool)
                .await
                .unwrap();
        assert_eq!(offered, vec![frozen.clone()]);

        clear_migration_fixture(&db, &all).await;
    }

    async fn clear_migration_fixture(db: &Db, repos: &[String]) {
        for statement in [
            "DELETE FROM star_fetch_queue WHERE repo = ANY($1)",
            "DELETE FROM repos WHERE repo = ANY($1)",
        ] {
            sqlx::query(statement)
                .bind(repos)
                .execute(&db.pool)
                .await
                .unwrap();
        }
    }
}
