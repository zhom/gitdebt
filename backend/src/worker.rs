//! Background star-history workers.
//!
//! Drains `star_fetch_queue` (see `queue.rs`): claims a repository, reads its
//! whole star history from GitHub's own star-history endpoint
//! (`GET /repos/{owner}/{repo}/stargazers/history`), and writes it through the
//! cache, honoring the `*_complete` invariant (the series flips readable only
//! inside the committed write transaction; never on a rate-limit or error
//! path).
//!
//! That endpoint is public, needs no credential, names no stargazer, and is
//! exact: it counts the repository's current stargazers by the day they
//! starred, back to the creation week. It replaced the stargazer list, which
//! GitHub limited to a repository's own admins and collaborators in July 2026
//! and which gitdebt no longer reads at all. A read costs one request per
//! thirty weeks of the repository's life, so a whole history is re-read on
//! every refresh rather than patched: that is what keeps an unstar of a
//! years-old star out of the curve as well as a new star in it.
//!
//! Budget safety: every GitHub call routes through
//! `GithubClient::send` → `RateLimitTracker::acquire`, which *blocks*
//! until the per-token budget has headroom (and honors `Retry-After` on
//! secondary limits). Each request reserves budget before it is sent.
//!
//! Exponential backoff on transient errors.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::time::sleep;

use crate::cache::Cache;
use crate::db::Db;
use crate::github::{GithubClient, GithubError, RepoVisibility};
use crate::queue;
use crate::star_history;

/// Backoff ceiling for transient errors (1, 2, 4, … capped at 32s),
/// matching the AGENTS.md-documented schedule.
const BACKOFF_CAP_SECS: u64 = 32;

/// Workers this pool runs, whatever the caller asks for.
///
/// Every worker here contends with the repo-analysis pool for the same 12
/// vCPU, the same disk, and the same Postgres. A history read is a handful of
/// GitHub round-trips followed by one small write (one row per day with
/// stars), so a few workers keep the queue draining while one waits on GitHub
/// and another commits. GitHub's own per-token budget, not this number, is
/// what bounds throughput.
const MAX_STAR_FETCH_WORKERS: usize = 4;

#[derive(Clone)]
pub struct WorkerCtx {
    pub github: Arc<GithubClient>,
    pub cache: Cache,
}

impl WorkerCtx {
    pub fn new(github: Arc<GithubClient>, cache: Cache) -> Self {
        Self { github, cache }
    }
}

/// Repos one metadata-backfill sweep pass may enqueue. Bounds both the
/// per-pass GitHub metadata spend (one metadata call per repo when the claim
/// path processes it) and the queue growth from a single sweep.
///
/// A hundred is deliberately modest: this is one-off repair of legacy rows
/// with no deadline on it, running hourly against a database that interactive
/// analysis writes to, and a backlog that drains over a day of passes is
/// indistinguishable to any reader from one that drains in an afternoon.
const METADATA_BACKFILL_BATCH: i64 = 100;

/// Sweep cadence: one pass at startup, then hourly.
const METADATA_BACKFILL_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Gap between the individual enqueues of one backfill pass.
///
/// The candidate query is one statement, but the enqueues are one round trip
/// each, and firing a hundred of them back to back takes a pooled connection
/// and a burst of WAL from whatever analysis is committing aggregates at that
/// moment. At 50ms the whole pass is spread over ~5s of an hour-long cycle,
/// which is invisible to the backlog and invisible to Postgres.
const BACKFILL_ENQUEUE_PACING: Duration = Duration::from_millis(50);

/// Global ceiling on `pending` star-fetch rows: past this many the sweep
/// enqueues nothing and waits for its next pass. Mirrors the analyze path's
/// own ceiling in `analyzer`.
///
/// This bounds queue *depth*, not concurrency: a pending row is ~100 bytes and
/// costs nothing until a worker claims it, so the number is generous on
/// purpose. What protects the host is [`MAX_STAR_FETCH_WORKERS`]; what this
/// protects is the queue table from unbounded growth when acquisition is
/// slower than enqueue for a long stretch.
pub(crate) const MAX_PENDING_FETCHES: i64 = 5_000;

