use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::OnceLock;

use axum::extract::ConnectInfo;

use moka::future::Cache as MokaCache;
use tower_governor::{
    GovernorLayer,
    errors::GovernorError,
    governor::GovernorConfigBuilder,
    key_extractor::{KeyExtractor, SmartIpKeyExtractor},
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::aggregate;
use crate::analyzer::{AnalyzerCtx, analyze_repo, star_series};
use crate::auth::GithubAppConfig;
use crate::badge::{BadgeInput, BadgeStyle, Metric};
use crate::cards;
use crate::chart::{
    ChartConfig, ChartOpts, OverlayConfig, Point, TimeAxis, render_multi_svg, render_overlay_svg,
    render_svg,
};
use crate::export::{self, RangeSpec};
use crate::og::{self, CompareEntry, RepoCard};
use crate::repo_endpoints::is_valid_slug;
use crate::theme::theme_for;
use crate::usage::{self, Resolved, UsageDownloads, UsageOverrides};

#[derive(Clone)]
pub struct ApiState {
    pub analyzer: AnalyzerCtx,
    /// Hot in-memory cache of rendered SVGs, keyed by `owner/repo|theme`.
    /// 24h TTL matches star-history's CDN strategy: bytes-deterministic
    /// SVGs collapse repeated embed traffic into one origin render per day.
    pub svg_cache: MokaCache<String, String>,
    /// Pre-serialized JSON bodies for the analyze endpoint, keyed by
    /// `owner/repo`. Short TTL (5 min) so newly-fetched stargazers show
    /// up promptly; long enough to absorb SSR storms on viral pages.
    /// The underlying analyze pass loads ~tens of MB of cached rows for
    /// large repos (3.5M starred entries on donutbrowser-class repos),
    /// so caching the serialized JSON saves real wall-time per hit.
    pub analyze_cache: MokaCache<String, String>,
    /// Hot in-memory cache of rendered repo-history stat SVGs (bug
    /// magnets, top files, heatmap, contributors, todo trend, lines).
    /// Keyed by endpoint name + `owner/repo` + relevant query params
    /// (theme/year/since). 24h TTL matches the `Cache-Control: s-maxage`
    /// on the responses, so origin-cache misses fall through to a
    /// microsecond-scale memory hit instead of a Postgres query.
    pub stat_svg_cache: MokaCache<String, String>,
    /// Rasterized PNG/WebP variants of the SVG charts. Same 24h TTL
    /// since the source SVG is bytes-deterministic — when a chart's
    /// SVG changes (e.g. queue drained, new data), the cache key flips
    /// naturally because the analyze JSON is also re-derived. Keys:
    /// `{endpoint}:{owner}/{repo}|{theme}|{format}[|extra]`.
    pub raster_cache: MokaCache<String, std::sync::Arc<Vec<u8>>>,
    /// Built user aggregates (`aggregate::build` results), keyed by
    /// lowercased login. 5-min TTL like the analyze bodies. This is the
    /// single expensive step behind BOTH `/api/users/:login/analyze` and
    /// the user chart endpoints — memoizing it here means the chart
    /// endpoints' many distinct render keys (theme × axis × `from`/`to`
    /// range) share ONE aggregate build (and one batch of enqueue probes)
    /// per login per TTL, instead of re-running the multi-repo GROUP BY
    /// for every unique query-param combination.
    pub user_agg_cache: MokaCache<String, std::sync::Arc<aggregate::UserAggregate>>,
    /// Serialized leaderboard JSON bodies, 5-min TTL. Deliberately
    /// SEPARATE from `analyze_cache`: the leaderboard key space is
    /// param-derived (metric × per × page ≈ 40k combinations), so sharing
    /// the 500-entry analyze cache would let a param-churning client evict
    /// every warm `/analyze` body (cache pollution).
    pub leaderboard_cache: MokaCache<String, String>,
    /// GitHub App config (OAuth client_id/secret, webhook secret, session
    /// secret, token encryption key). `None` if env isn't set; auth +
    /// webhook routes return 503.
    pub gh_app: Option<GithubAppConfig>,
    /// Origin allowed to make credentialed (cookie-bearing) requests to
    /// `/api/me` and `/auth/*`. Defaults to local dev frontend.
    pub frontend_origin: String,
    pub metrics_token: Option<String>,
    /// Bare-clone storage. Shared with the repo-analysis pool; the usage
    /// endpoint reuses it to read package manifests out of existing clones
    /// (never clones itself — falls back to the repo-name heuristic when a
    /// clone is absent).
    pub storage: std::sync::Arc<crate::repo_history::RepoStorage>,
}

impl ApiState {
    pub fn new(
        analyzer: AnalyzerCtx,
        gh_app: Option<GithubAppConfig>,
        storage: std::sync::Arc<crate::repo_history::RepoStorage>,
    ) -> anyhow::Result<Self> {
        let svg_cache = MokaCache::builder()
            .max_capacity(2_000)
            .time_to_live(Duration::from_secs(24 * 60 * 60))
            .build();
        let analyze_cache = MokaCache::builder()
            .max_capacity(500)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();
        let stat_svg_cache = MokaCache::builder()
            .max_capacity(2_000)
            .time_to_live(Duration::from_secs(24 * 60 * 60))
            .build();
        // Raster cache: smaller capacity than the SVG caches because
        // PNGs are ~10–100× larger per entry. Keep it tight; raster
        // bytes are deterministic so re-rasterization on miss is fine.
        let raster_cache = MokaCache::builder()
            .max_capacity(1_000)
            .time_to_live(Duration::from_secs(24 * 60 * 60))
            .build();
        let user_agg_cache = MokaCache::builder()
            .max_capacity(500)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();
        let leaderboard_cache = MokaCache::builder()
            .max_capacity(1_000)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();
        let frontend_origin_raw = match std::env::var("PUBLIC_FRONTEND_ORIGIN") {
            Ok(value) if !value.trim().is_empty() => value,
            _ if cfg!(debug_assertions) => "http://localhost:14321".to_string(),
            _ => anyhow::bail!("PUBLIC_FRONTEND_ORIGIN must be set in release deployments"),
        };
        let frontend_origin = normalize_frontend_origin(&frontend_origin_raw)?;
        let metrics_token = match std::env::var("METRICS_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty())
        {
            Some(token) => Some(token),
            None if cfg!(debug_assertions) => {
                tracing::warn!("METRICS_TOKEN unset; /metrics is public in debug builds");
                None
            }
            None => anyhow::bail!("METRICS_TOKEN must be set in release deployments"),
        };
        Ok(Self {
            analyzer,
            svg_cache,
            analyze_cache,
            stat_svg_cache,
            raster_cache,
            user_agg_cache,
            leaderboard_cache,
            gh_app,
            frontend_origin,
            metrics_token,
            storage,
        })
    }
}

fn normalize_frontend_origin(raw: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(raw)
        .map_err(|e| anyhow::anyhow!("PUBLIC_FRONTEND_ORIGIN is invalid: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        anyhow::bail!("PUBLIC_FRONTEND_ORIGIN must be an http(s) origin without a path");
    }
    Ok(parsed.origin().ascii_serialization())
}

/// Max body size for the webhook receiver (`POST /webhooks/github`). The
/// handler buffers the body into `Bytes` before it can verify the HMAC, so
/// the cap is the only thing standing between an attacker and an unbounded
/// allocation. GitHub's own payloads are comfortably under this.
const WEBHOOK_BODY_LIMIT: usize = 64 * 1024;

/// Max body size for the extension ping (`POST /api/ext/ping`). The body is
/// a tiny `{owner,repo,stars}` JSON object.
const EXT_BODY_LIMIT: usize = 16 * 1024;

pub fn router(state: ApiState) -> Router {
    // Public responses are credential-free and embeddable from any origin.
    // CloudflareIpKeyExtractor only honors forwarded IPs from trusted peers.
    let public_cors = CorsLayer::new()
        .allow_methods([Method::GET])
        .allow_origin(Any)
        .max_age(Duration::from_secs(60 * 60));

    // Cold analyze requests can enqueue GitHub work; isolate them from image
    // traffic so scrapers cannot consume the shared API budget.
    let analyze_governor = std::sync::Arc::new(
        GovernorConfigBuilder::default()
            .per_second(2)
            .burst_size(20)
            .key_extractor(CloudflareIpKeyExtractor)
            .finish()
            .expect("analyze governor config builds"),
    );
    let analyze = Router::new()
        .route("/api/repos/{owner}/{repo}/analyze", get(analyze))
        .route("/api/repos/{owner}/{repo}/stars.csv", get(stars_csv))
        .route("/api/repos/{owner}/{repo}/stars.json", get(stars_json))
        .route("/api/users/{login}/analyze", get(user_analyze))
        .route("/api/leaderboard.json", get(leaderboard_json))
        .route("/api/activity.json", get(platform_activity))
        .route("/api/sitemap/repos", get(sitemap_repos))
        .layer(GovernorLayer::new(analyze_governor.clone()));

    // SSE is deliberately outside the global 60-second request timeout.
    // The handler owns a five-minute lifetime, heartbeats, and a process-wide
    // connection cap; it shares the analyze admission budget because it
    // polls the same ingestion state.
    let progress = Router::new()
        .route(
            "/api/repos/{owner}/{repo}/progress",
            get(crate::progress::repo_progress),
        )
        .layer(GovernorLayer::new(analyze_governor))
        .layer(public_cors.clone());

    // Render parameters create an unbounded cache-key space, so even
    // edge-cached images need an origin-side per-IP ceiling.
    let images_governor = std::sync::Arc::new(
        GovernorConfigBuilder::default()
            .per_second(10)
            .burst_size(60)
            .key_extractor(CloudflareIpKeyExtractor)
            .finish()
            .expect("images governor config builds"),
    );
    let images = Router::new()
        .route("/api/repos/{owner}/{repo}/chart.svg", get(chart))
        .route("/api/repos/{owner}/{repo}/chart.png", get(chart_png))
        .route("/api/repos/{owner}/{repo}/chart.webp", get(chart_webp))
        .route("/api/repos/{owner}/{repo}/chart.gif", get(chart_gif))
        .route("/api/chart.svg", get(multi_chart))
        .route("/api/chart.png", get(multi_chart_png))
        .route("/api/chart.webp", get(multi_chart_webp))
        .route("/api/users/{login}/chart.svg", get(user_chart))
        .route("/api/users/{login}/chart.png", get(user_chart_png))
        .route("/api/users/{login}/chart.webp", get(user_chart_webp))
        .route("/api/repos/{owner}/{repo}/usage", get(usage_json))
        .route("/api/repos/{owner}/{repo}/usage.svg", get(usage_svg))
        .route("/api/repos/{owner}/{repo}/usage.png", get(usage_png))
        .route("/api/repos/{owner}/{repo}/usage.webp", get(usage_webp))
        .route("/api/repos/{owner}/{repo}/badge.svg", get(badge_svg))
        .route("/api/repos/{owner}/{repo}/badge.png", get(badge_png))
        .route("/api/repos/{owner}/{repo}/badge.webp", get(badge_webp))
        .route("/api/users/{login}/card.svg", get(user_card_svg))
        .route("/api/users/{login}/card.png", get(user_card_png))
        .route("/api/users/{login}/card.webp", get(user_card_webp))
        .route("/api/repos/{owner}/{repo}/card.svg", get(repo_card_svg))
        .route("/api/repos/{owner}/{repo}/card.png", get(repo_card_png))
        .route("/api/repos/{owner}/{repo}/card.webp", get(repo_card_webp))
        .route("/api/repos/{owner}/{repo}/og.png", get(repo_og_png))
        .route("/api/repos/{owner}/{repo}/og.webp", get(repo_og_webp))
        .route("/api/og.png", get(site_og_png))
        .route("/api/og.webp", get(site_og_webp))
        .merge(crate::repo_endpoints::public_router())
        .layer(GovernorLayer::new(images_governor));

    // Extension origins vary by browser/install. The endpoint accepts no
    // credentials and has its own limiter because it can enqueue work.
    let ext_cors = CorsLayer::new()
        .allow_methods([Method::POST])
        .allow_origin(Any)
        .allow_headers(Any)
        .max_age(Duration::from_secs(60 * 60));
    let ext_governor = std::sync::Arc::new(
        GovernorConfigBuilder::default()
            .per_second(1)
            .burst_size(10)
            .key_extractor(CloudflareIpKeyExtractor)
            .finish()
            .expect("ext governor config builds"),
    );
    let ext = Router::new()
        .route("/api/ext/ping", axum::routing::post(ext_ping))
        .layer(GovernorLayer::new(ext_governor))
        .layer(ext_cors)
        .layer(RequestBodyLimitLayer::new(EXT_BODY_LIMIT));

    let public = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/metrics", get(metrics))
        .merge(images)
        .merge(analyze)
        .layer(public_cors);

    // Cookie-bearing routes use one validated origin, never wildcard CORS.
    let frontend_origin: HeaderValue = state
        .frontend_origin
        .parse()
        .expect("PUBLIC_FRONTEND_ORIGIN must be a valid origin URL");
    let credentialed_cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_origin(frontend_origin)
        .allow_credentials(true);
    let credentialed = Router::new()
        .merge(crate::auth::router())
        .layer(credentialed_cors);

    let webhook = crate::webhook::router().layer(RequestBodyLimitLayer::new(WEBHOOK_BODY_LIMIT));

    // Repo-history analysis is disk/CPU intensive despite queue deduplication.
    let governor_conf = std::sync::Arc::new(
        GovernorConfigBuilder::default()
            .per_second(1)
            .burst_size(5)
            .key_extractor(CloudflareIpKeyExtractor)
            .finish()
            .expect("governor config builds"),
    );
    let rate_limited = Router::new()
        .merge(crate::repo_endpoints::mutating_router())
        .layer(GovernorLayer::new(governor_conf))
        .layer(
            CorsLayer::new()
                .allow_methods([Method::POST])
                .allow_origin(
                    state
                        .frontend_origin
                        .parse::<HeaderValue>()
                        .expect("PUBLIC_FRONTEND_ORIGIN must be a valid origin URL"),
                )
                .allow_credentials(true),
        );

    let timed = Router::new()
        .merge(public)
        .merge(ext)
        .merge(credentialed)
        .merge(webhook)
        .merge(rate_limited)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(60),
        ));

    Router::new()
        .merge(timed)
        .merge(progress)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Debug, Serialize)]
struct PipelineSignals {
    histories_complete: i64,
    histories_pending: i64,
    star_jobs_active: i64,
    star_jobs_retrying: i64,
    star_jobs_provider_delayed: i64,
    star_jobs_dead: i64,
    star_jobs_tombstoned: i64,
    analysis_jobs_active: i64,
    analysis_jobs_retrying: i64,
    analysis_jobs_dead: i64,
    analysis_jobs_tombstoned: i64,
    oldest_star_job_seconds: i64,
    oldest_analysis_job_seconds: i64,
    last_archive_hour: Option<DateTime<Utc>>,
}

impl PipelineSignals {
    fn degraded(&self) -> bool {
        self.star_jobs_provider_delayed > 0
            || self.star_jobs_dead > 0
            || self.analysis_jobs_dead > 0
    }
}

async fn load_pipeline_signals(db: &crate::db::Db) -> Result<PipelineSignals, sqlx::Error> {
    let row = sqlx::query(
        "SELECT \
            (SELECT COUNT(*)::BIGINT FROM repos WHERE history_complete = TRUE AND missing = FALSE) \
                AS histories_complete, \
            (SELECT COUNT(*)::BIGINT FROM repos WHERE history_complete = FALSE AND missing = FALSE) \
                AS histories_pending, \
            (SELECT COUNT(*)::BIGINT FROM star_fetch_queue \
                WHERE status IN ('pending', 'in_progress')) AS star_jobs_active, \
            (SELECT COUNT(*)::BIGINT FROM star_fetch_queue \
                WHERE status IN ('pending', 'in_progress') \
                  AND (attempts > 0 OR last_error LIKE 'provider:%')) AS star_jobs_retrying, \
            (SELECT COUNT(*)::BIGINT FROM star_fetch_queue \
                WHERE status IN ('pending', 'in_progress') \
                  AND last_error LIKE 'provider:%') AS star_jobs_provider_delayed, \
            (SELECT COUNT(*)::BIGINT FROM star_fetch_queue queue \
                WHERE status = 'dead' AND NOT EXISTS ( \
                    SELECT 1 FROM repos \
                    WHERE repos.repo = queue.repo AND repos.missing = TRUE)) \
                AS star_jobs_dead, \
            (SELECT COUNT(*)::BIGINT FROM star_fetch_queue queue \
                WHERE status = 'dead' AND EXISTS ( \
                    SELECT 1 FROM repos \
                    WHERE repos.repo = queue.repo AND repos.missing = TRUE)) \
                AS star_jobs_tombstoned, \
            (SELECT COUNT(*)::BIGINT FROM repo_analysis_queue \
                WHERE status IN ('pending', 'in_progress')) AS analysis_jobs_active, \
            (SELECT COUNT(*)::BIGINT FROM repo_analysis_queue \
                WHERE status IN ('pending', 'in_progress') AND attempts > 0) \
                AS analysis_jobs_retrying, \
            (SELECT COUNT(*)::BIGINT FROM repo_analysis_queue queue \
                WHERE status = 'dead' AND NOT EXISTS ( \
                    SELECT 1 FROM repos \
                    WHERE repos.repo = queue.repo AND repos.missing = TRUE)) \
                AS analysis_jobs_dead, \
            (SELECT COUNT(*)::BIGINT FROM repo_analysis_queue queue \
                WHERE status = 'dead' AND EXISTS ( \
                    SELECT 1 FROM repos \
                    WHERE repos.repo = queue.repo AND repos.missing = TRUE)) \
                AS analysis_jobs_tombstoned, \
            COALESCE((SELECT EXTRACT(EPOCH FROM (NOW() - MIN(enqueued_at)))::BIGINT \
                FROM star_fetch_queue WHERE status IN ('pending', 'in_progress')), 0) \
                AS oldest_star_job_seconds, \
            COALESCE((SELECT EXTRACT(EPOCH FROM (NOW() - MIN(enqueued_at)))::BIGINT \
                FROM repo_analysis_queue WHERE status IN ('pending', 'in_progress')), 0) \
                AS oldest_analysis_job_seconds, \
            (SELECT MAX(archive_hour) FROM gh_archive_hours WHERE status = 'complete') \
                AS last_archive_hour",
    )
    .fetch_one(&db.pool)
    .await?;
    Ok(PipelineSignals {
        histories_complete: row.try_get("histories_complete")?,
        histories_pending: row.try_get("histories_pending")?,
        star_jobs_active: row.try_get("star_jobs_active")?,
        star_jobs_retrying: row.try_get("star_jobs_retrying")?,
        star_jobs_provider_delayed: row.try_get("star_jobs_provider_delayed")?,
        star_jobs_dead: row.try_get("star_jobs_dead")?,
        star_jobs_tombstoned: row.try_get("star_jobs_tombstoned")?,
        analysis_jobs_active: row.try_get("analysis_jobs_active")?,
        analysis_jobs_retrying: row.try_get("analysis_jobs_retrying")?,
        analysis_jobs_dead: row.try_get("analysis_jobs_dead")?,
        analysis_jobs_tombstoned: row.try_get("analysis_jobs_tombstoned")?,
        oldest_star_job_seconds: row.try_get::<i64, _>("oldest_star_job_seconds")?.max(0),
        oldest_analysis_job_seconds: row.try_get::<i64, _>("oldest_analysis_job_seconds")?.max(0),
        last_archive_hour: row.try_get("last_archive_hour")?,
    })
}

