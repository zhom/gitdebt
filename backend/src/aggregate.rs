//! Org/user aggregate star history: sums the cumulative star series of a
//! login's public repos into one series — the "all my repos on one chart"
//! view star-history.com never shipped (their issue #187 territory).
//!
//! Pipeline (mirrors the repo analyze path's non-blocking discipline):
//!
//!   1. **Resolve the login's repos.** The `login → repos` mapping is
//!      cached in Postgres (`login_repo_lists` / `login_repos`, see
//!      `cache.rs`) with a [`LOGIN_REPOS_TTL`]. On a cold or stale login we
//!      make ONE rate-limited repos-list call ([`GithubClient::user_repos`]
//!      — works for users and orgs), guarded by the non-blocking
//!      `has_budget` probe so an exhausted GitHub bucket degrades to the
//!      stale cached list instead of hanging the request. The list is
//!      capped at the top [`MAX_AGGREGATE_REPOS`] repos by star count
//!      (client-side sort — the API can't sort by stars).
//!   2. **Sum the cached star history.** Per-day star deltas come from
//!      `repo_stargazers` aggregated **in SQL** (one row per repo per
//!      calendar day — never one row per stargazer in memory), then are
//!      merged by the pure [`merge_day_deltas`] + [`deltas_to_series`]
//!      math. Only repos with `stargazers_complete = TRUE` contribute
//!      (readers never trust partial data); cold/incomplete repos are
//!      enqueued on the existing `star_fetch_queue` — the request NEVER
//!      blocks on fetching stars — and reported in `repos_pending`.
//!
//! NOTE (2026-06 stargazers-endpoint restriction): this module adds no new
//! stargazer pagination. Star data is read exclusively from Postgres, and
//! uncached repos ride the existing queue/worker path.
//!
//! Everything except the two `load_*` functions and [`build`] is pure —
//! the chart endpoints cache bytes-exact upstream, so the series math must
//! be free of clocks and randomness (it is; `BTreeMap` keeps day merging
//! deterministic).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use chrono::{NaiveDate, TimeZone, Utc};
use serde::Serialize;
use sqlx::Row;

use crate::analyzer::{self, AnalyzerCtx};
use crate::chart::{self, Point};
use crate::db::Db;
use crate::github::GithubClient;
use crate::github::RepoListItem;
use crate::repo_analysis;
use crate::repo_endpoints::is_valid_slug;

/// Cap on the number of repos included in a login's aggregate. Matches the
/// locked API contract ("top 50 by stars"); beyond it the marginal series
/// contribution is noise while the per-repo enqueue/query cost is linear.
pub const MAX_AGGREGATE_REPOS: usize = 50;

/// Cap on star-fetch enqueues per aggregate [`build`]. Without it one cold
/// login enqueued up to [`MAX_AGGREGATE_REPOS`] jobs, so a crawl (or a
/// scripted loop) over cold logins could fill the global
/// `MAX_PENDING_FETCHES` ceiling ~50× faster than any other surface,
/// crowding organic repos out of the queue. The list is stars-descending,
/// so the cap keeps the *biggest* cold repos flowing first; the rest still
/// count as `repos_pending` and are enqueued by later builds (the
/// aggregate is memoized ~5 min upstream) as earlier batches drain.
pub const MAX_ENQUEUES_PER_BUILD: usize = 10;

/// Repo-history work offered by one uncached profile build. Profile discovery
/// initializes code-health data as well as star history, while clones remain
/// background-only and globally capacity bounded.
const MAX_ANALYSIS_ENQUEUES_PER_BUILD: usize = 8;

/// How long a cached `login → repos` mapping is trusted before a live
/// repos-list refresh is attempted. Repo lists move slowly (new repos +
/// renames); 12h keeps a hot org page at ~2 list calls/day while a
/// newly-created hit repo still shows up same-day. The 404 tombstone uses
/// the same TTL so a renamed/recreated account recovers.
pub const LOGIN_REPOS_TTL: chrono::Duration = chrono::Duration::hours(12);

/// Cap on the number of points in the aggregate `history` array, matching
/// the repo `/analyze` cap (`analyzer::MAX_HISTORY_POINTS` is private, but
/// the value is part of the same payload-size budget).
pub const MAX_HISTORY_POINTS: usize = 400;

