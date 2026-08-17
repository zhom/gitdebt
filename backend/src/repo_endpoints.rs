//! HTTP endpoints for the repo-history feature.
//!
//! Every chart endpoint takes optional `?theme=light|dark` (default light)
//! and `?animate=0|1` (default static) query params. The theme is resolved at request time and
//! the resulting SVG bakes concrete hex colors directly — no CSS
//! variables, no `prefers-color-scheme` (see `theme.rs` for the why).
//! For theme-aware README embedding, point a `<picture>` element at the
//! `light` and `dark` URLs separately.
//!
//! Format: each stat is reachable as `.svg`, `.gif`, `.png`, or `.webp` via a
//! single dispatcher route. GIF / PNG / WebP are rasterized from the SVG via
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

/// Priority for anonymous work a real visitor (or a real embed impression)
/// asked for: popularity, ranked *inside* the visitor band.
///
/// Anonymous work used to be prioritized by `view_count` alone, which is `0`
/// for a repository nobody has opened yet — the same band the curated-catalog
/// bootstrap enqueues its whole list in. Since `enqueue_prioritized` never
/// refreshes `enqueued_at` on conflict, the catalog's boot-time timestamps won
/// every tie forever, so every first-time visitor queued behind the entire
/// backfill and saw none of the reserved capacity
/// [`repo_analysis::VISITOR_PRIORITY_FLOOR`] exists to give them.
///
/// The popularity bonus is clamped to the band: a repository with a million
/// views must not climb into the warm-up band and outrank work a signed-in
/// visitor is waiting on.
pub fn view_priority(view_count: i64) -> i64 {
    let span = repo_analysis::WARM_PRIORITY - repo_analysis::VISITOR_PRIORITY_FLOOR - 1;
    repo_analysis::VISITOR_PRIORITY_FLOOR.saturating_add(view_count.clamp(0, span))
}

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
    Router::new()
        .route(
            "/api/repos/{owner}/{repo}/stats/{filename}",
            get(stat_dispatcher),
        )
        .route("/api/repos/{owner}/{repo}/stats.json", get(repo_stats_json))
        .route(
            "/api/repos/{owner}/{repo}/health.json",
            get(repo_health_json),
        )
        // One contributor per request, addressed by rank. See
        // `contributor_avatar` for why a linked grid cannot be one image.
        .route(
            "/api/repos/{owner}/{repo}/contributors/{rank}/{filename}",
            get(contributor_avatar),
        )
        .route(
            "/api/repos/{owner}/{repo}/contributors/{rank}",
            get(contributor_profile_redirect),
        )
}

/// Postgres-only data contract for the interactive in-app charts. Embedded
/// assets continue through the deterministic SVG/raster renderers; the site
/// consumes these complete aggregate rows directly so hover, focus and scrub
/// interactions do not need to reverse-engineer pixels from an image.
async fn repo_stats_json(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let full = crate::analyzer::repo_key(&owner, &repo);
    let pool = &state.analyzer.cache.db().pool;
    let overview: Option<(i64, Option<i64>, bool, Option<String>)> = sqlx::query_as(
        "SELECT history.total_commits, history.analysis_scope_commits, \
                history.analysis_truncated, history.last_analyzed_sha \
         FROM repo_history history \
         JOIN repos public_repo ON public_repo.repo = history.repo \
         WHERE history.repo = $1 AND history.last_analyzed_at IS NOT NULL \
           AND public_repo.missing = FALSE \
           AND public_repo.metadata_fetched_at IS NOT NULL",
    )
    .bind(&full)
    .fetch_optional(pool)
    .await?;
    let Some((total_commits, scope_commits, truncated, revision)) = overview else {
        return Ok((
            StatusCode::ACCEPTED,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({"ready": false, "repo": full})),
        )
            .into_response());
    };

    let files = sqlx::query(
        "SELECT path, commits, fix_commits, lines_added, lines_deleted, \
                binary_changes, last_modified_at \
         FROM repo_file_stats \
         WHERE repo = $1 AND path !~ $2 \
         ORDER BY commits DESC, path ASC LIMIT 20",
    )
    .bind(&full)
    .bind(DEPENDENCY_FILE_REGEX)
    .fetch_all(pool)
    .await?;
    let file_rows: Vec<_> = files
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "path": row.try_get::<String, _>("path").unwrap_or_default(),
                "commits": row.try_get::<i64, _>("commits").unwrap_or(0),
                "fix_commits": row.try_get::<i64, _>("fix_commits").unwrap_or(0),
                "lines_added": row.try_get::<i64, _>("lines_added").unwrap_or(0),
                "lines_deleted": row.try_get::<i64, _>("lines_deleted").unwrap_or(0),
                "binary_changes": row.try_get::<i64, _>("binary_changes").unwrap_or(0),
                "last_modified_at": row.try_get::<chrono::DateTime<Utc>, _>("last_modified_at").ok(),
            })
        })
        .collect();

    let author_sql = format!(
        "SELECT COALESCE(NULLIF(github_login, ''), NULLIF(author_name, ''), author_email) AS label, \
                github_login, avatar_url, commits, SUM(commits) OVER ()::BIGINT AS analyzed_total \
         FROM repo_author_stats WHERE repo = $1 AND {NON_BOT_AUTHOR_FILTER} \
         ORDER BY commits DESC, author_email ASC LIMIT 32"
    );
    let author_rows = sqlx::query(sqlx::AssertSqlSafe(author_sql))
        .bind(&full)
        .bind(BOT_LOGINS)
        .fetch_all(pool)
        .await?;
    let analyzed_commits = author_rows
        .first()
        .and_then(|row| {
            row.try_get::<Option<i64>, _>("analyzed_total")
                .ok()
                .flatten()
        })
        .unwrap_or(0);
    let authors: Vec<_> = author_rows
        .into_iter()
        .map(|row| {
            let avatar = row
                .try_get::<Option<String>, _>("avatar_url")
                .unwrap_or(None)
                .filter(|url| trusted_avatar_url(url));
            serde_json::json!({
                "label": row.try_get::<String, _>("label").unwrap_or_default(),
                "login": row.try_get::<Option<String>, _>("github_login").unwrap_or(None),
                "avatar_url": avatar,
                "commits": row.try_get::<i64, _>("commits").unwrap_or(0),
            })
        })
        .collect();
    let author_counts: Vec<i64> = authors
        .iter()
        .filter_map(|author| author.get("commits").and_then(serde_json::Value::as_i64))
        .collect();
    let bus_factor = repo_charts::compute_bus_factor(&author_counts, analyzed_commits);

    let commit_days: Vec<(NaiveDate, i64, i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT day, commits, lines_added, lines_deleted, files_changed, \
                binary_files, large_changes \
         FROM repo_commit_days WHERE repo = $1 ORDER BY day",
    )
    .bind(&full)
    .fetch_all(pool)
    .await?;
    let todo_days: Vec<(NaiveDate, i64)> = sqlx::query_as(
        "SELECT day, SUM(todo_added - todo_removed) OVER (ORDER BY day)::BIGINT \
         FROM repo_todo_deltas WHERE repo = $1 ORDER BY day",
    )
    .bind(&full)
    .fetch_all(pool)
    .await?;

    let age_counts: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
            COUNT(*) FILTER (WHERE last_modified_at >= NOW() - INTERVAL '30 days')::BIGINT, \
            COALESCE(SUM(commits) FILTER (WHERE last_modified_at >= NOW() - INTERVAL '30 days'), 0)::BIGINT, \
            COUNT(*) FILTER (WHERE last_modified_at < NOW() - INTERVAL '30 days' \
                              AND last_modified_at >= NOW() - INTERVAL '1 year')::BIGINT, \
            COALESCE(SUM(commits) FILTER (WHERE last_modified_at < NOW() - INTERVAL '30 days' \
                                          AND last_modified_at >= NOW() - INTERVAL '1 year'), 0)::BIGINT, \
            COUNT(*) FILTER (WHERE last_modified_at < NOW() - INTERVAL '1 year' \
                              AND last_modified_at >= NOW() - INTERVAL '3 years')::BIGINT, \
            COALESCE(SUM(commits) FILTER (WHERE last_modified_at < NOW() - INTERVAL '1 year' \
                                          AND last_modified_at >= NOW() - INTERVAL '3 years'), 0)::BIGINT, \
            COUNT(*) FILTER (WHERE last_modified_at < NOW() - INTERVAL '3 years')::BIGINT, \
            COALESCE(SUM(commits) FILTER (WHERE last_modified_at < NOW() - INTERVAL '3 years'), 0)::BIGINT \
         FROM repo_file_stats WHERE repo = $1",
    )
    .bind(&full)
    .fetch_one(pool)
    .await?;
    let file_age_bands = if age_counts.0 + age_counts.2 + age_counts.4 + age_counts.6 > 0 {
        [
            ("this_month", age_counts.0, age_counts.1),
            ("within_year", age_counts.2, age_counts.3),
            ("two_to_three_years", age_counts.4, age_counts.5),
            ("older", age_counts.6, age_counts.7),
        ]
        .into_iter()
        .map(|(range, files, changes)| {
            serde_json::json!({
                "range": range,
                "files": files.max(0),
                "changes": changes.max(0),
            })
        })
        .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let coupling_rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT path_a, path_b, cochanges, fix_commits \
         FROM repo_file_couplings \
         WHERE repo = $1 AND path_a !~ $2 AND path_b !~ $2 \
         ORDER BY (cochanges + fix_commits) DESC, path_a, path_b LIMIT 40",
    )
    .bind(&full)
    .bind(DEPENDENCY_FILE_REGEX)
    .fetch_all(pool)
    .await?;
    let languages: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT language, files, lines_code, lines_blank, lines_comment \
         FROM repo_lines WHERE repo = $1 \
         AND ((lines_code + lines_blank + lines_comment) > 0 OR files > 0) \
         ORDER BY CASE WHEN (lines_code + lines_blank + lines_comment) > 0 \
                  THEN (lines_code + lines_blank + lines_comment) ELSE files END DESC LIMIT 12",
    )
    .bind(&full)
    .fetch_all(pool)
    .await?;

    let body = serde_json::json!({
        "ready": true,
        "repo": full,
        "revision": revision,
        "total_commits": total_commits.max(0),
        "attributed_commits": analyzed_commits.max(0),
        // Backward-compatible alias. This is the filtered denominator used
        // for ownership math, not the repository's displayed commit total.
        "analyzed_commits": analyzed_commits.max(0),
        "analysis_scope_commits": scope_commits.unwrap_or(0).max(0),
        "analysis_truncated": truncated,
        "bus_factor": bus_factor,
        "files": file_rows,
        "authors": authors,
        "commit_days": commit_days.into_iter().map(|(date, value, lines_added, lines_deleted, files_changed, binary_files, large_changes)| serde_json::json!({
            "date": date,
            "value": value.max(0),
            "lines_added": lines_added.max(0),
            "lines_deleted": lines_deleted.max(0),
            "files_changed": files_changed.max(0),
            "binary_files": binary_files.max(0),
            "large_changes": large_changes.max(0),
        })).collect::<Vec<_>>(),
        "todo_days": todo_days.into_iter().map(|(date, value)| serde_json::json!({"date": date, "value": value.max(0)})).collect::<Vec<_>>(),
        "file_age_bands": file_age_bands,
        "file_couplings": coupling_rows.into_iter().map(|(source, target, cochanges, fix_commits)| serde_json::json!({
            "source": source,
            "target": target,
            "cochanges": cochanges.max(0),
            "fix_commits": fix_commits.max(0),
        })).collect::<Vec<_>>(),
        "languages": languages.into_iter().map(|(language, files, code, blank, comment)| serde_json::json!({
            "language": language, "files": files, "code": code, "blank": blank, "comment": comment,
        })).collect::<Vec<_>>(),
    });
    Ok((
        [(header::CACHE_CONTROL, "public, s-maxage=300, max-age=60")],
        Json(body),
    )
        .into_response())
}

