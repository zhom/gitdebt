//! Persistent GH Archive historical backfill coordinator.
//!
//! One coordinator claims a batch of repository jobs and scans each date
//! window once for the whole batch. `WORKER_COUNT` controls only the cheap
//! GitHub metadata lookups needed to resolve stable numeric repository IDs;
//! it never creates eight parallel BigQuery corpus scans.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{Datelike, Days, Months, NaiveDate, Utc};
use futures::StreamExt;
use tokio::sync::watch;

use crate::cache::{ArchiveBackfillState, ArchiveStarEvent, Cache};
use crate::db::Db;
use crate::gh_archive::{
    GhArchiveError, GhArchiveEventSource, GhArchiveFetch, GhArchiveStarEvent, RepositorySpec,
};
use crate::github::{GithubClient, RepoVisibility};
use crate::queue;

const ARCHIVE_START: NaiveDate =
    NaiveDate::from_ymd_opt(2011, 2, 12).expect("GH Archive start date is valid");
/// Repositories one coordinator pass claims and holds events for.
///
/// A pass materializes every star event of every repository in the batch
/// before committing any of them, and the regrouping step briefly doubles
/// that. The batch exists to share one BigQuery corpus scan across many
/// repositories, so a larger value is cheaper per repository — but the peak
/// is unbounded in the data, and a batch that lands on popular repositories
/// is several gigabytes on a host that also runs Postgres. This default
/// keeps the worst case in the hundreds of megabytes; `GH_ARCHIVE_BATCH_SIZE`
/// raises it where the memory is available.
const DEFAULT_BATCH_SIZE: usize = 1_500;
const MAX_BATCH_SIZE: usize = 5_000;

/// Session advisory lock electing the single BigQuery coordinator across
/// worker replicas. Same `gitdebt` house family as the schema lock (`…7401`)
/// and the hourly commit lock (`…7402`); distinct from both.
pub const COORDINATOR_LEADER_LOCK: i64 = 0x6769_7464_6562_7403;

/// The queue's stale-lease steal window is 15 minutes; refreshing the whole
/// claimed batch every 5 keeps a legitimately long BigQuery cohort (a month
/// window can exceed 15 minutes) from being stolen mid-scan by a peer.
const BATCH_LEASE_REFRESH: Duration = Duration::from_secs(5 * 60);

/// Fixed worker id for the coordinator's queue claims. Leader election
/// guarantees at most one live coordinator, so the id can stay constant —
/// the heartbeat guard (`worker_id = $2 AND status = 'in_progress'`) is what
/// keeps a refresh from touching rows another path already released.
const COORDINATOR_WORKER_ID: &str = "gh-archive";

#[derive(Clone)]
pub struct ArchiveWorkerCtx {
    source: Arc<dyn GhArchiveEventSource>,
    github: Arc<GithubClient>,
    cache: Cache,
    batch_size: usize,
    metadata_concurrency: usize,
    history_window_days: i64,
}

impl ArchiveWorkerCtx {
    pub fn from_env(
        source: Arc<dyn GhArchiveEventSource>,
        github: Arc<GithubClient>,
        cache: Cache,
        metadata_concurrency: usize,
    ) -> Self {
        let batch_size = bounded_env(
            "GH_ARCHIVE_BATCH_SIZE",
            DEFAULT_BATCH_SIZE,
            1,
            MAX_BATCH_SIZE,
        );
        let history_window_days = source.max_range_days().max(1);
        Self {
            source,
            github,
            cache,
            batch_size,
            metadata_concurrency: metadata_concurrency.clamp(1, 32),
            history_window_days,
        }
    }
}

#[derive(Clone)]
struct Prepared {
    repo: String,
    state: ArchiveBackfillState,
}

/// Contend for the historical-coordinator leadership. Exactly one worker
/// replica runs the BigQuery batching loop at a time; the others retry the
/// session advisory lock about once a minute and take over on leadership
/// loss. The persistent queue is the durable work list; an empty queue
/// simply idles.
pub fn spawn(ctx: ArchiveWorkerCtx, database_url: String) {
    crate::bootstrap::spawn_leader(
        database_url,
        COORDINATOR_LEADER_LOCK,
        "gh-archive-coordinator",
        move || {
            let ctx = ctx.clone();
            async move {
                run(ctx).await;
            }
        },
    );
}

