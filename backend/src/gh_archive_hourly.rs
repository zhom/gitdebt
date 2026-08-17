//! Forward-only follower for GH Archive's completed hourly files.
//!
//! GH Archive publishes one gzip-compressed NDJSON object per UTC hour at
//! `https://data.gharchive.org/YYYY-MM-DD-H.json.gz`. This module owns the
//! scheduling, HTTP classification, filtering, retry, and checkpoint contract;
//! database access and gzip decoding are injected. The production decoder is
//! [`GzipArchiveDecoder`]; tests can use an identity decoder.
//!
//! # Durability contract
//!
//! [`HourlyArchiveSink::commit_hour`] must insert every event in the batch and
//! mark the hour committed in one durable transaction. Replaying a committed
//! hour must return [`HourCommit::AlreadyCommitted`] without duplicating rows.
//! A 404 is never checkpointed: it is retried within the bounded attempt budget
//! and then deferred for the next scheduled follower run.

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Timelike, Utc};
use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use serde::Deserialize;
use thiserror::Error;

use crate::gh_archive::GhArchiveStarEvent;

const ARCHIVE_BASE_URL: &str = "https://data.gharchive.org";
const DEFAULT_LAG_SECS: u64 = 15 * 60;
const DEFAULT_MAX_HOURS_PER_RUN: usize = 24;
const DEFAULT_MAX_FETCH_ATTEMPTS: usize = 4;
const DEFAULT_RETRY_BASE_MILLIS: u64 = 500;
const DEFAULT_RETRY_MAX_SECS: u64 = 15;
const DEFAULT_MAX_MATCHING_EVENTS: usize = 250_000;
const DEFAULT_MAX_DECODED_BYTES: usize = 512 * 1024 * 1024;
const DEFAULT_MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_MAX_COMPRESSED_BYTES: usize = 256 * 1024 * 1024;

const HARD_MAX_HOURS_PER_RUN: usize = 24 * 31;
const HARD_MAX_FETCH_ATTEMPTS: usize = 10;
const HARD_MAX_MATCHING_EVENTS: usize = 1_000_000;
const HARD_MAX_DECODED_BYTES: usize = 2 * 1024 * 1024 * 1024;
const HARD_MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const HARD_MAX_COMPRESSED_BYTES: usize = 1024 * 1024 * 1024;

/// Runtime limits for one follower pass.
#[derive(Clone, Debug)]
pub struct HourlyFollowerConfig {
    /// Only hours whose end is older than this lag are eligible.
    pub lag: Duration,
    /// Bounds catch-up work so one scheduled run cannot monopolize a worker.
    pub max_hours_per_run: usize,
    /// Total fetch attempts per hour, including the first request.
    pub max_fetch_attempts: usize,
    pub retry_base: Duration,
    pub retry_max: Duration,
    pub max_matching_events: usize,
    pub max_decoded_bytes: usize,
    pub max_line_bytes: usize,
}

impl Default for HourlyFollowerConfig {
    fn default() -> Self {
        Self {
            lag: Duration::from_secs(DEFAULT_LAG_SECS),
            max_hours_per_run: DEFAULT_MAX_HOURS_PER_RUN,
            max_fetch_attempts: DEFAULT_MAX_FETCH_ATTEMPTS,
            retry_base: Duration::from_millis(DEFAULT_RETRY_BASE_MILLIS),
            retry_max: Duration::from_secs(DEFAULT_RETRY_MAX_SECS),
            max_matching_events: DEFAULT_MAX_MATCHING_EVENTS,
            max_decoded_bytes: DEFAULT_MAX_DECODED_BYTES,
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
        }
    }
}

impl HourlyFollowerConfig {
    pub fn validate(&self) -> Result<(), HourlyArchiveError> {
        validate_bounded(
            "max_hours_per_run",
            self.max_hours_per_run,
            1,
            HARD_MAX_HOURS_PER_RUN,
        )?;
        validate_bounded(
            "max_fetch_attempts",
            self.max_fetch_attempts,
            1,
            HARD_MAX_FETCH_ATTEMPTS,
        )?;
        validate_bounded(
            "max_matching_events",
            self.max_matching_events,
            1,
            HARD_MAX_MATCHING_EVENTS,
        )?;
        validate_bounded(
            "max_decoded_bytes",
            self.max_decoded_bytes,
            1,
            HARD_MAX_DECODED_BYTES,
        )?;
        validate_bounded(
            "max_line_bytes",
            self.max_line_bytes,
            1,
            HARD_MAX_LINE_BYTES,
        )?;
        if self.retry_base > self.retry_max {
            return Err(HourlyArchiveError::InvalidConfig(
                "retry_base must not exceed retry_max".to_string(),
            ));
        }
        TimeDelta::from_std(self.lag).map_err(|_| {
            HourlyArchiveError::InvalidConfig("lag does not fit in chrono::TimeDelta".to_string())
        })?;
        Ok(())
    }
}

/// A fetched hourly object. `NotReady` is the expected meaning of HTTP 404.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveFetch {
    Ready(Vec<u8>),
    NotReady { retry_after: Option<Duration> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FetchErrorKind {
    Retryable,
    Permanent,
}

/// Fetch failures are classified so the follower can keep retries bounded.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{kind:?} archive fetch failure: {message}")]
pub struct ArchiveFetchError {
    pub kind: FetchErrorKind,
    pub message: String,
    pub retry_after: Option<Duration>,
}

impl ArchiveFetchError {
    pub fn retryable(message: impl Into<String>, retry_after: Option<Duration>) -> Self {
        Self {
            kind: FetchErrorKind::Retryable,
            message: message.into(),
            retry_after,
        }
    }

    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: FetchErrorKind::Permanent,
            message: message.into(),
            retry_after: None,
        }
    }
}