/// Trailing window behind every "recent" reading in the health summary. A
/// quarter is long enough to survive a quiet fortnight and short enough
/// that "still maintained" means something.
const HEALTH_WINDOW_DAYS: i32 = 90;

/// Months of commit history returned as the maintenance sparkline.
const HEALTH_MONTHS: usize = 24;

/// Deepest author rank read for the bus factor. A repository whose top 64
/// non-bot authors still do not hold half the attributed commits is
/// "broadly shared" by any reading, so the exact rank past that point buys
/// nothing and would cost a scan of every author row.
const HEALTH_AUTHOR_DEPTH: i64 = 64;

/// `(total_commits, analysis_truncated, star_count, archived, last_analyzed_at)`
type HealthOverviewRow = (i64, bool, Option<i64>, bool, Option<chrono::DateTime<Utc>>);

/// `(tracked_files, file_changes, fix_changes, fresh_files, hotspot_path,
/// hotspot_commits, hotspot_fix_commits)`
type HealthFileRow = (i64, i64, i64, i64, Option<String>, Option<i64>, Option<i64>);

/// Fixed-size health summary for a repository.
///
/// `stats.json` returns every commit day, TODO day and file row — hundreds
/// of kilobytes on an old repository, which is the right shape for the
/// report page and the wrong shape for a landing page that only needs one
/// legible verdict per signal. This aggregates in Postgres instead and
/// returns a body whose size does not grow with repository age.
async fn repo_health_json(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let full = crate::analyzer::repo_key(&owner, &repo);
    let pool = &state.analyzer.cache.db().pool;
    match load_repo_health(pool, &full).await? {
        Some(body) => Ok((
            [(header::CACHE_CONTROL, "public, s-maxage=300, max-age=60")],
            Json(body),
        )
            .into_response()),
        // Same contract as `stats.json`: an unanalyzed repository is a
        // not-yet, not a 404, and must never be cached as an answer.
        None => Ok((
            StatusCode::ACCEPTED,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({"ready": false, "repo": full})),
        )
            .into_response()),
    }
}

