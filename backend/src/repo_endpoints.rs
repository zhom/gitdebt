//! HTTP endpoints for the repo-history feature.
//!
//! Every chart endpoint takes optional `?theme=light|dark` (default light)
//! and `?animate=0|1` (default static) query params. The theme is resolved at request time and
//! the resulting SVG bakes concrete hex colors directly — no CSS
//! variables, no `prefers-color-scheme` (see `theme.rs` for the why).
//! For theme-aware README embedding, point a `<picture>` element at the
//! `light` and `dark` URLs separately.
//!
//! Format: each stat is reachable as `.svg`, `.png`, or `.webp` via a
//! single dispatcher route. PNG / WebP are rasterized from the SVG via
//! `raster::rasterize` and cached separately (`stat_svg_cache` holds
//! the source SVG; `raster_cache` holds the encoded bytes).

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use base64::Engine as _;
use chrono::{Datelike, NaiveDate, Utc};
use futures::{StreamExt, stream};
use serde::Deserialize;
use sqlx::Row;

use crate::api::{ApiError, ApiState};
use crate::brand;
use crate::raster::RasterFormat;
use crate::repo_analysis;
use crate::repo_charts::{
    self, AuthorShare, ContributorRow, DayCount, FileRow, LanguageBar, TodoPoint,
};
use crate::theme::theme_for;

/// Scale factor applied to raster output. 2.0 = retina density at the
/// SVG's CSS size — sharp on high-DPI screens, still reasonable file
/// size after lossless PNG / WebP encoding.
const RASTER_SCALE: f32 = 2.0;
const MAX_AVATAR_BYTES: usize = 128 * 1024;
const AVATAR_FETCH_CONCURRENCY: usize = 8;

fn trusted_avatar_url(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && matches!(
            url.host_str(),
            Some(
                "avatars.githubusercontent.com"
                    | "gravatar.com"
                    | "www.gravatar.com"
                    | "secure.gravatar.com"
            )
        )
}

