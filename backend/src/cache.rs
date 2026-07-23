use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgConnection, Row};

use crate::db::Db;

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
#[derive(Debug, Clone)]
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
    pub created_at: Option<DateTime<Utc>>,
    pub view_count: i64,
}

type RepoSummaryRow = (
    bool,
    Option<i64>,
    bool,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    i64,
);

impl RepoSummary {
    /// Mirror of [`Cache::repo_stargazers_fresh_within`] over the
    /// already-loaded summary: complete AND fetched within `ttl`.
    pub fn stargazers_fresh_within(&self, ttl: chrono::Duration) -> bool {
        match (self.stargazers_complete, self.stargazers_fetched_at) {
            (true, Some(fetched_at)) => Utc::now() - fetched_at < ttl,
            _ => false,
        }
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

const PLATFORM_ACTIVITY_SQL: &str = "SELECT r.repo, COALESCE(r.star_count, 0), r.view_count, \
            r.last_viewed_at, r.history_complete, \
            (h.last_analyzed_at IS NOT NULL), \
            COALESCE(g.gained_7d, 0), COALESCE(g.gained_30d, 0) \
     FROM repos r \
     LEFT JOIN repo_history h ON h.repo = r.repo \
     LEFT JOIN LATERAL ( \
         SELECT COUNT(*) FILTER (WHERE starred_at >= NOW() - INTERVAL '7 days')::BIGINT AS gained_7d, \
                COUNT(*) FILTER (WHERE starred_at >= NOW() - INTERVAL '30 days')::BIGINT AS gained_30d \
         FROM active_repo_star_history stars WHERE stars.repo = r.repo \
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
        let row: Option<RepoSummaryRow> = sqlx::query_as(
            "SELECT missing, github_id, history_complete, \
                    COALESCE(archive_fetched_at, stargazers_fetched_at), \
                    metadata_fetched_at, star_count, history_source, \
                    history_observed_count, history_coverage_start, history_coverage_end, \
                    created_at, view_count \
             FROM repos WHERE repo = $1",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.map(
            |(
                missing,
                github_id,
                complete,
                stargazers_fetched_at,
                metadata_fetched_at,
                star_count,
                history_source,
                history_observed_count,
                history_coverage_start,
                history_coverage_end,
                created_at,
                view_count,
            )| RepoSummary {
                missing,
                github_id,
                stargazers_complete: complete,
                stargazers_fetched_at,
                metadata_fetched_at,
                star_count,
                history_source,
                history_observed_count,
                history_coverage_start,
                history_coverage_end,
                created_at,
                view_count,
            },
        ))
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

    /// True iff the stargazer set is complete AND was fetched within
    /// `ttl`. Drives the analyze / ping "is it stale?" decision: a fresh
    /// complete repo is served straight from cache; a stale one is
    /// re-enqueued for an incremental refresh.
    pub async fn repo_stargazers_fresh_within(
        &self,
        repo: &str,
        ttl: chrono::Duration,
    ) -> Result<bool> {
        let row: Option<(bool, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT history_complete, COALESCE(archive_fetched_at, stargazers_fetched_at) \
             FROM repos WHERE repo = $1 AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL",
        )
        .bind(repo)
        .fetch_optional(&self.db.pool)
        .await?;
        match row {
            Some((true, Some(fetched_at))) => Ok(Utc::now() - fetched_at < ttl),
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

    /// Atomically replace a repository's star-event timeline with the
    /// complete GH Archive result. `authoritative_total` remains GitHub's
    /// current star count; GH Archive WatchEvents are an event history and
    /// can differ because unstars are not public events and coverage begins
    /// in 2011. Keeping both values avoids presenting the archive event count
    /// as the current GitHub total.
    pub async fn put_repo_stargazers_from_archive(
        &self,
        repo: &str,
        items: &[ArchiveStarEvent],
        authoritative_total: i64,
        coverage_start: DateTime<Utc>,
        coverage_end: DateTime<Utc>,
        truncated_before: bool,
    ) -> Result<()> {
        let mut tx = self.db.pool.begin().await?;
        sqlx::query("INSERT INTO repos (repo) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE repos SET archive_complete = FALSE WHERE repo = $1")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM repo_star_arrivals WHERE repo = $1")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        upsert_archive_events(&mut tx, repo, 1, items).await?;
        sqlx::query(
            "UPDATE repos SET archive_fetched_at = $1, archive_complete = TRUE, \
                history_complete = TRUE, \
                star_count = $2, history_source = 'gh_archive', \
                history_observed_count = $3, history_coverage_start = $4, \
                history_coverage_end = $5, archive_truncated_before = $6, \
                missing = FALSE \
             WHERE repo = $7",
        )
        .bind(Utc::now())
        .bind(authoritative_total.max(0))
        .bind(items.len() as i64)
        .bind(coverage_start)
        .bind(coverage_end)
        .bind(truncated_before)
        .bind(repo)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Append only the novel tail returned by GH Archive and advance the
    /// archive coverage cursor atomically. The worker removes the overlap
    /// multiset before calling this method; positions are assigned here so
    /// retries and multi-worker operation cannot race a caller-computed
    /// offset.
    pub async fn append_repo_stargazers_from_archive(
        &self,
        repo: &str,
        items: &[ArchiveStarEvent],
        authoritative_total: i64,
        coverage_start: DateTime<Utc>,
        coverage_end: DateTime<Utc>,
    ) -> Result<i64> {
        let mut tx = self.db.pool.begin().await?;
        sqlx::query("INSERT INTO repos (repo) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(repo)
            .execute(&mut *tx)
            .await?;
        // Queue dedup already gives one writer per repo. Locking the row here
        // makes the position assignment safe even if an operator manually
        // starts a second process against the same database.
        sqlx::query("SELECT repo FROM repos WHERE repo = $1 FOR UPDATE")
            .bind(repo)
            .fetch_one(&mut *tx)
            .await?;
        let base: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), 0)::BIGINT \
             FROM repo_star_arrivals WHERE repo = $1",
        )
        .bind(repo)
        .fetch_one(&mut *tx)
        .await?;
        upsert_archive_events(&mut tx, repo, base.saturating_add(1), items).await?;
        let observed: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM repo_star_arrivals WHERE repo = $1")
                .bind(repo)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "UPDATE repos SET archive_fetched_at = $1, archive_complete = TRUE, \
                history_complete = TRUE, \
                star_count = $2, \
                history_source = 'gh_archive', \
                history_observed_count = $3, \
                history_coverage_start = LEAST( \
                    COALESCE(history_coverage_start, $4), $4), \
                history_coverage_end = GREATEST( \
                    COALESCE(history_coverage_end, $5), $5), \
                missing = FALSE \
             WHERE repo = $6",
        )
        .bind(Utc::now())
        .bind(authoritative_total.max(0))
        .bind(observed)
        .bind(coverage_start)
        .bind(coverage_end)
        .bind(repo)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(observed)
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

        let observed: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM repo_star_arrivals WHERE repo = $1")
                .bind(repo)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "UPDATE repos SET archive_cursor = $1, archive_complete = $2, \
                archive_fetched_at = CASE WHEN $2 THEN $3 ELSE archive_fetched_at END, \
                archive_truncated_before = TRUE, \
                history_complete = $2, \
                history_source = CASE WHEN $2 THEN 'gh_archive' ELSE history_source END, \
                history_observed_count = CASE WHEN $2 THEN $4 ELSE history_observed_count END, \
                history_coverage_start = CASE WHEN $2 THEN \
                    (SELECT MIN(starred_at) FROM repo_star_arrivals WHERE repo = $5) \
                    ELSE history_coverage_start END, \
                history_coverage_end = CASE WHEN $2 THEN \
                    (SELECT MAX(starred_at) FROM repo_star_arrivals WHERE repo = $5) \
                    ELSE history_coverage_end END, \
                missing = FALSE \
             WHERE repo = $5",
        )
        .bind(next_cursor)
        .bind(complete)
        .bind(Utc::now())
        .bind(observed)
        .bind(repo)
        .execute(&mut *tx)
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

    /// Persist repo-metadata fields we actually use for the star-history +
    /// usage surfaces: the authoritative `stargazers_count` (sanity-checks
    /// our own pagination), `forks_count`, and the repo `created_at`.
    /// Idempotent. COALESCE keeps any previously-known field if the new
    /// fetch left it absent.
    pub async fn put_repo_metadata(
        &self,
        repo: &str,
        github_id: Option<u64>,
        stargazers: u64,
        forks: u64,
        created_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let now = Utc::now();
        let github_id = github_id.and_then(|value| i64::try_from(value).ok());
        sqlx::query(
            "INSERT INTO repos \
                (repo, github_id, star_count, forks_count, created_at, metadata_fetched_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (repo) DO UPDATE SET \
                github_id = COALESCE(EXCLUDED.github_id, repos.github_id), \
                star_count = COALESCE(EXCLUDED.star_count, repos.star_count), \
                forks_count = COALESCE(EXCLUDED.forks_count, repos.forks_count), \
                created_at = COALESCE(EXCLUDED.created_at, repos.created_at), \
                metadata_fetched_at = EXCLUDED.metadata_fetched_at, \
                missing = FALSE",
        )
        .bind(repo)
        .bind(github_id)
        .bind(stargazers as i64)
        .bind(forks as i64)
        .bind(created_at)
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
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM repos \
                 WHERE history_complete = TRUE AND missing = FALSE \
                   AND metadata_fetched_at IS NOT NULL",
        )
        .fetch_one(&self.db.pool)
        .await?;
        Ok(n)
    }

