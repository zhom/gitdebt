//! Repo star-history pipeline. For a given repo we fetch the stargazer
//! non-identifying arrival timestamps once, cache them, and derive
//! a cumulative total-stars time series from them on every request.
//!
//! No per-user fetching, no scoring — gitdebt is a star-history +
//! repo-debt analytics tool, not a fake-star detector (see AGENTS.md).
//!
//! **Non-blocking read path.** A browser extension fires on every
//! `github.com/owner/repo` a user opens, so `analyze_repo` (and the
//! chart/usage/og series readers) must never synchronously paginate
//! GitHub. They read the cache and, on a cold / stale / in-flight repo,
//! *enqueue* a background fetch (`queue` → `worker`) and return whatever
//! is cached immediately (an empty history + `pending: true` when cold).
//! The expensive pagination is amortized by the worker (once per repo,
//! cached forever) and capped by the shared `RateLimitTracker`.
//!
//! Caching invariant honored throughout: stargazer data flows back from
//! the cache only when `stargazers_complete` is set, and the worker only
//! sets that flag inside the same transaction that committed the rows.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::cache::Cache;
use crate::chart::{self, Point};
use crate::github::GithubClient;
use crate::queue;

/// Cap on the number of points in the API's `history` array. Even
/// sampling keeps the payload bounded for million-star repos while the
/// rendered chart stays smooth.
const MAX_HISTORY_POINTS: usize = 400;

/// How long a completed stargazer set is trusted before a background
/// incremental refresh is enqueued. 6h matches the `/api/ext/ping` TTL —
/// star counts move slowly and the incremental fetch only pulls the new
/// tail, so refreshing more often just burns budget. A read is always
/// served from the cached (possibly slightly-stale) set; the refresh
/// happens out of band.
pub const STARGAZER_REFRESH_TTL: chrono::Duration = chrono::Duration::hours(6);

#[derive(Clone)]
pub struct AnalyzerCtx {
    pub github: Arc<GithubClient>,
    pub cache: Cache,
}

/// Canonical cache / queue / worker key for a repo slug: `owner/repo`,
/// lowercased. This is the single normalization point every star-history
/// surface must agree on — split casing keys the same repo into duplicate
/// full fetches and duplicate leaderboard rows. Kept pure so the invariant
/// is unit-testable without a DB.
pub fn repo_key(owner: &str, repo: &str) -> String {
    format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    )
}