/// One pass of the profile-stats metadata backfill sweep.
///
/// Rows ingested before the public-metadata read gate existed have complete
/// history but `metadata_fetched_at IS NULL`, which makes them invisible to
/// every reader (user cards, aggregates, exports) with nothing on the read
/// path allowed to heal them. This sweep re-enqueues them into the durable
/// `star_fetch_queue`; the claim path writes metadata via `put_repo_metadata`
/// before touching any history, so healing costs one metadata call per repo
/// plus one short star-history read.
///
/// Bounded per pass ([`METADATA_BACKFILL_BATCH`]) and paced row by row
/// ([`BACKFILL_ENQUEUE_PACING`]), respects the global pending ceiling,
/// ordinary popularity-first priority, and skips repos that already hold any
/// queue row (pending/in-progress are already being handled; dead/restricted
/// parks are terminal and must not be revived here). Returns the repos
/// actually enqueued.
pub async fn sweep_missing_metadata(db: &Db) -> Result<Vec<String>> {
    let pending = queue::pending_only_count(db).await?;
    let headroom = MAX_PENDING_FETCHES.saturating_sub(pending);
    if headroom <= 0 {
        return Ok(Vec::new());
    }
    let limit = headroom.min(METADATA_BACKFILL_BATCH);
    let candidates: Vec<(String, i64)> = sqlx::query_as(
        "SELECT repo, view_count FROM repos \
         WHERE missing = FALSE \
           AND metadata_fetched_at IS NULL \
           AND (history_complete OR stargazers_complete OR star_count IS NOT NULL) \
           AND NOT EXISTS ( \
               SELECT 1 FROM star_fetch_queue queued WHERE queued.repo = repos.repo \
           ) \
         ORDER BY view_count DESC, repo \
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&db.pool)
    .await?;
    let mut enqueued = Vec::with_capacity(candidates.len());
    for (repo, view_count) in candidates {
        if !enqueued.is_empty() {
            sleep(BACKFILL_ENQUEUE_PACING).await;
        }
        queue::enqueue(db, &repo, view_count).await?;
        enqueued.push(repo);
    }
    Ok(enqueued)
}

/// How stale a tracked repository's public metadata may get before the worker
/// refreshes it.
const METADATA_REFRESH_TTL: chrono::Duration = chrono::Duration::hours(24);

/// Repositories one refresh pass may touch. One GitHub metadata call each, so
/// this is also the per-pass budget spend.
///
/// Self-pacing, which is why it stays this large while the backfill sweep does
/// not: each repository costs a blocking GitHub round-trip before its single
/// small `UPDATE`, so a pass is a trickle of ~3 writes/second spread over
/// tens of seconds of a 15-minute cycle — it cannot burst at Postgres the way
/// a loop of pure inserts can. 120 per pass covers ~11.5k repositories a day,
/// which is what keeps the whole tracked corpus inside the 24h
/// [`METADATA_REFRESH_TTL`] that badges and cards read from.
const METADATA_REFRESH_BATCH: i64 = 120;

/// Refresh cadence: one pass at startup, then every 15 minutes. Short enough
/// that the daily budget above is delivered as small trickles rather than as
/// one long burst competing with an analysis commit.
const METADATA_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// One pass of the public-metadata refresh sweep.
///
/// `repos.star_count` / `forks_count` are what badges, cards, and OG images
/// print. Until this sweep existed they were written only by the `/analyze`
/// request path, so a repository nobody ever opens on the site — the normal
/// case for a project that embeds a badge in its README and never visits —
/// kept serving whatever numbers its first ingestion happened to see.
///
/// Popularity-first and bounded per pass, and it stops as soon as the shared
/// GitHub budget runs dry, so it can never crowd out ingestion.
pub async fn sweep_stale_metadata(cache: &Cache, github: &Arc<GithubClient>) -> Result<usize> {
    let cutoff = Utc::now() - METADATA_REFRESH_TTL;
    let candidates: Vec<String> = sqlx::query_scalar(
        "SELECT repo FROM repos \
         WHERE missing = FALSE \
           AND metadata_fetched_at IS NOT NULL \
           AND metadata_fetched_at < $1 \
         ORDER BY view_count DESC, metadata_fetched_at \
         LIMIT $2",
    )
    .bind(cutoff)
    .bind(METADATA_REFRESH_BATCH)
    .fetch_all(&cache.db().pool)
    .await?;

    let mut refreshed = 0usize;
    for repo in candidates {
        if !github.has_budget().await {
            break;
        }
        let Some((owner, name)) = repo.split_once('/') else {
            continue;
        };
        match github.repo_metadata(owner, name).await {
            Ok(Some(metadata)) => {
                cache.put_repo_metadata(&repo, &metadata).await?;
                refreshed += 1;
            }
            // A repository that has become private or was deleted is
            // tombstoned exactly as the ingestion path would tombstone it.
            Ok(None) => cache.mark_repo_missing(&repo).await?,
            Err(error) => {
                tracing::warn!(%repo, %error, "metadata refresh failed");
                break;
            }
        }
    }
    Ok(refreshed)
}

/// Spawn the periodic public-metadata refresh (startup + every 15 minutes).
pub fn spawn_metadata_refresh(cache: Cache, github: Arc<GithubClient>) {
    tokio::spawn(async move {
        loop {
            match sweep_stale_metadata(&cache, &github).await {
                Ok(0) => {}
                Ok(refreshed) => {
                    tracing::info!(refreshed, "public metadata refreshed for tracked repos")
                }
                Err(error) => tracing::warn!(%error, "metadata refresh sweep failed"),
            }
            sleep(METADATA_REFRESH_INTERVAL).await;
        }
    });
}

/// Spawn the periodic metadata backfill sweep (startup + hourly). Runs in
/// every worker replica: the enqueue is idempotent and the candidate query
/// excludes repos that already hold a queue row, so overlapping passes are
/// harmless.
pub fn spawn_metadata_backfill(db: Db) {
    tokio::spawn(async move {
        loop {
            match sweep_missing_metadata(&db).await {
                Ok(enqueued) if enqueued.is_empty() => {}
                Ok(enqueued) => tracing::info!(
                    enqueued = enqueued.len(),
                    "metadata backfill: re-enqueued legacy repos missing public metadata"
                ),
                Err(error) => tracing::warn!(%error, "metadata backfill sweep failed"),
            }
            sleep(METADATA_BACKFILL_INTERVAL).await;
        }
    });
}

/// How old a published star history may get before the background sweep
/// re-reads it, for a repository nobody is looking at. A report view
/// refreshes on its own shorter TTL (`analyzer::STARGAZER_REFRESH_TTL`); this
/// is what keeps the leaderboards and the long tail of the sitemap moving.
const BACKGROUND_REFRESH_TTL: chrono::Duration = crate::analyzer::EMBED_REFRESH_TTL;

/// Repositories one sweep pass may enqueue.
const STAR_HISTORY_SWEEP_BATCH: i64 = 100;

/// The sweep only tops the queue up. While this many jobs are already waiting
/// it adds nothing, so a backlog of visitor-driven work is never buried under
/// background refreshes.
const STAR_HISTORY_SWEEP_MAX_BACKLOG: i64 = 50;

/// Share of the shared token's hourly budget that must still be unspent before
/// a pass adds work. Half, so background refreshes can never be the reason a
/// visitor's metadata lookup finds the token at its reserve.
const STAR_HISTORY_SWEEP_SPARE_BUDGET: f64 = 0.5;

/// Sweep cadence: one pass at startup, then every 15 minutes.
const STAR_HISTORY_SWEEP_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// One pass of the star-history sweep: offer repositories whose published
/// history is not GitHub's star history yet (an older exact snapshot frozen by
/// the July 2026 restriction, or an approximate archive series), then ones
/// whose GitHub star history is older than [`BACKGROUND_REFRESH_TTL`].
/// Returns how many it enqueued.
pub async fn sweep_star_history(db: &Db, github: &GithubClient) -> Result<usize> {
    if queue::pending_only_count(db).await? >= STAR_HISTORY_SWEEP_MAX_BACKLOG {
        return Ok(0);
    }
    if !github
        .has_spare_budget(STAR_HISTORY_SWEEP_SPARE_BUDGET)
        .await
    {
        return Ok(0);
    }
    let enqueued = queue::enqueue_star_history_sweep(
        db,
        STAR_HISTORY_SWEEP_BATCH,
        Utc::now() - BACKGROUND_REFRESH_TTL,
    )
    .await?;
    Ok(enqueued.len())
}

/// Spawn the periodic star-history sweep (startup + every 15 minutes). Runs in
/// every worker replica: the enqueue skips repositories that already hold a
/// queue row, so overlapping passes are harmless.
pub fn spawn_star_history_sweep(db: Db, github: Arc<GithubClient>) {
    tokio::spawn(async move {
        loop {
            match sweep_star_history(&db, &github).await {
                Ok(0) => {}
                Ok(enqueued) => tracing::info!(
                    enqueued,
                    "star-history sweep: offered histories to upgrade or refresh"
                ),
                Err(error) => tracing::warn!(%error, "star-history sweep failed"),
            }
            sleep(STAR_HISTORY_SWEEP_INTERVAL).await;
        }
    });
}

/// Workers this pool actually runs for a requested size. Pure so the clamp is
/// unit-testable without spawning anything.
fn effective_worker_count(requested: usize) -> usize {
    requested.clamp(1, MAX_STAR_FETCH_WORKERS)
}

/// Spawn background workers, clamped to [`MAX_STAR_FETCH_WORKERS`]. Each
/// loops claiming and processing jobs. The clamp is enforced here rather than
/// trusted to the caller because this pool's cost lands on the analysis pool's
/// CPU and database, not on the caller's.
pub fn spawn_pool(ctx: WorkerCtx, requested: usize) {
    let count = effective_worker_count(requested);
    if requested > count {
        tracing::info!(
            requested,
            effective = count,
            "star-history pool clamped so background star work yields to repo analysis"
        );
    }
    for i in 0..count {
        let ctx = ctx.clone();
        let id = format!("sf{i}");
        tokio::spawn(async move {
            run_worker(id, ctx).await;
        });
    }
}

async fn run_worker(worker_id: String, ctx: WorkerCtx) {
    tracing::info!(worker_id, "star-history worker started");
    let idle = Duration::from_secs(5);
    // Per-repo transient-failure counter feeds the backoff schedule. The
    // durable attempt count lives in the queue row; this is just the
    // in-process sleep between retries so we don't hammer on a flaky repo.
    let mut consecutive_failures: u32 = 0;
    loop {
        let job = match queue::claim_one(&ctx.cache.db().clone(), &worker_id).await {
            Ok(Some(job)) => job,
            Ok(None) => {
                sleep(idle).await;
                continue;
            }
            Err(e) => {
                tracing::error!(error = %e, "star-history claim failed");
                sleep(idle).await;
                continue;
            }
        };
        match process(&ctx, &job.repo).await {
            Ok(Outcome::Complete { total }) => {
                consecutive_failures = 0;
                tracing::info!(repo = %job.repo, total, "star history complete");
                if let Err(e) = queue::complete(ctx.cache.db(), &job.repo).await {
                    tracing::warn!(repo = %job.repo, error = %e, "queue complete failed");
                }
            }
            Ok(Outcome::Private) => {
                // The repository exists but is private: gitdebt is a
                // public-data product and never publishes it, whatever could
                // read it. Park it `restricted` (NOT `missing`), so it stops
                // costing a lookup on every view and is not buried for good —
                // a tombstone would survive the repository later going public.
                consecutive_failures = 0;
                tracing::info!(repo = %job.repo, "repository is private; parking restricted");
                if let Err(e) =
                    queue::mark_restricted(ctx.cache.db(), &job.repo, "repository is private").await
                {
                    tracing::warn!(repo = %job.repo, error = %e, "queue mark_restricted failed");
                }
            }
            Err(e) => {
                let msg = e.to_string();
                // A `NotFound` is PERMANENT (deleted/typo'd repo): retrying
                // can never succeed, and the extension re-enqueues these on
                // every page view. Treat it as terminal — park the queue row
                // `dead` (no requeue) AND tombstone the repo so the analyze /
                // ext-ping enqueue paths short-circuit. Anything else is
                // transient: `fail` re-queues it with a growing delay.
                if is_not_found(&e) {
                    tracing::info!(repo = %job.repo, "repo not found (404); tombstoning + parking dead");
                    if let Err(e2) = ctx.cache.mark_repo_missing(&job.repo).await {
                        tracing::warn!(repo = %job.repo, error = %e2, "mark_repo_missing failed");
                    }
                    if let Err(e2) = queue::mark_dead(ctx.cache.db(), &job.repo, &msg).await {
                        tracing::warn!(repo = %job.repo, error = %e2, "queue mark_dead failed");
                    }
                    // A 404 isn't our fault — don't escalate the in-process
                    // backoff that's meant for flaky-network blips.
                    consecutive_failures = 0;
                } else {
                    tracing::warn!(repo = %job.repo, error = %msg, "star-history fetch failed");
                    if let Err(e2) = queue::fail(ctx.cache.db(), &job.repo, &msg).await {
                        tracing::warn!(repo = %job.repo, error = %e2, "queue fail failed")
                    }
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    let secs = backoff_secs(consecutive_failures);
                    sleep(Duration::from_secs(secs)).await;
                }
            }
        }
    }
}

/// Exponential backoff: 1, 2, 4, 8, 16, 32, 32, … (seconds).
fn backoff_secs(failures: u32) -> u64 {
    let shift = failures.saturating_sub(1).min(5);
    (1u64 << shift).min(BACKOFF_CAP_SECS)
}

/// True iff this error is a *permanent* GitHub `NotFound` — meaning the
/// repo is deleted/typo'd and retrying is futile. The worker tombstones +
/// parks these `dead` instead of requeuing. Kept as a pure classifier so the
/// terminal-vs-transient decision is unit-testable.
fn is_not_found(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<GithubError>(),
        Some(GithubError::NotFound(_))
    )
}

