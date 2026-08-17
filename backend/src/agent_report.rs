//! The Markdown report served at `GET /api/repos/{owner}/{repo}/report.md`.
//!
//! The static site prerenders `<page>.md` only for catalogued repositories, so
//! an agent asking about any other public repository received the 404 page.
//! This renderer backs the universal surface: any public slug, answered from
//! Postgres, with the first request queueing the work it needs and saying where
//! to poll.
//!
//! Deliberately NOT a port of the frontend's `agent-markdown.ts`. That renderer
//! plus its embed catalog is ~1100 lines of TypeScript, and cloning it here
//! would be a permanent drift liability. This module carries the same content
//! model — measured figures, paste-ready snippets, data surfaces — in a compact
//! form, and points at `/badges.md` for the complete asset catalog.
//!
//! Pure and deterministic: every figure arrives in [`ReportView`], nothing here
//! reads the clock or the database, so identical inputs render identical bytes.

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::Deserialize;

use crate::agent_markdown::{bullet, document, document_header, fence, table, thousands};
use crate::analyzer::StarHistoryInsights;

/// Everything one report needs, already resolved by the handler.
#[derive(Debug, Clone)]
pub struct ReportView {
    /// Lowercased `owner/repo`, already validated by `is_valid_slug`.
    pub slug: String,
    /// Frontend origin without a trailing slash, for canonical and README links.
    pub site_origin: String,
    /// API origin without a trailing slash, for asset and data-surface URLs.
    pub api_origin: String,
    pub state: ReportState,
}

#[derive(Debug, Clone)]
pub enum ReportState {
    /// GitHub does not expose the slug publicly (tombstoned).
    NotPublic,
    /// The star history is not complete yet. Carries no star figures at all:
    /// the analyze path reports `total_stars = 0` for an incomplete history,
    /// and telling an agent a queued repository has zero stars is worse than
    /// telling it nothing.
    Running(RunningReport),
    /// The star history is complete. Star history is the product, so this
    /// state does not wait on repository health — [`ReportHealthSection`]
    /// says where the differentiator stands instead of withholding the
    /// figures Postgres already holds.
    Ready(Box<ReadyReport>),
}

#[derive(Debug, Clone)]
pub struct RunningReport {
    /// `queued`, `retrying`, or `ready` — the analyze path's public vocabulary.
    pub history_status: &'static str,
    pub backfilling: bool,
    pub health: ReportHealthSection,
    pub queue: QueueState,
    /// Place in the pending star-fetch line, 1 = next. `None` while a worker
    /// already holds the job or nothing is queued for this repository.
    pub queue_position: Option<i64>,
    /// Star-fetch jobs outstanding fleet-wide.
    pub queue_depth: u32,
}

#[derive(Debug, Clone)]
pub struct ReadyReport {
    pub stars: ReportStars,
    pub health: ReportHealthSection,
}

/// Repository health for one report: the measured figures, or why they are
/// absent. Health is the differentiator layered on the star history, never a
/// gate on it — a repository whose clone can never be analyzed still has a
/// complete, final star history to report.
#[derive(Debug, Clone)]
pub enum ReportHealthSection {
    Measured(Box<ReportHealth>),
    /// Queued, claimed, or waiting on a retry.
    Running,
    /// Parked after repeated failures, so polling will not produce figures.
    Unavailable,
}

/// What this request actually did to the durable queues. Reported rather than
/// assumed: both enqueue paths refuse work at their capacity ceiling, and a
/// 202 that promises a job nobody queued sends an agent into a poll loop that
/// can never end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueState {
    /// Work for this repository is queued or already in flight.
    Working,
    /// `?enqueue=0`: the caller asked to read, so nothing was offered.
    NotRequested,
    /// Nothing was queued because the queues are at their ceiling.
    Refused,
}

#[derive(Debug, Clone)]
pub struct ReportStars {
    pub total_stars: u32,
    /// GH Archive star *actions* rather than an exact stargazer snapshot.
    pub approximate: bool,
    pub event_count: u32,
    pub created_on: Option<NaiveDate>,
    pub coverage_start: Option<NaiveDate>,
    pub coverage_end: Option<NaiveDate>,
    pub insights: Option<StarHistoryInsights>,
}

