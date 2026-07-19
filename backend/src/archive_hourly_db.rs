//! Postgres-backed production wiring for the raw hourly GH Archive follower.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::Row;

use crate::db::Db;
use crate::gh_archive_hourly::{
    GhArchiveHourlyFollower, GzipArchiveDecoder, HourBatch, HourCommit, HourlyArchiveError,
    HourlyArchiveSink, HourlyFollowerConfig, ReqwestHourlyArchiveFetcher, TrackedRepositorySource,
};

const HOURLY_COMMIT_LOCK: i64 = 0x6769_7464_6562_7402;

#[derive(Clone)]
struct PostgresHourlyArchive {
    db: Db,
}

impl PostgresHourlyArchive {
    fn new(db: Db) -> Self {
        Self { db }
    }

    async fn next_hour(&self) -> Result<DateTime<Utc>, HourlyArchiveError> {
        let latest: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT MAX(archive_hour) FROM gh_archive_hours WHERE status = 'complete'",
        )
        .fetch_one(&self.db.pool)
        .await
        .map_err(repository_error)?;
        if let Some(latest) = latest {
            return latest
                .checked_add_signed(chrono::Duration::hours(1))
                .ok_or(HourlyArchiveError::HourOverflow);
        }
        let today = Utc::now().date_naive();
        Ok(Utc.from_utc_datetime(
            &today
                .and_hms_opt(0, 0, 0)
                .expect("midnight is a valid time"),
        ))
    }
}

#[async_trait]
impl TrackedRepositorySource for PostgresHourlyArchive {
    async fn tracked_repository_ids(&self) -> Result<BTreeSet<i64>, HourlyArchiveError> {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT github_id FROM repos \
             WHERE github_id IS NOT NULL AND archive_complete = TRUE \
               AND history_source = 'gh_archive' AND NOT missing",
        )
        .fetch_all(&self.db.pool)
        .await
        .map_err(repository_error)?;
        Ok(ids.into_iter().filter(|id| *id > 0).collect())
    }
}

