use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgConnection, Row};

use crate::db::Db;
use crate::github::RepoMetadata;

/// Cache repository over the Postgres schema. The star-history pipeline
/// caches one row per stargazer per repo (`repo_stargazers`) plus a small
/// per-repo metadata row (`repos`). All multi-row writers use transactions
/// so a failure mid-pagination cannot leave a `*_complete` flag set with
/// missing rows. Readers never return data unless the corresponding
/// `*_complete` flag is true — that is the invariant the rest of the
/// system trusts.
///
/// Timestamp columns are `TIMESTAMPTZ`. sqlx auto-decodes them to
/// `DateTime<Utc>` so the read path no longer hand-parses RFC3339 strings.
#[derive(Clone)]
pub struct Cache {
    db: Db,
}

pub type StargazerEvent = (i64, DateTime<Utc>);

/// The exact current-membership snapshot from GitHub's stargazer list.
/// Frozen since GitHub restricted that endpoint on 2026-07-20.
pub const HISTORY_SOURCE_EXACT: &str = "github_api";
/// Approximate public star activity reconstructed from the GH Archive corpus.
pub const HISTORY_SOURCE_ARCHIVE: &str = "gh_archive";
/// The exact segment through `repos.history_splice_at`, then archive activity
/// strictly after it. Approximate in its tail and exact everywhere else, which
/// is why it is a third source rather than a flavour of either.
pub const HISTORY_SOURCE_SPLICED: &str = "spliced";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ArchiveStarEvent {
    pub source_event_id: Option<String>,
    pub starred_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ArchiveBackfillState {
    pub github_id: Option<i64>,
    pub cursor: Option<NaiveDate>,
    pub complete: bool,
    pub authoritative_total: Option<i64>,
    pub exact_history_complete: bool,
    /// `metadata_fetched_at IS NULL`. Every public read surface gates on
    /// that timestamp, so a repo with complete history but missing
    /// metadata is invisible until a metadata fetch heals it — the archive
    /// coordinator must not settle such a job without writing metadata.
    pub metadata_missing: bool,
}

type ArchiveBackfillRow = (
    Option<i64>,
    Option<NaiveDate>,
    bool,
    Option<i64>,
    bool,
    bool,
);

async fn upsert_stargazer_events(
    conn: &mut PgConnection,
    repo: &str,
    items: &[StargazerEvent],
) -> Result<()> {
    let positions: Vec<i64> = items.iter().map(|(position, _)| *position).collect();
    let timestamps: Vec<DateTime<Utc>> = items.iter().map(|(_, at)| *at).collect();
    sqlx::query(
        "INSERT INTO repo_stargazers (repo, position, starred_at) \
         SELECT $1, events.position, events.starred_at \
         FROM UNNEST($2::BIGINT[], $3::TIMESTAMPTZ[]) \
              AS events(position, starred_at) \
         ON CONFLICT (repo, position) DO UPDATE \
         SET starred_at = EXCLUDED.starred_at",
    )
    .bind(repo)
    .bind(positions)
    .bind(timestamps)
    .execute(conn)
    .await?;
    Ok(())
}

async fn upsert_archive_events(
    conn: &mut PgConnection,
    repo: &str,
    start_position: i64,
    items: &[ArchiveStarEvent],
) -> Result<()> {
    let positions: Vec<i64> = (0..items.len())
        .map(|index| start_position.saturating_add(index as i64))
        .collect();
    let source_event_ids: Vec<Option<String>> = items
        .iter()
        .map(|item| item.source_event_id.clone())
        .collect();
    let timestamps: Vec<DateTime<Utc>> = items.iter().map(|item| item.starred_at).collect();
    sqlx::query(
        "INSERT INTO repo_star_arrivals (repo, position, source_event_id, starred_at) \
         SELECT $1, events.position, events.source_event_id, events.starred_at \
         FROM UNNEST($2::BIGINT[], $3::TEXT[], $4::TIMESTAMPTZ[]) \
              AS events(position, source_event_id, starred_at) \
         ON CONFLICT DO NOTHING",
    )
    .bind(repo)
    .bind(positions)
    .bind(source_event_ids)
    .bind(timestamps)
    .execute(conn)
    .await?;
    Ok(())
}

/// The single-row `repos` fields the `/analyze` hot path reads, fetched in
/// one query (see [`Cache::get_repo_summary`]).
#[derive(Debug, Clone, Default)]
pub struct RepoSummary {
    pub missing: bool,
    pub github_id: Option<i64>,
    pub stargazers_complete: bool,
    pub stargazers_fetched_at: Option<DateTime<Utc>>,
    pub metadata_fetched_at: Option<DateTime<Utc>>,
    pub star_count: Option<i64>,
    pub history_source: Option<String>,
    pub history_observed_count: Option<i64>,
    pub history_coverage_start: Option<DateTime<Utc>>,
    pub history_coverage_end: Option<DateTime<Utc>>,
    /// Where a spliced series changes method. Read here rather than derived,
    /// because it is the one provenance fact the series itself cannot show:
    /// the curve is continuous across the join and nothing in the drawing says
    /// the points on either side were measured differently.
    ///
    /// Only meaningful while `history_source` is `spliced` — a repository whose
    /// source moves back to an exact snapshot keeps the old boundary in the
    /// column, so every reader must gate on the source (see
    /// [`Self::history_splice_at_if_spliced`]).
    pub history_splice_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub archived: bool,
    pub pushed_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub default_branch: Option<String>,
    pub license_spdx: Option<String>,
    pub topics: Vec<String>,
    pub has_issues: bool,
    pub has_discussions: bool,
    pub has_pages: bool,
    pub is_template: bool,
    pub subscribers_count: i64,
    /// GitHub's upstream count includes open pull requests.
    pub open_issues_count: i64,
    pub view_count: i64,
}

impl<'row> sqlx::FromRow<'row, sqlx::postgres::PgRow> for RepoSummary {
    fn from_row(row: &'row sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            missing: row.try_get("missing")?,
            github_id: row.try_get("github_id")?,
            stargazers_complete: row.try_get("stargazers_complete")?,
            stargazers_fetched_at: row.try_get("stargazers_fetched_at")?,
            metadata_fetched_at: row.try_get("metadata_fetched_at")?,
            star_count: row.try_get("star_count")?,
            history_source: row.try_get("history_source")?,
            history_observed_count: row.try_get("history_observed_count")?,
            history_coverage_start: row.try_get("history_coverage_start")?,
            history_coverage_end: row.try_get("history_coverage_end")?,
            history_splice_at: row.try_get("history_splice_at")?,
            created_at: row.try_get("created_at")?,
            archived: row.try_get("archived")?,
            pushed_at: row.try_get("pushed_at")?,
            updated_at: row.try_get("updated_at")?,
            default_branch: row.try_get("default_branch")?,
            license_spdx: row.try_get("license_spdx")?,
            topics: row.try_get("topics")?,
            has_issues: row.try_get("has_issues")?,
            has_discussions: row.try_get("has_discussions")?,
            has_pages: row.try_get("has_pages")?,
            is_template: row.try_get("is_template")?,
            subscribers_count: row.try_get("subscribers_count")?,
            open_issues_count: row.try_get("open_issues_count")?,
            view_count: row.try_get("view_count")?,
        })
    }
}

impl RepoSummary {
    /// Mirror of [`Cache::repo_stargazers_fresh_within`] over the
    /// already-loaded summary. Exact GitHub API snapshots are immutable
    /// once complete; only approximate GH Archive histories age out.
    ///
    /// A spliced series is deliberately NOT permanently fresh. Its exact
    /// segment is immutable, but its archive tail is the part that has to keep
    /// moving — treating the whole thing as frozen because half of it is exact
    /// would recreate the stall the splice exists to end.
    pub fn stargazers_fresh_within(&self, ttl: chrono::Duration) -> bool {
        if self.stargazers_complete && self.history_source.as_deref() == Some(HISTORY_SOURCE_EXACT)
        {
            return true;
        }
        match (self.stargazers_complete, self.stargazers_fetched_at) {
            (true, Some(fetched_at)) => Utc::now() - fetched_at < ttl,
            _ => false,
        }
    }

