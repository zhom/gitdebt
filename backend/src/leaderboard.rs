//! Durable daily repository rankings.
//!
//! A refresh scans completed public histories once, then atomically replaces a
//! tiny snapshot used by the public API. It deliberately stores repository
//! aggregates only: no actor or stargazer identity is collected.

use anyhow::{Context, Result};
use sqlx::{Postgres, Transaction};
use std::time::Duration;

use crate::db::Db;

const REFRESH_LOCK_ID: i64 = 0x6769_7464_6c62_6401;
const REFRESH_EVERY_HOURS: i64 = 24;

/// Start the lightweight freshness coordinator. The transaction-level
/// advisory lock makes this safe when more than one API replica is running.
pub fn spawn(db: Db) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = refresh_if_stale(&db).await {
                tracing::warn!(%error, "leaderboard snapshot refresh failed");
            }
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        }
    });
}

pub async fn refresh_if_stale(db: &Db) -> Result<bool> {
    let mut tx = db.pool.begin().await.context("begin leaderboard refresh")?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(REFRESH_LOCK_ID)
        .fetch_one(&mut *tx)
        .await
        .context("lock leaderboard refresh")?;
    if !acquired {
        return Ok(false);
    }

    let fresh: bool = sqlx::query_scalar(
        "SELECT EXISTS (\
             SELECT 1 FROM leaderboard_snapshot_state \
             WHERE id = TRUE \
               AND computed_at > NOW() - make_interval(hours => $1)\
         )",
    )
    .bind(REFRESH_EVERY_HOURS as i32)
    .fetch_one(&mut *tx)
    .await
    .context("check leaderboard freshness")?;
    if fresh {
        return Ok(false);
    }

    refresh(&mut tx).await?;
    tx.commit().await.context("commit leaderboard refresh")?;
    tracing::info!("leaderboard daily/weekly/monthly snapshot refreshed");
    Ok(true)
}

/// Deepest rank any request can reach: the page-size ceiling times one past
/// the page ceiling in `api::leaderboard_params`. Ranks past it are
/// unreachable, and materializing them wrote (and re-wrote, daily) one row per
/// tracked repository per metric and window — the great majority of a table
/// that only ever serves its first few pages.
const MAX_STORED_RANK: i64 = 20_100;

/// The whole daily snapshot, hoisted so the eligibility rules it encodes can
/// be asserted without a database.
const SNAPSHOT_SQL: &str = "WITH eligible AS MATERIALIZED (\
             SELECT repo, COALESCE(star_count, 0)::BIGINT AS stars, \
                    history_source IN ('gh_archive', 'spliced') AS advancing \
             FROM repos \
             WHERE history_complete = TRUE \
               AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL \
               AND star_count IS NOT NULL\
         ), activity AS MATERIALIZED (\
             SELECT s.repo, \
                    COUNT(*) FILTER (WHERE s.starred_at >= NOW() - INTERVAL '1 day')::BIGINT AS gained_1d, \
                    COUNT(*) FILTER (WHERE s.starred_at >= NOW() - INTERVAL '7 days')::BIGINT AS gained_7d, \
                    COUNT(*)::BIGINT AS gained_30d \
             FROM active_repo_star_history s \
             JOIN eligible e ON e.repo = s.repo AND e.advancing \
             WHERE s.starred_at >= NOW() - INTERVAL '30 days' \
             GROUP BY s.repo\
         ), ranked AS MATERIALIZED (\
             SELECT e.repo, e.stars, \
                    COALESCE(a.gained_1d, 0) AS gained_1d, \
                    COALESCE(a.gained_7d, 0) AS gained_7d, \
                    COALESCE(a.gained_30d, 0) AS gained_30d, \
                    ROW_NUMBER() OVER (ORDER BY e.stars DESC, e.repo ASC) AS stars_rank, \
                    ROW_NUMBER() OVER (ORDER BY COALESCE(a.gained_1d, 0) DESC, e.repo ASC) AS rank_1d, \
                    ROW_NUMBER() OVER (ORDER BY COALESCE(a.gained_7d, 0) DESC, e.repo ASC) AS rank_7d, \
                    ROW_NUMBER() OVER (ORDER BY COALESCE(a.gained_30d, 0) DESC, e.repo ASC) AS rank_30d \
             FROM eligible e LEFT JOIN activity a ON a.repo = e.repo\
         ) \
         INSERT INTO leaderboard_snapshots \
             (metric, window_days, rank, repo, stars, velocity, computed_at) \
         SELECT 'stars', 1, stars_rank, repo, stars, gained_1d, NOW() FROM ranked \
             WHERE stars_rank <= $1 \
         UNION ALL \
         SELECT 'stars', 7, stars_rank, repo, stars, gained_7d, NOW() FROM ranked \
             WHERE stars_rank <= $1 \
         UNION ALL \
         SELECT 'stars', 30, stars_rank, repo, stars, gained_30d, NOW() FROM ranked \
             WHERE stars_rank <= $1 \
         UNION ALL \
         SELECT 'velocity', 1, rank_1d, repo, stars, gained_1d, NOW() FROM ranked \
             WHERE gained_1d > 0 AND rank_1d <= $1 \
         UNION ALL \
         SELECT 'velocity', 7, rank_7d, repo, stars, gained_7d, NOW() FROM ranked \
             WHERE gained_7d > 0 AND rank_7d <= $1 \
         UNION ALL \
         SELECT 'velocity', 30, rank_30d, repo, stars, gained_30d, NOW() FROM ranked \
             WHERE gained_30d > 0 AND rank_30d <= $1";