/// Readiness probe. Unlike `/health` (liveness — "the process is up"),
/// `/ready` verifies the dependency the server can't function without: the
/// Postgres pool. It also reports pipeline degradation without taking the
/// read API offline: queued/retrying states must remain visible while a
/// provider recovers. 503 is reserved for a database/schema failure.
async fn ready(State(state): State<ApiState>) -> impl IntoResponse {
    let db = state.analyzer.cache.db();
    let no_store = {
        let mut h = HeaderMap::new();
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        h
    };
    match load_pipeline_signals(db).await {
        Ok(pipeline) => {
            let degraded = pipeline.degraded();
            (
                StatusCode::OK,
                no_store,
                Json(serde_json::json!({
                    "ready": true,
                    "degraded": degraded,
                    "pipeline": pipeline,
                })),
            )
        }
        Err(e) => {
            tracing::error!(error = %e, "readiness check failed: database/schema unavailable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                no_store,
                Json(serde_json::json!({ "ready": false, "error": "database unavailable" })),
            )
        }
    }
}

/// Operational metrics (JSON). The key signal is GitHub rate-budget
/// exhaustion plus queue depth. Cheap: a handful of aggregate queries. When
/// Requires `Authorization: Bearer <METRICS_TOKEN>` in release deployments.
async fn metrics(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    // Optional bearer-token gate.
    if let Some(expected) = state.metrics_token.as_deref() {
        let provided = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "));
        if !provided.is_some_and(|token| constant_time_eq(token.as_bytes(), expected.as_bytes())) {
            return Err(ApiError::unauthorized("metrics unauthorized"));
        }
    }

    let db = state.analyzer.cache.db();

    // Per-source GitHub rate budget (read straight from the persisted
    // `api_quota` table — same numbers the tracker maintains).
    let quota_rows = sqlx::query(
        "SELECT source, remaining, limit_total, reset_at FROM api_quota ORDER BY source",
    )
    .fetch_all(&db.pool)
    .await?;
    let github_budget: Vec<serde_json::Value> = quota_rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "source": r.try_get::<String, _>("source").unwrap_or_default(),
                "remaining": r.try_get::<i64, _>("remaining").unwrap_or(0),
                "limit_total": r.try_get::<i64, _>("limit_total").unwrap_or(0),
                "reset_at": r.try_get::<i64, _>("reset_at").unwrap_or(0),
            })
        })
        .collect();

    // Star-fetch queue depth by status.
    let star_queue = queue_depth_by_status(db, "star_fetch_queue").await?;
    // Repo-analysis queue depth by status.
    let analysis_queue = queue_depth_by_status(db, "repo_analysis_queue").await?;
    let pipeline = load_pipeline_signals(db).await?;

    // Raster saturation: how many of the CPU-bound raster permits are free.
    // `available == 0` for a sustained stretch means chart PNG/WebP encodes
    // are queueing on the semaphore (the CPU is the bottleneck).
    let raster_available = RASTER_PERMITS.available_permits();
    let (progress_total, progress_available) = crate::progress::connection_metrics();

    // In-memory cache occupancy (approximate; moka counts lazily). Rising
    // toward `max_capacity` on a param-derived cache means a cache-busting
    // client is churning distinct keys and evicting warm entries.
    let cache_entries = serde_json::json!({
        "svg": state.svg_cache.entry_count(),
        "analyze": state.analyze_cache.entry_count(),
        "stat_svg": state.stat_svg_cache.entry_count(),
        "raster": state.raster_cache.entry_count(),
        "user_agg": state.user_agg_cache.entry_count(),
        "leaderboard": state.leaderboard_cache.entry_count(),
    });

    let body = serde_json::json!({
        "github_budget": github_budget,
        "star_fetch_queue": star_queue,
        "repo_analysis_queue": analysis_queue,
        "pipeline": pipeline,
        "raster": {
            "permits_total": RASTER_CONCURRENCY,
            "permits_available": raster_available,
        },
        "progress_streams": {
            "connections_limit": progress_total,
            "connections_active": progress_total.saturating_sub(progress_available),
        },
        "cache_entries": cache_entries,
    });
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((h, Json(body)))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

/// Count rows in a queue table grouped by status, returned as
/// `{ pending, in_progress, dead }` (zero-filled). The table name is a
/// fixed literal at every call site — never user input — so the interpolation
/// is safe.
async fn queue_depth_by_status(
    db: &crate::db::Db,
    table: &str,
) -> Result<serde_json::Value, ApiError> {
    let rows = match table {
        "star_fetch_queue" => {
            sqlx::query("SELECT status, COUNT(*) AS n FROM star_fetch_queue GROUP BY status")
                .fetch_all(&db.pool)
                .await?
        }
        "repo_analysis_queue" => {
            sqlx::query("SELECT status, COUNT(*) AS n FROM repo_analysis_queue GROUP BY status")
                .fetch_all(&db.pool)
                .await?
        }
        _ => return Err(ApiError::from(anyhow::anyhow!("unknown queue table"))),
    };
    let mut pending = 0i64;
    let mut in_progress = 0i64;
    let mut dead = 0i64;
    let mut other = 0i64;
    for r in rows {
        let status: String = r.try_get("status").unwrap_or_default();
        let n: i64 = r.try_get("n").unwrap_or(0);
        match status.as_str() {
            "pending" => pending = n,
            "in_progress" => in_progress = n,
            "dead" => dead = n,
            _ => other += n,
        }
    }
    Ok(serde_json::json!({
        "pending": pending,
        "in_progress": in_progress,
        "dead": dead,
        "other": other,
    }))
}

async fn analyze(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(query): Query<AnalyzeQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    // Lowercase the memo key so `Owner/Repo` and `owner/repo` share ONE
    // cached body — the analyzer normalizes the same way for the cache /
    // queue underneath, so a split key here would just double the work.
    let owner = owner.to_ascii_lowercase();
    let repo = repo.to_ascii_lowercase();
    let key = format!("{owner}/{repo}");
    let enqueue = query.enqueue != Some(0);
    let (json, live) = if !enqueue {
        let result = crate::analyzer::analyze_repo_readonly(&owner, &repo, &state.analyzer).await?;
        let live = result.pending || result.backfilling;
        (serde_json::to_string(&result)?, live)
    } else if let Some(json) = state.analyze_cache.get(&key).await {
        (json, false)
    } else {
        let result = analyze_repo(&owner, &repo, &state.analyzer).await?;
        let live = result.pending || result.backfilling;
        let json = serde_json::to_string(&result)?;
        if !live {
            state.analyze_cache.insert(key, json.clone()).await;
        }
        (json, live)
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, analyze_cache_control(live));
    Ok((headers, json))
}

#[derive(Debug, Default, Deserialize)]
struct AnalyzeQuery {
    enqueue: Option<u8>,
}

fn analyze_cache_control(live: bool) -> HeaderValue {
    if live {
        HeaderValue::from_static("no-store")
    } else {
        HeaderValue::from_static("public, s-maxage=300, max-age=60")
    }
}

// Star exports

/// Query params for the export endpoints: an optional inclusive
/// `[from, to]` date window (`YYYY-MM-DD`) plus `rebase=1` to rebase the
/// cumulative totals to the window start.
#[derive(Debug, Default, Clone, Deserialize)]
struct ExportQuery {
    from: Option<String>,
    to: Option<String>,
    rebase: Option<String>,
}

impl ExportQuery {
    fn spec(&self) -> Result<RangeSpec, ApiError> {
        parse_range_spec(
            self.from.as_deref(),
            self.to.as_deref(),
            self.rebase.as_deref(),
        )
    }
}

/// Shared `from`/`to`/`rebase` parsing for the export + chart endpoints.
/// Invalid dates or `from > to` → 400 with the (generic) validation text.
fn parse_range_spec(
    from: Option<&str>,
    to: Option<&str>,
    rebase: Option<&str>,
) -> Result<RangeSpec, ApiError> {
    let range = export::DateRange::parse(from, to).map_err(ApiError::bad_request)?;
    let rebase = matches!(rebase, Some("1") | Some("true"));
    Ok(RangeSpec { range, rebase })
}

/// Build the export payload for a repo: per-day (date, total, delta)
/// rows aggregated **in SQL** — never one row per stargazer in memory —
/// then window-filtered. Honors the cache invariants: the series is
/// only populated when `stargazers_complete` is set (readers never
/// trust partial data), and a tombstoned (404) repo is a plain 404.
/// Reads Postgres only — never touches GitHub.
async fn build_star_export(
    state: &ApiState,
    owner: &str,
    repo: &str,
    spec: &RangeSpec,
) -> Result<export::StarExport, ApiError> {
    let repo_full = format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    );
    let cache = &state.analyzer.cache;
    let summary = cache.get_repo_summary(&repo_full).await?;

    if summary.as_ref().is_some_and(|s| s.missing) {
        return Err(ApiError::not_found("repo not found"));
    }
    let complete = summary.as_ref().is_some_and(|s| s.stargazers_complete);
    if !complete {
        // No trustworthy history yet: empty series, best-effort headline
        // total from the denormalized metadata count (0 when truly cold).
        let total_stars = summary
            .as_ref()
            .and_then(|s| s.star_count)
            .filter(|n| *n >= 0)
            .map(|n| n as u64)
            .unwrap_or(0);
        return Ok(export::StarExport {
            repo: repo_full,
            total_stars,
            complete: false,
            history_kind: "unavailable".to_string(),
            approximate: false,
            series: Vec::new(),
        });
    }

    let deltas = export::load_day_deltas(cache.db(), &repo_full).await?;
    let full = export::accumulate(&deltas);
    // Full total, NOT window-filtered (matches /analyze semantics).
    let archive_activity = summary
        .as_ref()
        .is_some_and(|value| value.history_source.as_deref() == Some("gh_archive"));
    let total_stars = summary
        .as_ref()
        .and_then(|value| value.star_count)
        .filter(|value| *value >= 0)
        .map(|value| value as u64)
        .unwrap_or_else(|| full.last().map(|row| row.total).unwrap_or(0));
    let series = export::filter_day_stats(&full, spec);
    Ok(export::StarExport {
        repo: repo_full,
        total_stars,
        complete: true,
        history_kind: if archive_activity {
            "public_star_actions"
        } else {
            "current_stargazers"
        }
        .to_string(),
        approximate: archive_activity,
        series,
    })
}

/// Cache headers shared by the export endpoints — same policy as
/// `/analyze` (5 min edge, 1 min browser) so freshly-fetched history
/// shows up promptly while SSR/scrape storms stay flat.
fn export_response_headers(content_type: &'static str, history_kind: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, s-maxage=300, max-age=60"),
    );
    if let Ok(value) = HeaderValue::from_str(history_kind) {
        headers.insert("x-gitdebt-history-kind", value);
    }
    headers
}

/// `GET /api/repos/:owner/:repo/stars.csv` — header `date,total,delta`,
/// one row per day (full granularity — no downsampling).
async fn stars_csv(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ExportQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let spec = q.spec()?;
    let body = build_star_export(&state, &owner, &repo, &spec).await?;
    let headers = export_response_headers("text/csv; charset=utf-8", &body.history_kind);
    Ok((headers, export::to_csv(&body.series)))
}

/// `GET /api/repos/:owner/:repo/stars.json` —
/// `{repo,total_stars,complete,series:[{date,total,delta}]}`.
async fn stars_json(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ExportQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let spec = q.spec()?;
    let body = build_star_export(&state, &owner, &repo, &spec).await?;
    let headers = export_response_headers("application/json", &body.history_kind);
    Ok((headers, Json(body)))
}

// User aggregates

/// Map the aggregate module's error taxonomy onto HTTP statuses. 404 for a
/// GitHub-confirmed missing login; 503 (generic body — 5xx never echoes
/// internals) when there's neither a cached repo list nor budget headroom
/// to fetch one; everything else is a 500.
fn map_aggregate_err(e: aggregate::AggregateError) -> ApiError {
    match e {
        aggregate::AggregateError::LoginNotFound => ApiError::not_found("user not found"),
        aggregate::AggregateError::Busy => ApiError::unavailable("temporarily unavailable"),
        aggregate::AggregateError::Other(err) => err.into(),
    }
}