const STAR_MILESTONES: [u32; 4] = [100, 1_000, 10_000, 100_000];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StarMilestone {
    pub stars: u32,
    pub reached_at: Option<NaiveDate>,
    pub days_from_creation: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StarWindowRecord {
    pub stars_gained: u64,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// `1`, `7`, or `30`. Week/month records are rolling calendar windows,
    /// not ISO-week or variable-length calendar-month buckets.
    pub window_days: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StarHistoryInsights {
    pub milestones: Vec<StarMilestone>,
    pub largest_day: Option<StarWindowRecord>,
    pub largest_week: Option<StarWindowRecord>,
    pub largest_month: Option<StarWindowRecord>,
}

/// Pure milestone and growth-record derivation over a complete cumulative
/// daily series. Inputs are normalized by UTC date and decreasing totals are
/// clamped, so repeated or malformed points cannot invent star gains.
pub fn derive_star_history_insights(
    created_at: Option<DateTime<Utc>>,
    points: &[Point],
) -> StarHistoryInsights {
    let mut cumulative_by_day: BTreeMap<NaiveDate, u32> = BTreeMap::new();
    for point in points {
        cumulative_by_day
            .entry(point.at.date_naive())
            .and_modify(|stars| *stars = (*stars).max(point.stars))
            .or_insert(point.stars);
    }

    let milestones = STAR_MILESTONES
        .into_iter()
        .map(|threshold| {
            let reached_at = cumulative_by_day
                .iter()
                .find_map(|(date, stars)| (*stars >= threshold).then_some(*date));
            let days_from_creation = created_at.zip(reached_at).and_then(|(created, reached)| {
                let days = (reached - created.date_naive()).num_days();
                u32::try_from(days.max(0)).ok()
            });
            StarMilestone {
                stars: threshold,
                reached_at,
                days_from_creation,
            }
        })
        .collect();

    let mut previous = 0_u32;
    let daily: Vec<(NaiveDate, u64)> = cumulative_by_day
        .into_iter()
        .map(|(date, cumulative)| {
            let delta = cumulative.saturating_sub(previous);
            previous = previous.max(cumulative);
            (date, u64::from(delta))
        })
        .collect();

    StarHistoryInsights {
        milestones,
        largest_day: largest_star_window(&daily, 1),
        largest_week: largest_star_window(&daily, 7),
        largest_month: largest_star_window(&daily, 30),
    }
}

fn largest_star_window(daily: &[(NaiveDate, u64)], window_days: u16) -> Option<StarWindowRecord> {
    let mut start = 0_usize;
    let mut running = 0_u64;
    let mut best: Option<StarWindowRecord> = None;
    for (end, (to, gained)) in daily.iter().enumerate() {
        running = running.saturating_add(*gained);
        let from = *to - chrono::Duration::days(i64::from(window_days) - 1);
        while start <= end && daily[start].0 < from {
            running = running.saturating_sub(daily[start].1);
            start += 1;
        }
        if running > 0
            && best
                .as_ref()
                .is_none_or(|record| running > record.stars_gained)
        {
            best = Some(StarWindowRecord {
                stars_gained: running,
                from,
                to: *to,
                window_days,
            });
        }
    }
    best
}

/// Result of a star-history lookup. Shape matches the `/analyze` JSON
/// contract the frontend codes against.
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    pub repo: String,
    pub total_stars: u32,
    /// Repo creation date from GitHub metadata, if known. May be null
    /// until the fire-and-forget metadata refresh lands.
    pub created_at: Option<DateTime<Utc>>,
    /// Star-history fetch jobs not yet finished (queue depth). Lets the
    /// frontend show progress while a cold/stale fetch drains.
    pub queued: u32,
    /// True once the stargazer-timestamp fetch is complete and cached.
    pub history_complete: bool,
    /// Semantics of the plotted series. `current_stargazers` is the legacy
    /// exact current-membership snapshot; `public_star_actions` comes from
    /// GH Archive WatchEvents and is approximate because unstars are absent.
    pub history_kind: &'static str,
    /// Number of source events represented by the plotted series. This can
    /// differ from `total_stars`, which remains GitHub's current metadata
    /// count for GH Archive-backed histories.
    pub history_event_count: u32,
    pub history_coverage_start: Option<DateTime<Utc>>,
    pub history_coverage_end: Option<DateTime<Utc>>,
    pub history_approximate: bool,
    /// True when a background fetch is in flight / was just enqueued for
    /// this repo (cold, stale, or already queued). The frontend polls
    /// until this is false.
    pub pending: bool,
    /// True when the repo is very large (exceeded `MAX_STARGAZER_PAGES`) and
    /// is being fetched in resumable chunks. Partial rows are never served;
    /// a refresh can keep its prior complete snapshot visible, while a cold
    /// backfill remains `pending` with an empty history.
    pub backfilling: bool,
    /// Stable public state for the history pipeline: `ready`, `queued`,
    /// `retrying`, or `not_public`.
    pub history_status: &'static str,
    /// Deprecated compatibility field. Infrastructure failures are retryable,
    /// and repository visibility is represented by `not_found` /
    /// `history_status`, so this is always false.
    pub history_unavailable: bool,
    /// True when the default GitHub credentials cannot see the repository
    /// (private/deleted/typo'd). The frontend renders a clear "not public or
    /// not found" state; the backend does NOT re-enqueue a tombstone.
    pub not_found: bool,
    /// Milestone timing and largest rolling growth windows. `None` until the
    /// complete-history gate opens; records are never derived from partial
    /// cache rows.
    pub star_history_insights: Option<StarHistoryInsights>,
    /// Cumulative total-stars series, downsampled to ≤ MAX_HISTORY_POINTS
    /// points (even sampling, always includes first + last). Empty until
    /// the first fetch completes.
    pub history: Vec<Point>,
}

/// Non-blocking star-history lookup. Never paginates GitHub on the request
/// path:
///   * cached & fresh → return the cached history (`pending: false`).
///   * cold / stale / in-flight → enqueue a background fetch and return
///     immediately with whatever is cached (empty when cold) and
///     `pending: true`.
pub async fn analyze_repo(owner: &str, repo: &str, ctx: &AnalyzerCtx) -> Result<AnalysisResult> {
    analyze_repo_with_enqueue(owner, repo, ctx, true).await
}

/// Read the same report snapshot without refreshing metadata or adding queue
/// work. Static-site builds use this so publishing cached pages cannot crowd
/// real report visits out of the durable queues.
pub async fn analyze_repo_readonly(
    owner: &str,
    repo: &str,
    ctx: &AnalyzerCtx,
) -> Result<AnalysisResult> {
    analyze_repo_with_enqueue(owner, repo, ctx, false).await
}

async fn analyze_repo_with_enqueue(
    owner: &str,
    repo: &str,
    ctx: &AnalyzerCtx,
    enqueue: bool,
) -> Result<AnalysisResult> {
    // Case-normalize at the single chokepoint the cache / queue / worker
    // key on. Every other surface (ext_ping, export, cards, usage,
    // stat-charts, overlay, aggregate) already lowercases the slug; keying
    // the queue/cache on the raw URL case here would split a repo into two
    // full fetches and two leaderboard rows. GitHub is case-insensitive so
    // the metadata fetch below is unaffected.
    let owner = owner.to_ascii_lowercase();
    let repo = repo.to_ascii_lowercase();
    let repo_full = repo_key(&owner, &repo);

    // Single fold of the `repos` row: missing flag, completeness +
    // freshness, denormalized star count, created_at, and view_count (for
    // the enqueue priority) all come from one query instead of five.
    let summary = ctx.cache.get_repo_summary(&repo_full).await.unwrap_or(None);

    // Tombstone short-circuit: a repo GitHub already told us is 404
    // (private/deleted/typo) returns a clear not-found result and is NEVER
    // re-enqueued — otherwise the extension would re-queue it on every page
    // view, draining the GitHub budget (the launch-blocker this guards).
    if summary.as_ref().is_some_and(|s| s.missing) {
        let queued = queue::pending_count(ctx.cache.db()).await.unwrap_or(0);
        return Ok(AnalysisResult {
            repo: repo_full,
            total_stars: 0,
            created_at: None,
            queued: queued.clamp(0, u32::MAX as i64) as u32,
            history_complete: false,
            history_kind: "unavailable",
            history_event_count: 0,
            history_coverage_start: None,
            history_coverage_end: None,
            history_approximate: false,
            pending: false,
            backfilling: false,
            history_status: "not_public",
            history_unavailable: false,
            not_found: true,
            star_history_insights: None,
            history: Vec::new(),
        });
    }
    let public = summary
        .as_ref()
        .is_some_and(|s| !s.missing && s.metadata_fetched_at.is_some());

    // Fire-and-forget repo-metadata refresh on TTL miss. Surfaces the
    // authoritative star count + creation date without blocking; the
    // frontend polls so it appears within a tick.
    if enqueue {
        maybe_refresh_metadata(&owner, &repo, ctx);
    }

    // Read-side completeness gate: the cache returns the set only when the
    // fetch previously completed.
    let cached = ctx.cache.get_repo_star_series(&repo_full).await?;
    let fresh = summary
        .as_ref()
        .is_some_and(|s| s.stargazers_fresh_within(STARGAZER_REFRESH_TTL));

    let (history, history_complete, total_stars) = match &cached {
        Some(items) => {
            let total = summary
                .as_ref()
                .and_then(|value| value.star_count)
                .filter(|value| *value >= 0)
                .or_else(|| {
                    summary
                        .as_ref()
                        .and_then(|value| value.history_observed_count)
                })
                .unwrap_or_else(|| {
                    items
                        .last()
                        .map(|point| i64::from(point.stars))
                        .unwrap_or(0)
                })
                .clamp(0, u32::MAX as i64) as u32;
            (chart::downsample(items, MAX_HISTORY_POINTS), true, total)
        }
        None => {
            // Nothing trustworthy cached yet. Surface a best-effort total
            // only after public metadata exists (0 if cold or unverified) so
            // a legacy private row cannot leak through the analyze response.
            let total = summary
                .as_ref()
                .filter(|_| public)
                .and_then(|s| s.star_count)
                .filter(|n| *n >= 0)
                .map(|n| n as u32)
                .unwrap_or(0);
            (Vec::new(), false, total)
        }
    };

    // Enqueue a background fetch when cold or stale. Idempotent: the queue
    // dedups an already pending/in-flight repo, so repeated polls don't
    // pile up jobs. A cold repo is `pending` until the first fetch lands;
    // a stale-but-complete repo is still served from cache (not pending) —
    // the refresh happens out of band. We already know the repo isn't
    // missing (short-circuited above) and its view_count (priority), so use
    // the priority-carrying enqueue to avoid two more single-row reads.
    if enqueue && (!history_complete || !fresh) {
        let priority = summary.as_ref().map(|s| s.view_count).unwrap_or(0);
        enqueue_fetch_known(ctx, &repo_full, priority).await;
    }
    let pending = !history_complete;
    let queue_status = queue::repo_status(ctx.cache.db(), &repo_full)
        .await
        .unwrap_or_default();
    let retrying = !history_complete && queue_status.retrying;

    let created_at = summary
        .as_ref()
        .filter(|_| public)
        .and_then(|s| s.created_at);
    let star_history_insights = cached
        .as_deref()
        .map(|points| derive_star_history_insights(created_at, points));

    Ok(AnalysisResult {
        repo: repo_full,
        total_stars,
        created_at,
        queued: queue_status.queued.clamp(0, u32::MAX as i64) as u32,
        history_complete,
        history_kind: if public
            && summary
                .as_ref()
                .and_then(|value| value.history_source.as_deref())
                == Some("gh_archive")
        {
            "public_star_actions"
        } else if history_complete {
            "current_stargazers"
        } else {
            "unavailable"
        },
        history_event_count: summary
            .as_ref()
            .and_then(|value| value.history_observed_count)
            .map(|count| count.clamp(0, i64::from(u32::MAX)) as u32)
            .unwrap_or(0),
        history_coverage_start: summary
            .as_ref()
            .filter(|_| public)
            .and_then(|value| value.history_coverage_start),
        history_coverage_end: summary
            .as_ref()
            .filter(|_| public)
            .and_then(|value| value.history_coverage_end),
        history_approximate: public
            && summary
                .as_ref()
                .and_then(|value| value.history_source.as_deref())
                == Some("gh_archive"),
        pending,
        backfilling: queue_status.backfilling,
        history_status: if history_complete {
            "ready"
        } else if retrying {
            "retrying"
        } else {
            "queued"
        },
        history_unavailable: false,
        not_found: false,
        star_history_insights,
        history,
    })
}

/// Full (non-downsampled) cumulative star-history series for the chart /
/// usage / og renderers. Non-blocking like [`analyze_repo`]: returns the
/// cached series (empty if not yet fetched) and enqueues a background
/// fetch on a cold/stale miss. Never paginates GitHub inline.
pub async fn star_series(owner: &str, repo: &str, ctx: &AnalyzerCtx) -> Result<Vec<Point>> {
    // Case-normalize on the same key as [`analyze_repo`] and the rest of
    // the codebase so the chart/usage/overlay series share ONE cached
    // stargazer set (and one queue job) with /analyze regardless of the
    // URL's case.
    let repo_full = repo_key(owner, repo);
    match ctx.cache.get_repo_star_series(&repo_full).await? {
        Some(series) => {
            let fresh = ctx
                .cache
                .repo_stargazers_fresh_within(&repo_full, STARGAZER_REFRESH_TTL)
                .await
                .unwrap_or(false);
            if !fresh {
                enqueue_fetch(ctx, &repo_full).await;
            }
            Ok(series)
        }
        None => {
            enqueue_fetch(ctx, &repo_full).await;
            Ok(Vec::new())
        }
    }
}

/// Global cap on the number of `pending` star-fetch jobs. Past this, new
/// enqueues are skipped (the repo stays un-queued; a later request retries).
/// Bounds queue growth so an attacker scripting `/api/ext/ping` can't grow
/// `star_fetch_queue` unbounded (memory + cost). Overridable via
/// `MAX_PENDING_FETCHES`.
const DEFAULT_MAX_PENDING_FETCHES: i64 = 5_000;

fn max_pending_fetches() -> i64 {
    std::env::var("MAX_PENDING_FETCHES")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_PENDING_FETCHES)
}