/// Supplies compressed hourly objects. Implementations perform one request;
/// retry policy belongs to [`GhArchiveHourlyFollower`].
#[async_trait]
pub trait HourlyArchiveFetcher: Send + Sync {
    async fn fetch_hour(
        &self,
        archive_hour: DateTime<Utc>,
    ) -> Result<ArchiveFetch, ArchiveFetchError>;
}

/// Converts a gzip object into bounded NDJSON bytes.
///
/// Implementations must reject output larger than `max_decoded_bytes`. A
/// streaming implementation should stop decompression as soon as the bound is
/// crossed.
pub trait ArchiveDecoder: Send + Sync {
    fn decode(
        &self,
        compressed: &[u8],
        max_decoded_bytes: usize,
    ) -> Result<Vec<u8>, ArchiveDecodeError>;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ArchiveDecodeError {
    #[error("invalid gzip archive: {0}")]
    Invalid(String),
    #[error("decoded archive exceeds {maximum} bytes")]
    TooLarge { maximum: usize },
}

/// Bounded in-process gzip decoder. The compressed response is already capped
/// by the fetcher; `Read::take` adds one sentinel byte so an oversized decoded
/// stream is rejected without allocating past the configured limit.
#[derive(Clone, Copy, Debug, Default)]
pub struct GzipArchiveDecoder;

impl ArchiveDecoder for GzipArchiveDecoder {
    fn decode(
        &self,
        compressed: &[u8],
        max_decoded_bytes: usize,
    ) -> Result<Vec<u8>, ArchiveDecodeError> {
        let decoder = flate2::read::GzDecoder::new(compressed);
        let limit = u64::try_from(max_decoded_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut bounded = decoder.take(limit);
        let mut decoded =
            Vec::with_capacity(compressed.len().saturating_mul(3).min(max_decoded_bytes));
        bounded
            .read_to_end(&mut decoded)
            .map_err(|error| ArchiveDecodeError::Invalid(error.to_string()))?;
        if decoded.len() > max_decoded_bytes {
            return Err(ArchiveDecodeError::TooLarge {
                maximum: max_decoded_bytes,
            });
        }
        Ok(decoded)
    }
}

/// Supplies the numeric GitHub IDs currently tracked by gitdebt.
#[async_trait]
pub trait TrackedRepositorySource: Send + Sync {
    async fn tracked_repository_ids(&self) -> Result<BTreeSet<i64>, HourlyArchiveError>;
}

/// One atomically committed hour. Events contain repository identity and star
/// timestamps only; actor/profile data is never represented by this module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HourBatch {
    pub archive_hour: DateTime<Utc>,
    pub records_seen: usize,
    pub events: Vec<GhArchiveStarEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HourCommit {
    Committed,
    AlreadyCommitted,
}

/// Durable checkpoint and event sink.
#[async_trait]
pub trait HourlyArchiveSink: Send + Sync {
    async fn is_hour_committed(
        &self,
        archive_hour: DateTime<Utc>,
    ) -> Result<bool, HourlyArchiveError>;

    /// Implementations must commit the batch and hour checkpoint atomically.
    async fn commit_hour(&self, batch: HourBatch) -> Result<HourCommit, HourlyArchiveError>;
}

#[async_trait]
pub trait RetrySleeper: Send + Sync {
    async fn sleep(&self, duration: Duration);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TokioRetrySleeper;

#[async_trait]
impl RetrySleeper for TokioRetrySleeper {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// Bounded HTTP fetcher for the canonical GH Archive origin.
#[derive(Clone)]
pub struct ReqwestHourlyArchiveFetcher {
    client: reqwest::Client,
    max_compressed_bytes: usize,
}

impl fmt::Debug for ReqwestHourlyArchiveFetcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReqwestHourlyArchiveFetcher")
            .field("max_compressed_bytes", &self.max_compressed_bytes)
            .finish_non_exhaustive()
    }
}

impl ReqwestHourlyArchiveFetcher {
    pub fn new(
        client: reqwest::Client,
        max_compressed_bytes: usize,
    ) -> Result<Self, HourlyArchiveError> {
        validate_bounded(
            "max_compressed_bytes",
            max_compressed_bytes,
            1,
            HARD_MAX_COMPRESSED_BYTES,
        )?;
        Ok(Self {
            client,
            max_compressed_bytes,
        })
    }