/// The health summary for one already-analyzed public repository, or
/// `None` when no completed analysis backs it yet.
pub(crate) async fn load_repo_health(
    pool: &sqlx::PgPool,
    repo: &str,
) -> Result<Option<serde_json::Value>, ApiError> {
    let overview: Option<HealthOverviewRow> = sqlx::query_as(
        "SELECT history.total_commits, history.analysis_truncated, \
                public_repo.star_count, public_repo.archived, history.last_analyzed_at \
         FROM repo_history history \
         JOIN repos public_repo ON public_repo.repo = history.repo \
         WHERE history.repo = $1 AND history.last_analyzed_at IS NOT NULL \
           AND public_repo.missing = FALSE \
           AND public_repo.metadata_fetched_at IS NOT NULL",
    )
    .bind(repo)
    .fetch_optional(pool)
    .await?;
    let Some((total_commits, analysis_truncated, star_count, archived, analyzed_at)) = overview
    else {
        return Ok(None);
    };

    // Ownership. The window functions run before LIMIT, so the totals cover
    // every non-bot author even though only the head of the ranking is read.
    let author_sql = format!(
        "SELECT commits, \
                SUM(commits) OVER ()::BIGINT AS attributed_total, \
                COUNT(*) OVER ()::BIGINT AS contributor_count \
         FROM repo_author_stats \
         WHERE repo = $1 AND commits > 0 AND {NON_BOT_AUTHOR_FILTER} \
         ORDER BY commits DESC, author_email ASC LIMIT {HEALTH_AUTHOR_DEPTH}"
    );
    let author_rows = sqlx::query(sqlx::AssertSqlSafe(author_sql))
        .bind(repo)
        .bind(BOT_LOGINS)
        .fetch_all(pool)
        .await?;
    let attributed_commits = author_rows
        .first()
        .and_then(|row| {
            row.try_get::<Option<i64>, _>("attributed_total")
                .ok()
                .flatten()
        })
        .unwrap_or(0);
    let contributors = author_rows
        .first()
        .and_then(|row| row.try_get::<i64, _>("contributor_count").ok())
        .unwrap_or(0);
    let author_commits: Vec<i64> = author_rows
        .iter()
        .filter_map(|row| row.try_get::<i64, _>("commits").ok())
        .collect();
    let bus_factor = repo_charts::compute_bus_factor(&author_commits, attributed_commits);
    let top_author_commits = author_commits.first().copied().unwrap_or(0);

    // Maintenance: this window against the one before it.
    let (commits_window, commits_previous_window, last_commit_day): (i64, i64, Option<NaiveDate>) =
        sqlx::query_as(
            "SELECT COALESCE(SUM(commits) FILTER \
                        (WHERE day > CURRENT_DATE - $2::INT), 0)::BIGINT, \
                    COALESCE(SUM(commits) FILTER \
                        (WHERE day > CURRENT_DATE - 2 * $2::INT \
                           AND day <= CURRENT_DATE - $2::INT), 0)::BIGINT, \
                    MAX(day) FILTER (WHERE commits > 0) \
             FROM repo_commit_days WHERE repo = $1",
        )
        .bind(repo)
        .bind(HEALTH_WINDOW_DAYS)
        .fetch_one(pool)
        .await?;

    let today = Utc::now().date_naive();
    let current_month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today);
    let first_month = current_month
        .checked_sub_months(chrono::Months::new(HEALTH_MONTHS as u32 - 1))
        .unwrap_or(current_month);
    let month_rows: Vec<(NaiveDate, i64)> = sqlx::query_as(
        "SELECT date_trunc('month', day)::date AS month, SUM(commits)::BIGINT AS commits \
         FROM repo_commit_days WHERE repo = $1 AND day >= $2 \
         GROUP BY 1 ORDER BY 1",
    )
    .bind(repo)
    .bind(first_month)
    .fetch_all(pool)
    .await?;
    // Gaps are filled to zero: a month nobody committed in is a fact about
    // the repository, and dropping it would draw a flatter line than reality.
    let observed: std::collections::HashMap<NaiveDate, i64> = month_rows.into_iter().collect();
    let mut commit_months = Vec::with_capacity(HEALTH_MONTHS);
    let mut cursor = first_month;
    for _ in 0..HEALTH_MONTHS {
        commit_months.push(serde_json::json!({
            "month": format!("{:04}-{:02}", cursor.year(), cursor.month()),
            "commits": observed.get(&cursor).copied().unwrap_or(0).max(0),
        }));
        let Some(next) = cursor.checked_add_months(chrono::Months::new(1)) else {
            break;
        };
        cursor = next;
    }

    // Repair load, hotspot and freshness share one scan of the file rows.
    // Dependency manifests are excluded for the same reason the charts
    // exclude them: version bumps would otherwise be every repo's hotspot.
    let (
        tracked_files,
        file_changes,
        fix_changes,
        fresh_files,
        hotspot_path,
        hotspot_commits,
        hotspot_fixes,
    ): HealthFileRow = sqlx::query_as(
        "WITH tracked AS MATERIALIZED ( \
             SELECT path, commits, fix_commits, last_modified_at \
             FROM repo_file_stats WHERE repo = $1 AND path !~ $2 \
         ), totals AS ( \
             SELECT COUNT(*)::BIGINT AS files, \
                    COALESCE(SUM(commits), 0)::BIGINT AS changes, \
                    COALESCE(SUM(fix_commits), 0)::BIGINT AS fixes, \
                    COUNT(*) FILTER \
                        (WHERE last_modified_at >= NOW() - INTERVAL '1 year')::BIGINT AS fresh \
             FROM tracked \
         ), hotspot AS ( \
             SELECT path, commits, fix_commits FROM tracked \
             ORDER BY commits DESC, path ASC LIMIT 1 \
         ) \
         SELECT totals.files, totals.changes, totals.fixes, totals.fresh, \
                hotspot.path, hotspot.commits, hotspot.fix_commits \
         FROM totals LEFT JOIN hotspot ON TRUE",
    )
    .bind(repo)
    .bind(DEPENDENCY_FILE_REGEX)
    .fetch_one(pool)
    .await?;

    let (todo_delta_window, todo_outstanding): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(todo_added - todo_removed) FILTER \
                    (WHERE day > CURRENT_DATE - $2::INT), 0)::BIGINT, \
                COALESCE(SUM(todo_added - todo_removed), 0)::BIGINT \
         FROM repo_todo_deltas WHERE repo = $1",
    )
    .bind(repo)
    .bind(HEALTH_WINDOW_DAYS)
    .fetch_one(pool)
    .await?;

    Ok(Some(serde_json::json!({
        "ready": true,
        "repo": repo,
        "stars": star_count.unwrap_or(0).max(0),
        "archived": archived,
        "analyzed_at": analyzed_at,
        "window_days": HEALTH_WINDOW_DAYS,
        "total_commits": total_commits.max(0),
        "attributed_commits": attributed_commits.max(0),
        "analysis_truncated": analysis_truncated,
        "bus_factor": bus_factor,
        "contributors": contributors.max(0),
        "top_author_commits": top_author_commits.max(0),
        "commits_window": commits_window.max(0),
        "commits_previous_window": commits_previous_window.max(0),
        "last_commit_day": last_commit_day,
        "commit_months": commit_months,
        "tracked_files": tracked_files.max(0),
        "file_changes": file_changes.max(0),
        "fix_changes": fix_changes.max(0),
        "fresh_files": fresh_files.max(0),
        "hotspot": hotspot_path.map(|path| serde_json::json!({
            "path": path,
            "commits": hotspot_commits.unwrap_or(0).max(0),
            "fix_commits": hotspot_fixes.unwrap_or(0).max(0),
        })),
        "todo_delta_window": todo_delta_window,
        "todo_outstanding": todo_outstanding.max(0),
    })))
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
        view_priority(
            state
                .analyzer
                .cache
                .get_repo_view_count(&full)
                .await
                .unwrap_or(0),
        )
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
        .is_some_and(|repo| !repo.missing && repo.metadata_fetched_at.is_some());
    // Do not make a GitHub request on this HTTP path. Both durable workers
    // verify the repository through the public-only metadata decoder before
    // cloning or ingesting anything, and tombstone 404/private/deleted
    // slugs. Keeping verification in the worker makes a burst of 100 cold
    // analysis requests bounded by fast Postgres queue writes instead of
    // holding 100 HTTP requests open on an upstream API.

    // A report visit is the platform activity signal. Record it only after
    // the repository has been verified as public, and keep the counter write
    // off the enqueue request's latency path. The landing-page pulse reads
    // these aggregate repository counters from Postgres; it never exposes a
    // viewer identity or calls GitHub.
    if verified && request_is_frontend_view(&headers, &state.frontend_origin, query.view) {
        let cache_for_view = state.analyzer.cache.clone();
        let repo_for_view = full.clone();
        tokio::spawn(async move {
            if let Err(error) = cache_for_view.record_repo_view(&repo_for_view).await {
                tracing::debug!(repo = %repo_for_view, error = %error, "record repo report view failed");
            }
        });
    }

    // Both pipelines receive the same popularity bump, now from the organic
    // band rather than from a bare `view_count` — a repository a person is
    // looking at right now outranks the background backfill sweep in the star
    // queue for the same reason it outranks the catalog in the analysis queue.
    // Star history continues through the existing Postgres-backed GH Archive
    // path; an OAuth token is never pooled into unrelated repositories or
    // persisted on the queue.
    crate::analyzer::enqueue_fetch_known(&state.analyzer, &full, priority).await;
    let outcome =
        repo_analysis::enqueue_prioritized(state.analyzer.cache.db(), &full, priority, user_id)
            .await?;
    let (status, queued, reason) = match outcome {
        repo_analysis::EnqueueOutcome::Enqueued => (StatusCode::ACCEPTED, true, "enqueued"),
        repo_analysis::EnqueueOutcome::AlreadyActive => {
            (StatusCode::ACCEPTED, true, "already_active")
        }
        repo_analysis::EnqueueOutcome::Fresh => (StatusCode::OK, false, "fresh"),
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
    Gif,
    Png,
    Webp,
}

