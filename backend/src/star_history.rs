//! GitHub's own star history, reduced to the day series gitdebt stores.
//!
//! `GET /repos/{owner}/{repo}/stargazers/history` (GitHub, September 2026)
//! answers for any public repository, with no credential, and without naming a
//! single stargazer. Each record is one week:
//!
//! ```json
//! { "week": 1790467200, "total": 69, "days": [3, 16, 10, 6, 22, 12, 0] }
//! ```
//!
//! `days` counts the repository's CURRENT stargazers by the day they starred,
//! Sunday first, so an unstar removes a star from the day it was given and the
//! whole history sums to the live star count. That is the same measurement the
//! stargazer list used to give, at day resolution: exact, net of unstars, and
//! complete back to the creation week.
//!
//! GitHub states that week and day boundaries are not guaranteed to align with
//! UTC (in practice the days follow US Pacific time). This module therefore
//! treats each record as a run of seven CALENDAR dates and never as instants:
//! the week timestamp is rounded to the nearest UTC midnight to name its
//! Sunday, and day `i` is that Sunday plus `i`. Storing the date GitHub reports
//! is the honest reading; shifting stars across midnight to guess at an instant
//! would invent a precision the source does not have.
//!
//! Pure on purpose: no I/O and no clock, so every rule below is a unit test.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Days, NaiveDate};
use serde::Deserialize;
use thiserror::Error;

/// One record of the star-history endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StarHistoryWeek {
    /// Unix seconds of the week's start (a Sunday).
    pub week: i64,
    /// Stars given during the week; the sum of `days`.
    pub total: i64,
    /// Stars per day, Sunday first. Always seven entries.
    pub days: Vec<i64>,
}

/// The stars a repository's current stargazers gave on one calendar day.
/// Only days with at least one star exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarDay {
    pub day: NaiveDate,
    pub stars: i64,
}

/// A response that does not describe a star history. Every variant rejects
/// the whole read: a series built from part of a malformed answer would be
/// complete-looking and wrong, which is worse than one that arrives later.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StarHistoryError {
    #[error("week {week} has {len} days, expected 7")]
    WeekLength { week: i64, len: usize },
    #[error("week {week} has a negative day count")]
    NegativeDay { week: i64 },
    #[error("week {week} days sum to {sum}, but its total is {total}")]
    TotalMismatch { week: i64, sum: i64, total: i64 },
    #[error("week {week} is not a representable date")]
    InvalidWeek { week: i64 },
    #[error("weeks overlap on {day}")]
    OverlappingWeeks { day: NaiveDate },
}

const SECONDS_PER_DAY: i64 = 86_400;

/// The calendar Sunday a week record names.
///
/// Rounded to the NEAREST UTC midnight rather than truncated, so a week that
/// GitHub starts at Pacific midnight (07:00 or 08:00 UTC) and one it starts at
/// UTC midnight both name the same date, and so would a boundary up to twelve
/// hours either side.
fn week_start(week: i64) -> Result<NaiveDate, StarHistoryError> {
    week.checked_add(SECONDS_PER_DAY / 2)
        .map(|seconds| seconds.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY)
        .and_then(|midnight| DateTime::from_timestamp(midnight, 0))
        .map(|midnight| midnight.date_naive())
        .ok_or(StarHistoryError::InvalidWeek { week })
}