async fn refresh(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("DELETE FROM leaderboard_snapshots")
        .execute(&mut **tx)
        .await
        .context("clear previous leaderboard snapshot")?;

    // `activity` is materialized once so the 30-day history range is scanned
    // once even though it feeds three rankings. Eligibility is restricted to
    // complete, successfully fetched public metadata; private/404 tombstones
    // can therefore never enter a snapshot.
    //
    // Activity is counted only for a series that can still advance. A frozen
    // exact snapshot keeps rows inside the window for a few weeks after the
    // stargazer list stopped serving, and counting them measures how recently
    // the repository was read rather than how fast it is growing — a dead
    // series briefly outranks live ones and then vanishes as its tail ages
    // out. `advancing` is the source test, not a date test, because the fix
    // has to hold for whenever a given repository froze.
    sqlx::query(SNAPSHOT_SQL)
        .bind(MAX_STORED_RANK)
        .execute(&mut **tx)
        .await
        .context("build leaderboard snapshot")?;

    sqlx::query(
        "INSERT INTO leaderboard_snapshot_state (id, computed_at) VALUES (TRUE, NOW()) \
         ON CONFLICT (id) DO UPDATE SET computed_at = EXCLUDED.computed_at",
    )
    .execute(&mut **tx)
    .await
    .context("mark leaderboard snapshot complete")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MAX_STORED_RANK;
    use super::{REFRESH_EVERY_HOURS, REFRESH_LOCK_ID, SNAPSHOT_SQL};

    /// A frozen exact snapshot keeps rows inside the 30-day window for weeks
    /// after its source stopped serving, so counting them ranks a dead series
    /// above live ones. Only a source that can still receive points may
    /// contribute activity, and the gate has to sit on the join — filtering
    /// later would still let the counts be computed and surface as a delta.
    #[test]
    fn only_a_series_that_can_still_advance_contributes_activity() {
        assert!(
            SNAPSHOT_SQL.contains("history_source IN ('gh_archive', 'spliced') AS advancing"),
            "activity eligibility must be decided by source, not by date"
        );
        assert!(
            SNAPSHOT_SQL.contains("JOIN eligible e ON e.repo = s.repo AND e.advancing"),
            "the advancing gate must be on the activity join itself"
        );
        // The star ranking is unaffected: it orders by the metadata star count,
        // which stays accurate for a frozen repository.
        assert!(SNAPSHOT_SQL.contains("ORDER BY e.stars DESC, e.repo ASC"));
    }

    #[test]
    fn refresh_cadence_and_lock_are_stable() {
        assert_eq!(REFRESH_EVERY_HOURS, 24);
        assert_ne!(REFRESH_LOCK_ID, 0);
    }
    /// The snapshot stores only the ranks a request can actually ask for. If
    /// the request-side paging ceilings ever grow past this, the extra pages
    /// would silently come back empty.
    #[test]
    fn stored_ranks_cover_every_reachable_page() {
        let deepest = crate::api::LEADERBOARD_PER_MAX * (crate::api::LEADERBOARD_PAGE_MAX + 1);
        assert!(
            MAX_STORED_RANK >= deepest,
            "the snapshot must cover rank {deepest}, the deepest a request can reach"
        );
    }
}