/// The `ready: true` body of `health.json`, decoded rather than re-queried so
/// the report and the JSON surface can never disagree about a figure.
///
/// Every measured figure is required: this report states that its figures are
/// measured, so a key the summary stopped emitting must fail the decode rather
/// than render a fabricated zero under that banner. Only the three genuinely
/// nullable fields default.
#[derive(Debug, Clone, Deserialize)]
pub struct ReportHealth {
    pub archived: bool,
    #[serde(default)]
    pub analyzed_at: Option<DateTime<Utc>>,
    pub window_days: i64,
    pub total_commits: i64,
    pub attributed_commits: i64,
    pub analysis_truncated: bool,
    pub bus_factor: i64,
    pub contributors: i64,
    pub top_author_commits: i64,
    pub commits_window: i64,
    pub commits_previous_window: i64,
    #[serde(default)]
    pub last_commit_day: Option<NaiveDate>,
    pub tracked_files: i64,
    pub file_changes: i64,
    pub fix_changes: i64,
    pub fresh_files: i64,
    #[serde(default)]
    pub hotspot: Option<ReportHotspot>,
    pub todo_delta_window: i64,
    pub todo_outstanding: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReportHotspot {
    pub path: String,
    pub commits: i64,
    pub fix_commits: i64,
}

/// One README asset: heading, why it earns its place, and the path it lives at.
struct Embed {
    heading: &'static str,
    purpose: &'static str,
    /// Path under the API origin, including any asset-defining query.
    path: String,
    /// Alt text after the slug. README images are read by screen readers.
    alt: &'static str,
}

fn embeds(slug: &str) -> [Embed; 3] {
    let base = format!("/api/repos/{slug}");
    [
        Embed {
            heading: "Metrics badge",
            purpose: "Stars and forks in one compact chip. Goes in the badge row \
                      directly under the project title.",
            path: format!("{base}/badge.svg?metrics=stars,forks"),
            alt: "stars and forks",
        },
        Embed {
            heading: "Star history",
            purpose: "The full cumulative star curve. Goes in a `## Star history` \
                      section near the bottom of the README, above License.",
            path: format!("{base}/chart.svg"),
            alt: "star history",
        },
        Embed {
            heading: "Repository card",
            purpose: "Stars, forks, contributors, languages, and a 90-day \
                      sparkline in one panel. Goes in an About or Project status \
                      section.",
            path: format!("{base}/card.svg"),
            alt: "repository statistics",
        },
    ]
}

/// Render the report. The only entry point; every state goes through here.
pub fn render(view: &ReportView) -> String {
    let sections = match &view.state {
        ReportState::NotPublic => not_public_sections(view),
        ReportState::Running(running) => running_sections(view, running),
        ReportState::Ready(ready) => ready_sections(view, ready),
    };
    document(&sections)
}

fn canonical(view: &ReportView) -> String {
    format!("{}/{}", view.site_origin, view.slug)
}

fn header(view: &ReportView, title: &str) -> Vec<String> {
    vec![document_header(
        &format!("{} — {title}", view.slug),
        &canonical(view),
    )]
}

fn not_public_sections(view: &ReportView) -> Vec<String> {
    let mut sections = header(view, "not a public GitHub repository");
    sections.push(format!(
        "Status: not public. GitHub does not expose `{}` to gitdebt's \
         credentials, so there is nothing to report. Private repositories are \
         never analyzed or counted.",
        view.slug
    ));
    sections.push(format!(
        "Check the owner and name, or open https://github.com/{} directly.",
        view.slug
    ));
    sections
}

fn running_sections(view: &ReportView, running: &RunningReport) -> Vec<String> {
    let mut sections = header(view, "GitHub star history and repository health");
    sections.push(format!(
        "Status: {}. No star figures are available yet, and an absent figure \
         here does not mean zero.",
        match running.queue {
            QueueState::Working => "star history running",
            QueueState::NotRequested => "star history incomplete, nothing queued by this request",
            QueueState::Refused =>
                "nothing queued by this request — the ingestion queues are at \
                 capacity, so retry this URL in a few minutes",
        }
    ));

    let mut rows = vec![
        vec![
            "Star history".to_string(),
            match (running.backfilling, running.history_status) {
                (true, _) => "backfilling in resumable chunks".to_string(),
                (false, "ready") => "ready".to_string(),
                (false, "retrying") => "retrying after a failed attempt".to_string(),
                // Not "queued": nothing is queued and nothing will be. GitHub
                // serves this repository's stargazers only to its own admins
                // and collaborators, so the wait never ends on its own.
                (false, "restricted") => {
                    "not available — GitHub restricts this repository's stargazers".to_string()
                }
                (false, _) => "queued".to_string(),
            },
        ],
        vec![
            "Repository health".to_string(),
            match running.health {
                ReportHealthSection::Measured(_) => "analyzed, figures below".to_string(),
                ReportHealthSection::Running => "not analyzed yet".to_string(),
                ReportHealthSection::Unavailable => {
                    "unavailable — repeated attempts failed".to_string()
                }
            },
        ],
    ];
    if let Some(position) = running.queue_position {
        rows.push(vec![
            "Place in the star-fetch line".to_string(),
            thousands(position),
        ]);
    }
    rows.push(vec![
        "Star-fetch jobs outstanding".to_string(),
        thousands(i64::from(running.queue_depth)),
    ]);
    sections.push(format!(
        "## What is running\n\n{}",
        table(&["Pipeline", "State"], &rows)
    ));

    sections.push(format!(
        "Poll {}/api/repos/{}/progress.json for the phase, percent complete, \
         and `eta_seconds` where one can be measured — this report does not \
         guess a completion time. Request this URL again once that snapshot \
         reports `terminal: true`.",
        view.api_origin, view.slug
    ));

    // Health that has already been measured is printed even here: the
    // no-figures rule exists because an incomplete star history renders as a
    // confident zero, and that reasoning does not reach a completed analysis.
    // The absent cases are already covered by the table row above.
    if let ReportHealthSection::Measured(health) = &running.health {
        sections.extend(measured_health_sections(health));
    }
    sections.extend(readme_sections(view, true));
    sections.push(live_data_section(view));
    sections
}

fn ready_sections(view: &ReportView, ready: &ReadyReport) -> Vec<String> {
    let mut sections = header(view, "GitHub star history and repository health");
    sections.push(
        match ready.health {
            ReportHealthSection::Measured(_) => {
                "Status: analyzed. Every figure below is measured — star history from \
                 gitdebt's Postgres cache, repository health from the public Git \
                 history. Private repositories are never analyzed or counted."
            }
            _ => {
                "Status: star history analyzed. Every figure below is measured from \
                 gitdebt's Postgres cache. Private repositories are never analyzed \
                 or counted."
            }
        }
        .to_string(),
    );
    sections.extend(star_sections(&ready.stars));
    sections.extend(health_sections(view, &ready.health));
    sections.extend(readme_sections(view, false));
    sections.push(live_data_section(view));
    sections
}

fn star_sections(stars: &ReportStars) -> Vec<String> {
    let mut sections = Vec::new();
    let mut rows = vec![
        vec![
            "GitHub stars".to_string(),
            thousands(i64::from(stars.total_stars)),
        ],
        vec![
            "Series".to_string(),
            if stars.approximate {
                "public star actions (historical data)".to_string()
            } else {
                "current stargazers (exact snapshot)".to_string()
            },
        ],
    ];
    if stars.event_count > 0 {
        rows.push(vec![
            "Star events observed".to_string(),
            thousands(i64::from(stars.event_count)),
        ]);
    }
    if let (Some(start), Some(end)) = (stars.coverage_start, stars.coverage_end) {
        rows.push(vec![
            "History covers".to_string(),
            format!("{start} to {end}"),
        ]);
    }
    if let Some(created) = stars.created_on {
        rows.push(vec!["Repository created".to_string(), created.to_string()]);
    }
    sections.push(format!(
        "## Star snapshot\n\n{}",
        table(&["Metric", "Value"], &rows)
    ));

    if stars.approximate {
        sections.push(
            "The series records public star *actions* and cannot see unstars, \
             so it is an attention signal rather than a net-star curve. The \
             GitHub star total above is the headline figure."
                .to_string(),
        );
    }

    let Some(insights) = &stars.insights else {
        return sections;
    };

    let records = [
        ("Best day", insights.largest_day.as_ref()),
        ("Best week", insights.largest_week.as_ref()),
        ("Best 30 days", insights.largest_month.as_ref()),
    ];
    let record_rows: Vec<Vec<String>> = records
        .iter()
        .filter_map(|(label, record)| {
            record.map(|record| {
                vec![
                    (*label).to_string(),
                    format!(
                        "+{}",
                        thousands(record.stars_gained.min(i64::MAX as u64) as i64)
                    ),
                    if record.from == record.to {
                        record.to.to_string()
                    } else {
                        format!("{} to {}", record.from, record.to)
                    },
                ]
            })
        })
        .collect();
    if !record_rows.is_empty() {
        sections.push(format!(
            "### Growth records\n\n{}",
            table(&["Window", "Stars gained", "Dates"], &record_rows)
        ));
    }

    let milestone_rows: Vec<Vec<String>> = insights
        .milestones
        .iter()
        .filter_map(|milestone| {
            milestone.reached_at.map(|reached| {
                vec![
                    thousands(i64::from(milestone.stars)),
                    reached.to_string(),
                    milestone
                        .days_from_creation
                        .map(|days| thousands(i64::from(days)))
                        .unwrap_or_else(|| "unknown".to_string()),
                ]
            })
        })
        .collect();
    if !milestone_rows.is_empty() {
        sections.push(format!(
            "### Milestones\n\n{}",
            table(
                &["Stars", "First reached", "Days from creation"],
                &milestone_rows
            )
        ));
    }

    sections
}

/// The health section of a star-complete report: the measured figures, or the
/// one line that says why they are missing and where to watch for them.
fn health_sections(view: &ReportView, section: &ReportHealthSection) -> Vec<String> {
    match section {
        ReportHealthSection::Measured(health) => measured_health_sections(health),
        ReportHealthSection::Running => vec![format!(
            "## Repository health\n\nThe repository-health analysis has not \
             finished, so no health figures are included — an absent figure is \
             not a zero. The star figures above are final. Poll \
             {}/api/repos/{}/progress.json for the analysis phase, then request \
             this URL again.",
            view.api_origin, view.slug
        )],
        ReportHealthSection::Unavailable => vec![format!(
            "## Repository health\n\nRepository-health analysis is unavailable \
             for `{}`: repeated attempts failed and the job is parked, so \
             polling will not produce health figures. The star figures above \
             are final.",
            view.slug
        )],
    }
}

fn measured_health_sections(health: &ReportHealth) -> Vec<String> {
    let days = health.window_days.max(0);
    let mut sections = vec![format!(
        "## Repository health\n\nRaw figures over the trailing {days} days, \
         computed from the public Git history. gitdebt publishes the counts \
         rather than a grade here: judge them against what the project claims \
         to be."
    )];

    let mut rows = Vec::new();
    let commits_now = health.commits_window.max(0);
    let commits_before = health.commits_previous_window.max(0);
    let last_commit = health
        .last_commit_day
        .map(|day| format!(" · last commit {day}"))
        .unwrap_or_default();
    rows.push(vec![
        "Maintenance".to_string(),
        if commits_now == 0 && commits_before == 0 {
            format!("No commits in either {days}-day window{last_commit}")
        } else {
            format!(
                "{} commits in the last {days} days, {} in the {days} before{last_commit}",
                thousands(commits_now),
                thousands(commits_before)
            )
        },
    ]);

    let contributors = health.contributors.max(0);
    let bus_factor = health.bus_factor.max(0);
    rows.push(vec![
        "Ownership".to_string(),
        if bus_factor == 0 || contributors == 0 {
            "No commit authorship attributed yet".to_string()
        } else {
            format!(
                "{} of {} contributors write half of {} attributed commits · \
                 top author {} commits",
                thousands(bus_factor),
                thousands(contributors),
                thousands(health.attributed_commits.max(0)),
                thousands(health.top_author_commits.max(0))
            )
        },
    ]);

    let file_changes = health.file_changes.max(0);
    rows.push(vec![
        "Repair load".to_string(),
        if file_changes == 0 {
            "No file-level changes recorded".to_string()
        } else {
            format!(
                "{} of {} file changes came from fix-labelled commits",
                thousands(health.fix_changes.max(0)),
                thousands(file_changes)
            )
        },
    ]);

    let outstanding = health.todo_outstanding.max(0);
    rows.push(vec![
        "Debt markers".to_string(),
        if outstanding == 0 && health.todo_delta_window == 0 {
            "No TODO or FIXME markers found in the analysed history".to_string()
        } else {
            format!(
                "{}{} TODO/FIXME markers in the last {days} days · {} outstanding",
                if health.todo_delta_window > 0 {
                    "+"
                } else {
                    ""
                },
                thousands(health.todo_delta_window),
                thousands(outstanding)
            )
        },
    ]);

    if let Some(hotspot) = &health.hotspot {
        rows.push(vec![
            "Change hotspot".to_string(),
            format!(
                "`{}` · {} changes · {} fix-labelled",
                hotspot.path,
                thousands(hotspot.commits.max(0)),
                thousands(hotspot.fix_commits.max(0))
            ),
        ]);
    }

    let tracked = health.tracked_files.max(0);
    if tracked > 0 {
        rows.push(vec![
            "File freshness".to_string(),
            format!(
                "{} of {} tracked files touched in the last year",
                thousands(health.fresh_files.max(0)),
                thousands(tracked)
            ),
        ]);
    }

    rows.push(vec![
        "Commits read".to_string(),
        format!(
            "{} ({})",
            thousands(health.total_commits.max(0)),
            if health.analysis_truncated {
                "bounded analysis window — say so if you quote these figures"
            } else {
                "full commit history"
            }
        ),
    ]);

    if let Some(analyzed_at) = health.analyzed_at {
        rows.push(vec![
            "Analysis last ran".to_string(),
            analyzed_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        ]);
    }

    sections.push(table(&["Reading", "Figures"], &rows));

    if health.archived {
        sections.push(
            "GitHub reports this repository as archived. Read every maintenance \
             figure above as history, not as an ongoing rate."
                .to_string(),
        );
    }

    sections
}

fn readme_sections(view: &ReportView, pending: bool) -> Vec<String> {
    let link = format!("{}/{}?ref=readme", view.site_origin, view.slug);
    let mut sections = vec![format!(
        "## Put this in a README\n\nThree paste-ready snippets for `{}`. Every \
         URL is a plain public image: no account, token, build step, or GitHub \
         Action. Each snippet already carries light and dark variants, alt \
         text, and the link back to the report.",
        view.slug
    )];

    for embed in embeds(&view.slug) {
        sections.push(format!(
            "### {}\n\n{}\n\n{}",
            embed.heading,
            embed.purpose,
            fence("html", &picture_embed(view, &embed, &link))
        ));
    }

    if pending {
        sections.push(
            "These URLs are already correct. Until the analysis lands they \
             render a placeholder frame and queue the work rather than failing, \
             and the real asset replaces it at the same URL."
                .to_string(),
        );
    }

    sections.push(
        "Snippets are static by design, because motion in somebody else's \
         README should be their decision: add `animate=1` to an SVG URL when it \
         is wanted and it plays in a GitHub README, or use the `.gif` variant \
         for a surface that takes raster alone. Keep the surrounding link and \
         its `?ref=readme` — attribution lives on the link, never in an image \
         URL — and do not add cache-busting parameters."
            .to_string(),
    );
    sections.push(format!(
        "The complete asset catalog, with every snippet, is at {}/badges.md.",
        view.site_origin
    ));
    sections
}

/// The theme-aware embed shape, byte-identical to `readme-embeds.ts`'s
/// `pictureEmbed`: an SVG bakes its colors, so both variants ship and
/// `<picture>` picks the one matching the reader's OS preference.
fn picture_embed(view: &ReportView, embed: &Embed, link: &str) -> String {
    let separator = if embed.path.contains('?') { '&' } else { '?' };
    let url = |theme: &str| format!("{}{}{separator}theme={theme}", view.api_origin, embed.path);
    [
        format!("<a href=\"{link}\">"),
        "  <picture>".to_string(),
        format!(
            "    <source media=\"(prefers-color-scheme: dark)\" srcset=\"{}\" />",
            url("dark")
        ),
        format!(
            "    <img alt=\"{} {}\" src=\"{}\" />",
            view.slug,
            embed.alt,
            url("light")
        ),
        "  </picture>".to_string(),
        "</a>".to_string(),
    ]
    .join("\n")
}

fn live_data_section(view: &ReportView) -> String {
    let api = &view.api_origin;
    let slug = &view.slug;
    let lines = [
        format!("Repository on GitHub: https://github.com/{slug}"),
        format!("Star history JSON: {api}/api/repos/{slug}/stars.json"),
        format!("Star history CSV: {api}/api/repos/{slug}/stars.csv"),
        format!("Repository-health summary: {api}/api/repos/{slug}/health.json"),
        format!("Repository-health detail: {api}/api/repos/{slug}/stats.json"),
        format!("Earned badges: {api}/api/repos/{slug}/earned-badges.json"),
        format!("Queue and ETA snapshot: {api}/api/repos/{slug}/progress.json"),
    ];
    format!("## Live data\n\n{}", bullet(&lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{StarMilestone, StarWindowRecord};

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    fn view(state: ReportState) -> ReportView {
        ReportView {
            slug: "owner/repo".to_string(),
            site_origin: "https://gitdebt.com".to_string(),
            api_origin: "https://api.gitdebt.com".to_string(),
            state,
        }
    }

    #[test]
    fn tombstoned_repository_renders_the_not_public_report() {
        let expected = r##"# owner/repo — not a public GitHub repository

Canonical HTML: https://gitdebt.com/owner/repo

Status: not public. GitHub does not expose `owner/repo` to gitdebt's credentials, so there is nothing to report. Private repositories are never analyzed or counted.

Check the owner and name, or open https://github.com/owner/repo directly.
"##;
        assert_eq!(render(&view(ReportState::NotPublic)), expected);
    }

    #[test]
    fn queued_repository_reports_progress_and_never_prints_star_figures() {
        let rendered = render(&view(ReportState::Running(RunningReport {
            history_status: "queued",
            backfilling: false,
            health: ReportHealthSection::Running,
            queue: QueueState::Working,
            queue_position: Some(12),
            queue_depth: 431,
        })));
        let expected = r##"# owner/repo — GitHub star history and repository health

Canonical HTML: https://gitdebt.com/owner/repo

Status: star history running. No star figures are available yet, and an absent figure here does not mean zero.

## What is running

| Pipeline | State |
| --- | --- |
| Star history | queued |
| Repository health | not analyzed yet |
| Place in the star-fetch line | 12 |
| Star-fetch jobs outstanding | 431 |

Poll https://api.gitdebt.com/api/repos/owner/repo/progress.json for the phase, percent complete, and `eta_seconds` where one can be measured — this report does not guess a completion time. Request this URL again once that snapshot reports `terminal: true`.

## Put this in a README

Three paste-ready snippets for `owner/repo`. Every URL is a plain public image: no account, token, build step, or GitHub Action. Each snippet already carries light and dark variants, alt text, and the link back to the report.

### Metrics badge

Stars and forks in one compact chip. Goes in the badge row directly under the project title.

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=dark" />
    <img alt="owner/repo stars and forks" src="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=light" />
  </picture>
</a>
```

### Star history

The full cumulative star curve. Goes in a `## Star history` section near the bottom of the README, above License.

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=dark" />
    <img alt="owner/repo star history" src="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=light" />
  </picture>
</a>
```

### Repository card

Stars, forks, contributors, languages, and a 90-day sparkline in one panel. Goes in an About or Project status section.

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=dark" />
    <img alt="owner/repo repository statistics" src="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=light" />
  </picture>
</a>
```

These URLs are already correct. Until the analysis lands they render a placeholder frame and queue the work rather than failing, and the real asset replaces it at the same URL.

Snippets are static by design, because motion in somebody else's README should be their decision: add `animate=1` to an SVG URL when it is wanted and it plays in a GitHub README, or use the `.gif` variant for a surface that takes raster alone. Keep the surrounding link and its `?ref=readme` — attribution lives on the link, never in an image URL — and do not add cache-busting parameters.

The complete asset catalog, with every snippet, is at https://gitdebt.com/badges.md.

## Live data

- Repository on GitHub: https://github.com/owner/repo
- Star history JSON: https://api.gitdebt.com/api/repos/owner/repo/stars.json
- Star history CSV: https://api.gitdebt.com/api/repos/owner/repo/stars.csv
- Repository-health summary: https://api.gitdebt.com/api/repos/owner/repo/health.json
- Repository-health detail: https://api.gitdebt.com/api/repos/owner/repo/stats.json
- Earned badges: https://api.gitdebt.com/api/repos/owner/repo/earned-badges.json
- Queue and ETA snapshot: https://api.gitdebt.com/api/repos/owner/repo/progress.json
"##;
        assert_eq!(rendered, expected);
        // The contract that matters most: a queued repository is never
        // described with a star figure the analyze path defaulted to zero.
        assert!(!rendered.contains("GitHub stars"));
        assert!(!rendered.contains("Star snapshot"));
    }

    #[test]
    fn analyzed_repository_renders_measured_figures() {
        let rendered = render(&view(ReportState::Ready(Box::new(ReadyReport {
            stars: ReportStars {
                total_stars: 12_043,
                approximate: false,
                event_count: 11_904,
                created_on: Some(day(2016, 3, 28)),
                coverage_start: Some(day(2016, 4, 2)),
                coverage_end: Some(day(2026, 7, 29)),
                insights: Some(StarHistoryInsights {
                    milestones: vec![
                        StarMilestone {
                            stars: 100,
                            reached_at: Some(day(2016, 5, 11)),
                            days_from_creation: Some(44),
                        },
                        StarMilestone {
                            stars: 1_000,
                            reached_at: Some(day(2017, 2, 2)),
                            days_from_creation: Some(311),
                        },
                        StarMilestone {
                            stars: 100_000,
                            reached_at: None,
                            days_from_creation: None,
                        },
                    ],
                    largest_day: Some(StarWindowRecord {
                        stars_gained: 410,
                        from: day(2021, 5, 4),
                        to: day(2021, 5, 4),
                        window_days: 1,
                    }),
                    largest_week: Some(StarWindowRecord {
                        stars_gained: 1_204,
                        from: day(2021, 5, 1),
                        to: day(2021, 5, 7),
                        window_days: 7,
                    }),
                    largest_month: None,
                }),
            },
            health: ReportHealthSection::Measured(Box::new(ReportHealth {
                archived: false,
                analyzed_at: Some(
                    DateTime::parse_from_rfc3339("2026-07-29T04:12:11Z")
                        .expect("valid timestamp")
                        .with_timezone(&Utc),
                ),
                window_days: 90,
                total_commits: 12_900,
                attributed_commits: 12_744,
                analysis_truncated: false,
                bus_factor: 3,
                contributors: 45,
                top_author_commits: 4_102,
                commits_window: 143,
                commits_previous_window: 210,
                last_commit_day: Some(day(2026, 7, 28)),
                tracked_files: 1_204,
                file_changes: 8_110,
                fix_changes: 402,
                fresh_files: 640,
                hotspot: Some(ReportHotspot {
                    path: "src/api.rs".to_string(),
                    commits: 812,
                    fix_commits: 91,
                }),
                todo_delta_window: 12,
                todo_outstanding: 340,
            })),
        }))));
        let expected = r##"# owner/repo — GitHub star history and repository health

Canonical HTML: https://gitdebt.com/owner/repo

Status: analyzed. Every figure below is measured — star history from gitdebt's Postgres cache, repository health from the public Git history. Private repositories are never analyzed or counted.

## Star snapshot

| Metric | Value |
| --- | --- |
| GitHub stars | 12,043 |
| Series | current stargazers (exact snapshot) |
| Star events observed | 11,904 |
| History covers | 2016-04-02 to 2026-07-29 |
| Repository created | 2016-03-28 |

### Growth records

| Window | Stars gained | Dates |
| --- | --- | --- |
| Best day | +410 | 2021-05-04 |
| Best week | +1,204 | 2021-05-01 to 2021-05-07 |

### Milestones

| Stars | First reached | Days from creation |
| --- | --- | --- |
| 100 | 2016-05-11 | 44 |
| 1,000 | 2017-02-02 | 311 |

## Repository health

Raw figures over the trailing 90 days, computed from the public Git history. gitdebt publishes the counts rather than a grade here: judge them against what the project claims to be.

| Reading | Figures |
| --- | --- |
| Maintenance | 143 commits in the last 90 days, 210 in the 90 before · last commit 2026-07-28 |
| Ownership | 3 of 45 contributors write half of 12,744 attributed commits · top author 4,102 commits |
| Repair load | 402 of 8,110 file changes came from fix-labelled commits |
| Debt markers | +12 TODO/FIXME markers in the last 90 days · 340 outstanding |
| Change hotspot | `src/api.rs` · 812 changes · 91 fix-labelled |
| File freshness | 640 of 1,204 tracked files touched in the last year |
| Commits read | 12,900 (full commit history) |
| Analysis last ran | 2026-07-29T04:12:11Z |

## Put this in a README

Three paste-ready snippets for `owner/repo`. Every URL is a plain public image: no account, token, build step, or GitHub Action. Each snippet already carries light and dark variants, alt text, and the link back to the report.

### Metrics badge

Stars and forks in one compact chip. Goes in the badge row directly under the project title.

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=dark" />
    <img alt="owner/repo stars and forks" src="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=light" />
  </picture>
</a>
```

### Star history

The full cumulative star curve. Goes in a `## Star history` section near the bottom of the README, above License.

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=dark" />
    <img alt="owner/repo star history" src="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=light" />
  </picture>
</a>
```

### Repository card

Stars, forks, contributors, languages, and a 90-day sparkline in one panel. Goes in an About or Project status section.

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=dark" />
    <img alt="owner/repo repository statistics" src="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=light" />
  </picture>
</a>
```

Snippets are static by design, because motion in somebody else's README should be their decision: add `animate=1` to an SVG URL when it is wanted and it plays in a GitHub README, or use the `.gif` variant for a surface that takes raster alone. Keep the surrounding link and its `?ref=readme` — attribution lives on the link, never in an image URL — and do not add cache-busting parameters.

The complete asset catalog, with every snippet, is at https://gitdebt.com/badges.md.

## Live data

- Repository on GitHub: https://github.com/owner/repo
- Star history JSON: https://api.gitdebt.com/api/repos/owner/repo/stars.json
- Star history CSV: https://api.gitdebt.com/api/repos/owner/repo/stars.csv
- Repository-health summary: https://api.gitdebt.com/api/repos/owner/repo/health.json
- Repository-health detail: https://api.gitdebt.com/api/repos/owner/repo/stats.json
- Earned badges: https://api.gitdebt.com/api/repos/owner/repo/earned-badges.json
- Queue and ETA snapshot: https://api.gitdebt.com/api/repos/owner/repo/progress.json
"##;
        assert_eq!(rendered, expected);
    }

    #[test]
    fn identical_input_renders_identical_bytes() {
        let state = || {
            ReportState::Running(RunningReport {
                history_status: "retrying",
                backfilling: false,
                health: ReportHealthSection::Unavailable,
                queue: QueueState::NotRequested,
                queue_position: None,
                queue_depth: 0,
            })
        };
        assert_eq!(render(&view(state())), render(&view(state())));
    }

    /// The core product does not wait on the differentiator: a complete star
    /// history is reported the moment it exists, even when the repository's
    /// clone can never be analyzed.
    #[test]
    fn star_complete_report_prints_stars_without_health() {
        let rendered = render(&view(ReportState::Ready(Box::new(ReadyReport {
            stars: ReportStars {
                total_stars: 4_210,
                approximate: true,
                event_count: 4_190,
                created_on: Some(day(2019, 1, 2)),
                coverage_start: Some(day(2019, 1, 3)),
                coverage_end: Some(day(2026, 7, 29)),
                insights: None,
            },
            health: ReportHealthSection::Unavailable,
        }))));
        assert!(rendered.contains("Status: star history analyzed."));
        assert!(rendered.contains("| GitHub stars | 4,210 |"));
        assert!(rendered.contains("Repository-health analysis is unavailable"));
        // No health table may appear under a banner promising measured figures.
        assert!(!rendered.contains("| Reading | Figures |"));

        let pending = render(&view(ReportState::Ready(Box::new(ReadyReport {
            stars: ReportStars {
                total_stars: 4_210,
                approximate: true,
                event_count: 4_190,
                created_on: None,
                coverage_start: None,
                coverage_end: None,
                insights: None,
            },
            health: ReportHealthSection::Running,
        }))));
        assert!(pending.contains("| GitHub stars | 4,210 |"));
        assert!(pending.contains(
            "Poll https://api.gitdebt.com/api/repos/owner/repo/progress.json for the analysis phase"
        ));
    }

    /// A 202 must never claim work that a capacity ceiling refused.
    #[test]
    fn refused_queue_state_promises_nothing() {
        let rendered = render(&view(ReportState::Running(RunningReport {
            history_status: "queued",
            backfilling: false,
            health: ReportHealthSection::Running,
            queue: QueueState::Refused,
            queue_position: None,
            queue_depth: 5_000,
        })));
        assert!(rendered.contains(
            "Status: nothing queued by this request — the ingestion queues are at \
             capacity, so retry this URL in a few minutes."
        ));
        assert!(!rendered.contains("Place in the star-fetch line"));
    }

    #[test]
    fn health_json_decodes_into_the_report_shape() {
        let body = serde_json::json!({
            "ready": true,
            "repo": "owner/repo",
            "stars": 10,
            "archived": true,
            "analyzed_at": "2026-07-29T04:12:11Z",
            "window_days": 90,
            "total_commits": 5,
            "attributed_commits": 4,
            "analysis_truncated": true,
            "bus_factor": 1,
            "contributors": 2,
            "top_author_commits": 3,
            "commits_window": 1,
            "commits_previous_window": 0,
            "last_commit_day": "2026-07-28",
            "commit_months": [],
            "tracked_files": 7,
            "file_changes": 9,
            "fix_changes": 2,
            "fresh_files": 6,
            "hotspot": {"path": "a.rs", "commits": 3, "fix_commits": 1},
            "todo_delta_window": -2,
            "todo_outstanding": 4,
        });
        let health: ReportHealth =
            serde_json::from_value(body.clone()).expect("decode health summary");
        assert!(health.archived);
        assert!(health.analysis_truncated);
        assert_eq!(health.last_commit_day, Some(day(2026, 7, 28)));
        assert_eq!(
            health.hotspot.map(|hotspot| hotspot.path).as_deref(),
            Some("a.rs")
        );
        assert_eq!(health.todo_delta_window, -2);

        // Schema drift must fail the decode, not fabricate a zero under a
        // banner that says every figure is measured.
        let mut drifted = body.clone();
        drifted
            .as_object_mut()
            .expect("object body")
            .remove("bus_factor");
        assert!(serde_json::from_value::<ReportHealth>(drifted).is_err());

        // The three genuinely nullable fields stay optional.
        let mut sparse = body;
        for key in ["analyzed_at", "last_commit_day", "hotspot"] {
            sparse.as_object_mut().expect("object body").remove(key);
        }
        let health: ReportHealth =
            serde_json::from_value(sparse).expect("decode without nullables");
        assert!(health.analyzed_at.is_none());
        assert!(health.hotspot.is_none());
    }
}