/// Enqueue a star-history fetch for `repo_full`, prioritized by the repo's
/// current popularity (`view_count`). Best-effort: a queue error is logged,
/// never propagated to the request.
///
/// Two guards before the enqueue:
///   * **tombstone** — a repo marked `missing` (404) is never re-enqueued.
///   * **global ceiling** — if the pending queue is already at
///     [`max_pending_fetches`], skip (the repo stays un-queued and will be
///     retried by a later request). This bounds queue growth under a
///     ping-flood. An already-active repo is exempt from the ceiling so a
///     hot repo mid-fetch still gets its priority bump.
pub async fn enqueue_fetch(ctx: &AnalyzerCtx, repo_full: &str) {
    let db = ctx.cache.db();

    // Tombstoned (404) repos are terminal — never requeue.
    if ctx.cache.repo_is_missing(repo_full).await.unwrap_or(false) {
        return;
    }

    // Global ceiling: only enforce for repos not already active (an active
    // repo's enqueue is an idempotent priority bump, not new growth).
    let already_active = queue::is_active(db, repo_full).await.unwrap_or(false);
    if !already_active {
        let pending = queue::pending_only_count(db).await.unwrap_or(0);
        if pending >= max_pending_fetches() {
            tracing::warn!(
                repo = %repo_full,
                pending,
                "star-fetch queue at ceiling; skipping enqueue"
            );
            return;
        }
    }

    let priority = ctx.cache.get_repo_view_count(repo_full).await.unwrap_or(0);
    if let Err(e) = queue::enqueue(db, repo_full, priority).await {
        tracing::warn!(repo = %repo_full, error = %e, "star-fetch enqueue failed");
    }
}