/// Reduce every page of a star-history read to its ascending day series.
///
/// * Every week is validated before any of it is used: seven days, none
///   negative, summing to the week's own total.
/// * A week that appears twice is kept once. Pages are read one after another
///   while the history keeps moving, so the week on a page boundary can be read
///   twice when a new week starts between two page reads.
/// * Days after `today` fold into `today`. A source whose day runs ahead of UTC
///   could otherwise place a star on a date that has not happened yet here,
///   and a chart would draw it past the present.
/// * Days with no stars are dropped: the series is a list of events, and a
///   zero adds nothing to any reader.
pub fn star_days(
    weeks: &[StarHistoryWeek],
    today: NaiveDate,
) -> Result<Vec<StarDay>, StarHistoryError> {
    let mut by_week: BTreeMap<NaiveDate, &StarHistoryWeek> = BTreeMap::new();
    for week in weeks {
        if week.days.len() != 7 {
            return Err(StarHistoryError::WeekLength {
                week: week.week,
                len: week.days.len(),
            });
        }
        if week.days.iter().any(|stars| *stars < 0) {
            return Err(StarHistoryError::NegativeDay { week: week.week });
        }
        let sum = week.days.iter().sum::<i64>();
        if sum != week.total {
            return Err(StarHistoryError::TotalMismatch {
                week: week.week,
                sum,
                total: week.total,
            });
        }
        by_week.entry(week_start(week.week)?).or_insert(week);
    }

    let mut by_day: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    let mut seen: BTreeSet<NaiveDate> = BTreeSet::new();
    for (start, week) in by_week {
        for (offset, stars) in week.days.iter().enumerate() {
            let day = start
                .checked_add_days(Days::new(offset as u64))
                .ok_or(StarHistoryError::InvalidWeek { week: week.week })?;
            // Two different weeks may never claim the same date. Weeks are
            // seven days apart, so a collision means the boundaries moved
            // between pages, and there is no way to tell which count is right.
            if !seen.insert(day) {
                return Err(StarHistoryError::OverlappingWeeks { day });
            }
            if *stars > 0 {
                *by_day.entry(day.min(today)).or_insert(0) += stars;
            }
        }
    }

    Ok(by_day
        .into_iter()
        .map(|(day, stars)| StarDay { day, stars })
        .collect())
}

/// Stars across a day series: the repository's star count as of the read.
pub fn total_stars(days: &[StarDay]) -> i64 {
    days.iter().map(|day| day.stars).sum()
}

/// Whether a read is too short to be the history of a repository GitHub says
/// has `authoritative` stars.
///
/// The endpoint is GitHub's own and already net of unstars, so the two figures
/// normally agree to within the stars given between two requests. An EMPTY
/// history for a starred repository, or one less than half its count and short
/// by a meaningful margin, is not that: it is a read that went wrong
/// somewhere, and publishing it would collapse a real curve to nothing. Such a
/// read is retried rather than stored. With no authoritative count to compare,
/// nothing is rejected — a new repository with no stars has an empty history.
pub fn implausibly_short(total: i64, authoritative: Option<i64>) -> bool {
    match authoritative {
        Some(authoritative) if authoritative > 0 => {
            total <= 0
                || (total.saturating_mul(2) < authoritative
                    && authoritative.saturating_sub(total) >= 50)
        }
        _ => false,
    }
}

