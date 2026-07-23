//! Background star-history fetch worker(s).
//!
//! Drains `star_fetch_queue` (see `queue.rs`): claims a repo, fetches its
//! stargazer timeline from GitHub, and writes it through the cache,
//! honoring the `*_complete` invariant (the flag flips only inside the
//! committed write transaction; never on a rate-limit or error path).
//!
//! Budget safety: every GitHub call routes through
//! `GithubClient::send` → `RateLimitTracker::acquire`, which *blocks*
//! until the per-token budget has headroom (and honors `Retry-After` on
//! secondary limits). Each request reserves budget before it is sent.
//!
//! Default to a SINGLE worker (avoid burstiness, per AGENTS.md);
//! `WORKER_COUNT` overrides. Exponential backoff on transient errors.
//!
//! Big-repo guardrail: a single attempt fetches at most
//! `MAX_STARGAZER_PAGES` pages (env, default 400 → 40k stars). A repo
//! larger than that is written partial (cache stays `*_complete = FALSE`)
//! and re-enqueued with a persisted page cursor, so one viral repo can't
//! eat the whole hourly budget in a single job.
//
// TODO: GH Archive/BigQuery backfill for >cap repos — the right primary
// source for the full timeline of million-star repos, moving the hot path
// off GitHub's per-token API budget entirely (see AGENTS.md roadmap).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use tokio::time::sleep;

use crate::cache::{Cache, StargazerEvent};
use crate::db::Db;
use crate::github::{GithubClient, GithubError};
use crate::queue;

/// Default per-attempt page cap. 400 pages × 100/page = 40k stars per
/// attempt. Tunable via `MAX_STARGAZER_PAGES`.
const DEFAULT_MAX_STARGAZER_PAGES: u32 = 400;

/// Backoff ceiling for transient errors (1, 2, 4, … capped at 32s),
/// matching the AGENTS.md-documented schedule.
const BACKOFF_CAP_SECS: u64 = 32;

#[derive(Clone)]
pub struct WorkerCtx {
    pub github: Arc<GithubClient>,
    pub cache: Cache,
    /// Per-attempt page cap (the big-repo guardrail).
    pub max_pages: u32,
}

impl WorkerCtx {
    pub fn new(github: Arc<GithubClient>, cache: Cache) -> Self {
        let max_pages = std::env::var("MAX_STARGAZER_PAGES")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_MAX_STARGAZER_PAGES);
        Self {
            github,
            cache,
            max_pages,
        }
    }
}

/// Max repos one metadata-backfill sweep pass may enqueue. Bounds both the
/// per-pass GitHub metadata spend (one metadata call per repo when the
/// claim path processes it) and the queue growth from a single sweep.
const METADATA_BACKFILL_BATCH: i64 = 200;

/// Sweep cadence: one pass at startup, then hourly.
const METADATA_BACKFILL_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Mirror of the analyze-path global pending-queue ceiling
/// (`analyzer::max_pending_fetches`): past this many `pending` rows the
/// sweep enqueues nothing and waits for its next pass.
fn max_pending_fetches() -> i64 {
    std::env::var("MAX_PENDING_FETCHES")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(5_000)
}

/// One pass of the profile-stats metadata backfill sweep.
///
/// Rows ingested before the public-metadata read gate existed have complete
/// history but `metadata_fetched_at IS NULL`, which makes them invisible to
/// every reader (user cards, aggregates, exports) with nothing on the read
/// path allowed to heal them. This sweep re-enqueues them into the durable
/// `star_fetch_queue`; the claim path (archive coordinator or the debug
/// GitHub fallback) writes metadata via `put_repo_metadata` before touching
/// any history, so healing costs one metadata call per repo and never
/// re-paginates stargazers.
///
/// Bounded per pass ([`METADATA_BACKFILL_BATCH`]), respects the global
/// pending ceiling, ordinary popularity-first priority, and skips repos that
/// already hold any queue row (pending/in-progress are already being
/// handled; dead/restricted parks are terminal and must not be revived
/// here). Returns the repos actually enqueued.
pub async fn sweep_missing_metadata(db: &Db) -> Result<Vec<String>> {
    let pending = queue::pending_only_count(db).await?;
    let headroom = max_pending_fetches().saturating_sub(pending);
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
        queue::enqueue(db, &repo, view_count).await?;
        enqueued.push(repo);
    }
    Ok(enqueued)
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