async fn run(ctx: ArchiveWorkerCtx) {
    let mut consecutive_provider_failures = 0_u32;
    loop {
        let jobs =
            match queue::claim_many(ctx.cache.db(), COORDINATOR_WORKER_ID, ctx.batch_size).await {
                Ok(jobs) => jobs,
                Err(error) => {
                    tracing::error!(%error, "gh-archive: failed to claim batch");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
        if jobs.is_empty() {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        // Keep the whole claimed batch's lease fresh while the (possibly
        // >15-minute) metadata + BigQuery pass runs.
        let claimed = jobs.iter().map(|job| job.repo.clone()).collect::<Vec<_>>();
        let heartbeat_stop = spawn_batch_lease_heartbeat(ctx.cache.db().clone(), claimed);

        let queue_cache = ctx.cache.clone();
        let prepared = futures::stream::iter(jobs)
            .map(|job| prepare_job(&ctx, job))
            .buffer_unordered(ctx.metadata_concurrency)
            .filter_map(|result| {
                let cache = queue_cache.clone();
                async move {
                match result {
                    Ok(prepared) => prepared,
                    Err((repo, error)) => {
                        tracing::warn!(%repo, %error, "gh-archive: metadata preparation failed");
                        if let Err(queue_error) =
                                queue::fail(cache.db(), &repo, &compact_error(&error)).await
                        {
                            tracing::error!(%repo, %queue_error, "gh-archive: failed to requeue");
                        }
                        None
                    }
                }
                }
            })
            .collect::<Vec<_>>()
            .await;

        let provider_delay = provider_backoff_seconds(consecutive_provider_failures + 1);
        let outcome = process_prepared(&ctx, prepared, provider_delay).await;
        // The batch is finished (committed, requeued, or released); its
        // rows no longer carry this coordinator's live lease.
        let _ = heartbeat_stop.send(true);
        match outcome {
            Ok(()) => consecutive_provider_failures = 0,
            Err(error) => {
                consecutive_provider_failures = consecutive_provider_failures.saturating_add(1);
                let delay_seconds =
                    provider_retry_delay_seconds(&error, consecutive_provider_failures);
                tracing::error!(
                    error = %format!("{error:#}"),
                    delay_seconds,
                    "gh-archive: provider batch failed; retry scheduled"
                );
                tokio::time::sleep(Duration::from_secs(delay_seconds)).await;
            }
        }
    }
}

/// Periodically refresh `claimed_at` for the coordinator's claimed batch so
/// a legitimately long pass cannot cross the queue's 15-minute stale-lease
/// steal window. Guarded by worker id + status: rows that were completed
/// (deleted), released, requeued, or failed in the meantime are untouched.
/// Stops on signal, or when the sender drops (coordinator aborted on
/// leadership loss).
fn spawn_batch_lease_heartbeat(db: Db, repos: Vec<String>) -> watch::Sender<bool> {
    let (stop, mut stopped) = watch::channel(false);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(BATCH_LEASE_REFRESH) => {
                    if *stopped.borrow() {
                        break;
                    }
                    if let Err(error) = sqlx::query(
                        "UPDATE star_fetch_queue SET claimed_at = NOW() \
                         WHERE repo = ANY($1) AND status = 'in_progress' AND worker_id = $2",
                    )
                    .bind(&repos)
                    .bind(COORDINATOR_WORKER_ID)
                    .execute(&db.pool)
                    .await
                    {
                        tracing::warn!(%error, "gh-archive: batch lease heartbeat failed");
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

async fn prepare_job(
    ctx: &ArchiveWorkerCtx,
    job: queue::Job,
) -> std::result::Result<Option<Prepared>, (String, anyhow::Error)> {
    let repo = job.repo;
    let operation = async {
        let mut state = ctx.cache.get_archive_backfill_state(&repo).await?;
        // Metadata comes FIRST, even for repos whose exact history is
        // already complete: every public read surface gates on
        // `metadata_fetched_at`, so completing a legacy job without
        // healing its metadata would leave a fully-ingested repo
        // permanently invisible. One metadata call, never a stargazer
        // page.
        if needs_metadata(state.as_ref()) {
            let (owner, name) = split_slug(&repo)?;
            match ctx.github.repo_visibility(owner, name).await? {
                RepoVisibility::Public(metadata) => {
                    ctx.cache.put_repo_metadata(&repo, &metadata).await?;
                }
                // Exists, but is not ours to publish. Parking it restricted
                // rather than tombstoning it `missing` is the difference
                // between "we will look again if this ever goes public" and a
                // permanent not-found that never re-enqueues.
                RepoVisibility::Private => {
                    queue::mark_restricted(ctx.cache.db(), &repo, "repository is private").await?;
                    return Ok(None);
                }
                RepoVisibility::Absent => {
                    ctx.cache.mark_repo_missing(&repo).await?;
                    queue::mark_dead(ctx.cache.db(), &repo, "repo not found").await?;
                    return Ok(None);
                }
            }
            state = ctx.cache.get_archive_backfill_state(&repo).await?;
        }

        let state = state.context("repository metadata row was not persisted")?;
        if state.exact_history_complete {
            queue::complete(ctx.cache.db(), &repo).await?;
            return Ok(None);
        }
        Ok(Some(Prepared {
            repo: repo.clone(),
            state,
        }))
    }
    .await;
    operation.map_err(|error| (repo, error))
}

/// Whether the coordinator must fetch GitHub repo metadata before acting on
/// a claimed job. True for unknown repos, for repos still missing the
/// numeric id / authoritative total a backfill needs, and for legacy rows
/// whose `metadata_fetched_at` was never stamped — the public-visibility
/// gate every reader checks. Pure so the heal condition is unit-testable.
fn needs_metadata(state: Option<&ArchiveBackfillState>) -> bool {
    state.is_none_or(|value| {
        value.github_id.is_none() || value.authoritative_total.is_none() || value.metadata_missing
    })
}

async fn process_prepared(
    ctx: &ArchiveWorkerCtx,
    prepared: Vec<Prepared>,
    provider_delay_seconds: u64,
) -> Result<()> {
    let cutoff = Utc::now()
        .date_naive()
        .pred_opt()
        .context("cannot calculate GH Archive cutoff")?;
    let claimed_repos = prepared
        .iter()
        .map(|item| item.repo.clone())
        .collect::<Vec<_>>();
    let mut by_start: BTreeMap<NaiveDate, Vec<Prepared>> = BTreeMap::new();
    for item in prepared {
        // The table scan costs the same whether one or one thousand requested
        // repositories match. A shared cold cursor keeps the queue in one
        // large cohort instead of rescanning months per creation-date cohort.
        let start = item.state.cursor.unwrap_or(ARCHIVE_START);
        if start > cutoff {
            // Empty repositories still need a final atomic commit so their
            // source/provenance becomes visible.
            let committed = ctx
                .cache
                .commit_archive_backfill_window(&item.repo, start, start, &[], true)
                .await;
            if let Err(error) = committed {
                let _ = queue::fail(ctx.cache.db(), &item.repo, &compact_error(&error)).await;
                continue;
            }
            if let Err(error) = queue::complete(ctx.cache.db(), &item.repo).await {
                tracing::error!(repo = %item.repo, %error, "gh-archive: queue completion failed");
            }
            continue;
        }
        by_start.entry(start).or_default().push(item);
    }

    for (start, items) in by_start {
        let end = archive_window_end(start, cutoff, ctx.history_window_days)?;
        match fetch_complete(ctx.source.as_ref(), &items, start, end).await {
            Ok(events) => commit_group(ctx, items, start, end, cutoff, events).await?,
            Err(error) => {
                let detail = compact_error(&error);
                let provider_delay_seconds =
                    provider_retry_delay_seconds(&error, 1).max(provider_delay_seconds);
                queue::release_archive_provider_error(
                    ctx.cache.db(),
                    &claimed_repos,
                    &detail,
                    i64::try_from(provider_delay_seconds).unwrap_or(3_600),
                )
                .await?;
                return Err(error).context("GH Archive provider query failed");
            }
        }
    }
    Ok(())
}

async fn commit_group(
    ctx: &ArchiveWorkerCtx,
    items: Vec<Prepared>,
    start: NaiveDate,
    end: NaiveDate,
    cutoff: NaiveDate,
    events: Vec<GhArchiveStarEvent>,
) -> Result<()> {
    let mut by_id: HashMap<i64, Vec<ArchiveStarEvent>> = HashMap::new();
    let mut by_name: HashMap<String, Vec<ArchiveStarEvent>> = HashMap::new();
    for event in events {
        let cached = ArchiveStarEvent {
            source_event_id: event.source_event_id,
            starred_at: event.created_at,
        };
        if let Some(id) = event.github_repo_id {
            by_id.entry(id).or_default().push(cached);
        } else {
            by_name
                .entry(event.repository.to_ascii_lowercase())
                .or_default()
                .push(cached);
        }
    }
    let next_cursor = end.succ_opt().context("archive date overflow")?;
    let complete = end >= cutoff;
    for item in items {
        let mut events = item
            .state
            .github_id
            .and_then(|id| by_id.remove(&id))
            .unwrap_or_default();
        if item.state.github_id.is_none() {
            events.extend(
                by_name
                    .remove(&item.repo.to_ascii_lowercase())
                    .unwrap_or_default(),
            );
        }
        events.sort_unstable_by(|left, right| {
            left.starred_at
                .cmp(&right.starred_at)
                .then_with(|| left.source_event_id.cmp(&right.source_event_id))
        });
        let observed = match ctx
            .cache
            .commit_archive_backfill_window(&item.repo, start, next_cursor, &events, complete)
            .await
        {
            Ok(observed) => observed,
            Err(error) => {
                tracing::error!(
                    repo = %item.repo,
                    %error,
                    "gh-archive: failed to commit backfill window"
                );
                let _ = queue::fail(ctx.cache.db(), &item.repo, &compact_error(&error)).await;
                continue;
            }
        };
        if complete {
            if let Err(error) = queue::complete(ctx.cache.db(), &item.repo).await {
                tracing::error!(repo = %item.repo, %error, "gh-archive: queue completion failed");
            }
            tracing::info!(
                repo = %item.repo,
                observed_events = observed,
                current_stars = item.state.authoritative_total,
                "gh-archive: historical backfill complete"
            );
        } else {
            if let Err(error) = queue::requeue_archive_window(ctx.cache.db(), &item.repo).await {
                tracing::error!(repo = %item.repo, %error, "gh-archive: window requeue failed");
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct QueryUnit {
    items: Vec<Prepared>,
    start: NaiveDate,
    end: NaiveDate,
}

/// Fetch a window without accepting the client's safety-cap truncation. A
/// truncated batch is divided by date first, then — only for a single day that
/// still overflows — by repository. One repo exceeding the cap in one day is a
/// hard error.
///
/// The order used to be the other way around, to "avoid rescanning dates".
/// That reasoning inverts what BigQuery charges for: dates are the partition
/// key, so re-splitting them is the one axis that *does* prune, while the
/// repository filter prunes nothing and makes each half re-read the whole
/// corpus. See the split itself for the arithmetic.
async fn fetch_complete(
    source: &dyn GhArchiveEventSource,
    items: &[Prepared],
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<GhArchiveStarEvent>> {
    let mut pending = VecDeque::from([QueryUnit {
        items: items.to_vec(),
        start,
        end,
    }]);
    let mut events = Vec::new();
    while let Some(unit) = pending.pop_front() {
        let specs = unit
            .items
            .iter()
            .map(|item| RepositorySpec::new(item.state.github_id, &item.repo))
            .collect::<Vec<_>>();
        let GhArchiveFetch {
            events: fetched,
            truncated,
            total_bytes_processed,
        } = source
            .fetch_star_events(&specs, unit.start, unit.end)
            .await?;
        tracing::debug!(
            repositories = specs.len(),
            start = %unit.start,
            end = %unit.end,
            total_bytes_processed,
            truncated,
            "gh-archive: BigQuery window"
        );
        if !truncated {
            events.extend(fetched);
            continue;
        }
        // A truncated result is discarded, so what this split chooses to halve
        // decides what the whole retry costs — and the two dimensions are not
        // interchangeable.
        //
        // `created_at` is the table's partition key. Halving the *window*
        // halves the partitions each side reads, so the two halves together
        // scan what the parent scanned: splitting on dates is free, and a
        // window subdivided sixteen times still costs one corpus.
        //
        // The repository filter is a semi-join over a parameter array, which
        // BigQuery cannot turn into partition or cluster elimination. Halving
        // the *list* therefore changes nothing either side reads: both re-scan
        // everything the parent did. That path costs 2^depth corpora, and at
        // 39.9 GB and ~$0.24 a scan it is what turned one backfill into 518
        // full-table scans and $126.
        //
        // So: divide the window while there is any window left to divide, and
        // fall back to the list only for a single day that still overflows.
        if unit.start < unit.end {
            let days = unit.end.signed_duration_since(unit.start).num_days();
            let left_end = unit
                .start
                .checked_add_days(Days::new((days / 2) as u64))
                .context("archive split overflow")?;
            let right_start = left_end.succ_opt().context("archive split overflow")?;
            pending.push_front(QueryUnit {
                items: unit.items.clone(),
                start: right_start,
                end: unit.end,
            });
            pending.push_front(QueryUnit {
                items: unit.items,
                start: unit.start,
                end: left_end,
            });
        } else if unit.items.len() > 1 {
            let right = unit.items.len() / 2;
            pending.push_front(QueryUnit {
                items: unit.items[right..].to_vec(),
                start: unit.start,
                end: unit.end,
            });
            pending.push_front(QueryUnit {
                items: unit.items[..right].to_vec(),
                start: unit.start,
                end: unit.end,
            });
        } else {
            anyhow::bail!(
                "GH Archive result cap exceeded for {} on {}",
                unit.items[0].repo,
                unit.start
            );
        }
    }
    Ok(events)
}

fn split_slug(slug: &str) -> Result<(&str, &str)> {
    let (owner, repo) = slug
        .split_once('/')
        .with_context(|| format!("invalid repository slug: {slug}"))?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        anyhow::bail!("invalid repository slug: {slug}");
    }
    Ok((owner, repo))
}

fn compact_error(error: &anyhow::Error) -> String {
    format!("{error:#}")
        .chars()
        .filter(|character| !character.is_control())
        .take(500)
        .collect()
}

fn provider_backoff_seconds(failures: u32) -> u64 {
    let shift = failures.saturating_sub(1).min(5);
    (30_u64 << shift).min(15 * 60)
}

fn provider_retry_delay_seconds(error: &anyhow::Error, failures: u32) -> u64 {
    if error
        .downcast_ref::<GhArchiveError>()
        .is_some_and(GhArchiveError::is_free_query_quota_exhausted)
    {
        60 * 60
    } else {
        provider_backoff_seconds(failures)
    }
}

fn archive_window_end(
    start: NaiveDate,
    cutoff: NaiveDate,
    max_range_days: i64,
) -> Result<NaiveDate> {
    // Keep direct queries over the official month resources calendar-aligned:
    // a 31-day cross-month range would scan two large tables for almost every
    // job. Wider ranges are intended for the optimized partitioned source and
    // should use the full configured window so a fresh deploy can finish a
    // multi-year backfill in one query.
    if max_range_days <= 31 {
        let month_start = start
            .with_day(1)
            .context("cannot calculate archive month start")?;
        let next_month = month_start
            .checked_add_months(Months::new(1))
            .context("archive month overflow")?;
        return Ok(next_month
            .pred_opt()
            .context("archive month overflow")?
            .min(cutoff));
    }
    let days =
        u64::try_from(max_range_days.saturating_sub(1)).context("invalid archive range size")?;
    Ok(start
        .checked_add_days(chrono::Days::new(days))
        .context("archive range overflow")?
        .min(cutoff))
}

fn bounded_env(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_validation() {
        assert_eq!(split_slug("owner/repo").unwrap(), ("owner", "repo"));
        assert!(split_slug("owner").is_err());
        assert!(split_slug("owner/repo/extra").is_err());
    }

    #[test]
    fn archive_start_is_documented_boundary() {
        assert_eq!(ARCHIVE_START.to_string(), "2011-02-12");
    }

    fn state(
        github_id: Option<i64>,
        authoritative_total: Option<i64>,
        exact_history_complete: bool,
        metadata_missing: bool,
    ) -> ArchiveBackfillState {
        ArchiveBackfillState {
            github_id,
            cursor: None,
            complete: false,
            authoritative_total,
            exact_history_complete,
            metadata_missing,
        }
    }

    #[test]
    fn metadata_is_fetched_before_settling_legacy_complete_jobs() {
        // A legacy row: exact history complete, but ingested before the
        // public-metadata gate existed (`metadata_fetched_at` NULL). The
        // coordinator must fetch metadata rather than short-circuiting the
        // job, or the repo stays invisible to every reader forever.
        assert!(needs_metadata(Some(&state(Some(1), Some(10), true, true))));
        // Same for an archive-history row missing only the stamp.
        assert!(needs_metadata(Some(&state(Some(1), Some(10), false, true))));
        // Unknown repo → metadata required.
        assert!(needs_metadata(None));
        // Missing numeric id or authoritative total → metadata required.
        assert!(needs_metadata(Some(&state(None, Some(10), false, false))));
        assert!(needs_metadata(Some(&state(Some(1), None, false, false))));
    }

    #[test]
    fn healed_repos_skip_the_metadata_call() {
        // Fully-stamped rows spend no GitHub budget in prepare: complete
        // repos settle immediately, incomplete ones go straight to the
        // BigQuery batch.
        assert!(!needs_metadata(Some(&state(
            Some(1),
            Some(10),
            true,
            false
        ))));
        assert!(!needs_metadata(Some(&state(
            Some(1),
            Some(10),
            false,
            false
        ))));
    }

    #[test]
    fn provider_backoff_is_bounded() {
        assert_eq!(provider_backoff_seconds(1), 30);
        assert_eq!(provider_backoff_seconds(2), 60);
        assert_eq!(provider_backoff_seconds(6), 900);
        assert_eq!(provider_backoff_seconds(100), 900);
    }

    #[test]
    fn free_query_quota_uses_hourly_durable_retry() {
        let error = anyhow::Error::new(GhArchiveError::Api {
            status: 403,
            message: "project exceeded quota for free query bytes scanned".to_string(),
        });
        assert_eq!(provider_retry_delay_seconds(&error, 1), 3_600);
    }

    #[test]
    fn archive_windows_are_calendar_aligned() {
        assert_eq!(
            archive_window_end(
                NaiveDate::from_ymd_opt(2011, 2, 12).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
                31,
            )
            .unwrap(),
            NaiveDate::from_ymd_opt(2011, 2, 28).unwrap()
        );
        assert_eq!(
            archive_window_end(
                NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
                31,
            )
            .unwrap(),
            NaiveDate::from_ymd_opt(2026, 7, 19).unwrap()
        );
    }

    #[test]
    fn indexed_archive_uses_the_full_configured_window() {
        assert_eq!(
            archive_window_end(
                NaiveDate::from_ymd_opt(2011, 2, 12).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
                6_000,
            )
            .unwrap(),
            NaiveDate::from_ymd_opt(2026, 7, 19).unwrap()
        );
        assert_eq!(
            archive_window_end(
                NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
                366,
            )
            .unwrap(),
            NaiveDate::from_ymd_opt(2020, 12, 31).unwrap()
        );
    }
}
