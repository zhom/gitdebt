//! Read-only Server-Sent Events for repo ingestion progress.
//!
//! The stream observes the two durable Postgres queues; it never enqueues
//! work. That distinction matters because browsers reconnect EventSource
//! automatically, and a progress reconnect must not become an anonymous
//! work-amplification primitive.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use futures::stream;
use serde::Serialize;
use sqlx::Row;
use tokio::sync::SemaphorePermit;
use tokio::time::{Instant, Interval, MissedTickBehavior};

use crate::api::{ApiError, ApiState};
use crate::queue;
use crate::repo_endpoints::is_valid_slug;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const MAX_STREAM_LIFETIME: Duration = Duration::from_secs(5 * 60);
const CLIENT_RETRY: Duration = Duration::from_secs(3);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

/// A long-lived response consumes a socket and a Postgres poll every two
/// seconds. Bound the process-wide fan-out even if an upstream proxy's own
/// connection limits are misconfigured.
const MAX_PROGRESS_CONNECTIONS: usize = 128;
const MAX_PROGRESS_CONNECTIONS_PER_CLIENT: usize = 6;
static PROGRESS_PERMITS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(MAX_PROGRESS_CONNECTIONS);

pub(crate) fn connection_metrics() -> (usize, usize) {
    (
        MAX_PROGRESS_CONNECTIONS,
        PROGRESS_PERMITS.available_permits(),
    )
}

fn progress_clients() -> &'static Mutex<HashMap<IpAddr, usize>> {
    static CLIENTS: OnceLock<Mutex<HashMap<IpAddr, usize>>> = OnceLock::new();
    CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct ClientPermit {
    ip: IpAddr,
}

impl ClientPermit {
    fn try_acquire(ip: IpAddr) -> Option<Self> {
        let mut clients = progress_clients()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = clients.entry(ip).or_default();
        if *count >= MAX_PROGRESS_CONNECTIONS_PER_CLIENT {
            return None;
        }
        *count += 1;
        Some(Self { ip })
    }
}