    /// A page of repos with real cached star history, for the
    /// programmatic sitemap. Returns `(slug, updated_at)` where
    /// `updated_at` is the most recent of the stargazer / metadata fetch
    /// timestamps (falling back to the other when one is null). Ordered
    /// by that timestamp descending, then slug, so the ordering is stable
    /// across calls with the same data and pages don't overlap.
    pub async fn list_sitemap_repos(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<(String, DateTime<Utc>)>> {
        let rows = sqlx::query(
            "SELECT repo, \
                    COALESCE(GREATEST(archive_fetched_at, stargazers_fetched_at, metadata_fetched_at), \
                             archive_fetched_at, stargazers_fetched_at, metadata_fetched_at, NOW()) AS updated_at \
             FROM repos \
             WHERE history_complete = TRUE AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL \
             ORDER BY updated_at DESC, repo ASC \
             LIMIT $1 OFFSET $2",
        )
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
        let row: Option<(Option<DateTime<Utc>>, bool, bool)> = sqlx::query_as(
            "SELECT fetched_at, complete, missing FROM login_repo_lists WHERE login = $1",
        )
        .bind(login)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.map(|(fetched_at, complete, missing)| LoginReposMeta {
            fetched_at,
            complete,
            missing,
        }))
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
    pub async fn put_login_repos(&self, login: &str, repos: &[(String, i64)]) -> Result<()> {
        let now = Utc::now();
        let mut tx = self.db.pool.begin().await?;
        sqlx::query(
            "INSERT INTO login_repo_lists (login, fetched_at, complete, missing) \
             VALUES ($1, $2, FALSE, FALSE) \
             ON CONFLICT (login) DO UPDATE SET \
                fetched_at = EXCLUDED.fetched_at, complete = FALSE, missing = FALSE",
        )
        .bind(login)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM login_repos WHERE login = $1")
            .bind(login)
            .execute(&mut *tx)
            .await?;
        for (rank, (repo, stars)) in repos.iter().enumerate() {
            sqlx::query(
                "INSERT INTO login_repos (login, repo, stars, rank) VALUES ($1, $2, $3, $4)",
            )
            .bind(login)
            .bind(repo)
            .bind(*stars)
            .bind(rank as i64)
            .execute(&mut *tx)
            .await?;
        }
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
            "INSERT INTO login_repo_lists (login, fetched_at, complete, missing) \
             VALUES ($1, $2, FALSE, TRUE) \
             ON CONFLICT (login) DO UPDATE SET \
                fetched_at = EXCLUDED.fetched_at, complete = FALSE, missing = TRUE",
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
    use super::PLATFORM_ACTIVITY_SQL;
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
}