/// Spawn `count` background workers (min 1). Each loops claiming and
/// processing jobs.
pub fn spawn_pool(ctx: WorkerCtx, count: usize) {
    let count = count.max(1);
    for i in 0..count {
        let ctx = ctx.clone();
        let id = format!("sf{i}");
        tokio::spawn(async move {
            run_worker(id, ctx).await;
        });
    }
}

async fn run_worker(worker_id: String, ctx: WorkerCtx) {
    tracing::info!(
        worker_id,
        max_pages = ctx.max_pages,
        "star-fetch worker started"
    );
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
                tracing::error!(error = %e, "star-fetch claim failed");
                sleep(idle).await;
                continue;
            }
        };
        match process(&ctx, &job).await {
            Ok(Outcome::Complete { total }) => {
                consecutive_failures = 0;
                tracing::info!(repo = %job.repo, total, "star history complete");
                if let Err(e) = queue::complete(ctx.cache.db(), &job.repo).await {
                    tracing::warn!(repo = %job.repo, error = %e, "queue complete failed");
                }
            }
            Ok(Outcome::Partial { fetched, next_page }) => {
                consecutive_failures = 0;
                match queue::requeue_partial(ctx.cache.db(), &job.repo, next_page).await {
                    Ok(()) => tracing::info!(
                        repo = %job.repo,
                        fetched,
                        next_page,
                        "star history hit page cap; re-enqueued to continue"
                    ),
                    Err(e) => {
                        tracing::warn!(repo = %job.repo, error = %e, "queue requeue_partial failed")
                    }
                }
            }
            Ok(Outcome::Restricted { fetched }) => {
                // Empty/short stargazer response — the endpoint is restricted
                // or momentarily unreadable. Nothing was committed (existing
                // history is intact). Park `restricted` (NOT `missing`): the
                // repo exists, we just can't read its stargazers, and re-
                // polling on every view would burn budget for nothing.
                consecutive_failures = 0;
                tracing::info!(
                    repo = %job.repo,
                    fetched,
                    "stargazers empty/short; parking restricted (not missing)"
                );
                let detail = format!("empty/short stargazer response (fetched {fetched})");
                if let Err(e) = queue::mark_restricted(ctx.cache.db(), &job.repo, &detail).await {
                    tracing::warn!(repo = %job.repo, error = %e, "queue mark_restricted failed");
                }
            }
            Err(e) => {
                let msg = e.to_string();
                // A `NotFound` is PERMANENT (private/deleted/typo'd repo):
                // retrying can never succeed, and the extension re-enqueues
                // these on every page view. Treat it as terminal — park the
                // queue row `dead` (no requeue) AND tombstone the repo so the
                // analyze / ext-ping enqueue paths short-circuit. Anything
                // else is transient: `fail` bumps attempts and re-queues
                // until the attempts cap parks it dead.
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
                } else if is_forbidden(&e) {
                    // Durable 403 (no rate-limit headers) — chiefly the
                    // stargazer restriction. The repo EXISTS, so do NOT
                    // tombstone it `missing`; park it `restricted` so it stops
                    // re-polling GitHub on every view. Nothing was committed,
                    // so any existing complete history stays intact.
                    tracing::info!(repo = %job.repo, "stargazers forbidden (403); parking restricted (not missing)");
                    if let Err(e2) =
                        queue::mark_restricted(ctx.cache.db(), &job.repo, "forbidden (403)").await
                    {
                        tracing::warn!(repo = %job.repo, error = %e2, "queue mark_restricted failed");
                    }
                    consecutive_failures = 0;
                } else {
                    tracing::warn!(repo = %job.repo, error = %msg, "star-fetch failed");
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
/// repo is private/deleted/typo'd and retrying is futile. The worker
/// tombstones + parks these `dead` instead of requeuing. Kept as a pure
/// classifier so the terminal-vs-transient decision is unit-testable.
fn is_not_found(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<GithubError>(),
        Some(GithubError::NotFound(_))
    )
}

/// True iff this error is a *durable* GitHub `Forbidden` (a bare 403 with no
/// rate-limit signal — chiefly the 2026-06-30 stargazer restriction). Unlike
/// a 404 the repo is NOT missing, so the worker parks it `restricted`
/// (without a `missing` tombstone) rather than tombstoning or looping on it.
/// Pure classifier so the restricted-vs-transient decision is unit-testable.
fn is_forbidden(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<GithubError>(),
        Some(GithubError::Forbidden(_))
    )
}