async fn fetch_avatar_data_uri(client: &reqwest::Client, raw: &str) -> anyhow::Result<String> {
    if !trusted_avatar_url(raw) {
        anyhow::bail!("untrusted avatar URL");
    }
    let mut response = client.get(raw).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_AVATAR_BYTES as u64)
    {
        anyhow::bail!("avatar exceeds size limit");
    }
    let mime = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .filter(|value| matches!(*value, "image/png" | "image/jpeg" | "image/webp"))
        .ok_or_else(|| anyhow::anyhow!("unsupported avatar content type"))?
        .to_string();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > MAX_AVATAR_BYTES {
            anyhow::bail!("avatar exceeds size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        anyhow::bail!("empty avatar response");
    }
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

async fn self_contained_avatar(state: &ApiState, raw: String) -> Option<String> {
    if raw.starts_with("data:image/") {
        return Some(raw);
    }
    if !trusted_avatar_url(&raw) {
        return None;
    }
    let client = state.avatar_http.clone();
    let fetch_url = raw.clone();
    state
        .avatar_data_cache
        .try_get_with(raw, async move {
            fetch_avatar_data_uri(&client, &fetch_url).await
        })
        .await
        .ok()
}

async fn self_contained_avatars(
    state: &ApiState,
    avatars: impl IntoIterator<Item = Option<String>>,
) -> Vec<Option<String>> {
    stream::iter(avatars)
        .map(|avatar| async move {
            match avatar {
                Some(raw) => self_contained_avatar(state, raw).await,
                None => None,
            }
        })
        .buffered(AVATAR_FETCH_CONCURRENCY)
        .collect()
        .await
}

/// Postgres regex matching dependency manifests + lockfiles across the
/// major language ecosystems. Excluded from "bug-magnet" / "top-changed"
/// rankings because dep bumps dominate the churn count and crowd out
/// real signal — `Cargo.toml`/`package.json`/`pom.xml` would otherwise
/// top every active project.
///
/// `(^|/)` anchors at the basename so subdirectory files match too
/// (e.g. `crates/foo/Cargo.toml`). Add to this list as new ecosystems
/// show up; the cost of false-positives here is just one fewer file in
/// the chart, which is the right error direction.
const DEPENDENCY_FILE_REGEX: &str = concat!(
    "(^|/)(",
    "package\\.json|package-lock\\.json|pnpm-lock\\.yaml|yarn\\.lock",
    "|npm-shrinkwrap\\.json|bun\\.lockb|bower\\.json",
    "|Cargo\\.toml|Cargo\\.lock",
    "|pyproject\\.toml|setup\\.cfg|requirements\\.txt|requirements-[^/]+\\.txt",
    "|Pipfile|Pipfile\\.lock|poetry\\.lock|uv\\.lock",
    "|Gemfile|Gemfile\\.lock|[^/]+\\.gemspec",
    "|go\\.mod|go\\.sum",
    "|composer\\.json|composer\\.lock",
    "|mix\\.exs|mix\\.lock",
    "|Package\\.swift|Package\\.resolved",
    "|pom\\.xml|build\\.gradle|build\\.gradle\\.kts",
    "|settings\\.gradle|settings\\.gradle\\.kts|gradle\\.properties|build\\.sbt",
    "|pubspec\\.yaml|pubspec\\.lock",
    "|stack\\.yaml|stack\\.yaml\\.lock|package\\.yaml|[^/]+\\.cabal",
    "|dune-project|[^/]+\\.opam",
    "|shard\\.yml|shard\\.lock|rebar\\.config|Project\\.toml|Manifest\\.toml",
    "|build\\.zig\\.zon",
    "|vcpkg\\.json|conanfile\\.txt|conanfile\\.py",
    "|flake\\.nix|flake\\.lock",
    "|environment\\.yml|cpanfile",
    ")$",
);

/// Known automation accounts that don't render with `[bot]` in their
/// commit author. The `[bot]` LIKE filter catches the GitHub-style bot
/// accounts (`dependabot[bot]`, `github-actions[bot]`, etc.); this list
/// is for everything else that shows up under a plain login.
const BOT_LOGINS: &[&str] = &[
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

/// Shared SQL fragment excluding bot authors from `repo_author_stats`
/// queries (contributors grid, bus factor). The `[bot]` LIKE patterns
/// catch GitHub-style bot accounts; `$2` must be bound to `BOT_LOGINS`
/// for the plain-login automation accounts.
const NON_BOT_AUTHOR_FILTER: &str = "author_name NOT LIKE '%[bot]%' \
       AND author_email NOT LIKE '%[bot]@%' \
       AND (github_login IS NULL OR github_login NOT LIKE '%[bot]%') \
       AND COALESCE(github_login, '') <> ALL($2::text[])";

/// Read-only stat endpoints. One route, dispatched on
/// `{name}.{svg|png|webp}` in the filename segment. Public CORS in api.rs.
pub fn public_router() -> Router<ApiState> {
    Router::new().route(
        "/api/repos/{owner}/{repo}/stats/{filename}",
        get(stat_dispatcher),
    )
}

/// Mutating endpoints. api.rs wraps this in a per-IP rate limiter.
pub fn mutating_router() -> Router<ApiState> {
    Router::new().route(
        "/api/repos/{owner}/{repo}/analyze-history",
        post(enqueue_analysis),
    )
}

async fn enqueue_analysis(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(query): Query<AnalyzeHistoryQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    // Lowercase on the same key as the star-fetch queue / cache / worker so
    // the repo-analysis queue can't fork `Owner/Repo` from `owner/repo`.
    let full = crate::analyzer::repo_key(&owner, &repo);
    let user_id = state
        .gh_app
        .as_ref()
        .and_then(|config| crate::auth::current_user_id(config, &jar));
    let priority = if user_id.is_some() {
        repo_analysis::INTERACTIVE_PRIORITY
    } else {
        state
            .analyzer
            .cache
            .get_repo_view_count(&full)
            .await
            .unwrap_or(0)
    };
    let summary = state.analyzer.cache.get_repo_summary(&full).await?;
    if summary.as_ref().is_some_and(|repo| repo.missing) {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"queued": false, "repo": full, "reason": "not_found"})),
        )
            .into_response());
    }
    let verified = summary
        .as_ref()
        .is_some_and(|repo| repo.metadata_fetched_at.is_some() || repo.stargazers_complete);
    if !verified {
        let github = if let (Some(user_id), Some(config)) = (user_id, state.gh_app.as_ref()) {
            match crate::auth::user_access_token(state.analyzer.cache.db(), config, user_id).await {
                Ok(Some(token)) => state
                    .analyzer
                    .github
                    .for_user_token(&token)
                    .map(Arc::new)
                    .unwrap_or_else(|_| state.analyzer.github.clone()),
                _ => state.analyzer.github.clone(),
            }
        } else {
            state.analyzer.github.clone()
        };
        match github.repo_metadata(&owner, &repo).await {
            Ok(Some(metadata)) => {
                state
                    .analyzer
                    .cache
                    .put_repo_metadata(
                        &full,
                        metadata.id,
                        metadata.stargazers_count,
                        metadata.forks_count,
                        metadata.created_at,
                    )
                    .await?;
            }
            Ok(None) => {
                state.analyzer.cache.mark_repo_missing(&full).await?;
                return Ok((
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"queued": false, "repo": full, "reason": "not_found"})),
                )
                    .into_response());
            }
            Err(error) => {
                tracing::warn!(repo = %full, error = %error, "repo verification failed");
                return Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"queued": false, "repo": full, "reason": "verification_unavailable"})),
                )
                    .into_response());
            }
        }
    }

    // A report visit is the platform activity signal. Record it only after
    // the repository has been verified as public, and keep the counter write
    // off the enqueue request's latency path. The landing-page pulse reads
    // these aggregate repository counters from Postgres; it never exposes a
    // viewer identity or calls GitHub.
    if request_is_frontend_view(&headers, &state.frontend_origin, query.view) {
        let cache_for_view = state.analyzer.cache.clone();
        let repo_for_view = full.clone();
        tokio::spawn(async move {
            if let Err(error) = cache_for_view.record_repo_view(&repo_for_view).await {
                tracing::debug!(repo = %repo_for_view, error = %error, "record repo report view failed");
            }
        });
    }

    // Both pipelines receive the same popularity bump. Star history continues
    // through the existing Postgres-backed GH Archive path; an OAuth token is
    // never pooled into unrelated repositories or persisted on the queue.
    crate::queue::enqueue(state.analyzer.cache.db(), &full, priority).await?;
    let outcome =
        repo_analysis::enqueue_prioritized(state.analyzer.cache.db(), &full, priority, user_id)
            .await?;
    let (status, queued, reason) = match outcome {
        repo_analysis::EnqueueOutcome::Enqueued => (StatusCode::ACCEPTED, true, "enqueued"),
        repo_analysis::EnqueueOutcome::AlreadyActive => {
            (StatusCode::ACCEPTED, true, "already_active")
        }
        repo_analysis::EnqueueOutcome::Fresh => (StatusCode::OK, false, "fresh"),
        repo_analysis::EnqueueOutcome::AtCapacity => {
            (StatusCode::SERVICE_UNAVAILABLE, false, "queue_full")
        }
    };
    Ok((
        status,
        Json(serde_json::json!({
            "queued": queued,
            "repo": full,
            "reason": reason,
            "priority": if user_id.is_some() { "interactive" } else { "standard" }
        })),
    )
        .into_response())
}

