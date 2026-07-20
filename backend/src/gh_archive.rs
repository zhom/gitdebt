//! Read-only GH Archive star-event acquisition through BigQuery.
//!
//! This module deliberately has no database or queue dependencies. A worker can
//! consume [`GhArchiveEventSource`] directly, or replace it with a test double.
//! Queries select only repository identity and `created_at`; actor/profile data
//! never leaves BigQuery.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use gcp_auth::{CustomServiceAccount, TokenProvider};
use reqwest::header::RETRY_AFTER;
use reqwest::{Method, StatusCode, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::Semaphore;

const BIGQUERY_API_BASE: &str = "https://bigquery.googleapis.com/bigquery/v2/";
const BIGQUERY_SCOPE: &str = "https://www.googleapis.com/auth/bigquery";
const USER_AGENT: &str = concat!("gitdebt/", env!("CARGO_PKG_VERSION"));

const DEFAULT_MAX_BYTES_BILLED: u64 = 25_000_000_000;
const DEFAULT_MAX_EVENTS: usize = 100_000;
const DEFAULT_PAGE_SIZE: u32 = 10_000;
const DEFAULT_CONCURRENCY: usize = 1;
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;
const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 120;
const DEFAULT_POLL_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_MAX_RETRIES: usize = 3;
const DEFAULT_MAX_REPOSITORIES: usize = 200;
const DEFAULT_MAX_RANGE_DAYS: i64 = 31;

const HARD_MAX_BYTES_BILLED: u64 = 1_000_000_000_000;
const HARD_MAX_EVENTS: usize = 500_000;
const HARD_MAX_PAGE_SIZE: u32 = 10_000;
const HARD_MAX_CONCURRENCY: usize = 8;
const HARD_MAX_REQUEST_TIMEOUT_SECS: u64 = 120;
const HARD_MAX_QUERY_TIMEOUT_SECS: u64 = 600;
const HARD_MAX_POLL_TIMEOUT_MS: u64 = 20_000;
const HARD_MAX_RETRIES: usize = 8;
const HARD_MAX_REPOSITORIES: usize = 1_000;
const HARD_MAX_RANGE_DAYS: i64 = 366;

/// A repository to match in GH Archive.
///
/// `github_id` should be populated whenever GitHub metadata has supplied it.
/// `full_name` is always required so pre-ID/legacy events still have a
/// lowercase-name fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositorySpec {
    pub github_id: Option<i64>,
    pub full_name: String,
}

impl RepositorySpec {
    pub fn new(github_id: Option<i64>, full_name: impl Into<String>) -> Self {
        Self {
            github_id,
            full_name: full_name.into(),
        }
    }
}

/// The only event data exposed by the GH Archive client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GhArchiveStarEvent {
    pub github_repo_id: Option<i64>,
    pub repository: String,
    pub source_event_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A bounded result batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GhArchiveFetch {
    pub events: Vec<GhArchiveStarEvent>,
    /// Conservative completeness signal. `true` means the configured event
    /// cap was reached and the caller must continue with a narrower window.
    pub truncated: bool,
    pub total_bytes_processed: u64,
}