/// A 404 from the stargazer-list endpoint is ambiguous after GitHub's
/// endpoint restriction. `process` confirms the repository through the
/// metadata endpoint before the worker is allowed to tombstone it.
fn is_stargazers_unavailable(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<GithubError>(),
        Some(GithubError::StargazersUnavailable(_))
    )
}

/// Result of one fetch attempt.
enum Outcome {
    /// The full list was fetched (or refreshed) and committed complete.
    Complete { total: i64 },
    /// The per-attempt page cap was hit; rows written partial, job must
    /// continue on a later claim.
    Partial { fetched: usize, next_page: u32 },
    /// The stargazer response was empty or drastically short of the
    /// authoritative star count — i.e. the endpoint is restricted (the
    /// 2026-06-30 rollout serves empty-200 to non-admin callers) or the repo
    /// is momentarily unreadable. Nothing was committed: the existing cached
    /// history (if any) is left intact and `stargazers_complete` stays as it
    /// was. The job is parked `restricted` so it doesn't loop hot poisoning
    /// the cache with a `star_count = 0` complete write.
    Restricted { fetched: usize },
}

/// A source of stargazer pages — abstracted so the
/// fetch-and-append logic is unit-testable with a fake source (the real
/// impl wraps `GithubClient`; tests use an in-memory page list).
#[async_trait]
trait PageSource {
    /// Fetch page `page` (1-indexed, oldest-first, 100/page). Returns the
    /// page items and, on page 1, the total last-page count.
    async fn page(&self, page: u32) -> Result<(Vec<StargazerEvent>, Option<u32>), GithubError>;

    /// Fetch a batch of pages (1-indexed) and return their items in the
    /// SAME order as `pages`. The default implementation is sequential
    /// (used by the in-memory test fake); the real GitHub-backed source
    /// overrides this to fan the requests out concurrently, every call
    /// still routing through `RateLimitTracker::acquire` so the per-token
    /// budget holds — concurrency only reclaims TCP/TLS RTT. Used by the
    /// cold/continuation `full_fetch`; the incremental tail walk stays
    /// strictly sequential (it early-stops at the cached boundary, so it
    /// must observe pages in order).
    async fn pages(&self, pages: &[u32]) -> Result<Vec<Vec<StargazerEvent>>, GithubError> {
        let mut out = Vec::with_capacity(pages.len());
        for &p in pages {
            let (items, _) = self.page(p).await?;
            out.push(items);
        }
        Ok(out)
    }
}

/// Real page source backed by the GitHub client.
struct GithubPages<'a> {
    github: &'a GithubClient,
    owner: String,
    repo: String,
}

#[async_trait]
impl PageSource for GithubPages<'_> {
    async fn page(&self, page: u32) -> Result<(Vec<StargazerEvent>, Option<u32>), GithubError> {
        let p = self
            .github
            .stargazers_page(&self.owner, &self.repo, page)
            .await?;
        let items = p
            .items
            .into_iter()
            .enumerate()
            .map(|(index, s)| {
                let position = i64::from(page.saturating_sub(1)) * 100 + index as i64 + 1;
                (position, s.starred_at)
            })
            .collect();
        Ok((items, p.last_page))
    }

    async fn pages(&self, pages: &[u32]) -> Result<Vec<Vec<StargazerEvent>>, GithubError> {
        // Concurrent buffered fetch of the page range. `buffered` preserves
        // input order, so the returned Vec aligns 1:1 with `pages` — the
        // cold-path round-trip count drops from O(N) serial RTTs to O(N/k).
        self.github
            .stargazers_pages(&self.owner, &self.repo, pages)
            .await
    }
}