    /// Whether any part of the plotted series is GH Archive activity.
    ///
    /// True for a spliced series as well as a purely archive-backed one: its
    /// tail counts public star actions, which omit unstars and (since the
    /// upstream corpus lost most of its WatchEvent volume in 2026) undercount
    /// what it does include. A series is approximate if any part of it is.
    pub fn history_is_approximate(&self) -> bool {
        matches!(
            self.history_source.as_deref(),
            Some(HISTORY_SOURCE_ARCHIVE | HISTORY_SOURCE_SPLICED)
        )
    }

    /// The splice boundary, but only for a series that actually has one.
    ///
    /// The column outlives the source. `put_repo_stargazers` and
    /// `finish_repo_stargazers_partial` set `history_source = 'github_api'`
    /// without clearing the boundary, so a spliced repository that later takes
    /// an exact write keeps a stale instant in the row. Publishing it would
    /// date a join that no longer exists — the read surfaces ask this instead
    /// of the field.
    pub fn history_splice_at_if_spliced(&self) -> Option<DateTime<Utc>> {
        self.history_splice_at
            .filter(|_| self.history_source.as_deref() == Some(HISTORY_SOURCE_SPLICED))
    }
}

/// Meta row for a login's cached public-repos list (the org/user aggregate
/// chart's `login → repos` mapping): fetch timestamp (drives the caller's
/// TTL), completeness flag, and the 404 tombstone. See
/// [`Cache::get_login_repos_meta`].
#[derive(Debug, Clone)]
pub struct LoginReposMeta {
    pub fetched_at: Option<DateTime<Utc>>,
    pub complete: bool,
    pub missing: bool,
    /// GitHub's account kind for this login, as resolved at fetch time.
    /// `None` on rows written before the kind was recorded, or on a
    /// tombstone.
    pub account_kind: Option<crate::github::AccountKind>,
    /// The account's public-repository count at fetch time. The cached list
    /// is capped, so this is what tells a reader how much of the account
    /// the cap actually covers.
    pub public_repos: Option<i64>,
    /// A deeper repos-list walk would still improve this list: the fetch
    /// stopped at a page budget below the full cap. False once no further
    /// walk can add anything, including for an account larger than the cap
    /// itself — coverage is reported from `public_repos`, not from here.
    pub list_truncated: bool,
}

/// What a repos-list fetch learned about the account itself, written
/// alongside the list in the same transaction so the kind and the coverage
/// numbers can never describe a different fetch than the rows do.
#[derive(Debug, Clone, Copy)]
pub struct LoginListFacts {
    pub kind: crate::github::AccountKind,
    pub public_repos: Option<i64>,
    pub truncated: bool,
}

/// A privacy-safe row for the landing-page activity pulse. This is derived
/// entirely from repository counters already stored in Postgres; no viewer,
/// account, or request metadata leaves the backend.
#[derive(Debug, Clone)]
pub struct PlatformActivity {
    pub repo: String,
    pub stars: i64,
    pub views: i64,
    pub viewed_at: DateTime<Utc>,
    pub history_ready: bool,
    pub analysis_ready: bool,
    pub gained_7d: i64,
    pub gained_30d: i64,
}

type PlatformActivityRow = (String, i64, i64, DateTime<Utc>, bool, bool, i64, i64);

/// Recently-viewed repositories with their 7/30-day star gains.
///
/// The `starred_at >= NOW() - INTERVAL '30 days'` bound inside the LATERAL is
/// not redundant with the per-row FILTERs: without it the subquery reads every
/// star row a repository has ever had, and the planner evaluates it for every
/// candidate row rather than only the ones the LIMIT returns. With the bound
/// (and `idx_repos_last_viewed` ordering the outer scan) both become bounded
/// index range scans. `NOW()` is stable within a statement, so rows outside
/// the window contribute 0 to both counters and the result is unchanged.
/// Which repositories the programmatic sitemap publishes. Shared by the page
/// query, the count, and the predicate of `idx_repos_sitemap`, which only
/// serves the query while all three agree.
pub(crate) const SITEMAP_ELIGIBLE_SQL: &str =
    "history_complete = TRUE AND missing = FALSE AND metadata_fetched_at IS NOT NULL";

/// The sitemap's `lastmod`, and the expression `idx_repos_sitemap` is built on.
pub(crate) const SITEMAP_UPDATED_AT_SQL: &str =
    "GREATEST(archive_fetched_at, stargazers_fetched_at, metadata_fetched_at)";

const PLATFORM_ACTIVITY_SQL: &str = "SELECT r.repo, COALESCE(r.star_count, 0), r.view_count, \
            r.last_viewed_at, r.history_complete, \
            (h.last_analyzed_at IS NOT NULL), \
            COALESCE(g.gained_7d, 0), COALESCE(g.gained_30d, 0) \
     FROM repos r \
     LEFT JOIN repo_history h ON h.repo = r.repo \
     LEFT JOIN LATERAL ( \
         SELECT COUNT(*) FILTER (WHERE starred_at >= NOW() - INTERVAL '7 days')::BIGINT AS gained_7d, \
                COUNT(*) FILTER (WHERE starred_at >= NOW() - INTERVAL '30 days')::BIGINT AS gained_30d \
         FROM active_repo_star_history stars \
         WHERE stars.repo = r.repo \
           AND stars.starred_at >= NOW() - INTERVAL '30 days' \
     ) g ON TRUE \
     WHERE r.last_viewed_at IS NOT NULL AND NOT r.missing \
       AND r.metadata_fetched_at IS NOT NULL \
     ORDER BY r.last_viewed_at DESC, r.repo ASC \
     LIMIT $1";

impl LoginReposMeta {
    /// Whether this meta row was fetched within `ttl`. A row without a
    /// timestamp is never fresh.
    pub fn fresh_within(&self, ttl: chrono::Duration) -> bool {
        self.fetched_at.is_some_and(|t| Utc::now() - t < ttl)
    }
}