#[derive(Debug, Default, Deserialize)]
struct AnalyzeHistoryQuery {
    view: Option<u8>,
}

fn request_is_frontend_view(headers: &HeaderMap, frontend_origin: &str, view: Option<u8>) -> bool {
    view == Some(1)
        && headers
            .get(header::ORIGIN)
            .and_then(|origin| origin.to_str().ok())
            .is_some_and(|origin| origin == frontend_origin)
}

/// Mirror of the frontend slug validator. Rejects `..`, slashes, query
/// strings, and other shell/SQL/path-traversal vectors before any value
/// reaches the queue, the cloner, or the GitHub client. The shape
/// matches what GitHub itself accepts in URLs. Shared with `api.rs` for
/// the multi-repo overlay `?repos=` validation.
pub fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
        && s != "."
        && s != ".."
}

// Filename → (chart kind, output format) dispatch

#[derive(Copy, Clone, Debug)]
enum StatKind {
    BugMagnets,
    TopFiles,
    Heatmap,
    Contributors,
    TodoTrend,
    Lines,
    BusFactor,
    CommitTrend,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum OutputFormat {
    Svg,
    Png,
    Webp,
}

impl OutputFormat {
    fn cache_suffix(self) -> &'static str {
        match self {
            OutputFormat::Svg => "svg",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
        }
    }
    fn raster(self) -> Option<RasterFormat> {
        match self {
            OutputFormat::Svg => None,
            OutputFormat::Png => Some(RasterFormat::Png),
            OutputFormat::Webp => Some(RasterFormat::Webp),
        }
    }
}

fn parse_filename(s: &str) -> Option<(StatKind, OutputFormat)> {
    let (name, ext) = s.rsplit_once('.')?;
    let kind = match name {
        "bug-magnets" => StatKind::BugMagnets,
        "top-files" => StatKind::TopFiles,
        "heatmap" => StatKind::Heatmap,
        "contributors" => StatKind::Contributors,
        "todo-trend" => StatKind::TodoTrend,
        "lines" => StatKind::Lines,
        "bus-factor" => StatKind::BusFactor,
        "commit-trend" => StatKind::CommitTrend,
        _ => return None,
    };
    let format = match ext {
        "svg" => OutputFormat::Svg,
        "png" => OutputFormat::Png,
        "webp" => OutputFormat::Webp,
        _ => return None,
    };
    Some((kind, format))
}