async fn process(ctx: &WorkerCtx, job: &queue::Job) -> Result<Outcome> {
    let (owner, repo) = split_slug(&job.repo);
    // Queue membership is never a visibility grant. Confirm through the
    // public-only metadata decoder before touching the stargazer endpoint;
    // OAuth-visible private repositories therefore cannot enter this worker.
    if !ctx
        .cache
        .repo_metadata_fresh_within(&job.repo, chrono::Duration::hours(1))
        .await?
    {
        match ctx.github.repo_metadata(&owner, &repo).await? {
            Some(metadata) => {
                ctx.cache
                    .put_repo_metadata(
                        &job.repo,
                        metadata.id,
                        metadata.stargazers_count,
                        metadata.forks_count,
                        metadata.created_at,
                    )
                    .await?;
            }
            None => return Err(GithubError::NotFound(job.repo.clone()).into()),
        }
    }
    // An exact GitHub snapshot is immutable once complete. A legacy exact
    // row may still reach this worker solely to heal its missing public
    // metadata stamp; settle that queue row after metadata instead of
    // touching the stargazers endpoint again.
    if let Some(state) = ctx.cache.get_archive_backfill_state(&job.repo).await?
        && state.exact_history_complete
    {
        return Ok(Outcome::Complete {
            total: state.authoritative_total.unwrap_or(0).max(0),
        });
    }
    let src = GithubPages {
        github: &ctx.github,
        owner: owner.clone(),
        repo: repo.clone(),
    };
    // A repo that is already complete and merely stale → incremental tail
    // fetch (only the new pages). Anything else (cold, or a `partial`
    // continuation that never completed) → full fetch from page 1. We
    // detect "complete" via the read-side flag.
    let complete = ctx.cache.repo_stargazers_complete(&job.repo).await?;
    let result = if complete && !job.partial {
        incremental_fetch(ctx, &job.repo, &src).await
    } else {
        full_fetch(ctx, &job.repo, &src, job.next_page).await
    };

    if let Err(error) = &result
        && is_stargazers_unavailable(error)
    {
        return match ctx.github.repo_metadata(&owner, &repo).await? {
            Some(metadata) => {
                ctx.cache
                    .put_repo_metadata(
                        &job.repo,
                        metadata.id,
                        metadata.stargazers_count,
                        metadata.forks_count,
                        metadata.created_at,
                    )
                    .await?;
                Ok(Outcome::Restricted { fetched: 0 })
            }
            None => Err(GithubError::NotFound(job.repo.clone()).into()),
        };
    }

    result
}

/// Fetch one resumable oldest-first chunk. Page 1 is always probed for the
/// current last-page link; continuation chunks then start at `next_page`.
async fn full_fetch<S: PageSource + Sync>(
    ctx: &WorkerCtx,
    repo: &str,
    src: &S,
    next_page: u32,
) -> Result<Outcome> {
    let (first, last_page) = src.page(1).await?;
    let last_page = last_page.unwrap_or(1);
    // A large unstar wave can make a stored cursor exceed the new last page.
    // Restarting avoids publishing stale rows from the superseded snapshot.
    let start_page = if next_page.max(1) > last_page {
        1
    } else {
        next_page.max(1)
    };
    let plan = plan_full_chunk(start_page, last_page, ctx.max_pages);
    let mut acc = if start_page == 1 { first } else { Vec::new() };
    let pages: Vec<u32> = plan
        .pages
        .iter()
        .copied()
        .filter(|page| *page != 1)
        .collect();
    if !pages.is_empty() {
        for items in src.pages(&pages).await? {
            acc.extend(items);
        }
    }

    if let Some(next_page) = plan.next_page {
        if start_page == 1 {
            ctx.cache
                .replace_repo_stargazers_partial(repo, &acc)
                .await?;
        } else {
            ctx.cache.put_repo_stargazers_partial(repo, &acc).await?;
        }
        return Ok(Outcome::Partial {
            fetched: acc.len(),
            next_page,
        });
    }

    // Sanity guard against the empty-200 / restricted-response cache poison.
    // GitHub's 2026-06-30 stargazer restriction serves empty or truncated
    // 200s to non-admin callers; committing that as `stargazers_complete =
    // TRUE, star_count = 0` would WIPE an existing complete history and pin a
    // bogus zero. If the fetched set is empty (or drastically below the
    // authoritative `repos.star_count` we already hold from metadata / a
    // prior fetch), do NOT commit: leave the flag and existing rows untouched
    // and surface `Restricted` so the caller parks the job instead of looping.
    let existing = if start_page == 1 {
        0
    } else {
        ctx.cache.repo_stargazer_row_count(repo).await?
    };
    let fetched = existing.saturating_add(acc.len() as i64).max(0) as usize;
    let mut authoritative = ctx.cache.get_repo_star_count(repo).await?;
    if needs_metadata_confirmation(fetched, authoritative) {
        let (owner, name) = split_slug(repo);
        match ctx.github.repo_metadata(&owner, &name).await? {
            Some(metadata) => {
                ctx.cache
                    .put_repo_metadata(
                        repo,
                        metadata.id,
                        metadata.stargazers_count,
                        metadata.forks_count,
                        metadata.created_at,
                    )
                    .await?;
                authoritative = Some(metadata.stargazers_count as i64);
            }
            None => return Err(GithubError::NotFound(repo.to_string()).into()),
        }
    }
    if is_restricted_result(fetched, authoritative) {
        return Ok(Outcome::Restricted { fetched });
    }

    let total = if start_page == 1 {
        let total = acc.len() as i64;
        ctx.cache.put_repo_stargazers(repo, &acc).await?;
        total
    } else {
        ctx.cache.finish_repo_stargazers_partial(repo, &acc).await?
    };
    Ok(Outcome::Complete { total })
}