impl Drop for ClientPermit {
    fn drop(&mut self) {
        let mut clients = progress_clients()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = clients.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                clients.remove(&self.ip);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProgressPhase {
    Idle,
    Pending,
    Retrying,
    Fetching,
    Backfilling,
    Analyzing,
    Complete,
    NotFound,
}

impl ProgressPhase {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Retrying | Self::Fetching | Self::Backfilling | Self::Analyzing
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WorkProgress {
    phase: ProgressPhase,
    complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    queue_position: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    processed_units: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_units: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    elapsed_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    eta_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocked_reason: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProgressSnapshot {
    repo: String,
    phase: ProgressPhase,
    terminal: bool,
    stars: WorkProgress,
    analysis: WorkProgress,
}

/// Aggregate profile progress over the atomically cached repository list.
/// Counts intentionally mirror `aggregate::UserAggregate`: every unfinished
/// repo is represented, even while the bounded queues admit it in batches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProfileProgressSnapshot {
    login: String,
    terminal: bool,
    repos_included: u32,
    repos_pending: u32,
    repos_analyzed: u32,
    repos_analyzing: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    repos_total: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct RawProgress {
    missing: bool,
    stars_complete: bool,
    star_status: Option<String>,
    star_partial: bool,
    star_next_page: Option<i64>,
    star_last_error: Option<String>,
    star_attempts: i64,
    star_priority: i64,
    star_position: Option<i64>,
    star_next_attempt_at: Option<DateTime<Utc>>,
    archive_cursor: Option<NaiveDate>,
    analysis_status: Option<String>,
    analysis_attempts: i32,
    analysis_phase: Option<String>,
    analysis_priority: i64,
    analysis_position: Option<i64>,
    analysis_next_attempt_at: Option<DateTime<Utc>>,
    analysis_started_at: Option<DateTime<Utc>>,
    analysis_total_units: Option<i64>,
    analysis_completed_units: i64,
    analysis_median_ms: i64,
    repo_last_duration_ms: Option<i64>,
    analysis_complete: bool,
    analysis_scope_commits: Option<i64>,
    analysis_truncated: bool,
}

impl ProgressSnapshot {
    fn from_raw(repo: String, raw: RawProgress) -> Self {
        if raw.missing {
            let missing = WorkProgress {
                phase: ProgressPhase::NotFound,
                complete: false,
                next_page: None,
                detail: Some("not_public"),
                queue_position: None,
                processed_units: None,
                total_units: None,
                percent: None,
                elapsed_seconds: None,
                eta_seconds: None,
                retry_at: None,
                priority: None,
                blocked_reason: None,
            };
            return Self {
                repo,
                phase: ProgressPhase::NotFound,
                terminal: true,
                stars: missing.clone(),
                analysis: missing,
            };
        }

        let star_retrying = raw.star_attempts > 0
            || raw
                .star_last_error
                .as_deref()
                .is_some_and(|error| error.starts_with(queue::PROVIDER_MARKER));
        let star_phase = match raw.star_status.as_deref() {
            Some("pending") if star_retrying => ProgressPhase::Retrying,
            Some("pending" | "in_progress") if raw.star_partial => ProgressPhase::Backfilling,
            Some("pending") => ProgressPhase::Pending,
            Some("in_progress") => ProgressPhase::Fetching,
            Some("dead") | Some(_) => ProgressPhase::Retrying,
            None if raw.stars_complete => ProgressPhase::Complete,
            None => ProgressPhase::Idle,
        };
        let analysis_phase = match raw.analysis_status.as_deref() {
            Some("pending") if raw.analysis_attempts > 0 => ProgressPhase::Retrying,
            Some("pending") => ProgressPhase::Pending,
            Some("in_progress") => ProgressPhase::Analyzing,
            Some("dead") | Some(_) => ProgressPhase::Retrying,
            None if raw.analysis_complete => ProgressPhase::Complete,
            None => ProgressPhase::Idle,
        };

        // Active work wins while either queue is still moving. Once both
        // queues settle, surface the most actionable terminal condition.
        let phase = if star_phase == ProgressPhase::Backfilling {
            ProgressPhase::Backfilling
        } else if analysis_phase == ProgressPhase::Analyzing {
            ProgressPhase::Analyzing
        } else if star_phase == ProgressPhase::Fetching {
            ProgressPhase::Fetching
        } else if star_phase == ProgressPhase::Retrying || analysis_phase == ProgressPhase::Retrying
        {
            ProgressPhase::Retrying
        } else if star_phase == ProgressPhase::Pending || analysis_phase == ProgressPhase::Pending {
            ProgressPhase::Pending
        } else if star_phase == ProgressPhase::Complete || analysis_phase == ProgressPhase::Complete
        {
            // Either pipeline may be requested independently. The component
            // phases tell clients exactly which data is available.
            ProgressPhase::Complete
        } else {
            ProgressPhase::Idle
        };

        let next_page = star_phase
            .active()
            .then(|| raw.star_next_page.unwrap_or(1).clamp(1, u32::MAX as i64) as u32);
        let (mut star_processed, star_total, mut star_percent) =
            archive_month_progress(raw.archive_cursor);
        // The final archive window ends at yesterday rather than at a month
        // boundary, so its cursor remains inside the current month. Once the
        // cache completeness flag is committed, the terminal snapshot must
        // still say 100% instead of leaving the UI at 99% forever.
        if star_phase == ProgressPhase::Complete && raw.stars_complete {
            star_processed = star_total;
            star_percent = Some(100);
        }
        let provider_quota = raw.star_last_error.as_deref().is_some_and(|error| {
            error
                .to_ascii_lowercase()
                .contains("free query bytes scanned")
        });
        let star_eta = if !star_phase.active() {
            None
        } else if provider_quota {
            raw.star_next_attempt_at
                .map(|retry| (retry - Utc::now()).num_seconds().max(0) as u64)
        } else {
            // Project-owned partitioned indexes complete a monthly window in
            // roughly 1.5 seconds in production. Direct public-corpus scans
            // remain deliberately conservative because their latency and
            // bytes scanned are substantially less predictable.
            let indexed_source = std::env::var("GH_ARCHIVE_SOURCE_TABLE")
                .ok()
                .is_some_and(|value| !value.trim().is_empty());
            star_total
                .zip(star_processed)
                .map(|(total, done)| archive_eta_seconds(total, done, indexed_source))
                .filter(|seconds| *seconds > 0)
        };

        let analysis_detail = match raw.analysis_phase.as_deref() {
            Some("cloning") => Some("cloning"),
            Some("scanning_history") => Some("scanning_history"),
            Some("scanning_todos") => Some("scanning_todos"),
            Some("saving_history") => Some("saving_history"),
            Some("finishing") => Some("finishing"),
            Some("retrying") => Some("retrying"),
            _ if raw.analysis_complete && raw.analysis_truncated => Some("recent_window"),
            _ => None,
        };
        let elapsed = raw
            .analysis_started_at
            .map(|started| (Utc::now() - started).num_seconds().max(0) as u64);
        let completed_scope = raw
            .analysis_complete
            .then_some(raw.analysis_scope_commits)
            .flatten()
            .map(|value| value.max(0) as u64);
        let analysis_processed = raw
            .analysis_total_units
            .map(|_| raw.analysis_completed_units.max(0) as u64)
            .or(completed_scope);
        let analysis_total = raw
            .analysis_total_units
            .map(|value| value.max(0) as u64)
            .or(completed_scope);
        let scan_ratio = analysis_processed
            .zip(analysis_total)
            .and_then(|(done, total)| (total > 0).then_some(done as f64 / total as f64));
        let analysis_percent = match analysis_detail {
            Some("cloning") => Some(5),
            Some("scanning_history") => {
                Some((10.0 + scan_ratio.unwrap_or(0.0).clamp(0.0, 1.0) * 70.0).round() as u8)
            }
            Some("scanning_todos") => {
                Some((80.0 + scan_ratio.unwrap_or(0.0).clamp(0.0, 1.0) * 3.0).round() as u8)
            }
            Some("saving_history") => Some(84),
            Some("finishing") => Some(92),
            Some("retrying") => None,
            _ if raw.analysis_complete => Some(100),
            _ => Some(0),
        };
        // A row parked in exponential backoff is excluded from every other
        // row's queue position, so nothing else in the projection accounts for
        // the wait it owes itself. Without this it reports position 1 and an
        // ETA of its own last run while being unclaimable for another half
        // hour.
        let analysis_retry_at = raw
            .analysis_next_attempt_at
            .filter(|retry| *retry > Utc::now());
        let analysis_backoff_seconds = analysis_retry_at
            .map(|retry| (retry - Utc::now()).num_seconds().max(0) as u64)
            .unwrap_or(0);
        let analysis_eta = if raw.analysis_status.as_deref() == Some("in_progress") {
            match (elapsed, analysis_processed, analysis_total) {
                // The commit walk is the measured part of a run; saving and
                // finishing are not. Scale the extrapolation instead of adding
                // a flat tail nobody measured.
                (Some(elapsed), Some(done), Some(total)) if done > 0 && total > done => Some(
                    elapsed
                        .saturating_mul(total.saturating_sub(done))
                        .saturating_div(done)
                        .saturating_mul(5)
                        .saturating_div(4),
                ),
                _ => None,
            }
        } else {
            raw.analysis_position.and_then(|position| {
                pending_analysis_eta_seconds(
                    position,
                    configured_analysis_workers() as u64,
                    raw.analysis_median_ms,
                    raw.repo_last_duration_ms,
                    analysis_backoff_seconds,
                )
            })
        };
        Self {
            repo,
            phase,
            terminal: !phase.active(),
            stars: WorkProgress {
                phase: star_phase,
                complete: raw.stars_complete,
                next_page,
                detail: None,
                queue_position: bounded_position(raw.star_position),
                processed_units: star_processed,
                total_units: star_total,
                percent: star_percent,
                elapsed_seconds: None,
                eta_seconds: star_eta,
                retry_at: provider_quota.then_some(raw.star_next_attempt_at).flatten(),
                priority: (raw.star_priority >= crate::repo_analysis::INTERACTIVE_PRIORITY)
                    .then_some("interactive"),
                blocked_reason: provider_quota.then_some("provider_quota"),
            },
            analysis: WorkProgress {
                phase: analysis_phase,
                complete: raw.analysis_complete,
                next_page: None,
                detail: analysis_detail,
                queue_position: bounded_position(raw.analysis_position),
                processed_units: analysis_processed,
                total_units: analysis_total,
                percent: analysis_percent,
                elapsed_seconds: elapsed,
                eta_seconds: analysis_eta,
                retry_at: analysis_phase
                    .active()
                    .then_some(analysis_retry_at)
                    .flatten(),
                priority: (raw.analysis_priority >= crate::repo_analysis::INTERACTIVE_PRIORITY)
                    .then_some("interactive"),
                blocked_reason: None,
            },
        }
    }
}

/// Wait for a job that has not started yet.
///
/// The fleet median describes the jobs queued *ahead* of this one; this
/// repository's own last measured run describes its own wave, and it is taken
/// as measured. It used to be capped by the fleet median on the theory that
/// the queued run is only an incremental re-analysis, but the cases that make
/// a run expensive — a bumped `CURRENT_ANALYSIS_REVISION`, an evicted clone —
/// are exactly the ones that skip the head-unchanged branch and re-walk the
/// whole window. Over-estimating from this repository's own evidence is the
/// honest direction to be wrong in.
///
/// A repository that has never been analyzed contributes nothing of its own.
/// When nothing is queued ahead of it either and it is not parked in backoff,
/// every input to the number would be a statistic about unrelated
/// repositories — return `None` and let the client say it is still measuring
/// rather than state a confident wait that the viewed repository had no part
/// in.
fn pending_analysis_eta_seconds(
    position: i64,
    workers: u64,
    fleet_median_ms: i64,
    repo_last_duration_ms: Option<i64>,
    backoff_seconds: u64,
) -> Option<u64> {
    let waves = (position.max(1) as u64).div_ceil(workers.max(1));
    let fleet = (fleet_median_ms.max(1) as u64).div_ceil(1_000);
    let queue_wait = waves.saturating_sub(1).saturating_mul(fleet);
    let own = repo_last_duration_ms
        .map(|ms| (ms.max(1) as u64).div_ceil(1_000))
        .or_else(|| (queue_wait > 0).then_some(fleet));
    match own {
        Some(own) => Some(
            backoff_seconds
                .saturating_add(queue_wait)
                .saturating_add(own),
        ),
        // Backoff is a measurement of this row, not of the fleet: it is worth
        // reporting on its own even when nothing else about the job is known.
        None => (backoff_seconds > 0).then_some(backoff_seconds),
    }
}

fn bounded_position(position: Option<i64>) -> Option<u32> {
    position.map(|value| value.clamp(1, u32::MAX as i64) as u32)
}

fn configured_analysis_workers() -> usize {
    crate::repo_analysis::configured_analysis_workers()
}

fn archive_month_progress(cursor: Option<NaiveDate>) -> (Option<u64>, Option<u64>, Option<u8>) {
    let start_index = 2011_i64 * 12 + 1; // February 2011, zero-based month index.
    let today = Utc::now().date_naive();
    let end_index = i64::from(today.year()) * 12 + i64::from(today.month0());
    let total = end_index.saturating_sub(start_index).saturating_add(1) as u64;
    let done = cursor
        .map(|date| {
            let index = i64::from(date.year()) * 12 + i64::from(date.month0());
            index.saturating_sub(start_index).clamp(0, total as i64) as u64
        })
        .unwrap_or(0);
    let percent = (total > 0).then_some(((done as f64 / total as f64) * 100.0).round() as u8);
    (Some(done), Some(total), percent)
}

fn archive_eta_seconds(total: u64, done: u64, indexed_source: bool) -> u64 {
    let remaining = total.saturating_sub(done);
    if indexed_source {
        remaining.saturating_mul(3).div_ceil(2)
    } else {
        remaining.saturating_mul(45)
    }
}

/// Rank among the analysis jobs a claimer could take right now.
///
/// Kept as one string because [`analysis_queue_position`] runs it verbatim:
/// the `next_attempt_at` predicate is the part most easily broken by a later
/// edit, and a database-backed test of a hand-copied query proves nothing.
const ANALYSIS_POSITION_SQL: &str = "SELECT COUNT(*)::BIGINT + 1 FROM repo_analysis_queue ahead \
     WHERE ahead.status = 'pending' \
       AND ahead.next_attempt_at <= NOW() \
       AND (ahead.priority > analysis.priority OR \
            (ahead.priority = analysis.priority AND ahead.enqueued_at < analysis.enqueued_at))";

/// The queue rank the progress surface reports for one pending analysis, or
/// `None` when the repository has no queued job.
pub async fn analysis_queue_position(
    db: &crate::db::Db,
    repo: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let sql = format!(
        "SELECT CASE WHEN analysis.status = 'pending' THEN ({ANALYSIS_POSITION_SQL}) END \
         FROM repo_analysis_queue analysis WHERE analysis.repo = $1"
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(repo)
        .fetch_optional(&db.pool)
        .await
        .map(Option::flatten)
}

async fn load_snapshot(state: &ApiState, repo: &str) -> Result<ProgressSnapshot, ApiError> {
    // One round-trip and one row: all joined columns are primary-key
    // lookups. Error strings are read solely for retry classification and
    // never copied into the public payload. The queue-position subqueries
    // mirror what the claimers actually take, including `next_attempt_at`:
    // counting rows parked in exponential backoff (up to half an hour out)
    // as "ahead of you" inflates every reported wait.
    let sql = format!(
        "SELECT \
            COALESCE(r.missing, FALSE) AS missing, \
            COALESCE(r.history_complete, FALSE) AS stars_complete, \
            r.archive_cursor, \
            stars.status AS star_status, stars.partial AS star_partial, \
            stars.next_page AS star_next_page, stars.last_error AS star_last_error, \
            COALESCE(stars.attempts, 0) AS star_attempts, \
            COALESCE(stars.priority, 0) AS star_priority, \
            stars.next_attempt_at AS star_next_attempt_at, \
            CASE WHEN stars.status = 'pending' THEN ( \
                SELECT COUNT(*)::BIGINT + 1 FROM star_fetch_queue ahead \
                WHERE ahead.status = 'pending' \
                  AND ahead.next_attempt_at <= NOW() \
                  AND (ahead.priority > stars.priority OR \
                       (ahead.priority = stars.priority AND ahead.enqueued_at < stars.enqueued_at)) \
            ) END AS star_position, \
            analysis.status AS analysis_status, \
            COALESCE(analysis.attempts, 0) AS analysis_attempts, \
            analysis.phase AS analysis_phase, \
            COALESCE(analysis.priority, 0) AS analysis_priority, \
            analysis.next_attempt_at AS analysis_next_attempt_at, \
            analysis.started_at AS analysis_started_at, \
            analysis.total_units AS analysis_total_units, \
            COALESCE(analysis.completed_units, 0) AS analysis_completed_units, \
            CASE WHEN analysis.status = 'pending' THEN ({ANALYSIS_POSITION_SQL}) END \
                AS analysis_position, \
            (history.last_analyzed_at IS NOT NULL) AS analysis_complete, \
            history.analysis_duration_ms AS repo_last_duration_ms, \
            history.analysis_scope_commits, \
            COALESCE(history.analysis_truncated, FALSE) AS analysis_truncated \
         FROM (SELECT $1::TEXT AS repo) requested \
         LEFT JOIN repos r ON r.repo = requested.repo \
         LEFT JOIN star_fetch_queue stars ON stars.repo = requested.repo \
         LEFT JOIN repo_analysis_queue analysis ON analysis.repo = requested.repo \
         LEFT JOIN repo_history history ON history.repo = requested.repo"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(repo)
        .fetch_one(&state.analyzer.cache.db().pool)
        .await?;
    let analysis_median_ms = fleet_analysis_median_ms(state).await;
    Ok(ProgressSnapshot::from_raw(
        repo.to_string(),
        RawProgress {
            missing: row.try_get("missing")?,
            stars_complete: row.try_get("stars_complete")?,
            star_status: row.try_get("star_status")?,
            star_partial: row
                .try_get::<Option<bool>, _>("star_partial")?
                .unwrap_or(false),
            star_next_page: row.try_get("star_next_page")?,
            star_last_error: row.try_get("star_last_error")?,
            star_attempts: row.try_get("star_attempts")?,
            star_priority: row.try_get("star_priority")?,
            star_position: row.try_get("star_position")?,
            star_next_attempt_at: row.try_get("star_next_attempt_at")?,
            archive_cursor: row.try_get("archive_cursor")?,
            analysis_status: row.try_get("analysis_status")?,
            analysis_attempts: row.try_get("analysis_attempts")?,
            analysis_phase: row.try_get("analysis_phase")?,
            analysis_priority: row.try_get("analysis_priority")?,
            analysis_position: row.try_get("analysis_position")?,
            analysis_next_attempt_at: row.try_get("analysis_next_attempt_at")?,
            analysis_started_at: row.try_get("analysis_started_at")?,
            analysis_total_units: row.try_get("analysis_total_units")?,
            analysis_completed_units: row.try_get("analysis_completed_units")?,
            analysis_median_ms,
            repo_last_duration_ms: row.try_get("repo_last_duration_ms")?,
            analysis_complete: row.try_get("analysis_complete")?,
            analysis_scope_commits: row.try_get("analysis_scope_commits")?,
            analysis_truncated: row.try_get("analysis_truncated")?,
        },
    ))
}

/// Fallback ETA when no analysis has ever been timed.
const DEFAULT_ANALYSIS_DURATION_MS: i64 = 300_000;
/// How long the fleet-wide median is reused before it is re-measured.
const ANALYSIS_MEDIAN_TTL: Duration = Duration::from_secs(60);

/// Median duration of the most recent analysis runs, cached per process.
///
/// The median rather than the mean: a single monorepo taking twenty minutes
/// used to drag every queued repository's reported wait up with it, and the
/// sample is only twenty rows wide, so one outlier moves the mean by 5%.
///
/// This is a fleet-wide constant, not a per-repository value, but it used to
/// ride along in the per-poll progress query — where it read `repo_history`
/// (one row per repository ever analyzed) with a sort that no index serves,
/// once per stream per two seconds. Measuring it on its own schedule keeps
/// the poll to primary-key lookups.
async fn fleet_analysis_median_ms(state: &ApiState) -> i64 {
    static CACHED: OnceLock<Mutex<Option<(Instant, i64)>>> = OnceLock::new();
    let cell = CACHED.get_or_init(|| Mutex::new(None));
    let now = Instant::now();
    if let Some((measured_at, value)) =
        *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
        && now.duration_since(measured_at) < ANALYSIS_MEDIAN_TTL
    {
        return value;
    }
    // `percentile_cont` is double precision; the subselect keeps the ordered
    // index scan that `idx_repo_history_duration_recent` serves.
    let measured: Option<f64> = sqlx::query_scalar(
        "SELECT percentile_cont(0.5) WITHIN GROUP ( \
             ORDER BY sample.analysis_duration_ms::DOUBLE PRECISION \
         ) FROM ( \
             SELECT analysis_duration_ms FROM repo_history \
             WHERE analysis_duration_ms IS NOT NULL \
             ORDER BY last_analyzed_at DESC NULLS LAST LIMIT 20 \
         ) sample",
    )
    .fetch_one(&state.analyzer.cache.db().pool)
    .await
    .unwrap_or(None);
    let value = measured
        .filter(|ms| ms.is_finite())
        .map(|ms| ms.round().clamp(1.0, i64::MAX as f64) as i64)
        .unwrap_or(DEFAULT_ANALYSIS_DURATION_MS)
        .max(1);
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((now, value));
    value
}

async fn load_snapshot_bounded(state: &ApiState, repo: &str) -> Result<ProgressSnapshot, ApiError> {
    tokio::time::timeout(SNAPSHOT_TIMEOUT, load_snapshot(state, repo))
        .await
        .map_err(|_| ApiError::unavailable("progress snapshot timed out"))?
}

async fn load_profile_snapshot(
    state: &ApiState,
    login: &str,
) -> Result<ProfileProgressSnapshot, ApiError> {
    // The list meta row gates the read exactly as Cache::get_login_repos does:
    // no profile progress is inferred from a half-replaced login_repos set.
    // All other relations are primary-key joins over the bounded top-repo
    // slice, so this stays cheap enough for the two-second SSE cadence.
    let row = sqlx::query(
        "SELECT lists.public_repos, \
                COUNT(owned.repo) FILTER ( \
                    WHERE NOT COALESCE(repo.missing, FALSE))::BIGINT AS candidates, \
                COUNT(owned.repo) FILTER ( \
                    WHERE NOT COALESCE(repo.missing, FALSE) \
                      AND COALESCE(repo.history_complete, FALSE) \
                      AND repo.metadata_fetched_at IS NOT NULL)::BIGINT AS included, \
                COUNT(owned.repo) FILTER ( \
                    WHERE NOT COALESCE(repo.missing, FALSE) \
                      AND history.last_analyzed_at >= $2 \
                      AND history.analysis_revision >= $3 \
                      AND history.last_analyzed_sha IS NOT NULL \
                      AND history.head_sha = history.last_analyzed_sha \
                      AND (analysis.status IS NULL \
                           OR analysis.status NOT IN ('pending', 'in_progress')))::BIGINT \
                    AS analyzed \
         FROM login_repo_lists lists \
         LEFT JOIN login_repos owned ON owned.login = lists.login \
         LEFT JOIN repos repo ON repo.repo = owned.repo \
         LEFT JOIN repo_history history ON history.repo = owned.repo \
         LEFT JOIN repo_analysis_queue analysis ON analysis.repo = owned.repo \
         WHERE lists.login = $1 AND lists.complete = TRUE AND lists.missing = FALSE \
         GROUP BY lists.public_repos",
    )
    .bind(login)
    .bind(Utc::now() - crate::repo_analysis::analysis_freshness())
    .bind(crate::repo_analysis::CURRENT_ANALYSIS_REVISION)
    .fetch_optional(&state.analyzer.cache.db().pool)
    .await?
    .ok_or_else(|| ApiError::unavailable("profile progress unavailable"))?;

    let candidates = row.try_get::<i64, _>("candidates")?.max(0) as u64;
    let included = row.try_get::<i64, _>("included")?.max(0) as u64;
    let analyzed = row.try_get::<i64, _>("analyzed")?.max(0) as u64;
    let repos_pending = candidates.saturating_sub(included);
    let repos_analyzing = candidates.saturating_sub(analyzed);
    let bounded = |value: u64| value.min(u64::from(u32::MAX)) as u32;

    Ok(ProfileProgressSnapshot {
        login: login.to_string(),
        terminal: repos_pending == 0 && repos_analyzing == 0,
        repos_included: bounded(included),
        repos_pending: bounded(repos_pending),
        repos_analyzed: bounded(analyzed),
        repos_analyzing: bounded(repos_analyzing),
        repos_total: row
            .try_get::<Option<i64>, _>("public_repos")?
            .and_then(|value| u64::try_from(value).ok()),
    })
}

async fn load_profile_snapshot_bounded(
    state: &ApiState,
    login: &str,
) -> Result<ProfileProgressSnapshot, ApiError> {
    tokio::time::timeout(SNAPSHOT_TIMEOUT, load_profile_snapshot(state, login))
        .await
        .map_err(|_| ApiError::unavailable("profile progress snapshot timed out"))?
}

struct StreamState {
    state: ApiState,
    repo: String,
    last: ProgressSnapshot,
    initial: Option<ProgressSnapshot>,
    interval: Interval,
    deadline: Instant,
    event_id: u64,
    done: bool,
    _permit: SemaphorePermit<'static>,
    _client_permit: ClientPermit,
}

struct ProfileStreamState {
    state: ApiState,
    login: String,
    last: ProfileProgressSnapshot,
    initial: Option<ProfileProgressSnapshot>,
    interval: Interval,
    deadline: Instant,
    event_id: u64,
    done: bool,
    _permit: SemaphorePermit<'static>,
    _client_permit: ClientPermit,
}

fn progress_event(snapshot: &ProgressSnapshot, id: u64) -> Event {
    Event::default()
        .event("progress")
        .id(id.to_string())
        .retry(CLIENT_RETRY)
        .json_data(snapshot)
        .expect("progress snapshots are always JSON serializable")
}

fn control_event(name: &'static str, repo: &str) -> Event {
    Event::default()
        .event(name)
        .retry(CLIENT_RETRY)
        .json_data(serde_json::json!({
            "repo": repo,
            "retry_after_ms": CLIENT_RETRY.as_millis(),
        }))
        .expect("control event is always JSON serializable")
}

fn profile_progress_event(snapshot: &ProfileProgressSnapshot, id: u64) -> Event {
    Event::default()
        .event("progress")
        .id(id.to_string())
        .retry(CLIENT_RETRY)
        .json_data(snapshot)
        .expect("profile progress snapshots are always JSON serializable")
}

fn profile_control_event(name: &'static str, login: &str) -> Event {
    Event::default()
        .event(name)
        .retry(CLIENT_RETRY)
        .json_data(serde_json::json!({
            "login": login,
            "retry_after_ms": CLIENT_RETRY.as_millis(),
        }))
        .expect("profile control event is always JSON serializable")
}

pub async fn repo_progress(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    connect_info: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let repo = crate::analyzer::repo_key(&owner, &repo);
    let permit = PROGRESS_PERMITS
        .try_acquire()
        .map_err(|_| ApiError::unavailable("progress stream capacity reached"))?;
    let client_ip = crate::api::request_client_ip(&headers, Some(connect_info))
        // `ConnectInfo` is installed by main.rs; this only protects an
        // alternate embedding that accidentally strips the extension.
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let client_permit = ClientPermit::try_acquire(client_ip)
        .ok_or_else(|| ApiError::unavailable("client progress stream capacity reached"))?;

    // Do the first DB read before committing the response headers. A failed
    // initial read can still return the normal generic 5xx JSON contract.
    let initial = load_snapshot_bounded(&state, &repo).await?;
    let mut interval = tokio::time::interval_at(Instant::now() + POLL_INTERVAL, POLL_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let stream_state = StreamState {
        state,
        repo,
        last: initial.clone(),
        initial: Some(initial),
        interval,
        deadline: Instant::now() + MAX_STREAM_LIFETIME,
        event_id: 0,
        done: false,
        _permit: permit,
        _client_permit: client_permit,
    };

    let events = stream::unfold(stream_state, |mut stream| async move {
        if stream.done {
            return None;
        }
        if let Some(initial) = stream.initial.take() {
            stream.event_id += 1;
            stream.done = initial.terminal;
            let event = progress_event(&initial, stream.event_id);
            return Some((Ok::<Event, Infallible>(event), stream));
        }

        loop {
            if Instant::now() >= stream.deadline {
                stream.done = true;
                let event = control_event("timeout", &stream.repo);
                return Some((Ok(event), stream));
            }
            stream.interval.tick().await;
            match load_snapshot_bounded(&stream.state, &stream.repo).await {
                Ok(snapshot) if snapshot != stream.last => {
                    stream.event_id += 1;
                    stream.done = snapshot.terminal;
                    stream.last = snapshot;
                    let event = progress_event(&stream.last, stream.event_id);
                    return Some((Ok(event), stream));
                }
                Ok(_) => {
                    // KeepAlive emits the wire heartbeat while state is
                    // unchanged; avoid repeating identical JSON events.
                }
                Err(error) => {
                    tracing::warn!(
                        repo = %stream.repo,
                        error = ?error,
                        "progress stream poll failed"
                    );
                    stream.done = true;
                    let event = control_event("unavailable", &stream.repo);
                    return Some((Ok(event), stream));
                }
            }
        }
    });

    let mut response = Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(HEARTBEAT_INTERVAL)
                .text("keep-alive"),
        )
        .into_response();
    set_stream_headers(response.headers_mut());
    Ok(response)
}

/// Read-only profile progress stream. The initial `/analyze` or `/warm`
/// request creates durable work; this endpoint only observes the cached
/// owned-repository set and the two queues, so EventSource reconnects cannot
/// amplify GitHub or git work.
pub async fn profile_progress(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    connect_info: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !crate::aggregate::is_valid_login(&login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let login = login.to_ascii_lowercase();
    let permit = PROGRESS_PERMITS
        .try_acquire()
        .map_err(|_| ApiError::unavailable("progress stream capacity reached"))?;
    let client_ip = crate::api::request_client_ip(&headers, Some(connect_info))
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let client_permit = ClientPermit::try_acquire(client_ip)
        .ok_or_else(|| ApiError::unavailable("client progress stream capacity reached"))?;
    let initial = load_profile_snapshot_bounded(&state, &login).await?;
    let mut interval = tokio::time::interval_at(Instant::now() + POLL_INTERVAL, POLL_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let stream_state = ProfileStreamState {
        state,
        login,
        last: initial.clone(),
        initial: Some(initial),
        interval,
        deadline: Instant::now() + MAX_STREAM_LIFETIME,
        event_id: 0,
        done: false,
        _permit: permit,
        _client_permit: client_permit,
    };

    let events = stream::unfold(stream_state, |mut stream| async move {
        if stream.done {
            return None;
        }
        if let Some(initial) = stream.initial.take() {
            stream.event_id += 1;
            stream.done = initial.terminal;
            let event = profile_progress_event(&initial, stream.event_id);
            return Some((Ok::<Event, Infallible>(event), stream));
        }

        loop {
            if Instant::now() >= stream.deadline {
                stream.done = true;
                let event = profile_control_event("timeout", &stream.login);
                return Some((Ok(event), stream));
            }
            stream.interval.tick().await;
            match load_profile_snapshot_bounded(&stream.state, &stream.login).await {
                Ok(snapshot) if snapshot != stream.last => {
                    stream.event_id += 1;
                    stream.done = snapshot.terminal;
                    stream.last = snapshot;
                    let event = profile_progress_event(&stream.last, stream.event_id);
                    return Some((Ok(event), stream));
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        login = %stream.login,
                        error = ?error,
                        "profile progress stream poll failed"
                    );
                    stream.done = true;
                    let event = profile_control_event("unavailable", &stream.login);
                    return Some((Ok(event), stream));
                }
            }
        }
    });

    let mut response = Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(HEARTBEAT_INTERVAL)
                .text("keep-alive"),
        )
        .into_response();
    set_stream_headers(response.headers_mut());
    Ok(response)
}

/// One bounded progress snapshot for clients that cannot keep an SSE
/// connection open. This is read-only and shares the exact durable queue
/// projection used by the stream.
pub async fn repo_progress_snapshot(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let repo = crate::analyzer::repo_key(&owner, &repo);
    let snapshot = load_snapshot_bounded(&state, &repo).await?;
    let mut response = Json(snapshot).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn set_stream_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-transform"),
    );
    headers.insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(raw: RawProgress) -> ProgressSnapshot {
        ProgressSnapshot::from_raw("owner/repo".to_string(), raw)
    }

    #[test]
    fn partial_fetch_is_backfilling_and_non_terminal() {
        let value = snapshot(RawProgress {
            star_status: Some("pending".into()),
            star_partial: true,
            star_next_page: Some(401),
            analysis_status: Some("in_progress".into()),
            ..RawProgress::default()
        });
        assert_eq!(value.phase, ProgressPhase::Backfilling);
        assert_eq!(value.stars.phase, ProgressPhase::Backfilling);
        assert_eq!(value.stars.next_page, Some(401));
        assert_eq!(value.analysis.phase, ProgressPhase::Analyzing);
        assert!(!value.terminal);
    }

    #[test]
    fn missing_repo_is_terminal_for_both_pipelines() {
        let value = snapshot(RawProgress {
            missing: true,
            star_status: Some("pending".into()),
            ..RawProgress::default()
        });
        assert_eq!(value.phase, ProgressPhase::NotFound);
        assert_eq!(value.stars.phase, ProgressPhase::NotFound);
        assert_eq!(value.analysis.phase, ProgressPhase::NotFound);
        assert!(value.terminal);
    }

    #[test]
    fn provider_retry_is_public_phase_but_error_detail_is_private() {
        let detail = "provider: upstream account-specific detail";
        let value = snapshot(RawProgress {
            star_status: Some("pending".into()),
            star_last_error: Some(detail.into()),
            ..RawProgress::default()
        });
        assert_eq!(value.phase, ProgressPhase::Retrying);
        assert!(!value.terminal);
        let json = serde_json::to_string(&value).unwrap();
        assert!(!json.contains(detail));
        assert!(!json.contains("upstream"));
    }

    #[test]
    fn historic_dead_job_is_retrying_and_non_terminal() {
        let value = snapshot(RawProgress {
            analysis_status: Some("dead".into()),
            ..RawProgress::default()
        });
        assert_eq!(value.phase, ProgressPhase::Retrying);
        assert_eq!(value.analysis.phase, ProgressPhase::Retrying);
        assert!(!value.terminal);
    }

    #[test]
    fn either_independent_pipeline_can_complete_the_stream() {
        let stars = snapshot(RawProgress {
            stars_complete: true,
            archive_cursor: Some(Utc::now().date_naive()),
            ..RawProgress::default()
        });
        assert_eq!(stars.phase, ProgressPhase::Complete);
        assert!(stars.terminal);
        assert_eq!(stars.stars.processed_units, stars.stars.total_units);
        assert_eq!(stars.stars.percent, Some(100));
        assert_eq!(stars.stars.eta_seconds, None);

        let analysis = snapshot(RawProgress {
            analysis_complete: true,
            ..RawProgress::default()
        });
        assert_eq!(analysis.phase, ProgressPhase::Complete);
        assert!(analysis.terminal);
    }

    #[test]
    fn indexed_archive_eta_uses_observed_partition_rate() {
        assert_eq!(archive_eta_seconds(186, 58, true), 192);
        assert_eq!(archive_eta_seconds(186, 58, false), 5_760);
        assert_eq!(archive_eta_seconds(186, 186, true), 0);
    }

    #[test]
    fn completed_bounded_analysis_reports_its_exact_scope() {
        let value = snapshot(RawProgress {
            analysis_complete: true,
            analysis_scope_commits: Some(500),
            analysis_truncated: true,
            ..RawProgress::default()
        });
        assert_eq!(value.analysis.detail, Some("recent_window"));
        assert_eq!(value.analysis.processed_units, Some(500));
        assert_eq!(value.analysis.total_units, Some(500));
        assert_eq!(value.analysis.percent, Some(100));
    }

    #[test]
    fn active_refresh_preserves_available_data_flag() {
        let value = snapshot(RawProgress {
            stars_complete: true,
            star_status: Some("in_progress".into()),
            star_next_page: Some(9),
            ..RawProgress::default()
        });
        assert_eq!(value.phase, ProgressPhase::Fetching);
        assert!(value.stars.complete);
        assert_eq!(value.stars.next_page, Some(9));
        assert!(!value.terminal);
    }

    #[test]
    fn queued_repo_without_its_own_timing_reports_no_eta() {
        let value = snapshot(RawProgress {
            analysis_status: Some("pending".into()),
            analysis_position: Some(1),
            analysis_median_ms: 300_000,
            ..RawProgress::default()
        });
        assert_eq!(value.analysis.phase, ProgressPhase::Pending);
        assert_eq!(value.analysis.queue_position, Some(1));
        assert_eq!(value.analysis.eta_seconds, None);
    }

    #[test]
    fn next_queued_repo_is_estimated_from_its_own_last_run() {
        let value = snapshot(RawProgress {
            analysis_status: Some("pending".into()),
            analysis_position: Some(1),
            analysis_median_ms: 300_000,
            repo_last_duration_ms: Some(4_000),
            ..RawProgress::default()
        });
        assert_eq!(value.analysis.eta_seconds, Some(4));
    }

    #[test]
    fn queue_wait_uses_the_fleet_median_and_the_repo_owns_its_wave() {
        // Twelve jobs ahead fill two waves of six workers; the thirteenth then
        // runs for as long as this repository itself last took.
        assert_eq!(
            pending_analysis_eta_seconds(13, 6, 300_000, Some(4_000), 0),
            Some(2 * 300 + 4)
        );
        assert_eq!(
            pending_analysis_eta_seconds(13, 6, 300_000, None, 0),
            Some(900)
        );
        assert_eq!(pending_analysis_eta_seconds(1, 6, 300_000, None, 0), None);
        // The repository's own measured run is reported as measured: a repo
        // that has only ever taken twenty minutes is not promised five.
        assert_eq!(
            pending_analysis_eta_seconds(1, 6, 300_000, Some(1_200_000), 0),
            Some(1_200)
        );
    }

    #[test]
    fn a_job_parked_in_backoff_is_promised_its_own_backoff_first() {
        // Nothing claimable is ahead of it, so its position is 1 — but it is
        // not claimable either for another half hour.
        assert_eq!(
            pending_analysis_eta_seconds(1, 6, 300_000, Some(4_000), 1_800),
            Some(1_804)
        );
        // With no timing of its own, the backoff is still a real measurement
        // of this job and is worth reporting alone.
        assert_eq!(
            pending_analysis_eta_seconds(1, 6, 300_000, None, 1_800),
            Some(1_800)
        );
    }

    #[test]
    fn backoff_wait_is_carried_into_the_reported_analysis_eta() {
        let retry_at = Utc::now() + chrono::Duration::minutes(30);
        let value = snapshot(RawProgress {
            analysis_status: Some("pending".into()),
            analysis_attempts: 6,
            analysis_position: Some(1),
            analysis_next_attempt_at: Some(retry_at),
            analysis_median_ms: 300_000,
            repo_last_duration_ms: Some(4_000),
            ..RawProgress::default()
        });
        assert_eq!(value.analysis.phase, ProgressPhase::Retrying);
        assert_eq!(value.analysis.retry_at, Some(retry_at));
        let eta = value.analysis.eta_seconds.expect("backoff is measurable");
        assert!(
            (1_800..=1_804).contains(&eta),
            "expected the half-hour park plus this repo's own run, got {eta}"
        );
    }

    #[test]
    fn an_elapsed_backoff_stamp_does_not_inflate_the_wait() {
        let value = snapshot(RawProgress {
            analysis_status: Some("pending".into()),
            analysis_position: Some(1),
            analysis_next_attempt_at: Some(Utc::now() - chrono::Duration::minutes(5)),
            analysis_median_ms: 300_000,
            repo_last_duration_ms: Some(4_000),
            ..RawProgress::default()
        });
        assert_eq!(value.analysis.retry_at, None);
        assert_eq!(value.analysis.eta_seconds, Some(4));
    }

    #[test]
    fn unmeasurable_analysis_tail_reports_no_eta() {
        let value = snapshot(RawProgress {
            analysis_status: Some("in_progress".into()),
            analysis_phase: Some("saving_history".into()),
            analysis_started_at: Some(Utc::now() - chrono::Duration::seconds(30)),
            ..RawProgress::default()
        });
        assert_eq!(value.analysis.detail, Some("saving_history"));
        assert_eq!(value.analysis.eta_seconds, None);
    }

    #[test]
    fn in_progress_eta_extrapolates_the_measured_walk() {
        let value = snapshot(RawProgress {
            analysis_status: Some("in_progress".into()),
            analysis_phase: Some("scanning_history".into()),
            analysis_started_at: Some(Utc::now() - chrono::Duration::seconds(40)),
            analysis_total_units: Some(400),
            analysis_completed_units: 100,
            ..RawProgress::default()
        });
        // 40s bought 100 of 400 commits: 120s of walk left, plus a quarter of
        // that for the phases after the walk.
        assert_eq!(value.analysis.eta_seconds, Some(150));
    }

    #[test]
    fn stream_headers_disable_proxy_caching_and_buffering() {
        let mut headers = HeaderMap::new();
        set_stream_headers(&mut headers);
        assert_eq!(
            headers.get(header::CACHE_CONTROL).unwrap(),
            "no-store, no-transform"
        );
        assert_eq!(headers.get("x-accel-buffering").unwrap(), "no");
    }

    #[test]
    fn profile_progress_snapshot_reports_the_full_unfinished_slice() {
        let snapshot = ProfileProgressSnapshot {
            login: "google".into(),
            terminal: false,
            repos_included: 8,
            repos_pending: 42,
            repos_analyzed: 6,
            repos_analyzing: 44,
            repos_total: Some(2_913),
        };
        let json = serde_json::to_value(snapshot).unwrap();
        assert_eq!(json["login"], "google");
        assert_eq!(json["terminal"], false);
        assert_eq!(json["repos_pending"], 42);
        assert_eq!(json["repos_analyzing"], 44);
        assert_eq!(json["repos_total"], 2_913);
    }

    #[test]
    fn client_connection_limit_releases_on_drop() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 201));
        let permits: Vec<_> = (0..MAX_PROGRESS_CONNECTIONS_PER_CLIENT)
            .map(|_| ClientPermit::try_acquire(ip).expect("within per-client cap"))
            .collect();
        assert!(ClientPermit::try_acquire(ip).is_none());
        drop(permits);
        assert!(ClientPermit::try_acquire(ip).is_some());
    }
}