/// Unified query struct for the dispatcher. Each chart-type uses only
/// the fields it cares about; the rest are ignored. Cache keys are
/// per-chart so unused query params can't cross-pollute.
#[derive(Deserialize, Default)]
struct StatQuery {
    theme: Option<String>,
    /// SMIL is on-site-only and explicit. README/default output is static.
    animate: Option<String>,
    /// `bug-magnets` / `contributors` / `todo-trend` / `lines` ignore this.
    since: Option<String>,
    /// `heatmap` only.
    year: Option<i32>,
}

impl StatQuery {
    fn animate(&self) -> bool {
        matches!(self.animate.as_deref(), Some("1") | Some("true"))
    }
}

async fn stat_dispatcher(
    State(state): State<ApiState>,
    Path((owner, repo, filename)): Path<(String, String, String)>,
    Query(q): Query<StatQuery>,
) -> Result<Response, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let Some((kind, format)) = parse_filename(&filename) else {
        return Err(ApiError::bad_request("unknown chart or format"));
    };
    let full = format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    );
    let theme = theme_for(q.theme.as_deref());
    let Some(revision) = stat_revision(&state, &full, kind).await? else {
        let svg = render_analysis_pending(&full, theme);
        let Some(raster_format) = format.raster() else {
            return Ok(svg_response_with_policy(svg, true).into_response());
        };
        let bytes = crate::api::rasterize_limited(svg, raster_format, RASTER_SCALE).await?;
        return Ok(
            raster_response_with_policy(raster_format, Arc::new(bytes), true).into_response(),
        );
    };
    let theme_key = format!(
        "{}|rev:{revision}",
        if theme.dark { "dark" } else { "light" }
    );

    // Build (or fetch from cache) the SVG. The cache key is chart-
    // specific and includes all query params that affect output.
    let (svg_key, animated_svg) = match kind {
        StatKind::BugMagnets => ensure_bug_magnets_svg(&state, &full, theme, &theme_key).await?,
        StatKind::TopFiles => {
            ensure_top_files_svg(&state, &full, theme, &theme_key, q.since.as_deref()).await?
        }
        StatKind::Heatmap => ensure_heatmap_svg(&state, &full, theme, &theme_key, q.year).await?,
        StatKind::Contributors => ensure_contributors_svg(&state, &full, theme, &theme_key).await?,
        StatKind::TodoTrend => ensure_todo_trend_svg(&state, &full, theme, &theme_key).await?,
        StatKind::Lines => ensure_lines_svg(&state, &full, theme, &theme_key).await?,
        StatKind::BusFactor => ensure_bus_factor_svg(&state, &full, theme, &theme_key).await?,
        StatKind::CommitTrend => ensure_commit_trend_svg(&state, &full, theme, &theme_key).await?,
    };
    let svg = crate::texture::decorate(stat_svg_motion(animated_svg, q.animate()), theme);

    let Some(raster_format) = format.raster() else {
        return Ok(svg_response(svg).into_response());
    };

    // Raster path. Key off the SVG's cache key + format suffix so
    // both PNG and WebP variants memoize independently.
    let raster_key = format!("{svg_key}|{}", format.cache_suffix());
    if let Some(cached) = state.raster_cache.get(&raster_key).await {
        return Ok(raster_response(raster_format, cached).into_response());
    }
    // Shared semaphore-capped raster path (see api.rs::rasterize_limited)
    // so stat-chart encodes count against the same process-wide CPU cap
    // as the chart/card/OG rasters.
    let bytes = crate::api::rasterize_limited(svg, raster_format, RASTER_SCALE).await?;
    let arc = Arc::new(bytes);
    state.raster_cache.insert(raster_key, arc.clone()).await;
    Ok(raster_response(raster_format, arc).into_response())
}

fn stat_svg_motion(svg: String, animate: bool) -> String {
    if animate {
        svg
    } else {
        // All static attributes already equal their finished values.
        // Removing SMIL is therefore a complete, deterministic README frame.
        crate::raster::freeze_svg_animations(&svg)
    }
}