/// Worker-facing abstraction. Implementations must return events in
/// deterministic oldest-first order.
#[async_trait]
pub trait GhArchiveEventSource: Send + Sync {
    async fn fetch_star_events(
        &self,
        repositories: &[RepositorySpec],
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<GhArchiveFetch, GhArchiveError>;
}

/// Validated production configuration. Use [`Self::from_env`] for normal
/// startup and [`Self::new`] for explicit construction.
#[derive(Clone)]
pub struct GhArchiveConfig {
    pub project_id: String,
    pub location: String,
    pub max_bytes_billed: u64,
    pub max_events: usize,
    pub page_size: u32,
    pub concurrency: usize,
    pub request_timeout: Duration,
    pub query_timeout: Duration,
    pub poll_timeout: Duration,
    pub max_retries: usize,
    pub max_repositories: usize,
    pub max_range_days: i64,
    credentials_json: Option<Arc<str>>,
}

impl fmt::Debug for GhArchiveConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GhArchiveConfig")
            .field("project_id", &self.project_id)
            .field("location", &self.location)
            .field("max_bytes_billed", &self.max_bytes_billed)
            .field("max_events", &self.max_events)
            .field("page_size", &self.page_size)
            .field("concurrency", &self.concurrency)
            .field("request_timeout", &self.request_timeout)
            .field("query_timeout", &self.query_timeout)
            .field("poll_timeout", &self.poll_timeout)
            .field("max_retries", &self.max_retries)
            .field("max_repositories", &self.max_repositories)
            .field("max_range_days", &self.max_range_days)
            .field(
                "credentials_json",
                &self.credentials_json.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl GhArchiveConfig {
    pub fn new(project_id: impl Into<String>) -> Result<Self, GhArchiveError> {
        let config = Self {
            project_id: project_id.into(),
            location: "US".to_string(),
            max_bytes_billed: DEFAULT_MAX_BYTES_BILLED,
            max_events: DEFAULT_MAX_EVENTS,
            page_size: DEFAULT_PAGE_SIZE,
            concurrency: DEFAULT_CONCURRENCY,
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
            query_timeout: Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
            poll_timeout: Duration::from_millis(DEFAULT_POLL_TIMEOUT_MS),
            max_retries: DEFAULT_MAX_RETRIES,
            max_repositories: DEFAULT_MAX_REPOSITORIES,
            max_range_days: DEFAULT_MAX_RANGE_DAYS,
            credentials_json: None,
        };
        config.validate()?;
        Ok(config)
    }

    /// `GH_ARCHIVE_ENABLED` defaults to false. When enabled,
    /// `GH_ARCHIVE_BIGQUERY_PROJECT` is mandatory.
    ///
    /// Optional settings:
    /// - `GH_ARCHIVE_GOOGLE_CREDENTIALS_JSON` (inline service-account JSON)
    /// - `GH_ARCHIVE_BIGQUERY_LOCATION` (default `US`)
    /// - `GH_ARCHIVE_MAX_BYTES_BILLED`
    /// - `GH_ARCHIVE_MAX_EVENTS`
    /// - `GH_ARCHIVE_PAGE_SIZE`
    /// - `GH_ARCHIVE_CONCURRENCY`
    /// - `GH_ARCHIVE_HTTP_TIMEOUT_SECS`
    /// - `GH_ARCHIVE_QUERY_TIMEOUT_SECS`
    /// - `GH_ARCHIVE_POLL_TIMEOUT_MS`
    /// - `GH_ARCHIVE_MAX_RETRIES`
    /// - `GH_ARCHIVE_MAX_REPOSITORIES`
    /// - `GH_ARCHIVE_MAX_RANGE_DAYS`
    pub fn from_env() -> Result<Option<Self>, GhArchiveError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, GhArchiveError> {
        let enabled = parse_bool_env(
            "GH_ARCHIVE_ENABLED",
            lookup("GH_ARCHIVE_ENABLED").as_deref(),
            false,
        )?;
        if !enabled {
            return Ok(None);
        }

        let project_id = required_env(
            "GH_ARCHIVE_BIGQUERY_PROJECT",
            lookup("GH_ARCHIVE_BIGQUERY_PROJECT"),
        )?;
        let mut config = Self::new(project_id)?;

        if let Some(value) = nonempty(lookup("GH_ARCHIVE_BIGQUERY_LOCATION")) {
            config.location = value;
        }
        config.max_bytes_billed = parse_u64_env(
            "GH_ARCHIVE_MAX_BYTES_BILLED",
            lookup("GH_ARCHIVE_MAX_BYTES_BILLED"),
            DEFAULT_MAX_BYTES_BILLED,
            1,
            HARD_MAX_BYTES_BILLED,
        )?;
        config.max_events = parse_usize_env(
            "GH_ARCHIVE_MAX_EVENTS",
            lookup("GH_ARCHIVE_MAX_EVENTS"),
            DEFAULT_MAX_EVENTS,
            1,
            HARD_MAX_EVENTS,
        )?;
        config.page_size = parse_u32_env(
            "GH_ARCHIVE_PAGE_SIZE",
            lookup("GH_ARCHIVE_PAGE_SIZE"),
            DEFAULT_PAGE_SIZE,
            1,
            HARD_MAX_PAGE_SIZE,
        )?;
        config.concurrency = parse_usize_env(
            "GH_ARCHIVE_CONCURRENCY",
            lookup("GH_ARCHIVE_CONCURRENCY"),
            DEFAULT_CONCURRENCY,
            1,
            HARD_MAX_CONCURRENCY,
        )?;
        config.request_timeout = Duration::from_secs(parse_u64_env(
            "GH_ARCHIVE_HTTP_TIMEOUT_SECS",
            lookup("GH_ARCHIVE_HTTP_TIMEOUT_SECS"),
            DEFAULT_REQUEST_TIMEOUT_SECS,
            1,
            HARD_MAX_REQUEST_TIMEOUT_SECS,
        )?);
        config.query_timeout = Duration::from_secs(parse_u64_env(
            "GH_ARCHIVE_QUERY_TIMEOUT_SECS",
            lookup("GH_ARCHIVE_QUERY_TIMEOUT_SECS"),
            DEFAULT_QUERY_TIMEOUT_SECS,
            5,
            HARD_MAX_QUERY_TIMEOUT_SECS,
        )?);
        config.poll_timeout = Duration::from_millis(parse_u64_env(
            "GH_ARCHIVE_POLL_TIMEOUT_MS",
            lookup("GH_ARCHIVE_POLL_TIMEOUT_MS"),
            DEFAULT_POLL_TIMEOUT_MS,
            100,
            HARD_MAX_POLL_TIMEOUT_MS,
        )?);
        config.max_retries = parse_usize_env(
            "GH_ARCHIVE_MAX_RETRIES",
            lookup("GH_ARCHIVE_MAX_RETRIES"),
            DEFAULT_MAX_RETRIES,
            0,
            HARD_MAX_RETRIES,
        )?;
        config.max_repositories = parse_usize_env(
            "GH_ARCHIVE_MAX_REPOSITORIES",
            lookup("GH_ARCHIVE_MAX_REPOSITORIES"),
            DEFAULT_MAX_REPOSITORIES,
            1,
            HARD_MAX_REPOSITORIES,
        )?;
        config.max_range_days = parse_i64_env(
            "GH_ARCHIVE_MAX_RANGE_DAYS",
            lookup("GH_ARCHIVE_MAX_RANGE_DAYS"),
            DEFAULT_MAX_RANGE_DAYS,
            1,
            HARD_MAX_RANGE_DAYS,
        )?;

        if let Some(credentials) = nonempty(lookup("GH_ARCHIVE_GOOGLE_CREDENTIALS_JSON")) {
            validate_credentials_json(&credentials)?;
            config.credentials_json = Some(Arc::from(credentials));
        }

        config.validate()?;
        Ok(Some(config))
    }

    fn validate(&self) -> Result<(), GhArchiveError> {
        validate_project_id(&self.project_id)?;
        validate_location(&self.location)?;
        validate_range(
            "GH_ARCHIVE_MAX_BYTES_BILLED",
            self.max_bytes_billed,
            1,
            HARD_MAX_BYTES_BILLED,
        )?;
        validate_range("GH_ARCHIVE_MAX_EVENTS", self.max_events, 1, HARD_MAX_EVENTS)?;
        validate_range(
            "GH_ARCHIVE_PAGE_SIZE",
            self.page_size,
            1,
            HARD_MAX_PAGE_SIZE,
        )?;
        validate_range(
            "GH_ARCHIVE_CONCURRENCY",
            self.concurrency,
            1,
            HARD_MAX_CONCURRENCY,
        )?;
        validate_range(
            "GH_ARCHIVE_MAX_RETRIES",
            self.max_retries,
            0,
            HARD_MAX_RETRIES,
        )?;
        validate_range(
            "GH_ARCHIVE_MAX_REPOSITORIES",
            self.max_repositories,
            1,
            HARD_MAX_REPOSITORIES,
        )?;
        validate_range(
            "GH_ARCHIVE_MAX_RANGE_DAYS",
            self.max_range_days,
            1,
            HARD_MAX_RANGE_DAYS,
        )?;
        if self.request_timeout.is_zero()
            || self.request_timeout > Duration::from_secs(HARD_MAX_REQUEST_TIMEOUT_SECS)
            || self.query_timeout < Duration::from_secs(5)
            || self.query_timeout > Duration::from_secs(HARD_MAX_QUERY_TIMEOUT_SECS)
            || self.poll_timeout < Duration::from_millis(100)
            || self.poll_timeout > Duration::from_millis(HARD_MAX_POLL_TIMEOUT_MS)
        {
            return Err(invalid_config(
                "GH_ARCHIVE_TIMEOUTS",
                "timeouts must be positive and the total query timeout must be at least 5 seconds",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum GhArchiveError {
    #[error("missing required configuration: {0}")]
    MissingConfig(&'static str),
    #[error("invalid {name}: {reason}")]
    InvalidConfig { name: &'static str, reason: String },
    #[error("invalid repository spec: {0}")]
    InvalidRepository(String),
    #[error("too many repositories: {actual}; maximum is {maximum}")]
    TooManyRepositories { actual: usize, maximum: usize },
    #[error("date range must be ordered and no longer than {maximum_days} days")]
    InvalidDateRange { maximum_days: i64 },
    #[error("GCP authentication failed: {0}")]
    Auth(#[from] gcp_auth::Error),
    #[error("BigQuery HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("BigQuery API returned HTTP {status}: {message}")]
    Api { status: u16, message: String },
    #[error("BigQuery response was invalid: {0}")]
    InvalidResponse(String),
    #[error("BigQuery query exceeded its {0:?} client deadline")]
    Timeout(Duration),
    #[error("GH Archive concurrency limiter is closed")]
    ConcurrencyClosed,
    #[error("could not serialize or parse BigQuery JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone)]
pub struct GhArchiveBigQueryClient {
    config: Arc<GhArchiveConfig>,
    http: reqwest::Client,
    token_provider: Arc<dyn TokenProvider>,
    permits: Arc<Semaphore>,
}

impl fmt::Debug for GhArchiveBigQueryClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GhArchiveBigQueryClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl GhArchiveBigQueryClient {
    /// Build an enabled client from environment configuration. Authentication
    /// is not attempted when `GH_ARCHIVE_ENABLED` is false.
    pub async fn from_env() -> Result<Option<Self>, GhArchiveError> {
        match GhArchiveConfig::from_env()? {
            Some(config) => Self::new(config).await.map(Some),
            None => Ok(None),
        }
    }

    pub async fn new(config: GhArchiveConfig) -> Result<Self, GhArchiveError> {
        config.validate()?;
        let token_provider: Arc<dyn TokenProvider> =
            if let Some(credentials) = config.credentials_json.as_deref() {
                Arc::new(CustomServiceAccount::from_json(credentials)?)
            } else {
                gcp_auth::provider().await?
            };
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(5))
            .timeout(config.request_timeout)
            .pool_idle_timeout(Duration::from_secs(90))
            .build()?;
        let concurrency = config.concurrency;
        Ok(Self {
            config: Arc::new(config),
            http,
            token_provider,
            permits: Arc::new(Semaphore::new(concurrency)),
        })
    }

    async fn fetch_inner(
        &self,
        repositories: &[NormalizedRepository],
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<GhArchiveFetch, GhArchiveError> {
        let request = build_query_request(&self.config, repositories, start, end);
        let body = serde_json::to_value(request)?;
        let url = query_url(&self.config.project_id);
        let mut page: QueryResultsPage = self.send_json(Method::POST, url, Some(&body)).await?;

        let mut events = Vec::with_capacity(self.config.page_size as usize);
        let mut seen_page_tokens = HashSet::new();
        let mut job_reference = page.job_reference.clone();
        let mut total_rows = None;
        let mut total_bytes_processed = 0u64;

        loop {
            if !page.errors.is_empty() {
                return Err(GhArchiveError::InvalidResponse(format!(
                    "query failed: {}",
                    format_query_errors(&page.errors)
                )));
            }

            if !page.job_complete {
                let job = job_reference.as_ref().ok_or_else(|| {
                    GhArchiveError::InvalidResponse(
                        "incomplete query response omitted jobReference".to_string(),
                    )
                })?;
                tokio::time::sleep(Duration::from_millis(200)).await;
                page = self.get_results_page(job, None).await?;
                if page.job_reference.is_some() {
                    job_reference = page.job_reference.clone();
                }
                continue;
            }

            update_u64_max(
                &mut total_bytes_processed,
                page.total_bytes_processed.as_deref(),
                "totalBytesProcessed",
            )?;
            if let Some(rows) = page.total_rows.as_deref() {
                total_rows = Some(parse_u64_field(rows, "totalRows")?);
            }
            for row in &page.rows {
                if events.len() > self.config.max_events {
                    break;
                }
                events.push(parse_event_row(row)?);
            }

            let known_truncated =
                total_rows.is_some_and(|rows| rows > self.config.max_events as u64);
            if events.len() > self.config.max_events
                || (known_truncated && events.len() >= self.config.max_events)
            {
                break;
            }

            let Some(page_token) = nonempty(page.page_token.take()) else {
                break;
            };
            if !seen_page_tokens.insert(page_token.clone()) {
                return Err(GhArchiveError::InvalidResponse(
                    "BigQuery repeated a pageToken".to_string(),
                ));
            }
            let job = job_reference.as_ref().ok_or_else(|| {
                GhArchiveError::InvalidResponse(
                    "paginated query response omitted jobReference".to_string(),
                )
            })?;
            page = self.get_results_page(job, Some(&page_token)).await?;
            if page.job_reference.is_some() {
                job_reference = page.job_reference.clone();
            }
        }

        sort_events(&mut events);
        let truncated = total_rows.is_some_and(|rows| rows > self.config.max_events as u64)
            || events.len() > self.config.max_events;
        events.truncate(self.config.max_events);

        Ok(GhArchiveFetch {
            events,
            truncated,
            total_bytes_processed,
        })
    }

    async fn get_results_page(
        &self,
        job: &JobReference,
        page_token: Option<&str>,
    ) -> Result<QueryResultsPage, GhArchiveError> {
        if job.project_id.is_empty() || job.job_id.is_empty() {
            return Err(GhArchiveError::InvalidResponse(
                "jobReference contains an empty projectId or jobId".to_string(),
            ));
        }
        let url = results_page_url(&self.config, job, page_token);
        self.send_json(Method::GET, url, None).await
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
    ) -> Result<T, GhArchiveError> {
        for attempt in 0..=self.config.max_retries {
            let token = self.token_provider.token(&[BIGQUERY_SCOPE]).await?;
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(token.as_str());
            if let Some(body) = body {
                request = request.json(body);
            }

            match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    let retry_after = retry_after_seconds(response.headers());
                    let text = response.text().await?;
                    if status.is_success() {
                        return serde_json::from_str(&text).map_err(GhArchiveError::from);
                    }
                    if is_retryable_status(status) && attempt < self.config.max_retries {
                        tokio::time::sleep(retry_delay(attempt, retry_after)).await;
                        continue;
                    }
                    return Err(GhArchiveError::Api {
                        status: status.as_u16(),
                        message: api_error_message(&text),
                    });
                }
                Err(error)
                    if (error.is_connect() || error.is_timeout())
                        && attempt < self.config.max_retries =>
                {
                    tokio::time::sleep(retry_delay(attempt, None)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("retry loop always returns on its last attempt")
    }
}

#[async_trait]
impl GhArchiveEventSource for GhArchiveBigQueryClient {
    async fn fetch_star_events(
        &self,
        repositories: &[RepositorySpec],
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<GhArchiveFetch, GhArchiveError> {
        if repositories.is_empty() {
            return Ok(GhArchiveFetch {
                events: Vec::new(),
                truncated: false,
                total_bytes_processed: 0,
            });
        }
        let repositories = normalize_repositories(repositories)?;
        if repositories.len() > self.config.max_repositories {
            return Err(GhArchiveError::TooManyRepositories {
                actual: repositories.len(),
                maximum: self.config.max_repositories,
            });
        }
        let days = end.signed_duration_since(start).num_days() + 1;
        if days <= 0 || days > self.config.max_range_days {
            return Err(GhArchiveError::InvalidDateRange {
                maximum_days: self.config.max_range_days,
            });
        }

        let timeout = self.config.query_timeout;
        let operation = async {
            let _permit = self
                .permits
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| GhArchiveError::ConcurrencyClosed)?;
            self.fetch_inner(&repositories, start, end).await
        };
        tokio::time::timeout(timeout, operation)
            .await
            .map_err(|_| GhArchiveError::Timeout(timeout))?
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NormalizedRepository {
    github_id: Option<i64>,
    lower_name: String,
}

fn normalize_repositories(
    repositories: &[RepositorySpec],
) -> Result<Vec<NormalizedRepository>, GhArchiveError> {
    let mut normalized = Vec::with_capacity(repositories.len());
    for repository in repositories {
        if repository.github_id.is_some_and(|id| id <= 0) {
            return Err(GhArchiveError::InvalidRepository(format!(
                "{} has a non-positive GitHub ID",
                repository.full_name
            )));
        }
        let full_name = repository.full_name.trim();
        if !is_valid_repo_name(full_name) {
            return Err(GhArchiveError::InvalidRepository(
                repository.full_name.clone(),
            ));
        }
        normalized.push(NormalizedRepository {
            github_id: repository.github_id,
            lower_name: full_name.to_ascii_lowercase(),
        });
    }
    normalized.sort_by(|a, b| {
        a.github_id
            .cmp(&b.github_id)
            .then_with(|| a.lower_name.cmp(&b.lower_name))
    });
    normalized.dedup();
    Ok(normalized)
}

fn is_valid_repo_name(name: &str) -> bool {
    if name.len() > 201 {
        return false;
    }
    let mut parts = name.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() {
        return false;
    }
    owner
        .bytes()
        .chain(repo.bytes())
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryRequest {
    query: String,
    use_legacy_sql: bool,
    parameter_mode: &'static str,
    query_parameters: Vec<QueryParameter>,
    max_results: u32,
    timeout_ms: u64,
    job_timeout_ms: String,
    maximum_bytes_billed: String,
    use_query_cache: bool,
    location: String,
    labels: BTreeMap<&'static str, &'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryParameter {
    name: &'static str,
    parameter_type: Value,
    parameter_value: Value,
}

fn build_query_request(
    config: &GhArchiveConfig,
    repositories: &[NormalizedRepository],
    start: NaiveDate,
    end: NaiveDate,
) -> QueryRequest {
    let repository_values = repositories
        .iter()
        .map(|repository| {
            // BigQuery query parameters cannot be NULL. GitHub repository IDs
            // are positive, so zero is an unambiguous "name-only" sentinel.
            json!({
                "structValues": {
                    "github_id": {
                        "value": repository.github_id.map(|id| id.to_string())
                    },
                    "lower_name": {"value": repository.lower_name},
                }
            })
        })
        .collect::<Vec<_>>();
    let query_limit = config.max_events.saturating_add(1);
    let query_timeout_ms = config.poll_timeout.as_millis().min(u64::MAX as u128) as u64;
    let job_timeout_ms = config
        .query_timeout
        .as_millis()
        .min(u64::MAX as u128)
        .to_string();

    QueryRequest {
        query: build_star_events_sql(start, end),
        use_legacy_sql: false,
        parameter_mode: "NAMED",
        query_parameters: vec![
            scalar_parameter("start_date", "DATE", start.to_string()),
            scalar_parameter("end_date", "DATE", end.to_string()),
            QueryParameter {
                name: "repositories",
                parameter_type: json!({
                    "type": "ARRAY",
                    "arrayType": {
                        "type": "STRUCT",
                        "structTypes": [
                            {"name": "github_id", "type": {"type": "INT64"}},
                            {"name": "lower_name", "type": {"type": "STRING"}}
                        ]
                    }
                }),
                parameter_value: json!({"arrayValues": repository_values}),
            },
            scalar_parameter("query_limit", "INT64", query_limit.to_string()),
        ],
        max_results: config.page_size.min(query_limit as u32),
        timeout_ms: query_timeout_ms,
        job_timeout_ms,
        maximum_bytes_billed: config.max_bytes_billed.to_string(),
        use_query_cache: true,
        location: config.location.clone(),
        labels: BTreeMap::from([("component", "gh_archive"), ("service", "gitdebt")]),
    }
}

/// Build GoogleSQL over exact official GH Archive month resources.
///
/// `githubarchive.day.*` cannot be used safely because that prefix also
/// contains the `day.yesterday` view, and BigQuery rejects wildcard queries
/// when any matched resource is a view. Month identifiers are derived only
/// from validated `NaiveDate` values, so the dynamic table names cannot carry
/// user input. The selected columns intentionally exclude actors and payloads.
fn build_star_events_sql(start: NaiveDate, end: NaiveDate) -> String {
    let mut year = start.year();
    let mut month = start.month();
    let mut selects = Vec::new();
    loop {
        selects.push(format!(
            r#"SELECT
  repo.id AS github_repo_id,
  repo.name AS repository,
  id AS source_event_id,
  FORMAT_TIMESTAMP('%Y-%m-%dT%H:%M:%E6SZ', created_at) AS created_at
FROM `githubarchive.month.{year:04}{month:02}`
WHERE DATE(created_at) BETWEEN @start_date AND @end_date
  AND type = 'WatchEvent'
  AND public = TRUE
  AND JSON_VALUE(payload, '$.action') = 'started'
  AND EXISTS (
    SELECT 1
    FROM UNNEST(@repositories) AS requested
    WHERE
      (
        requested.github_id > 0
        AND repo.id = requested.github_id
      )
      OR
      (
        LOWER(repo.name) = requested.lower_name
        AND (requested.github_id = 0 OR repo.id IS NULL)
      )
  )"#
        ));
        if year == end.year() && month == end.month() {
            break;
        }
        if month == 12 {
            year += 1;
            month = 1;
        } else {
            month += 1;
        }
    }
    format!(
        "SELECT * FROM (\n{}\n)\n\
         ORDER BY created_at ASC, github_repo_id ASC, LOWER(repository) ASC, source_event_id ASC\n\
         LIMIT @query_limit",
        selects.join("\nUNION ALL\n")
    )
}

fn scalar_parameter(name: &'static str, kind: &'static str, value: String) -> QueryParameter {
    QueryParameter {
        name,
        parameter_type: json!({"type": kind}),
        parameter_value: json!({"value": value}),
    }
}

fn query_url(project_id: &str) -> Url {
    api_url(&["projects", project_id, "queries"])
}

fn results_url(project_id: &str, job_id: &str) -> Url {
    api_url(&["projects", project_id, "queries", job_id])
}

fn results_page_url(config: &GhArchiveConfig, job: &JobReference, page_token: Option<&str>) -> Url {
    let mut url = results_url(&job.project_id, &job.job_id);
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("maxResults", &config.page_size.to_string());
        query.append_pair("timeoutMs", &config.poll_timeout.as_millis().to_string());
        query.append_pair(
            "location",
            job.location.as_deref().unwrap_or(&config.location),
        );
        if let Some(page_token) = page_token {
            query.append_pair("pageToken", page_token);
        }
    }
    url
}

fn api_url(segments: &[&str]) -> Url {
    let mut url = Url::parse(BIGQUERY_API_BASE).expect("BigQuery API base URL is valid");
    let mut path = url
        .path_segments_mut()
        .expect("BigQuery API base URL supports path segments");
    path.pop_if_empty();
    for segment in segments {
        path.push(segment);
    }
    drop(path);
    url
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryResultsPage {
    #[serde(default)]
    job_complete: bool,
    job_reference: Option<JobReference>,
    total_rows: Option<String>,
    page_token: Option<String>,
    #[serde(default)]
    rows: Vec<TableRow>,
    total_bytes_processed: Option<String>,
    #[serde(default)]
    errors: Vec<QueryError>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobReference {
    project_id: String,
    job_id: String,
    location: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct TableRow {
    #[serde(default)]
    f: Vec<TableCell>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct TableCell {
    v: Value,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct QueryError {
    reason: Option<String>,
    message: Option<String>,
}

fn parse_event_row(row: &TableRow) -> Result<GhArchiveStarEvent, GhArchiveError> {
    if row.f.len() != 4 {
        return Err(GhArchiveError::InvalidResponse(format!(
            "expected 4 fields per row, got {}",
            row.f.len()
        )));
    }
    let github_repo_id = match cell_string(&row.f[0].v) {
        Some(value) => {
            let id = value.parse::<i64>().map_err(|_| {
                GhArchiveError::InvalidResponse("github_repo_id was not an INT64".to_string())
            })?;
            if id <= 0 {
                return Err(GhArchiveError::InvalidResponse(
                    "github_repo_id was not positive".to_string(),
                ));
            }
            Some(id)
        }
        None => None,
    };
    // Old GH Archive events occasionally retain a missing or malformed
    // repository name after a rename/deletion. A positive GitHub repository
    // ID is the authoritative identity in that case, so preserve the event
    // and leave the display-only name empty. Rows without an ID still require
    // a validated slug because name matching is their only identity.
    let repository = match cell_string(&row.f[1].v).filter(|value| is_valid_repo_name(value)) {
        Some(value) => value.to_string(),
        None if github_repo_id.is_some() => String::new(),
        None => {
            return Err(GhArchiveError::InvalidResponse(
                "repository identity was missing or malformed".to_string(),
            ));
        }
    };
    let source_event_id = cell_string(&row.f[2].v)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let created_at = cell_string(&row.f[3].v)
        .ok_or_else(|| GhArchiveError::InvalidResponse("created_at was missing".to_string()))
        .and_then(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|timestamp| timestamp.with_timezone(&Utc))
                .map_err(|_| {
                    GhArchiveError::InvalidResponse(
                        "created_at was not an RFC3339 timestamp".to_string(),
                    )
                })
        })?;
    Ok(GhArchiveStarEvent {
        github_repo_id,
        repository,
        source_event_id,
        created_at,
    })
}

fn sort_events(events: &mut [GhArchiveStarEvent]) {
    events.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.github_repo_id.cmp(&b.github_repo_id))
            .then_with(|| {
                a.repository
                    .to_ascii_lowercase()
                    .cmp(&b.repository.to_ascii_lowercase())
            })
            .then_with(|| a.repository.cmp(&b.repository))
            .then_with(|| a.source_event_id.cmp(&b.source_event_id))
    });
}

fn cell_string(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        Value::Null => None,
        _ => None,
    }
}

fn update_u64_max(
    target: &mut u64,
    value: Option<&str>,
    field: &str,
) -> Result<(), GhArchiveError> {
    if let Some(value) = value {
        *target = (*target).max(parse_u64_field(value, field)?);
    }
    Ok(())
}

fn parse_u64_field(value: &str, field: &str) -> Result<u64, GhArchiveError> {
    value.parse::<u64>().map_err(|_| {
        GhArchiveError::InvalidResponse(format!("{field} was not an unsigned integer"))
    })
}

fn format_query_errors(errors: &[QueryError]) -> String {
    errors
        .iter()
        .take(3)
        .map(|error| match (&error.reason, &error.message) {
            (Some(reason), Some(message)) => format!("{reason}: {message}"),
            (Some(reason), None) => reason.clone(),
            (None, Some(message)) => message.clone(),
            (None, None) => "unknown BigQuery error".to_string(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn retry_delay(attempt: usize, retry_after: Option<u64>) -> Duration {
    if let Some(seconds) = retry_after {
        return Duration::from_secs(seconds.min(30));
    }
    let exponent = attempt.min(5) as u32;
    Duration::from_millis((200u64.saturating_mul(1u64 << exponent)).min(5_000))
}

fn api_error_message(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|value| value.pointer("/error/message"))
        .and_then(Value::as_str)
        .unwrap_or(body);
    let sanitized = message
        .chars()
        .filter(|character| !character.is_control() || *character == ' ')
        .take(1_024)
        .collect::<String>();
    if sanitized.is_empty() {
        "empty error response".to_string()
    } else {
        sanitized
    }
}

fn required_env(name: &'static str, value: Option<String>) -> Result<String, GhArchiveError> {
    nonempty(value).ok_or(GhArchiveError::MissingConfig(name))
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn parse_bool_env(
    name: &'static str,
    value: Option<&str>,
    default: bool,
) -> Result<bool, GhArchiveError> {
    match value.map(str::trim) {
        None | Some("") => Ok(default),
        Some(value) if value.eq_ignore_ascii_case("true") || value == "1" => Ok(true),
        Some(value) if value.eq_ignore_ascii_case("false") || value == "0" => Ok(false),
        Some(_) => Err(invalid_config(name, "expected true, false, 1, or 0")),
    }
}

fn parse_u64_env(
    name: &'static str,
    value: Option<String>,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, GhArchiveError> {
    let value = match nonempty(value) {
        Some(value) => value
            .parse::<u64>()
            .map_err(|_| invalid_config(name, "expected an unsigned integer"))?,
        None => default,
    };
    validate_range(name, value, min, max)?;
    Ok(value)
}

fn parse_u32_env(
    name: &'static str,
    value: Option<String>,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32, GhArchiveError> {
    let value = match nonempty(value) {
        Some(value) => value
            .parse::<u32>()
            .map_err(|_| invalid_config(name, "expected an unsigned integer"))?,
        None => default,
    };
    validate_range(name, value, min, max)?;
    Ok(value)
}

fn parse_usize_env(
    name: &'static str,
    value: Option<String>,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, GhArchiveError> {
    let value = match nonempty(value) {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| invalid_config(name, "expected an unsigned integer"))?,
        None => default,
    };
    validate_range(name, value, min, max)?;
    Ok(value)
}

fn parse_i64_env(
    name: &'static str,
    value: Option<String>,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, GhArchiveError> {
    let value = match nonempty(value) {
        Some(value) => value
            .parse::<i64>()
            .map_err(|_| invalid_config(name, "expected an integer"))?,
        None => default,
    };
    validate_range(name, value, min, max)?;
    Ok(value)
}

fn validate_range<T>(name: &'static str, value: T, min: T, max: T) -> Result<(), GhArchiveError>
where
    T: Copy + fmt::Display + PartialOrd,
{
    if value < min || value > max {
        return Err(invalid_config(
            name,
            format!("expected a value from {min} through {max}"),
        ));
    }
    Ok(())
}

fn validate_project_id(project_id: &str) -> Result<(), GhArchiveError> {
    let bytes = project_id.as_bytes();
    let valid = (6..=30).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
    if !valid {
        return Err(invalid_config(
            "GH_ARCHIVE_BIGQUERY_PROJECT",
            "expected a 6-30 character lowercase GCP project ID",
        ));
    }
    Ok(())
}

fn validate_location(location: &str) -> Result<(), GhArchiveError> {
    let valid = !location.is_empty()
        && location.len() <= 32
        && location
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if !valid {
        return Err(invalid_config(
            "GH_ARCHIVE_BIGQUERY_LOCATION",
            "expected an alphanumeric BigQuery location",
        ));
    }
    Ok(())
}

fn validate_credentials_json(credentials: &str) -> Result<(), GhArchiveError> {
    if credentials.len() > 128 * 1_024 {
        return Err(invalid_config(
            "GH_ARCHIVE_GOOGLE_CREDENTIALS_JSON",
            "credentials JSON is too large",
        ));
    }
    let value: Value = serde_json::from_str(credentials).map_err(|_| {
        invalid_config(
            "GH_ARCHIVE_GOOGLE_CREDENTIALS_JSON",
            "expected valid service-account JSON",
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        invalid_config(
            "GH_ARCHIVE_GOOGLE_CREDENTIALS_JSON",
            "expected a service-account JSON object",
        )
    })?;
    for key in ["client_email", "private_key", "token_uri"] {
        if object
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(invalid_config(
                "GH_ARCHIVE_GOOGLE_CREDENTIALS_JSON",
                format!("service-account JSON is missing {key}"),
            ));
        }
    }
    Ok(())
}

fn invalid_config(name: &'static str, reason: impl Into<String>) -> GhArchiveError {
    GhArchiveError::InvalidConfig {
        name,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(values: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            values
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| (*value).to_string()))
        }
    }

    fn config() -> GhArchiveConfig {
        let mut config = GhArchiveConfig::new("gitdebt-prod").unwrap();
        config.max_events = 25;
        config.page_size = 10;
        config.max_bytes_billed = 123_456_789;
        config.query_timeout = Duration::from_secs(45);
        config.poll_timeout = Duration::from_millis(2_500);
        config
    }

    #[test]
    fn disabled_config_needs_no_project_or_credentials() {
        assert!(GhArchiveConfig::from_lookup(env(&[])).unwrap().is_none());
        assert!(
            GhArchiveConfig::from_lookup(env(&[("GH_ARCHIVE_ENABLED", "false")]))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn enabled_config_requires_and_validates_project_and_limits() {
        let missing =
            GhArchiveConfig::from_lookup(env(&[("GH_ARCHIVE_ENABLED", "true")])).unwrap_err();
        assert!(matches!(
            missing,
            GhArchiveError::MissingConfig("GH_ARCHIVE_BIGQUERY_PROJECT")
        ));

        let config = GhArchiveConfig::from_lookup(env(&[
            ("GH_ARCHIVE_ENABLED", "1"),
            ("GH_ARCHIVE_BIGQUERY_PROJECT", "gitdebt-prod"),
            ("GH_ARCHIVE_MAX_EVENTS", "4321"),
            ("GH_ARCHIVE_CONCURRENCY", "3"),
            ("GH_ARCHIVE_MAX_RANGE_DAYS", "14"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(config.project_id, "gitdebt-prod");
        assert_eq!(config.max_events, 4_321);
        assert_eq!(config.concurrency, 3);
        assert_eq!(config.max_range_days, 14);
        assert!(config.credentials_json.is_none());

        let invalid = GhArchiveConfig::from_lookup(env(&[
            ("GH_ARCHIVE_ENABLED", "true"),
            ("GH_ARCHIVE_BIGQUERY_PROJECT", "Not/A/Project"),
        ]))
        .unwrap_err();
        assert!(matches!(
            invalid,
            GhArchiveError::InvalidConfig {
                name: "GH_ARCHIVE_BIGQUERY_PROJECT",
                ..
            }
        ));
    }

    #[test]
    fn inline_credentials_are_validated_and_redacted() {
        let credentials = r#"{
            "client_email":"worker@example.iam.gserviceaccount.com",
            "private_key":"not-a-real-key",
            "token_uri":"https://oauth2.googleapis.com/token"
        }"#;
        let config = GhArchiveConfig::from_lookup(env(&[
            ("GH_ARCHIVE_ENABLED", "true"),
            ("GH_ARCHIVE_BIGQUERY_PROJECT", "gitdebt-prod"),
            ("GH_ARCHIVE_GOOGLE_CREDENTIALS_JSON", credentials),
        ]))
        .unwrap()
        .unwrap();
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("not-a-real-key"));

        let invalid = GhArchiveConfig::from_lookup(env(&[
            ("GH_ARCHIVE_ENABLED", "true"),
            ("GH_ARCHIVE_BIGQUERY_PROJECT", "gitdebt-prod"),
            (
                "GH_ARCHIVE_GOOGLE_CREDENTIALS_JSON",
                r#"{"private_key":"secret"}"#,
            ),
        ]))
        .unwrap_err();
        assert!(!invalid.to_string().contains("secret"));
    }

    #[test]
    fn query_is_standard_parameterized_and_identity_only() {
        let repositories = normalize_repositories(&[
            RepositorySpec::new(None, "Owner/Example"),
            RepositorySpec::new(Some(42), "Zhom/GitDebt"),
        ])
        .unwrap();
        let request = build_query_request(
            &config(),
            &repositories,
            NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 1, 3).unwrap(),
        );
        let value = serde_json::to_value(request).unwrap();
        let sql = value["query"].as_str().unwrap();
        assert!(sql.contains("`githubarchive.month.202601`"));
        assert!(!sql.contains("githubarchive.day.*"));
        assert!(!sql.contains("_TABLE_SUFFIX"));
        assert!(sql.contains("@repositories"));
        assert!(sql.contains("repo.id"));
        assert!(sql.contains("LOWER(repo.name)"));
        assert!(sql.contains("type = 'WatchEvent'"));
        assert!(!sql.to_ascii_lowercase().contains("actor"));
        assert!(!sql.contains("owner/example"));
        assert_eq!(value["useLegacySql"], false);
        assert_eq!(value["parameterMode"], "NAMED");
        assert_eq!(value["maximumBytesBilled"], "123456789");
        assert_eq!(value["maxResults"], 10);
        assert_eq!(value["jobTimeoutMs"], "45000");
        assert_eq!(value["timeoutMs"], 2500);
        assert_eq!(
            value["queryParameters"][0]["parameterValue"]["value"],
            "2026-01-02"
        );
        assert_eq!(
            value["queryParameters"][1]["parameterValue"]["value"],
            "2026-01-03"
        );
        assert_eq!(value["queryParameters"][3]["parameterValue"]["value"], "26");
        let repository_values = &value["queryParameters"][2]["parameterValue"]["arrayValues"];
        assert!(
            repository_values[0]["structValues"]["github_id"]["value"].is_null(),
            "a missing stable ID must stay NULL so the name fallback is eligible"
        );
        assert_eq!(
            repository_values[0]["structValues"]["lower_name"]["value"],
            "owner/example"
        );
        assert_eq!(
            repository_values[1]["structValues"]["github_id"]["value"],
            "42"
        );
        assert_eq!(
            repository_values[1]["structValues"]["lower_name"]["value"],
            "zhom/gitdebt"
        );
    }

    #[test]
    fn query_unions_only_exact_months_in_the_requested_range() {
        let sql = build_star_events_sql(
            NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
        );
        assert!(sql.contains("`githubarchive.month.202512`"));
        assert!(sql.contains("`githubarchive.month.202601`"));
        assert!(sql.contains("`githubarchive.month.202602`"));
        assert_eq!(sql.matches("UNION ALL").count(), 2);
        assert!(!sql.contains("day.yesterday"));
    }

    #[test]
    fn urls_encode_opaque_job_and_page_values() {
        assert_eq!(
            query_url("gitdebt-prod").as_str(),
            "https://bigquery.googleapis.com/bigquery/v2/projects/gitdebt-prod/queries"
        );
        let job = JobReference {
            project_id: "gitdebt-prod".to_string(),
            job_id: "job/with spaces".to_string(),
            location: Some("US".to_string()),
        };
        let url = results_page_url(&config(), &job, Some("a/b+c ="));
        assert!(url.as_str().contains("job%2Fwith%20spaces"));
        assert!(url.as_str().contains("pageToken=a%2Fb%2Bc+%3D"));
        assert!(url.as_str().contains("maxResults=10"));
        assert!(url.as_str().contains("timeoutMs=2500"));
        assert!(url.as_str().contains("location=US"));
    }

    #[test]
    fn response_rows_parse_without_actor_data() {
        let page: QueryResultsPage = serde_json::from_value(json!({
            "jobComplete": true,
            "jobReference": {
                "projectId": "gitdebt-prod",
                "jobId": "job-1",
                "location": "US"
            },
            "totalRows": "2",
            "totalBytesProcessed": "98765",
            "pageToken": "next-page",
            "rows": [
                {"f": [
                    {"v": "42"},
                    {"v": "Zhom/GitDebt"},
                    {"v": "evt-42"},
                    {"v": "2026-01-02T03:04:05.123456Z"}
                ]},
                {"f": [
                    {"v": null},
                    {"v": "Owner/Legacy"},
                    {"v": null},
                    {"v": "2026-01-02T04:05:06.000000Z"}
                ]}
            ]
        }))
        .unwrap();
        assert!(page.job_complete);
        assert_eq!(page.total_rows.as_deref(), Some("2"));
        assert_eq!(page.page_token.as_deref(), Some("next-page"));
        let first = parse_event_row(&page.rows[0]).unwrap();
        assert_eq!(first.github_repo_id, Some(42));
        assert_eq!(first.repository, "Zhom/GitDebt");
        assert_eq!(first.source_event_id.as_deref(), Some("evt-42"));
        assert_eq!(
            first.created_at,
            "2026-01-02T03:04:05.123456Z"
                .parse::<DateTime<Utc>>()
                .unwrap()
        );
        let legacy = parse_event_row(&page.rows[1]).unwrap();
        assert_eq!(legacy.github_repo_id, None);
        assert_eq!(legacy.repository, "Owner/Legacy");
        assert_eq!(legacy.source_event_id, None);

        let mut reversed = vec![legacy, first.clone()];
        sort_events(&mut reversed);
        assert_eq!(reversed[0], first);
    }

    #[test]
    fn response_row_uses_repository_id_when_archive_name_is_bad() {
        let with_id: TableRow = serde_json::from_value(json!({
            "f": [
                {"v": "42"},
                {"v": null},
                {"v": "evt-42"},
                {"v": "2026-01-02T03:04:05.123456Z"}
            ]
        }))
        .unwrap();
        let parsed = parse_event_row(&with_id).unwrap();
        assert_eq!(parsed.github_repo_id, Some(42));
        assert!(parsed.repository.is_empty());

        let without_id: TableRow = serde_json::from_value(json!({
            "f": [
                {"v": null},
                {"v": "not-a-slug"},
                {"v": "evt-legacy"},
                {"v": "2026-01-02T03:04:05.123456Z"}
            ]
        }))
        .unwrap();
        assert!(parse_event_row(&without_id).is_err());
    }

    #[test]
    fn repository_normalization_is_deterministic_and_rejects_injection() {
        let normalized = normalize_repositories(&[
            RepositorySpec::new(Some(2), "B/Repo"),
            RepositorySpec::new(Some(1), "A/Repo"),
            RepositorySpec::new(Some(1), "a/repo"),
        ])
        .unwrap();
        assert_eq!(
            normalized,
            vec![
                NormalizedRepository {
                    github_id: Some(1),
                    lower_name: "a/repo".to_string(),
                },
                NormalizedRepository {
                    github_id: Some(2),
                    lower_name: "b/repo".to_string(),
                },
            ]
        );
        assert!(
            normalize_repositories(&[RepositorySpec::new(Some(1), "owner/repo` WHERE TRUE --")])
                .is_err()
        );
        assert!(normalize_repositories(&[RepositorySpec::new(Some(0), "a/b")]).is_err());
    }

    #[test]
    fn retry_policy_is_bounded_to_429_and_server_errors() {
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
        assert_eq!(retry_delay(0, None), Duration::from_millis(200));
        assert_eq!(retry_delay(99, None), Duration::from_secs(5));
        assert_eq!(retry_delay(0, Some(90)), Duration::from_secs(30));
    }
}