fn needs_metadata_confirmation(fetched: usize, authoritative: Option<i64>) -> bool {
    fetched == 0 && authoritative.is_none()
}

struct FullChunkPlan {
    pages: Vec<u32>,
    next_page: Option<u32>,
}

fn plan_full_chunk(start_page: u32, last_page: u32, max_pages: u32) -> FullChunkPlan {
    let start_page = start_page.max(1);
    let last_page = last_page.max(1);
    let max_pages = max_pages.max(1);
    if start_page > last_page {
        return FullChunkPlan {
            pages: Vec::new(),
            next_page: None,
        };
    }
    let end_page = last_page.min(start_page.saturating_add(max_pages - 1));
    FullChunkPlan {
        pages: (start_page..=end_page).collect(),
        next_page: (end_page < last_page).then_some(end_page.saturating_add(1)),
    }
}

/// Whether a completed (non-capped) full-fetch result looks like the
/// empty-200 / restricted response rather than a real stargazer list, given
/// the authoritative star count we already hold (if any). Pure so the
/// poison-guard boundary is unit-testable without a DB.
///
///   * Empty result with a known-positive (or unknown) authoritative count →
///     restricted. A genuinely zero-star repo (authoritative known to be 0)
///     is NOT flagged — an empty fetch there is correct.
///   * A non-empty result that is less than half the authoritative count AND
///     short by a meaningful absolute margin (≥50) → restricted (a truncated
///     response). Ordinary unstar churn stays well above this floor.
fn is_restricted_result(fetched: usize, authoritative: Option<i64>) -> bool {
    match authoritative {
        Some(auth) => {
            let auth = auth.max(0) as usize;
            if auth == 0 {
                // Authoritatively zero stars → an empty fetch is the truth.
                false
            } else {
                fetched == 0 || (fetched.saturating_mul(2) < auth && auth - fetched >= 50)
            }
        }
        // No authoritative count to compare against: only guard the
        // unambiguous empty-200 poison.
        None => fetched == 0,
    }
}

