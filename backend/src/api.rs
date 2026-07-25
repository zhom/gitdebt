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
use axum_extra::extract::cookie::CookieJar;

use axum::error_handling::HandleErrorLayer;
use axum::middleware::Next;
use moka::future::Cache as MokaCache;
use tower::BoxError;
use tower::limit::GlobalConcurrencyLimitLayer;
use tower::load_shed::LoadShedLayer;
use tower_http::compression::CompressionLayer;
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
use crate::redis::{Decision, HttpLimiter, RedisHandle, WindowLimit};
use crate::repo_endpoints::is_valid_slug;
use crate::streak::{CommitStreak, summarize_commit_streak};
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
    /// The underlying analyze pass aggregates complete history to UTC days
    /// in Postgres, then downsamples the result. Caching still absorbs the
    /// aggregate query and serialization on repeat hits.
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
    /// Byte-weighed (entries range from ~10 KB badges to MB-scale GIFs),
    /// capped at [`RASTER_CACHE_MAX_BYTES`].
    pub raster_cache: MokaCache<String, std::sync::Arc<Vec<u8>>>,
    /// Self-contained avatar data URIs used by contributor media. SVGs loaded
    /// through an `<img>` and the server-side rasterizer cannot reliably load
    /// remote subresources, so the first trusted CDN read is cached here and
    /// baked into every SVG/PNG/WebP variant.
    pub(crate) avatar_data_cache: MokaCache<String, String>,
    pub(crate) avatar_http: reqwest::Client,
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
    /// Bare-clone storage. Written by the worker's repo-analysis pool; the
    /// usage endpoint reuses it (read-only) to read package manifests out of
    /// existing clones (never clones itself; a missing clone or an absent
    /// volume means no manifest-backed package association is shown).
    pub storage: std::sync::Arc<crate::repo_history::RepoStorage>,
    /// Shared Redis for the distributed admission limiter and the cache
    /// invalidation bus. `None` (debug builds only) falls back to
    /// per-process limiting and local-only eviction.
    pub redis: Option<std::sync::Arc<RedisHandle>>,
}

/// RAM budgets for the byte-holding moka caches. These four caches hold
/// rendered bodies whose sizes vary by orders of magnitude (a badge SVG is
/// ~10 KB, a wave GIF can exceed 1 MB), so they are bounded by **weighed
/// bytes**, not entry counts — `max_capacity` is a byte budget and each
/// entry weighs its value's byte length. The small JSON/aggregate caches
/// (analyze, user-agg, leaderboard) stay entry-count-bounded.
const SVG_CACHE_MAX_BYTES: u64 = 32 * 1024 * 1024;
const STAT_SVG_CACHE_MAX_BYTES: u64 = 32 * 1024 * 1024;
const RASTER_CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;
const AVATAR_CACHE_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// moka weigher unit: clamp a body length into the non-zero `u32` weight
/// moka expects (a zero weight would make an entry free; an over-4GiB body
/// cannot happen but saturates instead of wrapping).
fn byte_weight(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX).max(1)
}

/// Byte-weighed string cache (SVG bodies, avatar data URIs).
fn weighted_string_cache(max_bytes: u64, ttl: Duration) -> MokaCache<String, String> {
    MokaCache::builder()
        .max_capacity(max_bytes)
        .weigher(|_key: &String, value: &String| byte_weight(value.len()))
        .time_to_live(ttl)
        .build()
}

/// Byte-weighed raster cache (PNG/WebP/GIF bodies).
fn weighted_bytes_cache(
    max_bytes: u64,
    ttl: Duration,
) -> MokaCache<String, std::sync::Arc<Vec<u8>>> {
    MokaCache::builder()
        .max_capacity(max_bytes)
        .weigher(|_key: &String, value: &std::sync::Arc<Vec<u8>>| byte_weight(value.len()))
        .time_to_live(ttl)
        .build()
}

impl ApiState {
    pub fn new(
        analyzer: AnalyzerCtx,
        gh_app: Option<GithubAppConfig>,
        storage: std::sync::Arc<crate::repo_history::RepoStorage>,
        redis: Option<std::sync::Arc<RedisHandle>>,
    ) -> anyhow::Result<Self> {
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
        Self::with_settings(
            analyzer,
            gh_app,
            storage,
            redis,
            frontend_origin,
            metrics_token,
        )
    }

    /// `new` with the deployment settings supplied directly instead of read
    /// from the environment. Tests use this so they exercise the same state
    /// in debug and release builds, where `new` demands real deployment
    /// values.
    #[allow(clippy::too_many_arguments)]
    pub fn with_settings(
        analyzer: AnalyzerCtx,
        gh_app: Option<GithubAppConfig>,
        storage: std::sync::Arc<crate::repo_history::RepoStorage>,
        redis: Option<std::sync::Arc<RedisHandle>>,
        frontend_origin: String,
        metrics_token: Option<String>,
    ) -> anyhow::Result<Self> {
        let day = Duration::from_secs(24 * 60 * 60);
        let svg_cache = weighted_string_cache(SVG_CACHE_MAX_BYTES, day);
        let analyze_cache = MokaCache::builder()
            .max_capacity(500)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();
        let stat_svg_cache = weighted_string_cache(STAT_SVG_CACHE_MAX_BYTES, day);
        // Raster cache: the largest byte budget because PNGs/GIFs are
        // ~10–100× larger than their source SVGs. Raster bytes are
        // deterministic so re-rasterization on miss is always safe.
        let raster_cache = weighted_bytes_cache(RASTER_CACHE_MAX_BYTES, day);
        let avatar_data_cache = weighted_string_cache(AVATAR_CACHE_MAX_BYTES, day);
        let avatar_http = reqwest::Client::builder()
            .user_agent("gitdebt-avatar-media/1")
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
            .build()?;
        let user_agg_cache = MokaCache::builder()
            .max_capacity(500)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();
        let leaderboard_cache = MokaCache::builder()
            .max_capacity(1_000)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();
        Ok(Self {
            analyzer,
            svg_cache,
            analyze_cache,
            stat_svg_cache,
            raster_cache,
            avatar_data_cache,
            avatar_http,
            user_agg_cache,
            leaderboard_cache,
            gh_app,
            frontend_origin,
            metrics_token,
            storage,
            redis,
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
    let analyze_limiter = HttpLimiter::shared(
        WindowLimit::per_second("analyze", 2, 20),
        state.redis.clone(),
    );
    let analyze = Router::new()
        .route("/api/repos/{owner}/{repo}/analyze", get(analyze))
        .route("/api/repos/{owner}/{repo}/stars.csv", get(stars_csv))
        .route("/api/repos/{owner}/{repo}/stars.json", get(stars_json))
        .route(
            "/api/repos/{owner}/{repo}/earned-badges.json",
            get(earned_badges_json),
        )
        .route("/api/users/{login}/analyze", get(user_analyze))
        .route("/api/leaderboard.json", get(leaderboard_json))
        .route("/api/activity.json", get(platform_activity))
        .route("/api/sitemap/repos", get(sitemap_repos))
        .layer(axum::middleware::from_fn_with_state(
            analyze_limiter.clone(),
            admission,
        ));

    // SSE is deliberately outside the global 60-second request timeout.
    // The handler owns a five-minute lifetime, heartbeats, and a process-wide
    // connection cap; it shares the analyze admission budget because it
    // polls the same ingestion state.
    let progress = Router::new()
        .route(
            "/api/repos/{owner}/{repo}/progress",
            get(crate::progress::repo_progress),
        )
        .route(
            "/api/repos/{owner}/{repo}/progress.json",
            get(crate::progress::repo_progress_snapshot),
        )
        .route(
            "/api/users/{login}/progress",
            get(crate::progress::profile_progress),
        )
        .layer(axum::middleware::from_fn_with_state(
            analyze_limiter,
            admission,
        ))
        .layer(public_cors.clone());

    // Render parameters create an unbounded cache-key space, so even
    // edge-cached images need an origin-side per-IP ceiling.
    let images_limiter = HttpLimiter::shared(
        WindowLimit::per_second("images", 10, 60),
        state.redis.clone(),
    );
    let images = Router::new()
        .route("/api/repos/{owner}/{repo}/chart.svg", get(chart))
        .route("/api/repos/{owner}/{repo}/chart.png", get(chart_png))
        .route("/api/repos/{owner}/{repo}/chart.webp", get(chart_webp))
        .route("/api/repos/{owner}/{repo}/chart.gif", get(chart_gif))
        .route("/api/chart.svg", get(multi_chart))
        .route("/api/chart.png", get(multi_chart_png))
        .route("/api/chart.webp", get(multi_chart_webp))
        .route("/api/chart.gif", get(multi_chart_gif))
        .route("/api/users/{login}/chart.svg", get(user_chart))
        .route("/api/users/{login}/chart.png", get(user_chart_png))
        .route("/api/users/{login}/chart.webp", get(user_chart_webp))
        .route("/api/users/{login}/chart.gif", get(user_chart_gif))
        .route("/api/repos/{owner}/{repo}/usage", get(usage_json))
        .route("/api/repos/{owner}/{repo}/usage.svg", get(usage_svg))
        .route("/api/repos/{owner}/{repo}/usage.png", get(usage_png))
        .route("/api/repos/{owner}/{repo}/usage.webp", get(usage_webp))
        .route("/api/repos/{owner}/{repo}/badge.svg", get(badge_svg))
        .route("/api/repos/{owner}/{repo}/badge.png", get(badge_png))
        .route("/api/repos/{owner}/{repo}/badge.webp", get(badge_webp))
        // Profile signals sit with the media budget, not the analyze
        // budget: they read Postgres and enqueue nothing, exactly like the
        // per-repo `stats.json` in `repo_endpoints::public_router`.
        .route("/api/users/{login}/stats.json", get(user_stats_json))
        .route(
            "/api/users/{login}/stats/{filename}",
            get(user_stat_dispatcher),
        )
        .route("/api/users/{login}/card.svg", get(user_card_svg))
        .route("/api/users/{login}/card.png", get(user_card_png))
        .route("/api/users/{login}/card.webp", get(user_card_webp))
        .route("/api/users/{login}/card.gif", get(user_card_gif))
        .route("/api/repos/{owner}/{repo}/card.svg", get(repo_card_svg))
        .route("/api/repos/{owner}/{repo}/card.png", get(repo_card_png))
        .route("/api/repos/{owner}/{repo}/card.webp", get(repo_card_webp))
        .route("/api/repos/{owner}/{repo}/card.gif", get(repo_card_gif))
        .route("/api/repos/{owner}/{repo}/og.png", get(repo_og_png))
        .route("/api/repos/{owner}/{repo}/og.webp", get(repo_og_webp))
        .route("/api/users/{login}/og.png", get(user_og_png))
        .route("/api/users/{login}/og.webp", get(user_og_webp))
        .route("/api/og.png", get(site_og_png))
        .route("/api/og.webp", get(site_og_webp))
        .merge(crate::repo_endpoints::public_router())
        .layer(axum::middleware::from_fn_with_state(
            images_limiter,
            admission,
        ));

    // Extension origins vary by browser/install. The endpoint accepts no
    // credentials and has its own limiter because it can enqueue work.
    let ext_cors = CorsLayer::new()
        .allow_methods([Method::POST])
        .allow_origin(Any)
        .allow_headers(Any)
        .max_age(Duration::from_secs(60 * 60));
    let ext_limiter =
        HttpLimiter::shared(WindowLimit::per_second("ext", 1, 10), state.redis.clone());
    let ext = Router::new()
        .route("/api/ext/ping", axum::routing::post(ext_ping))
        .layer(axum::middleware::from_fn_with_state(ext_limiter, admission))
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
    let mutating_limiter = HttpLimiter::shared(
        WindowLimit::per_second("mutating", 1, 5),
        state.redis.clone(),
    );
    let rate_limited = Router::new()
        .merge(crate::repo_endpoints::mutating_router())
        .route(
            "/api/users/{login}/warm",
            axum::routing::post(warm_user_profile),
        )
        .layer(axum::middleware::from_fn_with_state(
            mutating_limiter,
            admission,
        ))
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
        ))
        // Text bodies only: the default predicate skips already-compressed
        // image types and event streams, so rasters are never re-encoded on
        // a CPU-constrained host. SVG charts, analyze JSON, and the CSV/JSON
        // exports compress several-fold, and the extension fetches the
        // analyze body on every repository page a user opens.
        .layer(CompressionLayer::new());

    Router::new()
        .merge(timed)
        .merge(progress)
        .with_state(state)
        // Shed load instead of queueing it. A saturated raster path otherwise
        // accumulates accepted requests for the full 60-second timeout, each
        // holding its rendered body; the queue, not the CPU, is what turns a
        // burst into an out-of-memory restart.
        .layer(
            tower::ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|_: BoxError| async {
                    let mut headers = HeaderMap::new();
                    headers.insert(header::RETRY_AFTER, HeaderValue::from_static("2"));
                    (StatusCode::SERVICE_UNAVAILABLE, headers, "server busy")
                }))
                .layer(LoadShedLayer::new())
                .layer(GlobalConcurrencyLimitLayer::new(max_inflight_requests())),
        )
        .layer(TraceLayer::new_for_http())
}

/// Ceiling on requests being served at once, above which the tier sheds
/// rather than queues. Sized from the visible CPUs by default because the
/// expensive request classes are CPU-bound (rasterization) or
/// Postgres-bound, and both degrade worse when oversubscribed than when
/// callers are told to retry.
fn max_inflight_requests() -> usize {
    std::env::var("MAX_INFLIGHT_REQUESTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|cpus| cpus.get() * 64)
                .unwrap_or(512)
        })
        .clamp(32, 8_192)
}

/// Admission middleware shared by the four rate-limited route classes.
/// Client identity comes from [`CloudflareIpKeyExtractor`] (forwarding
/// headers honored only from trusted proxies); the budget check runs
/// against the shared limiter backend (Redis, or in-process in debug
/// builds without `REDIS_URL`). Limiter unavailability admits the request.
async fn admission(
    State(limiter): State<std::sync::Arc<HttpLimiter>>,
    request: axum::extract::Request,
    next: Next,
) -> axum::response::Response {
    let key = CloudflareIpKeyExtractor
        .extract(&request)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    match limiter.check(&key).await {
        Decision::Allow => next.run(request).await,
        Decision::Deny { retry_after_secs } => too_many_requests(retry_after_secs),
    }
}

/// 429 with the same envelope tower_governor produced: `retry-after` and
/// `x-ratelimit-after` in seconds plus the plain-text wait hint.
fn too_many_requests(retry_after_secs: u64) -> axum::response::Response {
    let wait = HeaderValue::from_str(&retry_after_secs.to_string())
        .unwrap_or_else(|_| HeaderValue::from_static("1"));
    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-after", wait.clone());
    headers.insert(header::RETRY_AFTER, wait);
    (
        StatusCode::TOO_MANY_REQUESTS,
        headers,
        format!("Too Many Requests! Wait for {retry_after_secs}s"),
    )
        .into_response()
}