    pub fn with_default_limit(client: reqwest::Client) -> Self {
        Self {
            client,
            max_compressed_bytes: DEFAULT_MAX_COMPRESSED_BYTES,
        }
    }
}

#[async_trait]
impl HourlyArchiveFetcher for ReqwestHourlyArchiveFetcher {
    async fn fetch_hour(
        &self,
        archive_hour: DateTime<Utc>,
    ) -> Result<ArchiveFetch, ArchiveFetchError> {
        let url = archive_url(archive_hour);
        let response = self.client.get(url).send().await.map_err(|error| {
            ArchiveFetchError::retryable(
                if error.is_timeout() {
                    "request timed out"
                } else if error.is_connect() {
                    "connection failed"
                } else {
                    "request failed"
                },
                None,
            )
        })?;
        let status = response.status();
        let retry_after = retry_after(response.headers());

        if status == StatusCode::NOT_FOUND {
            return Ok(ArchiveFetch::NotReady { retry_after });
        }
        if !status.is_success() {
            let message = format!("GH Archive returned HTTP {}", status.as_u16());
            return if is_retryable_status(status) {
                Err(ArchiveFetchError::retryable(message, retry_after))
            } else {
                Err(ArchiveFetchError::permanent(message))
            };
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.max_compressed_bytes as u64)
        {
            return Err(ArchiveFetchError::permanent(format!(
                "compressed archive exceeds {} bytes",
                self.max_compressed_bytes
            )));
        }

        let mut response = response;
        let mut compressed = Vec::with_capacity(
            response
                .content_length()
                .unwrap_or_default()
                .min(self.max_compressed_bytes as u64) as usize,
        );
        loop {
            let chunk = response
                .chunk()
                .await
                .map_err(|_| ArchiveFetchError::retryable("archive body transfer failed", None))?;
            let Some(chunk) = chunk else {
                break;
            };
            let next_len = compressed.len().checked_add(chunk.len()).ok_or_else(|| {
                ArchiveFetchError::permanent("compressed archive length overflow")
            })?;
            if next_len > self.max_compressed_bytes {
                return Err(ArchiveFetchError::permanent(format!(
                    "compressed archive exceeds {} bytes",
                    self.max_compressed_bytes
                )));
            }
            compressed.extend_from_slice(&chunk);
        }
        Ok(ArchiveFetch::Ready(compressed))
    }
}

/// Outcome of a bounded catch-up pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FollowReport {
    pub eligible_through: Option<DateTime<Utc>>,
    pub hours_committed: usize,
    pub hours_already_committed: usize,
    pub matching_events: usize,
    /// First hour not handled by this pass (deferred, beyond the run bound, or
    /// beyond the lag window).
    pub next_hour: DateTime<Utc>,
    pub deferred: Option<DeferredHour>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeferredHour {
    pub archive_hour: DateTime<Utc>,
    pub attempts: usize,
    pub reason: DeferredReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeferredReason {
    NotReady,
    RetryableFetch(String),
}

#[derive(Debug, Error)]
pub enum HourlyArchiveError {
    #[error("invalid hourly follower configuration: {0}")]
    InvalidConfig(String),
    #[error("tracked repository ID must be positive, got {0}")]
    InvalidTrackedRepositoryId(i64),
    #[error("archive fetch for {hour} failed permanently: {message}")]
    PermanentFetch {
        hour: DateTime<Utc>,
        message: String,
    },
    #[error("archive decode for {hour} failed: {source}")]
    Decode {
        hour: DateTime<Utc>,
        #[source]
        source: ArchiveDecodeError,
    },
    #[error("decoded archive for {hour} exceeds {maximum} bytes")]
    DecodedArchiveTooLarge { hour: DateTime<Utc>, maximum: usize },
    #[error("archive {hour} line {line} exceeds {maximum} bytes")]
    LineTooLarge {
        hour: DateTime<Utc>,
        line: usize,
        maximum: usize,
    },
    #[error("archive {hour} line {line} is invalid JSON: {message}")]
    InvalidEventJson {
        hour: DateTime<Utc>,
        line: usize,
        message: String,
    },
    #[error("archive {hour} line {line} has an invalid created_at value: {message}")]
    InvalidEventTimestamp {
        hour: DateTime<Utc>,
        line: usize,
        message: String,
    },
    #[error("archive {hour} exceeds the matching-event limit of {maximum}")]
    TooManyMatchingEvents { hour: DateTime<Utc>, maximum: usize },
    #[error("tracked repository lookup failed: {0}")]
    RepositorySource(String),
    #[error("hourly archive sink failed: {0}")]
    Sink(String),
    #[error("hour arithmetic overflow")]
    HourOverflow,
}

/// Trait-driven hourly follower. It performs no database writes except through
/// [`HourlyArchiveSink`].
pub struct GhArchiveHourlyFollower {
    config: HourlyFollowerConfig,
    fetcher: Arc<dyn HourlyArchiveFetcher>,
    decoder: Arc<dyn ArchiveDecoder>,
    repositories: Arc<dyn TrackedRepositorySource>,
    sink: Arc<dyn HourlyArchiveSink>,
    sleeper: Arc<dyn RetrySleeper>,
}

impl fmt::Debug for GhArchiveHourlyFollower {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GhArchiveHourlyFollower")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl GhArchiveHourlyFollower {
    pub fn new(
        config: HourlyFollowerConfig,
        fetcher: Arc<dyn HourlyArchiveFetcher>,
        decoder: Arc<dyn ArchiveDecoder>,
        repositories: Arc<dyn TrackedRepositorySource>,
        sink: Arc<dyn HourlyArchiveSink>,
    ) -> Result<Self, HourlyArchiveError> {
        Self::with_sleeper(
            config,
            fetcher,
            decoder,
            repositories,
            sink,
            Arc::new(TokioRetrySleeper),
        )
    }

    pub fn with_sleeper(
        config: HourlyFollowerConfig,
        fetcher: Arc<dyn HourlyArchiveFetcher>,
        decoder: Arc<dyn ArchiveDecoder>,
        repositories: Arc<dyn TrackedRepositorySource>,
        sink: Arc<dyn HourlyArchiveSink>,
        sleeper: Arc<dyn RetrySleeper>,
    ) -> Result<Self, HourlyArchiveError> {
        config.validate()?;
        Ok(Self {
            config,
            fetcher,
            decoder,
            repositories,
            sink,
            sleeper,
        })
    }

    /// Process completed hours from `start_hour` in chronological order.
    ///
    /// The pass stops at the first unavailable hour; later hours are not
    /// committed across a gap. Callers should pass `report.next_hour` to the
    /// next scheduled run.
    pub async fn catch_up(
        &self,
        start_hour: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<FollowReport, HourlyArchiveError> {
        let mut hour = floor_hour(start_hour);
        let eligible_through = latest_eligible_hour(now, self.config.lag)?;
        let mut report = FollowReport {
            eligible_through,
            hours_committed: 0,
            hours_already_committed: 0,
            matching_events: 0,
            next_hour: hour,
            deferred: None,
        };
        let Some(last_hour) = eligible_through else {
            return Ok(report);
        };
        if hour > last_hour {
            return Ok(report);
        }

        let mut tracked_ids: Option<BTreeSet<i64>> = None;
        let mut hours_examined = 0usize;
        while hour <= last_hour && hours_examined < self.config.max_hours_per_run {
            hours_examined += 1;
            if self.sink.is_hour_committed(hour).await? {
                report.hours_already_committed += 1;
                hour = next_hour(hour)?;
                report.next_hour = hour;
                continue;
            }

            if tracked_ids.is_none() {
                let ids = self.repositories.tracked_repository_ids().await?;
                if let Some(invalid) = ids.iter().copied().find(|id| *id <= 0) {
                    return Err(HourlyArchiveError::InvalidTrackedRepositoryId(invalid));
                }
                tracked_ids = Some(ids);
            }
            let ids = tracked_ids
                .as_ref()
                .expect("tracked IDs are initialized above");

            match self.fetch_parse_hour(hour, ids).await? {
                FetchHourOutcome::Ready(batch) => {
                    let event_count = batch.events.len();
                    match self.sink.commit_hour(batch).await? {
                        HourCommit::Committed => {
                            report.hours_committed += 1;
                            report.matching_events += event_count;
                        }
                        HourCommit::AlreadyCommitted => {
                            report.hours_already_committed += 1;
                        }
                    }
                    hour = next_hour(hour)?;
                    report.next_hour = hour;
                }
                FetchHourOutcome::Deferred(deferred) => {
                    report.next_hour = hour;
                    report.deferred = Some(deferred);
                    break;
                }
            }
        }
        Ok(report)
    }

    async fn fetch_parse_hour(
        &self,
        archive_hour: DateTime<Utc>,
        tracked_ids: &BTreeSet<i64>,
    ) -> Result<FetchHourOutcome, HourlyArchiveError> {
        for attempt in 1..=self.config.max_fetch_attempts {
            let retry = match self.fetcher.fetch_hour(archive_hour).await {
                Ok(ArchiveFetch::Ready(compressed)) => {
                    let decoded = self
                        .decoder
                        .decode(&compressed, self.config.max_decoded_bytes)
                        .map_err(|source| HourlyArchiveError::Decode {
                            hour: archive_hour,
                            source,
                        })?;
                    let batch = parse_archive(
                        archive_hour,
                        &decoded,
                        tracked_ids,
                        self.config.max_decoded_bytes,
                        self.config.max_line_bytes,
                        self.config.max_matching_events,
                    )?;
                    return Ok(FetchHourOutcome::Ready(batch));
                }
                Ok(ArchiveFetch::NotReady { retry_after }) => {
                    if attempt == self.config.max_fetch_attempts {
                        return Ok(FetchHourOutcome::Deferred(DeferredHour {
                            archive_hour,
                            attempts: attempt,
                            reason: DeferredReason::NotReady,
                        }));
                    }
                    retry_after
                }
                Err(error) if error.kind == FetchErrorKind::Retryable => {
                    if attempt == self.config.max_fetch_attempts {
                        return Ok(FetchHourOutcome::Deferred(DeferredHour {
                            archive_hour,
                            attempts: attempt,
                            reason: DeferredReason::RetryableFetch(error.message),
                        }));
                    }
                    error.retry_after
                }
                Err(error) => {
                    return Err(HourlyArchiveError::PermanentFetch {
                        hour: archive_hour,
                        message: error.message,
                    });
                }
            };
            self.sleeper
                .sleep(retry_delay(&self.config, attempt - 1, retry))
                .await;
        }
        unreachable!("validated attempt count is non-zero")
    }
}

#[derive(Debug)]
enum FetchHourOutcome {
    Ready(HourBatch),
    Deferred(DeferredHour),
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    id: Option<String>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    #[serde(default)]
    public: bool,
    repo: Option<RawRepository>,
    payload: Option<RawPayload>,
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRepository {
    id: Option<i64>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPayload {
    action: Option<String>,
}

/// Parse decoded NDJSON and retain only public `WatchEvent/started` rows for
/// tracked numeric repository IDs.
pub fn parse_archive(
    archive_hour: DateTime<Utc>,
    decoded: &[u8],
    tracked_ids: &BTreeSet<i64>,
    max_decoded_bytes: usize,
    max_line_bytes: usize,
    max_matching_events: usize,
) -> Result<HourBatch, HourlyArchiveError> {
    if decoded.len() > max_decoded_bytes {
        return Err(HourlyArchiveError::DecodedArchiveTooLarge {
            hour: archive_hour,
            maximum: max_decoded_bytes,
        });
    }

    let mut events = Vec::new();
    let mut records_seen = 0usize;
    let mut seen_event_ids = HashSet::new();
    for (index, line) in decoded.split(|byte| *byte == b'\n').enumerate() {
        let line_number = index + 1;
        let line = trim_ascii(line);
        if line.is_empty() {
            continue;
        }
        records_seen += 1;
        if line.len() > max_line_bytes {
            return Err(HourlyArchiveError::LineTooLarge {
                hour: archive_hour,
                line: line_number,
                maximum: max_line_bytes,
            });
        }
        let raw: RawEvent =
            serde_json::from_slice(line).map_err(|error| HourlyArchiveError::InvalidEventJson {
                hour: archive_hour,
                line: line_number,
                message: error.to_string(),
            })?;
        if !raw.public
            || raw.event_type.as_deref() != Some("WatchEvent")
            || raw
                .payload
                .as_ref()
                .and_then(|payload| payload.action.as_deref())
                != Some("started")
        {
            continue;
        }
        let Some(repository) = raw.repo else {
            continue;
        };
        let Some(repo_id) = repository.id.filter(|id| tracked_ids.contains(id)) else {
            continue;
        };
        if raw
            .id
            .as_ref()
            .is_some_and(|event_id| !seen_event_ids.insert(event_id.clone()))
        {
            continue;
        }
        let created_at =
            raw.created_at
                .ok_or_else(|| HourlyArchiveError::InvalidEventTimestamp {
                    hour: archive_hour,
                    line: line_number,
                    message: "missing created_at".to_string(),
                })?;
        let created_at = DateTime::parse_from_rfc3339(&created_at)
            .map_err(|error| HourlyArchiveError::InvalidEventTimestamp {
                hour: archive_hour,
                line: line_number,
                message: error.to_string(),
            })?
            .with_timezone(&Utc);

        if events.len() == max_matching_events {
            return Err(HourlyArchiveError::TooManyMatchingEvents {
                hour: archive_hour,
                maximum: max_matching_events,
            });
        }
        events.push(GhArchiveStarEvent {
            github_repo_id: Some(repo_id),
            repository: repository.name.unwrap_or_default(),
            source_event_id: raw.id,
            created_at,
        });
    }
    events.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.github_repo_id.cmp(&b.github_repo_id))
            .then_with(|| a.repository.cmp(&b.repository))
            .then_with(|| a.source_event_id.cmp(&b.source_event_id))
    });
    Ok(HourBatch {
        archive_hour: floor_hour(archive_hour),
        records_seen,
        events,
    })
}

pub fn archive_url(archive_hour: DateTime<Utc>) -> String {
    let hour = floor_hour(archive_hour);
    format!(
        "{ARCHIVE_BASE_URL}/{}-{}.json.gz",
        hour.format("%Y-%m-%d"),
        hour.hour()
    )
}

/// Latest archive start whose entire one-hour interval is older than `lag`.
pub fn latest_eligible_hour(
    now: DateTime<Utc>,
    lag: Duration,
) -> Result<Option<DateTime<Utc>>, HourlyArchiveError> {
    let lag = TimeDelta::from_std(lag).map_err(|_| {
        HourlyArchiveError::InvalidConfig("lag does not fit in chrono::TimeDelta".to_string())
    })?;
    let Some(cutoff) = now
        .checked_sub_signed(lag)
        .and_then(|time| time.checked_sub_signed(TimeDelta::hours(1)))
    else {
        return Ok(None);
    };
    Ok(Some(floor_hour(cutoff)))
}

fn floor_hour(time: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp(time.timestamp().div_euclid(3600) * 3600, 0)
        .expect("an existing DateTime remains representable when rounded down by less than an hour")
}

fn next_hour(hour: DateTime<Utc>) -> Result<DateTime<Utc>, HourlyArchiveError> {
    hour.checked_add_signed(TimeDelta::hours(1))
        .ok_or(HourlyArchiveError::HourOverflow)
}

fn retry_delay(
    config: &HourlyFollowerConfig,
    retry_index: usize,
    retry_after: Option<Duration>,
) -> Duration {
    if let Some(retry_after) = retry_after {
        return retry_after.min(config.retry_max);
    }
    let multiplier = 1u32
        .checked_shl(retry_index.min(31) as u32)
        .unwrap_or(u32::MAX);
    config
        .retry_base
        .saturating_mul(multiplier)
        .min(config.retry_max)
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

fn validate_bounded(
    name: &str,
    value: usize,
    minimum: usize,
    maximum: usize,
) -> Result<(), HourlyArchiveError> {
    if !(minimum..=maximum).contains(&value) {
        return Err(HourlyArchiveError::InvalidConfig(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};
    use std::sync::Mutex;

    use super::*;
    use chrono::{NaiveDate, TimeZone};

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.from_utc_datetime(
            &NaiveDate::from_ymd_opt(year, month, day)
                .unwrap()
                .and_hms_opt(hour, minute, 0)
                .unwrap(),
        )
    }

    #[derive(Default)]
    struct IdentityDecoder;

    impl ArchiveDecoder for IdentityDecoder {
        fn decode(
            &self,
            compressed: &[u8],
            max_decoded_bytes: usize,
        ) -> Result<Vec<u8>, ArchiveDecodeError> {
            if compressed.len() > max_decoded_bytes {
                return Err(ArchiveDecodeError::TooLarge {
                    maximum: max_decoded_bytes,
                });
            }
            Ok(compressed.to_vec())
        }
    }

    struct MockFetcher {
        results: Mutex<VecDeque<Result<ArchiveFetch, ArchiveFetchError>>>,
        calls: Mutex<Vec<DateTime<Utc>>>,
    }

    impl MockFetcher {
        fn new(results: Vec<Result<ArchiveFetch, ArchiveFetchError>>) -> Self {
            Self {
                results: Mutex::new(results.into()),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl HourlyArchiveFetcher for MockFetcher {
        async fn fetch_hour(
            &self,
            archive_hour: DateTime<Utc>,
        ) -> Result<ArchiveFetch, ArchiveFetchError> {
            self.calls.lock().unwrap().push(archive_hour);
            self.results
                .lock()
                .unwrap()
                .pop_front()
                .expect("mock fetch result")
        }
    }

    struct StaticRepositories(BTreeSet<i64>);

    #[async_trait]
    impl TrackedRepositorySource for StaticRepositories {
        async fn tracked_repository_ids(&self) -> Result<BTreeSet<i64>, HourlyArchiveError> {
            Ok(self.0.clone())
        }
    }

    #[derive(Default)]
    struct MockSink {
        committed: Mutex<BTreeSet<DateTime<Utc>>>,
        batches: Mutex<Vec<HourBatch>>,
    }

    #[async_trait]
    impl HourlyArchiveSink for MockSink {
        async fn is_hour_committed(
            &self,
            archive_hour: DateTime<Utc>,
        ) -> Result<bool, HourlyArchiveError> {
            Ok(self.committed.lock().unwrap().contains(&archive_hour))
        }

        async fn commit_hour(&self, batch: HourBatch) -> Result<HourCommit, HourlyArchiveError> {
            let mut committed = self.committed.lock().unwrap();
            if !committed.insert(batch.archive_hour) {
                return Ok(HourCommit::AlreadyCommitted);
            }
            self.batches.lock().unwrap().push(batch);
            Ok(HourCommit::Committed)
        }
    }

    #[derive(Default)]
    struct NoopSleeper {
        sleeps: Mutex<Vec<Duration>>,
    }

    #[async_trait]
    impl RetrySleeper for NoopSleeper {
        async fn sleep(&self, duration: Duration) {
            self.sleeps.lock().unwrap().push(duration);
        }
    }

    fn event(
        id: &str,
        event_type: &str,
        public: bool,
        action: &str,
        repo_id: i64,
        repository: &str,
        created_at: &str,
    ) -> String {
        format!(
            r#"{{"id":"{id}","type":"{event_type}","actor":{{"login":"must-not-escape"}},"repo":{{"id":{repo_id},"name":"{repository}"}},"payload":{{"action":"{action}"}},"public":{public},"created_at":"{created_at}"}}"#
        )
    }

    fn follower(
        config: HourlyFollowerConfig,
        fetcher: Arc<MockFetcher>,
        sink: Arc<MockSink>,
        sleeper: Arc<NoopSleeper>,
    ) -> GhArchiveHourlyFollower {
        GhArchiveHourlyFollower::with_sleeper(
            config,
            fetcher,
            Arc::new(IdentityDecoder),
            Arc::new(StaticRepositories(BTreeSet::from([42]))),
            sink,
            sleeper,
        )
        .unwrap()
    }

    #[test]
    fn builds_canonical_hour_url() {
        assert_eq!(
            archive_url(at(2026, 7, 19, 8, 59)),
            "https://data.gharchive.org/2026-07-19-8.json.gz"
        );
    }

    #[test]
    fn lag_requires_the_entire_archive_hour_to_be_complete() {
        assert_eq!(
            latest_eligible_hour(at(2026, 7, 19, 13, 5), Duration::from_secs(15 * 60)).unwrap(),
            Some(at(2026, 7, 19, 11, 0))
        );
        assert_eq!(
            latest_eligible_hour(at(2026, 7, 19, 13, 0), Duration::ZERO).unwrap(),
            Some(at(2026, 7, 19, 12, 0))
        );
    }

    /// Verbatim GH Archive records from `2026-08-14`, one per line, with only
    /// the `actor` values neutralized — CLAUDE.md forbids storing stargazer
    /// identity, and the parser must prove it never reads those fields. Every
    /// key, nesting level, and JSON type is exactly as published, including the
    /// `repo.url` and `actor.*` members the parser ignores and the `PushEvent`
    /// payload that carries no `action` at all.
    ///
    /// This shape is byte-identical to a January 2026 record. It is pinned here
    /// because a renamed field, a changed `action` value, or a new event type
    /// replacing `WatchEvent` would not fail any mock-built test — it would
    /// silently empty the star series instead, which is indistinguishable from
    /// upstream simply carrying fewer events.
    const LIVE_ARCHIVE_RECORDS: &str = concat!(
        r#"{"id":"13268201074","type":"WatchEvent","actor":{"id":1,"login":"must-not-escape","display_login":"must-not-escape","gravatar_id":"","url":"https://api.github.com/users/must-not-escape","avatar_url":"https://avatars.githubusercontent.com/u/1?"},"repo":{"id":2325298,"name":"torvalds/linux","url":"https://api.github.com/repos/torvalds/linux"},"payload":{"action":"started"},"public":true,"created_at":"2026-08-14T04:28:08Z"}"#,
        "\n",
        r#"{"id":"13271400823","type":"WatchEvent","actor":{"id":2,"login":"must-not-escape","display_login":"must-not-escape","gravatar_id":"","url":"https://api.github.com/users/must-not-escape","avatar_url":"https://avatars.githubusercontent.com/u/2?"},"repo":{"id":658928958,"name":"ollama/ollama","url":"https://api.github.com/repos/ollama/ollama"},"payload":{"action":"started"},"public":true,"created_at":"2026-08-14T06:14:22Z"}"#,
        "\n",
        r#"{"id":"13306684751","type":"WatchEvent","actor":{"id":3,"login":"must-not-escape","display_login":"must-not-escape","gravatar_id":"","url":"https://api.github.com/users/must-not-escape","avatar_url":"https://avatars.githubusercontent.com/u/3?"},"repo":{"id":888092115,"name":"microsoft/markitdown","url":"https://api.github.com/repos/microsoft/markitdown"},"payload":{"action":"started"},"public":true,"created_at":"2026-08-14T19:17:44Z"}"#,
        "\n",
        r#"{"id":"13329007852","type":"WatchEvent","actor":{"id":4,"login":"must-not-escape","display_login":"must-not-escape","gravatar_id":"","url":"https://api.github.com/users/must-not-escape","avatar_url":"https://avatars.githubusercontent.com/u/4?"},"repo":{"id":1170291083,"name":"xingkongliang/skills-manager","url":"https://api.github.com/repos/xingkongliang/skills-manager"},"payload":{"action":"started"},"public":true,"created_at":"2026-08-14T12:01:55Z"}"#,
        "\n",
        r#"{"id":"13268201075","type":"PushEvent","actor":{"id":5,"login":"must-not-escape","display_login":"must-not-escape","gravatar_id":"","url":"https://api.github.com/users/must-not-escape","avatar_url":"https://avatars.githubusercontent.com/u/5?"},"repo":{"id":2325298,"name":"torvalds/linux","url":"https://api.github.com/repos/torvalds/linux"},"payload":{"repository_id":2325298,"push_id":26000000000,"ref":"refs/heads/master","head":"0000000000000000000000000000000000000000","before":"1111111111111111111111111111111111111111"},"public":true,"created_at":"2026-08-14T04:30:00Z"}"#,
        "\n",
    );

    /// Pins the filter against the records GH Archive actually publishes, not
    /// against a hand-built approximation of them.
    #[test]
    fn parser_matches_the_live_gh_archive_record_shape() {
        let tracked = BTreeSet::from([2_325_298, 658_928_958, 888_092_115]);
        let batch = parse_archive(
            at(2026, 8, 14, 4, 0),
            LIVE_ARCHIVE_RECORDS.as_bytes(),
            &tracked,
            1024 * 1024,
            64 * 1024,
            100,
        )
        .unwrap();

        assert_eq!(batch.records_seen, 5);
        assert_eq!(
            batch.events,
            vec![
                GhArchiveStarEvent {
                    github_repo_id: Some(2_325_298),
                    repository: "torvalds/linux".to_string(),
                    source_event_id: Some("13268201074".to_string()),
                    created_at: at(2026, 8, 14, 4, 28) + TimeDelta::seconds(8),
                },
                GhArchiveStarEvent {
                    github_repo_id: Some(658_928_958),
                    repository: "ollama/ollama".to_string(),
                    source_event_id: Some("13271400823".to_string()),
                    created_at: at(2026, 8, 14, 6, 14) + TimeDelta::seconds(22),
                },
                GhArchiveStarEvent {
                    github_repo_id: Some(888_092_115),
                    repository: "microsoft/markitdown".to_string(),
                    source_event_id: Some("13306684751".to_string()),
                    created_at: at(2026, 8, 14, 19, 17) + TimeDelta::seconds(44),
                },
            ],
            "the untracked repository and the PushEvent must both be dropped, \
             and the three tracked stars kept in created_at order"
        );

        // The archive carries full actor profiles on every record; none of it
        // may survive parsing.
        assert!(!format!("{batch:?}").contains("must-not-escape"));
    }

    /// A `PushEvent` payload has no `action` member at all. Nothing about that
    /// is exceptional — it is now ~98% of every hourly object — so it must be
    /// skipped silently rather than rejecting the whole hour.
    #[test]
    fn payload_without_an_action_member_is_skipped_not_an_error() {
        let batch = parse_archive(
            at(2026, 8, 14, 4, 0),
            br#"{"id":"1","type":"PushEvent","repo":{"id":42,"name":"owner/repo"},"payload":{"push_id":7,"ref":"refs/heads/main"},"public":true,"created_at":"2026-08-14T04:30:00Z"}"#,
            &BTreeSet::from([42]),
            1024 * 1024,
            64 * 1024,
            100,
        )
        .unwrap();

        assert_eq!(batch.records_seen, 1);
        assert!(batch.events.is_empty());
    }

    #[test]
    fn parser_keeps_only_public_started_watch_events_for_tracked_numeric_ids() {
        let input = [
            event(
                "keep",
                "WatchEvent",
                true,
                "started",
                42,
                "owner/repo",
                "2026-07-19T12:01:00Z",
            ),
            event(
                "private",
                "WatchEvent",
                false,
                "started",
                42,
                "owner/repo",
                "2026-07-19T12:02:00Z",
            ),
            event(
                "wrong-action",
                "WatchEvent",
                true,
                "deleted",
                42,
                "owner/repo",
                "2026-07-19T12:03:00Z",
            ),
            event(
                "wrong-type",
                "PushEvent",
                true,
                "started",
                42,
                "owner/repo",
                "2026-07-19T12:04:00Z",
            ),
            event(
                "untracked",
                "WatchEvent",
                true,
                "started",
                99,
                "other/repo",
                "2026-07-19T12:05:00Z",
            ),
            // Duplicate source IDs within one archive are ignored.
            event(
                "keep",
                "WatchEvent",
                true,
                "started",
                42,
                "owner/repo",
                "2026-07-19T12:06:00Z",
            ),
        ]
        .join("\n");

        let batch = parse_archive(
            at(2026, 7, 19, 12, 30),
            input.as_bytes(),
            &BTreeSet::from([42]),
            1024 * 1024,
            64 * 1024,
            100,
        )
        .unwrap();

        assert_eq!(batch.archive_hour, at(2026, 7, 19, 12, 0));
        assert_eq!(batch.records_seen, 6);
        assert_eq!(
            batch.events,
            vec![GhArchiveStarEvent {
                github_repo_id: Some(42),
                repository: "owner/repo".to_string(),
                source_event_id: Some("keep".to_string()),
                created_at: at(2026, 7, 19, 12, 1),
            }]
        );
    }

    #[test]
    fn malformed_json_rejects_the_whole_hour() {
        let error = parse_archive(
            at(2026, 7, 19, 12, 0),
            b"{not-json}\n",
            &BTreeSet::from([42]),
            1024,
            1024,
            100,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            HourlyArchiveError::InvalidEventJson { line: 1, .. }
        ));
    }

    #[tokio::test]
    async fn not_found_retries_then_defers_without_checkpointing() {
        let fetcher = Arc::new(MockFetcher::new(vec![
            Ok(ArchiveFetch::NotReady { retry_after: None }),
            Ok(ArchiveFetch::NotReady { retry_after: None }),
            Ok(ArchiveFetch::NotReady { retry_after: None }),
        ]));
        let sink = Arc::new(MockSink::default());
        let sleeper = Arc::new(NoopSleeper::default());
        let config = HourlyFollowerConfig {
            max_fetch_attempts: 3,
            ..HourlyFollowerConfig::default()
        };
        let follower = follower(config, fetcher.clone(), sink.clone(), sleeper.clone());
        let hour = at(2026, 7, 19, 10, 0);

        let report = follower
            .catch_up(hour, at(2026, 7, 19, 13, 0))
            .await
            .unwrap();

        assert_eq!(fetcher.calls.lock().unwrap().len(), 3);
        assert_eq!(sleeper.sleeps.lock().unwrap().len(), 2);
        assert!(sink.batches.lock().unwrap().is_empty());
        assert_eq!(
            report.deferred,
            Some(DeferredHour {
                archive_hour: hour,
                attempts: 3,
                reason: DeferredReason::NotReady,
            })
        );
        assert_eq!(report.next_hour, hour);
    }

    #[tokio::test]
    async fn retryable_fetch_can_recover_and_commit_atomically() {
        let body = event(
            "one",
            "WatchEvent",
            true,
            "started",
            42,
            "owner/repo",
            "2026-07-19T10:05:00Z",
        );
        let fetcher = Arc::new(MockFetcher::new(vec![
            Err(ArchiveFetchError::retryable("temporary", None)),
            Ok(ArchiveFetch::Ready(body.into_bytes())),
        ]));
        let sink = Arc::new(MockSink::default());
        let sleeper = Arc::new(NoopSleeper::default());
        let follower = follower(
            HourlyFollowerConfig {
                max_hours_per_run: 1,
                ..HourlyFollowerConfig::default()
            },
            fetcher,
            sink.clone(),
            sleeper.clone(),
        );

        let report = follower
            .catch_up(at(2026, 7, 19, 10, 0), at(2026, 7, 19, 13, 0))
            .await
            .unwrap();

        assert_eq!(report.hours_committed, 1);
        assert_eq!(report.matching_events, 1);
        assert_eq!(sleeper.sleeps.lock().unwrap().len(), 1);
        assert_eq!(sink.batches.lock().unwrap().len(), 1);
        assert_eq!(report.next_hour, at(2026, 7, 19, 11, 0));
    }

    #[tokio::test]
    async fn committed_hours_are_skipped_without_fetching() {
        let first = at(2026, 7, 19, 10, 0);
        let second = at(2026, 7, 19, 11, 0);
        let sink = Arc::new(MockSink::default());
        sink.committed.lock().unwrap().insert(first);
        let fetcher = Arc::new(MockFetcher::new(vec![Ok(ArchiveFetch::Ready(Vec::new()))]));
        let follower = follower(
            HourlyFollowerConfig {
                max_hours_per_run: 2,
                ..HourlyFollowerConfig::default()
            },
            fetcher.clone(),
            sink,
            Arc::new(NoopSleeper::default()),
        );

        let report = follower
            .catch_up(first, at(2026, 7, 19, 13, 0))
            .await
            .unwrap();

        assert_eq!(report.hours_already_committed, 1);
        assert_eq!(report.hours_committed, 1);
        assert_eq!(fetcher.calls.lock().unwrap().as_slice(), &[second]);
        assert_eq!(report.next_hour, at(2026, 7, 19, 12, 0));
    }
}