// Live repository-list throttle
// `resolve_login_repos` is the only place a public, unauthenticated request
// spends GitHub budget *synchronously* (every other surface only enqueues).
// The login namespace is infinite, so per-IP governors and the 404
// tombstone cannot bound the aggregate burn rate: a client cycling made-up
// logins inside the analyze governor would otherwise drain the shared PAT
// bucket and stall the background star-fetch workers site-wide. The fix is
// a process-wide fixed-window budget on live repos-list fetches,
// pessimistically costed at the [`GithubClient::user_repos`] page cap.
// Exhausted window → same degrade path as "no GitHub budget": stale cached
// list, else `Busy` (503, retry later). The window mutex also closes the
// probe race (N concurrent requests can no longer all pass `has_budget`
// and all spend — the debit is atomic).

/// Fixed-window length for the live repos-list budget.
const LIVE_LIST_WINDOW_SECS: i64 = 60;

/// Pessimistic per-fetch cost in GitHub API calls: `user_repos` paginates
/// up to 10 pages (`github.rs::REPO_LIST_MAX_PAGES`). Debiting the worst
/// case keeps the accounting simple and errs toward protecting the budget.
const LIVE_LIST_FETCH_COST: u32 = 10;

/// Per-window budget in API-call units. 30/min with a flat cost of 10 →
/// at most 3 live list fetches per minute process-wide: worst case
/// ~1,800 GitHub calls/hr (every login a max-page org), ~180/hr for the
/// junk-login pattern (one 404 call each) — the shared 5k/hr PAT keeps
/// majority headroom for the background workers under any request mix.
const LIVE_LIST_CALLS_PER_WINDOW: u32 = 30;

/// Pure fixed-window accounting: `state` is `(window_index, spent)`.
/// Rolls the window when `now` crosses a boundary, refuses when the debit
/// would exceed `budget`, records it otherwise. Pure (caller supplies the
/// clock) so the arithmetic is unit-testable.
fn window_try_spend(
    state: &mut (i64, u32),
    now_secs: i64,
    window_secs: i64,
    cost: u32,
    budget: u32,
) -> bool {
    let window = now_secs.div_euclid(window_secs);
    if state.0 != window {
        *state = (window, 0);
    }
    let spent_after = state.1.saturating_add(cost);
    if spent_after > budget {
        return false;
    }
    state.1 = spent_after;
    true
}

/// Try to debit one live repos-list fetch from the process-wide window.
fn try_spend_live_list_fetch() -> bool {
    static WINDOW: std::sync::Mutex<(i64, u32)> = std::sync::Mutex::new((0, 0));
    let mut state = WINDOW
        .lock()
        .expect("live-list window mutex never poisoned");
    window_try_spend(
        &mut state,
        Utc::now().timestamp(),
        LIVE_LIST_WINDOW_SECS,
        LIVE_LIST_FETCH_COST,
        LIVE_LIST_CALLS_PER_WINDOW,
    )
}

/// Errors the API layer maps to distinct HTTP statuses. Everything else
/// (DB failures etc.) flows through `Other` → 500 with a generic body.
#[derive(Debug, thiserror::Error)]
pub enum AggregateError {
    /// GitHub says the login doesn't exist (fresh 404 tombstone or a live
    /// 404). → 404.
    #[error("login not found")]
    LoginNotFound,
    /// No cached repo list AND no GitHub budget headroom (or the live
    /// fetch failed) — nothing trustworthy to serve. → 503; the client
    /// retries later. Never blocks waiting for quota.
    #[error("login repo list unavailable")]
    Busy,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// The aggregate result. `series` is the full day-granular cumulative
/// series (the chart endpoints window+render it); the JSON endpoint
/// downsamples via [`UserAggregate::to_json`].
#[derive(Debug, Clone, Serialize)]
pub struct UserAggregate {
    pub login: String,
    /// Repos whose complete cached star history contributed to the sum.
    pub repos_included: u32,
    /// Repos in the top-50 list without complete history yet — enqueued on
    /// the star-fetch queue; the aggregate grows as they land.
    pub repos_pending: u32,
    /// Owned repositories whose code-health analysis is complete and not
    /// currently being refreshed.
    pub repos_analyzed: u32,
    /// Owned repositories still waiting on or running code-health analysis.
    pub repos_analyzing: u32,
    /// Full summed total (not window-filtered), like `/analyze`.
    pub total_stars: u64,
    pub series: Vec<Point>,
}

impl UserAggregate {
    /// The `/api/users/:login/analyze` JSON body. Locked contract:
    /// `{login,repos_included,repos_pending,repos_analyzed,repos_analyzing,`
    /// `total_stars,history:[{date,stars}]}`.
    /// `history` is downsampled to ≤ [`MAX_HISTORY_POINTS`], same policy as
    /// the repo `/analyze` payload.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "login": self.login,
            "repos_included": self.repos_included,
            "repos_pending": self.repos_pending,
            "repos_analyzed": self.repos_analyzed,
            "repos_analyzing": self.repos_analyzing,
            "total_stars": self.total_stars,
            "history": chart::downsample(&self.series, MAX_HISTORY_POINTS),
        })
    }
}