/// Result of one read that did not fail.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The whole history was read and committed complete.
    Complete { total: i64 },
    /// The repository exists and is private. Nothing was read or written.
    Private,
}

/// What a repository lookup means for a history read, as a pure step so the
/// three-way split is testable: public goes ahead, private parks, absent
/// tombstones.
fn settle_visibility(repo: &str, visibility: &RepoVisibility) -> Result<Option<Outcome>> {
    match visibility {
        RepoVisibility::Public(_) => Ok(None),
        RepoVisibility::Private => Ok(Some(Outcome::Private)),
        RepoVisibility::Absent => Err(GithubError::NotFound(repo.to_string()).into()),
    }
}

async fn process(ctx: &WorkerCtx, repo_full: &str) -> Result<Outcome> {
    let (owner, repo) = split_slug(repo_full);
    // Queue membership is never a visibility grant. Confirm through the
    // public-only metadata decoder before reading any history, so a private
    // repository can never enter this worker's writes.
    if !ctx
        .cache
        .repo_metadata_fresh_within(repo_full, chrono::Duration::hours(1))
        .await?
    {
        let visibility = ctx.github.repo_visibility(&owner, &repo).await?;
        if let Some(outcome) = settle_visibility(repo_full, &visibility)? {
            return Ok(outcome);
        }
        if let RepoVisibility::Public(metadata) = &visibility {
            ctx.cache.put_repo_metadata(repo_full, metadata).await?;
        }
    }

    let weeks = match ctx.github.star_history(&owner, &repo).await {
        Ok(weeks) => weeks,
        // A 404 from the history endpoint means gone or private, and the two
        // must not be confused: settle it through the repository itself.
        Err(GithubError::StarHistoryUnavailable(_)) => {
            let visibility = ctx.github.repo_visibility(&owner, &repo).await?;
            if let Some(outcome) = settle_visibility(repo_full, &visibility)? {
                return Ok(outcome);
            }
            if let RepoVisibility::Public(metadata) = &visibility {
                ctx.cache.put_repo_metadata(repo_full, metadata).await?;
            }
            // Public, yet no history: nothing this worker can decide. Retry
            // later rather than publish an empty curve or bury a live repo.
            anyhow::bail!("star history unavailable for a public repository");
        }
        Err(error) => return Err(error.into()),
    };

    let days = star_history::star_days(&weeks, Utc::now().date_naive())?;
    let total = star_history::total_stars(&days);
    let authoritative = ctx.cache.get_repo_star_count(repo_full).await?;
    if star_history::implausibly_short(total, authoritative) {
        anyhow::bail!(
            "star history totals {total}, implausibly short of the repository's {} stars",
            authoritative.unwrap_or_default()
        );
    }
    ctx.cache.put_repo_star_days(repo_full, &days).await?;
    Ok(Outcome::Complete { total })
}

