//! Deterministic commit-activity streaks for profile achievements.
//!
//! The caller supplies distinct calendar days with observed commits from the
//! profile's bounded, Postgres-backed repository scope. This module contains
//! no database or wall-clock access, which keeps badge thresholds and tests
//! independent from request timing.

use chrono::NaiveDate;
use serde::Serialize;

/// Public achievement ladder. Thresholds are deliberately calendar-day based:
/// commit volume cannot turn one busy day into a multi-day streak.
pub const COMMIT_STREAK_TIERS: [CommitStreakTierDefinition; 5] = [
    CommitStreakTierDefinition {
        key: "week-signal",
        label: "Week signal",
        days: 7,
        description: "Seven consecutive days of tracked project activity.",
    },
    CommitStreakTierDefinition {
        key: "month-in-motion",
        label: "Month in motion",
        days: 30,
        description: "Thirty consecutive days of tracked project activity.",
    },
    CommitStreakTierDefinition {
        key: "quarter-keeper",
        label: "Quarter keeper",
        days: 90,
        description: "Ninety consecutive days of tracked project activity.",
    },
    CommitStreakTierDefinition {
        key: "half-year-maintainer",
        label: "Half-year maintainer",
        days: 180,
        description: "One hundred eighty consecutive days of tracked project activity.",
    },
    CommitStreakTierDefinition {
        key: "year-in-motion",
        label: "Year in motion",
        days: 365,
        description: "A full year of consecutive tracked project activity.",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitStreakTierDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub days: i64,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitStreakTier {
    pub key: &'static str,
    pub label: &'static str,
    pub days: i64,
    pub description: &'static str,
    pub earned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitStreak {
    /// Consecutive active days in the still-live run. A run whose latest day
    /// is older than yesterday has ended and reports zero.
    pub current_days: i64,
    /// Longest consecutive-day run across the complete cached history.
    pub longest_days: i64,
    pub latest_active_date: Option<NaiveDate>,
    /// The complete stable ladder lets an authenticated owner render useful
    /// locked goals without inventing thresholds in the frontend.
    pub tiers: Vec<CommitStreakTier>,
}

impl CommitStreak {
    fn from_counts(
        current_days: i64,
        longest_days: i64,
        latest_active_date: Option<NaiveDate>,
    ) -> Self {
        Self {
            current_days,
            longest_days,
            latest_active_date,
            tiers: COMMIT_STREAK_TIERS
                .iter()
                .map(|tier| CommitStreakTier {
                    key: tier.key,
                    label: tier.label,
                    days: tier.days,
                    description: tier.description,
                    earned: longest_days >= tier.days,
                })
                .collect(),
        }
    }
}

/// Summarize consecutive activity days at `today`.
///
/// Input ordering, duplicates, zero-commit filtering and future-date filtering
/// are deliberately normalized here rather than entrusted to a caller. A
/// streak stays current through the day after its last activity so an
/// overnight profile view does not declare it dead before the user has had a
/// chance to contribute.
pub fn summarize_commit_streak(
    active_days: impl IntoIterator<Item = NaiveDate>,
    today: NaiveDate,
) -> CommitStreak {
    let mut days: Vec<NaiveDate> = active_days
        .into_iter()
        .filter(|day| *day <= today)
        .collect();
    days.sort_unstable();
    days.dedup();

    let Some(&first) = days.first() else {
        return CommitStreak::from_counts(0, 0, None);
    };

    let mut run = 1_i64;
    let mut longest = 1_i64;
    let mut previous = first;
    for &day in days.iter().skip(1) {
        if day == previous + chrono::Duration::days(1) {
            run += 1;
        } else {
            run = 1;
        }
        longest = longest.max(run);
        previous = day;
    }

    let latest = *days.last().expect("non-empty days has a last element");
    let yesterday = today - chrono::Duration::days(1);
    let current = if latest >= yesterday { run } else { 0 };
    CommitStreak::from_counts(current, longest, Some(latest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn empty_history_has_the_complete_locked_ladder() {
        let summary = summarize_commit_streak([], day("2026-07-24"));
        assert_eq!(summary.current_days, 0);
        assert_eq!(summary.longest_days, 0);
        assert_eq!(summary.latest_active_date, None);
        assert_eq!(
            summary
                .tiers
                .iter()
                .map(|tier| tier.days)
                .collect::<Vec<_>>(),
            vec![7, 30, 90, 180, 365]
        );
        assert!(summary.tiers.iter().all(|tier| !tier.earned));
    }

    #[test]
    fn ordering_duplicates_and_future_dates_cannot_inflate_a_streak() {
        let today = day("2026-07-24");
        let summary = summarize_commit_streak(
            [
                day("2026-07-24"),
                day("2026-07-22"),
                day("2026-07-23"),
                day("2026-07-23"),
                day("2026-07-25"),
            ],
            today,
        );
        assert_eq!(summary.current_days, 3);
        assert_eq!(summary.longest_days, 3);
        assert_eq!(summary.latest_active_date, Some(today));
    }

    #[test]
    fn an_old_run_stays_the_longest_but_is_not_current() {
        let start = day("2025-01-01");
        let days = (0..30).map(|offset| start + chrono::Duration::days(offset));
        let summary = summarize_commit_streak(days, day("2026-07-24"));
        assert_eq!(summary.current_days, 0);
        assert_eq!(summary.longest_days, 30);
        assert_eq!(
            summary
                .tiers
                .iter()
                .filter(|tier| tier.earned)
                .map(|tier| tier.key)
                .collect::<Vec<_>>(),
            vec!["week-signal", "month-in-motion"]
        );
    }

    #[test]
    fn yesterday_keeps_the_live_run_open_and_thresholds_are_inclusive() {
        let today = day("2026-07-24");
        let start = today - chrono::Duration::days(365);
        let summary = summarize_commit_streak(
            (0..365).map(|offset| start + chrono::Duration::days(offset)),
            today,
        );
        assert_eq!(summary.current_days, 365);
        assert_eq!(summary.longest_days, 365);
        assert!(summary.tiers.iter().all(|tier| tier.earned));
    }

    #[test]
    fn serialized_shape_carries_public_results_and_owner_goal_metadata() {
        let today = day("2026-07-24");
        let summary = summarize_commit_streak(
            (0..7).map(|offset| today - chrono::Duration::days(offset)),
            today,
        );
        let json = serde_json::to_value(summary).unwrap();
        assert_eq!(json["current_days"], 7);
        assert_eq!(json["longest_days"], 7);
        assert_eq!(json["latest_active_date"], "2026-07-24");
        assert_eq!(json["tiers"][0]["key"], "week-signal");
        assert_eq!(json["tiers"][0]["earned"], true);
        assert_eq!(json["tiers"][1]["earned"], false);
        assert_eq!(json["tiers"][4]["days"], 365);
    }

    #[test]
    fn every_tier_unlocks_on_its_exact_boundary() {
        let today = day("2026-07-24");
        for (index, threshold) in [7_i64, 30, 90, 180, 365].into_iter().enumerate() {
            let summary = summarize_commit_streak(
                (0..threshold).map(|offset| today - chrono::Duration::days(offset)),
                today,
            );
            assert_eq!(summary.longest_days, threshold);
            assert!(
                summary.tiers.iter().take(index + 1).all(|tier| tier.earned),
                "{threshold}-day history earns every tier through index {index}"
            );
            assert!(
                summary
                    .tiers
                    .iter()
                    .skip(index + 1)
                    .all(|tier| !tier.earned),
                "{threshold}-day history cannot earn a later tier"
            );
        }
    }
}