/// Incremental refresh: the repo is already complete but stale (count
/// likely grew). GitHub paginates stargazers oldest-first, so the newest
/// stars are on the LAST pages. Walk backward from `last_page`, collecting
/// rows strictly newer than the newest cached timestamp, and stop as soon
/// as a page is fully at-or-before the cached boundary (every earlier page
/// is older still). Append the new tail and re-commit complete.
async fn incremental_fetch<S: PageSource>(ctx: &WorkerCtx, repo: &str, src: &S) -> Result<Outcome> {
    let cached = ctx.cache.get_repo_stargazers_partial(repo).await?;
    let boundary = cached.iter().map(|(_, t)| *t).max();

    // Discover the current last page (page 1 carries the `rel=last`).
    let (_first, last_page) = src.page(1).await?;
    let last_page = last_page.unwrap_or(1);

    let plan = plan_incremental(last_page, ctx.max_pages);
    let mut new_items: Vec<StargazerEvent> = Vec::new();
    let mut reached_boundary = false;

    for page in plan.pages {
        // Page 1 was already fetched above; refetch only if it's in the
        // plan and isn't page 1, otherwise we'd double-count. (The plan
        // walks high→low and only includes page 1 when last_page == 1.)
        let (items, _) = src.page(page).await?;
        let before = new_items.len();
        for (position, at) in items {
            match boundary {
                Some(b) if at <= b => {
                    reached_boundary = true;
                }
                _ => new_items.push((position, at)),
            }
        }
        // If this page contributed nothing new (all at/below boundary),
        // every older page is older too — stop.
        if reached_boundary && new_items.len() == before {
            break;
        }
        if reached_boundary {
            // We straddled the boundary on this page; older pages are all
            // cached. Done.
            break;
        }
    }

    if plan.capped && !reached_boundary {
        // Keep the last complete snapshot visible. The next claim starts a
        // fresh cursor-based backfill rather than trying to splice a large,
        // potentially shifted gap into it.
        return Ok(Outcome::Partial {
            fetched: new_items.len(),
            next_page: 1,
        });
    }

    let total = (cached.len() + new_items.len()) as i64;
    ctx.cache
        .append_repo_stargazers(repo, &new_items, total)
        .await?;
    Ok(Outcome::Complete { total })
}

/// Pages to walk for an incremental refresh, high→low, plus whether the
/// walk was truncated by the page cap. We fetch at most `max_pages` of the
/// newest pages; if the new tail is larger than that the caller treats it
/// as a partial continuation.
struct IncrementalPlan {
    pages: Vec<u32>,
    capped: bool,
}