async fn stat_revision(
    state: &ApiState,
    repo: &str,
    kind: StatKind,
) -> Result<Option<String>, ApiError> {
    let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT \
            (SELECT last_analyzed_sha FROM repo_history WHERE repo = $1), \
            (SELECT status FROM repo_analysis_queue WHERE repo = $1), \
            (SELECT phase FROM repo_analysis_queue WHERE repo = $1)",
    )
    .bind(repo)
    .fetch_one(&state.analyzer.cache.db().pool)
    .await?;
    let active = matches!(row.1.as_deref(), Some("pending" | "in_progress"));
    let core_saved = row.2.as_deref() == Some("finishing") && kind.ready_while_finishing();
    Ok(if active && !core_saved { None } else { row.0 })
}

impl StatKind {
    /// These charts read tables atomically replaced by `apply_commits_at_head`
    /// before the worker enters its expensive language-count / contributor-
    /// enrichment phase. Contributor and bus-factor cards wait because their
    /// author identities are still being enriched; lines wait for the tree
    /// count itself.
    fn ready_while_finishing(self) -> bool {
        matches!(
            self,
            Self::BugMagnets | Self::TopFiles | Self::Heatmap | Self::TodoTrend | Self::CommitTrend
        )
    }
}

fn render_analysis_pending(repo: &str, theme: &crate::theme::Theme) -> String {
    let repo = repo
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 760 180" role="img" aria-label="Repository analysis pending for {repo}">
  <style><![CDATA[
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
  ]]></style>
  <rect width="760" height="180" rx="12" fill="{bg}"/>
  <text x="28" y="62" fill="{fg}" font-family="ui-sans-serif,system-ui,sans-serif" font-size="19" font-weight="600">{repo}</text>
  <text x="28" y="105" fill="{muted}" font-family="ui-sans-serif,system-ui,sans-serif" font-size="14">Repository analysis is still running</text>
  <text x="28" y="132" fill="{muted}" font-family="ui-sans-serif,system-ui,sans-serif" font-size="12">Refresh shortly for code-health data.</text>
{footer}
</svg>"#,
        bg = theme.track,
        fg = theme.fg,
        muted = theme.muted,
        footer = brand::footer_lockup(732.0, 164.0, theme),
    )
}

// Per-chart SVG builders (memoized in stat_svg_cache)

async fn ensure_bug_magnets_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
) -> Result<(String, String), ApiError> {
    let cache_key = format!("bug-magnets:{full}|{theme_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        let rows = sqlx::query(
            "SELECT path, fix_commits AS count FROM repo_file_stats \
             WHERE repo = $1 AND fix_commits > 0 \
               AND path !~ $2 \
             ORDER BY fix_commits DESC LIMIT 10",
        )
        .bind(full)
        .bind(DEPENDENCY_FILE_REGEX)
        .fetch_all(&state.analyzer.cache.db().pool)
        .await?;
        let rows: Vec<FileRow> = rows
            .into_iter()
            .map(|r| FileRow {
                path: r.try_get("path").unwrap_or_default(),
                count: r.try_get("count").unwrap_or(0),
            })
            .collect();
        Ok(repo_charts::render_bug_magnets(full, &rows, theme))
    })
    .await?;
    Ok((cache_key, svg))
}

async fn ensure_top_files_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
    since: Option<&str>,
) -> Result<(String, String), ApiError> {
    let since_key = since.unwrap_or("all");
    let cache_key = format!("top-files:{full}|{theme_key}|{since_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        let cutoff = since
            .and_then(parse_since)
            .map(|d| Utc::now() - chrono::Duration::days(d as i64));
        let rows = if let Some(cutoff) = cutoff {
            sqlx::query(
                "SELECT path, commits AS count FROM repo_file_stats \
                 WHERE repo = $1 AND last_modified_at >= $2 \
                   AND path !~ $3 \
                 ORDER BY commits DESC LIMIT 10",
            )
            .bind(full)
            .bind(cutoff)
            .bind(DEPENDENCY_FILE_REGEX)
            .fetch_all(&state.analyzer.cache.db().pool)
            .await?
        } else {
            sqlx::query(
                "SELECT path, commits AS count FROM repo_file_stats \
                 WHERE repo = $1 \
                   AND path !~ $2 \
                 ORDER BY commits DESC LIMIT 10",
            )
            .bind(full)
            .bind(DEPENDENCY_FILE_REGEX)
            .fetch_all(&state.analyzer.cache.db().pool)
            .await?
        };
        let rows: Vec<FileRow> = rows
            .into_iter()
            .map(|r| FileRow {
                path: r.try_get("path").unwrap_or_default(),
                count: r.try_get("count").unwrap_or(0),
            })
            .collect();
        Ok(repo_charts::render_top_changed(full, &rows, theme))
    })
    .await?;
    Ok((cache_key, svg))
}