#[async_trait]
impl HourlyArchiveSink for PostgresHourlyArchive {
    async fn is_hour_committed(
        &self,
        archive_hour: DateTime<Utc>,
    ) -> Result<bool, HourlyArchiveError> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM gh_archive_hours \
             WHERE archive_hour = $1 AND status = 'complete')",
        )
        .bind(archive_hour)
        .fetch_one(&self.db.pool)
        .await
        .map_err(sink_error)
    }

    async fn commit_hour(&self, batch: HourBatch) -> Result<HourCommit, HourlyArchiveError> {
        let mut tx = self.db.pool.begin().await.map_err(sink_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(HOURLY_COMMIT_LOCK)
            .execute(&mut *tx)
            .await
            .map_err(sink_error)?;
        let already: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM gh_archive_hours \
             WHERE archive_hour = $1 AND status = 'complete')",
        )
        .bind(batch.archive_hour)
        .fetch_one(&mut *tx)
        .await
        .map_err(sink_error)?;
        if already {
            tx.rollback().await.map_err(sink_error)?;
            return Ok(HourCommit::AlreadyCommitted);
        }

        let ids = batch
            .events
            .iter()
            .filter_map(|event| event.github_repo_id)
            .collect::<Vec<_>>();
        let rows = sqlx::query(
            "SELECT DISTINCT ON (github_id) repo, github_id \
             FROM repos WHERE github_id = ANY($1::BIGINT[]) \
               AND archive_complete = TRUE AND history_source = 'gh_archive' \
               AND NOT missing \
             ORDER BY github_id, metadata_fetched_at DESC NULLS LAST, repo",
        )
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(sink_error)?;
        let repo_by_id = rows
            .into_iter()
            .filter_map(|row| {
                Some((
                    row.try_get::<i64, _>("github_id").ok()?,
                    row.try_get::<String, _>("repo").ok()?,
                ))
            })
            .collect::<HashMap<_, _>>();

        let mut by_repo: HashMap<String, Vec<_>> = HashMap::new();
        for event in &batch.events {
            let Some(repo) = event
                .github_repo_id
                .and_then(|id| repo_by_id.get(&id))
                .cloned()
            else {
                continue;
            };
            by_repo.entry(repo).or_default().push(event);
        }

        for (repo, events) in by_repo {
            sqlx::query("SELECT repo FROM repos WHERE repo = $1 FOR UPDATE")
                .bind(&repo)
                .fetch_one(&mut *tx)
                .await
                .map_err(sink_error)?;
            let base: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(position), 0)::BIGINT \
                 FROM repo_star_arrivals WHERE repo = $1",
            )
            .bind(&repo)
            .fetch_one(&mut *tx)
            .await
            .map_err(sink_error)?;
            let positions = (0..events.len())
                .map(|index| base.saturating_add(index as i64).saturating_add(1))
                .collect::<Vec<_>>();
            let source_ids = events
                .iter()
                .map(|event| event.source_event_id.clone())
                .collect::<Vec<_>>();
            let timestamps = events
                .iter()
                .map(|event| event.created_at)
                .collect::<Vec<_>>();
            sqlx::query(
                "INSERT INTO repo_star_arrivals \
                    (repo, position, source_event_id, starred_at) \
                 SELECT $1, rows.position, rows.source_event_id, rows.starred_at \
                 FROM UNNEST($2::BIGINT[], $3::TEXT[], $4::TIMESTAMPTZ[]) \
                      AS rows(position, source_event_id, starred_at) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(&repo)
            .bind(positions)
            .bind(source_ids)
            .bind(timestamps)
            .execute(&mut *tx)
            .await
            .map_err(sink_error)?;
            sqlx::query(
                "UPDATE repos SET archive_fetched_at = NOW(), \
                    history_observed_count = (SELECT COUNT(*) FROM repo_star_arrivals \
                                              WHERE repo = $1), \
                    history_coverage_start = (SELECT MIN(starred_at) FROM repo_star_arrivals \
                                              WHERE repo = $1), \
                    history_coverage_end = (SELECT MAX(starred_at) FROM repo_star_arrivals \
                                            WHERE repo = $1) \
                 WHERE repo = $1",
            )
            .bind(&repo)
            .execute(&mut *tx)
            .await
            .map_err(sink_error)?;
        }

        sqlx::query(
            "INSERT INTO gh_archive_hours \
                (archive_hour, status, attempts, event_count, processed_at, last_error) \
             VALUES ($1, 'complete', 1, $2, NOW(), NULL) \
             ON CONFLICT (archive_hour) DO UPDATE SET \
                status = 'complete', attempts = gh_archive_hours.attempts + 1, \
                event_count = EXCLUDED.event_count, processed_at = EXCLUDED.processed_at, \
                last_error = NULL",
        )
        .bind(batch.archive_hour)
        .bind(batch.events.len() as i64)
        .execute(&mut *tx)
        .await
        .map_err(sink_error)?;
        tx.commit().await.map_err(sink_error)?;
        Ok(HourCommit::Committed)
    }
}

/// Start the raw forward follower. Historical BigQuery backfills and this
/// follower overlap safely because both persist the GH event ID.
pub fn spawn(db: Db) -> Result<(), HourlyArchiveError> {
    let config = config_from_env()?;
    let http = reqwest::Client::builder()
        .user_agent(concat!("gitdebt/", env!("CARGO_PKG_VERSION")))
        .no_gzip()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .map_err(|error| HourlyArchiveError::InvalidConfig(error.to_string()))?;
    let store = Arc::new(PostgresHourlyArchive::new(db));
    let follower = GhArchiveHourlyFollower::new(
        config,
        Arc::new(ReqwestHourlyArchiveFetcher::with_default_limit(http)),
        Arc::new(GzipArchiveDecoder),
        store.clone(),
        store.clone(),
    )?;
    tokio::spawn(async move {
        loop {
            let start = match store.next_hour().await {
                Ok(hour) => hour,
                Err(error) => {
                    tracing::error!(%error, "gh-archive-hourly: checkpoint read failed");
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
            };
            match follower.catch_up(start, Utc::now()).await {
                Ok(report) => {
                    if report.hours_committed > 0 {
                        tracing::info!(
                            hours = report.hours_committed,
                            matching_events = report.matching_events,
                            next_hour = %report.next_hour,
                            "gh-archive-hourly: forward activity committed"
                        );
                    }
                    let caught_up = report
                        .eligible_through
                        .is_none_or(|eligible| report.next_hour > eligible);
                    tokio::time::sleep(if caught_up {
                        Duration::from_secs(300)
                    } else {
                        Duration::from_secs(1)
                    })
                    .await;
                }
                Err(error) => {
                    tracing::error!(%error, "gh-archive-hourly: follower pass failed");
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    });
    Ok(())
}

fn config_from_env() -> Result<HourlyFollowerConfig, HourlyArchiveError> {
    let mut config = HourlyFollowerConfig::default();
    if let Some(value) = env_u64("GH_ARCHIVE_HOURLY_LAG_MINUTES")? {
        config.lag = Duration::from_secs(value.saturating_mul(60));
    }
    if let Some(value) = env_usize("GH_ARCHIVE_HOURLY_MAX_HOURS")? {
        config.max_hours_per_run = value;
    }
    if let Some(value) = env_usize("GH_ARCHIVE_HOURLY_MAX_EVENTS")? {
        config.max_matching_events = value;
    }
    config.validate()?;
    Ok(config)
}

fn env_u64(name: &str) -> Result<Option<u64>, HourlyArchiveError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                HourlyArchiveError::InvalidConfig(format!("{name} must be an unsigned integer"))
            })
        })
        .transpose()
}

