//! Full-granularity star-history export + date-range filtering.
//!
//! Two surfaces:
//!   * `GET /api/repos/:o/:r/stars.csv` / `stars.json` — per-day
//!     aggregates (date, running total, delta) of the cached stargazer
//!     set. The aggregation happens **in SQL** ([`load_day_deltas`]), so
//!     a 300k-star repo never materializes one row per stargazer in
//!     memory for an export — at most one row per calendar day comes
//!     back over the wire.
//!   * `from=` / `to=` / `rebase=` query params on the chart endpoints —
//!     the pure filters here ([`filter_points`], [`filter_downloads`],
//!     [`filter_day_stats`]) slice a cumulative series to a date window
//!     *without* losing the true running total: the left edge of a
//!     `from=` window still reflects stars accumulated before the
//!     window, unless `rebase=1` explicitly rebases totals to the
//!     window start.
//!
//! Everything here is pure (data in → data out) except
//! [`load_day_deltas`]. Determinism matters: the chart endpoints'
//! output is cached bytes-exact upstream, so the filters must be free of
//! clocks and randomness — they are.

use anyhow::Result;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::Row;

use crate::chart::{DownloadCumPoint, Point};
use crate::db::Db;

/// One exported day: the date, the cumulative star total up to and
/// including that day, and the number of stars gained on that day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayStat {
    /// Serializes as `"YYYY-MM-DD"` (chrono's ISO 8601 date form).
    pub date: NaiveDate,
    pub total: u64,
    pub delta: u64,
}

/// The `/stars.json` body. Locked contract:
/// `{repo,total_stars,complete,series:[{date,total,delta}]}`.
#[derive(Debug, Clone, Serialize)]
pub struct StarExport {
    pub repo: String,
    /// Full cumulative total (NOT window-filtered) — matches the
    /// `/analyze` `total_stars` semantics.
    pub total_stars: u64,
    /// False while the stargazer fetch hasn't completed. The series is
    /// then empty — readers never trust partial data (see `cache.rs`).
    pub complete: bool,
    pub series: Vec<DayStat>,
}

/// An inclusive `[from, to]` date window. Either bound may be absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DateRange {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

impl DateRange {
    /// Parse `from=` / `to=` query values (`YYYY-MM-DD`). Absent / empty
    /// values mean "unbounded" on that side; `from > to` is rejected.
    /// Error strings are generic, user-facing 400 text — no internals.
    pub fn parse(from: Option<&str>, to: Option<&str>) -> Result<Self, &'static str> {
        fn one(s: &str) -> Result<NaiveDate, &'static str> {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| "invalid date: expected YYYY-MM-DD")
        }
        let from = match from.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(one(s)?),
            None => None,
        };
        let to = match to.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(one(s)?),
            None => None,
        };
        if let (Some(f), Some(t)) = (from, to)
            && f > t
        {
            return Err("invalid date range: from is after to");
        }
        Ok(Self { from, to })
    }
}

/// A parsed range request: the window plus whether cumulative totals
/// should be rebased to the window start (`rebase=1`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RangeSpec {
    pub range: DateRange,
    pub rebase: bool,
}

impl RangeSpec {
    /// True when the spec has no effect (no bounds, no rebase) — callers
    /// can skip the filter pass entirely.
    pub fn is_noop(&self) -> bool {
        self.range.from.is_none() && self.range.to.is_none() && !self.rebase
    }

    /// Stable cache-key fragment. Built from the *parsed* dates so
    /// spellings like `from=2020-1-1` and `from=2020-01-01` normalize to
    /// one cache entry.
    pub fn key(&self) -> String {
        let fmt = |d: Option<NaiveDate>| match d {
            Some(d) => d.format("%Y-%m-%d").to_string(),
            None => "-".to_string(),
        };
        format!(
            "r:{}..{}|rb:{}",
            fmt(self.range.from),
            fmt(self.range.to),
            u8::from(self.rebase)
        )
    }
}

/// Fold sorted per-day deltas into running-total day stats. Input must be
/// ascending by date with at most one entry per day (which is what
/// [`load_day_deltas`]'s `GROUP BY ... ORDER BY` guarantees). Negative
/// deltas can't occur in that query; they're clamped defensively.
pub fn accumulate(deltas: &[(NaiveDate, i64)]) -> Vec<DayStat> {
    let mut total = 0u64;
    let mut out = Vec::with_capacity(deltas.len());
    for (date, delta) in deltas {
        let d = (*delta).max(0) as u64;
        total += d;
        out.push(DayStat {
            date: *date,
            total,
            delta: d,
        });
    }
    out
}