async fn ensure_heatmap_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
    year: Option<i32>,
) -> Result<(String, String), ApiError> {
    let year_key = year
        .map(|y| y.to_string())
        .unwrap_or_else(|| "rolling".to_string());
    let cache_key = format!("heatmap:{full}|{theme_key}|{year_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        let (from, to, subtitle) = if let Some(year) = year {
            let from = NaiveDate::from_ymd_opt(year, 1, 1)
                .ok_or_else(|| ApiError::bad_request("bad year"))?;
            let to = NaiveDate::from_ymd_opt(year, 12, 31)
                .ok_or_else(|| ApiError::bad_request("bad year"))?;
            (from, to, format!("Commits in {year}"))
        } else {
            let today = Utc::now().date_naive();
            let this_monday =
                today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
            let from = this_monday - chrono::Duration::days(51 * 7);
            (from, today, "Commits in the last 52 weeks".to_string())
        };
        let rows = sqlx::query(
            "SELECT day, commits FROM repo_commit_days \
             WHERE repo = $1 AND day BETWEEN $2 AND $3 \
             ORDER BY day",
        )
        .bind(full)
        .bind(from)
        .bind(to)
        .fetch_all(&state.analyzer.cache.db().pool)
        .await?;
        let days: Vec<DayCount> = rows
            .into_iter()
            .map(|r| DayCount {
                day: r.try_get("day").unwrap_or(from),
                commits: r.try_get("commits").unwrap_or(0),
            })
            .collect();
        Ok(repo_charts::render_heatmap(
            full, &subtitle, from, to, &days, theme,
        ))
    })
    .await?;
    Ok((cache_key, svg))
}

async fn ensure_contributors_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
) -> Result<(String, String), ApiError> {
    let cache_key = format!("contributors:{full}|{theme_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        let sql = format!(
            "SELECT github_login, author_name, avatar_url, commits FROM repo_author_stats \
             WHERE repo = $1 AND {NON_BOT_AUTHOR_FILTER} \
             ORDER BY commits DESC LIMIT 200"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(full)
            .bind(BOT_LOGINS)
            .fetch_all(&state.analyzer.cache.db().pool)
            .await?;
        let mut rows: Vec<ContributorRow> = rows
            .into_iter()
            .map(|r| ContributorRow {
                login: r
                    .try_get::<Option<String>, _>("github_login")
                    .unwrap_or(None),
                name: r.try_get("author_name").unwrap_or_default(),
                avatar_url: r.try_get::<Option<String>, _>("avatar_url").unwrap_or(None),
                commits: r.try_get("commits").unwrap_or(0),
            })
            .collect();
        let avatar_urls: Vec<Option<String>> = rows
            .iter()
            .take(16)
            .map(|row| row.avatar_url.clone())
            .collect();
        let avatars = self_contained_avatars(state, avatar_urls).await;
        for (row, avatar) in rows.iter_mut().zip(avatars) {
            row.avatar_url = avatar;
        }
        Ok(repo_charts::render_contributors(full, &rows, theme))
    })
    .await?;
    Ok((cache_key, svg))
}

async fn ensure_todo_trend_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
) -> Result<(String, String), ApiError> {
    let cache_key = format!("todo-trend:{full}|{theme_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        let rows = sqlx::query(
            "SELECT day, \
                    SUM(todo_added - todo_removed) OVER (ORDER BY day) AS running_total \
             FROM repo_todo_deltas \
             WHERE repo = $1 \
             ORDER BY day",
        )
        .bind(full)
        .fetch_all(&state.analyzer.cache.db().pool)
        .await?;
        let pts: Vec<TodoPoint> = rows
            .into_iter()
            .map(|r| TodoPoint {
                day: r.try_get("day").unwrap_or_else(|_| Utc::now().date_naive()),
                running_total: r
                    .try_get::<Option<i64>, _>("running_total")
                    .ok()
                    .flatten()
                    .unwrap_or(0),
            })
            .collect();
        Ok(repo_charts::render_todo_trend(full, &pts, theme))
    })
    .await?;
    Ok((cache_key, svg))
}