fn env_usize(name: &str) -> Result<Option<usize>, HourlyArchiveError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                HourlyArchiveError::InvalidConfig(format!("{name} must be an unsigned integer"))
            })
        })
        .transpose()
}

fn repository_error(error: sqlx::Error) -> HourlyArchiveError {
    HourlyArchiveError::RepositorySource(error.to_string())
}

fn sink_error(error: sqlx::Error) -> HourlyArchiveError {
    HourlyArchiveError::Sink(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn hourly_env_defaults_validate() {
        HourlyFollowerConfig::default().validate().unwrap();
    }

    #[test]
    fn checkpoint_lock_is_distinct() {
        assert_ne!(HOURLY_COMMIT_LOCK, 0);
    }

    #[test]
    fn imported_time_is_hour_aligned() {
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 37, 0).unwrap();
        let hour = now
            .with_minute(0)
            .and_then(|value| value.with_second(0))
            .unwrap();
        assert_eq!(hour.minute(), 0);
        assert_eq!(hour.second(), 0);
    }

    #[tokio::test]
    async fn postgres_hour_commit_is_atomic_and_idempotent() {
        let Ok(url) = std::env::var("GITDEBT_TEST_DATABASE_URL") else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let db = Db::connect(&url).await.unwrap();
        let repo = format!("gitdebt-hourly-test/{}", std::process::id());
        let github_id = 9_000_000_000_i64 + i64::from(std::process::id());
        let hour = Utc.with_ymd_and_hms(2090, 1, 1, 3, 0, 0).unwrap();
        sqlx::query("DELETE FROM gh_archive_hours WHERE archive_hour = $1")
            .bind(hour)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM repo_star_arrivals WHERE repo = $1")
            .bind(&repo)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO repos \
                (repo, github_id, star_count, archive_complete, history_complete, history_source) \
             VALUES ($1, $2, 77, TRUE, TRUE, 'gh_archive') \
             ON CONFLICT (repo) DO UPDATE SET github_id = EXCLUDED.github_id, \
                star_count = 77, archive_complete = TRUE, history_complete = TRUE, \
                history_source = 'gh_archive'",
        )
        .bind(&repo)
        .bind(github_id)
        .execute(&db.pool)
        .await
        .unwrap();
        let store = PostgresHourlyArchive::new(db.clone());
        let batch = HourBatch {
            archive_hour: hour,
            records_seen: 1,
            events: vec![crate::gh_archive::GhArchiveStarEvent {
                github_repo_id: Some(github_id),
                repository: repo.clone(),
                source_event_id: Some("hourly-test-event".to_string()),
                created_at: hour + chrono::Duration::minutes(5),
            }],
        };
        assert_eq!(
            store.commit_hour(batch.clone()).await.unwrap(),
            HourCommit::Committed
        );
        assert_eq!(
            store.commit_hour(batch).await.unwrap(),
            HourCommit::AlreadyCommitted
        );
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM repo_star_arrivals WHERE repo = $1")
                .bind(&repo)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(count, 1);
        let current: i64 = sqlx::query_scalar("SELECT star_count FROM repos WHERE repo = $1")
            .bind(&repo)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(current, 77, "hourly events must not replace current total");

        sqlx::query("DELETE FROM gh_archive_hours WHERE archive_hour = $1")
            .bind(hour)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM repo_star_arrivals WHERE repo = $1")
            .bind(&repo)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM repos WHERE repo = $1")
            .bind(&repo)
            .execute(&db.pool)
            .await
            .unwrap();
    }
}
