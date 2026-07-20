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

use crate::cache::{ArchiveBackfillState, ArchiveStarEvent, Cache};
use crate::gh_archive::{GhArchiveEventSource, GhArchiveFetch, GhArchiveStarEvent, RepositorySpec};
use crate::github::GithubClient;
use crate::queue;

const ARCHIVE_START: NaiveDate =
    NaiveDate::from_ymd_opt(2011, 2, 12).expect("GH Archive start date is valid");
const DEFAULT_BATCH_SIZE: usize = 1_000;
const MAX_BATCH_SIZE: usize = 1_000;

#[derive(Clone)]
pub struct ArchiveWorkerCtx {
    source: Arc<dyn GhArchiveEventSource>,
    github: Arc<GithubClient>,
    cache: Cache,
    batch_size: usize,
    metadata_concurrency: usize,
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
        Self {
            source,
            github,
            cache,
            batch_size,
            metadata_concurrency: metadata_concurrency.clamp(1, 32),
        }
    }
}

#[derive(Clone)]
struct Prepared {
    repo: String,
    state: ArchiveBackfillState,
}

/// Spawn the sole historical coordinator. The persistent queue is the durable
/// work list; an empty queue simply idles.
pub fn spawn(ctx: ArchiveWorkerCtx) {
    tokio::spawn(async move {
        run(ctx).await;
    });
}

async fn run(ctx: ArchiveWorkerCtx) {
    let mut consecutive_provider_failures = 0_u32;
    loop {
        let jobs = match queue::claim_many(ctx.cache.db(), "gh-archive", ctx.batch_size).await {
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
        match process_prepared(&ctx, prepared, provider_delay).await {
            Ok(()) => consecutive_provider_failures = 0,
            Err(error) => {
                consecutive_provider_failures = consecutive_provider_failures.saturating_add(1);
                let delay_seconds = provider_backoff_seconds(consecutive_provider_failures);
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

async fn prepare_job(
    ctx: &ArchiveWorkerCtx,
    job: queue::Job,
) -> std::result::Result<Option<Prepared>, (String, anyhow::Error)> {
    let repo = job.repo;
    let operation = async {
        let mut state = ctx.cache.get_archive_backfill_state(&repo).await?;
        if state
            .as_ref()
            .is_some_and(|value| value.exact_history_complete)
        {
            queue::complete(ctx.cache.db(), &repo).await?;
            return Ok(None);
        }

        if state
            .as_ref()
            .is_none_or(|value| value.github_id.is_none() || value.authoritative_total.is_none())
        {
            let (owner, name) = split_slug(&repo)?;
            match ctx.github.repo_metadata(owner, name).await? {
                Some(metadata) => {
                    ctx.cache
                        .put_repo_metadata(
                            &repo,
                            metadata.id,
                            metadata.stargazers_count,
                            metadata.forks_count,
                            metadata.created_at,
                        )
                        .await?;
                }
                None => {
                    ctx.cache.mark_repo_missing(&repo).await?;
                    queue::mark_dead(ctx.cache.db(), &repo, "repo not found").await?;
                    return Ok(None);
                }
            }
            state = ctx.cache.get_archive_backfill_state(&repo).await?;
        }

        let state = state.context("repository metadata row was not persisted")?;
        Ok(Some(Prepared {
            repo: repo.clone(),
            state,
        }))
    }
    .await;
    operation.map_err(|error| (repo, error))
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
        let end = archive_window_end(start, cutoff)?;
        match fetch_complete(ctx.source.as_ref(), &items, start, end).await {
            Ok(events) => commit_group(ctx, items, start, end, cutoff, events).await?,
            Err(error) => {
                let detail = compact_error(&error);
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
/// truncated batch is divided by repository first (avoids rescanning dates),
/// then by date. One repo exceeding the cap in one day is a hard error.
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
        if unit.items.len() > 1 {
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
        } else if unit.start < unit.end {
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

fn archive_window_end(start: NaiveDate, cutoff: NaiveDate) -> Result<NaiveDate> {
    let month_start = start
        .with_day(1)
        .context("cannot calculate archive month start")?;
    let next_month = month_start
        .checked_add_months(Months::new(1))
        .context("archive month overflow")?;
    Ok(next_month
        .pred_opt()
        .context("archive month overflow")?
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

    #[test]
    fn provider_backoff_is_bounded() {
        assert_eq!(provider_backoff_seconds(1), 30);
        assert_eq!(provider_backoff_seconds(2), 60);
        assert_eq!(provider_backoff_seconds(6), 900);
        assert_eq!(provider_backoff_seconds(100), 900);
    }

    #[test]
    fn archive_windows_are_calendar_aligned() {
        assert_eq!(
            archive_window_end(
                NaiveDate::from_ymd_opt(2011, 2, 12).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
            )
            .unwrap(),
            NaiveDate::from_ymd_opt(2011, 2, 28).unwrap()
        );
        assert_eq!(
            archive_window_end(
                NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 19).unwrap(),
            )
            .unwrap(),
            NaiveDate::from_ymd_opt(2026, 7, 19).unwrap()
        );
    }
}