impl OutputFormat {
    fn cache_suffix(self) -> &'static str {
        match self {
            OutputFormat::Svg => "svg",
            OutputFormat::Gif => "gif",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
        }
    }
    fn raster(self) -> Option<RasterFormat> {
        match self {
            OutputFormat::Svg | OutputFormat::Gif => None,
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
        "gif" => OutputFormat::Gif,
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
    /// In-app media omits embed-only attribution; README output keeps it.
    context: Option<String>,
}

impl StatQuery {
    fn animate(&self) -> bool {
        matches!(self.animate.as_deref(), Some("1") | Some("true"))
    }

    fn in_app(&self) -> bool {
        self.context.as_deref() == Some("app")
    }
}

async fn stat_dispatcher(
    State(state): State<ApiState>,
    Path((owner, repo, filename)): Path<(String, String, String)>,
    Query(q): Query<StatQuery>,
    request_headers: HeaderMap,
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
        // A repo-health embed for a repository nobody has opened on the site
        // is the only thing that will ever ask for its analysis. Offer the
        // durable job (bounded, deduplicated, and capacity-gated inside
        // `enqueue_prioritized`) and return immediately — without this the
        // frame told embedders that "analysis is still running" when nothing
        // was queued and nothing ever would be.
        //
        // An impression in somebody's README is organic demand, so it starts
        // at the band floor rather than at the catalog band. It gets no
        // popularity bonus: embeds are the highest-volume miss path here and
        // must not add a `view_count` read to a render that already missed
        // every cache.
        if let Err(error) = crate::repo_analysis::enqueue_prioritized(
            state.analyzer.cache.db(),
            &full,
            repo_analysis::VISITOR_PRIORITY_FLOOR,
            None,
        )
        .await
        {
            tracing::warn!(repo = %full, %error, "stat embed analysis enqueue failed");
        }
        let mut svg = render_analysis_pending(&full, theme);
        if q.in_app() {
            svg = brand::without_embed_footer(svg);
        }
        if format == OutputFormat::Gif {
            let encoded = crate::api::with_raster_permit(move || {
                crate::animated_gif::encode_dither_loop(&svg, theme.bg)
            })
            .await?
            .map_err(ApiError::from)?;
            return Ok(gif_response_with_policy(
                &request_headers,
                Arc::new(encoded.bytes),
                true,
            ));
        }
        let Some(raster_format) = format.raster() else {
            return Ok(svg_response_with_policy(
                &request_headers,
                svg,
                true,
                !q.in_app(),
            ));
        };
        let bytes = crate::api::rasterize_limited(svg, raster_format, RASTER_SCALE).await?;
        return Ok(raster_response_with_policy(
            &request_headers,
            raster_format,
            Arc::new(bytes),
            true,
        ));
    };
    let theme_key = format!(
        "{}|rev:{revision}|{}",
        if theme.dark { "dark" } else { "light" },
        crate::api::RENDER_REVISION,
    );

    // Build (or fetch from cache) the SVG. The cache key is chart-
    // specific and includes all query params that affect output.
    let (svg_key, animated_svg) = match kind {
        StatKind::BugMagnets => ensure_bug_magnets_svg(&state, &full, theme, &theme_key).await?,
        StatKind::TopFiles => {
            ensure_top_files_svg(&state, &full, theme, &theme_key, q.since.as_deref()).await?
        }
        StatKind::Heatmap => ensure_heatmap_svg(&state, &full, theme, &theme_key, q.year).await?,
        StatKind::Contributors => {
            ensure_contributors_svg(&state, &full, theme, &theme_key, &revision).await?
        }
        StatKind::TodoTrend => ensure_todo_trend_svg(&state, &full, theme, &theme_key).await?,
        StatKind::Lines => ensure_lines_svg(&state, &full, theme, &theme_key).await?,
        StatKind::BusFactor => ensure_bus_factor_svg(&state, &full, theme, &theme_key).await?,
        StatKind::CommitTrend => ensure_commit_trend_svg(&state, &full, theme, &theme_key).await?,
    };
    let mut svg = crate::texture::decorate(stat_svg_motion(animated_svg, q.animate()), theme);
    if q.in_app() {
        svg = brand::without_embed_footer(svg);
    }

    if format == OutputFormat::Gif {
        let gif_key = format!("{svg_key}|gif|{}", if q.in_app() { "app" } else { "embed" });
        if let Some(cached) = state.raster_cache.get(&gif_key).await {
            return Ok(gif_response_with_policy(&request_headers, cached, false));
        }
        let encoded = crate::api::with_raster_permit(move || {
            crate::animated_gif::encode_dither_loop(&svg, theme.bg)
        })
        .await?
        .map_err(ApiError::from)?;
        let arc = Arc::new(encoded.bytes);
        state.raster_cache.insert(gif_key, arc.clone()).await;
        return Ok(gif_response_with_policy(&request_headers, arc, false));
    }

    let Some(raster_format) = format.raster() else {
        return Ok(svg_response_with_policy(
            &request_headers,
            svg,
            false,
            !q.in_app(),
        ));
    };

    // Raster path. Key off the SVG's cache key + format suffix so
    // both PNG and WebP variants memoize independently.
    let raster_key = format!(
        "{svg_key}|{}|{}",
        format.cache_suffix(),
        if q.in_app() { "app" } else { "embed" }
    );
    if let Some(cached) = state.raster_cache.get(&raster_key).await {
        return Ok(raster_response(&request_headers, raster_format, cached));
    }
    // Shared semaphore-capped raster path (see api.rs::rasterize_limited)
    // so stat-chart encodes count against the same process-wide CPU cap
    // as the chart/card/OG rasters.
    let bytes = crate::api::rasterize_limited(svg, raster_format, RASTER_SCALE).await?;
    let arc = Arc::new(bytes);
    state.raster_cache.insert(raster_key, arc.clone()).await;
    Ok(raster_response(&request_headers, raster_format, arc))
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

// One contributor per request, addressed by rank
//
// The contributors chart already draws an `<a>` around every avatar, and in a
// README none of them can ever fire: an SVG behind an HTML `<img>` renders in
// SVG2 secure animated mode, where declarative animation still plays but
// script, external references and every form of interactivity are disabled.
// Linking the avatars therefore means one `<a>` per avatar in the README's own
// markup, and that needs one image per avatar — these routes.
//
// The addressing unit is a rank, not a login, because pasted markup has to
// keep working: gitdebt resolves rank → current contributor on every request,
// so a grid re-ranks itself as the repository's history moves and nobody has
// to regenerate the snippet.

/// Only `avatar.{svg,png,webp}` — the same filename-plus-format dispatch idiom
/// as [`parse_filename`]. GIF is deliberately absent: the animated encoder
/// flattens onto a theme background, which is the one thing a README tile must
/// not have.
fn parse_avatar_filename(s: &str) -> Option<OutputFormat> {
    let (name, ext) = s.rsplit_once('.')?;
    if name != "avatar" {
        return None;
    }
    match ext {
        "svg" => Some(OutputFormat::Svg),
        "png" => Some(OutputFormat::Png),
        "webp" => Some(OutputFormat::Webp),
        _ => None,
    }
}

/// A rank is a plain 0-based index. Rejecting `+1`, `01`, whitespace and signs
/// keeps one contributor addressable by exactly one URL, so an edge cache
/// cannot hold the same avatar under a dozen spellings.
fn parse_rank(s: &str) -> Option<usize> {
    if s.is_empty() || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// GitHub's own login grammar: ASCII alphanumeric with single internal
/// hyphens, 1–39 characters.
///
/// Stricter than [`crate::cards::is_valid_login`], which permits `a--b`,
/// because this value is about to be written into a `Location` header. The
/// login comes out of `repo_author_stats`, where it was written by the author
/// enrichment pass from a GitHub API response — a value gitdebt did not author
/// and must not forward on trust.
fn is_github_login(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 39
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// Where a contributor rank sends a reader.
///
/// Anything that is not recognizably a login — an unenriched author, a rank
/// past the roster, a stored value that does not match GitHub's grammar —
/// falls back to the repository's own contributor graph. A slot that cannot be
/// resolved must still land somewhere true; it must never land on whatever the
/// stored string happened to say.
fn contributor_destination(owner: &str, repo: &str, login: Option<&str>) -> String {
    match login.filter(|login| is_github_login(login)) {
        Some(login) => format!("https://github.com/{login}"),
        None => format!("https://github.com/{owner}/{repo}/graphs/contributors"),
    }
}

/// About an hour. This redirect *is* the auto-update mechanism — it is what
/// lets pasted markup keep pointing at whoever holds the rank today — so it
/// cannot be cached on the four-hour asset policy.
const CONTRIBUTOR_REDIRECT_CACHE: &str = "public, max-age=3600, s-maxage=3600";

/// `GET /api/repos/{owner}/{repo}/contributors/{rank}` → the profile of
/// whoever holds that rank right now.
async fn contributor_profile_redirect(
    State(state): State<ApiState>,
    Path((owner, repo, rank)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let full = crate::analyzer::repo_key(&owner, &repo);
    let login = match parse_rank(&rank) {
        Some(rank) if rank < CONTRIBUTOR_RANK_LIMIT => {
            match stat_revision(&state, &full, StatKind::Contributors).await? {
                Some(revision) => contributor_roster(&state, &full, &revision)
                    .await?
                    .get(rank)
                    .and_then(|contributor| contributor.login.clone()),
                // Nothing analyzed yet: the graph is the honest answer, and a
                // one-hour TTL re-asks once the analysis lands.
                None => None,
            }
        }
        _ => None,
    };
    let destination = contributor_destination(&owner, &repo, login.as_deref());
    let Ok(location) = HeaderValue::from_str(&destination) else {
        // Unreachable: every byte came through `is_valid_slug` or
        // `is_github_login`, both of which are visible-ASCII-only.
        return Err(ApiError::bad_request("invalid contributor destination"));
    };
    Ok((
        StatusCode::FOUND,
        [
            (header::LOCATION, location),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(CONTRIBUTOR_REDIRECT_CACHE),
            ),
        ],
    )
        .into_response())
}

/// `GET /api/repos/{owner}/{repo}/contributors/{rank}/avatar.{svg,png,webp}` →
/// one avatar, nothing else.
async fn contributor_avatar(
    State(state): State<ApiState>,
    Path((owner, repo, rank, filename)): Path<(String, String, String, String)>,
    Query(q): Query<StatQuery>,
    request_headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !is_valid_slug(&owner) || !is_valid_slug(&repo) {
        return Err(ApiError::bad_request("invalid owner/repo"));
    }
    let Some(format) = parse_avatar_filename(&filename) else {
        return Err(ApiError::bad_request("unknown avatar format"));
    };
    let Some(rank) = parse_rank(&rank) else {
        return Err(ApiError::bad_request("invalid contributor rank"));
    };
    let theme = theme_for(q.theme.as_deref());
    // Past the roster's own LIMIT the answer is fixed for good, so it costs no
    // query and rides the full asset TTL.
    if rank >= CONTRIBUTOR_RANK_LIMIT {
        return blank_avatar_response(&state, &request_headers, format, false).await;
    }
    let full = crate::analyzer::repo_key(&owner, &repo);
    let Some(revision) = stat_revision(&state, &full, StatKind::Contributors).await? else {
        // Same organic-demand argument as the stat dispatcher: an impression
        // in a README is the only thing that will ever ask for this
        // repository's analysis. Only rank 0 enqueues, so a twelve-slot grid
        // offers the job once per page view instead of twelve times.
        if rank == 0
            && let Err(error) = repo_analysis::enqueue_prioritized(
                state.analyzer.cache.db(),
                &full,
                repo_analysis::VISITOR_PRIORITY_FLOOR,
                None,
            )
            .await
        {
            tracing::warn!(repo = %full, %error, "contributor avatar analysis enqueue failed");
        }
        return blank_avatar_response(&state, &request_headers, format, true).await;
    };
    let roster = contributor_roster(&state, &full, &revision).await?;
    let Some(contributor) = roster.get(rank) else {
        return blank_avatar_response(&state, &request_headers, format, false).await;
    };

    let theme_key = format!(
        "{}|rev:{revision}|{}",
        if theme.dark { "dark" } else { "light" },
        crate::api::RENDER_REVISION,
    );
    let cache_key = format!("contributor-avatar:{full}|{theme_key}|{rank}");
    let animated_svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        let mut row = contributor.clone();
        row.avatar_url = match row.avatar_url {
            // A remote `href` renders as a blank hole behind GitHub's camo
            // proxy, whose CSP is `default-src 'none'; img-src data:`.
            Some(raw) => self_contained_avatar(&state, raw).await,
            None => None,
        };
        Ok(repo_charts::render_contributor_avatar(&row, rank, theme))
    })
    .await?;
    let svg = stat_svg_motion(animated_svg, q.animate());

    let Some(raster_format) = format.raster() else {
        // No site-link overlay: the README's own `<a>` owns this tile, and a
        // second full-surface link inside it would fight for the same pixels.
        return Ok(svg_response_with_policy(
            &request_headers,
            svg,
            false,
            false,
        ));
    };
    let raster_key = format!("{cache_key}|{}", format.cache_suffix());
    if let Some(cached) = state.raster_cache.get(&raster_key).await {
        return Ok(raster_response(&request_headers, raster_format, cached));
    }
    let bytes = crate::api::rasterize_limited(svg, raster_format, RASTER_SCALE).await?;
    let arc = Arc::new(bytes);
    state.raster_cache.insert(raster_key, arc.clone()).await;
    Ok(raster_response(&request_headers, raster_format, arc))
}

/// The empty slot, in whichever format was asked for. Themeless and
/// data-free, so every repository and theme shares one cached raster.
async fn blank_avatar_response(
    state: &ApiState,
    request_headers: &HeaderMap,
    format: OutputFormat,
    pending: bool,
) -> Result<Response, ApiError> {
    let svg = repo_charts::render_blank_avatar();
    let Some(raster_format) = format.raster() else {
        return Ok(svg_response_with_policy(
            request_headers,
            svg,
            pending,
            false,
        ));
    };
    let raster_key = format!(
        "contributor-avatar:blank|{}|{}",
        format.cache_suffix(),
        crate::api::RENDER_REVISION,
    );
    if let Some(cached) = state.raster_cache.get(&raster_key).await {
        return Ok(raster_response_with_policy(
            request_headers,
            raster_format,
            cached,
            pending,
        ));
    }
    let bytes = crate::api::rasterize_limited(svg, raster_format, RASTER_SCALE).await?;
    let arc = Arc::new(bytes);
    state.raster_cache.insert(raster_key, arc.clone()).await;
    Ok(raster_response_with_policy(
        request_headers,
        raster_format,
        arc,
        pending,
    ))
}

async fn stat_revision(
    state: &ApiState,
    repo: &str,
    kind: StatKind,
) -> Result<Option<String>, ApiError> {
    stat_revision_in(&state.analyzer.cache.db().pool, repo, kind).await
}

/// Split from `stat_revision` so the DB-backed cache-key test can drive
/// it with a bare pool.
async fn stat_revision_in(
    pool: &sqlx::PgPool,
    repo: &str,
    kind: StatKind,
) -> Result<Option<String>, ApiError> {
    let revision: Option<(String, i32, i64)> = sqlx::query_as(
        "SELECT history.last_analyzed_sha, history.analysis_revision, \
                COALESCE(history.analysis_scope_commits, 0) \
         FROM repo_history history \
         JOIN repos public_repo ON public_repo.repo = history.repo \
         WHERE history.repo = $1 AND history.last_analyzed_at IS NOT NULL \
           AND public_repo.missing = FALSE \
           AND public_repo.metadata_fetched_at IS NOT NULL",
    )
    .bind(repo)
    .fetch_optional(pool)
    .await?;
    // Writers replace the aggregate tables and cursor in one transaction.
    // While a refresh is queued/running, the previous revision is therefore a
    // complete cache and should stay visible instead of regressing to a
    // misleading pending card.
    let Some((sha, schema, scope)) = revision else {
        return Ok(None);
    };
    let mut revision = format!("{sha}:r{schema}:n{scope}");
    // Author enrichment (github_login/avatar backfill) rewrites
    // repo_author_stats WITHOUT touching repo_history, so the two
    // author-derived charts must fold the enrichment cursor into their
    // revision — otherwise enriched avatars/logins stay invisible for
    // the full moka (24h) + CDN (4h) TTL. `repo` leads the
    // repo_author_stats PK, so MAX over one repo is a cheap index scan,
    // acceptable on this per-request path. The other six kinds keep the
    // bare revision to avoid needless cache churn.
    if matches!(kind, StatKind::Contributors | StatKind::BusFactor) {
        let enriched_epoch: i64 = sqlx::query_scalar(
            "SELECT COALESCE(EXTRACT(EPOCH FROM MAX(enrich_attempted_at)), 0)::BIGINT \
             FROM repo_author_stats WHERE repo = $1",
        )
        .bind(repo)
        .fetch_one(pool)
        .await?;
        revision.push_str(&format!(":e{enriched_epoch}"));
    }
    Ok(Some(revision))
}

/// This frame is returned before the decorated path, so it never reaches
/// `texture::decorate` and has to state its own transparency: a half-pixel
/// inset outlined frame, the same idiom as `cards::chrome`.
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
  <rect x="0.5" y="0.5" width="759" height="179" rx="12" fill="none" stroke="{border}" stroke-width="1"/>
  <text x="28" y="62" fill="{fg}" font-family="ui-sans-serif,system-ui,sans-serif" font-size="19" font-weight="600">{repo}</text>
  <text x="28" y="105" fill="{muted}" font-family="ui-sans-serif,system-ui,sans-serif" font-size="14">Repository analysis is still running</text>
  <text x="28" y="132" fill="{muted}" font-family="ui-sans-serif,system-ui,sans-serif" font-size="12">Refresh shortly for code-health data.</text>
{footer}
</svg>"#,
        border = theme.border,
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
        // A capped analysis window starts at the oldest commit day it walked;
        // everything before that is unobserved, not empty.
        let analyzed_from: Option<NaiveDate> = sqlx::query_scalar(
            "SELECT MIN(day) FROM repo_commit_days WHERE repo = $1 \
             AND EXISTS (SELECT 1 FROM repo_history \
                         WHERE repo = $1 AND analysis_truncated)",
        )
        .bind(full)
        .fetch_one(&state.analyzer.cache.db().pool)
        .await?;
        Ok(repo_charts::render_heatmap(
            full,
            &subtitle,
            from,
            to,
            &days,
            analyzed_from,
            theme,
        ))
    })
    .await?;
    Ok((cache_key, svg))
}