/// Validate a login path segment. Same charset rules as repo-slug segments
/// (`is_valid_slug`: ASCII alphanumeric + `.`/`_`/`-`, no `.`/`..`) plus
/// GitHub's 39-char login cap. Strictly more permissive than GitHub's own
/// login grammar, so no legitimate login is rejected, while path traversal
/// and metacharacters never reach the DB or the GitHub client.
pub fn is_valid_login(s: &str) -> bool {
    s.len() <= 39 && is_valid_slug(s)
}

/// Pick the top `cap` repos by star count from a repos-list response.
/// Normalizes slugs to lowercase (the cache convention shared with
/// `ext_ping`/export), validates both segments (a malformed `full_name`
/// is dropped, never propagated), dedups, and breaks star ties by slug so
/// the output is fully deterministic.
pub fn top_repos_by_stars(items: &[RepoListItem], cap: usize) -> Vec<(String, i64)> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<(String, i64)> = Vec::new();
    for it in items {
        let Some((owner, name)) = it.full_name.split_once('/') else {
            continue;
        };
        if !is_valid_slug(owner) || !is_valid_slug(name) {
            continue;
        }
        let slug = format!(
            "{}/{}",
            owner.to_ascii_lowercase(),
            name.to_ascii_lowercase()
        );
        if seen.insert(slug.clone()) {
            out.push((slug, it.stargazers_count.max(0)));
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out.truncate(cap);
    out
}

/// Merge several repos' per-day star-delta series into one summed per-day
/// series, date-ascending. Overlapping days add; days unique to one repo
/// pass through; empty inputs contribute nothing. Negative deltas (can't
/// occur from the `COUNT(*)` loader) are clamped to 0 defensively, same as
/// `export::accumulate`. Deterministic: `BTreeMap` orders by date.
pub fn merge_day_deltas(per_repo: &[Vec<(NaiveDate, i64)>]) -> Vec<(NaiveDate, i64)> {
    let mut acc: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for series in per_repo {
        for (day, delta) in series {
            *acc.entry(*day).or_insert(0) += (*delta).max(0);
        }
    }
    acc.into_iter().collect()
}

/// Fold merged per-day deltas (date-ascending) into a cumulative series of
/// chart [`Point`]s (one per day, at midnight UTC — deterministic) plus the
/// full summed total. `Point::stars` is `u32`; the running total saturates
/// there while the returned `u64` total stays exact.
pub fn deltas_to_series(deltas: &[(NaiveDate, i64)]) -> (Vec<Point>, u64) {
    let mut total: u64 = 0;
    let mut out = Vec::with_capacity(deltas.len());
    for (day, delta) in deltas {
        total = total.saturating_add((*delta).max(0) as u64);
        let at = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight is valid"));
        out.push(Point {
            at,
            stars: total.min(u32::MAX as u64) as u32,
        });
    }
    (out, total)
}

/// Per-day star deltas for a set of repos, aggregated **in SQL** (one row
/// per repo per calendar day, UTC-bucketed for session-TZ independence),
/// grouped into per-repo vectors. Same caller-gates-completeness contract
/// as `export::load_day_deltas`: callers must only pass repos whose
/// `stargazers_complete` flag is set — this reads raw rows.
pub async fn load_day_deltas_by_repo(
    db: &Db,
    repos: &[String],
) -> Result<Vec<Vec<(NaiveDate, i64)>>> {
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT repo, (starred_at AT TIME ZONE 'UTC')::date AS day, COUNT(*) AS delta \
         FROM active_repo_star_history \
         WHERE repo = ANY($1) \
         GROUP BY 1, 2 \
         ORDER BY 1, 2",
    )
    .bind(repos)
    .fetch_all(&db.pool)
    .await?;
    let mut out: Vec<Vec<(NaiveDate, i64)>> = Vec::new();
    let mut cur: Option<String> = None;
    for row in rows {
        let repo: String = row.try_get("repo")?;
        let day: NaiveDate = row.try_get("day")?;
        let delta: i64 = row.try_get("delta")?;
        if cur.as_deref() != Some(repo.as_str()) {
            out.push(Vec::new());
            cur = Some(repo);
        }
        out.last_mut().expect("pushed above").push((day, delta));
    }
    Ok(out)
}

