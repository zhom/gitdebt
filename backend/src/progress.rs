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
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures::stream;
use serde::Serialize;
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
    Fetching,
    Backfilling,
    Analyzing,
    Complete,
    NotFound,
    Restricted,
    Failed,
}

impl ProgressPhase {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Fetching | Self::Backfilling | Self::Analyzing
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WorkProgress {
    phase: ProgressPhase,
    complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProgressSnapshot {
    repo: String,
    phase: ProgressPhase,
    terminal: bool,
    stars: WorkProgress,
    analysis: WorkProgress,
}

#[derive(Debug, Clone, Default)]
struct RawProgress {
    missing: bool,
    stars_complete: bool,
    star_status: Option<String>,
    star_partial: bool,
    star_next_page: Option<i64>,
    star_last_error: Option<String>,
    analysis_status: Option<String>,
    analysis_complete: bool,
}

impl ProgressSnapshot {
    fn from_raw(repo: String, raw: RawProgress) -> Self {
        if raw.missing {
            let missing = WorkProgress {
                phase: ProgressPhase::NotFound,
                complete: false,
                next_page: None,
            };
            return Self {
                repo,
                phase: ProgressPhase::NotFound,
                terminal: true,
                stars: missing.clone(),
                analysis: missing,
            };
        }

        let star_phase = match raw.star_status.as_deref() {
            Some("pending" | "in_progress") if raw.star_partial => ProgressPhase::Backfilling,
            Some("pending") => ProgressPhase::Pending,
            Some("in_progress") => ProgressPhase::Fetching,
            Some("dead")
                if raw
                    .star_last_error
                    .as_deref()
                    .is_some_and(queue::is_restricted_error) =>
            {
                ProgressPhase::Restricted
            }
            Some("dead") | Some(_) => ProgressPhase::Failed,
            None if raw.stars_complete => ProgressPhase::Complete,
            None => ProgressPhase::Idle,
        };
        let analysis_phase = match raw.analysis_status.as_deref() {
            Some("pending") => ProgressPhase::Pending,
            Some("in_progress") => ProgressPhase::Analyzing,
            Some("dead") | Some(_) => ProgressPhase::Failed,
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
        } else if star_phase == ProgressPhase::Pending || analysis_phase == ProgressPhase::Pending {
            ProgressPhase::Pending
        } else if star_phase == ProgressPhase::Restricted {
            ProgressPhase::Restricted
        } else if star_phase == ProgressPhase::Failed || analysis_phase == ProgressPhase::Failed {
            ProgressPhase::Failed
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
        Self {
            repo,
            phase,
            terminal: !phase.active(),
            stars: WorkProgress {
                phase: star_phase,
                complete: raw.stars_complete,
                next_page,
            },
            analysis: WorkProgress {
                phase: analysis_phase,
                complete: raw.analysis_complete,
                next_page: None,
            },
        }
    }
}

type ProgressRow = (
    bool,
    bool,
    Option<String>,
    Option<bool>,
    Option<i64>,
    Option<String>,
    Option<String>,
    bool,
);

async fn load_snapshot(state: &ApiState, repo: &str) -> Result<ProgressSnapshot, ApiError> {
    // One round-trip and one row: all joined columns are primary-key
    // lookups. Error strings are read solely for restricted classification
    // and never copied into the public payload.
    let row: ProgressRow = sqlx::query_as(
        "SELECT \
            COALESCE(r.missing, FALSE), \
            COALESCE(r.history_complete, FALSE), \
            stars.status, stars.partial, stars.next_page, stars.last_error, \
            analysis.status, \
            EXISTS(SELECT 1 FROM repo_history history \
                   WHERE history.repo = $1 AND history.last_analyzed_at IS NOT NULL) \
         FROM (SELECT $1::TEXT AS repo) requested \
         LEFT JOIN repos r ON r.repo = requested.repo \
         LEFT JOIN star_fetch_queue stars ON stars.repo = requested.repo \
         LEFT JOIN repo_analysis_queue analysis ON analysis.repo = requested.repo",
    )
    .bind(repo)
    .fetch_one(&state.analyzer.cache.db().pool)
    .await?;
    Ok(ProgressSnapshot::from_raw(
        repo.to_string(),
        RawProgress {
            missing: row.0,
            stars_complete: row.1,
            star_status: row.2,
            star_partial: row.3.unwrap_or(false),
            star_next_page: row.4,
            star_last_error: row.5,
            analysis_status: row.6,
            analysis_complete: row.7,
        },
    ))
}

async fn load_snapshot_bounded(state: &ApiState, repo: &str) -> Result<ProgressSnapshot, ApiError> {
    tokio::time::timeout(SNAPSHOT_TIMEOUT, load_snapshot(state, repo))
        .await
        .map_err(|_| ApiError::unavailable("progress snapshot timed out"))?
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
    fn restricted_marker_is_public_phase_but_error_detail_is_private() {
        let detail = "restricted: upstream account-specific detail";
        let value = snapshot(RawProgress {
            star_status: Some("dead".into()),
            star_last_error: Some(detail.into()),
            ..RawProgress::default()
        });
        assert_eq!(value.phase, ProgressPhase::Restricted);
        assert!(value.terminal);
        let json = serde_json::to_string(&value).unwrap();
        assert!(!json.contains(detail));
        assert!(!json.contains("upstream"));
    }

    #[test]
    fn generic_dead_job_is_failed_and_terminal() {
        let value = snapshot(RawProgress {
            analysis_status: Some("dead".into()),
            ..RawProgress::default()
        });
        assert_eq!(value.phase, ProgressPhase::Failed);
        assert_eq!(value.analysis.phase, ProgressPhase::Failed);
        assert!(value.terminal);
    }

    #[test]
    fn either_independent_pipeline_can_complete_the_stream() {
        let stars = snapshot(RawProgress {
            stars_complete: true,
            ..RawProgress::default()
        });
        assert_eq!(stars.phase, ProgressPhase::Complete);
        assert!(stars.terminal);

        let analysis = snapshot(RawProgress {
            analysis_complete: true,
            ..RawProgress::default()
        });
        assert_eq!(analysis.phase, ProgressPhase::Complete);
        assert!(analysis.terminal);
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