/// Group star instants into the day series the writer stores, by UTC date.
/// Production reads arrive as days already; this is for callers that hold
/// instants, chiefly test fixtures.
pub fn days_of(instants: impl IntoIterator<Item = chrono::DateTime<chrono::Utc>>) -> Vec<StarDay> {
    let mut by_day: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for instant in instants {
        *by_day.entry(instant.date_naive()).or_insert(0) += 1;
    }
    by_day
        .into_iter()
        .map(|(day, stars)| StarDay { day, stars })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    fn week(week: i64, days: [i64; 7]) -> StarHistoryWeek {
        StarHistoryWeek {
            week,
            total: days.iter().sum(),
            days: days.to_vec(),
        }
    }

    const FAR_FUTURE: &str = "2100-01-01";

    /// The first two weeks of zhom/donutbrowser exactly as GitHub returned
    /// them. 1748131200 is Sunday 2025-05-25 00:00 UTC.
    #[test]
    fn weeks_become_ascending_calendar_days() {
        let weeks = vec![
            week(1748736000, [19, 8, 10, 3, 40, 25, 9]),
            week(1748131200, [0, 0, 0, 0, 13, 79, 21]),
        ];
        let days = star_days(&weeks, date(FAR_FUTURE)).unwrap();
        assert_eq!(
            days.first(),
            Some(&StarDay {
                day: date("2025-05-29"),
                stars: 13
            })
        );
        assert_eq!(days[1].day, date("2025-05-30"));
        assert_eq!(days[2].day, date("2025-05-31"));
        assert_eq!(
            days[3],
            StarDay {
                day: date("2025-06-01"),
                stars: 19
            }
        );
        assert!(days.windows(2).all(|pair| pair[0].day < pair[1].day));
        assert_eq!(total_stars(&days), 113 + 114);
        assert!(
            days.iter().all(|day| day.stars > 0),
            "empty days are not stored"
        );
    }

    /// GitHub does not promise UTC boundaries. A week starting at Pacific
    /// midnight still names the same Sunday as one starting at UTC midnight.
    #[test]
    fn week_boundaries_off_utc_name_the_same_sunday() {
        let utc = 1748131200;
        for offset in [-11 * 3600, -1, 1, 7 * 3600, 8 * 3600, 11 * 3600] {
            assert_eq!(
                week_start(utc + offset).unwrap(),
                date("2025-05-25"),
                "offset {offset}"
            );
        }
    }

    /// A week read twice across a page boundary counts once.
    #[test]
    fn a_week_read_on_two_pages_counts_once() {
        let repeated = week(1748131200, [0, 0, 0, 0, 13, 79, 21]);
        let days = star_days(&[repeated.clone(), repeated], date(FAR_FUTURE)).unwrap();
        assert_eq!(total_stars(&days), 113);
    }

    #[test]
    fn malformed_weeks_reject_the_whole_read() {
        let short = StarHistoryWeek {
            week: 1748131200,
            total: 1,
            days: vec![1],
        };
        assert!(matches!(
            star_days(&[short], date(FAR_FUTURE)),
            Err(StarHistoryError::WeekLength { len: 1, .. })
        ));

        let negative = StarHistoryWeek {
            week: 1748131200,
            total: 0,
            days: vec![1, -1, 0, 0, 0, 0, 0],
        };
        assert!(matches!(
            star_days(&[negative], date(FAR_FUTURE)),
            Err(StarHistoryError::NegativeDay { .. })
        ));

        let mut mismatched = week(1748131200, [1, 0, 0, 0, 0, 0, 0]);
        mismatched.total = 5;
        assert!(matches!(
            star_days(&[mismatched], date(FAR_FUTURE)),
            Err(StarHistoryError::TotalMismatch {
                sum: 1,
                total: 5,
                ..
            })
        ));
    }

    /// Weeks that are not seven days apart cannot both be right.
    #[test]
    fn overlapping_weeks_are_rejected() {
        let first = week(1748131200, [1, 1, 1, 1, 1, 1, 1]);
        let shifted = week(1748131200 + 3 * 86_400, [1, 1, 1, 1, 1, 1, 1]);
        assert!(matches!(
            star_days(&[first, shifted], date(FAR_FUTURE)),
            Err(StarHistoryError::OverlappingWeeks { .. })
        ));
    }

    /// A day reported ahead of the present is held at the present.
    #[test]
    fn days_past_today_fold_into_today() {
        let days = star_days(
            &[week(1748131200, [0, 0, 0, 2, 3, 4, 5])],
            date("2025-05-28"),
        )
        .unwrap();
        assert_eq!(
            days,
            vec![StarDay {
                day: date("2025-05-28"),
                stars: 14
            }]
        );
    }

    #[test]
    fn empty_history_is_an_empty_series() {
        assert_eq!(star_days(&[], date(FAR_FUTURE)).unwrap(), Vec::new());
        assert_eq!(
            star_days(&[week(1748131200, [0; 7])], date(FAR_FUTURE)).unwrap(),
            Vec::new()
        );
    }

    #[test]
    fn decodes_the_documented_shape() {
        let weeks: Vec<StarHistoryWeek> =
            serde_json::from_str(r#"[{"week":1790467200,"total":69,"days":[3,16,10,6,22,12,0]}]"#)
                .unwrap();
        assert_eq!(weeks[0].total, 69);
        assert_eq!(weeks[0].days.len(), 7);
    }

    #[test]
    fn implausibly_short_reads_are_retried_not_stored() {
        // An empty history for a starred repository.
        assert!(implausibly_short(0, Some(5_000)));
        // Truncated far below the count.
        assert!(implausibly_short(100, Some(5_000)));
        // Ordinary drift between two requests.
        assert!(!implausibly_short(4_990, Some(5_000)));
        assert!(!implausibly_short(5_010, Some(5_000)));
        // Small repositories: a gap under the absolute floor is not flagged.
        assert!(!implausibly_short(30, Some(60)));
        // No stars, or no count to compare: nothing to reject.
        assert!(!implausibly_short(0, Some(0)));
        assert!(!implausibly_short(0, None));
    }
}