/// Window-filter running-total day stats. Days before `from` are dropped
/// but their accumulation is preserved: the first surviving row carries
/// the true running total. With `rebase`, the total accumulated strictly
/// before the window is subtracted so the series restarts from the
/// window's own growth. Deltas are per-day and unaffected by rebasing.
pub fn filter_day_stats(rows: &[DayStat], spec: &RangeSpec) -> Vec<DayStat> {
    if spec.is_noop() {
        return rows.to_vec();
    }
    let mut baseline = 0u64;
    let mut out = Vec::new();
    for r in rows {
        if spec.range.from.is_some_and(|f| r.date < f) {
            baseline = r.total;
            continue;
        }
        if spec.range.to.is_some_and(|t| r.date > t) {
            break; // rows are date-ascending
        }
        let total = if spec.rebase {
            r.total.saturating_sub(baseline)
        } else {
            r.total
        };
        out.push(DayStat {
            date: r.date,
            total,
            delta: r.delta,
        });
    }
    out
}

/// Window-filter a cumulative star series ([`Point`]s, time-ascending)
/// for the chart renderers. Same semantics as [`filter_day_stats`]: the
/// left edge keeps the true running total unless `rebase` is set.
pub fn filter_points(series: &[Point], spec: &RangeSpec) -> Vec<Point> {
    if spec.is_noop() {
        return series.to_vec();
    }
    let mut baseline = 0u32;
    let mut out = Vec::new();
    for p in series {
        let d = p.at.date_naive();
        if spec.range.from.is_some_and(|f| d < f) {
            baseline = p.stars;
            continue;
        }
        if spec.range.to.is_some_and(|t| d > t) {
            break; // series is time-ascending
        }
        let stars = if spec.rebase {
            p.stars.saturating_sub(baseline)
        } else {
            p.stars
        };
        out.push(Point { at: p.at, stars });
    }
    out
}

/// Window-filter a cumulative download series for the dual-axis usage
/// chart. Mirrors [`filter_points`] over `u64` totals.
pub fn filter_downloads(series: &[DownloadCumPoint], spec: &RangeSpec) -> Vec<DownloadCumPoint> {
    if spec.is_noop() {
        return series.to_vec();
    }
    let mut baseline = 0u64;
    let mut out = Vec::new();
    for p in series {
        let d = p.at.date_naive();
        if spec.range.from.is_some_and(|f| d < f) {
            baseline = p.total;
            continue;
        }
        if spec.range.to.is_some_and(|t| d > t) {
            break;
        }
        let total = if spec.rebase {
            p.total.saturating_sub(baseline)
        } else {
            p.total
        };
        out.push(DownloadCumPoint { at: p.at, total });
    }
    out
}

/// Render day stats as CSV: header `date,total,delta`, one row per day,
/// `\n` line endings. Deterministic — same rows, same bytes.
pub fn to_csv(rows: &[DayStat]) -> String {
    let mut s = String::with_capacity(18 + rows.len() * 24);
    s.push_str("date,total,delta\n");
    for r in rows {
        s.push_str(&format!(
            "{},{},{}\n",
            r.date.format("%Y-%m-%d"),
            r.total,
            r.delta
        ));
    }
    s
}