/// `GET /api/users/:login/analyze` —
/// `{login,repos_included,repos_pending,total_stars,history:[{date,stars}]}`,
/// summing the cumulative star series across the login's top public repos.
/// Non-blocking on star data: uncached repos are enqueued on the existing
/// star-fetch queue and counted in `repos_pending`. Memoized like
/// `/analyze` (5 min in-process; the `user:` key can't collide with the
/// repo keys, which always contain a slash) with the same cache headers.
async fn user_analyze(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(query): Query<AnalyzeQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !aggregate::is_valid_login(&login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let json = if query.enqueue == Some(0) {
        let agg = aggregate::build_readonly(&state.analyzer, &login)
            .await
            .map_err(map_aggregate_err)?;
        serde_json::to_string(&agg.to_json())?
    } else {
        let key = format!("user:{}", login.to_ascii_lowercase());
        single_flight(&state.analyze_cache, key, async {
            let agg = build_user_aggregate(&state, &login).await?;
            Ok(serde_json::to_string(&agg.to_json())?)
        })
        .await?
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, s-maxage=300, max-age=60"),
    );
    Ok((headers, json))
}

/// Build-or-fetch the login's aggregate through `user_agg_cache` (5 min).
/// Every user endpoint funnels through here so the expensive step — the
/// multi-repo day-delta GROUP BY plus the enqueue probes — runs at most
/// once per login per TTL no matter how many distinct chart render keys
/// (theme/axis/`from`/`to`) are requested. Errors are never memoized:
/// `LoginNotFound` re-reads a cheap tombstone row and `Busy` should retry.
async fn build_user_aggregate(
    state: &ApiState,
    login: &str,
) -> Result<std::sync::Arc<aggregate::UserAggregate>, ApiError> {
    let key = login.to_ascii_lowercase();
    // Single-flight: a celebrity login (many concurrent chart render keys —
    // theme × axis × from/to) coalesces onto ONE aggregate build + enqueue
    // batch. `try_get_with` never memoizes the error arm, so `LoginNotFound`
    // re-reads its cheap tombstone and `Busy` retries — the pre-existing
    // "errors are never memoized" contract is preserved.
    state
        .user_agg_cache
        .try_get_with(key.clone(), async {
            aggregate::build(&state.analyzer, &key)
                .await
                .map(std::sync::Arc::new)
                .map_err(map_aggregate_err)
        })
        .await
        .map_err(|e| e.clone_shared())
}

/// Render-or-fetch the aggregate star chart for a login: the summed series
/// as ONE line through the same renderer, params (`theme`/`type`/`log`),
/// and `from`/`to`/`rebase` range filters as the single-repo chart.
/// Memoized in `svg_cache` under `user:`-prefixed keys — EXCEPT the
/// pending/empty aggregate, which for a login is the common cold state by
/// construction (all top repos still on the star-fetch queue). Those
/// renders come back `short_ttl` and are never inserted into the 24h
/// caches, the same self-healing policy as the pending stat cards, so a
/// first view can't pin a blank chart for a day.
async fn ensure_user_chart_svg(
    state: &ApiState,
    login: &str,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
) -> Result<RenderedCard, ApiError> {
    if !aggregate::is_valid_login(login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let spec = q.range_spec()?;
    let login = login.to_ascii_lowercase();
    let theme_key = if theme.dark { "dark" } else { "light" };
    let key = format!("user:{login}|{theme_key}|{}|{}", q.opts_key(), spec.key());
    if let Some(cached) = state.svg_cache.get(&key).await {
        return Ok(RenderedCard {
            svg: cached,
            short_ttl: false,
        });
    }
    let agg = build_user_aggregate(state, &login).await?;
    let series = export::filter_points(&agg.series, &spec);
    let pending = agg.repos_included == 0 || series.is_empty();
    let svg = render_svg(
        &series,
        &ChartConfig {
            repo: login,
            ..ChartConfig::default()
        },
        theme,
        &q.opts(),
    );
    if pending {
        // Aggregate still filling (or the window is empty): short TTL,
        // no 24h memo — the `user_agg_cache` above already absorbs the
        // expensive part of a re-render.
        return Ok(RenderedCard {
            svg,
            short_ttl: true,
        });
    }
    state.svg_cache.insert(key, svg.clone()).await;
    Ok(RenderedCard {
        svg,
        short_ttl: false,
    })
}

/// Two-level memoization like the repo chart rasters: the SVG cache
/// absorbs the aggregate build, the raster cache the per-format encode.
/// Pending/empty charts skip the raster cache too (`short_ttl`), same as
/// the card rasters.
async fn ensure_user_chart_raster(
    state: &ApiState,
    login: &str,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    if !aggregate::is_valid_login(login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let spec = q.range_spec()?;
    let theme_key = if theme.dark { "dark" } else { "light" };
    let fmt_key = raster_fmt_key(format);
    let key = format!(
        "user:{}|{theme_key}|{}|{}|{fmt_key}",
        login.to_ascii_lowercase(),
        q.opts_key(),
        spec.key()
    );
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    let card = ensure_user_chart_svg(state, login, theme, q).await?;
    if card.short_ttl {
        return Ok((rasterize_uncached(card.svg, format).await?, true));
    }
    Ok((
        rasterize_cached(state, &key, card.svg, format).await?,
        false,
    ))
}

async fn user_chart(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_user_chart_svg(&state, &login, theme, &q).await?;
    Ok(card_svg_response(card))
}

async fn user_chart_png(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_chart_raster(&state, &login, theme, &q, crate::raster::RasterFormat::Png)
            .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn user_chart_webp(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_chart_raster(&state, &login, theme, &q, crate::raster::RasterFormat::Webp)
            .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

// Browser extension

/// Request body for `POST /api/ext/ping`. `stars` is the client-observed
/// star count scraped from the GitHub page — an **untrusted hint** used
/// only to decide whether the cached set looks stale enough to refresh. It
/// is NEVER persisted as a star count.
///
/// `stars` is `Option`, not a defaulted `i64`: when the extension can't
/// read the count off the DOM it OMITS the field, and an omitted count
/// must fall back to age-based freshness only. Coercing a missing field to
/// `0` (the old `#[serde(default)]` behavior) would read as maximum drift
/// and force-refetch every fresh repo — exactly what omitting it avoids.
#[derive(Debug, Deserialize)]
struct PingBody {
    owner: String,
    repo: String,
    stars: Option<i64>,
}

/// Cache age past which a complete repo is considered stale and worth an
/// incremental refresh ping. Matches `analyzer::STARGAZER_REFRESH_TTL`.
const PING_STALE_TTL: chrono::Duration = chrono::Duration::hours(6);

/// True when the client-reported star count diverges from the cached
/// count by more than `max(50, 2% of cached)` — the "stale by count"
/// signal. Pure so it's the single source of truth for both the `stale`
/// flag and the enqueue decision.
fn ping_count_drifted(cached_stars: i64, reported_stars: i64) -> bool {
    let drift = (reported_stars - cached_stars).unsigned_abs();
    let threshold = std::cmp::max(50, cached_stars.unsigned_abs() * 2 / 100);
    drift > threshold
}

/// Decide whether a ping should enqueue a fetch, given the cache state.
/// Pure so the threshold is unit-testable. `cached_stars` is `None` when
/// the repo has no complete cached set; `reported_stars` is `None` when the
/// client couldn't read the count and omitted it.
///
///   * unknown (no complete cache) → always enqueue (cold).
///   * known but cache older than the TTL → enqueue (stale by age).
///   * known and fresh, but the count drifted past the threshold →
///     enqueue (stale by count).
///   * known, fresh, and no count reported → do NOT enqueue (an omitted
///     count relies on age-based freshness only, never a coerced 0=drift).
fn ping_should_enqueue(
    cached_stars: Option<i64>,
    reported_stars: Option<i64>,
    fresh: bool,
) -> bool {
    let Some(cached) = cached_stars else {
        return true; // unknown → cold fetch
    };
    if !fresh {
        return true; // stale by age
    }
    // Fresh: only a real, drifted count justifies a refetch. A missing
    // count (None) means "unknown" — not "0" — so we skip count-based
    // enqueue entirely.
    reported_stars.is_some_and(|r| ping_count_drifted(cached, r))
}

async fn ext_ping(
    State(state): State<ApiState>,
    Json(body): Json<PingBody>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&body.owner) || !is_valid_slug(&body.repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let repo_full = format!(
        "{}/{}",
        body.owner.to_ascii_lowercase(),
        body.repo.to_ascii_lowercase()
    );
    let cache = &state.analyzer.cache;

    // Tombstone short-circuit: a repo GitHub already 404'd is never
    // re-enqueued (budget drain). Report it plainly and return — don't even
    // record a view (a dead repo shouldn't accrue priority).
    if cache.repo_is_missing(&repo_full).await.unwrap_or(false) {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return Ok((
            headers,
            Json(serde_json::json!({
                "ok": true,
                "known": false,
                "stale": false,
                "enqueued": false,
                "not_found": true,
            })),
        ));
    }

    let known = cache
        .repo_stargazers_complete(&repo_full)
        .await
        .unwrap_or(false);
    let fresh = cache
        .repo_stargazers_fresh_within(&repo_full, PING_STALE_TTL)
        .await
        .unwrap_or(false);
    let cached_stars = if known {
        cache.get_repo_star_count(&repo_full).await.ok().flatten()
    } else {
        None
    };
    // `stale` here means "we have it but it's worth refreshing" — by age
    // or by the count drifting past the threshold. Unknown is reported via
    // `known: false`, not `stale`.
    let count_drifted = match (cached_stars, body.stars) {
        (Some(c), Some(r)) => ping_count_drifted(c, r),
        // No reported count → no count-based drift (age-only freshness).
        _ => false,
    };
    let stale = known && (!fresh || count_drifted);

    let enqueued = ping_should_enqueue(cached_stars, body.stars, fresh);
    if enqueued {
        // The worker decides full-vs-incremental from the cache state; we
        // just enqueue (idempotent dedup in the queue). The client's
        // reported `stars` is NEVER written.
        crate::analyzer::enqueue_fetch(&state.analyzer, &repo_full).await;
    }

    // Popularity bump, fully off the latency path: spawn it so a slow
    // write never delays the (tiny) ping response.
    let cache_for_view = cache.clone();
    let repo_for_view = repo_full.clone();
    tokio::spawn(async move {
        if let Err(e) = cache_for_view.record_repo_view(&repo_for_view).await {
            tracing::debug!(repo = %repo_for_view, error = %e, "record_repo_view failed");
        }
    });

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    // Don't cache a POST response.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((
        headers,
        Json(serde_json::json!({
            "ok": true,
            "known": known,
            "stale": stale,
            "enqueued": enqueued,
        })),
    ))
}

/// Query params for the chart endpoints. `theme=light|dark` picks the
/// baked palette; `type=date|timeline` picks the x-axis alignment;
/// `log=1` log-scales the y-axis. `repos` is the comma-separated slug
/// list for the multi-repo overlay endpoints. `from`/`to`
/// (`YYYY-MM-DD`, inclusive) window the series pre-render — the left
/// edge keeps the true running total unless `rebase=1` rebases it to
/// the window start. `animate=1` is an explicit SVG-only on-site reveal;
/// the default is static. `motion=draw` opts into the separate GIF route.
#[derive(Debug, Default, Clone, Deserialize)]
struct ChartQuery {
    theme: Option<String>,
    #[serde(rename = "type")]
    type_: Option<String>,
    log: Option<String>,
    repos: Option<String>,
    from: Option<String>,
    to: Option<String>,
    rebase: Option<String>,
    animate: Option<String>,
    motion: Option<String>,
}

impl ChartQuery {
    fn opts(&self) -> ChartOpts {
        ChartOpts {
            axis: TimeAxis::parse(self.type_.as_deref()),
            log_y: matches!(self.log.as_deref(), Some("1") | Some("true")),
            animate: flag_on(self.animate.as_deref()),
        }
    }
    /// Parsed + validated date-range spec. 400 on invalid dates or
    /// `from > to`.
    fn range_spec(&self) -> Result<RangeSpec, ApiError> {
        parse_range_spec(
            self.from.as_deref(),
            self.to.as_deref(),
            self.rebase.as_deref(),
        )
    }
    /// Stable key fragment for series geometry. GIFs use this fragment so
    /// the SVG-only `animate` option cannot alter their output/cache key.
    fn series_opts_key(&self) -> String {
        let axis = match TimeAxis::parse(self.type_.as_deref()) {
            TimeAxis::Date => "date",
            TimeAxis::Timeline => "timeline",
        };
        let log = if self.opts().log_y { "log" } else { "lin" };
        format!("{axis}:{log}")
    }
    /// SVG/raster cache fragment, including the explicit animation choice.
    fn opts_key(&self) -> String {
        let animate = if self.opts().animate {
            "anim"
        } else {
            "static"
        };
        format!("{}:{animate}", self.series_opts_key())
    }
    fn gif_motion(&self) -> Result<&'static str, ApiError> {
        match self.motion.as_deref() {
            Some(value) if value.eq_ignore_ascii_case(crate::animated_gif::MOTION_PRESET) => {
                Ok(crate::animated_gif::MOTION_PRESET)
            }
            _ => Err(ApiError::bad_request(
                "chart.gif requires motion=draw; SVG is the static default",
            )),
        }
    }
}

async fn chart(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_chart_svg(&state, &owner, &repo, theme, &q).await?;
    Ok(card_svg_response(card))
}

async fn chart_png(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_chart_raster(
        &state,
        &owner,
        &repo,
        theme,
        &q,
        crate::raster::RasterFormat::Png,
    )
    .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn chart_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_chart_raster(
        &state,
        &owner,
        &repo,
        theme,
        &q,
        crate::raster::RasterFormat::Webp,
    )
    .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

/// Actual README animation. Unlike the SVG route, this is opt-in via
/// `motion=draw`, reads only complete cached Postgres stargazer timestamps,
/// and plays once before resting on the complete chart.
async fn chart_gif(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    q.gif_motion()?;
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_chart_gif(&state, &owner, &repo, theme, &q).await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/gif"));
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    let filename = format!(
        "inline; filename=\"{}-{}-chart-{}.gif\"",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase(),
        if theme.dark { "dark" } else { "light" }
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&filename)
            .map_err(|_| ApiError::bad_request("invalid owner/repo"))?,
    );
    Ok((headers, (*bytes).clone()))
}

async fn ensure_chart_gif(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    q.gif_motion()?;
    let spec = q.range_spec()?;
    let owner = owner.to_ascii_lowercase();
    let repo = repo.to_ascii_lowercase();
    let repo_full = format!("{owner}/{repo}");
    let summary = state.analyzer.cache.get_repo_summary(&repo_full).await?;
    if summary.as_ref().is_some_and(|s| s.missing) {
        return Err(ApiError::not_found("repo not found"));
    }

    // Deliberately bypass `analyzer::star_series`: that helper may enqueue
    // acquisition. Animated embeds are a pure cached-data read and never
    // spend GitHub budget.
    let arrivals = state.analyzer.cache.get_repo_stargazers(&repo_full).await?;
    let complete = arrivals.is_some();
    let full = arrivals
        .as_deref()
        .map(crate::chart::cumulative_series)
        .unwrap_or_default();
    let series = export::filter_points(&full, &spec);
    let revision = crate::animated_gif::data_revision(&series);
    let theme_key = if theme.dark { "dark" } else { "light" };
    let key = format!(
        "chart-gif:{repo_full}|{theme_key}|{}|{}|motion:{}|rev:{revision}",
        q.series_opts_key(),
        spec.key(),
        crate::animated_gif::MOTION_PRESET,
    );
    if complete && let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }

    let cfg = ChartConfig {
        repo: repo_full,
        metric_label: if summary
            .as_ref()
            .is_some_and(|value| value.history_source.as_deref() == Some("gh_archive"))
        {
            "public star actions"
        } else {
            "stars"
        }
        .to_string(),
        ..ChartConfig::default()
    };
    let mut opts = q.opts();
    opts.animate = false;
    let theme = *theme;
    let encoded = tokio::task::spawn_blocking(move || {
        crate::animated_gif::encode_draw(&series, &cfg, &theme, &opts)
    })
    .await
    .map_err(|e| ApiError::from(anyhow::anyhow!("GIF render task failed: {e}")))?
    .map_err(ApiError::from)?;
    debug_assert_eq!(
        encoded.frame_count,
        crate::animated_gif::FRAME_COUNT,
        "encoder contract"
    );
    debug_assert!(encoded.width > 0 && encoded.height > 0);
    let bytes = std::sync::Arc::new(encoded.bytes);
    if complete {
        state.raster_cache.insert(key, bytes.clone()).await;
    }
    Ok((bytes, !complete))
}

/// Render-or-fetch the single-repo star-history SVG. Memoized in
/// `svg_cache` keyed by repo + theme + axis/log + date-range so the
/// raster handlers don't have to re-walk the analyze pipeline.
async fn ensure_chart_svg(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
) -> Result<RenderedCard, ApiError> {
    if !is_valid_slug(owner) || !is_valid_slug(repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let spec = q.range_spec()?;
    let theme_key = if theme.dark { "dark" } else { "light" };
    // Lowercase the slug in both the memo key and the rendered label so
    // `Owner/Repo` and `owner/repo` share ONE render (and one series load)
    // and produce identical bytes.
    let owner = owner.to_ascii_lowercase();
    let repo = repo.to_ascii_lowercase();
    let repo_full = format!("{owner}/{repo}");
    let summary = state.analyzer.cache.get_repo_summary(&repo_full).await?;
    let archive_activity = summary
        .as_ref()
        .is_some_and(|value| value.history_source.as_deref() == Some("gh_archive"));
    let source_key = if archive_activity {
        "archive"
    } else {
        "github"
    };
    let key = format!(
        "{repo_full}|{theme_key}|{source_key}|{}|{}",
        q.opts_key(),
        spec.key()
    );
    single_flight_card(&state.svg_cache, key, async {
        let series = star_series(&owner, &repo, &state.analyzer)
            .await
            .map_err(ApiError::from)?;
        let series = export::filter_points(&series, &spec);
        // Cold / just-enqueued repo (empty series): render the placeholder
        // but serve it short-TTL and never pin it in the 24h svg cache, so a
        // first view can't lock "no data" at origin + CDN for a day.
        let empty = series.is_empty();
        let svg = render_svg(
            &series,
            &ChartConfig {
                repo: repo_full,
                metric_label: if archive_activity {
                    "public star actions"
                } else {
                    "stars"
                }
                .to_string(),
                ..ChartConfig::default()
            },
            theme,
            &q.opts(),
        );
        if empty {
            return Err(RenderMiss::Pending(svg));
        }
        Ok(svg)
    })
    .await
}

/// Render-or-fetch the rasterized single-repo chart. Two-level
/// memoization: the SVG cache absorbs the analyze cost, the raster cache
/// absorbs the per-format encode cost. WebP / PNG entries are independent.
/// Returns `(bytes, short_ttl)`: a cold/empty chart rasterizes uncached and
/// serves short-TTL, matching the SVG path — never pinned for 24h.
async fn ensure_chart_raster(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let spec = q.range_spec()?;
    let theme_key = if theme.dark { "dark" } else { "light" };
    let fmt_key = raster_fmt_key(format);
    let key = format!(
        "chart:{}/{}|{theme_key}|{}|{}|{fmt_key}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase(),
        q.opts_key(),
        spec.key()
    );
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    let card = ensure_chart_svg(state, owner, repo, theme, q).await?;
    if card.short_ttl {
        return Ok((rasterize_uncached(card.svg, format).await?, true));
    }
    Ok((
        rasterize_cached(state, &key, card.svg, format).await?,
        false,
    ))
}

// Multi-repo overlay

/// Max repos accepted in a single overlay request. Keeps the fan-out
/// bounded (each repo is a separate analyze pass) and matches the
/// categorical palette length doubling — beyond ~12 lines the chart is
/// unreadable anyway.
const MAX_OVERLAY_REPOS: usize = 12;

async fn multi_chart(
    State(state): State<ApiState>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_multi_svg(&state, theme, &q).await?;
    Ok(card_svg_response(card))
}

async fn multi_chart_png(
    State(state): State<ApiState>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_multi_raster(&state, theme, &q, crate::raster::RasterFormat::Png).await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn multi_chart_webp(
    State(state): State<ApiState>,
    Query(q): Query<ChartQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_multi_raster(&state, theme, &q, crate::raster::RasterFormat::Webp).await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

/// Parse, validate, dedup, and cap the `?repos=` slug list. Returns the
/// normalized `(owner, repo)` pairs in input order plus the canonical
/// `owner/repo` slugs (lowercased). Rejects the request if any slug is
/// malformed or the list is empty.
fn parse_overlay_repos(repos: Option<&str>) -> Result<Vec<(String, String)>, ApiError> {
    let raw = repos
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("missing repos= query param"))?;
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for slug in raw.split(',') {
        let slug = slug.trim();
        if slug.is_empty() {
            continue;
        }
        let Some((owner, repo)) = slug.split_once('/') else {
            return Err(ApiError::bad_request(format!("invalid repo slug: {slug}")));
        };
        if !crate::repo_endpoints::is_valid_slug(owner)
            || !crate::repo_endpoints::is_valid_slug(repo)
        {
            return Err(ApiError::bad_request(format!("invalid repo slug: {slug}")));
        }
        let owner = owner.to_ascii_lowercase();
        let repo = repo.to_ascii_lowercase();
        if seen.insert(format!("{owner}/{repo}")) {
            out.push((owner, repo));
            if out.len() >= MAX_OVERLAY_REPOS {
                break;
            }
        }
    }
    if out.is_empty() {
        return Err(ApiError::bad_request("no valid repos in repos= param"));
    }
    Ok(out)
}

/// Stable cache key for an overlay request: the normalized slug set +
/// theme + axis/log + date-range. The slug order is preserved (it
/// drives colors), so reordering produces a distinct, correct key.
fn overlay_key(
    pairs: &[(String, String)],
    theme: &crate::theme::Theme,
    q: &ChartQuery,
    spec: &RangeSpec,
) -> String {
    let slugs: Vec<String> = pairs.iter().map(|(o, r)| format!("{o}/{r}")).collect();
    let theme_key = if theme.dark { "dark" } else { "light" };
    format!(
        "multi:{}|{theme_key}|{}|{}",
        slugs.join(","),
        q.opts_key(),
        spec.key()
    )
}

async fn ensure_multi_svg(
    state: &ApiState,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
) -> Result<RenderedCard, ApiError> {
    let spec = q.range_spec()?;
    let pairs = parse_overlay_repos(q.repos.as_deref())?;
    let key = overlay_key(&pairs, theme, q, &spec);
    single_flight_card(&state.svg_cache, key, async {
        // Build each repo's full series via the same pipeline as the single
        // chart. Done sequentially: the per-repo stargazer fetch is itself
        // internally parallel, and overlay traffic is low-volume vs. the
        // single-repo embed path.
        let mut series_per_repo: Vec<(String, Vec<Point>)> = Vec::with_capacity(pairs.len());
        for (owner, repo) in &pairs {
            let series = star_series(owner, repo, &state.analyzer)
                .await
                .map_err(ApiError::from)?;
            let series = export::filter_points(&series, &spec);
            series_per_repo.push((format!("{owner}/{repo}"), series));
        }
        // All repos cold (no cached history yet): short-TTL placeholder,
        // never pinned in the 24h cache.
        let empty = series_per_repo.iter().all(|(_, s)| s.is_empty());
        let svg = render_multi_svg(&series_per_repo, &ChartConfig::default(), theme, &q.opts());
        if empty {
            return Err(RenderMiss::Pending(svg));
        }
        Ok(svg)
    })
    .await
}

async fn ensure_multi_raster(
    state: &ApiState,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let spec = q.range_spec()?;
    let pairs = parse_overlay_repos(q.repos.as_deref())?;
    let fmt_key = raster_fmt_key(format);
    let key = format!("{}|{fmt_key}", overlay_key(&pairs, theme, q, &spec));
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    let card = ensure_multi_svg(state, theme, q).await?;
    if card.short_ttl {
        return Ok((rasterize_uncached(card.svg, format).await?, true));
    }
    Ok((
        rasterize_cached(state, &key, card.svg, format).await?,
        false,
    ))
}

fn raster_fmt_key(format: crate::raster::RasterFormat) -> &'static str {
    match format {
        crate::raster::RasterFormat::Png => "png",
        crate::raster::RasterFormat::Webp => "webp",
    }
}

// Stars and package usage

/// Query params for the usage endpoints. Explicit registry overrides
/// (`npm`, `crate`, `pypi`, `docker`) short-circuit package resolution.
/// `theme`/`type`/`log` drive the overlay SVG; `source` picks which
/// resolved package's downloads back the right axis (`auto` = longest
/// history). `from`/`to`/`rebase` window both series pre-render (same
/// semantics as the star charts).
#[derive(Debug, Default, Clone, Deserialize)]
struct UsageQuery {
    theme: Option<String>,
    #[serde(rename = "type")]
    type_: Option<String>,
    log: Option<String>,
    source: Option<String>,
    npm: Option<String>,
    #[serde(rename = "crate")]
    crate_: Option<String>,
    pypi: Option<String>,
    docker: Option<String>,
    from: Option<String>,
    to: Option<String>,
    rebase: Option<String>,
}

impl UsageQuery {
    fn opts(&self) -> ChartOpts {
        ChartOpts {
            axis: TimeAxis::parse(self.type_.as_deref()),
            log_y: matches!(self.log.as_deref(), Some("1") | Some("true")),
            animate: false,
        }
    }
    /// Parsed + validated date-range spec. 400 on invalid dates or
    /// `from > to`.
    fn range_spec(&self) -> Result<RangeSpec, ApiError> {
        parse_range_spec(
            self.from.as_deref(),
            self.to.as_deref(),
            self.rebase.as_deref(),
        )
    }
    fn opts_key(&self) -> String {
        let axis = match TimeAxis::parse(self.type_.as_deref()) {
            TimeAxis::Date => "date",
            TimeAxis::Timeline => "timeline",
        };
        let log = if self.opts().log_y { "log" } else { "lin" };
        format!("{axis}:{log}")
    }
    fn overrides(&self) -> UsageOverrides {
        UsageOverrides {
            npm: self.npm.clone(),
            crate_: self.crate_.clone(),
            pypi: self.pypi.clone(),
            docker: self.docker.clone(),
        }
    }
    /// Cache-key fragment for the override set (so two requests with
    /// different explicit packages don't collide).
    fn overrides_key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.npm.as_deref().unwrap_or("-"),
            self.crate_.as_deref().unwrap_or("-"),
            self.pypi.as_deref().unwrap_or("-"),
            self.docker.as_deref().unwrap_or("-"),
        )
    }
}