async fn ensure_lines_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
) -> Result<(String, String), ApiError> {
    let cache_key = format!("lines:{full}|{theme_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        // Sort by total lines (code + comments + blanks), not lines_code, so
        // the chart's ordering matches the bar widths users see.
        let rows = sqlx::query(
            "SELECT language, files, lines_code, lines_blank, lines_comment \
             FROM repo_lines \
             WHERE repo = $1 AND (lines_code + lines_blank + lines_comment) > 0 \
             ORDER BY (lines_code + lines_blank + lines_comment) DESC LIMIT 12",
        )
        .bind(full)
        .fetch_all(&state.analyzer.cache.db().pool)
        .await?;
        let bars: Vec<LanguageBar> = rows
            .into_iter()
            .map(|r| LanguageBar {
                language: r.try_get("language").unwrap_or_default(),
                files: r.try_get("files").unwrap_or(0),
                lines_code: r.try_get("lines_code").unwrap_or(0),
                lines_blank: r.try_get("lines_blank").unwrap_or(0),
                lines_comment: r.try_get("lines_comment").unwrap_or(0),
            })
            .collect();
        Ok(repo_charts::render_languages(full, &bars, theme))
    })
    .await?;
    Ok((cache_key, svg))
}

async fn ensure_bus_factor_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
) -> Result<(String, String), ApiError> {
    let cache_key = format!("bus-factor:{full}|{theme_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        // The window SUM runs over ALL bot-filtered rows before ORDER/LIMIT,
        // so `total` is the true commit total even though only the top 500
        // author rows come back. Postgres returns NUMERIC for SUM(bigint) —
        // cast back so sqlx can decode an i64. The `author_email` tie-break
        // keeps the row order (and therefore the SVG bytes) deterministic.
        let sql = format!(
            "SELECT COALESCE(NULLIF(github_login, ''), NULLIF(author_name, ''), author_email) AS label, \
                    github_login, avatar_url, \
                    commits, \
                    SUM(commits) OVER ()::BIGINT AS total \
             FROM repo_author_stats \
             WHERE repo = $1 AND {NON_BOT_AUTHOR_FILTER} \
             ORDER BY commits DESC, author_email ASC LIMIT 500"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(full)
            .bind(BOT_LOGINS)
            .fetch_all(&state.analyzer.cache.db().pool)
            .await?;
        let total_commits: i64 = rows
            .first()
            .and_then(|r| r.try_get::<Option<i64>, _>("total").ok().flatten())
            .unwrap_or(0);
        let mut authors: Vec<AuthorShare> = rows
            .into_iter()
            .map(|r| AuthorShare {
                label: r.try_get("label").unwrap_or_default(),
                login: r.try_get::<Option<String>, _>("github_login").unwrap_or(None),
                avatar_url: r.try_get::<Option<String>, _>("avatar_url").unwrap_or(None),
                commits: r.try_get("commits").unwrap_or(0),
            })
            .collect();
        let avatar_urls: Vec<Option<String>> = authors
            .iter()
            .take(8)
            .map(|author| author.avatar_url.clone())
            .collect();
        let avatars = self_contained_avatars(state, avatar_urls).await;
        for (author, avatar) in authors.iter_mut().zip(avatars) {
            author.avatar_url = avatar;
        }
        Ok(repo_charts::render_bus_factor(full, &authors, total_commits, theme))
    })
    .await?;
    Ok((cache_key, svg))
}

async fn ensure_commit_trend_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
) -> Result<(String, String), ApiError> {
    let cache_key = format!("commit-trend:{full}|{theme_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        let rows =
            sqlx::query("SELECT day, commits FROM repo_commit_days WHERE repo = $1 ORDER BY day")
                .bind(full)
                .fetch_all(&state.analyzer.cache.db().pool)
                .await?;
        let days: Vec<DayCount> = rows
            .into_iter()
            .map(|r| DayCount {
                day: r.try_get("day").unwrap_or(NaiveDate::MIN),
                commits: r.try_get("commits").unwrap_or(0),
            })
            .collect();
        Ok(repo_charts::render_commit_trend(full, &days, theme))
    })
    .await?;
    Ok((cache_key, svg))
}

// Response helpers

fn parse_since(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('d') {
        num.parse().ok()
    } else if let Some(num) = s.strip_suffix("mo") {
        num.parse::<u32>().ok().map(|m| m * 30)
    } else if let Some(num) = s.strip_suffix('y') {
        num.parse::<u32>().ok().map(|y| y * 365)
    } else {
        s.parse::<u32>().ok()
    }
}

fn svg_response(svg: String) -> impl IntoResponse {
    svg_response_with_policy(svg, false)
}

fn svg_response_with_policy(svg: String, pending: bool) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, stat_cache_control(pending));
    (headers, brand::with_site_link(svg))
}

fn raster_response(format: RasterFormat, bytes: Arc<Vec<u8>>) -> impl IntoResponse {
    raster_response_with_policy(format, bytes, false)
}