fn plan_incremental(last_page: u32, max_pages: u32) -> IncrementalPlan {
    let last_page = last_page.max(1);
    let max_pages = max_pages.max(1);
    let lowest = last_page.saturating_sub(max_pages - 1).max(1);
    let pages: Vec<u32> = (lowest..=last_page).rev().collect();
    IncrementalPlan {
        capped: lowest > 1,
        pages,
    }
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
    use chrono::{DateTime, TimeZone, Utc};
    use std::sync::Mutex;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_600_000_000 + secs, 0).unwrap()
    }

    /// In-memory page source. `pages[0]` is page 1 (oldest), each a
    /// `Vec<(login, ts)>`. Records which pages were requested so tests can
    /// assert the walk stopped early.
    struct FakePages {
        pages: Vec<Vec<StargazerEvent>>,
        requested: Mutex<Vec<u32>>,
    }

    impl FakePages {
        fn new(pages: Vec<Vec<StargazerEvent>>) -> Self {
            Self {
                pages,
                requested: Mutex::new(Vec::new()),
            }
        }
        fn last_page(&self) -> u32 {
            self.pages.len().max(1) as u32
        }
    }

    #[async_trait]
    impl PageSource for FakePages {
        async fn page(&self, page: u32) -> Result<(Vec<StargazerEvent>, Option<u32>), GithubError> {
            self.requested.lock().unwrap().push(page);
            let idx = (page as usize).saturating_sub(1);
            let items = self.pages.get(idx).cloned().unwrap_or_default();
            // Only page 1 carries the rel=last header (mirrors GitHub).
            let last = if page == 1 {
                Some(self.last_page())
            } else {
                None
            };
            Ok((items, last))
        }
    }

    fn star(n: i64) -> StargazerEvent {
        (n, at(n))
    }

    /// Pure incremental-merge over a fake source, mirroring
    /// `incremental_fetch` without a DB: returns the new tail (rows
    /// strictly newer than `boundary`) and whether the walk reached the
    /// boundary.
    async fn merge_incremental(
        src: &FakePages,
        boundary: Option<DateTime<Utc>>,
        max_pages: u32,
    ) -> (Vec<StargazerEvent>, bool, bool) {
        let (_first, last_page) = src.page(1).await.unwrap();
        let last_page = last_page.unwrap_or(1);
        let plan = plan_incremental(last_page, max_pages);
        let mut new_items = Vec::new();
        let mut reached = false;
        for page in plan.pages {
            let (items, _) = src.page(page).await.unwrap();
            let before = new_items.len();
            for (position, t) in items {
                match boundary {
                    Some(b) if t <= b => reached = true,
                    _ => new_items.push((position, t)),
                }
            }
            if reached && new_items.len() == before {
                break;
            }
            if reached {
                break;
            }
        }
        (new_items, reached, plan.capped)
    }

    #[tokio::test]
    async fn incremental_appends_only_new_tail() {
        // 3 pages, ascending timestamps. Cached up to ts=4 (mid page 2,
        // since page size here is 2). New tail should be ts {5,6} only.
        let pages = vec![
            vec![star(1), star(2)],
            vec![star(3), star(4)],
            vec![star(5), star(6)],
        ];
        let src = FakePages::new(pages);
        let (new_tail, reached, capped) = merge_incremental(&src, Some(at(4)), 100).await;
        let got: Vec<i64> = new_tail.iter().map(|(position, _)| *position).collect();
        assert_eq!(got, vec![5, 6]);
        assert!(reached, "should reach the cached boundary");
        assert!(!capped);
        // Walked backward from page 3, stopped at page 2 (where boundary
        // sits) — never touched page 1.
        let req = src.requested.lock().unwrap().clone();
        assert!(req.contains(&3));
        assert!(req.contains(&2));
        assert!(
            !req.contains(&1) || req.iter().filter(|&&p| p == 1).count() == 1,
            "page 1 only the rel=last probe, never re-walked: {req:?}"
        );
    }

    #[tokio::test]
    async fn incremental_with_no_new_stars_appends_nothing() {
        let pages = vec![vec![star(1), star(2)], vec![star(3), star(4)]];
        let src = FakePages::new(pages);
        let (new_tail, reached, _) = merge_incremental(&src, Some(at(4)), 100).await;
        assert!(new_tail.is_empty());
        assert!(reached);
    }

    #[tokio::test]
    async fn incremental_cold_boundary_collects_all() {
        // No cached boundary (cold-ish) → every row is "new".
        let pages = vec![vec![star(1), star(2)], vec![star(3), star(4)]];
        let src = FakePages::new(pages);
        let (new_tail, reached, _) = merge_incremental(&src, None, 100).await;
        let mut got: Vec<i64> = new_tail.iter().map(|(position, _)| *position).collect();
        got.sort();
        assert_eq!(got, vec![1, 2, 3, 4]);
        assert!(!reached, "no boundary → never 'reached' it");
    }

    #[tokio::test]
    async fn pages_batch_preserves_order() {
        // The batched `pages()` must return items in the SAME order as the
        // requested page list so `full_fetch`'s oldest-first accumulation
        // is unchanged when the real source fans pages out concurrently.
        let pages = vec![
            vec![star(1), star(2)],
            vec![star(3), star(4)],
            vec![star(5), star(6)],
        ];
        let src = FakePages::new(pages);
        let got = src.pages(&[2, 3]).await.unwrap();
        let flat: Vec<i64> = got
            .into_iter()
            .flatten()
            .map(|(position, _)| position)
            .collect();
        assert_eq!(flat, vec![3, 4, 5, 6], "page 2 then page 3, in order");
    }

    #[test]
    fn plan_incremental_walks_newest_pages_high_to_low() {
        let p = plan_incremental(10, 100);
        assert_eq!(p.pages, vec![10, 9, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert!(!p.capped);
    }

    #[test]
    fn plan_incremental_caps_to_newest_max_pages() {
        // 50 total pages, cap 3 → only newest 3 (50,49,48), capped.
        let p = plan_incremental(50, 3);
        assert_eq!(p.pages, vec![50, 49, 48]);
        assert!(p.capped, "truncated before reaching page 1");
    }

    #[test]
    fn plan_incremental_single_page() {
        let p = plan_incremental(1, 400);
        assert_eq!(p.pages, vec![1]);
        assert!(!p.capped);
    }

    #[test]
    fn full_chunks_resume_without_repeating_pages() {
        let first = plan_full_chunk(1, 5, 2);
        assert_eq!(first.pages, vec![1, 2]);
        assert_eq!(first.next_page, Some(3));

        let second = plan_full_chunk(first.next_page.unwrap(), 5, 2);
        assert_eq!(second.pages, vec![3, 4]);
        assert_eq!(second.next_page, Some(5));

        let third = plan_full_chunk(second.next_page.unwrap(), 5, 2);
        assert_eq!(third.pages, vec![5]);
        assert_eq!(third.next_page, None);
    }

    #[test]
    fn full_chunk_cap_is_never_zero() {
        let plan = plan_full_chunk(1, 2, 0);
        assert_eq!(plan.pages, vec![1]);
        assert_eq!(plan.next_page, Some(2));
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
        // RateLimited / Api / a plain anyhow error are NOT terminal — they
        // go through the attempts-capped `fail` path instead.
        let rl: anyhow::Error = GithubError::RateLimited(None).into();
        assert!(!is_not_found(&rl));
        let api: anyhow::Error = GithubError::Api {
            status: 500,
            body: "boom".into(),
        }
        .into();
        assert!(!is_not_found(&api));
        let generic = anyhow::anyhow!("some db error");
        assert!(!is_not_found(&generic));
    }

    #[test]
    fn forbidden_is_restricted_not_notfound() {
        // A durable 403 classifies as forbidden (→ park restricted) and is
        // NEVER treated as a 404 (which would wrongly tombstone `missing`).
        let f: anyhow::Error = GithubError::Forbidden("o/r".into()).into();
        assert!(is_forbidden(&f));
        assert!(!is_not_found(&f));
        // And 404 is not forbidden.
        let nf: anyhow::Error = GithubError::NotFound("o/r".into()).into();
        assert!(!is_forbidden(&nf));
        // Rate limits / generic errors are neither (they stay transient).
        let rl: anyhow::Error = GithubError::RateLimited(None).into();
        assert!(!is_forbidden(&rl));
        assert!(!is_forbidden(&anyhow::anyhow!("db error")));
    }

    #[test]
    fn stargazer_404_requires_metadata_confirmation() {
        let unavailable: anyhow::Error = GithubError::StargazersUnavailable("o/r".into()).into();
        assert!(is_stargazers_unavailable(&unavailable));
        assert!(!is_not_found(&unavailable));

        let repo_404: anyhow::Error = GithubError::NotFound("o/r".into()).into();
        assert!(!is_stargazers_unavailable(&repo_404));
    }

    #[test]
    fn empty_response_is_flagged_restricted_not_committed() {
        // The empty-200 cache poison: an empty fetch must NEVER be committed
        // as complete. With no authoritative count we still flag empty.
        assert!(is_restricted_result(0, None));
        // With a known-positive authoritative count, empty is restricted.
        assert!(is_restricted_result(0, Some(5000)));
        // A drastically-short result (well under half, big absolute gap) is
        // restricted too (truncated response).
        assert!(is_restricted_result(100, Some(5000)));
    }

    #[test]
    fn unknown_empty_result_requires_metadata_confirmation() {
        assert!(needs_metadata_confirmation(0, None));
        assert!(!needs_metadata_confirmation(0, Some(0)));
        assert!(!needs_metadata_confirmation(1, None));
    }

    #[test]
    fn healthy_and_zero_star_results_are_not_restricted() {
        // A full result matching the authoritative count commits normally.
        assert!(!is_restricted_result(5000, Some(5000)));
        // Ordinary unstar churn (slightly below authoritative) is fine.
        assert!(!is_restricted_result(4990, Some(5000)));
        // A genuinely zero-star repo (authoritative known 0) → empty fetch is
        // the truth, NOT restricted.
        assert!(!is_restricted_result(0, Some(0)));
        // Small repos: a shortfall under the 50-absolute floor isn't flagged.
        assert!(!is_restricted_result(30, Some(60)));
        // A non-empty result with an unknown authoritative count is trusted
        // (we only guard the unambiguous empty-200 case when count unknown).
        assert!(!is_restricted_result(10, None));
    }
}