/// Resolved bundle reused by both the JSON and SVG usage handlers: the
/// repo's authoritative star/fork counts, the resolved package ids, and the
/// per-source download stats.
struct UsageBundle {
    repo_full: String,
    stars_total: u64,
    forks: i64,
    resolved: Resolved,
    downloads: UsageDownloads,
}

/// Resolve packages + fetch all download sources for a repo. Star/fork
/// counts come from the cache (best-effort — `0` until the metadata refresh
/// lands). Never errors on a missing source.
async fn build_usage(
    state: &ApiState,
    owner: &str,
    repo: &str,
    q: &UsageQuery,
) -> Result<UsageBundle, ApiError> {
    let owner = owner.to_ascii_lowercase();
    let repo = repo.to_ascii_lowercase();
    let repo_full = format!("{owner}/{repo}");

    let resolved = usage::resolve_packages(&owner, &repo, &q.overrides(), &state.storage).await;
    let downloads = usage::fetch_all(&state.analyzer.cache, &resolved).await;

    // Authoritative counts (best-effort from cache; the analyze path
    // refreshes them out-of-band).
    let stars_total = state
        .analyzer
        .cache
        .get_repo_stargazers(&repo_full)
        .await
        .ok()
        .flatten()
        .map(|v| v.len() as u64)
        .unwrap_or(0);
    let forks = state
        .analyzer
        .cache
        .get_repo_forks(&repo_full)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);

    Ok(UsageBundle {
        repo_full,
        stars_total,
        forks,
        resolved,
        downloads,
    })
}