fn raster_response_with_policy(
    format: RasterFormat,
    bytes: Arc<Vec<u8>>,
    pending: bool,
) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(header::CACHE_CONTROL, stat_cache_control(pending));
    (headers, (*bytes).clone())
}

fn stat_cache_control(pending: bool) -> HeaderValue {
    if pending {
        HeaderValue::from_static("no-store")
    } else {
        HeaderValue::from_static("public, s-maxage=86400, max-age=3600")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_media_only_reads_known_https_cdns() {
        assert!(trusted_avatar_url(
            "https://avatars.githubusercontent.com/u/1?s=80&v=4"
        ));
        assert!(trusted_avatar_url(
            "https://www.gravatar.com/avatar/abc?d=identicon&s=80"
        ));
        assert!(!trusted_avatar_url(
            "http://avatars.githubusercontent.com/u/1"
        ));
        assert!(!trusted_avatar_url(
            "https://avatars.githubusercontent.com.evil.example/u/1"
        ));
        assert!(!trusted_avatar_url("https://127.0.0.1/avatar.png"));
        assert!(!trusted_avatar_url(
            "https://avatars.githubusercontent.com:8443/u/1"
        ));
    }

    #[test]
    fn report_views_require_the_exact_frontend_origin() {
        let mut headers = HeaderMap::new();
        assert!(!request_is_frontend_view(
            &headers,
            "https://gitdebt.com",
            Some(1)
        ));

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://gitdebt.com"),
        );
        assert!(!request_is_frontend_view(
            &headers,
            "https://gitdebt.com",
            None
        ));
        assert!(!request_is_frontend_view(
            &headers,
            "https://gitdebt.com",
            Some(0)
        ));
        assert!(request_is_frontend_view(
            &headers,
            "https://gitdebt.com",
            Some(1)
        ));
        assert!(!request_is_frontend_view(
            &headers,
            "https://preview.gitdebt.com",
            Some(1)
        ));
    }

    #[test]
    fn parse_filename_dispatches_new_stat_kinds() {
        assert!(matches!(
            parse_filename("bus-factor.svg"),
            Some((StatKind::BusFactor, OutputFormat::Svg))
        ));
        assert!(matches!(
            parse_filename("bus-factor.png"),
            Some((StatKind::BusFactor, OutputFormat::Png))
        ));
        assert!(matches!(
            parse_filename("commit-trend.svg"),
            Some((StatKind::CommitTrend, OutputFormat::Svg))
        ));
        assert!(matches!(
            parse_filename("commit-trend.webp"),
            Some((StatKind::CommitTrend, OutputFormat::Webp))
        ));
    }

    #[test]
    fn parse_filename_rejects_unknown_names_and_formats() {
        assert!(parse_filename("bus-factor.gif").is_none());
        assert!(parse_filename("bus-factor").is_none());
        assert!(parse_filename("unknown.svg").is_none());
    }

    #[test]
    fn pending_stats_are_never_cached() {
        assert_eq!(
            stat_cache_control(true),
            HeaderValue::from_static("no-store")
        );
        let svg = render_analysis_pending("o/r", &crate::theme::LIGHT);
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.contains(">gitdebt</text>"));
    }

    #[test]
    fn only_atomically_saved_core_charts_open_during_finishing() {
        assert!(StatKind::BugMagnets.ready_while_finishing());
        assert!(StatKind::TopFiles.ready_while_finishing());
        assert!(StatKind::Heatmap.ready_while_finishing());
        assert!(StatKind::TodoTrend.ready_while_finishing());
        assert!(StatKind::CommitTrend.ready_while_finishing());
        assert!(!StatKind::Contributors.ready_while_finishing());
        assert!(!StatKind::BusFactor.ready_while_finishing());
        assert!(!StatKind::Lines.ready_while_finishing());
    }

    #[test]
    fn stat_animation_is_explicit_and_static_default_has_no_smil() {
        assert!(!StatQuery::default().animate());
        assert!(
            StatQuery {
                animate: Some("1".into()),
                ..StatQuery::default()
            }
            .animate()
        );

        let animated = repo_charts::render_bug_magnets(
            "foo/bar",
            &[FileRow {
                path: "src/lib.rs".into(),
                count: 4,
            }],
            crate::theme::theme_for(Some("dark")),
        );
        assert!(animated.contains("<animate"));
        let static_svg = stat_svg_motion(animated.clone(), false);
        assert!(!static_svg.contains("<animate"));
        assert!(static_svg.contains(crate::theme::DARK.fg));
        assert_eq!(stat_svg_motion(animated.clone(), true), animated);
    }
}