/// Per-repo `(stargazers_complete, missing)` flags for a slug set, in one
/// query. History is readable only after metadata has proved that the repo is
/// public. Slugs absent from `repos` (never seen) simply don't appear in the
/// map — the caller treats them as cold.
pub async fn load_repo_states(db: &Db, repos: &[String]) -> Result<HashMap<String, (bool, bool)>> {
    if repos.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        "SELECT repo, \
                (history_complete AND metadata_fetched_at IS NOT NULL) \
                    AS stargazers_complete, \
                missing \
             FROM repos WHERE repo = ANY($1)",
    )
    .bind(repos)
    .fetch_all(&db.pool)
    .await?;
    let mut out = HashMap::with_capacity(rows.len());
    for row in rows {
        let repo: String = row.try_get("repo")?;
        let complete: bool = row.try_get("stargazers_complete")?;
        let missing: bool = row.try_get("missing")?;
        out.insert(repo, (complete, missing));
    }
    Ok(out)
}

/// Build the aggregate for a login. Non-blocking on star data (cold repos
/// are enqueued, never fetched inline); at most one live GitHub call (the
/// TTL'd repos-list) and only when budget headroom exists. The caller
/// validates the login ([`is_valid_login`]); this normalizes to lowercase.
pub async fn build(ctx: &AnalyzerCtx, login: &str) -> Result<UserAggregate, AggregateError> {
    let login = login.to_ascii_lowercase();
    let repos = resolve_login_repos(ctx, &login, None).await?;
    build_from_repos(ctx, login, repos, true, None).await
}

/// Authenticated self-profile build. The caller has already proved that
/// `user_id` owns `login`. The OAuth-scoped client is used only for that
/// account's repository discovery, while durable work stores the user id—not
/// the token—and receives interactive priority.
pub async fn build_for_user(
    ctx: &AnalyzerCtx,
    login: &str,
    user_id: i64,
    github: Arc<GithubClient>,
) -> Result<UserAggregate, AggregateError> {
    let login = login.to_ascii_lowercase();
    let repos = resolve_login_repos(ctx, &login, Some(github.as_ref())).await?;
    build_from_repos(ctx, login, repos, true, Some(user_id)).await
}

/// Build an aggregate exclusively from cached Postgres state. Static-site
/// generation uses this path so it neither spends GitHub budget nor creates
/// star/code-health jobs for pages nobody has opened.
pub async fn build_readonly(
    ctx: &AnalyzerCtx,
    login: &str,
) -> Result<UserAggregate, AggregateError> {
    let login = login.to_ascii_lowercase();
    let meta = ctx.cache.get_login_repos_meta(&login).await?;
    if meta.as_ref().is_some_and(|value| value.missing) {
        return Err(AggregateError::LoginNotFound);
    }
    let repos = ctx
        .cache
        .get_login_repos(&login)
        .await?
        .ok_or(AggregateError::Busy)?;
    build_from_repos(ctx, login, repos, false, None).await
}