fn split_slug(slug: &str) -> (String, String) {
    match slug.split_once('/') {
        Some((o, r)) => (o.to_string(), r.to_string()),
        None => (slug.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pool shares one 12 vCPU co-tenant host and one Postgres with the
    /// repo-analysis pool, so its size is a property of this module, not of
    /// whatever a caller passes in.
    #[test]
    fn worker_pool_is_clamped_however_many_are_requested() {
        assert_eq!(effective_worker_count(0), 1, "always at least one worker");
        assert_eq!(effective_worker_count(1), 1);
        assert_eq!(effective_worker_count(8), MAX_STAR_FETCH_WORKERS);
        assert_eq!(effective_worker_count(usize::MAX), MAX_STAR_FETCH_WORKERS);
        const {
            assert!(
                MAX_STAR_FETCH_WORKERS <= 4,
                "star fetching has no deadline; analysis does"
            )
        };
    }

    #[test]
    fn backoff_schedule_matches_doc() {
        assert_eq!(backoff_secs(1), 1);
        assert_eq!(backoff_secs(2), 2);
        assert_eq!(backoff_secs(3), 4);
        assert_eq!(backoff_secs(4), 8);
        assert_eq!(backoff_secs(5), 16);
        assert_eq!(backoff_secs(6), 32);
        assert_eq!(backoff_secs(7), 32, "capped at 32s");
        assert_eq!(backoff_secs(100), 32);
    }

    #[test]
    fn split_slug_splits_owner_repo() {
        assert_eq!(split_slug("a/b"), ("a".into(), "b".into()));
        assert_eq!(split_slug("solo"), ("solo".into(), "".into()));
    }

    #[test]
    fn not_found_is_terminal() {
        // A GithubError::NotFound bubbled through anyhow is classified
        // terminal (→ tombstone + park dead, never requeue).
        let err: anyhow::Error = GithubError::NotFound("o/r".into()).into();
        assert!(is_not_found(&err));
    }

    #[test]
    fn other_github_errors_are_transient() {
        // Rate limits, access denials, an unavailable history, API errors and
        // plain errors are NOT terminal — they go through the delayed `fail`
        // path instead of burying a repository that exists.
        for err in [
            anyhow::Error::from(GithubError::RateLimited(None)),
            GithubError::Forbidden("o/r".into()).into(),
            GithubError::StarHistoryUnavailable("o/r".into()).into(),
            GithubError::Api {
                status: 422,
                body: "Pagination is limited to 100 pages.".into(),
            }
            .into(),
            anyhow::anyhow!("some db error"),
        ] {
            assert!(!is_not_found(&err), "{err}");
        }
    }

    /// A private repository must never be tombstoned as missing.
    ///
    /// `missing` drives a permanent not-found short-circuit that never
    /// re-enqueues, so burying a private repository records "does not exist"
    /// about one we know does, and keeps it buried after it is made public.
    /// Private parks `restricted` instead; only a genuine absence tombstones.
    #[test]
    fn visibility_settles_three_ways() {
        let private = settle_visibility("o/r", &RepoVisibility::Private).unwrap();
        assert_eq!(private, Some(Outcome::Private));

        let absent = settle_visibility("o/r", &RepoVisibility::Absent).unwrap_err();
        assert!(is_not_found(&absent));

        let metadata: crate::github::RepoMetadata = serde_json::from_value(serde_json::json!({
            "stargazers_count": 1,
            "forks_count": 0,
            "private": false
        }))
        .unwrap();
        let public = settle_visibility("o/r", &RepoVisibility::Public(metadata)).unwrap();
        assert_eq!(public, None, "a public repository goes ahead to the read");
    }
}