impl Cache {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Recently viewed public repositories for the landing page. The result
    /// deliberately contains repository-level activity only and never calls
    /// GitHub on the request path.
    pub async fn list_platform_activity(&self, limit: i64) -> Result<Vec<PlatformActivity>> {
        let rows: Vec<PlatformActivityRow> = sqlx::query_as(PLATFORM_ACTIVITY_SQL)
            .bind(limit.clamp(1, 12))
            .fetch_all(&self.db.pool)
            .await
            .context("list platform activity")?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    repo,
                    stars,
                    views,
                    viewed_at,
                    history_ready,
                    analysis_ready,
                    gained_7d,
                    gained_30d,
                )| PlatformActivity {
                    repo,
                    stars,
                    views,
                    viewed_at,
                    history_ready,
                    analysis_ready,
                    gained_7d,
                    gained_30d,
                },
            )
            .collect())
    }

    /// Underlying database handle. Other modules (auth, webhook,
    /// repo_endpoints) reuse the same pool rather than opening a second
    /// one.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Cached stargazer timestamps for a repo, oldest-first. Returns
    /// `None` unless the fetch previously completed (`stargazers_complete`)
    /// and current metadata has proved that the repository is public. The
    /// caller then triggers a verified re-fetch. Only non-identifying
    /// timestamps leave the cache layer.
    pub async fn get_repo_stargazers(&self, repo: &str) -> Result<Option<Vec<DateTime<Utc>>>> {
        let complete: Option<bool> = sqlx::query_scalar(
            "SELECT history_complete FROM repos \
             WHERE repo = $1 AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?;
        if complete != Some(true) {
            return Ok(None);
        }
        let rows: Vec<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT starred_at FROM active_repo_star_history \
             WHERE repo = $1 ORDER BY position",
        )
        .bind(repo)
        .fetch_all(&self.db.pool)
        .await?;
        Ok(Some(rows))
    }

    /// Complete star history at chart granularity: one cumulative point per
    /// UTC day. This preserves the same metadata/public/completeness gate as
    /// [`Self::get_repo_stargazers`] while keeping common read paths bounded
    /// by repository age rather than star count.
    pub async fn get_repo_star_series(
        &self,
        repo: &str,
    ) -> Result<Option<Vec<crate::chart::Point>>> {
        let complete: Option<bool> = sqlx::query_scalar(
            "SELECT history_complete FROM repos \
             WHERE repo = $1 AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?;
        if complete != Some(true) {
            return Ok(None);
        }
        let deltas = crate::export::load_day_deltas(&self.db, repo).await?;
        Ok(Some(crate::export::cumulative_points(&deltas)))
    }

    /// Atomically replace this repo's stargazer set and mark complete.
    /// Single transaction — the visible state is always either "old data,
    /// complete=false" or "new data, complete=true", never a mix.
    pub async fn put_repo_stargazers(&self, repo: &str, items: &[StargazerEvent]) -> Result<()> {
        let mut tx = self.db.pool.begin().await?;
        sqlx::query("INSERT INTO repos (repo) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM repo_stargazers WHERE repo = $1")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        upsert_stargazer_events(&mut tx, repo, items).await?;
        let now = Utc::now();
        // A successful complete write is positive evidence the repo exists,
        // so clear any stale `missing` tombstone (a repo can 404 transiently
        // or be un-deleted). Without this the 404 tombstone was one-way.
        sqlx::query(
            "UPDATE repos SET stargazers_fetched_at = $1, stargazers_complete = TRUE, \
                history_complete = TRUE, \
                star_count = $2, history_source = 'github_api', \
                history_observed_count = $2, history_coverage_start = $3, \
                history_coverage_end = $4, missing = FALSE \
             WHERE repo = $5",
        )
        .bind(now)
        .bind(items.len() as i64)
        .bind(items.first().map(|(_, at)| *at))
        .bind(items.last().map(|(_, at)| *at))
        .bind(repo)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// All the single-row `repos` fields the non-blocking analyze path
    /// needs, fetched in ONE query instead of the five separate single-row
    /// SELECTs the path used to issue (`repo_is_missing`,
    /// `repo_stargazers_fresh_within`, `get_repo_star_count`,
    /// `get_repo_created_at`, and the `get_repo_view_count` inside
    /// `enqueue_fetch`). They all hit the same `repos` row, so folding them
    /// removes four round-trips per `/analyze` request — which matters under
    /// the browser-extension's per-page-view volume. The actual stargazer
    /// rows still come from a separate `repo_stargazers` read. Returns
    /// `None` when the repo row doesn't exist (truly cold).
    pub async fn get_repo_summary(&self, repo: &str) -> Result<Option<RepoSummary>> {
        let row = sqlx::query_as::<_, RepoSummary>(
            "SELECT missing, github_id, history_complete AS stargazers_complete, \
                    COALESCE(archive_fetched_at, stargazers_fetched_at) \
                        AS stargazers_fetched_at, \
                    metadata_fetched_at, star_count, history_source, \
                    history_observed_count, history_coverage_start, history_coverage_end, \
                    history_splice_at, \
                    created_at, archived, pushed_at, updated_at, default_branch, \
                    license_spdx, topics, has_issues, has_discussions, has_pages, \
                    is_template, subscribers_count, open_issues_count, view_count \
             FROM repos WHERE repo = $1",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row)
    }

    /// Whether this repo's stargazer set is complete (the read-side
    /// completeness flag). Cheap single-column read — used by the
    /// non-blocking analyze path to decide between "serve cached history"
    /// and "enqueue + return pending" without loading the rows.
    pub async fn repo_stargazers_complete(&self, repo: &str) -> Result<bool> {
        let complete: Option<bool> = sqlx::query_scalar(
            "SELECT history_complete FROM repos \
             WHERE repo = $1 AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(complete == Some(true))
    }

    /// True iff the history needs no refresh. Exact GitHub API snapshots are
    /// never re-fetched after completion. Approximate GH Archive histories —
    /// including the archive tail of a spliced one — are fresh only within
    /// `ttl` and may be refreshed from later partitions.
    pub async fn repo_stargazers_fresh_within(
        &self,
        repo: &str,
        ttl: chrono::Duration,
    ) -> Result<bool> {
        let row: Option<(bool, Option<DateTime<Utc>>, Option<String>)> = sqlx::query_as(
            "SELECT history_complete, COALESCE(archive_fetched_at, stargazers_fetched_at), \
                    history_source \
             FROM repos WHERE repo = $1 AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?;
        match row {
            Some((true, _, Some(source))) if source == HISTORY_SOURCE_EXACT => Ok(true),
            Some((true, Some(fetched_at), _)) => Ok(Utc::now() - fetched_at < ttl),
            _ => Ok(false),
        }
    }

    /// The cached authoritative star count for a repo. Reads the
    /// denormalized `star_count` column (set whenever the stargazer set is
    /// written or metadata refreshed) so a pending analyze can surface a
    /// best-effort total without loading every stargazer row. Returns
    /// `None` when nothing is cached yet.
    pub async fn get_repo_star_count(&self, repo: &str) -> Result<Option<i64>> {
        let n: Option<i64> = sqlx::query_scalar(
            "SELECT star_count FROM repos \
             WHERE repo = $1 AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?
        .flatten();
        Ok(n)
    }

    /// Read every cached stargazer row for a repo regardless of the
    /// completeness flag, oldest-first. **Not** a public read path — the
    /// completeness invariant still holds for [`get_repo_stargazers`].
    /// The star-fetch worker uses this to seed an incremental refresh: it
    /// needs the previously-committed (complete) set as the base it
    /// appends the new tail onto.
    pub async fn get_repo_stargazers_partial(&self, repo: &str) -> Result<Vec<StargazerEvent>> {
        let rows = sqlx::query(
            "SELECT position, starred_at FROM repo_stargazers \
             WHERE repo = $1 ORDER BY position",
        )
        .bind(repo)
        .fetch_all(&self.db.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let position: i64 = row.try_get("position")?;
            let starred_at: DateTime<Utc> = row.try_get("starred_at")?;
            out.push((position, starred_at));
        }
        Ok(out)
    }

    pub async fn repo_stargazer_row_count(&self, repo: &str) -> Result<i64> {
        let count =
            sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM repo_stargazers WHERE repo = $1")
                .bind(repo)
                .fetch_one(&self.db.pool)
                .await?;
        Ok(count)
    }

    /// Read cached timestamps at or after `since`, regardless of completeness.
    /// The GH Archive refresh path uses this small overlap window to remove
    /// events it already stored without retaining actor identities or event
    /// payloads.
    pub async fn get_repo_stargazers_since(
        &self,
        repo: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>> {
        let rows = sqlx::query_scalar(
            "SELECT starred_at FROM active_repo_star_history \
             WHERE repo = $1 AND starred_at >= $2 ORDER BY starred_at, position",
        )
        .bind(repo)
        .bind(since)
        .fetch_all(&self.db.pool)
        .await?;
        Ok(rows)
    }

    /// Durable state for a historical GH Archive backfill. `cursor` is the
    /// first day that has not yet been committed.
    pub async fn get_archive_backfill_state(
        &self,
        repo: &str,
    ) -> Result<Option<ArchiveBackfillState>> {
        let row: Option<ArchiveBackfillRow> = sqlx::query_as(
            "SELECT github_id, archive_cursor, archive_complete, star_count, \
                        stargazers_complete, metadata_fetched_at IS NULL \
                 FROM repos WHERE repo = $1",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.map(
            |(
                github_id,
                cursor,
                complete,
                authoritative_total,
                exact_history_complete,
                metadata_missing,
            )| {
                ArchiveBackfillState {
                    github_id,
                    cursor,
                    complete,
                    authoritative_total,
                    exact_history_complete,
                    metadata_missing,
                }
            },
        ))
    }

    /// Settle the source of a completed archive backfill, and with it the
    /// splice boundary.
    ///
    /// Three cases, decided from the row's own state inside the writer's
    /// transaction rather than by the caller:
    ///
    ///   * The repository already carries a COMPLETE exact stargazer-list
    ///     snapshot. Replacing it with archive activity is a strict loss — the
    ///     exact curve is non-approximate, and the corpus lost most of its
    ///     WatchEvent volume in 2026, so the replacement would be both
    ///     approximate and badly undercounted over exactly the recent window
    ///     people look at. The exact rows are kept and the archive rows become
    ///     the tail: `spliced`, boundary at the exact segment's last star.
    ///   * No exact segment (or a partial one, which no reader may trust):
    ///     the ordinary cold-repository result, `gh_archive`, unchanged.
    ///   * Mid-backfill (`complete = FALSE`): the source is left exactly as it
    ///     was. That is what lets a repository being migrated keep serving its
    ///     old, complete, exact series until the last window lands — the
    ///     accumulating `repo_star_arrivals` rows are not selected by
    ///     `active_repo_star_history` while the source still says `github_api`.
    ///
    /// `history_complete` follows the same rule for the same reason: flipping
    /// it false mid-migration would blank a repository that is currently
    /// serving a complete series, so it only falls for sources whose rows the
    /// view is already selecting.
    const ARCHIVE_WINDOW_SETTLE_SQL: &str = "UPDATE repos SET archive_cursor = $1, \
            archive_complete = $2, \
            archive_fetched_at = CASE WHEN $2 THEN $3 ELSE archive_fetched_at END, \
            archive_truncated_before = TRUE, \
            history_complete = CASE \
                WHEN $2 THEN TRUE \
                WHEN repos.history_source = 'github_api' THEN repos.history_complete \
                ELSE FALSE END, \
            history_source = CASE \
                WHEN NOT $2 THEN repos.history_source \
                WHEN exact_segment.splice_at IS NOT NULL THEN 'spliced' \
                ELSE 'gh_archive' END, \
            history_splice_at = CASE \
                WHEN NOT $2 THEN repos.history_splice_at \
                WHEN exact_segment.splice_at IS NOT NULL THEN exact_segment.splice_at \
                ELSE NULL END, \
            history_splice_position = CASE \
                WHEN NOT $2 THEN repos.history_splice_position \
                WHEN exact_segment.splice_at IS NOT NULL THEN exact_segment.splice_position \
                ELSE NULL END, \
            missing = FALSE \
         FROM ( \
             SELECT MAX(stars.starred_at) AS splice_at, \
                    MAX(stars.position) AS splice_position \
             FROM repo_stargazers stars \
             JOIN repos exact_repo ON exact_repo.repo = stars.repo \
             WHERE stars.repo = $4 AND exact_repo.stargazers_complete \
         ) AS exact_segment \
         WHERE repos.repo = $4";

    /// Restate a repository's provenance from the series a reader actually
    /// gets. Runs after [`Self::ARCHIVE_WINDOW_SETTLE_SQL`] in the same
    /// transaction, because it reads `active_repo_star_history` and therefore
    /// needs the new `history_source` and boundary to already be in place — a
    /// subquery inside the settle statement would still see the old row.
    ///
    /// Reading the view rather than `repo_star_arrivals` is what keeps these
    /// three figures true for a splice: that table holds the archive's full
    /// history back to 2011, but a spliced series uses only the part after the
    /// boundary. Counting the table would inflate the event count and drag
    /// COVERAGE START back to a date the published series does not begin on.
    const ARCHIVE_WINDOW_PROVENANCE_SQL: &str = "UPDATE repos SET \
            history_observed_count = series.observed, \
            history_coverage_start = series.coverage_start, \
            history_coverage_end = series.coverage_end \
         FROM ( \
             SELECT COUNT(*)::BIGINT AS observed, \
                    MIN(starred_at) AS coverage_start, \
                    MAX(starred_at) AS coverage_end \
             FROM active_repo_star_history WHERE repo = $1 \
         ) AS series \
         WHERE repos.repo = $1";

    /// Commit one fully-fetched BigQuery date window and advance the cursor
    /// in the same transaction. Archive rows stay invisible until `complete`
    /// is true, so readers never observe a half-backfilled series.
    pub async fn commit_archive_backfill_window(
        &self,
        repo: &str,
        window_start: NaiveDate,
        next_cursor: NaiveDate,
        items: &[ArchiveStarEvent],
        complete: bool,
    ) -> Result<i64> {
        let mut tx = self.db.pool.begin().await?;
        sqlx::query("INSERT INTO repos (repo) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT repo FROM repos WHERE repo = $1 FOR UPDATE")
            .bind(repo)
            .fetch_one(&mut *tx)
            .await?;

        let stored_cursor: Option<NaiveDate> =
            sqlx::query_scalar("SELECT archive_cursor FROM repos WHERE repo = $1")
                .bind(repo)
                .fetch_one(&mut *tx)
                .await?;
        if let Some(stored_cursor) = stored_cursor {
            if stored_cursor != window_start {
                anyhow::bail!(
                    "archive cursor changed for {repo}: expected {window_start}, found {stored_cursor}"
                );
            }
        } else {
            sqlx::query("DELETE FROM repo_star_arrivals WHERE repo = $1")
                .bind(repo)
                .execute(&mut *tx)
                .await?;
        }

        let base: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), 0)::BIGINT \
             FROM repo_star_arrivals WHERE repo = $1",
        )
        .bind(repo)
        .fetch_one(&mut *tx)
        .await?;
        upsert_archive_events(&mut tx, repo, base.saturating_add(1), items).await?;

        // Settle the source (and, for an exact segment, the splice boundary)
        // first: the provenance figures below read the view, which selects on
        // exactly the columns this statement writes.
        sqlx::query(Self::ARCHIVE_WINDOW_SETTLE_SQL)
            .bind(next_cursor)
            .bind(complete)
            .bind(Utc::now())
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        if complete {
            sqlx::query(Self::ARCHIVE_WINDOW_PROVENANCE_SQL)
                .bind(repo)
                .execute(&mut *tx)
                .await?;
        }
        // What the caller logs and the coordinator reports as progress: the
        // rows now stored for this repository, not the subset the published
        // series draws from.
        let observed: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM repo_star_arrivals WHERE repo = $1")
                .bind(repo)
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(observed)
    }

    /// Append newly-fetched stargazers to the existing set and mark the
    /// repo complete, atomically. Used by the incremental refresh path:
    /// the worker fetched only the new tail (the rows GitHub added since
    /// the last fetch), so we keep the existing rows and `ON CONFLICT DO
    /// NOTHING` any overlap, then flip `stargazers_complete = TRUE` and
    /// update the count inside the same transaction. Like
    /// [`put_repo_stargazers`], the visible state is never a half-written
    /// mix. `total` is the full count after the append (the worker knows
    /// it; passing it avoids a COUNT(*) round-trip).
    pub async fn append_repo_stargazers(
        &self,
        repo: &str,
        new_items: &[StargazerEvent],
        total: i64,
    ) -> Result<()> {
        let mut tx = self.db.pool.begin().await?;
        sqlx::query("INSERT INTO repos (repo) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        upsert_stargazer_events(&mut tx, repo, new_items).await?;
        let now = Utc::now();
        // Clear any stale `missing` tombstone — a successful append proves
        // the repo is reachable again (see `put_repo_stargazers`).
        sqlx::query(
            "UPDATE repos SET stargazers_fetched_at = $1, stargazers_complete = TRUE, \
                history_complete = TRUE, \
                star_count = $2, history_source = 'github_api', \
                history_observed_count = $2, \
                history_coverage_start = COALESCE(history_coverage_start, \
                    (SELECT MIN(starred_at) FROM repo_stargazers WHERE repo = $3)), \
                history_coverage_end = (SELECT MAX(starred_at) FROM repo_stargazers WHERE repo = $3), \
                missing = FALSE WHERE repo = $3",
        )
        .bind(now)
        .bind(total)
        .bind(repo)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Persist rows from a *capped* (partial) fetch without marking the
    /// repo complete. Honors the caching invariant: a fetch that hit the
    /// per-attempt page cap leaves `stargazers_complete = FALSE` and no
    /// reader ever trusts the half-written set. Rows are upserted so a
    /// retried cursor chunk is idempotent.
    pub async fn put_repo_stargazers_partial(
        &self,
        repo: &str,
        items: &[StargazerEvent],
    ) -> Result<()> {
        let mut tx = self.db.pool.begin().await?;
        sqlx::query(
            "INSERT INTO repos (repo, stargazers_complete, history_complete) \
             VALUES ($1, FALSE, FALSE) \
             ON CONFLICT (repo) DO UPDATE SET \
                stargazers_complete = FALSE, history_complete = FALSE",
        )
        .bind(repo)
        .execute(&mut *tx)
        .await?;
        upsert_stargazer_events(&mut tx, repo, items).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Start a fresh capped backfill. Existing rows are replaced and the
    /// completeness flag stays false in the same transaction, so public
    /// readers can never observe a mixed old/new history.
    pub async fn replace_repo_stargazers_partial(
        &self,
        repo: &str,
        items: &[StargazerEvent],
    ) -> Result<()> {
        let mut tx = self.db.pool.begin().await?;
        sqlx::query(
            "INSERT INTO repos (repo, stargazers_complete, history_complete) \
             VALUES ($1, FALSE, FALSE) \
             ON CONFLICT (repo) DO UPDATE SET \
                stargazers_complete = FALSE, history_complete = FALSE",
        )
        .bind(repo)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM repo_stargazers WHERE repo = $1")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        upsert_stargazer_events(&mut tx, repo, items).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Append the final chunk of a resumable backfill and atomically make
    /// the accumulated set readable. The count is computed in the same
    /// transaction so retries or page overlap cannot inflate `star_count`.
    pub async fn finish_repo_stargazers_partial(
        &self,
        repo: &str,
        items: &[StargazerEvent],
    ) -> Result<i64> {
        let mut tx = self.db.pool.begin().await?;
        sqlx::query("INSERT INTO repos (repo) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        upsert_stargazer_events(&mut tx, repo, items).await?;
        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM repo_stargazers WHERE repo = $1")
                .bind(repo)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "UPDATE repos SET stargazers_fetched_at = $1, stargazers_complete = TRUE, \
             history_complete = TRUE, \
             star_count = $2, history_source = 'github_api', \
             history_observed_count = $2, \
             history_coverage_start = (SELECT MIN(starred_at) FROM repo_stargazers WHERE repo = $3), \
             history_coverage_end = (SELECT MAX(starred_at) FROM repo_stargazers WHERE repo = $3), \
             missing = FALSE WHERE repo = $3",
        )
        .bind(Utc::now())
        .bind(total)
        .bind(repo)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(total)
    }

    /// Best-effort popularity bump: increment `view_count` and stamp
    /// `last_viewed_at`. Called off the request latency path (fire-and-
    /// forget) from `/api/ext/ping` and analyze. Creates the row if the
    /// repo is otherwise unknown so a never-fetched repo still accrues
    /// popularity (which then drives queue priority).
    pub async fn record_repo_view(&self, repo: &str) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO repos (repo, view_count, last_viewed_at) VALUES ($1, 1, $2) \
             ON CONFLICT (repo) DO UPDATE SET \
                view_count = repos.view_count + 1, last_viewed_at = EXCLUDED.last_viewed_at",
        )
        .bind(repo)
        .bind(now)
        .execute(&self.db.pool)
        .await
        .context("record_repo_view")?;
        Ok(())
    }

    /// Current popularity counter for a repo (0 if unknown). Used as the
    /// star-fetch queue priority when a repo is enqueued.
    pub async fn get_repo_view_count(&self, repo: &str) -> Result<i64> {
        let n: Option<i64> = sqlx::query_scalar("SELECT view_count FROM repos WHERE repo = $1")
            .bind(repo)
            .fetch_optional(&self.db.pool)
            .await?;
        Ok(n.unwrap_or(0))
    }

    /// Read the cached repo creation date. Returns `None` if metadata
    /// hasn't been fetched yet — the caller surfaces `created_at: null`
    /// to the UI rather than blocking.
    pub async fn get_repo_created_at(&self, repo: &str) -> Result<Option<DateTime<Utc>>> {
        let row: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT created_at FROM repos WHERE repo = $1")
                .bind(repo)
                .fetch_optional(&self.db.pool)
                .await?
                .flatten();
        Ok(row)
    }

    /// Read the cached repo fork count. Returns `None` if metadata hasn't
    /// been fetched yet — the usage endpoint surfaces `forks: 0` in that
    /// case (the count is best-effort, never blocks the request).
    pub async fn get_repo_forks(&self, repo: &str) -> Result<Option<i64>> {
        let row: Option<i64> = sqlx::query_scalar("SELECT forks_count FROM repos WHERE repo = $1")
            .bind(repo)
            .fetch_optional(&self.db.pool)
            .await?
            .flatten();
        Ok(row)
    }

    /// Persist the complete retained public-metadata snapshot in one
    /// statement. A reader sees either the previous snapshot or the new one;
    /// `metadata_fetched_at` is never stamped before its sibling fields.
    pub async fn put_repo_metadata(&self, repo: &str, metadata: &RepoMetadata) -> Result<()> {
        anyhow::ensure!(
            !metadata.private,
            "refusing to persist private repository metadata"
        );
        let now = Utc::now();
        let github_id = metadata.id.and_then(|value| i64::try_from(value).ok());
        let stars = i64::try_from(metadata.stargazers_count).unwrap_or(i64::MAX);
        let forks = i64::try_from(metadata.forks_count).unwrap_or(i64::MAX);
        let subscribers = i64::try_from(metadata.subscribers_count).unwrap_or(i64::MAX);
        let open_issues = i64::try_from(metadata.open_issues_count).unwrap_or(i64::MAX);
        let license_spdx = metadata
            .license
            .as_ref()
            .and_then(|license| license.spdx_id.as_deref());
        sqlx::query(
            "INSERT INTO repos \
                (repo, github_id, star_count, forks_count, created_at, archived, \
                 pushed_at, updated_at, default_branch, license_spdx, topics, \
                 has_issues, has_discussions, has_pages, is_template, \
                 subscribers_count, open_issues_count, metadata_fetched_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, \
                     $13, $14, $15, $16, $17, $18) \
             ON CONFLICT (repo) DO UPDATE SET \
                github_id = COALESCE(EXCLUDED.github_id, repos.github_id), \
                star_count = EXCLUDED.star_count, \
                forks_count = EXCLUDED.forks_count, \
                created_at = COALESCE(EXCLUDED.created_at, repos.created_at), \
                archived = EXCLUDED.archived, \
                pushed_at = EXCLUDED.pushed_at, \
                updated_at = EXCLUDED.updated_at, \
                default_branch = EXCLUDED.default_branch, \
                license_spdx = EXCLUDED.license_spdx, \
                topics = EXCLUDED.topics, \
                has_issues = EXCLUDED.has_issues, \
                has_discussions = EXCLUDED.has_discussions, \
                has_pages = EXCLUDED.has_pages, \
                is_template = EXCLUDED.is_template, \
                subscribers_count = EXCLUDED.subscribers_count, \
                open_issues_count = EXCLUDED.open_issues_count, \
                metadata_fetched_at = EXCLUDED.metadata_fetched_at, \
                missing = FALSE",
        )
        .bind(repo)
        .bind(github_id)
        .bind(stars)
        .bind(forks)
        .bind(metadata.created_at)
        .bind(metadata.archived)
        .bind(metadata.pushed_at)
        .bind(metadata.updated_at)
        .bind(metadata.default_branch.as_deref())
        .bind(license_spdx)
        .bind(&metadata.topics)
        .bind(metadata.has_issues)
        .bind(metadata.has_discussions)
        .bind(metadata.has_pages)
        .bind(metadata.is_template)
        .bind(subscribers)
        .bind(open_issues)
        .bind(now)
        .execute(&self.db.pool)
        .await
        .context("put_repo_metadata")?;
        Ok(())
    }

    /// Read a cached external-usage blob if it exists and was fetched
    /// within `ttl`. Returns the stored (already-normalized) JSON string
    /// plus its age-freshness flag. `None` means "no usable row" — the
    /// caller should fetch. A stale row (older than `ttl`) is returned via
    /// [`get_usage_any`] so the caller can fall back to it if the refresh
    /// fails.
    pub async fn get_usage_fresh(
        &self,
        source: &str,
        package: &str,
        ttl: chrono::Duration,
    ) -> Result<Option<String>> {
        let row: Option<(String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT body, fetched_at FROM usage_cache WHERE source = $1 AND package = $2",
        )
        .bind(source)
        .bind(package)
        .fetch_optional(&self.db.pool)
        .await?;
        match row {
            Some((body, fetched_at)) if Utc::now() - fetched_at < ttl => Ok(Some(body)),
            _ => Ok(None),
        }
    }

    /// Read a cached external-usage blob regardless of age. Used as the
    /// graceful-degradation fallback when a live refresh errors/times out:
    /// stale data beats a missing source.
    pub async fn get_usage_any(&self, source: &str, package: &str) -> Result<Option<String>> {
        let body: Option<String> =
            sqlx::query_scalar("SELECT body FROM usage_cache WHERE source = $1 AND package = $2")
                .bind(source)
                .bind(package)
                .fetch_optional(&self.db.pool)
                .await?;
        Ok(body)
    }

    /// Upsert a normalized external-usage blob with the current timestamp.
    pub async fn put_usage(&self, source: &str, package: &str, body: &str) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO usage_cache (source, package, body, fetched_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (source, package) DO UPDATE SET \
                body = EXCLUDED.body, fetched_at = EXCLUDED.fetched_at",
        )
        .bind(source)
        .bind(package)
        .bind(body)
        .bind(now)
        .execute(&self.db.pool)
        .await
        .context("put_usage")?;
        Ok(())
    }

    /// Count repos that have real cached star history (the candidates for
    /// the programmatic sitemap). Matches the row set returned by
    /// [`list_sitemap_repos`] so the caller can paginate accurately.
    pub async fn count_sitemap_repos(&self) -> Result<i64> {
        let sql = format!("SELECT COUNT(*) FROM repos WHERE {SITEMAP_ELIGIBLE_SQL}");
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_one(&self.db.pool)
            .await?;
        Ok(n)
    }

    /// A page of repos with real cached star history, for the programmatic
    /// sitemap. Returns `(slug, updated_at)`, the most recent of the archive /
    /// stargazer / metadata fetch timestamps, ordered by it descending then by
    /// slug so pages are stable and non-overlapping.
    ///
    /// The ordering expression is exactly [`SITEMAP_UPDATED_AT_SQL`] because
    /// `idx_repos_sitemap` is built on it — an expression index only serves an
    /// `ORDER BY` that matches it character for character. This query used to
    /// wrap it in `COALESCE(..., NOW())`, which was both dead code (`GREATEST`
    /// returns NULL only when every argument is NULL, and
    /// `metadata_fetched_at IS NOT NULL` is in the predicate) and the reason it
    /// could not be indexed at all: `NOW()` is not immutable. Every page was a
    /// full scan and sort of every eligible repository, and this endpoint gates
    /// the entire static site build.
    pub async fn list_sitemap_repos(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<(String, DateTime<Utc>)>> {
        let sql = format!(
            "SELECT repo, {SITEMAP_UPDATED_AT_SQL} AS updated_at \
             FROM repos \
             WHERE {SITEMAP_ELIGIBLE_SQL} \
             ORDER BY {SITEMAP_UPDATED_AT_SQL} DESC, repo ASC \
             LIMIT $1 OFFSET $2"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db.pool)
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let repo: String = row.try_get("repo")?;
            let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
            out.push((repo, updated_at));
        }
        Ok(out)
    }

    /// Tombstone a repo GitHub reports as 404 (private/deleted/typo). Sets
    /// `repos.missing = TRUE` so the analyze + ext-ping enqueue paths
    /// short-circuit instead of re-enqueuing a dead repo on every page view.
    /// Idempotent; creates the row if absent.
    pub async fn mark_repo_missing(&self, repo: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO repos (repo, missing) VALUES ($1, TRUE) \
             ON CONFLICT (repo) DO UPDATE SET missing = TRUE",
        )
        .bind(repo)
        .execute(&self.db.pool)
        .await
        .context("mark_repo_missing")?;
        Ok(())
    }

    /// Whether a repo is tombstoned as missing (404). Read by the
    /// non-blocking analyze + ext-ping paths to avoid re-enqueuing a dead
    /// repo. Returns `false` for unknown repos (never fetched).
    pub async fn repo_is_missing(&self, repo: &str) -> Result<bool> {
        let missing: Option<bool> = sqlx::query_scalar("SELECT missing FROM repos WHERE repo = $1")
            .bind(repo)
            .fetch_optional(&self.db.pool)
            .await?;
        Ok(missing == Some(true))
    }

    // Login repository lists

    /// Meta row for a login's cached public-repos list. One read gives the
    /// caller everything the freshness/tombstone decision needs; the rows
    /// themselves come from [`get_login_repos`]. Returns `None` when the
    /// login has never been fetched.
    pub async fn get_login_repos_meta(&self, login: &str) -> Result<Option<LoginReposMeta>> {
        /// `(fetched_at, complete, missing, account_type, public_repos,
        /// list_truncated)` in the order the statement selects them.
        type MetaRow = (
            Option<DateTime<Utc>>,
            bool,
            bool,
            Option<String>,
            Option<i64>,
            bool,
        );
        let row: Option<MetaRow> = sqlx::query_as(
            "SELECT fetched_at, complete, missing, account_type, public_repos, list_truncated \
             FROM login_repo_lists WHERE login = $1",
        )
        .bind(login)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.map(
            |(fetched_at, complete, missing, account_type, public_repos, list_truncated)| {
                LoginReposMeta {
                    fetched_at,
                    complete,
                    missing,
                    account_kind: account_type
                        .as_deref()
                        .map(crate::github::AccountKind::parse),
                    public_repos,
                    list_truncated,
                }
            },
        ))
    }

    /// Cached `(repo_slug, stars)` list for a login, in rank order (stars
    /// descending at fetch time). Returns `None` unless the list is
    /// `complete` — readers never trust partial data, the same invariant
    /// as [`get_repo_stargazers`]. Freshness (TTL) is the caller's concern
    /// via [`get_login_repos_meta`]; a stale-but-complete list is still
    /// readable so callers can degrade to it when a refresh fails.
    pub async fn get_login_repos(&self, login: &str) -> Result<Option<Vec<(String, i64)>>> {
        let complete: Option<bool> =
            sqlx::query_scalar("SELECT complete FROM login_repo_lists WHERE login = $1")
                .bind(login)
                .fetch_optional(&self.db.pool)
                .await?;
        if complete != Some(true) {
            return Ok(None);
        }
        let rows =
            sqlx::query("SELECT repo, stars FROM login_repos WHERE login = $1 ORDER BY rank, repo")
                .bind(login)
                .fetch_all(&self.db.pool)
                .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let repo: String = row.try_get("repo")?;
            let stars: i64 = row.try_get("stars")?;
            out.push((repo, stars));
        }
        Ok(Some(out))
    }

    /// Atomically replace a login's cached repo list and mark it complete.
    /// Single transaction, same contract as [`put_repo_stargazers`]: the
    /// visible state is always either the previous complete list or the new
    /// one — never a mix — and a failure mid-write rolls everything back
    /// (including the `complete = FALSE` reset), so no reader ever sees a
    /// half-replaced set. Also clears any `missing` tombstone (a successful
    /// fetch is positive evidence the login exists). Rank is the input
    /// order (callers pass stars-descending).
    pub async fn put_login_repos(
        &self,
        login: &str,
        repos: &[(String, i64)],
        facts: LoginListFacts,
    ) -> Result<()> {
        let now = Utc::now();
        let mut tx = self.db.pool.begin().await?;
        sqlx::query(
            "INSERT INTO login_repo_lists \
                 (login, fetched_at, complete, missing, \
                  account_type, public_repos, list_truncated) \
             VALUES ($1, $2, FALSE, FALSE, $3, $4, $5) \
             ON CONFLICT (login) DO UPDATE SET \
                fetched_at = EXCLUDED.fetched_at, complete = FALSE, missing = FALSE, \
                account_type = EXCLUDED.account_type, \
                public_repos = EXCLUDED.public_repos, \
                list_truncated = EXCLUDED.list_truncated",
        )
        .bind(login)
        .bind(now)
        .bind(facts.kind.as_str())
        .bind(facts.public_repos)
        .bind(facts.truncated)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM login_repos WHERE login = $1")
            .bind(login)
            .execute(&mut *tx)
            .await?;
        // One statement, not one per repo: a cold organization writes the
        // whole capped list, and per-row round trips dominated that write.
        let slugs: Vec<&str> = repos.iter().map(|(repo, _)| repo.as_str()).collect();
        let stars: Vec<i64> = repos.iter().map(|(_, stars)| *stars).collect();
        let ranks: Vec<i64> = (0..repos.len() as i64).collect();
        sqlx::query(
            "INSERT INTO login_repos (login, repo, stars, rank) \
             SELECT $1, entry.repo, entry.stars, entry.rank \
             FROM UNNEST($2::text[], $3::bigint[], $4::bigint[]) \
                  AS entry(repo, stars, rank)",
        )
        .bind(login)
        .bind(&slugs)
        .bind(&stars)
        .bind(&ranks)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE login_repo_lists SET complete = TRUE WHERE login = $1")
            .bind(login)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Tombstone a login GitHub reports as 404. Clears the cached rows and
    /// the `complete` flag in the same transaction and stamps `fetched_at`,
    /// so the tombstone is honored for one TTL and then re-checked (unlike
    /// the forever-tombstone on `repos.missing` — accounts get renamed and
    /// recreated). Idempotent.
    pub async fn mark_login_missing(&self, login: &str) -> Result<()> {
        let now = Utc::now();
        let mut tx = self.db.pool.begin().await?;
        sqlx::query(
            "INSERT INTO login_repo_lists \
                 (login, fetched_at, complete, missing, \
                  account_type, public_repos, list_truncated) \
             VALUES ($1, $2, FALSE, TRUE, NULL, NULL, FALSE) \
             ON CONFLICT (login) DO UPDATE SET \
                fetched_at = EXCLUDED.fetched_at, complete = FALSE, missing = TRUE, \
                account_type = NULL, public_repos = NULL, list_truncated = FALSE",
        )
        .bind(login)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM login_repos WHERE login = $1")
            .bind(login)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// True iff repo metadata has been fetched within the given TTL.
    /// Lets the metadata-refresh path skip recently-fetched repos.
    pub async fn repo_metadata_fresh_within(
        &self,
        repo: &str,
        ttl: chrono::Duration,
    ) -> Result<bool> {
        let row: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT metadata_fetched_at FROM repos \
             WHERE repo = $1 AND missing = FALSE",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?
        .flatten();
        let Some(parsed) = row else { return Ok(false) };
        Ok(Utc::now() - parsed < ttl)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DateTime, NaiveDate, PLATFORM_ACTIVITY_SQL, SITEMAP_ELIGIBLE_SQL, SITEMAP_UPDATED_AT_SQL,
        Utc,
    };
    // The cache functions require a live Postgres pool, so they're
    // exercised by integration smoke tests rather than unit tests here.
    // What we *can* assert without a DB is the read-side completeness
    // invariant by construction: `get_repo_stargazers` early-returns
    // `None` unless `stargazers_complete` is `true` (see the
    // `complete != Some(true)` guard above), and `put_repo_stargazers`
    // flips that flag only inside the committing transaction. Those two
    // properties are what the rest of the system trusts; any change to
    // them should be paired with a regression test against a test DB.
    #[test]
    fn completeness_guard_is_documented() {
        // Placeholder to keep the module's test surface present and make
        // the invariant visible to future editors. The real coverage is
        // the `complete != Some(true)` guard in `get_repo_stargazers`.
    }

    #[test]
    fn platform_activity_is_postgres_only_and_excludes_tombstones() {
        assert!(PLATFORM_ACTIVITY_SQL.contains("NOT r.missing"));
        assert!(PLATFORM_ACTIVITY_SQL.contains("r.metadata_fetched_at IS NOT NULL"));
        assert!(PLATFORM_ACTIVITY_SQL.contains("r.history_complete"));
        assert!(PLATFORM_ACTIVITY_SQL.contains("h.last_analyzed_at IS NOT NULL"));
        assert!(PLATFORM_ACTIVITY_SQL.contains("ORDER BY r.last_viewed_at DESC"));
        assert!(!PLATFORM_ACTIVITY_SQL.to_ascii_lowercase().contains("actor"));
    }

    /// The activity pulse is polled by every open landing page. Both halves
    /// of its bounded plan are load-bearing: the star-window predicate makes
    /// the per-repo counts index range scans instead of reads of a
    /// repository's entire star history, and `idx_repos_last_viewed` makes
    /// the outer scan ordered so the LATERAL runs only for the returned rows.
    /// Without them this single query reads the whole star corpus per request.
    #[test]
    fn platform_activity_counts_are_bounded_to_the_reported_window() {
        assert!(
            PLATFORM_ACTIVITY_SQL.contains("stars.starred_at >= NOW() - INTERVAL '30 days'"),
            "the LATERAL must bound its scan to the widest window it reports"
        );
    }

    /// An expression index only serves an `ORDER BY` that matches it exactly,
    /// and the two live in different files. If they drift, nothing fails —
    /// the sitemap query silently goes back to scanning and sorting every
    /// eligible repository, and that endpoint gates the whole static build.
    #[test]
    fn sitemap_ordering_matches_its_expression_index() {
        let schema = crate::db::concurrent_index_sql("idx_repos_sitemap")
            .expect("the sitemap index must exist");
        assert!(
            schema.contains(SITEMAP_UPDATED_AT_SQL),
            "index expression must match {SITEMAP_UPDATED_AT_SQL}"
        );
        // `GREATEST` is NULL only when every argument is NULL, and the
        // predicate already excludes that — so no COALESCE fallback is needed,
        // and `NOW()` in one would make the expression unindexable.
        assert!(!SITEMAP_UPDATED_AT_SQL.contains("NOW()"));
        assert!(SITEMAP_ELIGIBLE_SQL.contains("metadata_fetched_at IS NOT NULL"));
    }

    /// The activity query and the index that serves it live in different
    /// files; this is the assertion that keeps them together.
    #[test]
    fn platform_activity_has_its_ordering_index() {
        assert!(crate::db::schema_sql().contains("idx_repos_last_viewed"));
    }

    /// A migration must not blank a repository that is currently serving a
    /// complete exact series.
    ///
    /// The archive rows accumulate over many windows, and the view only starts
    /// selecting them when `history_source` flips at the end. So the source
    /// and the completeness flag must both be left alone while a `github_api`
    /// row is being migrated — writing `history_complete = FALSE` per window
    /// (as the archive path does for its own rows, which the view *is*
    /// selecting) would take a working chart offline for the length of a
    /// full-history backfill.
    #[test]
    fn migrating_an_exact_series_keeps_it_readable_until_the_flip() {
        let sql = super::Cache::ARCHIVE_WINDOW_SETTLE_SQL;
        assert!(
            sql.contains("WHEN repos.history_source = 'github_api' THEN repos.history_complete"),
            "an exact series stays complete across the windows of its migration"
        );
        assert!(
            sql.contains("WHEN NOT $2 THEN repos.history_source"),
            "an unfinished window must not move the source"
        );
    }

    /// The splice is decided from the row, not from the caller.
    ///
    /// A completed archive backfill lands on a repository that either has a
    /// trustworthy exact segment or does not, and only the writer's
    /// transaction can read that without racing. `stargazers_complete` is the
    /// gate: a capped, partial stargazer fetch also leaves rows behind, and
    /// splicing onto those would publish exactly the partial data the
    /// completeness invariant exists to hide.
    #[test]
    fn splice_requires_a_complete_exact_segment() {
        let sql = super::Cache::ARCHIVE_WINDOW_SETTLE_SQL;
        assert!(sql.contains("AND exact_repo.stargazers_complete"));
        assert!(sql.contains("WHEN exact_segment.splice_at IS NOT NULL THEN 'spliced'"));
        assert!(
            sql.contains("ELSE 'gh_archive' END"),
            "a repository that was never on the exact path must behave as it does today"
        );
        // Boundary and offset are written together — the view needs both, and
        // `repos_spliced_needs_boundary` rejects the row without them.
        assert!(sql.contains("history_splice_at = CASE"));
        assert!(sql.contains("history_splice_position = CASE"));
    }

    /// Provenance is restated from the published series, never from the table
    /// underneath it. `repo_star_arrivals` holds the archive's full history
    /// back to 2011; a spliced series publishes only the part after the
    /// boundary, so counting the table would inflate the event count and
    /// report a COVERAGE START the series does not have.
    #[test]
    fn spliced_provenance_is_measured_from_the_published_series() {
        let sql = super::Cache::ARCHIVE_WINDOW_PROVENANCE_SQL;
        assert!(sql.contains("FROM active_repo_star_history WHERE repo = $1"));
        assert!(!sql.contains("repo_star_arrivals"));
        for column in [
            "history_observed_count",
            "history_coverage_start",
            "history_coverage_end",
        ] {
            assert!(sql.contains(column), "{column} must be restated");
        }
    }

    /// The archive tail of a spliced series is the half that has to keep
    /// moving. Treating the whole series as permanently fresh because its
    /// first half is exact would park it exactly where a frozen `github_api`
    /// row is parked today — the stall the splice exists to end.
    #[test]
    fn a_spliced_series_is_never_permanently_fresh() {
        let ttl = chrono::Duration::hours(6);
        let stale = |source: &str| super::RepoSummary {
            stargazers_complete: true,
            stargazers_fetched_at: Some(Utc::now() - chrono::Duration::days(30)),
            history_source: Some(source.to_string()),
            ..Default::default()
        };
        assert!(
            stale(super::HISTORY_SOURCE_EXACT).stargazers_fresh_within(ttl),
            "an exact snapshot is immutable once complete"
        );
        assert!(!stale(super::HISTORY_SOURCE_SPLICED).stargazers_fresh_within(ttl));
        assert!(!stale(super::HISTORY_SOURCE_ARCHIVE).stargazers_fresh_within(ttl));
    }

    /// Approximate is a property of the whole series: a spliced one is exact
    /// up to its boundary and public star actions after it, and any surface
    /// that reports it as exact is over-claiming about its tail.
    #[test]
    fn spliced_series_report_as_approximate() {
        let with = |source: Option<&str>| super::RepoSummary {
            history_source: source.map(str::to_string),
            ..Default::default()
        };
        assert!(with(Some(super::HISTORY_SOURCE_SPLICED)).history_is_approximate());
        assert!(with(Some(super::HISTORY_SOURCE_ARCHIVE)).history_is_approximate());
        assert!(!with(Some(super::HISTORY_SOURCE_EXACT)).history_is_approximate());
        assert!(!with(None).history_is_approximate());
    }

    /// The boundary column outlives the source that gave it meaning: the exact
    /// writers set `history_source = 'github_api'` and leave
    /// `history_splice_at` where it was. Publishing the raw column would date a
    /// join the series no longer has, on the one surface whose entire job is
    /// stating where the method changed.
    #[test]
    fn the_splice_instant_is_published_only_while_the_series_is_spliced() {
        let at = DateTime::<Utc>::from_timestamp(1_784_000_836, 0).expect("valid instant");
        let with = |source: &str| super::RepoSummary {
            history_source: Some(source.to_string()),
            history_splice_at: Some(at),
            ..Default::default()
        };
        assert_eq!(
            with(super::HISTORY_SOURCE_SPLICED).history_splice_at_if_spliced(),
            Some(at)
        );
        assert_eq!(
            with(super::HISTORY_SOURCE_EXACT).history_splice_at_if_spliced(),
            None
        );
        assert_eq!(
            with(super::HISTORY_SOURCE_ARCHIVE).history_splice_at_if_spliced(),
            None
        );
        assert_eq!(
            super::RepoSummary {
                history_source: Some(super::HISTORY_SOURCE_SPLICED.to_string()),
                ..Default::default()
            }
            .history_splice_at_if_spliced(),
            None
        );
    }

    /// End to end against Postgres: an archive backfill landing on a complete
    /// exact snapshot must keep every exact point, append only archive
    /// activity from strictly after the boundary, and leave the view a single
    /// ordered series with no duplicated or lost point where the two meet.
    #[tokio::test]
    async fn archive_backfill_splices_onto_a_frozen_exact_series() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let cache = super::Cache::new(db.clone());
        let repo = format!("gitdebt-splice-test/{}", std::process::id());
        let at = |day: u32, hour: u32| {
            chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 7, day, hour, 0, 0).unwrap()
        };
        let boundary = at(20, 12);

        cleanup_splice_fixture(&db, &repo).await;
        sqlx::query(
            "INSERT INTO repos (repo, github_id, star_count, metadata_fetched_at, \
                stargazers_complete, history_complete, history_source) \
             VALUES ($1, 4242, 3, NOW(), TRUE, TRUE, 'github_api')",
        )
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();
        let exact: Vec<super::StargazerEvent> = vec![(1, at(1, 0)), (2, at(10, 0)), (3, boundary)];
        for (position, starred_at) in &exact {
            sqlx::query(
                "INSERT INTO repo_stargazers (repo, position, starred_at) VALUES ($1, $2, $3)",
            )
            .bind(&repo)
            .bind(position)
            .bind(starred_at)
            .execute(&db.pool)
            .await
            .unwrap();
        }

        // The archive's own view of this repository: it covers the whole
        // history, including the part the exact segment already describes, and
        // one event lands exactly ON the boundary.
        let events: Vec<super::ArchiveStarEvent> =
            [at(2, 0), at(11, 0), boundary, at(21, 9), at(22, 9)]
                .into_iter()
                .enumerate()
                .map(|(index, starred_at)| super::ArchiveStarEvent {
                    source_event_id: Some(format!("splice-{index}")),
                    starred_at,
                })
                .collect();
        let window_start = NaiveDate::from_ymd_opt(2011, 2, 12).unwrap();
        let next_cursor = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        cache
            .commit_archive_backfill_window(&repo, window_start, next_cursor, &events, true)
            .await
            .unwrap();

        /// `(history_source, splice_at, splice_position, observed_count,
        /// coverage_start, coverage_end)` as the statement below selects them.
        type SplicedRow = (
            Option<String>,
            Option<DateTime<Utc>>,
            Option<i64>,
            Option<i64>,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
        );
        let (source, splice_at, splice_position, observed, coverage_start, coverage_end): SplicedRow = sqlx::query_as(
            "SELECT history_source, history_splice_at, history_splice_position, \
                    history_observed_count, history_coverage_start, history_coverage_end \
             FROM repos WHERE repo = $1",
        )
        .bind(&repo)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(source.as_deref(), Some(super::HISTORY_SOURCE_SPLICED));
        assert_eq!(splice_at, Some(boundary));
        assert_eq!(splice_position, Some(3));

        let series: Vec<(i64, DateTime<Utc>)> = sqlx::query_as(
            "SELECT position, starred_at FROM active_repo_star_history \
             WHERE repo = $1 ORDER BY position",
        )
        .bind(&repo)
        .fetch_all(&db.pool)
        .await
        .unwrap();
        // Exact through the boundary, archive strictly after it. The archive's
        // pre-boundary events are dropped (the exact segment already counted
        // those stars) and its on-boundary event is dropped too — that is the
        // one instant both halves could otherwise claim.
        assert_eq!(
            series.iter().map(|(_, at)| *at).collect::<Vec<_>>(),
            vec![at(1, 0), at(10, 0), boundary, at(21, 9), at(22, 9)]
        );
        let positions: Vec<i64> = series.iter().map(|(position, _)| *position).collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "position must stay a single strictly-increasing order: {positions:?}"
        );

        // Provenance describes the published series, not the arrivals table:
        // the archive rows before the boundary are stored but not published.
        assert_eq!(observed, Some(5));
        assert_eq!(coverage_start, Some(at(1, 0)));
        assert_eq!(coverage_end, Some(at(22, 9)));
        assert_eq!(
            cache.get_repo_stargazers(&repo).await.unwrap(),
            Some(series.iter().map(|(_, at)| *at).collect::<Vec<_>>()),
            "the completeness-gated reader serves the spliced series"
        );

        cleanup_splice_fixture(&db, &repo).await;
    }

    async fn cleanup_splice_fixture(db: &crate::db::Db, repo: &str) {
        for statement in [
            "DELETE FROM repo_stargazers WHERE repo = $1",
            "DELETE FROM repo_star_arrivals WHERE repo = $1",
            "DELETE FROM repos WHERE repo = $1",
        ] {
            sqlx::query(statement)
                .bind(repo)
                .execute(&db.pool)
                .await
                .unwrap();
        }
    }
}