async fn build_from_repos(
    ctx: &AnalyzerCtx,
    login: String,
    repos: Vec<(String, i64)>,
    enqueue: bool,
    user_id: Option<i64>,
) -> Result<UserAggregate, AggregateError> {
    let slugs: Vec<String> = repos.into_iter().map(|(slug, _)| slug).collect();

    let db = ctx.cache.db();
    let states = load_repo_states(db, &slugs).await?;

    let mut included: Vec<String> = Vec::new();
    let mut analysis_candidates: Vec<String> = Vec::new();
    let mut pending: u32 = 0;
    let mut enqueued: usize = 0;
    let star_enqueue_limit = if user_id.is_some() {
        MAX_AGGREGATE_REPOS
    } else {
        MAX_ENQUEUES_PER_BUILD
    };
    for slug in &slugs {
        match states.get(slug.as_str()) {
            // Complete cached history → contributes to the sum. (A
            // tombstoned-but-complete repo keeps its real historical data.)
            Some((true, _)) => included.push(slug.clone()),
            // Tombstoned with no complete history: it will never complete —
            // neither included nor pending (and never re-enqueued).
            Some((false, true)) => continue,
            // Cold or partial → ride the existing star-fetch queue
            // (idempotent dedup + tombstone/ceiling guards live inside),
            // capped per build ([`MAX_ENQUEUES_PER_BUILD`]) so one login
            // can't flood the global queue ceiling. Repos past the cap
            // still count as pending — "no complete history yet" — and get
            // enqueued by later builds as earlier batches drain.
            _ => {
                if enqueue && enqueued < star_enqueue_limit {
                    if user_id.is_some() {
                        if let Err(error) =
                            crate::queue::enqueue(db, slug, repo_analysis::INTERACTIVE_PRIORITY)
                                .await
                        {
                            tracing::warn!(repo = %slug, %error, "interactive star enqueue failed");
                        }
                    } else {
                        analyzer::enqueue_fetch(ctx, slug).await;
                    }
                    enqueued += 1;
                }
                pending += 1;
            }
        }
        if !states
            .get(slug.as_str())
            .is_some_and(|(_, missing)| *missing)
        {
            analysis_candidates.push(slug.clone());
        }
    }

    // A profile report promises commit/contributor statistics as well as star
    // history. Offer a bounded batch to the durable analysis queue so those
    // fields cannot remain at an unexplained zero forever. This is
    // Postgres-only on the request path; cloning and author enrichment happen
    // asynchronously in the existing worker pool.
    if enqueue {
        if let Some(user_id) = user_id {
            for repo in &analysis_candidates {
                if let Err(error) = repo_analysis::enqueue_prioritized(
                    db,
                    repo,
                    repo_analysis::INTERACTIVE_PRIORITY,
                    Some(user_id),
                )
                .await
                {
                    tracing::warn!(login, %repo, %error, "interactive profile analysis enqueue failed");
                }
            }
        } else if let Err(error) =
            repo_analysis::enqueue_many(db, &analysis_candidates, MAX_ANALYSIS_ENQUEUES_PER_BUILD)
                .await
        {
            tracing::warn!(login, %error, "profile repo-analysis enqueue failed");
        }
    }

    let per_repo = load_day_deltas_by_repo(db, &included).await?;
    let merged = merge_day_deltas(&per_repo);
    let (series, total_stars) = deltas_to_series(&merged);
    let repos_analyzed = if analysis_candidates.is_empty() {
        0
    } else {
        // Same definition of "analyzed" as the profile card
        // (`api.rs::load_user_card_data`) and the enqueue-freshness
        // predicate (`repo_analysis::ANALYSIS_IS_CURRENT_SQL`): the walk
        // ran, under the current revision, and reached the head it
        // observed. Author enrichment is not part of it — see
        // `repo_analysis::sweep_author_enrichment`.
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM repo_history history \
             WHERE history.repo = ANY($1) \
               AND history.last_analyzed_at IS NOT NULL \
               AND history.analysis_revision >= $2 \
               AND history.last_analyzed_sha IS NOT NULL \
               AND history.head_sha = history.last_analyzed_sha \
               AND NOT EXISTS (SELECT 1 FROM repo_analysis_queue active \
                               WHERE active.repo = history.repo \
                                 AND active.status IN ('pending', 'in_progress'))",
        )
        .bind(&analysis_candidates)
        .bind(repo_analysis::CURRENT_ANALYSIS_REVISION)
        .fetch_one(&db.pool)
        .await
        .map_err(anyhow::Error::from)?
        .max(0) as u32
    };
    let repos_analyzing = if analysis_candidates.is_empty() {
        0
    } else {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::BIGINT FROM repo_analysis_queue \
             WHERE repo = ANY($1) AND status IN ('pending', 'in_progress')",
        )
        .bind(&analysis_candidates)
        .fetch_one(&db.pool)
        .await
        .map_err(anyhow::Error::from)?
        .max(0) as u32
    };

    Ok(UserAggregate {
        login,
        repos_included: included.len() as u32,
        repos_pending: pending,
        repos_analyzed,
        repos_analyzing,
        total_stars,
        series,
    })
}