async fn usage_json(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<UsageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    // Validate range params for parity with the chart variants (the JSON
    // body itself is not windowed — it carries totals, not a rendered
    // series), so a bad `from=` fails loudly instead of being ignored.
    q.range_spec()?;
    let bundle = build_usage(&state, &owner, &repo, &q).await?;
    let body = serde_json::json!({
        "repo": bundle.repo_full,
        "stars_total": bundle.stars_total,
        "forks": bundle.forks,
        "resolved": bundle.resolved,
        "downloads": bundle.downloads,
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, s-maxage=86400, max-age=3600"),
    );
    Ok((headers, Json(body)))
}

/// Pick which resolved source backs the downloads axis. `source=auto`
/// chooses the resolved package with the longest series (Docker has no
/// series, so it's only picked when it's the only thing with data and an
/// explicit `source=docker` — auto prefers a time series). Returns the
/// chosen `(label, &DownloadStats)` or `None` for stars-only.
fn pick_download_source<'a>(
    downloads: &'a UsageDownloads,
    source: Option<&str>,
) -> Option<(String, &'a usage::DownloadStats)> {
    let want = source.map(|s| s.trim().to_ascii_lowercase());
    let candidates: [(&str, &str, Option<&'a usage::DownloadStats>); 4] = [
        ("npm", "npm downloads", downloads.npm.as_ref()),
        ("crates", "crates downloads", downloads.crates.as_ref()),
        ("pypi", "PyPI downloads", downloads.pypi.as_ref()),
        ("docker", "docker pulls", downloads.docker.as_ref()),
    ];
    match want.as_deref() {
        Some(name) if name != "auto" => candidates
            .iter()
            .find(|(key, _, stats)| *key == name && stats.is_some())
            .map(|(_, label, stats)| (label.to_string(), stats.unwrap())),
        // auto: prefer the source with the longest daily series.
        _ => candidates
            .iter()
            .filter_map(|(_, label, stats)| stats.map(|s| (label.to_string(), s)))
            .max_by_key(|(_, s)| s.series.len())
            .filter(|(_, s)| !s.series.is_empty()),
    }
}

async fn ensure_usage_svg(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &UsageQuery,
) -> Result<RenderedCard, ApiError> {
    if !is_valid_slug(owner) || !is_valid_slug(repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let spec = q.range_spec()?;
    let theme_key = if theme.dark { "dark" } else { "light" };
    let source_key = q.source.as_deref().unwrap_or("auto");
    let key = format!(
        "usage:{}/{}|{theme_key}|{}|src:{source_key}|{}|{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase(),
        q.opts_key(),
        q.overrides_key(),
        spec.key(),
    );
    single_flight_card(&state.svg_cache, key, async {
        let bundle = build_usage(state, owner, repo, q).await?;
        let stars = star_series(owner, repo, &state.analyzer)
            .await
            .unwrap_or_default();
        let stars = export::filter_points(&stars, &spec);

        let (label, cum) = match pick_download_source(&bundle.downloads, q.source.as_deref()) {
            Some((label, stats)) => (
                Some(label),
                export::filter_downloads(&usage::cumulative_downloads(stats), &spec),
            ),
            None => (None, Vec::new()),
        };

        // Cold repo (no cached star series yet): render the placeholder but
        // serve it short-TTL — never pin an empty overlay for 24h.
        let empty = stars.is_empty();
        let svg = render_overlay_svg(
            &stars,
            &cum,
            &ChartConfig::default(),
            &OverlayConfig {
                repo: bundle.repo_full,
                downloads_label: label,
            },
            theme,
            &q.opts(),
        );
        if empty {
            return Err(RenderMiss::Pending(svg));
        }
        Ok(svg)
    })
    .await
}

async fn usage_svg(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<UsageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_usage_svg(&state, &owner, &repo, theme, &q).await?;
    Ok(card_svg_response(card))
}

async fn ensure_usage_raster(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &UsageQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let spec = q.range_spec()?;
    let theme_key = if theme.dark { "dark" } else { "light" };
    let fmt_key = raster_fmt_key(format);
    let source_key = q.source.as_deref().unwrap_or("auto");
    let key = format!(
        "usage:{}/{}|{theme_key}|{}|src:{source_key}|{}|{}|{fmt_key}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase(),
        q.opts_key(),
        q.overrides_key(),
        spec.key(),
    );
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    let card = ensure_usage_svg(state, owner, repo, theme, q).await?;
    if card.short_ttl {
        return Ok((rasterize_uncached(card.svg, format).await?, true));
    }
    Ok((
        rasterize_cached(state, &key, card.svg, format).await?,
        false,
    ))
}

async fn usage_png(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<UsageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_usage_raster(
        &state,
        &owner,
        &repo,
        theme,
        &q,
        crate::raster::RasterFormat::Png,
    )
    .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn usage_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<UsageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_usage_raster(
        &state,
        &owner,
        &repo,
        theme,
        &q,
        crate::raster::RasterFormat::Webp,
    )
    .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

// Badges

/// Query params for the badge endpoints. See AGENTS / the badge-studio
/// contract for the exact param vocabulary.
#[derive(Debug, Default, Clone, Deserialize)]
struct BadgeQuery {
    theme: Option<String>,
    metrics: Option<String>,
    style: Option<String>,
    animate: Option<String>,
    source: Option<String>,
    npm: Option<String>,
    #[serde(rename = "crate")]
    crate_: Option<String>,
    pypi: Option<String>,
    docker: Option<String>,
}

impl BadgeQuery {
    fn animate(&self) -> bool {
        matches!(self.animate.as_deref(), Some("1") | Some("true"))
    }
    fn usage_query(&self) -> UsageQuery {
        UsageQuery {
            theme: self.theme.clone(),
            type_: None,
            log: None,
            source: self.source.clone(),
            npm: self.npm.clone(),
            crate_: self.crate_.clone(),
            pypi: self.pypi.clone(),
            docker: self.docker.clone(),
            // The badge shows lifetime totals — no date windowing.
            from: None,
            to: None,
            rebase: None,
        }
    }
    /// Stable cache-key fragment.
    fn key_fragment(&self, theme: &crate::theme::Theme) -> String {
        let theme_key = if theme.dark { "dark" } else { "light" };
        let metrics = self.metrics.as_deref().unwrap_or("default");
        let style = self.style.as_deref().unwrap_or("flat");
        let source = self.source.as_deref().unwrap_or("auto");
        format!(
            "{theme_key}|m:{metrics}|s:{style}|a:{}|src:{source}|{}|{}|{}|{}",
            self.animate(),
            self.npm.as_deref().unwrap_or("-"),
            self.crate_.as_deref().unwrap_or("-"),
            self.pypi.as_deref().unwrap_or("-"),
            self.docker.as_deref().unwrap_or("-"),
        )
    }
}

/// Whether the requested metric set needs download data (so we can skip the
/// registry round-trips entirely for a stars/forks-only badge).
fn needs_downloads(metrics: &[Metric]) -> bool {
    metrics.contains(&Metric::Downloads)
}

struct RepoRenderReadiness {
    stars: bool,
    metadata: bool,
    analysis: bool,
    revision: String,
}

async fn repo_render_readiness(
    state: &ApiState,
    repo: &str,
) -> Result<RepoRenderReadiness, ApiError> {
    let summary = state.analyzer.cache.get_repo_summary(repo).await?;
    let (analysis_sha, analysis_active): (Option<String>, bool) = sqlx::query_as(
        "SELECT \
            (SELECT last_analyzed_sha FROM repo_history WHERE repo = $1), \
            EXISTS(SELECT 1 FROM repo_analysis_queue \
                   WHERE repo = $1 AND status IN ('pending', 'in_progress'))",
    )
    .bind(repo)
    .fetch_one(&state.analyzer.cache.db().pool)
    .await?;
    let stars = summary
        .as_ref()
        .is_some_and(|value| value.stargazers_complete);
    let metadata = summary
        .as_ref()
        .is_some_and(|value| value.metadata_fetched_at.is_some());
    let analysis = analysis_sha.is_some() && !analysis_active;
    let revision = format!(
        "s:{}|m:{}|a:{}",
        summary
            .as_ref()
            .and_then(|value| value.stargazers_fetched_at)
            .map(|value| value.timestamp_millis())
            .unwrap_or(0),
        summary
            .as_ref()
            .and_then(|value| value.metadata_fetched_at)
            .map(|value| value.timestamp_millis())
            .unwrap_or(0),
        analysis_sha.as_deref().unwrap_or("-"),
    );
    Ok(RepoRenderReadiness {
        stars,
        metadata,
        analysis,
        revision,
    })
}

fn svg_digest(svg: &str) -> String {
    let digest = Sha256::digest(svg.as_bytes());
    hex::encode(&digest[..8])
}

async fn build_badge_svg(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &BadgeQuery,
) -> Result<RenderedCard, ApiError> {
    if !is_valid_slug(owner) || !is_valid_slug(repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let metrics = Metric::parse_list(q.metrics.as_deref());
    let style = BadgeStyle::parse(q.style.as_deref());
    let repo_full = format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    );
    let readiness = repo_render_readiness(state, &repo_full).await?;
    let stable = metrics.iter().all(|metric| match metric {
        Metric::Stars => readiness.stars,
        Metric::Forks => readiness.metadata,
        Metric::Downloads => readiness.analysis,
    });
    let key = format!(
        "badge:{repo_full}|{}|{}",
        q.key_fragment(theme),
        readiness.revision,
    );
    if stable && let Some(cached) = state.svg_cache.get(&key).await {
        return Ok(RenderedCard {
            svg: cached,
            short_ttl: false,
        });
    }

    // Stars + forks from the cache (best-effort).
    let stars = state
        .analyzer
        .cache
        .get_repo_stargazers(&repo_full)
        .await
        .ok()
        .flatten()
        .map(|v| v.len() as u64);
    let forks = state
        .analyzer
        .cache
        .get_repo_forks(&repo_full)
        .await
        .ok()
        .flatten()
        .map(|n| n.max(0) as u64);

    // Only hit registries if the badge actually shows downloads.
    let downloads = if needs_downloads(&metrics) {
        let uq = q.usage_query();
        let resolved = usage::resolve_packages(owner, repo, &uq.overrides(), &state.storage).await;
        let dl = usage::fetch_all(&state.analyzer.cache, &resolved).await;
        pick_download_source(&dl, q.source.as_deref())
            .map(|(_, stats)| stats.total)
            // Fall back to any source's total (incl. Docker, which has no
            // series) when `auto`/series-pick found nothing.
            .or_else(|| {
                [
                    dl.npm.as_ref(),
                    dl.crates.as_ref(),
                    dl.pypi.as_ref(),
                    dl.docker.as_ref(),
                ]
                .into_iter()
                .flatten()
                .map(|s| s.total)
                .max()
            })
    } else {
        None
    };

    let input = BadgeInput {
        stars,
        forks,
        downloads,
        metrics,
        style,
        animate: q.animate(),
    };
    let svg = crate::badge::render_badge(&input, theme);
    if stable {
        state.svg_cache.insert(key, svg.clone()).await;
    }
    Ok(RenderedCard {
        svg,
        short_ttl: !stable,
    })
}

async fn badge_svg(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<BadgeQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = build_badge_svg(&state, &owner, &repo, theme, &q).await?;
    Ok(card_svg_response(card))
}

async fn ensure_badge_raster(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &BadgeQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let fmt_key = raster_fmt_key(format);
    let card = build_badge_svg(state, owner, repo, theme, q).await?;
    let key = format!(
        "badge:{}/{}|{}|{fmt_key}|{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase(),
        q.key_fragment(theme),
        svg_digest(&card.svg),
    );
    if card.short_ttl {
        return Ok((rasterize_uncached(card.svg, format).await?, true));
    }
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    // Raster shows the final (frozen) frame; freeze_svg_animations in the
    // raster path handles the SMIL → static end-state rewrite.
    Ok((
        rasterize_cached(state, &key, card.svg, format).await?,
        false,
    ))
}

async fn badge_png(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<BadgeQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_badge_raster(
        &state,
        &owner,
        &repo,
        theme,
        &q,
        crate::raster::RasterFormat::Png,
    )
    .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn badge_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<BadgeQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_badge_raster(
        &state,
        &owner,
        &repo,
        theme,
        &q,
        crate::raster::RasterFormat::Webp,
    )
    .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

// Social cards

/// OG card scale. **1.0** is load-bearing: the card SVG's viewBox is
/// 1200×630, so scale 1.0 yields a PNG of exactly those dimensions — the
/// size the frontend declares in `og:image:width`/`height`. (Chart embeds
/// rasterize at 2.0 for retina; OG must match the declared size exactly.)
const OG_RASTER_SCALE: f32 = 1.0;

/// Query params for the OG endpoints. `theme` is accepted for symmetry
/// but defaults to the branded dark card; `repos` drives the compare card
/// on `/api/og.png`.
#[derive(Debug, Default, Clone, Deserialize)]
struct OgQuery {
    theme: Option<String>,
    repos: Option<String>,
}

/// Repo OG card. Sources the headline + secondary data best-effort from
/// the cache (stars/forks), the usage pipeline (download total), and the
/// star series (sparkline). Any missing piece is omitted; the card always
/// renders.
async fn build_repo_og_svg(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
) -> Result<String, ApiError> {
    if !is_valid_slug(owner) || !is_valid_slug(repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let owner = owner.to_ascii_lowercase();
    let repo = repo.to_ascii_lowercase();
    let slug = format!("{owner}/{repo}");

    // History drives the sparkline, while the headline falls back to the
    // authoritative metadata total. GitHub can restrict timeline access for
    // a repository even though its public star count remains available; a
    // social card must not turn that situation into a misleading "0 stars".
    let series = star_series(&owner, &repo, &state.analyzer)
        .await
        .unwrap_or_default();
    let metadata_stars = state
        .analyzer
        .cache
        .get_repo_star_count(&slug)
        .await
        .ok()
        .flatten();
    let stars = best_og_star_total(&series, metadata_stars);

    // Forks from the cache (best-effort; 0 → omit the segment).
    let forks = state
        .analyzer
        .cache
        .get_repo_forks(&slug)
        .await
        .ok()
        .flatten()
        .filter(|n| *n > 0)
        .map(|n| n as u64);

    // Best resolved download total + a short source label. Resolve +
    // fetch are themselves cached + best-effort; we never block the card
    // on a registry. `auto` prefers a time series; we fall back to any
    // source's lifetime total (e.g. Docker pulls).
    let resolved =
        usage::resolve_packages(&owner, &repo, &UsageOverrides::default(), &state.storage).await;
    let dl = usage::fetch_all(&state.analyzer.cache, &resolved).await;
    let downloads = best_download_total(&dl);

    let card = RepoCard {
        slug,
        stars,
        forks,
        downloads,
        series,
    };
    Ok(og::render_repo_card(&card, theme))
}

/// Pick the best `(total, short_label)` download figure for the card, in
/// the same precedence as the badge: prefer a source with a daily series,
/// else any source's lifetime total. The label is short ("npm",
/// "crates", "PyPI", "docker") because the card composes "{label}
/// downloads".
fn best_download_total(dl: &UsageDownloads) -> Option<(u64, String)> {
    // Prefer the source with the longest series (richest signal).
    let by_series = [
        ("npm", dl.npm.as_ref()),
        ("crates", dl.crates.as_ref()),
        ("PyPI", dl.pypi.as_ref()),
        ("docker", dl.docker.as_ref()),
    ];
    by_series
        .iter()
        .filter_map(|(label, s)| s.map(|s| (label, s)))
        .filter(|(_, s)| !s.series.is_empty())
        .max_by_key(|(_, s)| s.series.len())
        .map(|(label, s)| (s.total, label.to_string()))
        // No time series anywhere → take the largest lifetime total.
        .or_else(|| {
            by_series
                .iter()
                .filter_map(|(label, s)| s.map(|s| (s.total, label.to_string())))
                .max_by_key(|(total, _)| *total)
                .filter(|(total, _)| *total > 0)
        })
}

fn best_og_star_total(series: &[Point], metadata_stars: Option<i64>) -> u64 {
    let history_total = series.last().map(|point| point.stars as u64).unwrap_or(0);
    let metadata_total = metadata_stars
        .filter(|total| *total >= 0)
        .map(|total| total as u64)
        .unwrap_or(0);
    history_total.max(metadata_total)
}

async fn ensure_repo_og_raster(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let slug = format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    );
    let readiness = repo_render_readiness(state, &slug).await?;
    let stable = readiness.stars && readiness.metadata && readiness.analysis;
    let theme_key = if theme.dark { "dark" } else { "light" };
    let fmt_key = raster_fmt_key(format);
    let key = format!("og:{slug}|{theme_key}|{fmt_key}|{}", readiness.revision,);
    if stable && let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    let svg = build_repo_og_svg(state, owner, repo, theme).await?;
    if !stable {
        return Ok((
            std::sync::Arc::new(rasterize_limited(svg, format, OG_RASTER_SCALE).await?),
            true,
        ));
    }
    Ok((
        rasterize_cached_scaled(state, &key, svg, format, OG_RASTER_SCALE).await?,
        false,
    ))
}

async fn repo_og_png(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<OgQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = og_theme(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_repo_og_raster(
        &state,
        &owner,
        &repo,
        theme,
        crate::raster::RasterFormat::Png,
    )
    .await?;
    Ok(og_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn repo_og_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<OgQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = og_theme(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_repo_og_raster(
        &state,
        &owner,
        &repo,
        theme,
        crate::raster::RasterFormat::Webp,
    )
    .await?;
    Ok(og_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

/// Build the site card SVG: a compare card when `?repos=` lists ≥2 repos,
/// the default site card otherwise. A single valid repo also yields a
/// compare card (one entry) so `/api/og.png?repos=o/r` is meaningful.
async fn build_site_og_svg(
    state: &ApiState,
    theme: &crate::theme::Theme,
    q: &OgQuery,
) -> Result<String, ApiError> {
    let Some(raw) = q.repos.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(og::render_default_card(theme));
    };
    // Reuse the overlay parser (validate / dedup / cap / lowercase).
    let pairs = parse_overlay_repos(Some(raw))?;
    let mut entries: Vec<CompareEntry> = Vec::with_capacity(pairs.len());
    for (owner, repo) in &pairs {
        let slug = format!("{owner}/{repo}");
        // Best-effort per repo; a failure contributes an empty series.
        let series = star_series(owner, repo, &state.analyzer)
            .await
            .unwrap_or_default();
        let metadata_stars = state
            .analyzer
            .cache
            .get_repo_star_count(&slug)
            .await
            .ok()
            .flatten();
        let stars = best_og_star_total(&series, metadata_stars);
        entries.push(CompareEntry {
            slug,
            stars,
            series,
        });
    }
    Ok(og::render_compare_card(&entries, theme))
}

async fn ensure_site_og_raster(
    state: &ApiState,
    theme: &crate::theme::Theme,
    q: &OgQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let theme_key = if theme.dark { "dark" } else { "light" };
    let fmt_key = raster_fmt_key(format);
    // Normalize the repos list into the cache key so compare cards memoize
    // per slug-set; the default card keys on "site".
    let (repos_key, stable, revision) =
        match q.repos.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => match parse_overlay_repos(Some(raw)) {
                Ok(pairs) => {
                    let mut stable = true;
                    let mut revisions = Vec::with_capacity(pairs.len());
                    for (owner, repo) in &pairs {
                        let summary = state
                            .analyzer
                            .cache
                            .get_repo_summary(&format!("{owner}/{repo}"))
                            .await?;
                        stable &= summary
                            .as_ref()
                            .is_some_and(|value| value.stargazers_complete);
                        revisions.push(
                            summary
                                .and_then(|value| value.stargazers_fetched_at)
                                .map(|value| value.timestamp_millis())
                                .unwrap_or(0)
                                .to_string(),
                        );
                    }
                    (
                        pairs
                            .iter()
                            .map(|(o, r)| format!("{o}/{r}"))
                            .collect::<Vec<_>>()
                            .join(","),
                        stable,
                        revisions.join(","),
                    )
                }
                // Malformed list: surface the error rather than silently
                // serving the default card under a misleading key.
                Err(e) => return Err(e),
            },
            None => ("site".to_string(), true, "static".to_string()),
        };
    let key = format!("og-site:{repos_key}|{theme_key}|{fmt_key}|{revision}");
    if stable && let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    let svg = build_site_og_svg(state, theme, q).await?;
    if !stable {
        return Ok((
            std::sync::Arc::new(rasterize_limited(svg, format, OG_RASTER_SCALE).await?),
            true,
        ));
    }
    Ok((
        rasterize_cached_scaled(state, &key, svg, format, OG_RASTER_SCALE).await?,
        false,
    ))
}

async fn site_og_png(
    State(state): State<ApiState>,
    Query(q): Query<OgQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = og_theme(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_site_og_raster(&state, theme, &q, crate::raster::RasterFormat::Png).await?;
    Ok(og_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn site_og_webp(
    State(state): State<ApiState>,
    Query(q): Query<OgQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = og_theme(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_site_og_raster(&state, theme, &q, crate::raster::RasterFormat::Webp).await?;
    Ok(og_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

/// OG cards default to the branded **dark** card. Only an explicit
/// `theme=light` flips to the light variant; anything else (unset,
/// garbage, "dark") → dark.
fn og_theme(name: Option<&str>) -> &'static crate::theme::Theme {
    match name {
        Some(s) if s.eq_ignore_ascii_case("light") => &crate::theme::LIGHT,
        _ => &crate::theme::DARK,
    }
}

/// OG response headers. Same long edge cache as the charts but explicit
/// here because the dimensions + content-type are the contract social
/// platforms validate against.
fn og_response(
    format: crate::raster::RasterFormat,
    bytes: std::sync::Arc<Vec<u8>>,
    short_ttl: bool,
) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    (headers, (*bytes).clone())
}

// Profile and repository cards
// The github-readme-stats-shaped embeds (`cards.rs` holds the pure
// renderers + math). Everything here reads Postgres ONLY — never GitHub,
// never the analyze/enqueue pipeline — so a README full of cards can't
// touch the shared PAT budget.

/// Query params for the card endpoints — the GRS-compatible vocabulary
/// (`hide=`, `show=`, `card_width=`, `hide_rank=`, `rank_icon=`,
/// `custom_title=`, `show_icons=`, `number_format=`, `animate=`) plus
/// `theme=light|dark`. Unknown pasted GRS params are ignored by serde;
/// see `cards.rs` for the deliberately-unsupported list.
#[derive(Debug, Default, Clone, Deserialize)]
struct CardQuery {
    theme: Option<String>,
    hide: Option<String>,
    show: Option<String>,
    card_width: Option<u32>,
    hide_border: Option<String>,
    hide_title: Option<String>,
    hide_rank: Option<String>,
    rank_icon: Option<String>,
    custom_title: Option<String>,
    show_icons: Option<String>,
    number_format: Option<String>,
    animate: Option<String>,
}

/// `1`/`true` → on; anything else (including absent) → off.
fn flag_on(v: Option<&str>) -> bool {
    matches!(v, Some("1") | Some("true"))
}

impl CardQuery {
    fn user_options(&self) -> cards::UserCardOptions {
        let hide_rank = flag_on(self.hide_rank.as_deref());
        cards::UserCardOptions {
            metrics: cards::select_user_metrics(self.hide.as_deref(), self.show.as_deref()),
            width: cards::clamp_user_width(self.card_width, hide_rank),
            hide_border: flag_on(self.hide_border.as_deref()),
            hide_title: flag_on(self.hide_title.as_deref()),
            hide_rank,
            rank_icon_percentile: self
                .rank_icon
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("percentile")),
            custom_title: self.custom_title.clone(),
            show_icons: self.show_icons(),
            number_format: cards::NumberFormat::parse(self.number_format.as_deref()),
            animate: flag_on(self.animate.as_deref()),
        }
    }

    fn repo_options(&self) -> cards::RepoCardOptions {
        cards::RepoCardOptions {
            metrics: cards::select_repo_metrics(self.hide.as_deref(), self.show.as_deref()),
            width: cards::clamp_repo_width(self.card_width),
            hide_border: flag_on(self.hide_border.as_deref()),
            custom_title: self.custom_title.clone(),
            show_icons: self.show_icons(),
            number_format: cards::NumberFormat::parse(self.number_format.as_deref()),
            animate: flag_on(self.animate.as_deref()),
        }
    }

    /// Icons default ON (our cards are designed around them, unlike GRS);
    /// `show_icons=0|false` turns them off.
    fn show_icons(&self) -> bool {
        !matches!(self.show_icons.as_deref(), Some("0") | Some("false"))
    }

    /// Stable, injective cache-key fragment covering every
    /// render-affecting param. Values are length-prefixed so free-text
    /// params (`custom_title`, `hide`) can never collide across field
    /// boundaries — two different param sets always produce two keys.
    fn key_fragment(&self, theme: &crate::theme::Theme) -> String {
        fn kv(out: &mut String, tag: &str, v: Option<&str>) {
            use std::fmt::Write;
            match v {
                Some(v) => {
                    let _ = write!(out, "|{tag}:{}:{v}", v.len());
                }
                None => {
                    let _ = write!(out, "|{tag}:-");
                }
            }
        }
        let mut k = String::from(if theme.dark { "dark" } else { "light" });
        let w = self.card_width.map(|w| w.to_string());
        kv(&mut k, "h", self.hide.as_deref());
        kv(&mut k, "s", self.show.as_deref());
        kv(&mut k, "w", w.as_deref());
        kv(&mut k, "hb", self.hide_border.as_deref());
        kv(&mut k, "ht", self.hide_title.as_deref());
        kv(&mut k, "hr", self.hide_rank.as_deref());
        kv(&mut k, "ri", self.rank_icon.as_deref());
        kv(&mut k, "ct", self.custom_title.as_deref());
        kv(&mut k, "si", self.show_icons.as_deref());
        kv(&mut k, "nf", self.number_format.as_deref());
        kv(&mut k, "a", self.animate.as_deref());
        k
    }
}

/// A rendered card (or user aggregate chart) plus the TTL class to serve
/// it with. Pending/empty renders get a short TTL (and are never memoized
/// in the 24h caches) so README embeds self-heal once the existing queues
/// catch up; full renders share the standard chart cache policy.
#[derive(Debug)]
struct RenderedCard {
    svg: String,
    short_ttl: bool,
}

/// Single-flight string memo. Concurrent misses for the same `key` coalesce
/// onto ONE `init` future (moka `try_get_with`) instead of stampeding the
/// origin — a celebrity-repo miss on a viral README embed then runs the
/// heavy load (tens of MB of cached rows) once, not once per concurrent
/// request. Determinism is preserved: the closure is the same pure render
/// as before, only its execution is deduplicated. Errors are never memoized
/// (moka only caches the `Ok` value); the shared `Arc<ApiError>` is
/// reconstructed per waiter via [`ApiError::clone_shared`].
pub(crate) async fn single_flight(
    cache: &MokaCache<String, String>,
    key: String,
    init: impl std::future::Future<Output = Result<String, ApiError>>,
) -> Result<String, ApiError> {
    cache
        .try_get_with(key, init)
        .await
        .map_err(|e| e.clone_shared())
}

/// Miss outcome for a single-flight SVG render that must self-heal on the
/// cold/empty path. `Pending` is deliberately returned as the `Err` arm so
/// moka does NOT memoize it into the 24h SVG cache — the placeholder is
/// served at short TTL and re-derived on the next request once the star /
/// analysis queues catch up (same self-healing policy as the pending stat
/// cards). `Failed` carries a real error.
enum RenderMiss {
    Pending(String),
    Failed(ApiError),
}

impl From<ApiError> for RenderMiss {
    fn from(e: ApiError) -> Self {
        RenderMiss::Failed(e)
    }
}

/// Single-flight SVG memo with cold/empty self-healing. Coalesces
/// concurrent misses like [`single_flight`], but the `init` future signals
/// a cold/empty render via `Err(RenderMiss::Pending(svg))` so it is served
/// short-TTL and never pinned in the 24h cache. A full render is cached for
/// 24h and returned with the standard chart cache policy.
async fn single_flight_card(
    cache: &MokaCache<String, String>,
    key: String,
    init: impl std::future::Future<Output = Result<String, RenderMiss>>,
) -> Result<RenderedCard, ApiError> {
    match cache.try_get_with(key, init).await {
        Ok(svg) => Ok(RenderedCard {
            svg,
            short_ttl: false,
        }),
        Err(arc) => match &*arc {
            RenderMiss::Pending(svg) => Ok(RenderedCard {
                svg: svg.clone(),
                short_ttl: true,
            }),
            RenderMiss::Failed(e) => Err(e.clone_shared()),
        },
    }
}

fn card_cache_control(short_ttl: bool) -> HeaderValue {
    if short_ttl {
        HeaderValue::from_static("public, s-maxage=300, max-age=60")
    } else {
        HeaderValue::from_static("public, s-maxage=86400, max-age=3600")
    }
}

fn card_svg_response(card: RenderedCard) -> (HeaderMap, String) {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, card_cache_control(card.short_ttl));
    (headers, crate::brand::with_site_link(card.svg))
}

fn card_raster_response(
    format: crate::raster::RasterFormat,
    bytes: std::sync::Arc<Vec<u8>>,
    short_ttl: bool,
) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    (headers, (*bytes).clone())
}

/// Rasterize without memoizing — for short-TTL pending/empty cards whose
/// bytes must not sit in the 24h raster cache.
async fn rasterize_uncached(
    svg: String,
    format: crate::raster::RasterFormat,
) -> Result<std::sync::Arc<Vec<u8>>, ApiError> {
    let bytes = rasterize_limited(svg, format, RASTER_SCALE).await?;
    Ok(std::sync::Arc::new(bytes))
}

/// Aggregate the user-card stats from Postgres only: `repos` ownership
/// rows (stars/forks/tracked counts), `repo_author_stats` commit
/// aggregates (commits/contribs/since — via the `LOWER(github_login)`
/// partial index), and `repo_lines` (top languages). The login is
/// [`cards::is_valid_login`]-validated (alphanumeric + `-` only, so the
/// bound value contains no LIKE metacharacters and can never widen the
/// prefix match). SUMs over BIGINT columns are cast back to BIGINT
/// (Postgres would otherwise return NUMERIC).
async fn load_user_card_data(
    db: &crate::db::Db,
    login: &str,
) -> Result<cards::UserCardData, ApiError> {
    let owned = sqlx::query(
        "SELECT COUNT(*) AS repos_tracked, \
                COALESCE(SUM(GREATEST(star_count, 0)), 0)::BIGINT AS stars, \
                COALESCE(SUM(GREATEST(forks_count, 0)), 0)::BIGINT AS forks, \
                COUNT(history.repo) FILTER \
                    (WHERE history.last_analyzed_at IS NOT NULL \
                       AND active.repo IS NULL \
                       AND NOT EXISTS ( \
                           SELECT 1 FROM repo_author_stats author \
                           WHERE author.repo = repos.repo \
                             AND (author.github_login IS NULL \
                                  OR author.avatar_url LIKE 'https://www.gravatar.com/%') \
                             AND author.enrich_attempted_at IS NULL \
                       )) AS repos_analyzed \
         FROM repos \
         LEFT JOIN repo_history history ON history.repo = repos.repo \
         LEFT JOIN repo_analysis_queue active \
           ON active.repo = repos.repo \
          AND active.status IN ('pending', 'in_progress') \
         WHERE repos.repo LIKE $1 || '/%' AND NOT repos.missing",
    )
    .bind(login)
    .fetch_one(&db.pool)
    .await?;
    let repos_tracked: i64 = owned.try_get("repos_tracked")?;
    let stars: i64 = owned.try_get("stars")?;
    let forks: i64 = owned.try_get("forks")?;
    let repos_analyzed: i64 = owned.try_get("repos_analyzed")?;

    let authored = sqlx::query(
        "SELECT COALESCE(SUM(commits), 0)::BIGINT AS commits, \
                COUNT(DISTINCT repo) AS contribs, \
                MIN(first_commit_at) AS first_at \
         FROM repo_author_stats WHERE LOWER(github_login) = $1",
    )
    .bind(login)
    .fetch_one(&db.pool)
    .await?;
    let commits: i64 = authored.try_get("commits")?;
    let contribs: i64 = authored.try_get("contribs")?;
    let first_at: Option<chrono::DateTime<chrono::Utc>> = authored.try_get("first_at")?;

    let langs = load_top_langs(db, LangScope::Owner, login).await?;

    Ok(cards::UserCardData {
        login: login.to_string(),
        stars: stars.max(0) as u64,
        commits: commits.max(0) as u64,
        contribs: contribs.max(0) as u64,
        repos_tracked: repos_tracked.max(0) as u64,
        repos_analyzed: repos_analyzed.max(0) as u64,
        forks: forks.max(0) as u64,
        since_year: first_at.map(|t| t.year()),
        langs,
    })
}

#[derive(Clone, Copy)]
enum LangScope {
    Owner,
    Repo,
}

/// Ties are broken by name so rendered card bytes remain deterministic.
async fn load_top_langs(
    db: &crate::db::Db,
    scope: LangScope,
    bind: &str,
) -> Result<Vec<(String, i64)>, ApiError> {
    let rows = match scope {
        LangScope::Owner => {
            sqlx::query(
                "SELECT language, SUM(lines_code)::BIGINT AS lines FROM repo_lines \
                 WHERE repo LIKE $1 || '/%' \
                 GROUP BY language ORDER BY lines DESC, language LIMIT 5",
            )
            .bind(bind)
            .fetch_all(&db.pool)
            .await?
        }
        LangScope::Repo => {
            sqlx::query(
                "SELECT language, SUM(lines_code)::BIGINT AS lines FROM repo_lines \
                 WHERE repo = $1 \
                 GROUP BY language ORDER BY lines DESC, language LIMIT 5",
            )
            .bind(bind)
            .fetch_all(&db.pool)
            .await?
        }
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let language: String = row.try_get("language")?;
        let lines: i64 = row.try_get("lines")?;
        out.push((language, lines));
    }
    Ok(out)
}

/// Build the repo-card inputs from Postgres only. `None` cells mean "not
/// observed yet" and are dropped by the renderer — never faked as zero.
/// Star-derived fields (`stars_30d`, `spark`, and the day-accurate total)
/// honor the completeness invariant: they only populate when
/// `stargazers_complete` (readers never trust partial data); otherwise the
/// headline falls back to the denormalized metadata count. The 30-day
/// windows are relative to the *data's* last day, not the wall clock, so
/// renders stay deterministic for a given DB state.
async fn load_repo_card_data(
    state: &ApiState,
    repo_full: &str,
    summary: &crate::cache::RepoSummary,
) -> Result<cards::RepoCardData, ApiError> {
    let db = state.analyzer.cache.db();
    let archive_activity = summary.history_source.as_deref() == Some("gh_archive");

    let (stars, stars_30d, spark) = if summary.stargazers_complete {
        let day_stats = export::accumulate(&export::load_day_deltas(db, repo_full).await?);
        let total = if archive_activity {
            summary
                .star_count
                .filter(|value| *value >= 0)
                .map(|value| value as u64)
        } else {
            day_stats.last().map(|row| row.total)
        };
        // A WatchEvent count is not net new stars because unstars are absent.
        // Do not render it under the github-readme-stats-compatible
        // `stars_30d` label.
        let stars_30d = (!archive_activity)
            .then(|| {
                day_stats.last().map(|last| {
                    let cutoff = last.date - chrono::Duration::days(30);
                    day_stats
                        .iter()
                        .filter(|row| row.date > cutoff)
                        .map(|row| row.delta)
                        .sum::<u64>()
                })
            })
            .flatten();
        let points: Vec<Point> = day_stats
            .iter()
            .map(|r| Point {
                at: chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                    r.date.and_hms_opt(0, 0, 0).expect("midnight is valid"),
                    chrono::Utc,
                ),
                stars: r.total.min(u32::MAX as u64) as u32,
            })
            .collect();
        (total, stars_30d, cards::spark_window(&points, 90, 40))
    } else {
        (
            summary.star_count.filter(|n| *n >= 0).map(|n| n as u64),
            None,
            Vec::new(),
        )
    };

    let forks = state
        .analyzer
        .cache
        .get_repo_forks(repo_full)
        .await
        .ok()
        .flatten()
        .map(|n| n.max(0) as u64);

    // Clone-analysis aggregates — absent until the analysis pipeline has
    // walked the repo at least once.
    let commits: Option<i64> = sqlx::query_scalar(
        "SELECT total_commits FROM repo_history WHERE repo = $1 AND last_analyzed_at IS NOT NULL",
    )
    .bind(repo_full)
    .fetch_optional(&db.pool)
    .await?;
    let contributors: Option<i64> = if commits.is_some() {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repo_author_stats WHERE repo = $1")
            .bind(repo_full)
            .fetch_one(&db.pool)
            .await?;
        Some(n)
    } else {
        None
    };

    // Tokei lines (SUM over zero rows is NULL → None, exactly the
    // "unavailable" semantics the renderer wants).
    let lines_total: Option<i64> =
        sqlx::query_scalar("SELECT SUM(lines_code)::BIGINT FROM repo_lines WHERE repo = $1")
            .bind(repo_full)
            .fetch_one(&db.pool)
            .await?;

    // Trailing 30 days of commit activity, anchored on the last observed
    // commit day (determinism — no wall clock).
    let commits_30d: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(commits)::BIGINT FROM repo_commit_days \
         WHERE repo = $1 \
           AND day > (SELECT MAX(day) FROM repo_commit_days WHERE repo = $1) - 30",
    )
    .bind(repo_full)
    .fetch_one(&db.pool)
    .await?;

    let langs = load_top_langs(db, LangScope::Repo, repo_full).await?;

    Ok(cards::RepoCardData {
        slug: repo_full.to_string(),
        stars,
        forks,
        contributors: contributors.map(|n| n.max(0) as u64),
        commits: commits.map(|n| n.max(0) as u64),
        created_year: summary.created_at.map(|t| t.year()),
        lines_total: lines_total.map(|n| n.max(0) as u64),
        commits_30d: commits_30d.map(|n| n.max(0) as u64),
        stars_30d,
        langs,
        spark,
    })
}

/// Render-or-fetch the user profile card. Full cards memoize in
/// `stat_svg_cache`; the "no data yet" card is short-TTL and uncached.
async fn ensure_user_card_svg(
    state: &ApiState,
    login: &str,
    theme: &crate::theme::Theme,
    q: &CardQuery,
) -> Result<RenderedCard, ApiError> {
    if !cards::is_valid_login(login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let login = login.to_ascii_lowercase();
    let key = format!("card:user:{login}|{}", q.key_fragment(theme));
    if let Some(svg) = state.stat_svg_cache.get(&key).await {
        return Ok(RenderedCard {
            svg,
            short_ttl: false,
        });
    }
    let data = load_user_card_data(state.analyzer.cache.db(), &login).await?;
    if !data.has_data() {
        // Nothing tracked yet. No enqueue here — cards never drive
        // ingestion; the analyze/ext-ping paths own that.
        return Ok(RenderedCard {
            svg: cards::render_user_empty_card(&login, theme),
            short_ttl: true,
        });
    }
    let svg =
        cards::render_user_card(&data, &q.user_options(), theme).map_err(ApiError::bad_request)?;
    // Commit/contributor totals are lower bounds while owned tracked repos
    // are still warming. Never pin that intermediate state in the 24h cache:
    // the embed self-heals as the durable analysis queue drains.
    let analysis_pending = data.analysis_pending();
    if !analysis_pending {
        state.stat_svg_cache.insert(key, svg.clone()).await;
    }
    Ok(RenderedCard {
        svg,
        short_ttl: analysis_pending,
    })
}

/// Render-or-fetch the repo stats card. Tombstoned repos get the terminal
/// "not found" card (fully cacheable); cold repos (no star history AND no
/// clone analysis) get the short-TTL pending card.
async fn ensure_repo_card_svg(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &CardQuery,
) -> Result<RenderedCard, ApiError> {
    if !is_valid_slug(owner) || !is_valid_slug(repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let repo_full = format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    );
    let key = format!("card:repo:{repo_full}|{}", q.key_fragment(theme));
    if let Some(svg) = state.stat_svg_cache.get(&key).await {
        return Ok(RenderedCard {
            svg,
            short_ttl: false,
        });
    }
    let summary = state.analyzer.cache.get_repo_summary(&repo_full).await?;
    if summary.as_ref().is_some_and(|s| s.missing) {
        // Terminal tombstone → standard TTL is safe.
        let svg = cards::render_repo_missing_card(&repo_full, theme);
        state.stat_svg_cache.insert(key, svg.clone()).await;
        return Ok(RenderedCard {
            svg,
            short_ttl: false,
        });
    }
    let Some(summary) = summary else {
        return Ok(RenderedCard {
            svg: cards::render_repo_pending_card(&repo_full, None, theme),
            short_ttl: true,
        });
    };
    let data = load_repo_card_data(state, &repo_full, &summary).await?;
    let analyzed = data.commits.is_some() || data.lines_total.is_some();
    if !summary.stargazers_complete && !analyzed {
        return Ok(RenderedCard {
            svg: cards::render_repo_pending_card(&repo_full, data.stars, theme),
            short_ttl: true,
        });
    }
    let svg =
        cards::render_repo_card(&data, &q.repo_options(), theme).map_err(ApiError::bad_request)?;
    state.stat_svg_cache.insert(key, svg.clone()).await;
    Ok(RenderedCard {
        svg,
        short_ttl: false,
    })
}

async fn ensure_user_card_raster(
    state: &ApiState,
    login: &str,
    theme: &crate::theme::Theme,
    q: &CardQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    if !cards::is_valid_login(login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let fmt_key = raster_fmt_key(format);
    let key = format!(
        "card:user:{}|{}|{fmt_key}",
        login.to_ascii_lowercase(),
        q.key_fragment(theme)
    );
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    let card = ensure_user_card_svg(state, login, theme, q).await?;
    if card.short_ttl {
        return Ok((rasterize_uncached(card.svg, format).await?, true));
    }
    Ok((
        rasterize_cached(state, &key, card.svg, format).await?,
        false,
    ))
}

async fn ensure_repo_card_raster(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &CardQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    if !is_valid_slug(owner) || !is_valid_slug(repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let fmt_key = raster_fmt_key(format);
    let key = format!(
        "card:repo:{}/{}|{}|{fmt_key}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase(),
        q.key_fragment(theme)
    );
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    let card = ensure_repo_card_svg(state, owner, repo, theme, q).await?;
    if card.short_ttl {
        return Ok((rasterize_uncached(card.svg, format).await?, true));
    }
    Ok((
        rasterize_cached(state, &key, card.svg, format).await?,
        false,
    ))
}

async fn user_card_svg(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<CardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_user_card_svg(&state, &login, theme, &q).await?;
    Ok(card_svg_response(card))
}

async fn user_card_png(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<CardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_card_raster(&state, &login, theme, &q, crate::raster::RasterFormat::Png)
            .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn user_card_webp(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<CardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_card_raster(&state, &login, theme, &q, crate::raster::RasterFormat::Webp)
            .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

async fn repo_card_svg(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<CardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_repo_card_svg(&state, &owner, &repo, theme, &q).await?;
    Ok(card_svg_response(card))
}

async fn repo_card_png(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<CardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_repo_card_raster(
        &state,
        &owner,
        &repo,
        theme,
        &q,
        crate::raster::RasterFormat::Png,
    )
    .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn repo_card_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<CardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_repo_card_raster(
        &state,
        &owner,
        &repo,
        theme,
        &q,
        crate::raster::RasterFormat::Webp,
    )
    .await?;
    Ok(card_raster_response(
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

// Sitemap data

/// Default + max `per` (page size) for the sitemap endpoint. The default
/// matches a single sitemap chunk's URL ceiling (sitemaps cap at 50k
/// URLs); 20k leaves headroom for per-repo alt URLs on the frontend.
const SITEMAP_PER_DEFAULT: i64 = 20_000;
const SITEMAP_PER_MAX: i64 = 50_000;

#[derive(Debug, Default, Clone, Deserialize)]
struct SitemapQuery {
    page: Option<i64>,
    per: Option<i64>,
}

/// JSON list of analyzed repos (those with cached star history) for the
/// frontend to emit a programmatic sitemap on its own origin. Cheap,
/// stably ordered, and cacheable.
async fn sitemap_repos(
    State(state): State<ApiState>,
    Query(q): Query<SitemapQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let per = q
        .per
        .unwrap_or(SITEMAP_PER_DEFAULT)
        .clamp(1, SITEMAP_PER_MAX);
    let page = q.page.unwrap_or(0).max(0);
    let offset = page.saturating_mul(per);

    let cache = &state.analyzer.cache;
    let total = cache.count_sitemap_repos().await?;
    let rows = cache.list_sitemap_repos(per, offset).await?;
    let repos: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(slug, updated_at)| {
            serde_json::json!({
                "slug": slug,
                // RFC3339 (Z), matching the rest of the JSON contract.
                "updated_at": updated_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            })
        })
        .collect();

    let body = serde_json::json!({
        "total": total,
        "page": page,
        "per_page": per,
        "repos": repos,
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, s-maxage=3600, max-age=300"),
    );
    Ok((headers, Json(body)))
}

// Leaderboard

/// Ranking metric for `/api/leaderboard.json`. `Stars` ranks by the
/// denormalized GitHub star count on repo metadata; `Velocity` ranks by
/// stars added in the trailing window, counted directly from cached
/// stargazer timestamps. Both are celebratory popularity/growth surfaces
/// over repos gitdebt has already analyzed — never a judgment about
/// accounts or star provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeaderboardMetric {
    Stars,
    Velocity,
}

impl LeaderboardMetric {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stars => "stars",
            Self::Velocity => "velocity",
        }
    }
}

/// Trailing window (days) for the velocity metric — "stars added in the
/// last 7 days", per the export/leaderboard API contract.
const LEADERBOARD_WINDOW_DAYS: i64 = 7;
/// Default + max page size. The default matches the contract (`per=50`).
const LEADERBOARD_PER_DEFAULT: i64 = 50;
const LEADERBOARD_PER_MAX: i64 = 100;
/// Deep pagination is a scraper pattern, not a reader pattern. Capping
/// `page` bounds the OFFSET the DB is asked to scan past.
const LEADERBOARD_PAGE_MAX: i64 = 200;

#[derive(Debug, Default, Clone, Deserialize)]
struct LeaderboardQuery {
    metric: Option<String>,
    per: Option<i64>,
    page: Option<i64>,
}

/// Normalize + validate leaderboard params. An unknown metric is a 400
/// (fail loudly — a typo'd client should not silently get the wrong
/// ranking); `per`/`page` are clamped into DoS-safe bounds.
fn leaderboard_params(
    metric: Option<&str>,
    per: Option<i64>,
    page: Option<i64>,
) -> Result<(LeaderboardMetric, i64, i64), &'static str> {
    let metric = match metric.unwrap_or("stars") {
        "stars" => LeaderboardMetric::Stars,
        "velocity" => LeaderboardMetric::Velocity,
        _ => return Err("invalid metric (expected stars or velocity)"),
    };
    let per = per
        .unwrap_or(LEADERBOARD_PER_DEFAULT)
        .clamp(1, LEADERBOARD_PER_MAX);
    let page = page.unwrap_or(0).clamp(0, LEADERBOARD_PAGE_MAX);
    Ok((metric, per, page))
}

/// One leaderboard row: `(slug, total stars, stars added in the trailing
/// window)`. Only repos with **complete** cached star history participate
/// (readers never trust partial data) and tombstoned repos are excluded,
/// so every row links to a live, indexable repo page.
async fn load_leaderboard_rows(
    state: &ApiState,
    metric: LeaderboardMetric,
    limit: i64,
    offset: i64,
) -> Result<Vec<(String, i64, i64)>, ApiError> {
    let pool = &state.analyzer.cache.db().pool;
    let sql = match metric {
        // Most-starred: order by the metadata star count. The LATERAL
        // velocity count only runs for the LIMIT rows actually returned
        // (the plan puts the nested loop above the sorted+limited scan).
        LeaderboardMetric::Stars => {
            "SELECT r.repo, COALESCE(r.star_count, 0) AS stars, COALESCE(v.velocity, 0) AS velocity \
             FROM repos r \
             LEFT JOIN LATERAL ( \
                 SELECT COUNT(*) AS velocity FROM active_repo_star_history s \
                 WHERE s.repo = r.repo \
                   AND s.starred_at >= NOW() - make_interval(days => $3) \
             ) v ON TRUE \
             WHERE r.history_complete = TRUE AND r.missing = FALSE \
               AND r.star_count IS NOT NULL \
             ORDER BY r.star_count DESC, r.repo ASC \
             LIMIT $1 OFFSET $2"
        }
        // Fastest-growing: rank by stars added in the trailing window.
        // Inner join — a repo with zero recent stars has no business on
        // a velocity leaderboard.
        LeaderboardMetric::Velocity => {
            "SELECT r.repo, COALESCE(r.star_count, 0) AS stars, v.velocity \
             FROM repos r \
             JOIN ( \
                 SELECT repo, COUNT(*) AS velocity FROM active_repo_star_history \
                 WHERE starred_at >= NOW() - make_interval(days => $3) \
                 GROUP BY repo \
             ) v ON v.repo = r.repo \
             WHERE r.history_complete = TRUE AND r.missing = FALSE \
             ORDER BY v.velocity DESC, r.repo ASC \
             LIMIT $1 OFFSET $2"
        }
    };
    let rows = sqlx::query(sql)
        .bind(limit)
        .bind(offset)
        .bind(LEADERBOARD_WINDOW_DAYS as i32)
        .fetch_all(pool)
        .await
        .map_err(anyhow::Error::from)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let repo: String = row.try_get("repo").map_err(anyhow::Error::from)?;
        let stars: i64 = row.try_get("stars").map_err(anyhow::Error::from)?;
        let velocity: i64 = row.try_get("velocity").map_err(anyhow::Error::from)?;
        out.push((repo, stars, velocity));
    }
    Ok(out)
}

/// `GET /api/leaderboard.json?metric=stars|velocity&per=50&page=0` —
/// ranked repos from the repos/repo_stargazers tables only. No GitHub
/// calls on this path. Memoized 5 min in its OWN moka cache (see the
/// `leaderboard_cache` field docs — the param-derived key space must not
/// be able to evict warm `/analyze` bodies) and served with the same
/// cache envelope as `/analyze`.
async fn leaderboard_json(
    State(state): State<ApiState>,
    Query(q): Query<LeaderboardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let (metric, per, page) =
        leaderboard_params(q.metric.as_deref(), q.per, q.page).map_err(ApiError::bad_request)?;
    let key = format!("leaderboard:{}:{per}:{page}", metric.as_str());
    // Single-flight: the trailing-window velocity GROUP BY / stars LATERAL
    // is the heaviest read on the largest table; coalesce concurrent misses
    // for the same page onto one query instead of a stampede.
    let json = single_flight(&state.leaderboard_cache, key, async {
        let rows = load_leaderboard_rows(&state, metric, per, page.saturating_mul(per)).await?;
        let repos: Vec<serde_json::Value> = rows
            .iter()
            .enumerate()
            .map(|(i, (repo, stars, velocity))| {
                serde_json::json!({
                    "rank": page.saturating_mul(per) + i as i64 + 1,
                    "repo": repo,
                    "stars": stars,
                    "velocity": velocity,
                })
            })
            .collect();
        Ok(serde_json::to_string(&serde_json::json!({
            "metric": metric.as_str(),
            "page": page,
            "per_page": per,
            "window_days": LEADERBOARD_WINDOW_DAYS,
            "repos": repos,
        }))?)
    })
    .await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, s-maxage=300, max-age=60"),
    );
    Ok((headers, json))
}

#[derive(Serialize)]
struct PlatformActivityResponse {
    repos: Vec<PlatformActivityItem>,
}

#[derive(Serialize)]
struct PlatformActivityItem {
    repo: String,
    stars: i64,
    views: i64,
    viewed_at: DateTime<Utc>,
    history_ready: bool,
    analysis_ready: bool,
}

/// A short-lived, Postgres-only pulse of repositories people are actually
/// opening on gitdebt. It reveals no viewer identity and performs no GitHub
/// request, making it safe for a public landing-page surface.
async fn platform_activity(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let rows = state.analyzer.cache.list_platform_activity(8).await?;
    let repos = rows
        .into_iter()
        .map(|row| PlatformActivityItem {
            repo: row.repo,
            stars: row.stars,
            views: row.views,
            viewed_at: row.viewed_at,
            history_ready: row.history_ready,
            analysis_ready: row.analysis_ready,
        })
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, s-maxage=60, max-age=30"),
    );
    Ok((headers, Json(PlatformActivityResponse { repos })))
}

/// Rasterize `svg` on the blocking pool at the default chart scale
/// (`RASTER_SCALE`) and memoize under `key`. rasterize is CPU-bound
/// (~30–80ms on a 1200×600 chart); spawn_blocking keeps it off the
/// runtime threads.
async fn rasterize_cached(
    state: &ApiState,
    key: &str,
    svg: String,
    format: crate::raster::RasterFormat,
) -> Result<std::sync::Arc<Vec<u8>>, ApiError> {
    rasterize_cached_scaled(state, key, svg, format, RASTER_SCALE).await
}

/// Rasterize at an explicit scale. OG cards rasterize at **1.0** so the
/// PNG is exactly the SVG's 1200×630 viewBox — social platforms require
/// the file dimensions to match the declared `og:image` size, unlike the
/// retina-density chart embeds.
async fn rasterize_cached_scaled(
    state: &ApiState,
    key: &str,
    svg: String,
    format: crate::raster::RasterFormat,
    scale: f32,
) -> Result<std::sync::Arc<Vec<u8>>, ApiError> {
    let bytes = rasterize_limited(svg, format, scale).await?;
    let arc = std::sync::Arc::new(bytes);
    state
        .raster_cache
        .insert(key.to_string(), arc.clone())
        .await;
    Ok(arc)
}

/// Max concurrent SVG rasterizations, process-wide. `rasterize` is
/// 30–80ms of pure CPU per chart; tokio's blocking pool defaults to
/// hundreds of threads, so without a cap a burst of raster misses (or a
/// cache-busting param loop) becomes hundreds of parallel encodes
/// saturating every core. Four permits keeps rasterization to a few
/// cores; excess requests queue briefly on the semaphore instead.
const RASTER_CONCURRENCY: usize = 4;

/// Process-wide raster concurrency permits. Hoisted to a module-level
/// static (from a fn-local one) so `/metrics` can report available permits
/// — the key saturation signal for the CPU-bound raster path.
pub(crate) static RASTER_PERMITS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(RASTER_CONCURRENCY);

/// The single choke point every raster path (charts, cards, OG, stat
/// SVGs) must go through: semaphore-capped `spawn_blocking` around
/// [`crate::raster::rasterize`].
pub(crate) async fn rasterize_limited(
    svg: String,
    format: crate::raster::RasterFormat,
    scale: f32,
) -> Result<Vec<u8>, ApiError> {
    let _permit = RASTER_PERMITS
        .acquire()
        .await
        .expect("raster semaphore is never closed");
    tokio::task::spawn_blocking(move || crate::raster::rasterize(&svg, format, scale))
        .await
        .map_err(|e| ApiError::from(anyhow::anyhow!("raster task: {e}")))?
        .map_err(ApiError::from)
}

/// Scale factor applied to raster output. 2.0 = retina density at the
/// SVG's CSS size — sharp on high-DPI screens, still reasonable file
/// size after lossless WebP / PNG encoding.
const RASTER_SCALE: f32 = 2.0;

/// An IPv4 or IPv6 CIDR block, used to decide whether the socket peer is a
/// trusted reverse proxy whose forwarding headers we honor.
#[derive(Debug, Clone, Copy)]
struct Cidr {
    base: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Parse `a.b.c.d/n` or `addr::/n` (a bare address = host route, /32 or
    /// /128). Returns `None` on any malformed input.
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let (addr_part, prefix_part) = match s.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (s, None),
        };
        let base: IpAddr = addr_part.parse().ok()?;
        let max = if base.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix_part {
            Some(p) => p.parse::<u8>().ok().filter(|n| *n <= max)?,
            None => max,
        };
        Some(Self { base, prefix })
    }

    /// True iff `ip` falls inside this block. v4 and v6 never match across
    /// families (a v4-mapped v6 address is compared as v6 — callers should
    /// normalize the peer, which `SocketAddr` does not, so we also try the
    /// mapped form below).
    fn contains(&self, ip: IpAddr) -> bool {
        match (self.base, ip) {
            (IpAddr::V4(b), IpAddr::V4(x)) => v4_prefix_match(b, x, self.prefix),
            (IpAddr::V6(b), IpAddr::V6(x)) => v6_prefix_match(b, x, self.prefix),
            _ => false,
        }
    }
}

fn v4_prefix_match(base: Ipv4Addr, ip: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask: u32 = u32::MAX.checked_shl(32 - prefix as u32).unwrap_or(0);
    (u32::from(base) & mask) == (u32::from(ip) & mask)
}

fn v6_prefix_match(base: Ipv6Addr, ip: Ipv6Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask: u128 = u128::MAX.checked_shl(128 - prefix as u32).unwrap_or(0);
    (u128::from(base) & mask) == (u128::from(ip) & mask)
}

/// Default trusted-proxy CIDRs when `TRUSTED_PROXIES` is unset: loopback,
/// the RFC1918 private ranges (a reverse proxy on the same host/VPC), plus
/// Cloudflare's published IPv4/IPv6 ranges (so a CF-fronted deploy works
/// out of the box). Documented at https://www.cloudflare.com/ips/.
const DEFAULT_TRUSTED_PROXIES: &[&str] = &[
    // Loopback + private.
    "127.0.0.0/8",
    "::1/128",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    // Cloudflare IPv4.
    "173.245.48.0/20",
    "103.21.244.0/22",
    "103.22.200.0/22",
    "103.31.4.0/22",
    "141.101.64.0/18",
    "108.162.192.0/18",
    "190.93.240.0/20",
    "188.114.96.0/20",
    "197.234.240.0/22",
    "198.41.128.0/17",
    "162.158.0.0/15",
    "104.16.0.0/13",
    "104.24.0.0/14",
    "172.64.0.0/13",
    "131.0.72.0/22",
    // Cloudflare IPv6.
    "2400:cb00::/32",
    "2606:4700::/32",
    "2803:f800::/32",
    "2405:b500::/32",
    "2405:8100::/32",
    "2a06:98c0::/29",
    "2c0f:f248::/32",
];

/// Process-wide trusted-proxy set. Parsed once from `TRUSTED_PROXIES`
/// (comma-separated CIDRs) or the documented defaults. We only honor
/// forwarding headers (`cf-connecting-ip`, `x-forwarded-for`, `forwarded`)
/// when the socket peer falls in this set — otherwise a client with a
/// direct connection to the origin (default `0.0.0.0` bind) could spoof a
/// fresh rate-limit bucket on every request.
fn trusted_proxies() -> &'static [Cidr] {
    static CELL: OnceLock<Vec<Cidr>> = OnceLock::new();
    CELL.get_or_init(|| {
        let raw = std::env::var("TRUSTED_PROXIES").ok();
        let specs: Vec<String> = match raw.as_deref() {
            Some(s) if !s.trim().is_empty() => s.split(',').map(|p| p.trim().to_string()).collect(),
            _ => DEFAULT_TRUSTED_PROXIES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        let mut out = Vec::with_capacity(specs.len());
        for spec in specs {
            match Cidr::parse(&spec) {
                Some(c) => out.push(c),
                None => tracing::warn!(cidr = %spec, "TRUSTED_PROXIES: ignoring invalid CIDR"),
            }
        }
        out
    })
}

/// True iff `peer` is one of the configured trusted reverse proxies.
fn peer_is_trusted(peer: IpAddr) -> bool {
    // Normalize a v4-mapped v6 peer (`::ffff:a.b.c.d`) to plain v4 so it
    // matches the v4 CIDRs too.
    let normalized = match peer {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(peer),
        v4 => v4,
    };
    trusted_proxies()
        .iter()
        .any(|c| c.contains(peer) || c.contains(normalized))
}

/// Custom key extractor that prefers the `cf-connecting-ip` header
/// (set by Cloudflare to the original client IP), but only when the socket
/// peer is a trusted reverse proxy (see [`trusted_proxies`]). Without this
/// header every request behind Cloudflare hashes to one CF egress IP and
/// the per-IP limiter degenerates into a global limit; *with* it but
/// ungated, a client reaching the origin directly could spoof the header
/// for a fresh bucket per request. The gate closes that hole: from an
/// untrusted peer we use the socket peer IP itself and ignore all
/// forwarding headers.
#[derive(Debug, Clone, Copy)]
struct CloudflareIpKeyExtractor;

/// Resolve a client IP with exactly the same trusted-proxy policy as the
/// request governors. Long-lived handlers use this to apply concurrency
/// limits that cannot be represented by a token-bucket admission layer.
pub(crate) fn request_client_ip(
    headers: &HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Option<IpAddr> {
    let mut request = axum::http::Request::new(());
    *request.headers_mut() = headers.clone();
    if let Some(connect_info) = connect_info {
        request.extensions_mut().insert(connect_info);
    }
    CloudflareIpKeyExtractor.extract(&request).ok()
}

impl KeyExtractor for CloudflareIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &axum::http::Request<T>) -> Result<Self::Key, GovernorError> {
        // The socket peer IP (registered by
        // `into_make_service_with_connect_info::<SocketAddr>` in main.rs).
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip());

        // Only trust forwarding headers from a trusted proxy hop.
        if let Some(peer_ip) = peer {
            if peer_is_trusted(peer_ip) {
                if let Some(v) = req.headers().get("cf-connecting-ip")
                    && let Ok(s) = v.to_str()
                    && let Ok(ip) = s.parse::<IpAddr>()
                {
                    return Ok(ip);
                }
                // Trusted proxy but no cf-connecting-ip → fall back to the
                // XFF / Forwarded chain via SmartIpKeyExtractor.
                return SmartIpKeyExtractor.extract(req);
            }
            // Untrusted direct peer: key on the peer IP, ignore headers.
            return Ok(peer_ip);
        }

        // No ConnectInfo (shouldn't happen with the wiring in main.rs) —
        // fall back to the header-based extractor.
        SmartIpKeyExtractor.extract(req)
    }
}

/// Debug is derived for test ergonomics (`Result<_, ApiError>::unwrap`);
/// the client-facing rendering still goes through `IntoResponse`, which
/// never leaks 5xx internals.
#[derive(Debug)]
pub struct ApiError {
    inner: anyhow::Error,
    status: StatusCode,
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            inner: anyhow::anyhow!(msg.into()),
            status: StatusCode::BAD_REQUEST,
        }
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self {
            inner: anyhow::anyhow!(msg.into()),
            status: StatusCode::UNAUTHORIZED,
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            inner: anyhow::anyhow!(msg.into()),
            status: StatusCode::NOT_FOUND,
        }
    }

    /// 503 — the request can't be served right now (e.g. no cached data
    /// and no GitHub budget headroom) but a retry later will succeed. The
    /// message goes to the log only: `IntoResponse` gives every 5xx a
    /// generic body, never internals.
    pub fn unavailable(msg: impl Into<String>) -> Self {
        Self {
            inner: anyhow::anyhow!(msg.into()),
            status: StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Reconstruct a shareable copy of this error. `moka`'s single-flight
    /// `try_get_with` hands a failed init back to every coalesced waiter as
    /// `Arc<ApiError>` (the inner `anyhow::Error` isn't `Clone`, so we can't
    /// move it out of the Arc). Status is preserved; the message is
    /// preserved for logging + 4xx bodies (5xx bodies are generic anyway,
    /// so no internals leak through this path either).
    pub fn clone_shared(&self) -> ApiError {
        ApiError {
            inner: anyhow::anyhow!(self.inner.to_string()),
            status: self.status,
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(value: E) -> Self {
        Self {
            inner: value.into(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        // 4xx is the user's problem (bad input); 5xx is ours. Log
        // accordingly so the error volume on this stream is signal,
        // not someone hitting `/api/repos/--%/--`.
        //
        // On a 5xx the detailed message (DB errors, reqwest internals,
        // git stderr) goes ONLY to the tracing log — never to the client,
        // which could leak connection strings, internal hostnames, or
        // stack-ish detail. The client gets a generic body. 4xx messages
        // are user-facing validation text and are returned as-is.
        let body = if self.status.is_server_error() {
            tracing::error!(error = ?self.inner, "request failed");
            "internal error".to_string()
        } else {
            tracing::debug!(error = %self.inner, status = %self.status, "request rejected");
            self.inner.to_string()
        };
        (self.status, Json(serde_json::json!({ "error": body }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_origin_is_normalized_and_rejects_paths() {
        assert_eq!(
            normalize_frontend_origin("https://gitdebt.com/").unwrap(),
            "https://gitdebt.com"
        );
        assert!(normalize_frontend_origin("https://gitdebt.com/path").is_err());
        assert!(normalize_frontend_origin("javascript:alert(1)").is_err());
    }

    #[test]
    fn secret_comparison_checks_length_and_content() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"other"));
        assert!(!constant_time_eq(b"token", b"token-longer"));
    }

    fn test_string_cache() -> MokaCache<String, String> {
        MokaCache::builder().max_capacity(64).build()
    }

    /// The two TTL classes are distinct: pending/empty renders get the
    /// short (5-min) envelope, full renders the 24h one. Cold charts must
    /// ride the short class so a first view can't pin "no data" for a day.
    #[test]
    fn card_cache_control_short_vs_long() {
        assert_eq!(
            card_cache_control(true).to_str().unwrap(),
            "public, s-maxage=300, max-age=60"
        );
        assert_eq!(
            card_cache_control(false).to_str().unwrap(),
            "public, s-maxage=86400, max-age=3600"
        );
    }

    #[test]
    fn live_analyze_responses_are_not_cached() {
        assert_eq!(
            analyze_cache_control(true),
            HeaderValue::from_static("no-store")
        );
        assert_eq!(
            analyze_cache_control(false),
            HeaderValue::from_static("public, s-maxage=300, max-age=60")
        );
    }

    /// A cold/empty render (`RenderMiss::Pending`) is served short-TTL and
    /// is NEVER inserted into the 24h svg cache — the next request re-derives
    /// it (self-heals once the queue catches up).
    #[tokio::test]
    async fn single_flight_card_pending_is_short_ttl_and_uncached() {
        let cache = test_string_cache();
        let card = single_flight_card(&cache, "k".into(), async {
            Err::<String, _>(RenderMiss::Pending("<svg>cold</svg>".into()))
        })
        .await
        .unwrap();
        assert!(card.short_ttl, "cold render must be short-TTL");
        assert_eq!(card.svg, "<svg>cold</svg>");
        // Not memoized: the 24h cache stays empty so it self-heals.
        assert!(cache.get("k").await.is_none());
    }

    /// A full render is memoized (24h class) and coalesced: a second call
    /// with a different init returns the FIRST cached value.
    #[tokio::test]
    async fn single_flight_card_full_render_is_cached() {
        let cache = test_string_cache();
        let card = single_flight_card(&cache, "k".into(), async {
            Ok::<_, RenderMiss>("<svg>full</svg>".into())
        })
        .await
        .unwrap();
        assert!(!card.short_ttl);
        assert_eq!(cache.get("k").await.as_deref(), Some("<svg>full</svg>"));
        // Second call: cached value wins, init not re-run.
        let again = single_flight_card(&cache, "k".into(), async {
            Ok::<_, RenderMiss>("<svg>DIFFERENT</svg>".into())
        })
        .await
        .unwrap();
        assert_eq!(again.svg, "<svg>full</svg>");
    }

    /// A real failure propagates (not memoized) and reconstructs via
    /// `clone_shared` with the original status.
    #[tokio::test]
    async fn single_flight_card_failure_propagates_and_is_uncached() {
        let cache = test_string_cache();
        let err = single_flight_card(&cache, "k".into(), async {
            Err::<String, _>(RenderMiss::Failed(ApiError::bad_request("nope")))
        })
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(cache.get("k").await.is_none());
    }

    /// `single_flight` coalesces + caches the Ok body; a real error is not
    /// memoized and reconstructs with its status preserved.
    #[tokio::test]
    async fn single_flight_caches_ok_and_not_err() {
        let cache = test_string_cache();
        let body = single_flight(&cache, "k".into(), async { Ok("body".to_string()) })
            .await
            .unwrap();
        assert_eq!(body, "body");
        assert_eq!(cache.get("k").await.as_deref(), Some("body"));

        let err = single_flight(&cache, "e".into(), async {
            Err(ApiError::not_found("gone"))
        })
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert!(cache.get("e").await.is_none());
    }

    #[test]
    fn ping_unknown_always_enqueues() {
        // No complete cached set → cold fetch regardless of freshness/stars.
        assert!(ping_should_enqueue(None, Some(0), false));
        assert!(ping_should_enqueue(None, Some(9_999), true));
        // Cold + omitted count still enqueues (cold beats "no hint").
        assert!(ping_should_enqueue(None, None, true));
    }

    #[test]
    fn ping_stale_by_age_enqueues_even_if_count_matches() {
        // Known, count identical, but cache is stale by age → refresh.
        assert!(ping_should_enqueue(Some(1_000), Some(1_000), false));
    }

    #[test]
    fn ping_fresh_and_close_does_not_enqueue() {
        // Within max(50, 2%): cached 1000 → threshold = max(50, 20) = 50.
        assert!(!ping_should_enqueue(Some(1_000), Some(1_049), true));
        assert!(!ping_should_enqueue(Some(1_000), Some(951), true));
        // Exactly at the threshold is not "more than" → no enqueue.
        assert!(!ping_should_enqueue(Some(1_000), Some(1_050), true));
    }

    #[test]
    fn ping_fresh_but_big_drift_enqueues() {
        // 51 over the 50 floor → enqueue.
        assert!(ping_should_enqueue(Some(1_000), Some(1_051), true));
    }

    #[test]
    fn ping_omitted_count_on_fresh_repo_does_not_enqueue() {
        // Regression: an unreadable count is omitted (None), NOT coerced to
        // 0. A known + fresh repo with no count hint must rely on age-based
        // freshness only — never treat the missing field as max drift.
        assert!(!ping_should_enqueue(Some(1_000), None, true));
        assert!(!ping_should_enqueue(Some(500_000), None, true));
        // But a missing count on a stale-by-age repo still refreshes.
        assert!(ping_should_enqueue(Some(1_000), None, false));
    }

    #[test]
    fn ping_body_omitted_stars_deserializes_to_none() {
        // The extension omits `stars` when the DOM count is unreadable; the
        // backend must see None (not 0) so the drift logic can't force a
        // refetch of a fresh repo.
        let body: PingBody =
            serde_json::from_str(r#"{"owner":"rust-lang","repo":"rust"}"#).unwrap();
        assert_eq!(body.stars, None);
        let with_count: PingBody =
            serde_json::from_str(r#"{"owner":"a","repo":"b","stars":123}"#).unwrap();
        assert_eq!(with_count.stars, Some(123));
    }

    #[test]
    fn ping_threshold_uses_two_percent_for_large_repos() {
        // 100k stars → 2% = 2000 dominates the 50 floor.
        assert!(!ping_count_drifted(100_000, 101_500)); // within 2%
        assert!(ping_count_drifted(100_000, 103_000)); // beyond 2%
    }

    #[test]
    fn ping_threshold_uses_floor_for_small_repos() {
        // 100 stars → 2% = 2, but floor is 50, so <=50 drift is fine.
        assert!(!ping_count_drifted(100, 150));
        assert!(ping_count_drifted(100, 151));
    }

    #[test]
    fn ping_drift_is_symmetric() {
        // A drop in count (cache higher than reported) also triggers.
        assert!(ping_count_drifted(1_000, 900));
        assert!(!ping_count_drifted(1_000, 960));
    }

    #[test]
    fn range_spec_parses_and_rejects() {
        // Valid window + rebase flag.
        let s = parse_range_spec(Some("2020-01-01"), Some("2020-12-31"), Some("1")).unwrap();
        assert!(s.rebase);
        assert_eq!(s.key(), "r:2020-01-01..2020-12-31|rb:1");
        // Unset → noop spec.
        let s = parse_range_spec(None, None, None).unwrap();
        assert!(s.is_noop());
        // rebase=true also accepted; anything else is off.
        assert!(parse_range_spec(None, None, Some("true")).unwrap().rebase);
        assert!(!parse_range_spec(None, None, Some("yes")).unwrap().rebase);
        // Invalid dates and from>to are 400s.
        assert!(parse_range_spec(Some("garbage"), None, None).is_err());
        assert!(parse_range_spec(Some("2021-01-01"), Some("2020-01-01"), None).is_err());
    }

    #[test]
    fn range_spec_errors_are_bad_request() {
        let err = parse_range_spec(Some("nope"), None, None).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        let err = parse_range_spec(Some("2021-01-01"), Some("2020-01-01"), None).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn chart_query_range_spec_flows_through() {
        let q = ChartQuery {
            from: Some("2020-06-01".into()),
            to: Some("2020-06-01".into()),
            rebase: Some("1".into()),
            ..ChartQuery::default()
        };
        let spec = q.range_spec().unwrap();
        assert!(spec.rebase);
        assert_eq!(spec.key(), "r:2020-06-01..2020-06-01|rb:1");
        // Invalid range surfaces as an error from the query helper too.
        let bad = ChartQuery {
            from: Some("2020-13-99".into()),
            ..ChartQuery::default()
        };
        assert!(bad.range_spec().is_err());
    }

    #[test]
    fn chart_animation_is_explicit_and_gif_ignores_svg_flag() {
        let static_query = ChartQuery {
            motion: Some("draw".into()),
            ..ChartQuery::default()
        };
        let animated_query = ChartQuery {
            animate: Some("1".into()),
            motion: Some("DRAW".into()),
            ..ChartQuery::default()
        };
        assert!(!static_query.opts().animate);
        assert!(animated_query.opts().animate);
        assert_ne!(static_query.opts_key(), animated_query.opts_key());
        assert_eq!(
            static_query.series_opts_key(),
            animated_query.series_opts_key(),
            "animate is SVG-only and cannot fragment GIF output"
        );
        assert_eq!(static_query.gif_motion().unwrap(), "draw");
        assert_eq!(animated_query.gif_motion().unwrap(), "draw");
        assert!(ChartQuery::default().gif_motion().is_err());
    }

    #[test]
    fn gif_query_supports_existing_chart_geometry_and_range_options() {
        let q = ChartQuery {
            theme: Some("dark".into()),
            type_: Some("timeline".into()),
            log: Some("1".into()),
            from: Some("2026-01-01".into()),
            to: Some("2026-02-01".into()),
            rebase: Some("1".into()),
            motion: Some("draw".into()),
            ..ChartQuery::default()
        };
        assert!(theme_for(q.theme.as_deref()).dark);
        assert_eq!(q.opts().axis, TimeAxis::Timeline);
        assert!(q.opts().log_y);
        assert!(q.range_spec().unwrap().rebase);
        assert_eq!(q.gif_motion().unwrap(), "draw");
        assert!(
            !theme_for(ChartQuery::default().theme.as_deref()).dark,
            "GIF/SVG default theme is light"
        );
    }

    #[test]
    fn leaderboard_params_defaults() {
        let (metric, per, page) = leaderboard_params(None, None, None).unwrap();
        assert_eq!(metric, LeaderboardMetric::Stars);
        assert_eq!(per, LEADERBOARD_PER_DEFAULT);
        assert_eq!(page, 0);
    }

    #[test]
    fn leaderboard_params_accepts_both_metrics() {
        let (metric, _, _) = leaderboard_params(Some("stars"), None, None).unwrap();
        assert_eq!(metric, LeaderboardMetric::Stars);
        let (metric, _, _) = leaderboard_params(Some("velocity"), None, None).unwrap();
        assert_eq!(metric, LeaderboardMetric::Velocity);
    }

    #[test]
    fn leaderboard_params_rejects_unknown_metric() {
        // Fail loudly, never silently fall back to a different ranking.
        assert!(leaderboard_params(Some("downloads"), None, None).is_err());
        assert!(leaderboard_params(Some("STARS"), None, None).is_err());
        assert!(leaderboard_params(Some(""), None, None).is_err());
    }

    #[test]
    fn leaderboard_params_clamps_bounds() {
        // per: 1..=LEADERBOARD_PER_MAX; page: 0..=LEADERBOARD_PAGE_MAX.
        let (_, per, page) = leaderboard_params(None, Some(0), Some(-5)).unwrap();
        assert_eq!(per, 1);
        assert_eq!(page, 0);
        let (_, per, page) = leaderboard_params(None, Some(10_000), Some(i64::MAX)).unwrap();
        assert_eq!(per, LEADERBOARD_PER_MAX);
        assert_eq!(page, LEADERBOARD_PAGE_MAX);
        // Offsets computed from the clamped values can't overflow.
        assert!(page.checked_mul(per).is_some());
    }

    #[test]
    fn leaderboard_metric_key_strings_are_stable() {
        // The memo key embeds these strings; changing them silently would
        // decouple warm entries from their params.
        assert_eq!(LeaderboardMetric::Stars.as_str(), "stars");
        assert_eq!(LeaderboardMetric::Velocity.as_str(), "velocity");
    }

    #[test]
    fn card_flag_parsing() {
        assert!(flag_on(Some("1")));
        assert!(flag_on(Some("true")));
        assert!(!flag_on(Some("0")));
        assert!(!flag_on(Some("yes")));
        assert!(!flag_on(None));
    }

    #[test]
    fn card_user_options_defaults() {
        let o = CardQuery::default().user_options();
        assert!(o.show_icons); // icons default ON (unlike GRS)
        assert!(!o.hide_rank);
        assert!(!o.animate);
        assert_eq!(o.width, crate::cards::USER_CARD_DEFAULT_WIDTH);
        assert_eq!(o.number_format, crate::cards::NumberFormat::Short);
    }

    #[test]
    fn card_user_options_overrides_and_clamps() {
        let q = CardQuery {
            hide_rank: Some("1".into()),
            card_width: Some(100), // below the hide_rank floor
            show_icons: Some("0".into()),
            number_format: Some("long".into()),
            rank_icon: Some("Percentile".into()),
            ..CardQuery::default()
        };
        let o = q.user_options();
        assert!(o.hide_rank);
        assert!(o.rank_icon_percentile); // case-insensitive
        assert!(!o.show_icons);
        assert_eq!(o.width, crate::cards::USER_CARD_NORANK_WIDTH); // clamped up
        assert_eq!(o.number_format, crate::cards::NumberFormat::Long);
    }

    #[test]
    fn card_repo_options_width_clamp() {
        let q = CardQuery {
            card_width: Some(10_000),
            ..CardQuery::default()
        };
        assert_eq!(q.repo_options().width, 800); // repo-card max
        assert_eq!(
            CardQuery::default().repo_options().width,
            crate::cards::REPO_CARD_DEFAULT_WIDTH
        );
    }

    #[test]
    fn og_star_total_uses_metadata_when_timeline_is_unavailable() {
        assert_eq!(best_og_star_total(&[], Some(246_580)), 246_580);
        assert_eq!(best_og_star_total(&[], Some(-1)), 0);

        let series = vec![Point {
            at: chrono::DateTime::UNIX_EPOCH,
            stars: 120,
        }];
        assert_eq!(best_og_star_total(&series, Some(125)), 125);
        assert_eq!(best_og_star_total(&series, Some(100)), 120);
    }

    #[test]
    fn card_key_fragment_is_injective_and_theme_aware() {
        // A crafted hide= value that embeds another param's spelling must
        // not collide with the honestly-split query (length prefixes make
        // the encoding injective).
        let crafted = CardQuery {
            hide: Some("a|s:1:b".into()),
            ..CardQuery::default()
        };
        let honest = CardQuery {
            hide: Some("a".into()),
            show: Some("b".into()),
            ..CardQuery::default()
        };
        let t = &crate::theme::LIGHT;
        assert_ne!(crafted.key_fragment(t), honest.key_fragment(t));
        // Theme flips the key; identical queries share it.
        let q = CardQuery::default();
        assert_ne!(
            q.key_fragment(&crate::theme::LIGHT),
            q.key_fragment(&crate::theme::DARK)
        );
        assert_eq!(q.key_fragment(t), q.key_fragment(t));
    }

    #[test]
    fn cidr_parses_and_matches_v4() {
        let c = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(c.contains("10.1.2.3".parse().unwrap()));
        assert!(c.contains("10.255.255.255".parse().unwrap()));
        assert!(!c.contains("11.0.0.1".parse().unwrap()));
        assert!(!c.contains("192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn cidr_bare_address_is_host_route() {
        let c = Cidr::parse("127.0.0.1").unwrap();
        assert!(c.contains("127.0.0.1".parse().unwrap()));
        assert!(!c.contains("127.0.0.2".parse().unwrap()));
    }

    #[test]
    fn cidr_parses_v6() {
        let c = Cidr::parse("2606:4700::/32").unwrap();
        assert!(c.contains("2606:4700:1::1".parse().unwrap()));
        assert!(!c.contains("2400:cb00::1".parse().unwrap()));
    }

    #[test]
    fn cidr_rejects_garbage() {
        assert!(Cidr::parse("").is_none());
        assert!(Cidr::parse("not-an-ip").is_none());
        assert!(Cidr::parse("10.0.0.0/99").is_none());
        assert!(Cidr::parse("10.0.0.0/abc").is_none());
    }

    #[test]
    fn v4_prefix_match_zero_is_everything() {
        assert!(v4_prefix_match(
            "0.0.0.0".parse().unwrap(),
            "203.0.113.9".parse().unwrap(),
            0
        ));
    }

    #[test]
    fn peer_is_trusted_honors_defaults() {
        // With TRUSTED_PROXIES unset the defaults apply: loopback +
        // RFC1918 + Cloudflare ranges trusted; a random public IP isn't.
        // (This test reads the process-wide OnceLock; it asserts only on
        // the default set, which is stable when the env var is absent.)
        if std::env::var("TRUSTED_PROXIES").is_err() {
            assert!(peer_is_trusted("127.0.0.1".parse().unwrap()));
            assert!(peer_is_trusted("10.1.2.3".parse().unwrap()));
            assert!(peer_is_trusted("192.168.0.5".parse().unwrap()));
            // A Cloudflare range.
            assert!(peer_is_trusted("104.16.0.1".parse().unwrap()));
            // A random public IP is NOT a trusted proxy.
            assert!(!peer_is_trusted("203.0.113.9".parse().unwrap()));
            // v4-mapped loopback normalizes to v4 and matches.
            assert!(peer_is_trusted("::ffff:127.0.0.1".parse().unwrap()));
        }
    }

    #[test]
    fn expected_tombstones_do_not_degrade_readiness() {
        let mut pipeline = PipelineSignals {
            histories_complete: 0,
            histories_pending: 0,
            star_jobs_active: 0,
            star_jobs_retrying: 0,
            star_jobs_provider_delayed: 0,
            star_jobs_dead: 0,
            star_jobs_tombstoned: 1,
            analysis_jobs_active: 0,
            analysis_jobs_retrying: 0,
            analysis_jobs_dead: 0,
            analysis_jobs_tombstoned: 1,
            oldest_star_job_seconds: 0,
            oldest_analysis_job_seconds: 0,
            last_archive_hour: None,
        };
        assert!(!pipeline.degraded());

        pipeline.star_jobs_dead = 1;
        assert!(pipeline.degraded());
    }
}