/// Like [`enqueue_fetch`] but for callers (the `/analyze` hot path) that
/// already know the repo is NOT tombstoned and already have its
/// `view_count` (priority) in hand — skips the redundant `repo_is_missing`
/// + `get_repo_view_count` single-row reads. The global pending-queue
///
/// ceiling is still enforced (active repos exempt, as before).
pub async fn enqueue_fetch_known(ctx: &AnalyzerCtx, repo_full: &str, priority: i64) {
    let db = ctx.cache.db();

    let already_active = queue::is_active(db, repo_full).await.unwrap_or(false);
    if !already_active {
        let pending = queue::pending_only_count(db).await.unwrap_or(0);
        if pending >= max_pending_fetches() {
            tracing::warn!(
                repo = %repo_full,
                pending,
                "star-fetch queue at ceiling; skipping enqueue"
            );
            return;
        }
    }

    if let Err(e) = queue::enqueue(db, repo_full, priority).await {
        tracing::warn!(repo = %repo_full, error = %e, "star-fetch enqueue failed");
    }
}

/// Spawn a fire-and-forget metadata refresh if the cached metadata is
/// older than 1h (authoritative star count + creation date + forks).
fn maybe_refresh_metadata(owner: &str, repo: &str, ctx: &AnalyzerCtx) {
    let repo_full = format!("{owner}/{repo}");
    static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    static CONCURRENCY: Semaphore = Semaphore::const_new(8);
    let in_flight = IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()));
    if !in_flight
        .lock()
        .expect("metadata refresh lock poisoned")
        .insert(repo_full.clone())
    {
        return;
    }
    let cache = ctx.cache.clone();
    let github = ctx.github.clone();
    let owner_s = owner.to_string();
    let repo_s = repo.to_string();
    tokio::spawn(async move {
        let one_hour = chrono::Duration::hours(1);
        if cache
            .repo_metadata_fresh_within(&repo_full, one_hour)
            .await
            .unwrap_or(false)
        {
            IN_FLIGHT
                .get()
                .expect("metadata refresh set initialized")
                .lock()
                .expect("metadata refresh lock poisoned")
                .remove(&repo_full);
            return;
        }
        let _permit = CONCURRENCY
            .acquire()
            .await
            .expect("metadata refresh semaphore closed");
        if cache
            .repo_metadata_fresh_within(&repo_full, one_hour)
            .await
            .unwrap_or(false)
        {
            IN_FLIGHT
                .get()
                .expect("metadata refresh set initialized")
                .lock()
                .expect("metadata refresh lock poisoned")
                .remove(&repo_full);
            return;
        }
        match github.repo_metadata(&owner_s, &repo_s).await {
            Ok(Some(m)) => {
                if let Err(e) = cache.put_repo_metadata(&repo_full, &m).await {
                    tracing::warn!(repo = %repo_full, error = %e, "put_repo_metadata");
                }
            }
            Ok(None) => tracing::debug!(repo = %repo_full, "repo_metadata 404"),
            Err(e) => {
                tracing::debug!(repo = %repo_full, error = %e, "repo_metadata fetch failed")
            }
        }
        IN_FLIGHT
            .get()
            .expect("metadata refresh set initialized")
            .lock()
            .expect("metadata refresh lock poisoned")
            .remove(&repo_full);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Locks the exact `/analyze` JSON shape the frontend codes against.
    /// Field names + nesting must not drift without a coordinated change.
    #[test]
    fn analysis_result_json_shape() {
        let result = AnalysisResult {
            repo: "owner/repo".into(),
            total_stars: 12345,
            created_at: Some(Utc.timestamp_opt(1_546_300_800, 0).unwrap()),
            queued: 0,
            history_complete: true,
            history_kind: "current_stargazers",
            history_event_count: 10,
            history_coverage_start: None,
            history_coverage_end: None,
            history_approximate: false,
            pending: false,
            backfilling: false,
            history_status: "ready",
            history_unavailable: false,
            not_found: false,
            star_history_insights: Some(derive_star_history_insights(
                Some(Utc.timestamp_opt(1_546_300_800, 0).unwrap()),
                &[Point {
                    at: Utc.timestamp_opt(1_614_556_800, 0).unwrap(),
                    stars: 10,
                }],
            )),
            history: vec![Point {
                at: Utc.timestamp_opt(1_614_556_800, 0).unwrap(),
                stars: 10,
            }],
        };
        let v: serde_json::Value = serde_json::to_value(&result).expect("serialize AnalysisResult");
        // Top-level keys, exactly the contract.
        assert_eq!(v["repo"], "owner/repo");
        assert_eq!(v["total_stars"], 12345);
        assert_eq!(v["created_at"], "2019-01-01T00:00:00Z");
        assert_eq!(v["queued"], 0);
        assert_eq!(v["history_complete"], true);
        assert_eq!(v["history_kind"], "current_stargazers");
        assert_eq!(v["history_event_count"], 10);
        assert_eq!(v["history_approximate"], false);
        // `pending` is the extension/poll contract flag.
        assert_eq!(v["pending"], false);
        // New flags: present, default false on a healthy complete repo.
        assert_eq!(v["backfilling"], false);
        assert_eq!(v["history_unavailable"], false);
        assert_eq!(v["not_found"], false);
        assert_eq!(v["star_history_insights"]["milestones"][0]["stars"], 100);
        assert_eq!(
            v["star_history_insights"]["largest_day"]["stars_gained"],
            10
        );
        // history entries are { "date", "stars" } — `at` is renamed.
        assert_eq!(v["history"][0]["date"], "2021-03-01T00:00:00Z");
        assert_eq!(v["history"][0]["stars"], 10);
        assert!(v["history"][0].get("at").is_none());
        // No detection fields may exist.
        for k in [
            "fake_count",
            "fake_ratio",
            "verdicts",
            "analysis_complete",
            "bursts",
        ] {
            assert!(v.get(k).is_none(), "unexpected field {k}");
        }
    }

    /// A cold repo (no cached history yet) serializes as
    /// `pending: true, history_complete: false` with an empty history —
    /// the shape the extension and the poll widget expect while a fetch
    /// is in flight.
    #[test]
    fn cold_repo_json_is_pending_and_empty() {
        let result = AnalysisResult {
            repo: "o/r".into(),
            total_stars: 0,
            created_at: None,
            queued: 3,
            history_complete: false,
            history_kind: "unavailable",
            history_event_count: 0,
            history_coverage_start: None,
            history_coverage_end: None,
            history_approximate: false,
            pending: true,
            backfilling: false,
            history_status: "queued",
            history_unavailable: false,
            not_found: false,
            star_history_insights: None,
            history: vec![],
        };
        let v: serde_json::Value = serde_json::to_value(&result).unwrap();
        assert_eq!(v["pending"], true);
        assert_eq!(v["history_complete"], false);
        assert_eq!(v["queued"], 3);
        assert!(v["star_history_insights"].is_null());
        assert!(v["history"].as_array().unwrap().is_empty());
    }

    /// A refresh backfill can keep its prior complete snapshot visible while
    /// reporting that a newer snapshot is still being assembled.
    #[test]
    fn backfilling_repo_shape() {
        let result = AnalysisResult {
            repo: "o/r".into(),
            total_stars: 40_000,
            created_at: None,
            queued: 1,
            history_complete: true,
            history_kind: "current_stargazers",
            history_event_count: 40_000,
            history_coverage_start: None,
            history_coverage_end: None,
            history_approximate: false,
            pending: false,
            backfilling: true,
            history_status: "queued",
            history_unavailable: false,
            not_found: false,
            star_history_insights: Some(derive_star_history_insights(None, &[])),
            history: vec![],
        };
        let v: serde_json::Value = serde_json::to_value(&result).unwrap();
        assert_eq!(v["backfilling"], true);
        assert_eq!(v["pending"], false);
    }

    /// A tombstoned (404) repo: `not_found` true, no history, not pending.
    #[test]
    fn not_found_repo_shape() {
        let result = AnalysisResult {
            repo: "ghost/repo".into(),
            total_stars: 0,
            created_at: None,
            queued: 0,
            history_complete: false,
            history_kind: "unavailable",
            history_event_count: 0,
            history_coverage_start: None,
            history_coverage_end: None,
            history_approximate: false,
            pending: false,
            backfilling: false,
            history_status: "not_public",
            history_unavailable: false,
            not_found: true,
            star_history_insights: None,
            history: vec![],
        };
        let v: serde_json::Value = serde_json::to_value(&result).unwrap();
        assert_eq!(v["not_found"], true);
        assert_eq!(v["pending"], false);
        assert!(v["history"].as_array().unwrap().is_empty());
    }

    /// Mixed-case and already-lowercase slugs must resolve to ONE
    /// cache/queue/worker key. Before this, `/analyze` keyed on the raw URL
    /// case while ext_ping/export/cards lowercased, so `Owner/Repo` and
    /// `owner/repo` split into two full fetches + two leaderboard rows.
    #[test]
    fn repo_key_normalizes_case_to_one_key() {
        let lower = repo_key("owner", "repo");
        assert_eq!(lower, "owner/repo");
        assert_eq!(repo_key("Owner", "Repo"), lower);
        assert_eq!(repo_key("OWNER", "REPO"), lower);
        assert_eq!(repo_key("oWnEr", "rEpO"), lower);
        // Matches the convention the rest of the codebase lowercases with.
        assert_eq!(repo_key("Facebook", "React"), "facebook/react");
    }

    #[test]
    fn created_at_serializes_null_when_absent() {
        let result = AnalysisResult {
            repo: "o/r".into(),
            total_stars: 0,
            created_at: None,
            queued: 0,
            history_complete: true,
            history_kind: "current_stargazers",
            history_event_count: 0,
            history_coverage_start: None,
            history_coverage_end: None,
            history_approximate: false,
            pending: false,
            backfilling: false,
            history_status: "queued",
            history_unavailable: false,
            not_found: false,
            star_history_insights: Some(derive_star_history_insights(None, &[])),
            history: vec![],
        };
        let v: serde_json::Value = serde_json::to_value(&result).unwrap();
        assert!(v["created_at"].is_null());
    }

    #[test]
    fn star_history_milestones_measure_from_repository_creation() {
        let created = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let points = [(4_i64, 100_u32), (20, 1_000), (200, 10_000), (500, 100_000)]
            .into_iter()
            .map(|(days, stars)| Point {
                at: created + chrono::Duration::days(days),
                stars,
            })
            .collect::<Vec<_>>();
        let insights = derive_star_history_insights(Some(created), &points);
        assert_eq!(
            insights
                .milestones
                .iter()
                .map(|milestone| (
                    milestone.stars,
                    milestone.days_from_creation,
                    milestone.reached_at
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    100,
                    Some(4),
                    Some(created.date_naive() + chrono::Duration::days(4))
                ),
                (
                    1_000,
                    Some(20),
                    Some(created.date_naive() + chrono::Duration::days(20))
                ),
                (
                    10_000,
                    Some(200),
                    Some(created.date_naive() + chrono::Duration::days(200))
                ),
                (
                    100_000,
                    Some(500),
                    Some(created.date_naive() + chrono::Duration::days(500))
                ),
            ]
        );
    }

    #[test]
    fn star_history_records_use_rolling_day_week_and_thirty_day_windows() {
        let at = |day: u32| Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0).unwrap();
        let points = vec![
            Point {
                at: at(1),
                stars: 5,
            },
            Point {
                at: at(2),
                stars: 25,
            },
            Point {
                at: at(8),
                stars: 125,
            },
            Point {
                at: at(9),
                stars: 175,
            },
        ];
        let insights = derive_star_history_insights(None, &points);
        let day = insights.largest_day.unwrap();
        assert_eq!(day.stars_gained, 100);
        assert_eq!(day.from, at(8).date_naive());
        assert_eq!(day.to, at(8).date_naive());
        assert_eq!(day.window_days, 1);

        let week = insights.largest_week.unwrap();
        assert_eq!(week.stars_gained, 150);
        assert_eq!(week.from, at(3).date_naive());
        assert_eq!(week.to, at(9).date_naive());
        assert_eq!(week.window_days, 7);

        let month = insights.largest_month.unwrap();
        assert_eq!(month.stars_gained, 175);
        assert_eq!(month.to, at(9).date_naive());
        assert_eq!(month.window_days, 30);
    }

    #[test]
    fn star_history_insights_normalize_duplicate_days_and_decreasing_totals() {
        let at = |day: u32| Utc.with_ymd_and_hms(2026, 2, day, 0, 0, 0).unwrap();
        let insights = derive_star_history_insights(
            None,
            &[
                Point {
                    at: at(2),
                    stars: 90,
                },
                Point {
                    at: at(1),
                    stars: 100,
                },
                Point {
                    at: at(2),
                    stars: 80,
                },
            ],
        );
        assert_eq!(
            insights
                .largest_day
                .as_ref()
                .map(|record| record.stars_gained),
            Some(100)
        );
        assert_eq!(
            insights
                .largest_week
                .as_ref()
                .map(|record| record.stars_gained),
            Some(100),
            "a decreasing cumulative total cannot invent additional stars"
        );
        assert_eq!(insights.milestones[0].reached_at, Some(at(1).date_naive()));
    }
}