/// Resolve the login's top-repos list: fresh cache hit → serve it; cold or
/// stale → one budget-probed live repos-list call, cached atomically;
/// fetch failure / no budget → stale-but-complete fallback; nothing at all
/// → `Busy`. A 404 login is tombstoned (TTL'd) and surfaces `LoginNotFound`.
async fn resolve_login_repos(
    ctx: &AnalyzerCtx,
    login: &str,
    user_github: Option<&GithubClient>,
) -> Result<Vec<(String, i64)>, AggregateError> {
    let cache = &ctx.cache;
    let meta = cache.get_login_repos_meta(login).await?;
    if let Some(m) = &meta
        && m.fresh_within(LOGIN_REPOS_TTL)
    {
        if m.missing {
            return Err(AggregateError::LoginNotFound);
        }
        if m.complete
            && let Some(rows) = cache.get_login_repos(login).await?
        {
            return Ok(rows);
        }
    }

    // Cold or stale. One live repos-list call through the shared
    // rate-limited client — but only when the bucket has headroom
    // (`acquire` would otherwise sleep until the reset, hanging the
    // request into the global timeout) AND the process-wide live-fetch
    // window has budget (the login namespace is infinite, so this is the
    // only bound on attacker-paced synchronous GitHub spend). Either gate
    // failing → fall through to the stale cached list.
    let github = user_github.unwrap_or(ctx.github.as_ref());
    let live_allowed = if !github.has_budget().await {
        tracing::warn!(
            login,
            "no GitHub budget headroom for repos-list; falling back to cached list"
        );
        false
    } else if user_github.is_none() && !try_spend_live_list_fetch() {
        tracing::warn!(
            login,
            "live repos-list window budget exhausted; falling back to cached list"
        );
        false
    } else {
        true
    };
    if live_allowed {
        match github.user_repos(login).await {
            Ok(Some(items)) => {
                let top = top_repos_by_stars(&items, MAX_AGGREGATE_REPOS);
                // Best-effort cache write: we already hold the data, a
                // write failure shouldn't fail the request.
                if let Err(e) = cache.put_login_repos(login, &top).await {
                    tracing::warn!(login, error = %e, "put_login_repos failed");
                }
                return Ok(top);
            }
            Ok(None) => {
                if let Err(e) = cache.mark_login_missing(login).await {
                    tracing::warn!(login, error = %e, "mark_login_missing failed");
                }
                return Err(AggregateError::LoginNotFound);
            }
            Err(e) => {
                tracing::warn!(
                    login,
                    error = %e,
                    "login repos-list fetch failed; falling back to cached list"
                );
            }
        }
    }

    // Stale-but-complete beats an error (same degrade policy as usage.rs).
    if let Some(rows) = cache.get_login_repos(login).await? {
        return Ok(rows);
    }
    // A stale tombstone we couldn't re-verify is still our best knowledge.
    if meta.as_ref().is_some_and(|m| m.missing) {
        return Err(AggregateError::LoginNotFound);
    }
    Err(AggregateError::Busy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn item(full_name: &str, stars: i64) -> RepoListItem {
        RepoListItem {
            full_name: full_name.to_string(),
            stargazers_count: stars,
            fork: false,
            private: false,
        }
    }

    #[test]
    fn login_accepts_github_shaped_names() {
        assert!(is_valid_login("torvalds"));
        assert!(is_valid_login("rust-lang"));
        assert!(is_valid_login("a"));
        assert!(is_valid_login("user123"));
        // 39 chars — GitHub's max — is accepted.
        assert!(is_valid_login(&"a".repeat(39)));
    }

    #[test]
    fn login_rejects_traversal_and_separators() {
        assert!(!is_valid_login(""));
        assert!(!is_valid_login("."));
        assert!(!is_valid_login(".."));
        assert!(!is_valid_login("a/b"));
        assert!(!is_valid_login("../etc"));
        assert!(!is_valid_login("a b"));
        assert!(!is_valid_login("a%2Fb")); // percent not in charset
        assert!(!is_valid_login("a?x=1"));
        assert!(!is_valid_login("héllo")); // non-ASCII
        assert!(!is_valid_login("\0"));
        // Over GitHub's 39-char cap.
        assert!(!is_valid_login(&"a".repeat(40)));
    }

    #[test]
    fn window_spends_until_budget_then_refuses() {
        let mut s = (0i64, 0u32);
        // Budget 30, cost 10 → exactly three spends per window. All
        // timestamps fall inside the same [60, 120) window.
        assert!(window_try_spend(&mut s, 60, 60, 10, 30));
        assert!(window_try_spend(&mut s, 90, 60, 10, 30));
        assert!(window_try_spend(&mut s, 110, 60, 10, 30));
        // Fourth debit would exceed the budget → refused, spend unchanged.
        assert!(!window_try_spend(&mut s, 119, 60, 10, 30));
        assert_eq!(s.1, 30);
    }

    #[test]
    fn window_resets_on_boundary() {
        let mut s = (0i64, 0u32);
        // Exhaust window [60, 120).
        for t in [60, 70, 80] {
            assert!(window_try_spend(&mut s, t, 60, 10, 30));
        }
        assert!(!window_try_spend(&mut s, 119, 60, 10, 30));
        // Crossing into the next window resets the spend.
        assert!(window_try_spend(&mut s, 120, 60, 10, 30));
        assert_eq!(s, (2, 10));
    }

    #[test]
    fn window_cost_over_budget_never_spends() {
        let mut s = (0i64, 0u32);
        assert!(!window_try_spend(&mut s, 0, 60, 50, 30));
        assert_eq!(s.1, 0);
    }

    #[test]
    fn window_handles_negative_clock() {
        // div_euclid keeps pre-epoch (or skewed) clocks in a stable
        // window instead of panicking or aliasing window 0.
        let mut s = (0i64, 0u32);
        assert!(window_try_spend(&mut s, -30, 60, 10, 30));
        assert_eq!(s.0, -1);
    }

    #[test]
    fn live_fetch_constants_leave_worker_headroom() {
        // The worst-case hourly burn (every fetch a max-page org) must
        // stay well under half the shared 5k/hr PAT budget so background
        // workers never starve behind request-path spend.
        let per_hour = (3_600 / LIVE_LIST_WINDOW_SECS) as u32 * LIVE_LIST_CALLS_PER_WINDOW;
        assert!(
            per_hour <= 2_500,
            "live-list worst case {per_hour}/hr too high"
        );
        // The flat cost matches github.rs's REPO_LIST_MAX_PAGES cap.
        assert_eq!(LIVE_LIST_FETCH_COST, 10);
    }

    #[test]
    fn top_repos_sorts_desc_caps_and_lowercases() {
        let items = vec![
            item("Org/Small", 5),
            item("Org/Big", 500),
            item("Org/Mid", 50),
        ];
        let top = top_repos_by_stars(&items, 2);
        assert_eq!(
            top,
            vec![("org/big".to_string(), 500), ("org/mid".to_string(), 50)]
        );
    }

    #[test]
    fn top_repos_tie_breaks_by_slug_deterministically() {
        let items = vec![item("o/bbb", 10), item("o/aaa", 10), item("o/ccc", 10)];
        let top = top_repos_by_stars(&items, 3);
        let slugs: Vec<&str> = top.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(slugs, vec!["o/aaa", "o/bbb", "o/ccc"]);
        // Pure function: same input → same output.
        assert_eq!(top, top_repos_by_stars(&items, 3));
    }

    #[test]
    fn top_repos_drops_malformed_and_dedups() {
        let items = vec![
            item("no-slash", 1000), // malformed → dropped
            item("o/../evil", 999), // traversal segment → dropped
            item("o/a/b", 998),     // split_once keeps "a/b" → invalid → dropped
            item("O/Repo", 7),      // survives, lowercased
            item("o/repo", 3),      // dup after lowercasing → dropped
            item("o/negative", -5), // negative count clamps to 0
        ];
        let top = top_repos_by_stars(&items, 10);
        assert_eq!(
            top,
            vec![("o/repo".to_string(), 7), ("o/negative".to_string(), 0)]
        );
    }

    #[test]
    fn top_repos_empty_input_is_empty() {
        assert!(top_repos_by_stars(&[], 50).is_empty());
    }

    #[test]
    fn merge_overlapping_days_sum() {
        let a = vec![(d("2020-01-01"), 3), (d("2020-01-05"), 2)];
        let b = vec![(d("2020-01-01"), 4), (d("2020-01-03"), 1)];
        let merged = merge_day_deltas(&[a, b]);
        assert_eq!(
            merged,
            vec![
                (d("2020-01-01"), 7), // 3 + 4 overlap
                (d("2020-01-03"), 1),
                (d("2020-01-05"), 2),
            ]
        );
    }

    #[test]
    fn merge_single_repo_passes_through_sorted() {
        // Even out-of-order input from a single repo comes out sorted.
        let a = vec![(d("2020-02-01"), 2), (d("2020-01-01"), 1)];
        let merged = merge_day_deltas(&[a]);
        assert_eq!(merged, vec![(d("2020-01-01"), 1), (d("2020-02-01"), 2)]);
    }

    #[test]
    fn merge_empty_inputs() {
        assert!(merge_day_deltas(&[]).is_empty());
        // Empty repos among non-empty contribute nothing.
        let merged = merge_day_deltas(&[vec![], vec![(d("2020-01-01"), 5)], vec![]]);
        assert_eq!(merged, vec![(d("2020-01-01"), 5)]);
    }

    #[test]
    fn merge_clamps_negative_deltas() {
        let merged = merge_day_deltas(&[vec![(d("2020-01-01"), -3), (d("2020-01-02"), 2)]]);
        assert_eq!(merged, vec![(d("2020-01-01"), 0), (d("2020-01-02"), 2)]);
    }

    #[test]
    fn merge_is_deterministic() {
        let a = vec![(d("2020-01-02"), 1), (d("2020-01-01"), 9)];
        let b = vec![(d("2020-01-01"), 1)];
        let once = merge_day_deltas(&[a.clone(), b.clone()]);
        let twice = merge_day_deltas(&[a, b]);
        assert_eq!(once, twice);
    }

    #[test]
    fn deltas_accumulate_into_cumulative_points() {
        let deltas = vec![
            (d("2020-01-01"), 3),
            (d("2020-01-03"), 2),
            (d("2020-02-01"), 5),
        ];
        let (series, total) = deltas_to_series(&deltas);
        assert_eq!(total, 10);
        let stars: Vec<u32> = series.iter().map(|p| p.stars).collect();
        assert_eq!(stars, vec![3, 5, 10]);
        // Points land at midnight UTC of their day — deterministic.
        assert_eq!(series[0].at.to_rfc3339(), "2020-01-01T00:00:00+00:00");
    }

    #[test]
    fn deltas_to_series_empty() {
        let (series, total) = deltas_to_series(&[]);
        assert!(series.is_empty());
        assert_eq!(total, 0);
    }

    #[test]
    fn deltas_total_stays_exact_past_u32_saturation() {
        let deltas = vec![(d("2020-01-01"), u32::MAX as i64), (d("2020-01-02"), 10)];
        let (series, total) = deltas_to_series(&deltas);
        // The u64 total is exact; the per-point u32 saturates.
        assert_eq!(total, u32::MAX as u64 + 10);
        assert_eq!(series[1].stars, u32::MAX);
    }

    #[test]
    fn aggregate_of_two_repos_matches_hand_sum() {
        // repo A: 1 star on day 1, 1 on day 3. repo B: 2 stars on day 2.
        let a = vec![(d("2021-01-01"), 1), (d("2021-01-03"), 1)];
        let b = vec![(d("2021-01-02"), 2)];
        let (series, total) = deltas_to_series(&merge_day_deltas(&[a, b]));
        assert_eq!(total, 4);
        let got: Vec<(String, u32)> = series
            .iter()
            .map(|p| (p.at.date_naive().to_string(), p.stars))
            .collect();
        assert_eq!(
            got,
            vec![
                ("2021-01-01".to_string(), 1),
                ("2021-01-02".to_string(), 3),
                ("2021-01-03".to_string(), 4),
            ]
        );
    }

    #[test]
    fn user_aggregate_json_shape() {
        let (series, total) = deltas_to_series(&[(d("2020-01-01"), 3), (d("2020-01-02"), 1)]);
        let agg = UserAggregate {
            login: "octocat".into(),
            repos_included: 2,
            repos_pending: 1,
            repos_analyzed: 1,
            repos_analyzing: 1,
            total_stars: total,
            series,
        };
        let v = agg.to_json();
        assert_eq!(v["login"], "octocat");
        assert_eq!(v["repos_included"], 2);
        assert_eq!(v["repos_pending"], 1);
        assert_eq!(v["repos_analyzed"], 1);
        assert_eq!(v["repos_analyzing"], 1);
        assert_eq!(v["total_stars"], 4);
        // history entries are {date, stars} — Point's serde rename.
        assert_eq!(v["history"][0]["date"], "2020-01-01T00:00:00Z");
        assert_eq!(v["history"][0]["stars"], 3);
        assert_eq!(v["history"][1]["stars"], 4);
        assert!(v["history"][0].get("at").is_none());
        // Exactly the contract keys, nothing extra.
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 7);
    }

    #[test]
    fn user_aggregate_history_is_downsampled() {
        // 1000 days of deltas → history capped at MAX_HISTORY_POINTS.
        let deltas: Vec<(NaiveDate, i64)> = (0..1000)
            .map(|i| (d("2018-01-01") + chrono::Duration::days(i), 1))
            .collect();
        let (series, total) = deltas_to_series(&deltas);
        assert_eq!(total, 1000);
        let agg = UserAggregate {
            login: "big".into(),
            repos_included: 1,
            repos_pending: 0,
            repos_analyzed: 2,
            repos_analyzing: 0,
            total_stars: total,
            series,
        };
        let v = agg.to_json();
        let hist = v["history"].as_array().unwrap();
        assert!(hist.len() <= MAX_HISTORY_POINTS);
        // First and last points survive downsampling exactly.
        assert_eq!(hist.first().unwrap()["stars"], 1);
        assert_eq!(hist.last().unwrap()["stars"], 1000);
    }
}