/// Subscribe to the Redis invalidation channel and evict the published
/// keys from THIS replica's local moka caches. Reconnects forever; a lost
/// subscription only means stale entries age out by TTL, exactly the
/// pre-Redis behavior. No-op without Redis.
pub fn spawn_invalidation_listener(state: &ApiState) {
    let Some(redis) = state.redis.clone() else {
        return;
    };
    let user_agg_cache = state.user_agg_cache.clone();
    let analyze_cache = state.analyze_cache.clone();
    tokio::spawn(async move {
        use futures::StreamExt;
        loop {
            let mut pubsub = match redis.pubsub().await {
                Ok(pubsub) => pubsub,
                Err(error) => {
                    tracing::debug!(%error, "invalidation subscriber: connect failed");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            if let Err(error) = pubsub.subscribe(crate::redis::INVALIDATION_CHANNEL).await {
                tracing::warn!(%error, "invalidation subscriber: subscribe failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            tracing::info!("cache invalidation subscriber connected");
            let mut messages = pubsub.on_message();
            while let Some(message) = messages.next().await {
                let Ok(payload) = message.get_payload::<String>() else {
                    continue;
                };
                let Ok(invalidation) = serde_json::from_str::<crate::redis::Invalidation>(&payload)
                else {
                    continue;
                };
                for key in &invalidation.user_agg {
                    user_agg_cache.invalidate(key).await;
                }
                for key in &invalidation.analyze {
                    analyze_cache.invalidate(key).await;
                }
            }
            // Stream ended: the pub/sub connection dropped. Reconnect.
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Debug, Serialize)]
struct PipelineSignals {
    worker_online: bool,
    worker_last_seen: Option<DateTime<Utc>>,
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
        (!self.worker_online && (self.star_jobs_active > 0 || self.analysis_jobs_active > 0))
            || self.star_jobs_provider_delayed > 0
            || self.star_jobs_dead > 0
            || self.analysis_jobs_dead > 0
    }
}

async fn load_pipeline_signals(db: &crate::db::Db) -> Result<PipelineSignals, sqlx::Error> {
    let row = sqlx::query(
        "SELECT \
            EXISTS(SELECT 1 FROM service_heartbeats \
                WHERE service = 'worker' AND seen_at >= NOW() - INTERVAL '45 seconds') \
                AS worker_online, \
            (SELECT MAX(seen_at) FROM service_heartbeats WHERE service = 'worker') \
                AS worker_last_seen, \
            (SELECT COUNT(*)::BIGINT FROM repos WHERE history_complete = TRUE \
                AND missing = FALSE AND metadata_fetched_at IS NOT NULL) \
                AS histories_complete, \
            (SELECT COUNT(*)::BIGINT FROM repos WHERE history_complete = FALSE \
                AND missing = FALSE AND metadata_fetched_at IS NOT NULL) \
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
        worker_online: row.try_get("worker_online")?,
        worker_last_seen: row.try_get("worker_last_seen")?,
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
/// Readiness for orchestrator probes: can this process reach Postgres.
///
/// Deliberately a single primary-key-free `SELECT 1` rather than the pipeline
/// aggregate it used to run. Probes fire on a fixed cadence from every
/// replica and from any external monitor, so a probe that costs several
/// aggregate scans gets slower exactly when the database is loaded, and takes
/// the deployment down at the moment it is least able to absorb it. The
/// pipeline detail lives on the token-gated `/metrics`.
async fn ready(State(state): State<ApiState>) -> impl IntoResponse {
    let db = state.analyzer.cache.db();
    let no_store = {
        let mut h = HeaderMap::new();
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        h
    };
    match sqlx::query("SELECT 1").execute(&db.pool).await {
        Ok(_) => (
            StatusCode::OK,
            no_store,
            Json(serde_json::json!({ "ready": true })),
        ),
        Err(e) => {
            tracing::error!(error = %e, "readiness check failed: database unavailable");
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

    // In-memory cache occupancy (approximate; moka counts lazily). Entry
    // counts churning on a param-derived cache mean a cache-busting client
    // is evicting warm entries. The byte-holding caches are additionally
    // byte-weighed (`cache_bytes` vs their `*_MAX_BYTES` budgets) — RAM
    // use is bounded even when entries are MB-scale GIFs.
    let cache_entries = serde_json::json!({
        "svg": state.svg_cache.entry_count(),
        "analyze": state.analyze_cache.entry_count(),
        "stat_svg": state.stat_svg_cache.entry_count(),
        "raster": state.raster_cache.entry_count(),
        "avatar": state.avatar_data_cache.entry_count(),
        "user_agg": state.user_agg_cache.entry_count(),
        "leaderboard": state.leaderboard_cache.entry_count(),
    });
    let cache_bytes = serde_json::json!({
        "svg": state.svg_cache.weighted_size(),
        "svg_max": SVG_CACHE_MAX_BYTES,
        "stat_svg": state.stat_svg_cache.weighted_size(),
        "stat_svg_max": STAT_SVG_CACHE_MAX_BYTES,
        "raster": state.raster_cache.weighted_size(),
        "raster_max": RASTER_CACHE_MAX_BYTES,
        "avatar": state.avatar_data_cache.weighted_size(),
        "avatar_max": AVATAR_CACHE_MAX_BYTES,
    });

    let db_pool = serde_json::json!({
        "max": db.pool.options().get_max_connections(),
        "size": db.pool.size(),
        "idle": db.pool.num_idle(),
    });

    let body = serde_json::json!({
        "github_budget": github_budget,
        "star_fetch_queue": star_queue,
        "repo_analysis_queue": analysis_queue,
        "degraded": pipeline.degraded(),
        "pipeline": pipeline,
        // Pool saturation is the failure mode a co-tenant database reaches
        // first, and it is invisible from the outside: every surface just
        // starts returning 500 when `acquire` times out.
        "db_pool": db_pool,
        "raster": {
            "permits_total": RASTER_CONCURRENCY,
            "permits_available": raster_available,
        },
        // Fail-open admissions signal a Redis outage: requests were allowed
        // without a shared budget check. Sustained growth here means the
        // distributed limiter is running blind.
        "rate_limiter": {
            "backend": if state.redis.is_some() { "redis" } else { "memory" },
            "fail_open_total": crate::redis::limiter_fail_open_total(),
        },
        "progress_streams": {
            "connections_limit": progress_total,
            "connections_active": progress_total.saturating_sub(progress_available),
        },
        "cache_entries": cache_entries,
        "cache_bytes": cache_bytes,
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
    let flight_key = if enqueue {
        key
    } else {
        format!("readonly:{key}")
    };
    let (json, live) = single_flight_analyze(&state.analyze_cache, flight_key, async {
        let result = if enqueue {
            analyze_repo(&owner, &repo, &state.analyzer).await?
        } else {
            crate::analyzer::analyze_repo_readonly(&owner, &repo, &state.analyzer).await?
        };
        let live = result.pending || result.backfilling;
        let json = serde_json::to_string(&result)?;
        Ok((json, live))
    })
    .await?;
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
    let public = summary
        .as_ref()
        .is_some_and(|s| !s.missing && s.metadata_fetched_at.is_some());
    let complete = public && summary.as_ref().is_some_and(|s| s.stargazers_complete);
    if !complete {
        // No trustworthy history yet: empty series, best-effort headline
        // total from the denormalized metadata count (0 when truly cold).
        let total_stars = summary
            .as_ref()
            .filter(|_| public)
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
/// `{login,repos_included,repos_pending,repos_analyzed,repos_analyzing,`
/// `total_stars,history:[{date,stars}]}`,
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
        // Memoized like the enqueueing branch, under a distinct key. The
        // aggregate build is the heaviest read in the codebase (a bucketed
        // GROUP BY across a login's repositories), and this variant is what
        // the static build and any crawler walking `?enqueue=0` links hits.
        let key = format!("user-readonly:{}", login.to_ascii_lowercase());
        if let Some(json) = state.analyze_cache.get(&key).await {
            json
        } else {
            let agg = aggregate::build_readonly(&state.analyzer, &login)
                .await
                .map_err(map_aggregate_err)?;
            let json = serde_json::to_string(&agg.to_json())?;
            state.analyze_cache.insert(key, json.clone()).await;
            json
        }
    } else {
        let key = format!("user:{}", login.to_ascii_lowercase());
        if let Some(json) = state.analyze_cache.get(&key).await {
            json
        } else {
            let agg = build_user_aggregate(&state, &login).await?;
            let json = serde_json::to_string(&agg.to_json())?;
            if agg.repos_pending == 0 && agg.repos_analyzing == 0 {
                state.analyze_cache.insert(key, json.clone()).await;
            }
            json
        }
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
    if let Some(aggregate) = state.user_agg_cache.get(&key).await {
        return Ok(aggregate);
    }
    let aggregate = std::sync::Arc::new(
        aggregate::build(&state.analyzer, &key)
            .await
            .map_err(map_aggregate_err)?,
    );
    // A pending aggregate changes as workers land. Caching it for five
    // minutes made a fast backend look frozen to the profile UI.
    if aggregate.repos_pending == 0 && aggregate.repos_analyzing == 0 {
        state.user_agg_cache.insert(key, aggregate.clone()).await;
    }
    Ok(aggregate)
}

/// Credentialed self-profile warm-up. The session login must match the path;
/// an authenticated visitor cannot spend their token or interactive queue
/// priority on somebody else's account.
async fn warm_user_profile(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    jar: CookieJar,
) -> Result<impl IntoResponse, ApiError> {
    if !aggregate::is_valid_login(&login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let config = state
        .gh_app
        .as_ref()
        .ok_or_else(|| ApiError::unavailable("authentication unavailable"))?;
    let user_id = crate::auth::current_user_id(config, &jar)
        .ok_or_else(|| ApiError::unauthorized("sign in required"))?;
    let stored_login: Option<String> =
        sqlx::query_scalar("SELECT login FROM app_users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.analyzer.cache.db().pool)
            .await?;
    let login = login.to_ascii_lowercase();
    if !stored_login.is_some_and(|value| value.eq_ignore_ascii_case(&login)) {
        return Err(ApiError::unauthorized("profile does not match session"));
    }

    let github =
        match crate::auth::user_access_token(state.analyzer.cache.db(), config, user_id).await? {
            Some(token) => state
                .analyzer
                .github
                .for_user_token(&token)
                .map(std::sync::Arc::new)
                .unwrap_or_else(|_| state.analyzer.github.clone()),
            None => state.analyzer.github.clone(),
        };
    let aggregate = aggregate::build_for_user(&state.analyzer, &login, user_id, github)
        .await
        .map_err(map_aggregate_err)?;

    state.user_agg_cache.invalidate(&login).await;
    // The profile report memo shares `analyze_cache` under a distinct key
    // prefix; a warm-up refreshes the owned-repo set, so it must drop too
    // or the report keeps serving the pre-warm totals for its whole TTL.
    let analyze_keys = vec![format!("user:{login}"), format!("user-stats:{login}")];
    for key in &analyze_keys {
        state.analyze_cache.invalidate(key).await;
    }
    // Other replicas hold their own moka caches; publish the evicted keys
    // on the invalidation bus so they drop the same entries. Fire-and-forget
    // — a lost message degrades to TTL staleness, never blocks the response.
    if let Some(redis) = &state.redis {
        crate::redis::publish_invalidation(
            redis,
            crate::redis::Invalidation {
                user_agg: vec![login.clone()],
                analyze: analyze_keys,
            },
        );
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok((headers, Json(aggregate.to_json())))
}

// Profile-level code signals (Postgres only)

/// Known automation accounts that don't carry `[bot]` in the commit
/// author. Kept alongside the SQL fragment below so the profile-level
/// maintenance signal excludes the same population as the per-repo bus
/// factor without reaching across module boundaries.
const PROFILE_BOT_LOGINS: &[&str] = &[
    "dependabot",
    "dependabot-preview",
    "renovate",
    "renovate-bot",
    "mergify",
    "imgbot",
    "allcontributors",
    "pre-commit-ci",
    "github-actions",
    "claude",
    "claude-code",
    "anthropic",
    "cursor",
    "copilot",
    "gitkraken",
    "snyk-bot",
    "stale",
    "deepsource-autofix",
    "codacy-badger",
    "scout-bot",
    "lgtm-com",
];

/// `repo_author_stats` bot exclusion for the profile aggregate. `$2` is
/// bound to [`PROFILE_BOT_LOGINS`].
const PROFILE_NON_BOT_AUTHOR: &str = "author.author_name NOT LIKE '%[bot]%' \
       AND author.author_email NOT LIKE '%[bot]@%' \
       AND (author.github_login IS NULL OR author.github_login NOT LIKE '%[bot]%') \
       AND COALESCE(author.github_login, '') <> ALL($2::text[])";

/// How many of a login's owned repositories the Postgres-only code signals
/// fan out over, most-starred first.
///
/// An organization can own thousands of tracked repositories, and every
/// code signal (commit heatmap, activity ranking, language footprint, bus
/// factor) is a group-by across all of them. Left unbounded, one profile
/// view of a large organization reads millions of daily-commit and
/// per-author rows — work that grows with the account rather than with the
/// page. Two hundred keeps every one of those queries an index scan over a
/// bounded slug set while still covering every repository of all but a
/// handful of accounts; past it, the marginal repository moves a rendered
/// signal by less than a pixel. The account-wide totals
/// (`repos_tracked`/`total_stars`/`total_forks`) stay uncapped, and
/// `repos_scanned` reports the covered slice so a capped profile states its
/// coverage instead of presenting a slice as the whole account.
const PROFILE_MAX_REPOS: i64 = 200;

/// Rolling window for the "recent commit volume" ranking, in days.
const PROFILE_ACTIVE_WINDOW_DAYS: i64 = 90;
/// Rolling window for the aggregated commit heatmap: 52 weeks back from
/// the Monday of the current week, matching the per-repo heatmap.
const PROFILE_HEATMAP_WEEKS: i64 = 52;

#[derive(Debug, Clone, Serialize)]
struct UserLanguage {
    language: String,
    files: i64,
    code: i64,
    blank: i64,
    comment: i64,
}

#[derive(Debug, Clone, Serialize)]
struct UserRepoRow {
    repo: String,
    stars: i64,
    forks: i64,
    /// Analyzed commits over the repo's whole history; `0` until the
    /// clone analysis has completed a pass.
    commits: i64,
    /// Commits landed inside [`PROFILE_ACTIVE_WINDOW_DAYS`].
    commits_recent: i64,
    /// Cumulative monthly star totals. Empty unless the repo's star
    /// history is complete — readers never plot partial history.
    spark: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct UserDay {
    date: chrono::NaiveDate,
    value: i64,
}

#[derive(Debug, Clone, Serialize)]
struct UserVisionaryRepo {
    repo: String,
    current_stars: i64,
    stars_at_first_contribution: i64,
    first_contribution_at: DateTime<Utc>,
    owned: bool,
}

/// Everything the profile report renders, derived exclusively from
/// Postgres: `repos`, `repo_history`, `repo_author_stats`,
/// `repo_commit_days` and `repo_lines`. No GitHub call is on this path.
#[derive(Debug, Clone, Serialize)]
struct UserStats {
    login: String,
    /// At least one owned repo has a completed analysis pass. Readers
    /// gate every code-derived number on this.
    ready: bool,
    repos_tracked: i64,
    /// Owned repositories the code signals below actually cover, capped at
    /// [`PROFILE_MAX_REPOS`]. Equal to `repos_tracked` for every account
    /// under the cap; smaller for a large organization, which is what lets
    /// a reader say "top 200 of 2,913" instead of implying full coverage.
    repos_scanned: i64,
    repos_analyzed: i64,
    total_stars: i64,
    total_forks: i64,
    /// Commits authored by this login across every tracked repo.
    authored_commits: i64,
    /// Distinct tracked repos this login authored commits in.
    contributed_repos: i64,
    owned_contributed_repos: i64,
    external_contributed_repos: i64,
    owned_authored_commits: i64,
    external_authored_commits: i64,
    visionary_repos: Vec<UserVisionaryRepo>,
    /// Commits analyzed across the login's owned repos.
    analyzed_commits: i64,
    since_year: Option<i32>,
    /// Owned analyzed repos where one person carries more than half of
    /// the non-bot authorship, and the complement.
    solo_maintained: i64,
    shared_maintained: i64,
    languages: Vec<UserLanguage>,
    top_repos: Vec<UserRepoRow>,
    active_repos: Vec<UserRepoRow>,
    commit_days: Vec<UserDay>,
    /// Consecutive calendar days authored by this resolved GitHub login across
    /// analyzed public repositories. The full tier ladder is public; the UI
    /// only reveals unearned goals to the signed-in owner.
    commit_streak: CommitStreak,
}

/// The bounded owned-repository set every profile code signal reads:
/// publicly-proven, untombstoned repos owned by `login`, most-starred first
/// then slug, capped at [`PROFILE_MAX_REPOS`]. The tie-break keeps a
/// rendered profile byte-identical for a given database state.
///
/// Resolved once per profile render — one owner-prefix pass over `repos`,
/// served by `idx_repos_repo_prefix` — and then bound as a slug array, so
/// the downstream queries become per-repo index lookups instead of
/// owner-prefix scans over `repo_commit_days`, `repo_lines` and
/// `repo_author_stats`, the three tables that grow with commit history
/// rather than with repository count.
async fn load_profile_scope(pool: &sqlx::PgPool, login: &str) -> Result<Vec<String>, ApiError> {
    let rows = sqlx::query(
        "SELECT repo FROM repos \
         WHERE repo LIKE $1 || '/%' \
           AND NOT missing \
           AND metadata_fetched_at IS NOT NULL \
         ORDER BY star_count DESC NULLS LAST, repo ASC \
         LIMIT $2",
    )
    .bind(login)
    .bind(PROFILE_MAX_REPOS)
    .fetch_all(pool)
    .await?;
    let mut repos = Vec::with_capacity(rows.len());
    for row in &rows {
        repos.push(row.try_get::<String, _>("repo")?);
    }
    Ok(repos)
}

async fn load_visionary_repos(
    pool: &sqlx::PgPool,
    login: &str,
) -> Result<Vec<UserVisionaryRepo>, ApiError> {
    let rows = sqlx::query(
        "WITH contributions AS ( \
             SELECT author.repo, MIN(author.first_commit_at) AS first_at \
             FROM repo_author_stats author \
             JOIN repos public_repo ON public_repo.repo = author.repo \
             WHERE LOWER(author.github_login) = $1 \
               AND author.first_commit_at IS NOT NULL \
               AND public_repo.missing = FALSE \
               AND public_repo.metadata_fetched_at IS NOT NULL \
             GROUP BY author.repo \
         ), candidates AS ( \
             SELECT contribution.repo, contribution.first_at, \
                    GREATEST(public_repo.star_count, 0)::BIGINT AS current_stars, \
                    LOWER(SPLIT_PART(contribution.repo, '/', 1)) = $1 AS owned \
             FROM contributions contribution \
             JOIN repos public_repo ON public_repo.repo = contribution.repo \
             WHERE public_repo.history_complete = TRUE \
               AND GREATEST(public_repo.star_count, 0) >= 512 \
         ) \
         SELECT candidate.repo, candidate.first_at, candidate.current_stars, candidate.owned, \
                early.stars_at_first \
         FROM candidates candidate \
         CROSS JOIN LATERAL ( \
             SELECT COUNT(*)::BIGINT AS stars_at_first \
             FROM active_repo_star_history star \
             WHERE star.repo = candidate.repo \
               AND star.starred_at <= candidate.first_at \
         ) early \
         WHERE candidate.current_stars > early.stars_at_first * 5 \
         ORDER BY candidate.current_stars DESC, candidate.repo ASC \
         LIMIT 12",
    )
    .bind(login)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(UserVisionaryRepo {
            repo: row.try_get("repo")?,
            current_stars: row.try_get("current_stars")?,
            stars_at_first_contribution: row.try_get("stars_at_first")?,
            first_contribution_at: row.try_get("first_at")?,
            owned: row.try_get("owned")?,
        });
    }
    Ok(out)
}

struct UserContributionTotals {
    authored_commits: i64,
    contributed_repos: i64,
    owned_repos: i64,
    external_repos: i64,
    owned_commits: i64,
    external_commits: i64,
    first_at: Option<DateTime<Utc>>,
}

async fn load_user_contribution_totals(
    pool: &sqlx::PgPool,
    login: &str,
) -> Result<UserContributionTotals, ApiError> {
    let row = sqlx::query(
        "SELECT COALESCE(SUM(author.commits), 0)::BIGINT AS commits, \
                COUNT(DISTINCT author.repo) AS contribs, \
                COUNT(DISTINCT author.repo) FILTER \
                    (WHERE LOWER(SPLIT_PART(author.repo, '/', 1)) = $1) AS owned_contribs, \
                COUNT(DISTINCT author.repo) FILTER \
                    (WHERE LOWER(SPLIT_PART(author.repo, '/', 1)) <> $1) AS external_contribs, \
                COALESCE(SUM(author.commits) FILTER \
                    (WHERE LOWER(SPLIT_PART(author.repo, '/', 1)) = $1), 0)::BIGINT \
                    AS owned_commits, \
                COALESCE(SUM(author.commits) FILTER \
                    (WHERE LOWER(SPLIT_PART(author.repo, '/', 1)) <> $1), 0)::BIGINT \
                    AS external_commits, \
                MIN(author.first_commit_at) AS first_at \
         FROM repo_author_stats author \
         JOIN repos public_repo ON public_repo.repo = author.repo \
         WHERE LOWER(author.github_login) = $1 \
           AND public_repo.missing = FALSE \
           AND public_repo.metadata_fetched_at IS NOT NULL",
    )
    .bind(login)
    .fetch_one(pool)
    .await?;
    Ok(UserContributionTotals {
        authored_commits: row.try_get("commits")?,
        contributed_repos: row.try_get("contribs")?,
        owned_repos: row.try_get("owned_contribs")?,
        external_repos: row.try_get("external_contribs")?,
        owned_commits: row.try_get("owned_commits")?,
        external_commits: row.try_get("external_commits")?,
        first_at: row.try_get("first_at")?,
    })
}

/// Aggregate the profile report from Postgres. `login` must already be
/// [`cards::is_valid_login`]-validated: it is interpolated into a `LIKE`
/// prefix bind, and that validation guarantees no LIKE metacharacter can
/// widen the owner match.
async fn load_user_stats(db: &crate::db::Db, login: &str) -> Result<UserStats, ApiError> {
    let pool = &db.pool;
    let scope = load_profile_scope(pool, login).await?;

    let owned = sqlx::query(
        "SELECT COUNT(*) AS repos_tracked, \
                COALESCE(SUM(GREATEST(repos.star_count, 0)), 0)::BIGINT AS stars, \
                COALESCE(SUM(GREATEST(repos.forks_count, 0)), 0)::BIGINT AS forks, \
                COUNT(history.repo) FILTER \
                    (WHERE history.last_analyzed_at IS NOT NULL) AS repos_analyzed, \
                COALESCE(SUM(history.total_commits) FILTER \
                    (WHERE history.last_analyzed_at IS NOT NULL), 0)::BIGINT AS analyzed_commits \
         FROM repos \
         LEFT JOIN repo_history history ON history.repo = repos.repo \
         WHERE repos.repo LIKE $1 || '/%' \
           AND NOT repos.missing \
           AND repos.metadata_fetched_at IS NOT NULL",
    )
    .bind(login)
    .fetch_one(pool)
    .await?;
    let repos_tracked: i64 = owned.try_get("repos_tracked")?;
    let total_stars: i64 = owned.try_get("stars")?;
    let total_forks: i64 = owned.try_get("forks")?;
    let repos_analyzed: i64 = owned.try_get("repos_analyzed")?;
    let analyzed_commits: i64 = owned.try_get("analyzed_commits")?;

    let contributions = load_user_contribution_totals(pool, login).await?;
    let visionary_repos = load_visionary_repos(pool, login).await?;

    let languages = load_user_language_bars(pool, &scope).await?;

    let top_rows = sqlx::query(
        "SELECT repos.repo AS repo, \
                COALESCE(GREATEST(repos.star_count, 0), 0)::BIGINT AS stars, \
                COALESCE(GREATEST(repos.forks_count, 0), 0)::BIGINT AS forks, \
                COALESCE(history.total_commits, 0)::BIGINT AS commits, \
                repos.history_complete AS history_complete \
         FROM repos \
         LEFT JOIN repo_history history ON history.repo = repos.repo \
         WHERE repos.repo = ANY($1::text[]) \
         ORDER BY stars DESC, repos.repo ASC LIMIT 8",
    )
    .bind(&scope)
    .fetch_all(pool)
    .await?;
    let mut top_repos: Vec<UserRepoRow> = Vec::with_capacity(top_rows.len());
    // Only repos whose star history is confirmed complete may contribute a
    // sparkline; a partial series would draw a shape that isn't real.
    let mut sparkable: Vec<String> = Vec::new();
    for row in top_rows {
        let repo: String = row.try_get("repo")?;
        if row.try_get::<bool, _>("history_complete")? {
            sparkable.push(repo.clone());
        }
        top_repos.push(UserRepoRow {
            repo,
            stars: row.try_get("stars")?,
            forks: row.try_get("forks")?,
            commits: row.try_get("commits")?,
            commits_recent: 0,
            spark: Vec::new(),
        });
    }
    let sparks = load_repo_sparklines(pool, &sparkable).await?;
    for entry in top_repos.iter_mut() {
        if let Some(spark) = sparks.get(&entry.repo) {
            entry.spark = spark.clone();
        }
    }

    let active_rows = sqlx::query(
        "SELECT days.repo AS repo, SUM(days.commits)::BIGINT AS commits_recent \
         FROM repo_commit_days days \
         WHERE days.repo = ANY($1::text[]) \
           AND days.day >= CURRENT_DATE - $2::INT \
         GROUP BY days.repo \
         HAVING SUM(days.commits) > 0 \
         ORDER BY commits_recent DESC, days.repo ASC LIMIT 8",
    )
    .bind(&scope)
    .bind(PROFILE_ACTIVE_WINDOW_DAYS as i32)
    .fetch_all(pool)
    .await?;
    let active_repos: Vec<UserRepoRow> = active_rows
        .into_iter()
        .map(|row| {
            Ok(UserRepoRow {
                repo: row.try_get("repo")?,
                stars: 0,
                forks: 0,
                commits: 0,
                commits_recent: row.try_get("commits_recent")?,
                spark: Vec::new(),
            })
        })
        .collect::<Result<_, sqlx::Error>>()?;

    let (from, to) = profile_heatmap_window();
    let commit_days = load_user_commit_days(pool, &scope, from, to)
        .await?
        .into_iter()
        .map(|day| UserDay {
            date: day.day,
            value: day.commits,
        })
        .collect();
    let commit_streak = load_user_commit_streak(pool, login, Utc::now().date_naive()).await?;

    // Per owned analyzed repo: how many top non-bot authors it takes to
    // pass half of the authorship. Mirrors `repo_charts::compute_bus_factor`
    // (strictly more than 50%) as a window function so the whole profile
    // costs one query instead of one per repo.
    let bus_sql = format!(
        "WITH owned AS ( \
                 SELECT history.repo FROM repo_history history \
                 WHERE history.repo = ANY($1::text[]) \
                   AND history.last_analyzed_at IS NOT NULL \
             ), ranked AS ( \
                 SELECT author.repo AS repo, \
                        SUM(author.commits) OVER (PARTITION BY author.repo)::BIGINT AS total, \
                        SUM(author.commits) OVER ( \
                            PARTITION BY author.repo \
                            ORDER BY author.commits DESC, author.author_email ASC \
                            ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)::BIGINT AS running, \
                        ROW_NUMBER() OVER ( \
                            PARTITION BY author.repo \
                            ORDER BY author.commits DESC, author.author_email ASC) AS rn \
                 FROM repo_author_stats author \
                 JOIN owned ON owned.repo = author.repo \
                 WHERE author.commits > 0 AND {PROFILE_NON_BOT_AUTHOR} \
             ), bus AS ( \
                 SELECT repo, MIN(rn) AS bus_factor FROM ranked \
                 WHERE running * 2 > total GROUP BY repo \
             ) \
             SELECT COUNT(*) FILTER (WHERE bus_factor <= 1) AS solo, \
                    COUNT(*) AS scored FROM bus"
    );
    let bus = sqlx::query(sqlx::AssertSqlSafe(bus_sql))
        .bind(&scope)
        .bind(PROFILE_BOT_LOGINS)
        .fetch_one(pool)
        .await?;
    let solo_maintained: i64 = bus.try_get("solo")?;
    let scored: i64 = bus.try_get("scored")?;

    Ok(UserStats {
        login: login.to_string(),
        ready: repos_analyzed > 0 || contributions.contributed_repos > 0,
        repos_tracked,
        repos_scanned: scope.len() as i64,
        repos_analyzed,
        total_stars,
        total_forks,
        authored_commits: contributions.authored_commits,
        contributed_repos: contributions.contributed_repos,
        owned_contributed_repos: contributions.owned_repos,
        external_contributed_repos: contributions.external_repos,
        owned_authored_commits: contributions.owned_commits,
        external_authored_commits: contributions.external_commits,
        visionary_repos,
        analyzed_commits,
        since_year: contributions.first_at.map(|t| t.year()),
        solo_maintained,
        shared_maintained: (scored - solo_maintained).max(0),
        languages,
        top_repos,
        active_repos,
        commit_days,
        commit_streak,
    })
}

/// 52 weeks ending today, starting on a Monday — the same window the
/// per-repo commit heatmap uses, so the two read identically.
fn profile_heatmap_window() -> (chrono::NaiveDate, chrono::NaiveDate) {
    let today = Utc::now().date_naive();
    let this_monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    (
        this_monday - chrono::Duration::days((PROFILE_HEATMAP_WEEKS - 1) * 7),
        today,
    )
}

/// Cumulative monthly star totals per repo, for the profile's top-repo
/// sparklines. ONE grouped query over the star-history view for the whole
/// set (repo-leading index scan) instead of a per-repo day-delta load —
/// the profile lists up to eight repos and must not turn into eight full
/// history reads. Callers pass only repos with confirmed-complete
/// history, so a plotted line is never a partial series.
async fn load_repo_sparklines(
    pool: &sqlx::PgPool,
    repos: &[String],
) -> Result<std::collections::HashMap<String, Vec<i64>>, ApiError> {
    if repos.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows = sqlx::query(
        "SELECT repo, \
                date_trunc('month', starred_at AT TIME ZONE 'UTC')::date AS month, \
                COUNT(*)::BIGINT AS delta \
         FROM active_repo_star_history \
         WHERE repo = ANY($1::text[]) \
         GROUP BY 1, 2 ORDER BY 1, 2",
    )
    .bind(repos)
    .fetch_all(pool)
    .await?;
    let mut out: std::collections::HashMap<String, Vec<i64>> = std::collections::HashMap::new();
    for row in rows {
        let repo: String = row.try_get("repo")?;
        let delta: i64 = row.try_get("delta")?;
        let series = out.entry(repo).or_default();
        let running = series.last().copied().unwrap_or(0) + delta;
        series.push(running);
    }
    // Keep the tail: a sparkline is about the shape of recent growth.
    for series in out.values_mut() {
        if series.len() > 60 {
            series.drain(..series.len() - 60);
        }
    }
    Ok(out)
}

/// Language totals across a [`ProfileScope`], ordered by total lines
/// (falling back to the file census for language sets tokei reported
/// without line counts). The `language` tie-break keeps rendered bytes
/// deterministic. The scope is already tombstone- and visibility-filtered,
/// so this reads `repo_lines` by slug instead of re-joining `repos`.
async fn load_user_language_bars(
    pool: &sqlx::PgPool,
    repos: &[String],
) -> Result<Vec<UserLanguage>, ApiError> {
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    // Repositories whose breakdown is a file census are excluded when any
    // exact-counted repository exists: summing file counts and line counts
    // into one bar renders a language with thousands of lines next to one
    // with nine files under a single "lines" label.
    let rows = sqlx::query(
        "SELECT lines.language AS language, \
                SUM(lines.files)::BIGINT AS files, \
                SUM(lines.lines_code)::BIGINT AS code, \
                SUM(lines.lines_blank)::BIGINT AS blank, \
                SUM(lines.lines_comment)::BIGINT AS comment \
         FROM repo_lines lines \
         WHERE lines.repo = ANY($1::text[]) \
           AND (lines.lines_exact OR NOT EXISTS ( \
               SELECT 1 FROM repo_lines exact_rows \
               WHERE exact_rows.repo = ANY($1::text[]) AND exact_rows.lines_exact \
           )) \
         GROUP BY lines.language \
         HAVING SUM(lines.lines_code + lines.lines_blank + lines.lines_comment) > 0 \
             OR SUM(lines.files) > 0 \
         ORDER BY CASE WHEN SUM(lines.lines_code + lines.lines_blank + lines.lines_comment) > 0 \
                  THEN SUM(lines.lines_code + lines.lines_blank + lines.lines_comment) \
                  ELSE SUM(lines.files) END DESC, lines.language ASC \
         LIMIT 12",
    )
    .bind(repos)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(UserLanguage {
            language: row.try_get("language")?,
            files: row.try_get("files")?,
            code: row.try_get("code")?,
            blank: row.try_get("blank")?,
            comment: row.try_get("comment")?,
        });
    }
    Ok(out)
}

/// Commit days summed across a [`ProfileScope`]. Bound as a slug array so
/// the read is `(repo, day)` index scans over the bounded set — an
/// owner-prefix scan here reads every daily row of every repository the
/// account owns, which for a large organization is millions of rows per
/// profile view.
async fn load_user_commit_days(
    pool: &sqlx::PgPool,
    repos: &[String],
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<crate::repo_charts::DayCount>, ApiError> {
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT days.day AS day, SUM(days.commits)::BIGINT AS commits \
         FROM repo_commit_days days \
         WHERE days.repo = ANY($1::text[]) \
           AND days.day BETWEEN $2 AND $3 \
         GROUP BY days.day ORDER BY days.day ASC",
    )
    .bind(repos)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(crate::repo_charts::DayCount {
            day: row.try_get("day")?,
            commits: row.try_get("commits")?,
        });
    }
    Ok(out)
}

/// Complete active-day history for the profile achievement ladder.
///
/// This is intentionally a Postgres-only read over author/day rows, joined to
/// the already-enriched login in `repo_author_stats`. Repository-wide daily
/// totals cannot prove individual activity and are never used for awards.
/// The database reduces every matching repository/day row to at most one date
/// before Rust applies the pure streak math.
async fn load_user_commit_streak(
    pool: &sqlx::PgPool,
    login: &str,
    today: chrono::NaiveDate,
) -> Result<CommitStreak, ApiError> {
    let rows = sqlx::query(
        "SELECT days.day AS day \
         FROM repo_author_stats author \
         JOIN repo_author_commit_days days \
           ON days.repo = author.repo AND days.author_email = author.author_email \
         JOIN repos public_repo ON public_repo.repo = author.repo \
         WHERE LOWER(author.github_login) = $1 \
           AND days.day <= $2 \
           AND public_repo.missing = FALSE \
           AND public_repo.metadata_fetched_at IS NOT NULL \
         GROUP BY days.day \
         HAVING SUM(days.commits) > 0 \
         ORDER BY days.day ASC",
    )
    .bind(login)
    .bind(today)
    .fetch_all(pool)
    .await?;
    let active_days = rows
        .into_iter()
        .map(|row| row.try_get::<chrono::NaiveDate, _>("day"))
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(summarize_commit_streak(active_days, today))
}

/// Cache/render revision for the profile charts: the number of analyzed
/// owned repos, the newest analysis timestamp, and the analyzed commit
/// total. Any completed analysis pass moves at least one of the three, so
/// a stale chart can never outlive the data it was rendered from.
/// `None` means nothing has been analyzed yet → pending placeholder.
async fn user_stat_revision(pool: &sqlx::PgPool, login: &str) -> Result<Option<String>, ApiError> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS analyzed, \
                COALESCE(EXTRACT(EPOCH FROM MAX(history.last_analyzed_at)), 0)::BIGINT AS at, \
                COALESCE(SUM(history.total_commits), 0)::BIGINT AS commits \
         FROM repos \
         JOIN repo_history history ON history.repo = repos.repo \
         WHERE repos.repo LIKE $1 || '/%' \
           AND NOT repos.missing \
           AND repos.metadata_fetched_at IS NOT NULL \
           AND history.last_analyzed_at IS NOT NULL",
    )
    .bind(login)
    .fetch_one(pool)
    .await?;
    let analyzed: i64 = row.try_get("analyzed")?;
    let contributions = sqlx::query(
        "WITH authored AS ( \
             SELECT author.repo, SUM(author.commits)::BIGINT AS commits, \
                    MIN(author.first_commit_at) AS first_at \
             FROM repo_author_stats author \
             JOIN repos public_repo ON public_repo.repo = author.repo \
             WHERE LOWER(author.github_login) = $1 \
               AND public_repo.missing = FALSE \
               AND public_repo.metadata_fetched_at IS NOT NULL \
             GROUP BY author.repo \
         ) \
         SELECT COUNT(*) AS repos, \
                COALESCE(SUM(authored.commits), 0)::BIGINT AS commits, \
                COALESCE(SUM(GREATEST(public_repo.star_count, 0)), 0)::BIGINT AS stars, \
                COUNT(*) FILTER (WHERE public_repo.history_complete) AS complete_histories, \
                COALESCE(SUM(EXTRACT(EPOCH FROM authored.first_at)), 0)::BIGINT AS first_at \
         FROM authored \
         JOIN repos public_repo ON public_repo.repo = authored.repo",
    )
    .bind(login)
    .fetch_one(pool)
    .await?;
    let contributed_repos: i64 = contributions.try_get("repos")?;
    if analyzed <= 0 && contributed_repos <= 0 {
        return Ok(None);
    }
    let at: i64 = row.try_get("at")?;
    let commits: i64 = row.try_get("commits")?;
    let authored_commits: i64 = contributions.try_get("commits")?;
    let contribution_stars: i64 = contributions.try_get("stars")?;
    let complete_histories: i64 = contributions.try_get("complete_histories")?;
    let contribution_first_at: i64 = contributions.try_get("first_at")?;
    Ok(Some(format!(
        "n{analyzed}:t{at}:c{commits}:x{contributed_repos}:a{authored_commits}:\
         s{contribution_stars}:h{complete_histories}:f{contribution_first_at}"
    )))
}

/// `GET /api/users/:login/stats.json` — the profile report's data source.
/// Postgres only; the shape mirrors the per-repo `stats.json` contract
/// (an explicit `ready` flag rather than partial data dressed as final).
async fn user_stats_json(
    State(state): State<ApiState>,
    Path(login): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !cards::is_valid_login(&login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let login = login.to_ascii_lowercase();
    let key = format!("user-stats:{login}");
    let (json, live) = single_flight_analyze(&state.analyze_cache, key, async {
        let stats = load_user_stats(state.analyzer.cache.db(), &login).await?;
        let json = serde_json::to_string(&stats)?;
        Ok((json, !stats.ready))
    })
    .await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, analyze_cache_control(live));
    Ok((headers, json))
}

/// Embeddable profile charts. Deliberately a small, fixed set: each one
/// reuses a per-repo renderer over the owner-scoped aggregate, so a
/// profile asset and a repo asset are the same visual language.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UserStatKind {
    /// Aggregated commit heatmap across owned repos.
    CommitActivity,
    /// Monthly commit volume across owned repos.
    CommitTrend,
    /// Language footprint across owned repos.
    Languages,
    /// Authored work split between owned and outside projects.
    Contributions,
}

impl UserStatKind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "commit-activity" => Some(Self::CommitActivity),
            "commit-trend" => Some(Self::CommitTrend),
            "languages" => Some(Self::Languages),
            "contributions" => Some(Self::Contributions),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::CommitActivity => "commit-activity",
            Self::CommitTrend => "commit-trend",
            Self::Languages => "languages",
            Self::Contributions => "contributions",
        }
    }
}

#[derive(Clone, Copy)]
enum UserStatFormat {
    Svg,
    Raster(crate::raster::RasterFormat),
    Gif,
}

/// `{name}.{svg|gif|png|webp}` — the profile mirror of the per-repo stat
/// dispatcher. Every raster format shares the process-wide encode cap.
fn parse_user_stat_filename(s: &str) -> Option<(UserStatKind, UserStatFormat)> {
    let (name, ext) = s.rsplit_once('.')?;
    let format = match ext {
        "svg" => UserStatFormat::Svg,
        "gif" => UserStatFormat::Gif,
        "png" => UserStatFormat::Raster(crate::raster::RasterFormat::Png),
        "webp" => UserStatFormat::Raster(crate::raster::RasterFormat::Webp),
        _ => return None,
    };
    Some((UserStatKind::parse(name)?, format))
}

async fn render_user_stat_svg(
    state: &ApiState,
    login: &str,
    kind: UserStatKind,
    theme: &crate::theme::Theme,
) -> Result<String, ApiError> {
    let pool = &state.analyzer.cache.db().pool;
    let label = format!("@{login}");
    let scope = load_profile_scope(pool, login).await?;
    Ok(match kind {
        UserStatKind::CommitActivity => {
            let (from, to) = profile_heatmap_window();
            let days = load_user_commit_days(pool, &scope, from, to).await?;
            crate::repo_charts::render_heatmap(
                &label,
                "Commits across tracked repos · last 52 weeks",
                from,
                to,
                &days,
                // A profile spans many repositories with different analysis
                // windows, so there is no single day before which nothing was
                // observed.
                None,
                theme,
            )
        }
        UserStatKind::CommitTrend => {
            let days = load_user_commit_days(
                pool,
                &scope,
                chrono::NaiveDate::from_ymd_opt(2005, 1, 1).expect("2005-01-01 is a valid date"),
                Utc::now().date_naive(),
            )
            .await?;
            crate::repo_charts::render_commit_trend(&label, &days, theme)
        }
        UserStatKind::Languages => {
            let bars: Vec<crate::repo_charts::LanguageBar> = load_user_language_bars(pool, &scope)
                .await?
                .into_iter()
                .map(|row| crate::repo_charts::LanguageBar {
                    language: row.language,
                    files: row.files,
                    lines_code: row.code,
                    lines_blank: row.blank,
                    lines_comment: row.comment,
                })
                .collect();
            crate::repo_charts::render_languages(&label, &bars, theme)
        }
        UserStatKind::Contributions => {
            let totals = load_user_contribution_totals(pool, login).await?;
            let visionary_count = load_visionary_repos(pool, login).await?.len() as i64;
            crate::repo_charts::render_contribution_profile(
                &label,
                &crate::repo_charts::ContributionProfile {
                    owned_repos: totals.owned_repos,
                    external_repos: totals.external_repos,
                    owned_commits: totals.owned_commits,
                    external_commits: totals.external_commits,
                    visionary_count,
                },
                theme,
            )
        }
    })
}

async fn user_stat_dispatcher(
    State(state): State<ApiState>,
    Path((login, filename)): Path<(String, String)>,
    Query(q): Query<UserStatQuery>,
    request_headers: HeaderMap,
) -> Result<axum::response::Response, ApiError> {
    if !cards::is_valid_login(&login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let Some((kind, format)) = parse_user_stat_filename(&filename) else {
        return Err(ApiError::bad_request("unknown chart or format"));
    };
    let login = login.to_ascii_lowercase();
    let theme = theme_for(q.theme.as_deref());
    let theme_key = if theme.dark { "dark" } else { "light" };

    let Some(revision) = user_stat_revision(&state.analyzer.cache.db().pool, &login).await? else {
        // Nothing analyzed yet — short TTL, never memoized, so the embed
        // heals as soon as the durable queue lands the first pass.
        let mut svg = cards::render_user_pending_card(&login, theme);
        if q.in_app() {
            svg = crate::brand::without_embed_footer(svg);
        }
        return Ok(match format {
            UserStatFormat::Svg => user_stat_svg_response(&request_headers, svg, true, !q.in_app()),
            UserStatFormat::Raster(format) => {
                let bytes = rasterize_limited(svg, format, RASTER_SCALE).await?;
                card_raster_response(&request_headers, format, std::sync::Arc::new(bytes), true)
            }
            UserStatFormat::Gif => {
                let encoded =
                    with_raster_permit(move || crate::animated_gif::encode_dither_loop(&svg))
                        .await?
                        .map_err(ApiError::from)?;
                gif_media_response(
                    &request_headers,
                    std::sync::Arc::new(encoded.bytes),
                    true,
                    &format!(
                        "{login}-{}-{}.gif",
                        kind.key(),
                        if theme.dark { "dark" } else { "light" }
                    ),
                )?
            }
        });
    };

    let key = format!(
        "user-stat:{}:{login}|{theme_key}|rev:{revision}|{RENDER_REVISION}",
        kind.key(),
    );
    let svg = single_flight(&state.stat_svg_cache, key.clone(), async {
        render_user_stat_svg(&state, &login, kind, theme).await
    })
    .await?;
    // README assets are static by default; SMIL only on an explicit opt-in.
    let mut svg = if q.animate == Some(1) {
        svg
    } else {
        crate::raster::freeze_svg_animations(&svg)
    };
    if q.in_app() {
        svg = crate::brand::without_embed_footer(svg);
    }

    let format = match format {
        UserStatFormat::Svg => {
            return Ok(user_stat_svg_response(
                &request_headers,
                svg,
                false,
                !q.in_app(),
            ));
        }
        UserStatFormat::Gif => {
            let gif_key = format!(
                "{key}|gif|svg:{}|{}",
                svg_digest(&svg),
                if q.in_app() { "app" } else { "embed" }
            );
            let (bytes, short_ttl) = single_flight_gif(&state.raster_cache, gif_key, async move {
                let encoded =
                    with_raster_permit(move || crate::animated_gif::encode_dither_loop(&svg))
                        .await?
                        .map_err(ApiError::from)?;
                Ok(std::sync::Arc::new(encoded.bytes))
            })
            .await?;
            return gif_media_response(
                &request_headers,
                bytes,
                short_ttl,
                &format!(
                    "{login}-{}-{}.gif",
                    kind.key(),
                    if theme.dark { "dark" } else { "light" }
                ),
            );
        }
        UserStatFormat::Raster(format) => format,
    };
    let raster_key = format!(
        "{key}|{}|{}",
        raster_fmt_key(format),
        if q.in_app() { "app" } else { "embed" }
    );
    if let Some(cached) = state.raster_cache.get(&raster_key).await {
        return Ok(card_raster_response(
            &request_headers,
            format,
            cached,
            false,
        ));
    }
    let bytes = std::sync::Arc::new(rasterize_limited(svg, format, RASTER_SCALE).await?);
    state.raster_cache.insert(raster_key, bytes.clone()).await;
    Ok(card_raster_response(&request_headers, format, bytes, false))
}

#[derive(Debug, Deserialize)]
struct UserStatQuery {
    theme: Option<String>,
    animate: Option<u8>,
    /// `context=app` — rendered inside gitdebt's own UI, where the embed
    /// attribution footer is redundant chrome. Same explicit opt-in as
    /// the per-repo stat charts; README embeds keep the footer.
    context: Option<String>,
}

impl UserStatQuery {
    fn in_app(&self) -> bool {
        self.context.as_deref() == Some("app")
    }
}

fn user_stat_svg_response(
    request_headers: &HeaderMap,
    svg: String,
    short_ttl: bool,
    branded: bool,
) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    let body = if branded {
        crate::brand::with_site_link(svg)
    } else {
        svg
    };
    conditional_media_response(request_headers, headers, body.into_bytes())
}

fn gif_media_response(
    request_headers: &HeaderMap,
    bytes: std::sync::Arc<Vec<u8>>,
    short_ttl: bool,
    filename: &str,
) -> Result<axum::response::Response, ApiError> {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/gif"));
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("inline; filename=\"{filename}\""))
            .map_err(|_| ApiError::bad_request("invalid media filename"))?,
    );
    Ok(conditional_media_response(
        request_headers,
        headers,
        (*bytes).clone(),
    ))
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
    let key = format!(
        "user:{login}|{theme_key}|{}|{}|{}",
        q.opts_key(),
        spec.key(),
        user_data_revision(state, &login).await?
    );
    if let Some(cached) = state.svg_cache.get(&key).await {
        return Ok(RenderedCard {
            svg: cached,
            short_ttl: false,
        });
    }
    let agg = build_user_aggregate(state, &login).await?;
    let series = export::filter_points(&agg.series, &spec);
    let pending = agg.repos_included == 0 || series.is_empty();
    let svg = crate::texture::decorate(
        render_svg(
            &series,
            &ChartConfig {
                repo: login,
                ..ChartConfig::default()
            },
            theme,
            &q.opts(),
        ),
        theme,
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
        "user:{}|{theme_key}|{}|{}|{}|{fmt_key}",
        login.to_ascii_lowercase(),
        q.opts_key(),
        spec.key(),
        user_data_revision(state, &login.to_ascii_lowercase()).await?
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
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_user_chart_svg(&state, &login, theme, &q).await?;
    Ok(card_svg_response(&request_headers, card))
}

async fn user_chart_png(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_chart_raster(&state, &login, theme, &q, crate::raster::RasterFormat::Png)
            .await?;
    Ok(card_raster_response(
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn user_chart_webp(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_chart_raster(&state, &login, theme, &q, crate::raster::RasterFormat::Webp)
            .await?;
    Ok(card_raster_response(
        &request_headers,
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

/// Real README motion for aggregate profile star history. This is the same
/// bounded wave encoder and exact chart geometry as repository GIFs, fed by
/// the Postgres-only summed series.
async fn user_chart_gif(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    if !aggregate::is_valid_login(&login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    q.gif_motion()?;
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_user_chart_gif(&state, &login, theme, &q).await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/gif"));
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    let filename = format!(
        "inline; filename=\"{}-star-history-{}.gif\"",
        login.to_ascii_lowercase(),
        if theme.dark { "dark" } else { "light" }
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&filename).map_err(|_| ApiError::bad_request("invalid login"))?,
    );
    Ok(conditional_media_response(
        &request_headers,
        headers,
        (*bytes).clone(),
    ))
}

async fn ensure_user_chart_gif(
    state: &ApiState,
    login: &str,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let motion = q.gif_motion()?;
    let spec = q.range_spec()?;
    let login = login.to_ascii_lowercase();
    let aggregate = build_user_aggregate(state, &login).await?;
    let series = export::filter_points(&aggregate.series, &spec);
    let complete = aggregate.repos_pending == 0 && !series.is_empty();
    let revision = crate::animated_gif::data_revision(&series);
    let theme_key = if theme.dark { "dark" } else { "light" };
    let completeness_key = if complete { "complete" } else { "pending" };
    let key = format!(
        "user-chart-gif:{login}|{theme_key}|{}|{}|motion:{motion}|rev:{revision}|{completeness_key}|{RENDER_REVISION}",
        q.series_opts_key(),
        spec.key(),
    );
    let cfg = ChartConfig {
        repo: login.clone(),
        ..ChartConfig::default()
    };
    let mut opts = q.opts();
    opts.animate = false;
    let theme = *theme;
    let seed = crate::animated_gif::fnv1a(&format!("user:{login}"));
    single_flight_gif(&state.raster_cache, key, async move {
        let encoded = with_raster_permit(move || {
            if motion == crate::animated_gif::MOTION_DRAW {
                crate::animated_gif::encode_draw(&series, &cfg, &theme, &opts)
            } else {
                crate::animated_gif::encode_wave(&series, &cfg, &theme, &opts, seed)
            }
        })
        .await?
        .map_err(ApiError::from)?;
        let bytes = std::sync::Arc::new(encoded.bytes);
        if complete {
            Ok(bytes)
        } else {
            Err(GifMiss::Pending(bytes))
        }
    })
    .await
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

    let summary = cache.get_repo_summary(&repo_full).await.ok().flatten();
    let known = summary.as_ref().is_some_and(|value| {
        value.stargazers_complete && !value.missing && value.metadata_fetched_at.is_some()
    });
    let exact = known
        && summary
            .as_ref()
            .and_then(|value| value.history_source.as_deref())
            == Some("github_api");
    let fresh = summary
        .as_ref()
        .is_some_and(|value| value.stargazers_fresh_within(PING_STALE_TTL));
    let cached_stars = known
        .then(|| summary.as_ref().and_then(|value| value.star_count))
        .flatten();
    // `stale` here means "we have it but it's worth refreshing" — by age
    // or by the count drifting past the threshold. Unknown is reported via
    // `known: false`, not `stale`.
    let count_drifted = match (cached_stars, body.stars) {
        (Some(c), Some(r)) => ping_count_drifted(c, r),
        // No reported count → no count-based drift (age-only freshness).
        _ => false,
    };
    let stale = known && !exact && (!fresh || count_drifted);

    let enqueued = !exact && ping_should_enqueue(cached_stars, body.stars, fresh);
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
/// the default is static. `motion=wave|draw` picks the GIF preset (the
/// GIF route defaults to the looping `wave`).
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
    /// SVG/raster cache fragment, including the explicit animation choice
    /// and the render revision (a renderer redesign must never serve a
    /// stale look under an unchanged data key).
    fn opts_key(&self) -> String {
        let animate = if self.opts().animate {
            "anim"
        } else {
            "static"
        };
        format!("{}:{animate}|{RENDER_REVISION}", self.series_opts_key())
    }
    /// GIF motion preset. Absent → the looping `wave` default; `draw`
    /// stays supported (play-once reveal); anything else is a 400.
    fn gif_motion(&self) -> Result<&'static str, ApiError> {
        match self.motion.as_deref().map(str::trim) {
            None => Ok(crate::animated_gif::MOTION_WAVE),
            Some(value) if value.eq_ignore_ascii_case(crate::animated_gif::MOTION_WAVE) => {
                Ok(crate::animated_gif::MOTION_WAVE)
            }
            Some(value) if value.eq_ignore_ascii_case(crate::animated_gif::MOTION_DRAW) => {
                Ok(crate::animated_gif::MOTION_DRAW)
            }
            _ => Err(ApiError::bad_request(
                "chart.gif supports motion=wave (default) or motion=draw",
            )),
        }
    }
}

async fn chart(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_chart_svg(&state, &owner, &repo, theme, &q).await?;
    Ok(card_svg_response(&request_headers, card))
}

async fn chart_png(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn chart_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

/// Actual README animation. Reads only complete cached Postgres stargazer
/// timestamps. Defaults to the continuously-looping `wave` preset;
/// `motion=draw` keeps the play-once reveal.
async fn chart_gif(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
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
    Ok(conditional_media_response(
        &request_headers,
        headers,
        (*bytes).clone(),
    ))
}

async fn ensure_chart_gif(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let motion = q.gif_motion()?;
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
    let arrivals = state
        .analyzer
        .cache
        .get_repo_star_series(&repo_full)
        .await?;
    let complete = arrivals.is_some();
    let full = arrivals.unwrap_or_default();
    let series = export::filter_points(&full, &spec);
    let revision = crate::animated_gif::data_revision(&series);
    let theme_key = if theme.dark { "dark" } else { "light" };
    // Completeness is part of the key: pending (incomplete-data) encodes
    // ride the short-TTL policy and must never be answered by a complete
    // cached body (or vice versa), even when the two would render the
    // same series bytes.
    let completeness_key = if complete { "complete" } else { "pending" };
    let key = format!(
        "chart-gif:{repo_full}|{theme_key}|{}|{}|motion:{motion}|rev:{revision}|{completeness_key}|{RENDER_REVISION}",
        q.series_opts_key(),
        spec.key(),
    );

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
    let seed = crate::animated_gif::fnv1a(&cfg.repo);
    // Single-flight over the raster cache: concurrent misses for the same
    // key coalesce onto ONE encode, and the encode itself runs under a
    // raster permit — a wave GIF rasterizes 14 frames, the most expensive
    // render in the process, so it must queue at the same choke point as
    // every other raster path instead of fanning out on the blocking pool.
    single_flight_gif(&state.raster_cache, key, async move {
        let encoded = with_raster_permit(move || {
            if motion == crate::animated_gif::MOTION_DRAW {
                crate::animated_gif::encode_draw(&series, &cfg, &theme, &opts)
            } else {
                crate::animated_gif::encode_wave(&series, &cfg, &theme, &opts, seed)
            }
        })
        .await?
        .map_err(ApiError::from)?;
        debug_assert_eq!(
            encoded.frame_count,
            if motion == crate::animated_gif::MOTION_DRAW {
                crate::animated_gif::FRAME_COUNT
            } else {
                crate::animated_gif::WAVE_FRAME_COUNT
            },
            "encoder contract"
        );
        debug_assert!(encoded.width > 0 && encoded.height > 0);
        let bytes = std::sync::Arc::new(encoded.bytes);
        if complete {
            Ok(bytes)
        } else {
            // Incomplete cached stargazers: serve the frame short-TTL and
            // never pin it in the 24h raster cache (self-heals once the
            // star queue lands).
            Err(GifMiss::Pending(bytes))
        }
    })
    .await
}

/// Data version of a repository's star history, folded into every media memo
/// key that plots it.
///
/// The render caches are TTL-only — nothing invalidates them, and the Redis
/// invalidation bus reaches only the analyze/aggregate JSON caches — so a key
/// that does not depend on the data pins a README embed at whatever the
/// series looked like when it was first rendered. Each of these components
/// moves in the same transaction that appends new stars
/// (`archive_hourly_db::commit_hour`) or refreshes metadata, and none of them
/// moves while the data is unchanged, so quiet repositories keep their memo.
fn star_data_revision(summary: Option<&crate::cache::RepoSummary>) -> String {
    match summary {
        Some(summary) => format!(
            "d:{}:{}:{}",
            summary
                .stargazers_fetched_at
                .map(|value| value.timestamp_millis())
                .unwrap_or(0),
            summary.history_observed_count.unwrap_or(-1),
            summary.star_count.unwrap_or(-1),
        ),
        None => "d:cold".to_string(),
    }
}

/// Which physical history a repository's series comes from. Part of the memo
/// key because it also selects the rendered metric label.
fn history_source_key(summary: Option<&crate::cache::RepoSummary>) -> &'static str {
    match summary.and_then(|value| value.history_source.as_deref()) {
        Some("gh_archive") => "archive",
        _ => "github",
    }
}

/// Render-or-fetch the single-repo star-history SVG. Memoized in
/// `svg_cache` keyed by repo + theme + axis/log + date-range + the star data
/// revision so the raster handlers don't have to re-walk the analyze
/// pipeline and no variant can outlive its data.
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
    let source_key = history_source_key(summary.as_ref());
    let key = format!(
        "{repo_full}|{theme_key}|{source_key}|{}|{}|{}",
        q.opts_key(),
        spec.key(),
        star_data_revision(summary.as_ref())
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
        let svg = crate::texture::decorate(
            render_svg(
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
            ),
            theme,
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
    let repo_full = crate::analyzer::repo_key(owner, repo);
    // One indexed single-row read so the raster memo carries the same source
    // and data revision as the SVG it encodes. Without it a PNG embed could
    // hold a curve — and a metric label — that the SVG variant had already
    // replaced.
    let summary = state.analyzer.cache.get_repo_summary(&repo_full).await?;
    let key = format!(
        "chart:{repo_full}|{theme_key}|{}|{}|{}|{}|{fmt_key}",
        history_source_key(summary.as_ref()),
        q.opts_key(),
        spec.key(),
        star_data_revision(summary.as_ref())
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
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_multi_svg(&state, theme, &q).await?;
    Ok(card_svg_response(&request_headers, card))
}

async fn multi_chart_png(
    State(state): State<ApiState>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_multi_raster(&state, theme, &q, crate::raster::RasterFormat::Png).await?;
    Ok(card_raster_response(
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn multi_chart_webp(
    State(state): State<ApiState>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_multi_raster(&state, theme, &q, crate::raster::RasterFormat::Webp).await?;
    Ok(card_raster_response(
        &request_headers,
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

async fn multi_chart_gif(
    State(state): State<ApiState>,
    Query(q): Query<ChartQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_multi_gif(&state, theme, &q).await?;
    gif_media_response(
        &request_headers,
        bytes,
        short_ttl,
        &format!(
            "star-history-comparison-{}.gif",
            if theme.dark { "dark" } else { "light" }
        ),
    )
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

/// Data version for an overlay: one indexed read covering every requested
/// slug, so a comparison embed cannot keep showing the moment one project
/// pulled ahead after the other has caught up.
async fn overlay_revision(
    state: &ApiState,
    pairs: &[(String, String)],
) -> Result<String, ApiError> {
    let slugs: Vec<String> = pairs
        .iter()
        .map(|(owner, repo)| crate::analyzer::repo_key(owner, repo))
        .collect();
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT, \
                COALESCE(MAX(EXTRACT(EPOCH FROM stargazers_fetched_at)), 0)::BIGINT, \
                COALESCE(SUM(GREATEST(history_observed_count, 0)), 0)::BIGINT \
         FROM repos WHERE repo = ANY($1)",
    )
    .bind(&slugs)
    .fetch_one(&state.analyzer.cache.db().pool)
    .await?;
    Ok(format!("d:{}:{}:{}", row.0, row.1, row.2))
}

/// Stable cache key for an overlay request: the normalized slug set +
/// theme + axis/log + date-range + the data revision. The slug order is
/// preserved (it drives colors), so reordering produces a distinct,
/// correct key.
fn overlay_key(
    pairs: &[(String, String)],
    theme: &crate::theme::Theme,
    q: &ChartQuery,
    spec: &RangeSpec,
    revision: &str,
) -> String {
    let slugs: Vec<String> = pairs.iter().map(|(o, r)| format!("{o}/{r}")).collect();
    let theme_key = if theme.dark { "dark" } else { "light" };
    format!(
        "multi:{}|{theme_key}|{}|{}|{revision}",
        slugs.join(","),
        q.opts_key(),
        spec.key()
    )
}

/// Render-or-fetch the multi-repo overlay SVG. The render is pending
/// (short-TTL, never memoized) whenever ANY requested repo contributes an
/// empty series while its history is not confirmed complete — a mixed
/// warm+cold overlay would otherwise cache (moka + 4h edge) a chart that
/// silently omits the cold repo's line. A complete repo whose series is
/// empty only because the requested range excludes every point stays a
/// complete render: there is nothing for a short TTL to self-heal.
async fn ensure_multi_svg(
    state: &ApiState,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
) -> Result<RenderedCard, ApiError> {
    let spec = q.range_spec()?;
    let pairs = parse_overlay_repos(q.repos.as_deref())?;
    let revision = overlay_revision(state, &pairs).await?;
    let key = overlay_key(&pairs, theme, q, &spec, &revision);
    single_flight_card(&state.svg_cache, key, async {
        // Build each repo's daily cumulative series via the same pipeline as
        // the single chart. Done sequentially to keep one large overlay from
        // occupying the entire Postgres pool.
        let mut series_per_repo: Vec<(String, Vec<Point>)> = Vec::with_capacity(pairs.len());
        let mut pending = false;
        for (owner, repo) in &pairs {
            let series = star_series(owner, repo, &state.analyzer)
                .await
                .map_err(ApiError::from)?;
            let series = export::filter_points(&series, &spec);
            if series.is_empty() {
                // Same completeness gate as `ensure_site_og_raster`: the
                // summary's history-complete flag covers both the exact
                // GitHub snapshot and GH Archive-sourced repos. A cold /
                // just-enqueued repo (all-cold included) keeps the whole
                // overlay on the short-TTL pending policy.
                let summary = state
                    .analyzer
                    .cache
                    .get_repo_summary(&format!("{owner}/{repo}"))
                    .await
                    .map_err(ApiError::from)?;
                pending |= !summary.is_some_and(|s| s.stargazers_complete);
            }
            series_per_repo.push((format!("{owner}/{repo}"), series));
        }
        let svg = crate::texture::decorate(
            render_multi_svg(&series_per_repo, &ChartConfig::default(), theme, &q.opts()),
            theme,
        );
        if pending {
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
    // Encode memo keyed on the rendered SVG, like the animated variant: the
    // SVG memo already absorbs the per-repo series loads, and a content key
    // cannot drift from the bytes it encodes.
    let card = ensure_multi_svg(state, theme, q).await?;
    let key = format!(
        "{}|{fmt_key}|svg:{}",
        overlay_key(&pairs, theme, q, &spec, ""),
        svg_digest(&card.svg)
    );
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    if card.short_ttl {
        return Ok((rasterize_uncached(card.svg, format).await?, true));
    }
    Ok((
        rasterize_cached(state, &key, card.svg, format).await?,
        false,
    ))
}

/// Animated comparison export. The chart geometry and categorical line
/// colors stay fixed; only the ordered-dither signal phase moves, so every
/// frame remains a truthful rendering of the same Postgres-backed series.
async fn ensure_multi_gif(
    state: &ApiState,
    theme: &crate::theme::Theme,
    q: &ChartQuery,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let card = ensure_multi_svg(state, theme, q).await?;
    let spec = q.range_spec()?;
    let pairs = parse_overlay_repos(q.repos.as_deref())?;
    let key = format!(
        "{}|gif|svg:{}|{RENDER_REVISION}",
        overlay_key(&pairs, theme, q, &spec, ""),
        svg_digest(&card.svg),
    );
    let short_ttl = card.short_ttl;
    let svg = card.svg;
    single_flight_gif(&state.raster_cache, key, async move {
        let encoded = with_raster_permit(move || crate::animated_gif::encode_dither_loop(&svg))
            .await?
            .map_err(ApiError::from)?;
        let bytes = std::sync::Arc::new(encoded.bytes);
        if short_ttl {
            Err(GifMiss::Pending(bytes))
        } else {
            Ok(bytes)
        }
    })
    .await
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
        format!("{axis}:{log}|{RENDER_REVISION}")
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

    let declared = usage::resolve_packages(&owner, &repo, &q.overrides(), &state.storage).await;
    let downloads = usage::fetch_all(&state.analyzer.cache, &declared).await;
    // A manifest declaration proves repository intent; a successful registry
    // response proves the package exists. Only expose identities satisfying
    // both halves so a stale/typoed manifest can never become a public link.
    let resolved = Resolved {
        npm: downloads
            .npm
            .is_some()
            .then(|| declared.npm.clone())
            .flatten(),
        crate_: downloads
            .crates
            .is_some()
            .then(|| declared.crate_.clone())
            .flatten(),
        pypi: downloads
            .pypi
            .is_some()
            .then(|| declared.pypi.clone())
            .flatten(),
        docker: downloads
            .docker
            .is_some()
            .then(|| declared.docker.clone())
            .flatten(),
    };

    // Authoritative counts (best-effort from cache; the analyze path
    // refreshes them out-of-band).
    let stars_total = state
        .analyzer
        .cache
        .get_repo_star_count(&repo_full)
        .await
        .ok()
        .flatten()
        .map(|value| value.max(0) as u64)
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
    let repo_full = crate::analyzer::repo_key(owner, repo);
    let summary = state.analyzer.cache.get_repo_summary(&repo_full).await?;
    let key = format!(
        "usage:{repo_full}|{theme_key}|{}|src:{source_key}|{}|{}|{}",
        q.opts_key(),
        q.overrides_key(),
        spec.key(),
        star_data_revision(summary.as_ref()),
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
        let svg = crate::texture::decorate(
            render_overlay_svg(
                &stars,
                &cum,
                &ChartConfig::default(),
                &OverlayConfig {
                    repo: bundle.repo_full,
                    downloads_label: label,
                },
                theme,
                &q.opts(),
            ),
            theme,
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
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_usage_svg(&state, &owner, &repo, theme, &q).await?;
    Ok(card_svg_response(&request_headers, card))
}

async fn ensure_usage_raster(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &UsageQuery,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    let theme_key = if theme.dark { "dark" } else { "light" };
    let fmt_key = raster_fmt_key(format);
    let card = ensure_usage_svg(state, owner, repo, theme, q).await?;
    let key = format!(
        "usage:{}|{theme_key}|{fmt_key}|svg:{}",
        crate::analyzer::repo_key(owner, repo),
        svg_digest(&card.svg)
    );
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
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
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn usage_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<UsageQuery>,
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

// Badges

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct EarnedRepoBadge {
    id: &'static str,
    label: &'static str,
    detail: String,
    earned: bool,
    pending: bool,
}

#[derive(Debug, Default)]
struct RepoBadgeEvidence {
    latest_commit: Option<chrono::NaiveDate>,
    commits_30d: u64,
    contributor_commits: Vec<i64>,
    stars_30d: Option<u64>,
    total_stars: u64,
    analysis_complete: bool,
    stars_complete: bool,
}

fn evaluate_repo_badges(
    evidence: &RepoBadgeEvidence,
    today: chrono::NaiveDate,
) -> Vec<EarnedRepoBadge> {
    let active = evidence.analysis_complete
        && evidence
            .latest_commit
            .is_some_and(|day| day >= today - chrono::Duration::days(29))
        && evidence.commits_30d >= 5;
    let contributors = evidence.contributor_commits.len() as u64;
    let total_commits = evidence
        .contributor_commits
        .iter()
        .copied()
        .fold(0i64, i64::saturating_add);
    let ownership =
        crate::repo_charts::compute_bus_factor(&evidence.contributor_commits, total_commits) as u64;
    let community = evidence.analysis_complete && contributors >= 5 && ownership >= 2;
    let momentum = evidence.stars_complete
        && evidence.stars_30d.is_some_and(|gain| {
            gain >= 25 && (gain >= 100 || gain.saturating_mul(100) >= evidence.total_stars.max(1))
        });

    vec![
        EarnedRepoBadge {
            id: "active",
            label: "actively maintained",
            detail: if evidence.analysis_complete {
                format!(
                    "{} commits / 30d",
                    crate::badge::humanize(evidence.commits_30d)
                )
            } else {
                "analysis pending".to_string()
            },
            earned: active,
            pending: !evidence.analysis_complete,
        },
        EarnedRepoBadge {
            id: "community",
            label: "community powered",
            detail: if evidence.analysis_complete {
                format!("bus factor {ownership} / {contributors} contributors")
            } else {
                "analysis pending".to_string()
            },
            earned: community,
            pending: !evidence.analysis_complete,
        },
        EarnedRepoBadge {
            id: "momentum",
            label: "star momentum",
            detail: evidence.stars_30d.map_or_else(
                || "collecting star data".to_string(),
                |gain| format!("+{} stars / 30d", crate::badge::humanize(gain)),
            ),
            earned: momentum,
            pending: !evidence.stars_complete,
        },
    ]
}

async fn load_repo_badges(
    state: &ApiState,
    repo_full: &str,
) -> Result<Vec<EarnedRepoBadge>, ApiError> {
    let db = state.analyzer.cache.db();
    let today = Utc::now().date_naive();
    let readiness = repo_render_readiness(state, repo_full).await?;
    let (latest_commit, commits_30d, contributor_commits) = if readiness.analysis {
        let activity = sqlx::query(
            "SELECT MAX(day) AS latest_commit, \
                    COALESCE(SUM(commits) FILTER \
                        (WHERE day >= $2), 0)::BIGINT AS commits_30d \
             FROM repo_commit_days WHERE repo = $1",
        )
        .bind(repo_full)
        .bind(today - chrono::Duration::days(29))
        .fetch_one(&db.pool)
        .await?;
        let latest_commit: Option<chrono::NaiveDate> = activity.try_get("latest_commit")?;
        let commits_30d: i64 = activity.try_get("commits_30d")?;

        // Badge qualification follows the same broad bot exclusions as the
        // contributor chart. The result contains only aggregate commit counts.
        let contributor_commits = sqlx::query_scalar::<_, i64>(
            "SELECT commits FROM repo_author_stats \
             WHERE repo = $1 \
               AND author_name NOT LIKE '%[bot]%' \
               AND author_email NOT LIKE '%[bot]@%' \
               AND (github_login IS NULL OR github_login NOT LIKE '%[bot]%') \
             ORDER BY commits DESC",
        )
        .bind(repo_full)
        .fetch_all(&db.pool)
        .await?
        .into_iter()
        .map(|value| value.max(0))
        .collect::<Vec<_>>();
        (
            latest_commit,
            commits_30d.max(0) as u64,
            contributor_commits,
        )
    } else {
        (None, 0, Vec::new())
    };

    let summary = state.analyzer.cache.get_repo_summary(repo_full).await?;
    let (stars_30d, total_stars) = if readiness.stars {
        let rows = export::accumulate(&export::load_day_deltas(db, repo_full).await?);
        let gain = rows
            .iter()
            .filter(|row| row.date >= today - chrono::Duration::days(29))
            .map(|row| row.delta)
            .sum();
        let total = summary
            .as_ref()
            .and_then(|value| value.star_count)
            .filter(|value| *value >= 0)
            .map(|value| value as u64)
            .or_else(|| rows.last().map(|row| row.total))
            .unwrap_or(0);
        (Some(gain), total)
    } else {
        (
            None,
            if readiness.metadata {
                summary
                    .as_ref()
                    .and_then(|value| value.star_count)
                    .unwrap_or(0)
                    .max(0) as u64
            } else {
                0
            },
        )
    };

    Ok(evaluate_repo_badges(
        &RepoBadgeEvidence {
            latest_commit,
            commits_30d,
            contributor_commits,
            stars_30d,
            total_stars,
            analysis_complete: readiness.analysis,
            stars_complete: readiness.stars,
        },
        today,
    ))
}

struct ContributorReadinessSignal {
    present: usize,
    total: usize,
    ready: bool,
}

async fn load_contributor_readiness(
    state: &ApiState,
    repo: &str,
) -> Result<Option<ContributorReadinessSignal>, ApiError> {
    let row = sqlx::query(
        "SELECT r.readme, r.security, r.cla, r.code_of_conduct, r.contributing, \
                r.license, r.codeowners, r.changelog, r.issue_templates, \
                r.pr_template, r.ci, r.tests, r.dependency_updates \
         FROM repo_readiness r \
         JOIN repo_history h ON h.repo = r.repo AND h.head_sha = r.head_sha \
         JOIN repos public_repo ON public_repo.repo = r.repo \
         WHERE r.repo = $1 AND public_repo.missing = FALSE \
           AND public_repo.metadata_fetched_at IS NOT NULL",
    )
    .bind(repo)
    .fetch_optional(&state.analyzer.cache.db().pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let values = [
        row.try_get::<bool, _>("readme")?,
        row.try_get::<bool, _>("security")?,
        row.try_get::<bool, _>("cla")?,
        row.try_get::<bool, _>("code_of_conduct")?,
        row.try_get::<bool, _>("contributing")?,
        row.try_get::<bool, _>("license")?,
        row.try_get::<bool, _>("codeowners")?,
        row.try_get::<bool, _>("changelog")?,
        row.try_get::<bool, _>("issue_templates")?,
        row.try_get::<bool, _>("pr_template")?,
        row.try_get::<bool, _>("ci")?,
        row.try_get::<bool, _>("tests")?,
        row.try_get::<bool, _>("dependency_updates")?,
    ];
    let readme = values[0];
    let security = values[1];
    let conduct = values[3];
    let contributing = values[4];
    let license = values[5];
    let issue_templates = values[8];
    let pr_template = values[9];
    let ci = values[10];
    let tests = values[11];
    let present = values.iter().filter(|value| **value).count();
    Ok(Some(ContributorReadinessSignal {
        present,
        total: values.len(),
        // A contributor should be able to understand, legally use, change,
        // test, and submit the project. CLA is deliberately informational:
        // many welcoming projects intentionally do not require one.
        ready: readme
            && contributing
            && license
            && conduct
            && ci
            && tests
            && (issue_templates || pr_template)
            && security,
    }))
}

async fn earned_badges_json(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let repo_full = crate::analyzer::repo_key(&owner, &repo);
    let badges = load_repo_badges(&state, &repo_full).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, s-maxage=300, max-age=60"),
    );
    Ok((headers, Json(badges)))
}

/// Query params for the badge endpoints. See AGENTS / the badge-studio
/// contract for the exact param vocabulary.
#[derive(Debug, Default, Clone, Deserialize)]
struct BadgeQuery {
    theme: Option<String>,
    signal: Option<String>,
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
        let signal = self.signal.as_deref().unwrap_or("-");
        format!(
            "{theme_key}|m:{metrics}|s:{style}|a:{}|signal:{signal}|src:{source}|{}|{}|{}|{}|{RENDER_REVISION}",
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

/// Render-revision constant baked into every media cache key (badges,
/// cards, OG images, GIFs, charts). Bump it whenever renderer output
/// changes for identical data so stale in-process/CDN entries can never
/// serve the previous look under the same key. `r19` = star exports use the
/// app-blue density wave, comparisons/cards/stat charts gain GitHub-safe GIF
/// motion, and the profile-card hierarchy becomes data-scaled.
pub(crate) const RENDER_REVISION: &str = "r19";

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
    let public = summary
        .as_ref()
        .is_some_and(|value| !value.missing && value.metadata_fetched_at.is_some());
    let stars = public
        && summary
            .as_ref()
            .is_some_and(|value| value.stargazers_complete);
    let metadata = public;
    let analysis = public && analysis_sha.is_some() && !analysis_active;
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
    if let Some(signal) = q.signal.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        if signal == "contributor-ready" {
            let readiness = load_contributor_readiness(state, &repo_full).await?;
            let (detail, ready) = readiness
                .map(|value| {
                    (
                        format!("{}/{} project guides", value.present, value.total),
                        value.ready,
                    )
                })
                .unwrap_or_else(|| ("analysis pending".to_string(), false));
            return Ok(RenderedCard {
                svg: crate::badge::render_signal_badge(
                    "contributor ready",
                    &detail,
                    ready,
                    theme,
                    q.animate(),
                ),
                short_ttl: true,
            });
        }
        let badges = load_repo_badges(state, &repo_full).await?;
        let earned = badges
            .iter()
            .find(|badge| badge.id == signal)
            .ok_or_else(|| ApiError::bad_request("unknown badge signal"))?;
        return Ok(RenderedCard {
            svg: crate::badge::render_signal_badge(
                earned.label,
                &earned.detail,
                earned.earned,
                theme,
                q.animate(),
            ),
            // Qualification depends on a moving 30-day window and should
            // self-correct quickly in an existing README.
            short_ttl: true,
        });
    }
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
        .get_repo_star_count(&repo_full)
        .await
        .ok()
        .flatten()
        .map(|value| value.max(0) as u64);
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
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = build_badge_svg(&state, &owner, &repo, theme, &q).await?;
    Ok(card_svg_response(&request_headers, card))
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
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn badge_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<BadgeQuery>,
    request_headers: HeaderMap,
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
        &request_headers,
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
    let key = format!(
        "og:{slug}|{theme_key}|{fmt_key}|{}|{RENDER_REVISION}",
        readiness.revision,
    );
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
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn repo_og_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<OgQuery>,
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

/// User-profile OG card. Mirrors the repo OG structure: the same
/// Postgres-only [`load_user_card_data`] source as the user card, the same
/// completeness gates (no data → short-TTL placeholder that self-heals;
/// analysis pending → short-TTL, never pinned in the 24h cache), and the
/// same cache-key + revision discipline (content digest + render revision).
async fn ensure_user_og_raster(
    state: &ApiState,
    login: &str,
    theme: &crate::theme::Theme,
    format: crate::raster::RasterFormat,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    if !cards::is_valid_login(login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let login = login.to_ascii_lowercase();
    let data = load_user_card_data(state.analyzer.cache.db(), &login).await?;
    if !data.has_data() {
        // Nothing tracked yet — placeholder at short TTL, never cached,
        // and never an enqueue (cards/OGs don't drive ingestion).
        let svg = og::render_user_empty_og(&login, theme);
        return Ok((
            std::sync::Arc::new(rasterize_limited(svg, format, OG_RASTER_SCALE).await?),
            true,
        ));
    }
    let stable = !data.analysis_pending();
    let svg = og::render_user_og(&data, theme);
    let theme_key = if theme.dark { "dark" } else { "light" };
    let fmt_key = raster_fmt_key(format);
    let key = format!(
        "og-user:{login}|{theme_key}|{fmt_key}|{}|{RENDER_REVISION}",
        svg_digest(&svg),
    );
    if !stable {
        return Ok((
            std::sync::Arc::new(rasterize_limited(svg, format, OG_RASTER_SCALE).await?),
            true,
        ));
    }
    if let Some(cached) = state.raster_cache.get(&key).await {
        return Ok((cached, false));
    }
    Ok((
        rasterize_cached_scaled(state, &key, svg, format, OG_RASTER_SCALE).await?,
        false,
    ))
}

async fn user_og_png(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<OgQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = og_theme(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_og_raster(&state, &login, theme, crate::raster::RasterFormat::Png).await?;
    Ok(og_response(
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn user_og_webp(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<OgQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = og_theme(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_og_raster(&state, &login, theme, crate::raster::RasterFormat::Webp).await?;
    Ok(og_response(
        &request_headers,
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
    let key = format!("og-site:{repos_key}|{theme_key}|{fmt_key}|{revision}|{RENDER_REVISION}");
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
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = og_theme(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_site_og_raster(&state, theme, &q, crate::raster::RasterFormat::Png).await?;
    Ok(og_response(
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn site_og_webp(
    State(state): State<ApiState>,
    Query(q): Query<OgQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = og_theme(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_site_og_raster(&state, theme, &q, crate::raster::RasterFormat::Webp).await?;
    Ok(og_response(
        &request_headers,
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
    request_headers: &HeaderMap,
    format: crate::raster::RasterFormat,
    bytes: std::sync::Arc<Vec<u8>>,
    short_ttl: bool,
) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    conditional_media_response(request_headers, headers, (*bytes).clone())
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
        k.push('|');
        k.push_str(RENDER_REVISION);
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

enum AnalyzeMiss {
    Live(String),
    Failed(ApiError),
}

/// `/analyze` single-flight with a conditional cache policy. Complete
/// snapshots are memoized for five minutes; pending/backfilling snapshots use
/// moka's error arm so concurrent requests still coalesce but the live body
/// is never cached. This prevents a same-repo burst from repeating the daily
/// aggregate query or metadata enqueue work 100 times.
async fn single_flight_analyze(
    cache: &MokaCache<String, String>,
    key: String,
    init: impl std::future::Future<Output = Result<(String, bool), ApiError>>,
) -> Result<(String, bool), ApiError> {
    match cache
        .try_get_with(key, async {
            let (json, live) = init.await.map_err(AnalyzeMiss::Failed)?;
            if live {
                Err(AnalyzeMiss::Live(json))
            } else {
                Ok(json)
            }
        })
        .await
    {
        Ok(json) => Ok((json, false)),
        Err(error) => match &*error {
            AnalyzeMiss::Live(json) => Ok((json.clone(), true)),
            AnalyzeMiss::Failed(error) => Err(error.clone_shared()),
        },
    }
}

/// Single-flight string memo. Concurrent misses for the same `key` coalesce
/// onto ONE `init` future (moka `try_get_with`) instead of stampeding the
/// origin — a celebrity-repo miss on a viral README embed then runs the
/// aggregate/load once, not once per concurrent request. Determinism is
/// preserved: the closure is the same pure render
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

/// Miss outcome for the single-flight GIF encode: [`RenderMiss`]'s policy
/// applied to raster bytes. `Pending` (incomplete cached stargazers) rides
/// the `Err` arm so moka never memoizes it into the 24h raster cache — the
/// frame is served short-TTL and re-encoded on the next request once the
/// star queue catches up. `Failed` carries a real error.
enum GifMiss {
    Pending(std::sync::Arc<Vec<u8>>),
    Failed(ApiError),
}

impl From<ApiError> for GifMiss {
    fn from(e: ApiError) -> Self {
        GifMiss::Failed(e)
    }
}

/// Single-flight GIF memo over the raster cache. Coalesces concurrent
/// misses for the same key onto ONE `init` future (moka `try_get_with`) —
/// a GIF encode rasterizes up to 14 frames, so a same-key stampede must
/// run it once, not once per request. Complete encodes are memoized for
/// 24h and served `(bytes, short_ttl = false)`; pending encodes are served
/// `(bytes, true)` and never cached. Errors are never memoized.
async fn single_flight_gif(
    cache: &MokaCache<String, std::sync::Arc<Vec<u8>>>,
    key: String,
    init: impl std::future::Future<Output = Result<std::sync::Arc<Vec<u8>>, GifMiss>>,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    match cache.try_get_with(key, init).await {
        Ok(bytes) => Ok((bytes, false)),
        Err(arc) => match &*arc {
            GifMiss::Pending(bytes) => Ok((bytes.clone(), true)),
            GifMiss::Failed(e) => Err(e.clone_shared()),
        },
    }
}

/// Cache policy for COMPLETE media renders: browsers hold the bytes for an
/// hour, Cloudflare's edge for four (`s-maxage=14400`), and the edge may
/// serve a stale copy for a day while it revalidates in the background —
/// so a viral README or a 2k-visitor spike hits the origin once per asset
/// per ~4h instead of per request. Renderers are deterministic, so a
/// revalidation is almost always a cheap 304.
const MEDIA_CACHE_CONTROL: &str =
    "public, max-age=3600, s-maxage=14400, stale-while-revalidate=86400";
/// Cache policy for pending/cold renders: short enough that a placeholder
/// self-heals within minutes once the queues catch up. A pending frame
/// must NEVER ride the 4h edge policy.
const PENDING_CACHE_CONTROL: &str = "public, s-maxage=300, max-age=60";

fn card_cache_control(short_ttl: bool) -> HeaderValue {
    if short_ttl {
        HeaderValue::from_static(PENDING_CACHE_CONTROL)
    } else {
        HeaderValue::from_static(MEDIA_CACHE_CONTROL)
    }
}

/// Strong ETag for a deterministic media body: `hex(sha256(body))`
/// truncated to 32 hex chars (128 bits — collision-free for cache
/// revalidation), double-quoted per RFC 9110. Renderers are
/// bytes-deterministic, so identical inputs always revalidate.
pub(crate) fn media_etag(body: &[u8]) -> HeaderValue {
    use std::fmt::Write;
    let digest = Sha256::digest(body);
    let mut tag = String::with_capacity(34);
    tag.push('"');
    for byte in &digest[..16] {
        let _ = write!(tag, "{byte:02x}");
    }
    tag.push('"');
    // Quoted hex is always a valid header value.
    HeaderValue::from_str(&tag).expect("hex etag is a valid header value")
}

/// RFC 9110 §13.1.2 `If-None-Match` evaluation against a strong ETag:
/// `*` matches any current representation; otherwise the field is a
/// comma-separated entity-tag list compared with the *weak* comparison
/// (a `W/` prefix on a candidate is ignored; entity-tags cannot contain
/// commas, so the split is lossless).
pub(crate) fn if_none_match_matches(request_headers: &HeaderMap, etag: &HeaderValue) -> bool {
    let Ok(target) = etag.to_str() else {
        return false;
    };
    request_headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| {
            candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == target
        })
}

/// Shared conditional-media responder: every media body (SVG/PNG/WebP/GIF/
/// OG) flows through here exactly once. Attaches the strong ETag; when the
/// request's `If-None-Match` matches, answers `304 Not Modified` with the
/// SAME `Cache-Control` + `ETag` (per RFC 9110 §15.4.5) and an empty body,
/// so CDN revalidations cost headers instead of image bytes.
pub(crate) fn conditional_media_response(
    request_headers: &HeaderMap,
    mut headers: HeaderMap,
    body: Vec<u8>,
) -> axum::response::Response {
    let etag = media_etag(&body);
    if if_none_match_matches(request_headers, &etag) {
        let mut not_modified = HeaderMap::new();
        if let Some(cache_control) = headers.remove(header::CACHE_CONTROL) {
            not_modified.insert(header::CACHE_CONTROL, cache_control);
        }
        not_modified.insert(header::ETAG, etag);
        return (StatusCode::NOT_MODIFIED, not_modified).into_response();
    }
    headers.insert(header::ETAG, etag);
    (headers, body).into_response()
}

fn card_svg_response(request_headers: &HeaderMap, card: RenderedCard) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, card_cache_control(card.short_ttl));
    let body = crate::brand::with_site_link(card.svg);
    conditional_media_response(request_headers, headers, body.into_bytes())
}

fn card_raster_response(
    request_headers: &HeaderMap,
    format: crate::raster::RasterFormat,
    bytes: std::sync::Arc<Vec<u8>>,
    short_ttl: bool,
) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    conditional_media_response(request_headers, headers, (*bytes).clone())
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
///
/// Every aggregate column is table-qualified. Both statements join `repos`
/// to a second relation that also has a `repo` column, so an unqualified
/// `repo`/`commits`/`first_commit_at` is a planner-level ambiguity error —
/// a 500 on every profile card, not a wrong number. `card_sql_*` in the
/// tests below execute these exact statements against Postgres for that
/// reason.
///
/// `repos_analyzed` counts repos whose *analysis* is done and current — the
/// same three conditions as [`crate::repo_analysis::analysis_is_current`]
/// minus the enqueue-only freshness window, plus "no live queue row".
/// Author login/avatar enrichment is deliberately not part of it: it is
/// presentation-only metadata resolved best-effort against the GitHub API,
/// and gating this count on it pinned profiles at "Analyzing N
/// repositories" forever for any repo with unresolvable author emails.
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
                       AND history.analysis_revision >= $2 \
                       AND history.last_analyzed_sha IS NOT NULL \
                       AND history.head_sha = history.last_analyzed_sha \
                       AND active.repo IS NULL) AS repos_analyzed \
         FROM repos \
         LEFT JOIN repo_history history ON history.repo = repos.repo \
         LEFT JOIN repo_analysis_queue active \
           ON active.repo = repos.repo \
          AND active.status IN ('pending', 'in_progress') \
         WHERE repos.repo LIKE $1 || '/%' \
           AND NOT repos.missing \
           AND repos.metadata_fetched_at IS NOT NULL",
    )
    .bind(login)
    .bind(crate::repo_analysis::CURRENT_ANALYSIS_REVISION)
    .fetch_one(&db.pool)
    .await?;
    let repos_tracked: i64 = owned.try_get("repos_tracked")?;
    let stars: i64 = owned.try_get("stars")?;
    let forks: i64 = owned.try_get("forks")?;
    let repos_analyzed: i64 = owned.try_get("repos_analyzed")?;

    let authored = sqlx::query(
        "SELECT COALESCE(SUM(author.commits), 0)::BIGINT AS commits, \
                COUNT(DISTINCT author.repo) AS contribs, \
                MIN(author.first_commit_at) AS first_at \
         FROM repo_author_stats author \
         JOIN repos public_repo ON public_repo.repo = author.repo \
         WHERE LOWER(author.github_login) = $1 \
           AND public_repo.missing = FALSE \
           AND public_repo.metadata_fetched_at IS NOT NULL",
    )
    .bind(login)
    .fetch_one(&db.pool)
    .await?;
    let commits: i64 = authored.try_get("commits")?;
    let contribs: i64 = authored.try_get("contribs")?;
    let first_at: Option<chrono::DateTime<chrono::Utc>> = authored.try_get("first_at")?;

    let scope = load_profile_scope(&db.pool, login).await?;
    let langs = load_owner_top_langs(db, &scope).await?;

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

/// Card languages across a [`ProfileScope`]. Ties are broken by name so
/// rendered card bytes remain deterministic.
async fn load_owner_top_langs(
    db: &crate::db::Db,
    repos: &[String],
) -> Result<Vec<(String, i64)>, ApiError> {
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT lines.language, \
                CASE WHEN SUM(lines.lines_code) > 0 THEN SUM(lines.lines_code) \
                     ELSE SUM(lines.files) END::BIGINT AS lines \
         FROM repo_lines lines \
         WHERE lines.repo = ANY($1::text[]) \
         GROUP BY lines.language ORDER BY lines DESC, lines.language LIMIT 5",
    )
    .bind(repos)
    .fetch_all(&db.pool)
    .await?;
    collect_top_langs(rows)
}

/// Card languages for a single repository.
async fn load_repo_top_langs(
    db: &crate::db::Db,
    repo: &str,
) -> Result<Vec<(String, i64)>, ApiError> {
    let rows = sqlx::query(
        "SELECT lines.language, \
                CASE WHEN SUM(lines.lines_code) > 0 THEN SUM(lines.lines_code) \
                     ELSE SUM(lines.files) END::BIGINT AS lines \
         FROM repo_lines lines \
         JOIN repos public_repo ON public_repo.repo = lines.repo \
         WHERE lines.repo = $1 \
           AND public_repo.missing = FALSE \
           AND public_repo.metadata_fetched_at IS NOT NULL \
         GROUP BY lines.language ORDER BY lines DESC, lines.language LIMIT 5",
    )
    .bind(repo)
    .fetch_all(&db.pool)
    .await?;
    collect_top_langs(rows)
}

fn collect_top_langs(rows: Vec<sqlx::postgres::PgRow>) -> Result<Vec<(String, i64)>, ApiError> {
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

    // Total lines, or `None` when the repository only has a file census —
    // its line columns are zero because the count was not run, and printing
    // that as a confident "0 lines of code" is what every large or
    // asset-heavy repository used to render.
    let lines_total: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(lines_code)::BIGINT FROM repo_lines          WHERE repo = $1 AND lines_exact",
    )
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

    let langs = load_repo_top_langs(db, repo_full).await?;

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

/// Data version for a repository card. Beyond the star series it reports
/// commits, contributors and line counts, so it also tracks the analysis head
/// and the public-metadata stamp (which is what clears a tombstone).
async fn repo_card_revision(
    state: &ApiState,
    repo_full: &str,
    summary: Option<&crate::cache::RepoSummary>,
) -> Result<String, ApiError> {
    let analysis_sha: Option<String> =
        sqlx::query_scalar("SELECT last_analyzed_sha FROM repo_history WHERE repo = $1")
            .bind(repo_full)
            .fetch_optional(&state.analyzer.cache.db().pool)
            .await?
            .flatten();
    Ok(format!(
        "{}|m:{}|a:{}",
        star_data_revision(summary),
        summary
            .and_then(|value| value.metadata_fetched_at)
            .map(|value| value.timestamp_millis())
            .unwrap_or(0),
        analysis_sha.as_deref().unwrap_or("-"),
    ))
}

/// Data version for a profile surface: how many of the login's repositories
/// are analyzed, when the newest pass landed, and the star totals behind it.
/// One indexed aggregate — cheap enough for a memo key, and it moves whenever
/// anything the profile renders moves.
async fn user_data_revision(state: &ApiState, login: &str) -> Result<String, ApiError> {
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT, \
                COALESCE(MAX(EXTRACT(EPOCH FROM repos.stargazers_fetched_at)), 0)::BIGINT, \
                COALESCE(SUM(GREATEST(repos.star_count, 0)), 0)::BIGINT \
         FROM login_repos \
         JOIN repos ON repos.repo = login_repos.repo \
         WHERE login_repos.login = $1",
    )
    .bind(login)
    .fetch_one(&state.analyzer.cache.db().pool)
    .await?;
    Ok(format!("u:{}:{}:{}", row.0, row.1, row.2))
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
    let key = format!(
        "card:user:{login}|{}|{}",
        q.key_fragment(theme),
        user_data_revision(state, &login).await?
    );
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
    // Load the card's inputs *before* the memo probe: every number on this
    // card moves underneath a slug-only key, and nothing invalidates the
    // media caches, so the key has to carry the data version.
    let summary = state.analyzer.cache.get_repo_summary(&repo_full).await?;
    let key = format!(
        "card:repo:{repo_full}|{}|{}",
        q.key_fragment(theme),
        repo_card_revision(state, &repo_full, summary.as_ref()).await?
    );
    if let Some(svg) = state.stat_svg_cache.get(&key).await {
        return Ok(RenderedCard {
            svg,
            short_ttl: false,
        });
    }
    if summary.as_ref().is_some_and(|s| s.missing) {
        // A tombstone is reversible — a repository can be made public again,
        // and the metadata write clears the flag — so it rides the same
        // self-healing short TTL as the pending card instead of the four-hour
        // edge policy.
        return Ok(RenderedCard {
            svg: cards::render_repo_missing_card(&repo_full, theme),
            short_ttl: true,
        });
    }
    let Some(summary) = summary else {
        return Ok(RenderedCard {
            svg: cards::render_repo_pending_card(&repo_full, None, theme),
            short_ttl: true,
        });
    };
    if summary.metadata_fetched_at.is_none() {
        return Ok(RenderedCard {
            svg: cards::render_repo_pending_card(&repo_full, None, theme),
            short_ttl: true,
        });
    }
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

async fn ensure_user_card_gif(
    state: &ApiState,
    login: &str,
    theme: &crate::theme::Theme,
    q: &CardQuery,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    if !cards::is_valid_login(login) {
        return Err(ApiError::bad_request("invalid login"));
    }
    let login = login.to_ascii_lowercase();
    let card = ensure_user_card_svg(state, &login, theme, q).await?;
    let key = format!(
        "card:user-gif:{login}|{}|svg:{}|{RENDER_REVISION}",
        q.key_fragment(theme),
        svg_digest(&card.svg),
    );
    let short_ttl = card.short_ttl;
    let svg = card.svg;
    single_flight_gif(&state.raster_cache, key, async move {
        let encoded = with_raster_permit(move || crate::animated_gif::encode_dither_loop(&svg))
            .await?
            .map_err(ApiError::from)?;
        let bytes = std::sync::Arc::new(encoded.bytes);
        if short_ttl {
            Err(GifMiss::Pending(bytes))
        } else {
            Ok(bytes)
        }
    })
    .await
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

async fn ensure_repo_card_gif(
    state: &ApiState,
    owner: &str,
    repo: &str,
    theme: &crate::theme::Theme,
    q: &CardQuery,
) -> Result<(std::sync::Arc<Vec<u8>>, bool), ApiError> {
    if !is_valid_slug(owner) || !is_valid_slug(repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let owner = owner.to_ascii_lowercase();
    let repo = repo.to_ascii_lowercase();
    let card = ensure_repo_card_svg(state, &owner, &repo, theme, q).await?;
    let key = format!(
        "card:repo-gif:{owner}/{repo}|{}|svg:{}|{RENDER_REVISION}",
        q.key_fragment(theme),
        svg_digest(&card.svg),
    );
    let short_ttl = card.short_ttl;
    let svg = card.svg;
    single_flight_gif(&state.raster_cache, key, async move {
        let encoded = with_raster_permit(move || crate::animated_gif::encode_dither_loop(&svg))
            .await?
            .map_err(ApiError::from)?;
        let bytes = std::sync::Arc::new(encoded.bytes);
        if short_ttl {
            Err(GifMiss::Pending(bytes))
        } else {
            Ok(bytes)
        }
    })
    .await
}

async fn user_card_svg(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<CardQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_user_card_svg(&state, &login, theme, &q).await?;
    Ok(card_svg_response(&request_headers, card))
}

async fn user_card_png(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<CardQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_card_raster(&state, &login, theme, &q, crate::raster::RasterFormat::Png)
            .await?;
    Ok(card_raster_response(
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn user_card_webp(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<CardQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) =
        ensure_user_card_raster(&state, &login, theme, &q, crate::raster::RasterFormat::Webp)
            .await?;
    Ok(card_raster_response(
        &request_headers,
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

async fn user_card_gif(
    State(state): State<ApiState>,
    Path(login): Path<String>,
    Query(q): Query<CardQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_user_card_gif(&state, &login, theme, &q).await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/gif"));
    headers.insert(header::CACHE_CONTROL, card_cache_control(short_ttl));
    let filename = format!(
        "inline; filename=\"{}-profile-card-{}.gif\"",
        login.to_ascii_lowercase(),
        if theme.dark { "dark" } else { "light" }
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&filename).map_err(|_| ApiError::bad_request("invalid login"))?,
    );
    Ok(conditional_media_response(
        &request_headers,
        headers,
        (*bytes).clone(),
    ))
}

async fn repo_card_svg(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<CardQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let card = ensure_repo_card_svg(&state, &owner, &repo, theme, &q).await?;
    Ok(card_svg_response(&request_headers, card))
}

async fn repo_card_png(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<CardQuery>,
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Png,
        bytes,
        short_ttl,
    ))
}

async fn repo_card_webp(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<CardQuery>,
    request_headers: HeaderMap,
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
        &request_headers,
        crate::raster::RasterFormat::Webp,
        bytes,
        short_ttl,
    ))
}

async fn repo_card_gif(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(q): Query<CardQuery>,
    request_headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let theme = theme_for(q.theme.as_deref());
    let (bytes, short_ttl) = ensure_repo_card_gif(&state, &owner, &repo, theme, &q).await?;
    gif_media_response(
        &request_headers,
        bytes,
        short_ttl,
        &format!(
            "{}-{}-repository-card-{}.gif",
            owner.to_ascii_lowercase(),
            repo.to_ascii_lowercase(),
            if theme.dark { "dark" } else { "light" }
        ),
    )
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
    // An unbounded `page` is an unbounded cache-key space in front of a
    // COUNT plus a sorted scan: every distinct value misses the edge and
    // costs a full pass at the origin. Pages past the end have no content to
    // serve, so they are a client error rather than an expensive empty page.
    if offset > 0 && offset >= total {
        return Err(ApiError::bad_request("page beyond the last page"));
    }
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

/// Supported trailing windows for daily, weekly, and monthly momentum.
const LEADERBOARD_WINDOW_DEFAULT: i64 = 7;
const LEADERBOARD_WINDOWS: &[i64] = &[1, 7, 30];
/// Default + max page size. The default matches the contract (`per=50`).
const LEADERBOARD_PER_DEFAULT: i64 = 50;
pub(crate) const LEADERBOARD_PER_MAX: i64 = 100;
/// Deep pagination is a scraper pattern, not a reader pattern. Capping
/// `page` bounds the OFFSET the DB is asked to scan past.
pub(crate) const LEADERBOARD_PAGE_MAX: i64 = 200;

#[derive(Debug, Default, Clone, Deserialize)]
struct LeaderboardQuery {
    metric: Option<String>,
    window: Option<i64>,
    per: Option<i64>,
    page: Option<i64>,
}

/// Normalize + validate leaderboard params. An unknown metric is a 400
/// (fail loudly — a typo'd client should not silently get the wrong
/// ranking); `per`/`page` are clamped into DoS-safe bounds.
fn leaderboard_params(
    metric: Option<&str>,
    window: Option<i64>,
    per: Option<i64>,
    page: Option<i64>,
) -> Result<(LeaderboardMetric, i64, i64, i64), &'static str> {
    let metric = match metric.unwrap_or("stars") {
        "stars" => LeaderboardMetric::Stars,
        "velocity" => LeaderboardMetric::Velocity,
        _ => return Err("invalid metric (expected stars or velocity)"),
    };
    let window = window.unwrap_or(LEADERBOARD_WINDOW_DEFAULT);
    if !LEADERBOARD_WINDOWS.contains(&window) {
        return Err("invalid window (expected 1, 7, or 30)");
    }
    let per = per
        .unwrap_or(LEADERBOARD_PER_DEFAULT)
        .clamp(1, LEADERBOARD_PER_MAX);
    let page = page.unwrap_or(0).clamp(0, LEADERBOARD_PAGE_MAX);
    Ok((metric, window, per, page))
}

/// One leaderboard row: `(slug, total stars, stars added in the trailing
/// window)`. Only repos with **complete** cached star history participate
/// (readers never trust partial data) and tombstoned repos are excluded,
/// so every row links to a live, indexable repo page.
async fn load_leaderboard_rows(
    state: &ApiState,
    metric: LeaderboardMetric,
    window_days: i64,
    limit: i64,
    offset: i64,
) -> Result<Vec<(String, i64, i64)>, ApiError> {
    let pool = &state.analyzer.cache.db().pool;
    let snapshot_rows = sqlx::query(
        "SELECT repo, stars, velocity FROM leaderboard_snapshots \
         WHERE metric = $1 AND window_days = $2 \
         ORDER BY rank ASC LIMIT $3 OFFSET $4",
    )
    .bind(metric.as_str())
    .bind(window_days as i32)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(anyhow::Error::from)?;
    if !snapshot_rows.is_empty() {
        return snapshot_rows
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get("repo").map_err(anyhow::Error::from)?,
                    row.try_get("stars").map_err(anyhow::Error::from)?,
                    row.try_get("velocity").map_err(anyhow::Error::from)?,
                ))
            })
            .collect::<Result<Vec<_>, ApiError>>();
    }

    // First-start fallback while the initial durable snapshot is building.
    // It preserves availability without making the expensive aggregation the
    // steady-state request path.
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
               AND r.metadata_fetched_at IS NOT NULL \
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
               AND r.metadata_fetched_at IS NOT NULL \
             ORDER BY v.velocity DESC, r.repo ASC \
             LIMIT $1 OFFSET $2"
        }
    };
    let rows = sqlx::query(sql)
        .bind(limit)
        .bind(offset)
        .bind(window_days as i32)
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

/// `GET /api/leaderboard.json?metric=stars|velocity&window=1|7|30&per=50&page=0` —
/// ranked repos from the repos/repo_stargazers tables only. No GitHub
/// calls on this path. Memoized 5 min in its OWN moka cache (see the
/// `leaderboard_cache` field docs — the param-derived key space must not
/// be able to evict warm `/analyze` bodies) and served with the same
/// cache envelope as `/analyze`.
async fn leaderboard_json(
    State(state): State<ApiState>,
    Query(q): Query<LeaderboardQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let (metric, window, per, page) =
        leaderboard_params(q.metric.as_deref(), q.window, q.per, q.page)
            .map_err(ApiError::bad_request)?;
    let key = format!("leaderboard:{}:{window}:{per}:{page}", metric.as_str());
    // Single-flight: the trailing-window velocity GROUP BY / stars LATERAL
    // is the heaviest read on the largest table; coalesce concurrent misses
    // for the same page onto one query instead of a stampede.
    let json = single_flight(&state.leaderboard_cache, key, async {
        let rows =
            load_leaderboard_rows(&state, metric, window, per, page.saturating_mul(per)).await?;
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
            "window_days": window,
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
    gained_7d: i64,
    gained_30d: i64,
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
            gained_7d: row.gained_7d,
            gained_30d: row.gained_30d,
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
pub(crate) static RASTER_PERMITS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| {
        std::sync::Arc::new(tokio::sync::Semaphore::new(RASTER_CONCURRENCY))
    });

/// Run one CPU-bound render/encode on the blocking pool under a
/// [`RASTER_PERMITS`] permit. Every raster-class workload — resvg
/// rasterization AND the GIF frame encoder (which rasterizes up to 14
/// frames per request) — must go through here so a burst of misses queues
/// briefly on the semaphore instead of saturating every core.
pub(crate) async fn with_raster_permit<T, F>(work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    // The permit is moved INTO the blocking closure, so it is released when
    // the CPU work finishes rather than when this future is dropped. A
    // `spawn_blocking` body cannot be cancelled: with the permit held by the
    // caller's future, a timed-out request handed its permit to the next
    // request while its own encode kept running, and the cap stopped bounding
    // anything precisely during the bursts it exists for.
    let permit = RASTER_PERMITS
        .clone()
        .acquire_owned()
        .await
        .expect("raster semaphore is never closed");
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|e| ApiError::from(anyhow::anyhow!("raster task: {e}")))
}

/// The single choke point every raster path (charts, cards, OG, stat
/// SVGs) must go through: semaphore-capped `spawn_blocking` around
/// [`crate::raster::rasterize`].
pub(crate) async fn rasterize_limited(
    svg: String,
    format: crate::raster::RasterFormat,
    scale: f32,
) -> Result<Vec<u8>, ApiError> {
    with_raster_permit(move || crate::raster::rasterize(&svg, format, scale))
        .await?
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
/// admission limiter. Long-lived handlers use this to apply concurrency
/// limits that cannot be represented by a windowed admission layer.
pub(crate) fn request_client_ip(
    headers: &HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Option<IpAddr> {
    let mut request = axum::http::Request::new(());
    *request.headers_mut() = headers.clone();
    if let Some(connect_info) = connect_info {
        request.extensions_mut().insert(connect_info);
    }
    CloudflareIpKeyExtractor.extract(&request)
}

impl CloudflareIpKeyExtractor {
    fn extract<T>(&self, req: &axum::http::Request<T>) -> Option<IpAddr> {
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
                    return Some(ip);
                }
                // Trusted proxy but no cf-connecting-ip → fall back to the
                // XFF / X-Real-IP / Forwarded chain, then the peer itself.
                return smart_header_ip(req.headers()).or(Some(peer_ip));
            }
            // Untrusted direct peer: key on the peer IP, ignore headers.
            return Some(peer_ip);
        }

        // No ConnectInfo (shouldn't happen with the wiring in main.rs) —
        // fall back to the header-based extraction.
        smart_header_ip(req.headers())
    }
}

/// Header-based client-IP resolution matching tower_governor's
/// `SmartIpKeyExtractor` order: `x-forwarded-for` (first parseable entry),
/// then `x-real-ip`, then RFC 7239 `forwarded`. Callers gate this behind
/// the trusted-proxy check.
fn smart_header_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|list| {
            list.split(',')
                .find_map(|entry| entry.trim().parse::<IpAddr>().ok())
        })
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .and_then(|s| s.trim().parse::<IpAddr>().ok())
        })
        .or_else(|| {
            headers
                .get_all(header::FORWARDED)
                .iter()
                .find_map(|value| value.to_str().ok().and_then(forwarded_header_ip))
        })
}

/// Extract the first `for=` identifier from an RFC 7239 `Forwarded` header
/// value. Accepts `for=1.2.3.4`, `for="1.2.3.4:56"`, `for="[2001:db8::1]"`,
/// and `for="[2001:db8::1]:56"`; obfuscated (`_hidden`) and `unknown`
/// identifiers are skipped.
fn forwarded_header_ip(value: &str) -> Option<IpAddr> {
    value
        .split(',')
        .flat_map(|element| element.split(';'))
        .find_map(|pair| {
            let (name, raw) = pair.split_once('=')?;
            if !name.trim().eq_ignore_ascii_case("for") {
                return None;
            }
            let node = raw.trim().trim_matches('"');
            if let Ok(addr) = node.parse::<SocketAddr>() {
                return Some(addr.ip());
            }
            if let Ok(ip) = node.parse::<IpAddr>() {
                return Some(ip);
            }
            // Bracketed IPv6 without a port: `[2001:db8::1]`.
            node.strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|inner| inner.parse::<IpAddr>().ok())
        })
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

    /// Every media memo key must carry a data revision.
    ///
    /// These caches are TTL-only: nothing invalidates them, and the Redis
    /// invalidation bus reaches only the analyze/aggregate JSON caches. A key
    /// made of presentation options alone therefore pins a README embed to
    /// whatever the data was when it was first rendered — for a full day at
    /// the origin, and longer once the edge policy is layered on top. The
    /// revision has to move when the data moves and stay put when it does
    /// not, so a quiet repository keeps its memo.
    #[test]
    fn star_data_revision_tracks_the_series_and_nothing_else() {
        let at = |millis: i64| chrono::DateTime::from_timestamp_millis(millis).unwrap();
        let base = crate::cache::RepoSummary {
            missing: false,
            github_id: Some(1),
            stargazers_complete: true,
            stargazers_fetched_at: Some(at(1_000)),
            metadata_fetched_at: Some(at(1_000)),
            star_count: Some(42),
            history_source: Some("gh_archive".to_string()),
            history_observed_count: Some(40),
            history_coverage_start: None,
            history_coverage_end: None,
            created_at: None,
            view_count: 7,
            ..crate::cache::RepoSummary::default()
        };

        let unchanged = star_data_revision(Some(&base));
        assert_eq!(
            unchanged,
            star_data_revision(Some(&base.clone())),
            "identical data must produce an identical key"
        );

        // Each of these moves in the same transaction that appends stars or
        // refreshes public metadata.
        let mut newer_stars = base.clone();
        newer_stars.history_observed_count = Some(41);
        assert_ne!(unchanged, star_data_revision(Some(&newer_stars)));

        let mut refreshed = base.clone();
        refreshed.stargazers_fetched_at = Some(at(2_000));
        assert_ne!(unchanged, star_data_revision(Some(&refreshed)));

        let mut restarred = base.clone();
        restarred.star_count = Some(43);
        assert_ne!(unchanged, star_data_revision(Some(&restarred)));

        // A repository nothing is known about must not share a key with one
        // that has data.
        assert_ne!(unchanged, star_data_revision(None));
    }

    /// The metric label and the underlying table both follow
    /// `history_source`, so it belongs in the key of every variant — the
    /// raster key used to omit what the SVG key included, which let a PNG
    /// embed keep a curve and a label the SVG had already replaced.
    #[test]
    fn history_source_is_part_of_the_render_identity() {
        let mut summary = crate::cache::RepoSummary {
            missing: false,
            github_id: None,
            stargazers_complete: true,
            stargazers_fetched_at: None,
            metadata_fetched_at: None,
            star_count: None,
            history_source: Some("gh_archive".to_string()),
            history_observed_count: None,
            history_coverage_start: None,
            history_coverage_end: None,
            created_at: None,
            view_count: 0,
            ..crate::cache::RepoSummary::default()
        };
        assert_eq!(history_source_key(Some(&summary)), "archive");
        summary.history_source = Some("github_api".to_string());
        assert_eq!(history_source_key(Some(&summary)), "github");
        assert_eq!(history_source_key(None), "github");
    }

    /// The two TTL classes are distinct: pending/empty renders get the
    /// short (5-min) envelope, full renders the 4h edge policy (plus
    /// stale-while-revalidate). Cold charts must ride the short class so
    /// a first view can't pin "no data" at the edge for hours.
    #[test]
    fn card_cache_control_short_vs_long() {
        assert_eq!(
            card_cache_control(true).to_str().unwrap(),
            "public, s-maxage=300, max-age=60"
        );
        assert_eq!(
            card_cache_control(false).to_str().unwrap(),
            "public, max-age=3600, s-maxage=14400, stale-while-revalidate=86400"
        );
    }

    /// Strong ETag contract: quoted, 32 hex chars (truncated sha256),
    /// deterministic for identical bytes, distinct for different bytes.
    #[test]
    fn media_etag_is_truncated_quoted_sha256() {
        let tag = media_etag(b"hello");
        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b16...
        assert_eq!(
            tag.to_str().unwrap(),
            "\"2cf24dba5fb0a30e26e83b2ac5b9e29e\""
        );
        assert_eq!(media_etag(b"hello"), tag);
        assert_ne!(media_etag(b"other"), tag);
    }

    /// RFC 9110 `If-None-Match`: `*` matches anything, comma lists are
    /// split, weak candidates (`W/`) compare by opaque tag, and absent /
    /// non-matching headers never match.
    #[test]
    fn if_none_match_star_lists_and_weak_tags() {
        let etag = media_etag(b"body");
        let mut headers = HeaderMap::new();
        assert!(!if_none_match_matches(&headers, &etag));

        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("*"));
        assert!(if_none_match_matches(&headers, &etag));

        let list = format!("\"nope\", W/{} , \"other\"", etag.to_str().unwrap());
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_str(&list).unwrap());
        assert!(if_none_match_matches(&headers, &etag));

        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_static("\"aaaa\", \"bbbb\""),
        );
        assert!(!if_none_match_matches(&headers, &etag));
    }

    /// Full SVG render: first response is a 200 carrying an ETag + the 4h
    /// edge policy; replaying the ETag via `If-None-Match` yields a 304
    /// with the SAME Cache-Control + ETag and an empty body; a stale ETag
    /// yields a fresh 200.
    #[tokio::test]
    async fn svg_response_etag_match_is_304_and_mismatch_200() {
        let full = |svg: &str| RenderedCard {
            svg: svg.to_string(),
            short_ttl: false,
        };
        let none = HeaderMap::new();
        let first = card_svg_response(&none, full("<svg>chart</svg>"));
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(
            first.headers().get(header::CACHE_CONTROL).unwrap(),
            &card_cache_control(false)
        );
        let etag = first.headers().get(header::ETAG).unwrap().clone();

        let mut revalidate = HeaderMap::new();
        revalidate.insert(header::IF_NONE_MATCH, etag.clone());
        let not_modified = card_svg_response(&revalidate, full("<svg>chart</svg>"));
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(not_modified.headers().get(header::ETAG).unwrap(), &etag);
        assert_eq!(
            not_modified.headers().get(header::CACHE_CONTROL).unwrap(),
            &card_cache_control(false)
        );
        let body = axum::body::to_bytes(not_modified.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body.is_empty(), "304 must carry no body");

        let mut mismatched = HeaderMap::new();
        mismatched.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_static("\"deadbeefdeadbeefdeadbeefdeadbeef\""),
        );
        let fresh = card_svg_response(&mismatched, full("<svg>chart</svg>"));
        assert_eq!(fresh.status(), StatusCode::OK);
        let body = axum::body::to_bytes(fresh.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(!body.is_empty());
    }

    /// Raster responses: complete renders ride the 4h edge policy, pending
    /// renders the short one, and the conditional-request path answers 304
    /// for a matching ETag on both classes.
    #[tokio::test]
    async fn raster_response_pending_vs_complete_and_304() {
        let bytes = std::sync::Arc::new(vec![1u8, 2, 3, 4]);
        let none = HeaderMap::new();

        let complete = card_raster_response(
            &none,
            crate::raster::RasterFormat::Png,
            bytes.clone(),
            false,
        );
        assert_eq!(complete.status(), StatusCode::OK);
        assert_eq!(
            complete.headers().get(header::CACHE_CONTROL).unwrap(),
            &HeaderValue::from_static(MEDIA_CACHE_CONTROL)
        );
        assert_eq!(
            complete.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        let etag = complete.headers().get(header::ETAG).unwrap().clone();

        let pending =
            card_raster_response(&none, crate::raster::RasterFormat::Png, bytes.clone(), true);
        assert_eq!(
            pending.headers().get(header::CACHE_CONTROL).unwrap(),
            &HeaderValue::from_static(PENDING_CACHE_CONTROL)
        );

        let mut revalidate = HeaderMap::new();
        revalidate.insert(header::IF_NONE_MATCH, etag);
        let not_modified =
            card_raster_response(&revalidate, crate::raster::RasterFormat::Png, bytes, false);
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    }

    #[test]
    fn byte_weight_clamps_to_nonzero_u32() {
        assert_eq!(byte_weight(0), 1);
        assert_eq!(byte_weight(10), 10);
        assert_eq!(byte_weight(usize::MAX), u32::MAX);
    }

    /// The byte-weighed caches enforce their RAM budget: inserting more
    /// bytes than `max_capacity` evicts (or refuses) entries so the
    /// weighted size never exceeds the budget.
    #[tokio::test]
    async fn weighted_caches_evict_to_byte_budget() {
        let cache = weighted_string_cache(1024, Duration::from_secs(60));
        for i in 0..4 {
            cache.insert(format!("k{i}"), "x".repeat(512)).await;
        }
        cache.run_pending_tasks().await;
        assert!(
            cache.weighted_size() <= 1024,
            "weighted size {} exceeds budget",
            cache.weighted_size()
        );
        assert!(cache.entry_count() <= 2);

        let raster = weighted_bytes_cache(1024, Duration::from_secs(60));
        for i in 0..4 {
            raster
                .insert(format!("k{i}"), std::sync::Arc::new(vec![0u8; 512]))
                .await;
        }
        raster.run_pending_tasks().await;
        assert!(raster.weighted_size() <= 1024);
        assert!(raster.entry_count() <= 2);
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

    /// The GIF single-flight mirrors the card policy on raster bytes:
    /// complete encodes memoize into the raster cache, pending encodes are
    /// served short-TTL and never cached, real failures propagate uncached
    /// with their status preserved.
    #[tokio::test]
    async fn single_flight_gif_complete_caches_pending_and_failure_do_not() {
        let cache = weighted_bytes_cache(1024 * 1024, Duration::from_secs(60));

        let (bytes, short_ttl) = single_flight_gif(&cache, "k".into(), async {
            Ok(std::sync::Arc::new(vec![1u8, 2]))
        })
        .await
        .unwrap();
        assert!(!short_ttl, "complete encode rides the full cache policy");
        assert_eq!(*bytes, vec![1, 2]);
        assert!(cache.get("k").await.is_some(), "complete encode memoizes");

        let (bytes, short_ttl) = single_flight_gif(&cache, "p".into(), async {
            Err(GifMiss::Pending(std::sync::Arc::new(vec![9u8])))
        })
        .await
        .unwrap();
        assert!(short_ttl, "pending encode must be short-TTL");
        assert_eq!(*bytes, vec![9]);
        assert!(
            cache.get("p").await.is_none(),
            "pending encode never cached"
        );

        let err = single_flight_gif(&cache, "e".into(), async {
            Err(GifMiss::Failed(ApiError::not_found("gone")))
        })
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert!(cache.get("e").await.is_none());
    }

    /// Concurrent same-key GIF misses coalesce onto ONE encode. Regression
    /// guard for the stampede: distinct keys are bounded by RASTER_PERMITS,
    /// but a same-key burst must not re-encode at all.
    #[tokio::test]
    async fn single_flight_gif_coalesces_concurrent_misses() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = weighted_bytes_cache(1024 * 1024, Duration::from_secs(60));
        let runs = std::sync::Arc::new(AtomicUsize::new(0));
        let init = |runs: std::sync::Arc<AtomicUsize>| async move {
            runs.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(std::sync::Arc::new(vec![7u8]))
        };
        let (a, b) = tokio::join!(
            single_flight_gif(&cache, "k".into(), init(runs.clone())),
            single_flight_gif(&cache, "k".into(), init(runs.clone())),
        );
        assert_eq!(*a.unwrap().0, vec![7]);
        assert_eq!(*b.unwrap().0, vec![7]);
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "concurrent same-key misses must share one encode"
        );
    }

    /// The blocking-encode helper must queue on `RASTER_PERMITS`: with
    /// every permit held even a trivial closure cannot run, and releasing
    /// the permits lets it complete. Regression guard for the GIF path
    /// that used a bare `spawn_blocking`, bypassing the raster choke point.
    #[tokio::test]
    async fn raster_permit_helper_waits_for_capacity() {
        let held = RASTER_PERMITS
            .acquire_many(RASTER_CONCURRENCY as u32)
            .await
            .expect("raster semaphore is never closed");
        let task = tokio::spawn(with_raster_permit(|| 42u8));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !task.is_finished(),
            "encode must wait for a raster permit, not bypass the semaphore"
        );
        drop(held);
        assert_eq!(task.await.unwrap().unwrap(), 42);
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

    #[tokio::test]
    async fn analyze_single_flight_coalesces_one_hundred_live_requests() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let cache = test_string_cache();
        let runs = std::sync::Arc::new(AtomicUsize::new(0));
        let requests = (0..100).map(|_| {
            let runs = runs.clone();
            single_flight_analyze(&cache, "repo".into(), async move {
                runs.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(("pending".to_string(), true))
            })
        });
        let responses = futures::future::join_all(requests).await;
        assert!(
            responses
                .into_iter()
                .all(|response| { matches!(response, Ok((ref body, true)) if body == "pending") })
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(
            cache.get("repo").await.is_none(),
            "live snapshots must coalesce without becoming stale cache entries"
        );
    }

    /// Full `ApiState` over the test database, or `None` (test no-ops)
    /// when `GITDEBT_TEST_DATABASE_URL` is unset — the same gating
    /// convention as the integration suites.
    async fn test_db_state() -> Option<ApiState> {
        let db = crate::test_db::shared().await?;
        let rate = std::sync::Arc::new(
            crate::rate_limit::RateLimitTracker::load(db.clone())
                .await
                .expect("load rate tracker"),
        );
        let github = std::sync::Arc::new(
            crate::github::GithubClient::new(None, rate).expect("github client"),
        );
        let analyzer = AnalyzerCtx {
            github,
            cache: crate::cache::Cache::new(db),
        };
        Some(
            ApiState::with_settings(
                analyzer,
                None,
                std::sync::Arc::new(crate::repo_history::RepoStorage::from_env()),
                None,
                "http://localhost:14321".to_string(),
                None,
            )
            .expect("api state"),
        )
    }

    async fn cleanup_overlay_rows(state: &ApiState, prefix: &str) {
        let like = format!("{prefix}%");
        for statement in [
            "DELETE FROM star_fetch_queue WHERE repo LIKE $1",
            "DELETE FROM repo_stargazers WHERE repo LIKE $1",
            "DELETE FROM repos WHERE repo LIKE $1",
        ] {
            sqlx::query(statement)
                .bind(&like)
                .execute(&state.analyzer.cache.db().pool)
                .await
                .expect("cleanup");
        }
    }

    async fn cleanup_card_rows(state: &ApiState, prefix: &str) {
        let like = format!("{prefix}%");
        for statement in [
            "DELETE FROM repo_analysis_queue WHERE repo LIKE $1",
            "DELETE FROM repo_author_commit_days WHERE repo LIKE $1",
            "DELETE FROM repo_author_stats WHERE repo LIKE $1",
            "DELETE FROM repo_lines WHERE repo LIKE $1",
            "DELETE FROM repo_history WHERE repo LIKE $1",
            "DELETE FROM repos WHERE repo LIKE $1",
        ] {
            sqlx::query(statement)
                .bind(&like)
                .execute(&state.analyzer.cache.db().pool)
                .await
                .expect("cleanup");
        }
    }

    /// `cleanup_card_rows` plus the daily-commit rows the profile report
    /// reads but the card does not.
    async fn cleanup_profile_scope_rows(state: &ApiState, prefix: &str) {
        sqlx::query("DELETE FROM repo_commit_days WHERE repo LIKE $1")
            .bind(format!("{prefix}%"))
            .execute(&state.analyzer.cache.db().pool)
            .await
            .expect("cleanup");
        cleanup_card_rows(state, prefix).await;
    }

    /// Seed one owned repository: a publicly-proven `repos` row plus an
    /// optional analysis state.
    async fn seed_card_repo(
        state: &ApiState,
        repo: &str,
        stars: i64,
        forks: i64,
        analysis: Option<(&str, &str, i32)>,
    ) {
        let db = state.analyzer.cache.db();
        sqlx::query(
            "INSERT INTO repos (repo, star_count, forks_count, metadata_fetched_at, missing) \
             VALUES ($1, $2, $3, NOW(), FALSE) \
             ON CONFLICT (repo) DO UPDATE SET star_count = EXCLUDED.star_count, \
                 forks_count = EXCLUDED.forks_count, \
                 metadata_fetched_at = EXCLUDED.metadata_fetched_at, missing = FALSE",
        )
        .bind(repo)
        .bind(stars)
        .bind(forks)
        .execute(&db.pool)
        .await
        .expect("seed repos row");
        if let Some((analyzed_sha, head_sha, revision)) = analysis {
            sqlx::query(
                "INSERT INTO repo_history \
                    (repo, last_analyzed_sha, head_sha, last_analyzed_at, analysis_revision) \
                 VALUES ($1, $2, $3, NOW(), $4) \
                 ON CONFLICT (repo) DO UPDATE SET \
                     last_analyzed_sha = EXCLUDED.last_analyzed_sha, \
                     head_sha = EXCLUDED.head_sha, \
                     last_analyzed_at = EXCLUDED.last_analyzed_at, \
                     analysis_revision = EXCLUDED.analysis_revision",
            )
            .bind(repo)
            .bind(analyzed_sha)
            .bind(head_sha)
            .bind(revision)
            .execute(&db.pool)
            .await
            .expect("seed repo_history row");
        }
    }

    async fn seed_card_author(
        state: &ApiState,
        repo: &str,
        email: &str,
        login: Option<&str>,
        commits: i64,
        first_commit_at: chrono::DateTime<chrono::Utc>,
    ) {
        sqlx::query(
            "INSERT INTO repo_author_stats \
                (repo, author_email, github_login, avatar_url, commits, \
                 first_commit_at, last_commit_at, enrich_attempted_at) \
             VALUES ($1, $2, $3, \
                     CASE WHEN $3::TEXT IS NULL \
                          THEN 'https://www.gravatar.com/avatar/deadbeef' ELSE NULL END, \
                     $4, $5, $5, NULL)",
        )
        .bind(repo)
        .bind(email)
        .bind(login)
        .bind(commits)
        .bind(first_commit_at)
        .execute(&state.analyzer.cache.db().pool)
        .await
        .expect("seed repo_author_stats row");
    }

    /// Executes the exact profile-card statements against Postgres.
    ///
    /// Both statements join a second relation that also carries `repo`,
    /// `commits` and `first_commit_at` columns, so an unqualified aggregate
    /// is not a wrong number — it is `column reference "repo" is ambiguous`
    /// at plan time, i.e. HTTP 500 on `/api/users/{login}/card.svg` for
    /// every login. A previous fix to this function shipped without a test
    /// that ran the SQL, which is exactly how that reached production. This
    /// test covers the owned-repos query, the authored-commits query and
    /// both language scopes.
    ///
    /// It also pins the readiness contract: a repository whose analysis is
    /// current still counts as analyzed while its author rows are
    /// unenriched. Author identity is presentation-only metadata swept by
    /// `repo_analysis::sweep_author_enrichment`; gating this count on it is
    /// what pinned profiles at "Analyzing N repositories" indefinitely.
    #[tokio::test]
    async fn user_card_sql_aggregates_without_ambiguous_columns() {
        let Some(state) = test_db_state().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        use chrono::TimeZone;
        let db = state.analyzer.cache.db().clone();
        let login = "gitdebt-test-card-owner";
        let prefix = format!("{login}/");
        cleanup_card_rows(&state, &prefix).await;
        // A foreign repository this login only contributed to.
        let foreign_prefix = "gitdebt-test-card-foreign/";
        cleanup_card_rows(&state, foreign_prefix).await;

        let analyzed = format!("{prefix}analyzed");
        let unenriched = format!("{prefix}unenriched");
        let mid_analysis = format!("{prefix}mid-analysis");
        let queued = format!("{prefix}queued");
        let foreign = format!("{foreign_prefix}library");
        let revision = crate::repo_analysis::CURRENT_ANALYSIS_REVISION;

        seed_card_repo(&state, &analyzed, 10, 2, Some(("a1", "a1", revision))).await;
        seed_card_repo(&state, &unenriched, 20, 3, Some(("b1", "b1", revision))).await;
        // Analysis stopped short of the head it observed → still working.
        seed_card_repo(&state, &mid_analysis, 5, 0, Some(("c1", "c2", revision))).await;
        // Analysis is current but a live queue row says work is in flight.
        seed_card_repo(&state, &queued, 1, 0, Some(("d1", "d1", revision))).await;
        seed_card_repo(&state, &foreign, 900, 90, Some(("f1", "f1", revision))).await;
        sqlx::query(
            "INSERT INTO repo_analysis_queue (repo, status, enqueued_at) \
             VALUES ($1, 'pending', NOW())",
        )
        .bind(&queued)
        .execute(&db.pool)
        .await
        .expect("seed queue row");

        let first = chrono::Utc.timestamp_opt(1_600_000_000, 0).unwrap(); // 2020
        let later = chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap(); // 2023
        // Mixed case on purpose: the authored query folds with LOWER().
        seed_card_author(
            &state,
            &analyzed,
            "me@example.com",
            Some("GitDebt-Test-Card-Owner"),
            7,
            later,
        )
        .await;
        seed_card_author(
            &state,
            &foreign,
            "me@work.example",
            Some("gitdebt-test-card-owner"),
            5,
            first,
        )
        .await;
        // Someone else's commits in one of the owned repos.
        seed_card_author(
            &state,
            &analyzed,
            "other@example.com",
            Some("someone-else"),
            4,
            first,
        )
        .await;
        // Unresolved author: no login, gravatar avatar, never attempted.
        seed_card_author(&state, &unenriched, "ghost@example.com", None, 3, first).await;

        for (repo, language, lines) in [
            (&analyzed, "Rust", 900_i64),
            (&analyzed, "TypeScript", 400),
            (&unenriched, "Rust", 100),
            (&foreign, "Go", 5_000),
        ] {
            sqlx::query(
                "INSERT INTO repo_lines (repo, language, files, lines_code) \
                 VALUES ($1, $2, 1, $3)",
            )
            .bind(repo)
            .bind(language)
            .bind(lines)
            .execute(&db.pool)
            .await
            .expect("seed repo_lines row");
        }

        let data = load_user_card_data(&db, login)
            .await
            .expect("profile-card SQL must execute, not 500");

        assert_eq!(data.login, login);
        assert_eq!(data.repos_tracked, 4, "four publicly-proven owned repos");
        assert_eq!(
            data.repos_analyzed, 2,
            "analyzed + unenriched count; the mid-analysis and queued repos do not"
        );
        assert_eq!(
            data.stars, 36,
            "owned repos only — the foreign repo is excluded"
        );
        assert_eq!(data.forks, 5);
        assert_eq!(data.commits, 12, "7 owned + 5 foreign, case-folded login");
        assert_eq!(
            data.contribs, 2,
            "COUNT(DISTINCT author.repo), not repos.repo"
        );
        assert_eq!(data.since_year, Some(2020), "MIN over the author rows");
        assert_eq!(
            data.langs,
            vec![("Rust".to_string(), 1_000), ("TypeScript".to_string(), 400)],
            "owner scope sums the prefix and excludes the foreign repo"
        );

        let repo_langs = load_repo_top_langs(&db, &analyzed)
            .await
            .expect("repo-scope language SQL must execute");
        assert_eq!(
            repo_langs,
            vec![("Rust".to_string(), 900), ("TypeScript".to_string(), 400)]
        );

        // A tombstoned owned repo drops out of every column.
        sqlx::query("UPDATE repos SET missing = TRUE WHERE repo = $1")
            .bind(&analyzed)
            .execute(&db.pool)
            .await
            .expect("tombstone");
        let after = load_user_card_data(&db, login).await.expect("card SQL");
        assert_eq!(after.repos_tracked, 3);
        assert_eq!(after.repos_analyzed, 1);
        assert_eq!(after.commits, 5, "the tombstoned repo's commits are hidden");
        assert_eq!(after.contribs, 1);

        cleanup_card_rows(&state, &prefix).await;
        cleanup_card_rows(&state, foreign_prefix).await;
    }

    /// Organizations are first-class profile subjects, and the big ones own
    /// thousands of repositories. Every code signal must fan out over the
    /// bounded [`PROFILE_MAX_REPOS`] scope — most-starred first — while the
    /// account-wide totals stay uncapped, so the report states its coverage
    /// (`repos_scanned` vs `repos_tracked`) instead of quietly presenting a
    /// slice of an organization as all of it.
    #[tokio::test]
    async fn profile_signals_stay_bounded_for_a_massive_organization() {
        let Some(state) = test_db_state().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let db = state.analyzer.cache.db().clone();
        let login = "gitdebt-test-bigorg";
        let prefix = format!("{login}/");
        cleanup_profile_scope_rows(&state, &prefix).await;

        // 25 repositories past the cap. Star counts descend with `n`, so
        // the last 25 are exactly the ones the cap must drop, and they
        // carry marker rows no in-scope repository has.
        let over_cap = PROFILE_MAX_REPOS + 25;
        let seeds: [(&str, i64); 4] = [
            (
                "INSERT INTO repos (repo, star_count, forks_count, metadata_fetched_at, missing) \
                 SELECT $1 || n, $2 - n, 1, NOW(), FALSE FROM generate_series(1, $3) n",
                0,
            ),
            (
                "INSERT INTO repo_history \
                     (repo, last_analyzed_sha, head_sha, last_analyzed_at, \
                      total_commits, analysis_revision) \
                 SELECT $1 || n, 'sha', 'sha', NOW(), 3, $2 FROM generate_series(1, $3) n",
                crate::repo_analysis::CURRENT_ANALYSIS_REVISION as i64,
            ),
            (
                "INSERT INTO repo_lines (repo, language, files, lines_code) \
                 SELECT $1 || n, \
                        CASE WHEN n <= $2 THEN 'ScopeLang' ELSE 'OverCapLang' END, 1, 10 \
                 FROM generate_series(1, $3) n",
                PROFILE_MAX_REPOS,
            ),
            (
                "INSERT INTO repo_commit_days (repo, day, commits) \
                 SELECT $1 || n, \
                        CURRENT_DATE - CASE WHEN n <= $2 THEN 1 ELSE 2 END, \
                        CASE WHEN n <= $2 THEN 5 ELSE 7 END \
                 FROM generate_series(1, $3) n",
                PROFILE_MAX_REPOS,
            ),
        ];
        for (statement, second) in seeds {
            sqlx::query(statement)
                .bind(&prefix)
                .bind(second)
                .bind(over_cap)
                .execute(&db.pool)
                .await
                .expect("seed organization rows");
        }
        sqlx::query(
            "INSERT INTO repo_author_stats \
                 (repo, author_email, author_name, github_login, commits, \
                  first_commit_at, last_commit_at) \
             SELECT $1 || n, 'solo@example.com', 'Solo', 'solo', 9, NOW(), NOW() \
             FROM generate_series(1, $2) n",
        )
        .bind(&prefix)
        .bind(over_cap)
        .execute(&db.pool)
        .await
        .expect("seed authors");

        let scope = load_profile_scope(&db.pool, login)
            .await
            .expect("scope resolves");
        assert_eq!(scope.len() as i64, PROFILE_MAX_REPOS, "the scope is capped");
        assert!(
            scope.contains(&format!("{prefix}1")),
            "the most-starred repository is in scope"
        );
        assert!(
            !scope.contains(&format!("{prefix}{over_cap}")),
            "the least-starred repository past the cap is dropped"
        );

        let stats = load_user_stats(&db, login).await.expect("profile SQL runs");
        assert_eq!(
            stats.repos_tracked, over_cap,
            "the account-wide count stays uncapped"
        );
        assert_eq!(
            stats.repos_scanned, PROFILE_MAX_REPOS,
            "the code signals report the slice they actually cover"
        );
        assert!(
            stats.repos_scanned < stats.repos_tracked,
            "this fixture must exercise the capped case"
        );

        // Every fanned-out signal is confined to the scope.
        let languages: Vec<&str> = stats
            .languages
            .iter()
            .map(|row| row.language.as_str())
            .collect();
        assert_eq!(languages, vec!["ScopeLang"]);
        let heatmap_days: Vec<i64> = stats.commit_days.iter().map(|day| day.value).collect();
        assert_eq!(
            heatmap_days,
            vec![5 * PROFILE_MAX_REPOS],
            "only in-scope repositories contribute commit days"
        );
        assert_eq!(
            stats.commit_streak.longest_days, 0,
            "repository-wide activity cannot impersonate an individual streak"
        );
        assert_eq!(stats.commit_streak.latest_active_date, None);
        assert_eq!(stats.active_repos.len(), 8);
        assert!(
            stats
                .active_repos
                .iter()
                .all(|row| scope.contains(&row.repo)),
            "the activity ranking never reaches past the scope"
        );
        assert_eq!(stats.top_repos.len(), 8);
        assert_eq!(stats.top_repos[0].repo, format!("{prefix}1"));
        assert_eq!(
            stats.solo_maintained, PROFILE_MAX_REPOS,
            "the bus-factor pass scores exactly the scope"
        );

        // An account under the cap reports full coverage.
        let small = "gitdebt-test-smallorg";
        let small_prefix = format!("{small}/");
        cleanup_profile_scope_rows(&state, &small_prefix).await;
        seed_card_repo(&state, &format!("{small_prefix}only"), 4, 0, None).await;
        let small_stats = load_user_stats(&db, small).await.expect("profile SQL runs");
        assert_eq!(small_stats.repos_tracked, 1);
        assert_eq!(small_stats.repos_scanned, 1);

        cleanup_profile_scope_rows(&state, &prefix).await;
        cleanup_profile_scope_rows(&state, &small_prefix).await;
    }

    /// Cache policy for the multi-repo overlay: a mixed warm+cold request
    /// (one complete repo, one with no confirmed history) must ride the
    /// short-TTL pending policy and stay OUT of the 24h svg/raster memos —
    /// otherwise the first view pins an overlay that silently omits the
    /// cold repo's line at the edge for hours. Once every repo is
    /// complete, the same request flips to the complete policy; an
    /// all-cold request stays pending as before.
    #[tokio::test]
    async fn multi_overlay_mixed_warm_cold_is_pending_until_all_complete() {
        let Some(state) = test_db_state().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        use chrono::TimeZone;
        let prefix = format!("gitdebt-test-overlay-{}", std::process::id());
        // Sweep the whole family, not just this process' prefix: a run killed
        // mid-test leaves queue rows behind, and a stale row's expired lease
        // is claimable by every other suite sharing the database.
        cleanup_overlay_rows(&state, "gitdebt-test-overlay-").await;
        let warm = format!("{prefix}/warm");
        let cold = format!("{prefix}/cold");
        let events: Vec<crate::cache::StargazerEvent> = (0..5)
            .map(|i| {
                (
                    i + 1,
                    chrono::Utc
                        .timestamp_opt(1_600_000_000 + i * 86_400, 0)
                        .unwrap(),
                )
            })
            .collect();
        // Warm repo: complete stargazers via the cache's atomic writer.
        state
            .analyzer
            .cache
            .put_repo_stargazers(&warm, &events)
            .await
            .expect("seed warm repo");

        let theme = &crate::theme::DARK;
        let q = ChartQuery {
            repos: Some(format!("{warm},{cold}")),
            ..ChartQuery::default()
        };

        // Mixed warm+cold: pending, and neither memo layer keeps it.
        let card = ensure_multi_svg(&state, theme, &q)
            .await
            .expect("mixed overlay renders");
        assert!(
            card.short_ttl,
            "warm+cold overlay must be pending/short-TTL, not the 4h policy"
        );
        state.svg_cache.run_pending_tasks().await;
        assert_eq!(
            state.svg_cache.entry_count(),
            0,
            "pending overlay must not enter the 24h svg cache"
        );
        let (bytes, short_ttl) =
            ensure_multi_raster(&state, theme, &q, crate::raster::RasterFormat::Png)
                .await
                .expect("mixed overlay rasterizes");
        assert!(short_ttl, "raster path shares the pending policy");
        assert!(!bytes.is_empty());
        state.raster_cache.run_pending_tasks().await;
        assert_eq!(
            state.raster_cache.entry_count(),
            0,
            "pending overlay raster must not be memoized"
        );

        // All-cold stays pending (unchanged behavior).
        let q_cold = ChartQuery {
            repos: Some(format!("{prefix}/cold-a,{prefix}/cold-b")),
            ..ChartQuery::default()
        };
        let card = ensure_multi_svg(&state, theme, &q_cold)
            .await
            .expect("all-cold overlay renders");
        assert!(card.short_ttl, "all-cold overlay stays pending");

        // Completing the cold repo flips the SAME request to the complete
        // policy and memoizes it.
        state
            .analyzer
            .cache
            .put_repo_stargazers(&cold, &events)
            .await
            .expect("complete cold repo");
        let card = ensure_multi_svg(&state, theme, &q)
            .await
            .expect("all-warm overlay renders");
        assert!(
            !card.short_ttl,
            "overlay with every repo complete rides the full cache policy"
        );
        state.svg_cache.run_pending_tasks().await;
        assert_eq!(
            state.svg_cache.entry_count(),
            1,
            "complete overlay memoizes in the svg cache"
        );

        cleanup_overlay_rows(&state, &prefix).await;
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
        // Absent → the looping wave default; unknown values stay a 400.
        assert_eq!(ChartQuery::default().gif_motion().unwrap(), "wave");
        let wave = ChartQuery {
            motion: Some("WAVE".into()),
            ..ChartQuery::default()
        };
        assert_eq!(wave.gif_motion().unwrap(), "wave");
        let unknown = ChartQuery {
            motion: Some("sparkle".into()),
            ..ChartQuery::default()
        };
        assert!(unknown.gif_motion().is_err());
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
            theme_for(ChartQuery::default().theme.as_deref()).dark,
            "GIF/SVG default theme is dark"
        );
    }

    #[test]
    fn leaderboard_params_defaults() {
        let (metric, window, per, page) = leaderboard_params(None, None, None, None).unwrap();
        assert_eq!(metric, LeaderboardMetric::Stars);
        assert_eq!(window, LEADERBOARD_WINDOW_DEFAULT);
        assert_eq!(per, LEADERBOARD_PER_DEFAULT);
        assert_eq!(page, 0);
    }

    #[test]
    fn leaderboard_params_accepts_both_metrics() {
        let (metric, _, _, _) = leaderboard_params(Some("stars"), Some(1), None, None).unwrap();
        assert_eq!(metric, LeaderboardMetric::Stars);
        let (metric, window, _, _) =
            leaderboard_params(Some("velocity"), Some(30), None, None).unwrap();
        assert_eq!(metric, LeaderboardMetric::Velocity);
        assert_eq!(window, 30);
    }

    #[test]
    fn leaderboard_params_rejects_unknown_metric() {
        // Fail loudly, never silently fall back to a different ranking.
        assert!(leaderboard_params(Some("downloads"), None, None, None).is_err());
        assert!(leaderboard_params(Some("STARS"), None, None, None).is_err());
        assert!(leaderboard_params(Some(""), None, None, None).is_err());
        assert!(leaderboard_params(None, Some(2), None, None).is_err());
    }

    #[test]
    fn leaderboard_params_clamps_bounds() {
        // per: 1..=LEADERBOARD_PER_MAX; page: 0..=LEADERBOARD_PAGE_MAX.
        let (_, _, per, page) = leaderboard_params(None, None, Some(0), Some(-5)).unwrap();
        assert_eq!(per, 1);
        assert_eq!(page, 0);
        let (_, _, per, page) =
            leaderboard_params(None, None, Some(10_000), Some(i64::MAX)).unwrap();
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
    fn earned_badges_require_current_and_distributed_evidence() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 22).unwrap();
        let badges = evaluate_repo_badges(
            &RepoBadgeEvidence {
                latest_commit: Some(today - chrono::Duration::days(2)),
                commits_30d: 18,
                contributor_commits: vec![40, 35, 25, 10, 5, 5],
                stars_30d: Some(125),
                total_stars: 4_000,
                analysis_complete: true,
                stars_complete: true,
            },
            today,
        );
        assert!(badges.iter().all(|badge| badge.earned));
        assert_eq!(badges[1].detail, "bus factor 2 / 6 contributors");
    }

    #[test]
    fn earned_badges_do_not_turn_unknown_data_into_an_award() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 22).unwrap();
        let badges = evaluate_repo_badges(&RepoBadgeEvidence::default(), today);
        assert!(badges.iter().all(|badge| !badge.earned));
        assert!(badges.iter().all(|badge| badge.pending));
        assert_eq!(badges[0].detail, "analysis pending");
        assert_eq!(badges[2].detail, "collecting star data");
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

    fn request_with(peer: Option<&str>, headers: &[(&str, &str)]) -> axum::http::Request<()> {
        let mut request = axum::http::Request::new(());
        for (name, value) in headers {
            request.headers_mut().insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        if let Some(peer) = peer {
            let addr: SocketAddr = peer.parse().unwrap();
            request.extensions_mut().insert(ConnectInfo(addr));
        }
        request
    }

    /// The admission key policy: forwarding headers are honored ONLY from a
    /// trusted proxy peer; a direct client cannot spoof a fresh bucket.
    #[test]
    fn key_extraction_gates_forwarding_headers_on_trusted_peer() {
        if std::env::var("TRUSTED_PROXIES").is_ok() {
            return; // policy depends on the process-wide default set
        }
        // Trusted (loopback) peer: cf-connecting-ip wins.
        let req = request_with(
            Some("127.0.0.1:9999"),
            &[("cf-connecting-ip", "203.0.113.7")],
        );
        assert_eq!(
            CloudflareIpKeyExtractor.extract(&req),
            Some("203.0.113.7".parse().unwrap())
        );
        // Trusted peer without cf-connecting-ip: XFF chain applies.
        let req = request_with(
            Some("10.0.0.2:9999"),
            &[("x-forwarded-for", "198.51.100.4, 10.0.0.2")],
        );
        assert_eq!(
            CloudflareIpKeyExtractor.extract(&req),
            Some("198.51.100.4".parse().unwrap())
        );
        // Trusted peer, no headers at all: the peer itself keys the bucket.
        let req = request_with(Some("192.168.1.9:1234"), &[]);
        assert_eq!(
            CloudflareIpKeyExtractor.extract(&req),
            Some("192.168.1.9".parse().unwrap())
        );
        // UNTRUSTED public peer: every forwarding header is ignored.
        let req = request_with(
            Some("203.0.113.50:443"),
            &[
                ("cf-connecting-ip", "1.2.3.4"),
                ("x-forwarded-for", "5.6.7.8"),
            ],
        );
        assert_eq!(
            CloudflareIpKeyExtractor.extract(&req),
            Some("203.0.113.50".parse().unwrap())
        );
    }

    #[test]
    fn smart_header_ip_prefers_xff_then_real_ip_then_forwarded() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("garbage, 198.51.100.1"),
        );
        headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.2"));
        headers.insert(
            header::FORWARDED,
            HeaderValue::from_static("for=198.51.100.3"),
        );
        // First parseable XFF entry wins even after junk.
        assert_eq!(
            smart_header_ip(&headers),
            Some("198.51.100.1".parse().unwrap())
        );
        headers.remove("x-forwarded-for");
        assert_eq!(
            smart_header_ip(&headers),
            Some("198.51.100.2".parse().unwrap())
        );
        headers.remove("x-real-ip");
        assert_eq!(
            smart_header_ip(&headers),
            Some("198.51.100.3".parse().unwrap())
        );
        headers.remove(header::FORWARDED);
        assert_eq!(smart_header_ip(&headers), None);
    }

    #[test]
    fn forwarded_header_parses_rfc7239_identifiers() {
        assert_eq!(
            forwarded_header_ip("for=192.0.2.60;proto=http;by=203.0.113.43"),
            Some("192.0.2.60".parse().unwrap())
        );
        assert_eq!(
            forwarded_header_ip("for=\"192.0.2.60:4711\""),
            Some("192.0.2.60".parse().unwrap())
        );
        assert_eq!(
            forwarded_header_ip("For=\"[2001:db8:cafe::17]:4711\""),
            Some("2001:db8:cafe::17".parse().unwrap())
        );
        assert_eq!(
            forwarded_header_ip("for=\"[2001:db8:cafe::17]\""),
            Some("2001:db8:cafe::17".parse().unwrap())
        );
        // Obfuscated and unknown identifiers are skipped, later ones used.
        assert_eq!(
            forwarded_header_ip("for=_hidden, for=192.0.2.61"),
            Some("192.0.2.61".parse().unwrap())
        );
        assert_eq!(forwarded_header_ip("for=unknown"), None);
        assert_eq!(forwarded_header_ip("proto=https"), None);
    }

    #[test]
    fn too_many_requests_preserves_governor_envelope() {
        let response = too_many_requests(7);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers().get(header::RETRY_AFTER).unwrap(),
            &HeaderValue::from_static("7")
        );
        assert_eq!(
            response.headers().get("x-ratelimit-after").unwrap(),
            &HeaderValue::from_static("7")
        );
    }

    #[test]
    fn expected_tombstones_do_not_degrade_readiness() {
        let mut pipeline = PipelineSignals {
            worker_online: true,
            worker_last_seen: None,
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

        pipeline.star_jobs_dead = 0;
        pipeline.worker_online = false;
        pipeline.analysis_jobs_active = 1;
        assert!(pipeline.degraded());
    }

    // Profile-level stats

    #[test]
    fn profile_stat_filenames_dispatch_known_charts_only() {
        assert!(matches!(
            parse_user_stat_filename("commit-activity.svg"),
            Some((UserStatKind::CommitActivity, UserStatFormat::Svg))
        ));
        assert!(matches!(
            parse_user_stat_filename("commit-trend.png"),
            Some((
                UserStatKind::CommitTrend,
                UserStatFormat::Raster(crate::raster::RasterFormat::Png)
            ))
        ));
        assert!(matches!(
            parse_user_stat_filename("languages.webp"),
            Some((
                UserStatKind::Languages,
                UserStatFormat::Raster(crate::raster::RasterFormat::Webp)
            ))
        ));
        assert!(matches!(
            parse_user_stat_filename("contributions.svg"),
            Some((UserStatKind::Contributions, UserStatFormat::Svg))
        ));
        assert!(matches!(
            parse_user_stat_filename("languages.gif"),
            Some((UserStatKind::Languages, UserStatFormat::Gif))
        ));
        // Unknown chart names and formats are 400s, never a silent default.
        assert!(parse_user_stat_filename("bus-factor.svg").is_none());
        assert!(parse_user_stat_filename("languages").is_none());
    }

    /// The embed attribution footer is on by default and only dropped for
    /// an explicit in-app render — a README asset must always carry it.
    #[test]
    fn profile_stat_in_app_context_is_explicit() {
        let embed = UserStatQuery {
            theme: None,
            animate: None,
            context: None,
        };
        assert!(!embed.in_app());
        assert!(
            !UserStatQuery {
                context: Some("readme".into()),
                ..embed
            }
            .in_app()
        );
        assert!(
            UserStatQuery {
                theme: None,
                animate: None,
                context: Some("app".into()),
            }
            .in_app()
        );
    }

    /// Profile charts share the media cache policy: a not-yet-analyzed
    /// profile rides the short TTL so the embed self-heals, a rendered
    /// one rides the 4h edge policy, and both revalidate on a strong ETag.
    #[tokio::test]
    async fn profile_stat_media_policy_and_revalidation() {
        let none = HeaderMap::new();
        let pending = user_stat_svg_response(&none, "<svg>pending</svg>".into(), true, true);
        assert_eq!(
            pending.headers().get(header::CACHE_CONTROL).unwrap(),
            &card_cache_control(true)
        );

        let ready = user_stat_svg_response(&none, "<svg>ready</svg>".into(), false, true);
        assert_eq!(
            ready.headers().get(header::CACHE_CONTROL).unwrap(),
            &card_cache_control(false)
        );
        let etag = ready.headers().get(header::ETAG).unwrap().clone();

        let mut revalidate = HeaderMap::new();
        revalidate.insert(header::IF_NONE_MATCH, etag.clone());
        let not_modified =
            user_stat_svg_response(&revalidate, "<svg>ready</svg>".into(), false, true);
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(not_modified.headers().get(header::ETAG).unwrap(), &etag);
        let body = axum::body::to_bytes(not_modified.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body.is_empty(), "304 must carry no body");

        // The in-app variant must be a distinct representation, so its
        // ETag cannot collide with the branded README bytes.
        let in_app = user_stat_svg_response(&none, "<svg>ready</svg>".into(), false, false);
        assert_ne!(in_app.headers().get(header::ETAG).unwrap(), &etag);
    }

    async fn cleanup_profile_rows(state: &ApiState, prefix: &str) {
        let like = format!("{prefix}%");
        for statement in [
            "DELETE FROM repo_commit_days WHERE repo LIKE $1",
            "DELETE FROM repo_author_stats WHERE repo LIKE $1",
            "DELETE FROM repo_lines WHERE repo LIKE $1",
            "DELETE FROM repo_history WHERE repo LIKE $1",
            "DELETE FROM repos WHERE repo LIKE $1",
        ] {
            sqlx::query(statement)
                .bind(&like)
                .execute(&state.analyzer.cache.db().pool)
                .await
                .expect("cleanup");
        }
    }

    /// The profile report is Postgres-only and every code-derived number
    /// is gated on a completed analysis pass. This exercises the whole
    /// aggregate: owned totals, authored commits, the language footprint,
    /// the star and recent-activity rankings, and the maintenance signal
    /// (bus factor per owned repo, bots excluded).
    #[tokio::test]
    async fn profile_stats_aggregate_owned_repos_from_postgres() {
        let Some(state) = test_db_state().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let login = format!("gitdebtprofile{}", std::process::id());
        let pool = &state.analyzer.cache.db().pool;
        cleanup_profile_rows(&state, &format!("{login}/")).await;

        let solo = format!("{login}/solo");
        let shared = format!("{login}/shared");
        for (repo, stars, forks) in [(&solo, 120i64, 7i64), (&shared, 40, 2)] {
            sqlx::query(
                "INSERT INTO repos (repo, star_count, forks_count, metadata_fetched_at) \
                 VALUES ($1, $2, $3, NOW())",
            )
            .bind(repo)
            .bind(stars)
            .bind(forks)
            .execute(pool)
            .await
            .expect("seed repo");
        }

        // Nothing analyzed yet: the charts must report "pending" so their
        // embeds ride the short TTL and never pin an empty frame.
        assert!(
            user_stat_revision(pool, &login).await.unwrap().is_none(),
            "revision must be absent until an analysis pass completes"
        );
        let cold = load_user_stats(state.analyzer.cache.db(), &login)
            .await
            .expect("cold stats");
        assert!(!cold.ready);
        assert_eq!(cold.repos_tracked, 2);
        assert_eq!(cold.total_stars, 160);

        // Star history only for `solo`, through the atomic writer that flips
        // completeness — `shared` stays incomplete, so it must come back
        // with no sparkline at all rather than a partial one.
        use chrono::TimeZone;
        let events: Vec<crate::cache::StargazerEvent> = (0..6)
            .map(|i| {
                (
                    i + 1,
                    chrono::Utc
                        .timestamp_opt(1_600_000_000 + i * 40 * 86_400, 0)
                        .unwrap(),
                )
            })
            .collect();
        state
            .analyzer
            .cache
            .put_repo_stargazers(&solo, &events)
            .await
            .expect("seed star history");
        // The writer denormalizes star_count from the events it just wrote;
        // restore the metadata headline the rest of the assertions use.
        sqlx::query(
            "UPDATE repos SET star_count = 120, forks_count = 7, metadata_fetched_at = NOW() \
             WHERE repo = $1",
        )
        .bind(&solo)
        .execute(pool)
        .await
        .expect("restore metadata");

        for (repo, commits) in [(&solo, 300i64), (&shared, 100)] {
            sqlx::query(
                "INSERT INTO repo_history (repo, last_analyzed_at, total_commits) \
                 VALUES ($1, NOW(), $2)",
            )
            .bind(repo)
            .bind(commits)
            .execute(pool)
            .await
            .expect("seed history");
        }
        // solo: one human carries >50%. shared: two humans split it evenly,
        // so half of the authorship needs both. The bot row must not tip
        // either verdict.
        for (repo, email, name, gh_login, commits) in [
            (&solo, "solo@example.com", "Solo", Some(&login), 280i64),
            (&solo, "helper@example.com", "Helper", None, 20),
            (&shared, "a@example.com", "A", Some(&login), 50),
            (&shared, "b@example.com", "B", None, 50),
            (&shared, "bot@example.com", "renovate[bot]", None, 900),
        ] {
            sqlx::query(
                "INSERT INTO repo_author_stats \
                     (repo, author_email, author_name, github_login, commits, first_commit_at) \
                 VALUES ($1, $2, $3, $4, $5, TIMESTAMPTZ '2019-04-02T00:00:00Z')",
            )
            .bind(repo)
            .bind(email)
            .bind(name)
            .bind(gh_login)
            .bind(commits)
            .execute(pool)
            .await
            .expect("seed author");
        }
        sqlx::query(
            "INSERT INTO repo_lines (repo, language, files, lines_code, lines_blank, lines_comment) \
             VALUES ($1, 'Rust', 10, 5000, 200, 300), ($2, 'Rust', 4, 1000, 50, 60), \
                    ($2, 'TypeScript', 6, 2000, 100, 120)",
        )
        .bind(&solo)
        .bind(&shared)
        .execute(pool)
        .await
        .expect("seed lines");
        sqlx::query(
            "INSERT INTO repo_commit_days (repo, day, commits) \
             VALUES ($1, CURRENT_DATE - 3, 4), ($1, CURRENT_DATE - 2, 6), \
                    ($2, CURRENT_DATE - 1, 3)",
        )
        .bind(&shared)
        .bind(&solo)
        .execute(pool)
        .await
        .expect("seed commit days");
        sqlx::query(
            "INSERT INTO repo_author_commit_days (repo, author_email, day, commits) \
             VALUES ($1, 'a@example.com', CURRENT_DATE - 3, 4), \
                    ($1, 'a@example.com', CURRENT_DATE - 2, 6), \
                    ($2, 'solo@example.com', CURRENT_DATE - 1, 3)",
        )
        .bind(&shared)
        .bind(&solo)
        .execute(pool)
        .await
        .expect("seed author commit days");

        let stats = load_user_stats(state.analyzer.cache.db(), &login)
            .await
            .expect("stats");
        assert!(stats.ready);
        assert_eq!(stats.repos_tracked, 2);
        assert_eq!(stats.repos_analyzed, 2);
        assert_eq!(stats.total_stars, 160);
        assert_eq!(stats.total_forks, 9);
        assert_eq!(stats.analyzed_commits, 400);
        // Only the rows carrying this login count as authored work.
        assert_eq!(stats.authored_commits, 330);
        assert_eq!(stats.contributed_repos, 2);
        assert_eq!(stats.owned_contributed_repos, 2);
        assert_eq!(stats.external_contributed_repos, 0);
        assert_eq!(stats.owned_authored_commits, 330);
        assert_eq!(stats.external_authored_commits, 0);
        assert!(stats.visionary_repos.is_empty());
        assert_eq!(stats.since_year, Some(2019));
        assert_eq!(stats.solo_maintained, 1);
        assert_eq!(stats.shared_maintained, 1);

        // Language footprint sums across owned repos, biggest first.
        assert_eq!(stats.languages.len(), 2);
        assert_eq!(stats.languages[0].language, "Rust");
        assert_eq!(stats.languages[0].code, 6000);
        assert_eq!(stats.languages[1].language, "TypeScript");

        // Star ranking and recent-activity ranking are independent orders.
        assert_eq!(stats.top_repos[0].repo, solo);
        assert_eq!(stats.top_repos[0].stars, 120);
        assert_eq!(stats.top_repos[0].commits, 300);
        // Sparklines are cumulative and gated on confirmed-complete history.
        assert_eq!(stats.top_repos[0].spark.last().copied(), Some(6));
        assert!(
            stats.top_repos[0]
                .spark
                .windows(2)
                .all(|pair| pair[1] >= pair[0]),
            "a cumulative star series never decreases"
        );
        assert!(
            stats.top_repos[1].spark.is_empty(),
            "incomplete history must not draw a sparkline"
        );
        assert_eq!(stats.active_repos[0].repo, shared);
        assert_eq!(stats.active_repos[0].commits_recent, 10);

        let total_days: i64 = stats.commit_days.iter().map(|d| d.value).sum();
        assert_eq!(total_days, 13);
        assert_eq!(stats.commit_streak.current_days, 3);
        assert_eq!(stats.commit_streak.longest_days, 3);
        assert_eq!(stats.commit_streak.tiers.len(), 5);
        assert!(stats.commit_streak.tiers.iter().all(|tier| !tier.earned));

        // The JSON contract carries both public earned-state data and the
        // stable locked ladder used only when `/api/me` identifies the owner.
        let shape = serde_json::to_value(&stats).expect("serialize profile stats");
        assert_eq!(shape["commit_streak"]["current_days"], 3);
        assert_eq!(shape["commit_streak"]["longest_days"], 3);
        assert_eq!(shape["commit_streak"]["tiers"][0]["days"], 7);
        assert_eq!(shape["commit_streak"]["tiers"][4]["key"], "year-in-motion");
        assert_eq!(shape["commit_streak"]["tiers"][4]["earned"], false);

        // A completed pass yields a revision, and the render revision moves
        // with the data so a stale chart can never outlive it.
        let revision = user_stat_revision(pool, &login)
            .await
            .unwrap()
            .expect("revision after analysis");
        assert!(revision.contains("n2"));
        assert!(revision.contains("c400"));
        sqlx::query("UPDATE repo_history SET total_commits = 301 WHERE repo = $1")
            .bind(&solo)
            .execute(pool)
            .await
            .expect("bump commits");
        let next = user_stat_revision(pool, &login)
            .await
            .unwrap()
            .expect("revision after bump");
        assert_ne!(revision, next);

        // Every profile chart renders deterministically from that state.
        for kind in [
            UserStatKind::CommitActivity,
            UserStatKind::CommitTrend,
            UserStatKind::Languages,
            UserStatKind::Contributions,
        ] {
            let first = render_user_stat_svg(&state, &login, kind, &crate::theme::DARK)
                .await
                .expect("render");
            let second = render_user_stat_svg(&state, &login, kind, &crate::theme::DARK)
                .await
                .expect("render");
            assert_eq!(first, second, "{} must be deterministic", kind.key());
            assert!(first.starts_with("<svg"));
        }

        cleanup_profile_rows(&state, &format!("{login}/")).await;
    }

    #[tokio::test]
    async fn visionary_requires_complete_history_strict_five_x_growth_and_512_stars() {
        let Some(state) = test_db_state().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        use chrono::TimeZone;

        let login = format!("visionary{}", std::process::id());
        let breakout = format!("outside{}/breakout", std::process::id());
        let early = format!("outside{}/early", std::process::id());
        let repos = vec![breakout.clone(), early.clone()];
        let pool = &state.analyzer.cache.db().pool;

        for repo in &repos {
            for statement in [
                "DELETE FROM repo_author_stats WHERE repo = $1",
                "DELETE FROM repo_stargazers WHERE repo = $1",
                "DELETE FROM repo_star_arrivals WHERE repo = $1",
                "DELETE FROM repos WHERE repo = $1",
            ] {
                sqlx::query(statement)
                    .bind(repo)
                    .execute(pool)
                    .await
                    .expect("clean visionary fixture");
            }
        }

        for (repo, current, stars_at_first) in [
            (&breakout, 1001_i64, 200_usize),
            (&early, 512_i64, 20_usize),
        ] {
            let events: Vec<crate::cache::StargazerEvent> = (0..current)
                .map(|index| {
                    (
                        index + 1,
                        Utc.timestamp_opt(1_500_000_000 + index * 86_400, 0)
                            .unwrap(),
                    )
                })
                .collect();
            state
                .analyzer
                .cache
                .put_repo_stargazers(repo, &events)
                .await
                .expect("seed complete history");
            sqlx::query(
                "UPDATE repos SET star_count = $2, metadata_fetched_at = NOW() WHERE repo = $1",
            )
            .bind(repo)
            .bind(current)
            .execute(pool)
            .await
            .expect("seed current stars");
            sqlx::query(
                "INSERT INTO repo_author_stats \
                     (repo, author_email, github_login, commits, first_commit_at) \
                 VALUES ($1, $2, $3, 7, $4)",
            )
            .bind(repo)
            .bind(format!("{login}@example.com"))
            .bind(&login)
            .bind(events[stars_at_first - 1].1)
            .execute(pool)
            .await
            .expect("seed attributed contribution");
        }

        let stats = load_user_stats(state.analyzer.cache.db(), &login)
            .await
            .expect("load visionary profile");
        assert_eq!(stats.owned_contributed_repos, 0);
        assert_eq!(stats.external_contributed_repos, 2);
        assert_eq!(stats.external_authored_commits, 14);
        assert_eq!(stats.visionary_repos.len(), 2);
        assert_eq!(stats.visionary_repos[0].current_stars, 1001);
        assert_eq!(stats.visionary_repos[0].stars_at_first_contribution, 200);
        assert_eq!(stats.visionary_repos[1].current_stars, 512);
        assert_eq!(stats.visionary_repos[1].stars_at_first_contribution, 20);

        sqlx::query(
            "UPDATE repos SET star_count = CASE repo WHEN $1 THEN 1000 ELSE 511 END \
             WHERE repo = ANY($2::text[])",
        )
        .bind(&breakout)
        .bind(&repos)
        .execute(pool)
        .await
        .expect("move below award thresholds");
        assert!(
            load_visionary_repos(pool, &login)
                .await
                .expect("reload thresholds")
                .is_empty()
        );

        for repo in &repos {
            for statement in [
                "DELETE FROM repo_author_stats WHERE repo = $1",
                "DELETE FROM repo_stargazers WHERE repo = $1",
                "DELETE FROM repo_star_arrivals WHERE repo = $1",
                "DELETE FROM repos WHERE repo = $1",
            ] {
                sqlx::query(statement)
                    .bind(repo)
                    .execute(pool)
                    .await
                    .expect("remove visionary fixture");
            }
        }
    }
}