/// Deepest contributor rank gitdebt resolves, and the `LIMIT` on the roster
/// the grid renders. A rank at or past it is out of range by definition, so
/// it is answered without touching Postgres.
const CONTRIBUTOR_RANK_LIMIT: usize = 200;

/// The ordered non-bot author set behind the contributors grid: rank `n` in a
/// README embed is index `n` here.
///
/// Every contributor surface reads this one roster, so the grid, the per-rank
/// avatar and the per-rank profile redirect cannot disagree about who rank `n`
/// is. `author_email` breaks commit ties — without it Postgres may return tied
/// authors in any order, and rank → person is exactly what a pasted README
/// link is addressed by, so an unstable order would silently repoint somebody
/// else's markup.
///
/// Rows keep the remote avatar URL. Only the single rank a request renders is
/// fetched and inlined, so serving a twelve-slot grid never pulls 200 images.
async fn contributor_roster(
    state: &ApiState,
    full: &str,
    revision: &str,
) -> Result<Arc<Vec<ContributorRow>>, ApiError> {
    let key = format!("contributor-roster:{full}|rev:{revision}");
    state
        .contributor_roster_cache
        .try_get_with(key, async {
            let sql = format!(
                "SELECT github_login, author_name, avatar_url, commits FROM repo_author_stats \
                 WHERE repo = $1 AND {NON_BOT_AUTHOR_FILTER} \
                 ORDER BY commits DESC, author_email ASC LIMIT {CONTRIBUTOR_RANK_LIMIT}"
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(full)
                .bind(BOT_LOGINS)
                .fetch_all(&state.analyzer.cache.db().pool)
                .await?;
            Ok::<_, ApiError>(Arc::new(
                rows.into_iter()
                    .map(|r| ContributorRow {
                        login: r
                            .try_get::<Option<String>, _>("github_login")
                            .unwrap_or(None),
                        name: r.try_get("author_name").unwrap_or_default(),
                        avatar_url: r.try_get::<Option<String>, _>("avatar_url").unwrap_or(None),
                        commits: r.try_get("commits").unwrap_or(0),
                    })
                    .collect(),
            ))
        })
        .await
        .map_err(|error| error.clone_shared())
}

async fn ensure_contributors_svg(
    state: &ApiState,
    full: &str,
    theme: &crate::theme::Theme,
    theme_key: &str,
    revision: &str,
) -> Result<(String, String), ApiError> {
    let cache_key = format!("contributors:{full}|{theme_key}");
    let svg = crate::api::single_flight(&state.stat_svg_cache, cache_key.clone(), async {
        let mut rows: Vec<ContributorRow> = (*contributor_roster(state, full, revision).await?)
            .as_slice()
            .to_vec();
        let avatar_urls: Vec<Option<String>> =
            rows.iter().map(|row| row.avatar_url.clone()).collect();
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
             WHERE repo = $1 AND ((lines_code + lines_blank + lines_comment) > 0 OR files > 0) \
             ORDER BY CASE WHEN (lines_code + lines_blank + lines_comment) > 0 \
                      THEN (lines_code + lines_blank + lines_comment) ELSE files END DESC LIMIT 12",
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

fn svg_response_with_policy(
    request_headers: &HeaderMap,
    svg: String,
    pending: bool,
    branded: bool,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, stat_cache_control(pending));
    let body = if branded {
        brand::with_site_link(svg)
    } else {
        svg
    };
    crate::api::conditional_media_response(request_headers, headers, body.into_bytes())
}

fn raster_response(
    request_headers: &HeaderMap,
    format: RasterFormat,
    bytes: Arc<Vec<u8>>,
) -> Response {
    raster_response_with_policy(request_headers, format, bytes, false)
}

fn raster_response_with_policy(
    request_headers: &HeaderMap,
    format: RasterFormat,
    bytes: Arc<Vec<u8>>,
    pending: bool,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(header::CACHE_CONTROL, stat_cache_control(pending));
    crate::api::conditional_media_response(request_headers, headers, (*bytes).clone())
}

fn gif_response_with_policy(
    request_headers: &HeaderMap,
    bytes: Arc<Vec<u8>>,
    pending: bool,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/gif"));
    headers.insert(header::CACHE_CONTROL, stat_cache_control(pending));
    crate::api::conditional_media_response(request_headers, headers, (*bytes).clone())
}

/// Cache policy split for the stat charts. Ready charts ride the same 4h
/// edge policy as the other media (`api::MEDIA_CACHE_CONTROL` semantics); a
/// repo whose analysis hasn't landed yet renders a "pending" frame on a
/// deliberately short TTL so it self-heals within one analysis cycle without
/// making every viewer of the README re-render it at the origin.
fn stat_cache_control(pending: bool) -> HeaderValue {
    if pending {
        // Short, but positive: `no-store` meant every viewer of a README with
        // a not-yet-analyzed embed reached the origin and re-rendered the
        // same placeholder. Five minutes at the edge absorbs that while still
        // self-healing well inside one analysis cycle.
        HeaderValue::from_static("public, s-maxage=300, max-age=60")
    } else {
        HeaderValue::from_static(
            "public, max-age=3600, s-maxage=14400, stale-while-revalidate=86400",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the organic band: an anonymous view of a
    /// repository nobody has opened before must still outrank every
    /// curated-catalog row, which sits at priority 0 with an older
    /// `enqueued_at` and would otherwise win the tie-break forever.
    #[test]
    fn organic_views_outrank_the_background_band() {
        assert_eq!(
            view_priority(0),
            repo_analysis::VISITOR_PRIORITY_FLOOR,
            "a repository nobody has opened yet must still reach the visitor lane"
        );
        // Popularity still orders repositories within the band.
        assert!(view_priority(5) > view_priority(0));
    }

    /// Popularity is a ranking inside the band, never a way out of it: no
    /// view count may promote anonymous work into the warm-up band or above
    /// a signed-in visitor's single repository.
    #[test]
    fn view_priority_stays_inside_its_band() {
        for count in [0, 1, 42, 999_999, i64::MAX] {
            let priority = view_priority(count);
            assert!(
                priority >= repo_analysis::VISITOR_PRIORITY_FLOOR,
                "{count} fell below the visitor band"
            );
            assert!(
                priority < repo_analysis::WARM_PRIORITY,
                "{count} escaped into the warm band"
            );
            assert!(priority < repo_analysis::INTERACTIVE_PRIORITY);
        }
        // A negative counter (impossible in the schema, cheap to survive)
        // clamps to the floor instead of sinking below the catalog.
        assert_eq!(view_priority(-5), repo_analysis::VISITOR_PRIORITY_FLOOR);
    }

    /// Every table the health summary reads, for one test repository.
    #[cfg(test)]
    async fn cleanup_health_rows(pool: &sqlx::PgPool, repo: &str) {
        for statement in [
            "DELETE FROM repo_todo_deltas WHERE repo = $1",
            "DELETE FROM repo_file_stats WHERE repo = $1",
            "DELETE FROM repo_commit_days WHERE repo = $1",
            "DELETE FROM repo_author_stats WHERE repo = $1",
            "DELETE FROM repo_history WHERE repo = $1",
            "DELETE FROM repos WHERE repo = $1",
        ] {
            sqlx::query(statement)
                .bind(repo)
                .execute(pool)
                .await
                .expect("cleanup health rows");
        }
    }

    /// The summary is five aggregate queries across five tables. A renamed
    /// column or a mis-parenthesised FILTER surfaces as HTTP 500 on every
    /// repository rather than as a wrong number, so this runs all of them
    /// against Postgres and pins each aggregate.
    #[tokio::test]
    async fn repo_health_aggregates_over_the_analysis_tables() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let pool = &db.pool;
        let repo = "gitdebt-test-health/repo";
        let unanalyzed = "gitdebt-test-health/cold";
        cleanup_health_rows(pool, repo).await;
        cleanup_health_rows(pool, unanalyzed).await;

        assert!(
            load_repo_health(pool, unanalyzed)
                .await
                .expect("cold health query")
                .is_none(),
            "a repository with no completed analysis has no summary to serve"
        );

        sqlx::query(
            "INSERT INTO repos (repo, star_count, missing, metadata_fetched_at, archived) \
             VALUES ($1, 1234, FALSE, NOW(), FALSE)",
        )
        .bind(repo)
        .execute(pool)
        .await
        .expect("seed repo");
        sqlx::query(
            "INSERT INTO repo_history (repo, total_commits, last_analyzed_at, analysis_truncated) \
             VALUES ($1, 500, NOW(), TRUE)",
        )
        .bind(repo)
        .execute(pool)
        .await
        .expect("seed history");

        // 60 + 30 + 10 attributed; the bot's 900 commits must not count.
        for (email, name, login, commits) in [
            ("a@example.com", "A", Some("a"), 60),
            ("b@example.com", "B", Some("b"), 30),
            ("c@example.com", "C", Some("c"), 10),
            ("bot@example.com", "renovate[bot]", Some("renovate"), 900),
        ] {
            sqlx::query(
                "INSERT INTO repo_author_stats \
                    (repo, author_email, author_name, github_login, commits) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(repo)
            .bind(email)
            .bind(name)
            .bind(login)
            .bind(commits as i64)
            .execute(pool)
            .await
            .expect("seed author");
        }

        let today = Utc::now().date_naive();
        for (offset, commits) in [(10i64, 7i64), (100, 5), (1_000, 3)] {
            sqlx::query("INSERT INTO repo_commit_days (repo, day, commits) VALUES ($1, $2, $3)")
                .bind(repo)
                .bind(today - chrono::Duration::days(offset))
                .bind(commits)
                .execute(pool)
                .await
                .expect("seed commit day");
        }

        for (path, commits, fixes, modified_days_ago) in [
            ("src/app.ts", 40i64, 12i64, 3i64),
            ("legacy/old.ts", 30, 3, 800),
            // A dependency manifest out-changes everything and must still be
            // excluded from the hotspot and from the repair-load ratio.
            ("package.json", 900, 0, 1),
        ] {
            sqlx::query(
                "INSERT INTO repo_file_stats \
                    (repo, path, commits, fix_commits, last_modified_at) \
                 VALUES ($1, $2, $3, $4, NOW() - make_interval(days => $5))",
            )
            .bind(repo)
            .bind(path)
            .bind(commits)
            .bind(fixes)
            .bind(modified_days_ago as i32)
            .execute(pool)
            .await
            .expect("seed file stats");
        }

        for (offset, added, removed) in [(10i64, 9i64, 4i64), (300, 20, 0)] {
            sqlx::query(
                "INSERT INTO repo_todo_deltas (repo, day, todo_added, todo_removed) \
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(repo)
            .bind(today - chrono::Duration::days(offset))
            .bind(added)
            .bind(removed)
            .execute(pool)
            .await
            .expect("seed todo delta");
        }

        let body = load_repo_health(pool, repo)
            .await
            .expect("health query")
            .expect("analyzed repository has a summary");

        assert_eq!(body["ready"], serde_json::json!(true));
        assert_eq!(body["stars"], serde_json::json!(1234));
        assert_eq!(body["total_commits"], serde_json::json!(500));
        assert_eq!(body["analysis_truncated"], serde_json::json!(true));

        assert_eq!(body["attributed_commits"], serde_json::json!(100));
        assert_eq!(body["contributors"], serde_json::json!(3));
        assert_eq!(body["top_author_commits"], serde_json::json!(60));
        // 60 of 100 attributed commits pass half on the first author.
        assert_eq!(body["bus_factor"], serde_json::json!(1));

        assert_eq!(body["commits_window"], serde_json::json!(7));
        assert_eq!(body["commits_previous_window"], serde_json::json!(5));
        assert_eq!(
            body["last_commit_day"].as_str(),
            Some((today - chrono::Duration::days(10)).to_string()).as_deref()
        );

        let months = body["commit_months"]
            .as_array()
            .expect("commit months array");
        assert_eq!(months.len(), HEALTH_MONTHS);
        assert_eq!(
            months
                .iter()
                .filter_map(|month| month["commits"].as_i64())
                .sum::<i64>(),
            12,
            "the day ~33 months back falls outside the sparkline window"
        );
        assert_eq!(
            months.last().and_then(|month| month["month"].as_str()),
            Some(format!("{:04}-{:02}", today.year(), today.month()).as_str()),
            "the series ends on the current month"
        );

        assert_eq!(body["tracked_files"], serde_json::json!(2));
        assert_eq!(body["file_changes"], serde_json::json!(70));
        assert_eq!(body["fix_changes"], serde_json::json!(15));
        assert_eq!(body["fresh_files"], serde_json::json!(1));
        assert_eq!(body["hotspot"]["path"], serde_json::json!("src/app.ts"));
        assert_eq!(body["hotspot"]["commits"], serde_json::json!(40));
        assert_eq!(body["hotspot"]["fix_commits"], serde_json::json!(12));

        assert_eq!(body["todo_delta_window"], serde_json::json!(5));
        assert_eq!(body["todo_outstanding"], serde_json::json!(25));

        cleanup_health_rows(pool, repo).await;
    }

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
        assert!(matches!(
            parse_filename("commit-trend.gif"),
            Some((StatKind::CommitTrend, OutputFormat::Gif))
        ));
    }

    #[test]
    fn parse_filename_rejects_unknown_names_and_formats() {
        assert!(parse_filename("bus-factor").is_none());
        assert!(parse_filename("unknown.svg").is_none());
    }

    #[test]
    fn avatar_filenames_dispatch_three_still_formats() {
        assert!(matches!(
            parse_avatar_filename("avatar.svg"),
            Some(OutputFormat::Svg)
        ));
        assert!(matches!(
            parse_avatar_filename("avatar.png"),
            Some(OutputFormat::Png)
        ));
        assert!(matches!(
            parse_avatar_filename("avatar.webp"),
            Some(OutputFormat::Webp)
        ));
        // A GIF frame is flattened onto a theme background, which is the one
        // thing a README tile must not carry.
        assert!(parse_avatar_filename("avatar.gif").is_none());
        assert!(parse_avatar_filename("contributors.svg").is_none());
        assert!(parse_avatar_filename("avatar").is_none());
    }

    /// One contributor, one URL: an edge cache must not end up holding the
    /// same avatar under `0`, `00`, `+0` and ` 0`.
    #[test]
    fn ranks_are_plain_zero_based_indices() {
        assert_eq!(parse_rank("0"), Some(0));
        assert_eq!(parse_rank("11"), Some(11));
        assert_eq!(parse_rank("199"), Some(199));
        assert!(parse_rank("00").is_none());
        assert!(parse_rank("01").is_none());
        assert!(parse_rank("+1").is_none());
        assert!(parse_rank("-1").is_none());
        assert!(parse_rank(" 1").is_none());
        assert!(parse_rank("1.0").is_none());
        assert!(parse_rank("").is_none());
        assert!(parse_rank("one").is_none());
        // Past the roster's LIMIT the handler answers blank without a query;
        // it still has to parse, or it would 400 instead.
        assert_eq!(parse_rank("200"), Some(200));
    }

    /// This value is written into a `Location` header. It comes from
    /// `repo_author_stats`, where the enrichment pass copied it out of a
    /// GitHub API response — a string gitdebt did not author. Anything that is
    /// not recognizably a login lands on the repository's contributor graph.
    #[test]
    fn only_a_real_login_reaches_the_location_header() {
        assert!(is_github_login("zhom"));
        assert!(is_github_login("rust-lang"));
        assert!(is_github_login("a"));
        assert!(is_github_login(&"a".repeat(39)));

        assert!(!is_github_login(""));
        assert!(!is_github_login(&"a".repeat(40)));
        assert!(!is_github_login("-lead"));
        assert!(!is_github_login("trail-"));
        // GitHub's grammar allows single hyphens only.
        assert!(!is_github_login("double--hyphen"));
        assert!(!is_github_login("under_score"));
        assert!(!is_github_login("dot.ted"));
        assert!(!is_github_login("héllo"));

        let graph = "https://github.com/owner/repo/graphs/contributors";
        assert_eq!(
            contributor_destination("owner", "repo", Some("zhom")),
            "https://github.com/zhom"
        );
        // A rank nobody holds, and an author the enrichment pass never named.
        assert_eq!(contributor_destination("owner", "repo", None), graph);
        // Hostile stored values must reach the fallback, never the header.
        for hostile in [
            "evil.com",
            "//evil.com",
            "..",
            "../../evil",
            "a/../../evil",
            "zhom@evil.com",
            "zhom?next=https://evil.com",
            "zhom#@evil.com",
            "zhom\r\nLocation: https://evil.com",
            "zhom\nSet-Cookie: session=1",
            " zhom",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                contributor_destination("owner", "repo", Some(hostile)),
                graph,
                "{hostile} must not reach the Location header"
            );
        }
    }

    /// A slot past the end of the roster answers 200 with an empty tile.
    /// A 404 would draw the broken-image glyph in the README, and a visible
    /// placeholder would assert a contributor who does not exist.
    #[test]
    fn an_empty_slot_is_a_transparent_two_hundred() {
        let response = svg_response_with_policy(
            &HeaderMap::new(),
            repo_charts::render_blank_avatar(),
            false,
            false,
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/svg+xml; charset=utf-8"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            &stat_cache_control(false)
        );
        // Unbranded: the README's own <a> owns the tile, so no full-surface
        // gitdebt link is layered under it.
        let svg = repo_charts::render_blank_avatar();
        assert!(!svg.contains("data-gitdebt-surface-link"));

        // An unanalyzed repository answers the same tile on the short policy
        // so the grid fills in within one analysis cycle.
        let pending = svg_response_with_policy(&HeaderMap::new(), svg, true, false);
        assert_eq!(pending.status(), StatusCode::OK);
        assert_eq!(
            pending.headers().get(header::CACHE_CONTROL).unwrap(),
            &stat_cache_control(true)
        );
    }

    /// The redirect is the auto-update mechanism: pasted markup addresses a
    /// rank, and the rank has to be re-resolved on a human timescale rather
    /// than pinned for the four hours an image rides.
    #[test]
    fn the_rank_redirect_expires_in_about_an_hour() {
        assert_eq!(
            CONTRIBUTOR_REDIRECT_CACHE,
            "public, max-age=3600, s-maxage=3600"
        );
        assert_ne!(
            CONTRIBUTOR_REDIRECT_CACHE,
            stat_cache_control(false).to_str().unwrap()
        );
    }

    /// Path conflicts are a construction-time panic in axum, and the rank
    /// routes sit one literal segment away from `stats/{filename}` — the
    /// pattern most likely to collide with them.
    #[test]
    fn the_public_router_builds_with_the_rank_routes() {
        let _router: Router<ApiState> = public_router();
    }

    /// `ApiState` over the test database, or `None` (test no-ops) when
    /// `GITDEBT_TEST_DATABASE_URL` is unset.
    #[cfg(test)]
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
        let analyzer = crate::analyzer::AnalyzerCtx {
            github,
            cache: crate::cache::Cache::new(db),
        };
        Some(
            crate::api::ApiState::with_settings(
                analyzer,
                None,
                std::sync::Arc::new(crate::repo_history::RepoStorage::from_env()),
                None,
                "http://localhost:14321".to_string(),
                Some("http://localhost:8787".to_string()),
                None,
            )
            .expect("api state"),
        )
    }

    /// Rank → author is the whole contract: pasted markup names a rank, and
    /// gitdebt has to resolve it to the same person the contributors grid
    /// draws in that position. Bots are excluded from both, the roster stops
    /// at [`CONTRIBUTOR_RANK_LIMIT`], and tied commit counts order by
    /// `author_email` so the mapping cannot drift between two requests.
    #[tokio::test]
    async fn contributor_ranks_resolve_to_the_grid_order() {
        let Some(state) = test_db_state().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let pool = state.analyzer.cache.db().pool.clone();
        let repo = format!("gitdebt-test-rank/{}", std::process::id());
        cleanup_health_rows(&pool, &repo).await;

        sqlx::query(
            "INSERT INTO repos (repo, missing, metadata_fetched_at) VALUES ($1, FALSE, NOW())",
        )
        .bind(&repo)
        .execute(&pool)
        .await
        .expect("seed repo");
        sqlx::query(
            "INSERT INTO repo_history \
                (repo, total_commits, last_analyzed_sha, last_analyzed_at, analysis_revision) \
             VALUES ($1, 30, 'abc', NOW(), 1)",
        )
        .bind(&repo)
        .execute(&pool)
        .await
        .expect("seed history");
        for (email, name, login, commits) in [
            ("top@example.com", "Top", Some("top-author"), 30i64),
            // Tied on commits: `author_email` decides, so `b@` is rank 1 and
            // `c@` is rank 2 on every request.
            ("b@example.com", "B", Some("bee"), 10),
            ("c@example.com", "C", None, 10),
            ("bot@example.com", "renovate[bot]", Some("renovate"), 900),
        ] {
            sqlx::query(
                "INSERT INTO repo_author_stats \
                    (repo, author_email, author_name, github_login, commits) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&repo)
            .bind(email)
            .bind(name)
            .bind(login)
            .bind(commits)
            .execute(&pool)
            .await
            .expect("seed author");
        }

        let revision = stat_revision(&state, &repo, StatKind::Contributors)
            .await
            .expect("revision query")
            .expect("an analyzed repository has a revision");
        let roster = contributor_roster(&state, &repo, &revision)
            .await
            .expect("roster query");

        assert_eq!(roster.len(), 3, "the bot must not occupy a rank");
        assert_eq!(roster[0].login.as_deref(), Some("top-author"));
        assert_eq!(roster[1].login.as_deref(), Some("bee"));
        assert_eq!(roster[2].login, None);
        assert_eq!(
            contributor_destination("owner", "repo", roster[0].login.as_deref()),
            "https://github.com/top-author"
        );
        // Rank 2 exists but nobody enriched it, so it falls back rather than
        // redirecting to a non-login.
        assert_eq!(
            contributor_destination("owner", "repo", roster[2].login.as_deref()),
            "https://github.com/owner/repo/graphs/contributors"
        );
        // Past the end of the roster there is no author to resolve.
        assert!(roster.get(3).is_none());
        assert!(roster.get(CONTRIBUTOR_RANK_LIMIT).is_none());

        // A second call is served from the shared roster cache: a twelve-slot
        // grid must not become twelve identical ranked queries.
        let again = contributor_roster(&state, &repo, &revision)
            .await
            .expect("roster query");
        assert!(Arc::ptr_eq(&roster, &again));

        cleanup_health_rows(&pool, &repo).await;
    }

    #[test]
    fn pending_stats_use_the_short_self_healing_policy() {
        assert_eq!(
            stat_cache_control(true).to_str().unwrap(),
            "public, s-maxage=300, max-age=60"
        );
        let svg = render_analysis_pending("o/r", &crate::theme::LIGHT);
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.contains(">gitdebt</text>"));
    }

    /// Ready stat charts ride the shared 4h edge policy; the pending frame
    /// expires in minutes so a finished analysis shows up promptly.
    #[test]
    fn ready_stats_get_the_edge_cache_policy() {
        assert_eq!(
            stat_cache_control(false).to_str().unwrap(),
            "public, max-age=3600, s-maxage=14400, stale-while-revalidate=86400"
        );
    }

    /// The stat SVG/raster helpers speak conditional requests: a 200
    /// carries an ETag, replaying it via `If-None-Match` yields a 304 with
    /// the same Cache-Control, and a stale tag re-serves the bytes.
    #[tokio::test]
    async fn stat_responses_support_etag_revalidation() {
        let none = HeaderMap::new();
        let first = svg_response_with_policy(&none, "<svg>ready</svg>".into(), false, false);
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(
            first.headers().get(header::CACHE_CONTROL).unwrap(),
            &stat_cache_control(false)
        );
        let etag = first.headers().get(header::ETAG).unwrap().clone();

        let mut revalidate = HeaderMap::new();
        revalidate.insert(header::IF_NONE_MATCH, etag.clone());
        let not_modified =
            svg_response_with_policy(&revalidate, "<svg>ready</svg>".into(), false, false);
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(not_modified.headers().get(header::ETAG).unwrap(), &etag);
        assert_eq!(
            not_modified.headers().get(header::CACHE_CONTROL).unwrap(),
            &stat_cache_control(false)
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
        let raster = raster_response_with_policy(
            &mismatched,
            RasterFormat::Png,
            Arc::new(vec![1u8, 2, 3]),
            false,
        );
        assert_eq!(raster.status(), StatusCode::OK);
        assert!(raster.headers().get(header::ETAG).is_some());
    }

    #[test]
    fn in_app_media_context_is_explicit() {
        assert!(!StatQuery::default().in_app());
        assert!(
            StatQuery {
                context: Some("app".into()),
                ..StatQuery::default()
            }
            .in_app()
        );
    }

    /// Author enrichment rewrites `repo_author_stats` without touching
    /// `repo_history`, so the contributors and bus-factor cache
    /// revisions must advance with `enrich_attempted_at` — otherwise
    /// backfilled logins/avatars stay invisible for the full moka+CDN
    /// TTL. Every other chart kind must keep the bare revision so an
    /// enrichment pass can't churn their caches.
    #[tokio::test]
    async fn author_enrichment_advances_only_author_chart_revisions() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let repo = format!("gitdebt-test-stat-revision/{}", std::process::id());
        let cleanup = |pool: sqlx::PgPool, repo: String| async move {
            for statement in [
                "DELETE FROM repo_author_stats WHERE repo = $1",
                "DELETE FROM repo_history WHERE repo = $1",
                "DELETE FROM repos WHERE repo = $1",
            ] {
                sqlx::query(statement)
                    .bind(&repo)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        };
        cleanup(db.pool.clone(), repo.clone()).await;

        sqlx::query("INSERT INTO repos (repo, metadata_fetched_at) VALUES ($1, NOW())")
            .bind(&repo)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO repo_history \
                (repo, last_analyzed_sha, last_analyzed_at, \
                 analysis_revision, analysis_scope_commits) \
             VALUES ($1, 'abc', NOW(), 3, 42)",
        )
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();

        let revision = |kind: StatKind| {
            let pool = db.pool.clone();
            let repo = repo.clone();
            async move {
                stat_revision_in(&pool, &repo, kind)
                    .await
                    .unwrap()
                    .expect("analyzed repo must have a revision")
            }
        };
        let contributors = revision(StatKind::Contributors).await;
        let bus_factor = revision(StatKind::BusFactor).await;
        let heatmap = revision(StatKind::Heatmap).await;
        assert!(
            contributors.starts_with("abc:r3:n42"),
            "analysis fields still lead the revision: {contributors}"
        );

        // A brand-new author row with no enrichment attempt keeps the
        // same revision as having no author rows at all (both epoch 0).
        sqlx::query(
            "INSERT INTO repo_author_stats (repo, author_email, commits) \
             VALUES ($1, 'a@example.com', 1)",
        )
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();
        assert_eq!(
            revision(StatKind::Contributors).await,
            contributors,
            "unattempted enrichment must not move the revision"
        );

        // Stamping the enrichment attempt must move both author charts
        // and nothing else.
        sqlx::query(
            "UPDATE repo_author_stats \
             SET enrich_attempted_at = TIMESTAMPTZ '2026-01-01 00:00:00+00' \
             WHERE repo = $1",
        )
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();
        let enriched = revision(StatKind::Contributors).await;
        assert_ne!(
            enriched, contributors,
            "enrichment must bust the contributors cache key"
        );
        assert_ne!(
            revision(StatKind::BusFactor).await,
            bus_factor,
            "enrichment must bust the bus-factor cache key"
        );
        assert_eq!(
            revision(StatKind::Heatmap).await,
            heatmap,
            "non-author charts must not churn on enrichment"
        );

        // A later attempt (avatar/login backfill retry) moves it again.
        sqlx::query(
            "UPDATE repo_author_stats \
             SET enrich_attempted_at = TIMESTAMPTZ '2026-01-01 00:00:01+00' \
             WHERE repo = $1",
        )
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();
        assert_ne!(
            revision(StatKind::Contributors).await,
            enriched,
            "advancing enrich_attempted_at must advance the revision"
        );

        cleanup(db.pool.clone(), repo.clone()).await;
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