/// Per-day star deltas for a repo, aggregated **in SQL** (`GROUP BY`
/// day), oldest-first. At most one row per calendar day, so even a
/// decade-old mega-repo returns a few thousand rows — never one row per
/// stargazer. Days are bucketed in UTC explicitly (`AT TIME ZONE 'UTC'`)
/// so the result is independent of the session timezone (determinism).
///
/// NOTE: callers must gate on `stargazers_complete` themselves — this
/// reads the raw rows and the completeness invariant (readers never
/// trust partial data) lives with the caller, same as
/// `cache::get_repo_stargazers_partial`.
pub async fn load_day_deltas(db: &Db, repo: &str) -> Result<Vec<(NaiveDate, i64)>> {
    let rows = sqlx::query(
        "SELECT (starred_at AT TIME ZONE 'UTC')::date AS day, COUNT(*) AS delta \
         FROM repo_stargazers \
         WHERE repo = $1 \
         GROUP BY 1 \
         ORDER BY 1",
    )
    .bind(repo)
    .fetch_all(&db.pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let day: NaiveDate = row.try_get("day")?;
        let delta: i64 = row.try_get("delta")?;
        out.push((day, delta));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::cumulative_series;
    use chrono::{DateTime, TimeZone, Utc};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn at_day(s: &str) -> DateTime<Utc> {
        Utc.from_utc_datetime(&d(s).and_hms_opt(12, 0, 0).unwrap())
    }

    fn spec(from: Option<&str>, to: Option<&str>, rebase: bool) -> RangeSpec {
        RangeSpec {
            range: DateRange::parse(from, to).unwrap(),
            rebase,
        }
    }

    fn sample_days() -> Vec<DayStat> {
        accumulate(&[
            (d("2020-01-01"), 3),
            (d("2020-01-03"), 2),
            (d("2020-02-10"), 5),
            (d("2020-03-01"), 1),
        ])
    }

    #[test]
    fn accumulate_running_totals() {
        let rows = sample_days();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0],
            DayStat {
                date: d("2020-01-01"),
                total: 3,
                delta: 3
            }
        );
        assert_eq!(
            rows[1],
            DayStat {
                date: d("2020-01-03"),
                total: 5,
                delta: 2
            }
        );
        assert_eq!(
            rows[2],
            DayStat {
                date: d("2020-02-10"),
                total: 10,
                delta: 5
            }
        );
        assert_eq!(
            rows[3],
            DayStat {
                date: d("2020-03-01"),
                total: 11,
                delta: 1
            }
        );
    }

    #[test]
    fn accumulate_empty_and_negative_clamped() {
        assert!(accumulate(&[]).is_empty());
        // Defensive clamp: a (impossible) negative delta contributes 0.
        let rows = accumulate(&[(d("2020-01-01"), -4), (d("2020-01-02"), 2)]);
        assert_eq!(rows[0].total, 0);
        assert_eq!(rows[0].delta, 0);
        assert_eq!(rows[1].total, 2);
    }

    #[test]
    fn range_parse_valid_and_unbounded() {
        let r = DateRange::parse(Some("2020-01-01"), Some("2020-12-31")).unwrap();
        assert_eq!(r.from, Some(d("2020-01-01")));
        assert_eq!(r.to, Some(d("2020-12-31")));
        // Absent / empty → unbounded.
        assert_eq!(DateRange::parse(None, None).unwrap(), DateRange::default());
        assert_eq!(
            DateRange::parse(Some(""), Some("  ")).unwrap(),
            DateRange::default()
        );
    }

    #[test]
    fn range_parse_from_after_to_is_error() {
        assert!(DateRange::parse(Some("2021-01-01"), Some("2020-01-01")).is_err());
        // Equal bounds (single-day window) are fine.
        assert!(DateRange::parse(Some("2020-06-15"), Some("2020-06-15")).is_ok());
    }

    #[test]
    fn range_parse_garbage_is_error() {
        assert!(DateRange::parse(Some("not-a-date"), None).is_err());
        assert!(DateRange::parse(None, Some("2020-13-40")).is_err());
        assert!(DateRange::parse(Some("2020/01/01"), None).is_err());
        // Error text is generic — no internals.
        let e = DateRange::parse(Some("nope"), None).unwrap_err();
        assert!(e.contains("YYYY-MM-DD"));
    }

    #[test]
    fn range_spec_key_is_stable_and_normalized() {
        let a = spec(Some("2020-01-01"), None, false);
        assert_eq!(a.key(), "r:2020-01-01..-|rb:0");
        // Non-zero-padded spelling normalizes to the same key.
        let b = spec(Some("2020-1-1"), None, false);
        assert_eq!(a.key(), b.key());
        let c = spec(Some("2020-01-01"), Some("2020-02-02"), true);
        assert_eq!(c.key(), "r:2020-01-01..2020-02-02|rb:1");
        assert_eq!(spec(None, None, false).key(), "r:-..-|rb:0");
    }

    #[test]
    fn filter_day_stats_noop_returns_all() {
        let rows = sample_days();
        assert_eq!(filter_day_stats(&rows, &spec(None, None, false)), rows);
    }

    #[test]
    fn filter_day_stats_left_edge_keeps_true_running_total() {
        let rows = sample_days();
        let out = filter_day_stats(&rows, &spec(Some("2020-02-01"), None, false));
        // First surviving row still reflects the 5 stars accumulated
        // before the window — NOT rebased to zero.
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0],
            DayStat {
                date: d("2020-02-10"),
                total: 10,
                delta: 5
            }
        );
        assert_eq!(
            out[1],
            DayStat {
                date: d("2020-03-01"),
                total: 11,
                delta: 1
            }
        );
    }

    #[test]
    fn filter_day_stats_rebase_restarts_at_window() {
        let rows = sample_days();
        let out = filter_day_stats(&rows, &spec(Some("2020-02-01"), None, true));
        assert_eq!(
            out[0],
            DayStat {
                date: d("2020-02-10"),
                total: 5,
                delta: 5
            }
        );
        assert_eq!(
            out[1],
            DayStat {
                date: d("2020-03-01"),
                total: 6,
                delta: 1
            }
        );
    }

    #[test]
    fn filter_day_stats_to_bound_is_inclusive() {
        let rows = sample_days();
        let out = filter_day_stats(&rows, &spec(None, Some("2020-02-10"), false));
        assert_eq!(out.len(), 3);
        assert_eq!(out.last().unwrap().date, d("2020-02-10"));
        // Totals unchanged — later rows don't affect earlier cumulatives.
        assert_eq!(out.last().unwrap().total, 10);
    }

    #[test]
    fn filter_day_stats_empty_window() {
        let rows = sample_days();
        // A valid window with no data in it → empty series, not an error.
        let out = filter_day_stats(&rows, &spec(Some("2020-01-10"), Some("2020-02-01"), false));
        assert!(out.is_empty());
        // Entirely after the data.
        assert!(filter_day_stats(&rows, &spec(Some("2021-01-01"), None, false)).is_empty());
    }

    #[test]
    fn filter_day_stats_single_day_window() {
        let rows = sample_days();
        let out = filter_day_stats(&rows, &spec(Some("2020-01-03"), Some("2020-01-03"), false));
        assert_eq!(
            out,
            vec![DayStat {
                date: d("2020-01-03"),
                total: 5,
                delta: 2
            }]
        );
        // Same single day, rebased: only the in-window growth remains.
        let out = filter_day_stats(&rows, &spec(Some("2020-01-03"), Some("2020-01-03"), true));
        assert_eq!(
            out,
            vec![DayStat {
                date: d("2020-01-03"),
                total: 2,
                delta: 2
            }]
        );
    }

    #[test]
    fn filter_points_left_edge_and_rebase() {
        let arrivals = vec![
            at_day("2020-01-01"),
            at_day("2020-01-02"),
            at_day("2020-02-01"),
            at_day("2020-03-01"),
        ];
        let series = cumulative_series(&arrivals);
        let s = spec(Some("2020-02-01"), None, false);
        let out = filter_points(&series, &s);
        assert_eq!(out.len(), 2);
        // True running total at the left edge (2 stars pre-window + 1).
        assert_eq!(out[0].stars, 3);
        assert_eq!(out[1].stars, 4);

        let rebased = filter_points(&series, &spec(Some("2020-02-01"), None, true));
        assert_eq!(rebased[0].stars, 1);
        assert_eq!(rebased[1].stars, 2);
    }

    #[test]
    fn filter_points_window_bounds_inclusive_and_deterministic() {
        let series = cumulative_series(&[
            at_day("2020-01-01"),
            at_day("2020-06-15"),
            at_day("2020-12-31"),
            at_day("2021-01-01"),
        ]);
        let s = spec(Some("2020-01-01"), Some("2020-12-31"), false);
        let a = filter_points(&series, &s);
        let b = filter_points(&series, &s);
        assert_eq!(a.len(), 3); // both bounds inclusive; 2021 point dropped
        // Pure function: same input → identical output.
        assert_eq!(
            a.iter().map(|p| (p.at, p.stars)).collect::<Vec<_>>(),
            b.iter().map(|p| (p.at, p.stars)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn filter_points_noop_and_empty() {
        let series = cumulative_series(&[at_day("2020-01-01")]);
        let out = filter_points(&series, &spec(None, None, false));
        assert_eq!(out.len(), 1);
        assert!(filter_points(&[], &spec(Some("2020-01-01"), None, true)).is_empty());
    }

    #[test]
    fn filter_downloads_mirrors_points() {
        let series = vec![
            DownloadCumPoint {
                at: at_day("2020-01-01"),
                total: 100,
            },
            DownloadCumPoint {
                at: at_day("2020-02-01"),
                total: 300,
            },
            DownloadCumPoint {
                at: at_day("2020-03-01"),
                total: 700,
            },
        ];
        let out = filter_downloads(&series, &spec(Some("2020-02-01"), None, false));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].total, 300); // true cumulative preserved
        let rebased = filter_downloads(&series, &spec(Some("2020-02-01"), None, true));
        assert_eq!(rebased[0].total, 200); // 300 - 100 pre-window baseline
        assert_eq!(rebased[1].total, 600);
    }

    #[test]
    fn csv_header_and_rows_exact() {
        let rows = sample_days();
        let csv = to_csv(&rows[..2]);
        assert_eq!(csv, "date,total,delta\n2020-01-01,3,3\n2020-01-03,5,2\n");
    }

    #[test]
    fn csv_empty_is_header_only() {
        assert_eq!(to_csv(&[]), "date,total,delta\n");
    }

    #[test]
    fn csv_is_deterministic() {
        let rows = sample_days();
        assert_eq!(to_csv(&rows), to_csv(&rows));
    }

    #[test]
    fn star_export_json_shape() {
        let body = StarExport {
            repo: "owner/repo".into(),
            total_stars: 11,
            complete: true,
            series: sample_days(),
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["repo"], "owner/repo");
        assert_eq!(v["total_stars"], 11);
        assert_eq!(v["complete"], true);
        assert_eq!(v["series"][0]["date"], "2020-01-01");
        assert_eq!(v["series"][0]["total"], 3);
        assert_eq!(v["series"][0]["delta"], 3);
        assert_eq!(v["series"].as_array().unwrap().len(), 4);
    }
}
