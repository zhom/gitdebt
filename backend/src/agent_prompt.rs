//! The prompt behind "Ask an agent to add this to my repo".
//!
//! A coding agent lands in somebody's checkout with no idea what gitdebt is,
//! which URLs are real, or where a star-history chart belongs. This module
//! writes it all down once: the measured numbers (so the agent never invents a
//! statistic), the exact paste-ready snippets, the rules that make a published
//! embed correct, and the places to look beyond `README.md`.
//!
//! A port of `frontend/src/lib/agent-prompt.ts`, which stays in the frontend to
//! back the clipboard button. Pure and deterministic — the same repository and
//! the same snapshot always produce the same prompt, so what a visitor copies
//! is what the API's Markdown surfaces document.

use chrono::{Datelike, Duration, NaiveDate};

use crate::agent_embeds::{
    CANDIDATE_FILES, EMBED_RULES, EXISTING_STAR_HISTORY_MARKERS, EmbedAsset, EmbedGroup,
    QUERY_REFERENCE, best_embed, best_embed_language, profile_embed_assets, readme_link,
    repo_embed_assets,
};
use crate::agent_markdown::{bullet, document, fence, origin, thousands};

/// The placeholder slug the repository-less prompt carries.
pub const PLACEHOLDER_SLUG: &str = "OWNER/REPO";

/// How the trailing 90-day pace compares to the lifetime average.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarTrend {
    Accelerating,
    Steady,
    Slowing,
}

/// The star facts every agent-facing surface quotes.
///
/// Every figure is optional and absence means "not measured", never zero. A
/// repository whose star history is still incomplete must be described with
/// `None` rather than a `StarSummary` full of zeros: the analyze path reports
/// `total_stars = 0` for an empty history, and publishing that as a fact is the
/// one defect these surfaces exist to avoid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarSummary {
    pub total_stars: Option<i64>,
    pub gained_30: Option<i64>,
    pub gained_90: Option<i64>,
    pub trend: Option<StarTrend>,
    /// First point of the cached series, rendered as `Mar 2013`.
    pub first_star_month: Option<NaiveDate>,
    /// True when the curve is GH Archive star *activity* rather than a
    /// stargazer snapshot. The distinction has to survive into the prompt: an
    /// agent that writes "net stars" about an activity series publishes a wrong
    /// claim.
    pub approximate: bool,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl StarSummary {
    /// Derive the summary from a cumulative, date-ascending star series.
    ///
    /// Windows are anchored on the series' own last point rather than the wall
    /// clock, which is what keeps a cached prompt identical to the one a
    /// visitor copies from the live page.
    pub fn from_history(
        history: &[(NaiveDate, i64)],
        total_stars: Option<i64>,
        approximate: bool,
    ) -> Self {
        Self {
            total_stars,
            gained_30: gained_in_trailing_days(history, 30),
            gained_90: gained_in_trailing_days(history, 90),
            trend: growth_trend(history),
            first_star_month: history.first().map(|(day, _)| *day),
            approximate,
        }
    }

    /// `Mar 2013`, in the same shape the frontend prints.
    pub fn first_month_label(&self) -> Option<String> {
        let day = self.first_star_month?;
        let month = MONTHS.get(day.month0() as usize)?;
        Some(format!("{month} {}", day.year()))
    }
}

/// Cumulative total at or before `cutoff`; 0 before the series begins.
fn total_at_or_before(history: &[(NaiveDate, i64)], cutoff: NaiveDate) -> i64 {
    let mut total = 0;
    for (day, stars) in history {
        if *day > cutoff {
            break;
        }
        total = *stars;
    }
    total
}

/// Stars gained in the trailing window, anchored on the last data point.
fn gained_in_trailing_days(history: &[(NaiveDate, i64)], days: i64) -> Option<i64> {
    let (last_day, last_total) = *history.last()?;
    let cutoff = last_day.checked_sub_signed(Duration::days(days))?;
    Some((last_total - total_at_or_before(history, cutoff)).max(0))
}

/// Recent pace against the lifetime average. `None` under roughly six months of
/// history, where no honest verdict is available.
fn growth_trend(history: &[(NaiveDate, i64)]) -> Option<StarTrend> {
    let (first_day, _) = *history.first()?;
    let (last_day, last_total) = *history.last()?;
    let span_days = (last_day - first_day).num_days();
    if span_days < 180 {
        return None;
    }
    let lifetime = last_total as f64 / span_days as f64;
    if lifetime <= 0.0 {
        return None;
    }
    let recent = gained_in_trailing_days(history, 90)? as f64 / 90.0;
    Some(if recent > lifetime * 1.25 {
        StarTrend::Accelerating
    } else if recent < lifetime * 0.75 {
        StarTrend::Slowing
    } else {
        StarTrend::Steady
    })
}

fn numbered(lines: &[String]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{}. {line}", index + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One asset as a headed, fenced, paste-ready block.
fn snippet_block(api: &str, asset: &EmbedAsset, link: &str, heading: &str) -> String {
    format!(
        "{heading}\n\n{}",
        fence(best_embed_language(asset), &best_embed(api, asset, link))
    )
}

/// One asset as a "path — name: purpose" bullet, for the optional lists.
fn asset_bullets<'a>(api: &str, assets: impl Iterator<Item = &'a EmbedAsset>) -> String {
    bullet(
        assets
            .map(|asset| format!("`{api}{}` — {}: {}", asset.path, asset.name, asset.purpose))
            .collect::<Vec<_>>(),
    )
}

/// The measured-facts block. Empty when nothing has been measured yet, which is
/// what routes the prompt to the "read it from health.json" branch instead of
/// letting it print a figure nobody computed.
fn repo_evidence(stars: Option<&StarSummary>) -> Vec<String> {
    let mut facts = Vec::new();
    let Some(stars) = stars else {
        return facts;
    };

    if let Some(total) = stars.total_stars {
        let mut window = Vec::new();
        if let Some(gained) = stars.gained_90 {
            window.push(format!("+{} in 90 days", thousands(gained)));
        }
        if let Some(gained) = stars.gained_30 {
            window.push(format!("+{} in 30", thousands(gained)));
        }
        let pace = match stars.trend {
            Some(StarTrend::Accelerating) => ", running ahead of its lifetime pace",
            Some(StarTrend::Slowing) => ", below its lifetime pace",
            Some(StarTrend::Steady) => ", in line with its lifetime pace",
            None => "",
        };
        facts.push(format!(
            "{} GitHub stars{}{pace}.",
            thousands(total),
            if window.is_empty() {
                String::new()
            } else {
                format!(" ({})", window.join(", "))
            }
        ));
    }

    // Outside the total-stars branch on purpose: a series can be known to be
    // GH Archive activity before any total is available, and an agent that
    // calls an activity curve "net stars" publishes a wrong claim either way.
    if stars.approximate {
        facts.push(
            "The star curve is historical star activity, not a net-star \
             series: it records star actions and cannot see unstars. Describe it \
             as star activity, never as net stars."
                .to_string(),
        );
    }

    if let Some(month) = stars.first_month_label() {
        facts.push(format!("Star history begins {month}."));
    }

    facts
}

/// The prompt for one repository.
///
/// `slug` may be [`PLACEHOLDER_SLUG`], in which case the prompt opens by
/// telling the agent to resolve the real slug from the checkout's git remote.
pub fn repo_agent_prompt(slug: &str, site: &str, api: &str, stars: Option<&StarSummary>) -> String {
    let (site, api) = (origin(site), origin(api));
    let generic = slug == PLACEHOLDER_SLUG;
    let report = format!("{site}/{slug}");
    let link = readme_link(site, &format!("/{slug}"));
    let assets = repo_embed_assets(slug);
    let asset = |id: &str| assets.iter().find(|asset| asset.id == id);
    let evidence = repo_evidence(stars);

    let mut sections = vec![
        format!(
            "# Add gitdebt analytics to the {} README",
            if generic { "project's" } else { slug }
        ),
        format!(
            "gitdebt ({site}) turns public GitHub data into plain image URLs: \
             star history, a metrics badge, and repository-health charts. No \
             account, token, build step, or GitHub Action is involved — the URLs \
             below are already live and already pointed at this project."
        ),
    ];

    if generic {
        sections.push(format!(
            "## Step 0 — resolve the repository\n\nRun `git remote get-url \
             origin` and take the `owner/repo` slug from it. Replace every \
             `{PLACEHOLDER_SLUG}` below with that slug, lowercased. If the \
             remote is not a public GitHub repository, stop and say so: gitdebt \
             only serves public repositories."
        ));
    }

    if evidence.is_empty() {
        sections.push(format!(
            "## Numbers\n\nDo not write statistics into the README by hand — \
             they go stale. The images below are regenerated from live data. If \
             you need a figure for prose, read it from \
             {api}/api/repos/{slug}/health.json."
        ));
    } else {
        sections.push(format!(
            "## What gitdebt has measured\n\n{}\n\nUse these numbers if you \
             write prose around the images. Do not invent others. Every figure \
             is re-checkable at {api}/api/repos/{slug}/health.json and \
             {api}/api/repos/{slug}/stars.json.",
            bullet(&evidence)
        ));
    }

    sections.push(
        "## What to add\n\nPaste these snippets as-is. They are complete, and \
         they already carry light and dark variants plus alt text."
            .to_string(),
    );

    if let Some(badge) = asset("badge-metrics") {
        sections.push(snippet_block(
            api,
            badge,
            &link,
            &format!("### 1. Metrics badge — {}", badge.placement),
        ));
    }
    if let Some(chart) = asset("chart") {
        sections.push(format!(
            "{}\n\nGive it a `## Star history` heading of its own if the README \
             does not already have one.",
            snippet_block(
                api,
                chart,
                &link,
                &format!("### 2. Star history — {}", chart.placement)
            )
        ));
    }
    if let Some(card) = asset("card") {
        sections.push(snippet_block(
            api,
            card,
            &link,
            &format!("### 3. Repository card (optional) — {}", card.placement),
        ));
    }

    sections.push(format!(
        "### 4. Repository-health charts (optional)\n\nEach of these is the same \
         `<picture>` shape as above, with a different path. Add at most two, and \
         only where a reader would want them — typically a Project health or \
         Contributing section. More than that reads as clutter and slows the \
         page down.\n\n{}",
        asset_bullets(
            api,
            assets
                .iter()
                .filter(|asset| asset.group == EmbedGroup::Health)
        )
    ));

    sections.push(format!(
        "### 5. Earned signal badge (optional)\n\nFetch \
         `{api}/api/repos/{slug}/earned-badges.json` first. It returns one entry \
         per signal with an `earned` boolean. Publish only the signals where \
         `earned` is `true` — an unearned signal renders greyed out and claims \
         nothing.\n\nBadge URL shape: \
         `{api}/api/repos/{slug}/badge.svg?signal=SIGNAL&theme=dark`, where \
         `SIGNAL` is `active`, `community`, `momentum`, or `contributor-ready`."
    ));

    sections.push(format!("## Rules\n\n{}", bullet(EMBED_RULES)));

    sections.push(format!(
        "## If the project already shows a star-history chart\n\nReplace it in \
         place. Keep the surrounding heading and prose; swap only the image and \
         the link it wraps. Do not stack a second chart underneath. Search the \
         repository for these markers:\n\n{}",
        bullet(
            EXISTING_STAR_HISTORY_MARKERS
                .iter()
                .map(|marker| format!("`{marker}`"))
                .collect::<Vec<_>>()
        )
    ));

    sections.push(format!(
        "## Where else to look\n\n{}\n\nOnly touch a file where the addition \
         genuinely belongs. An unrelated docs page does not need a commit \
         calendar.",
        bullet(CANDIDATE_FILES)
    ));

    sections.push(format!(
        "## Tuning\n\nQuery parameters, if the defaults do not fit:\n\n{}",
        bullet(
            QUERY_REFERENCE
                .iter()
                .map(|entry| format!("`{}` ({}) — {}", entry.param, entry.applies, entry.effect))
                .collect::<Vec<_>>()
        )
    ));

    sections.push(format!(
        "## Finish\n\n{}",
        numbered(&[
            "Request each URL you added and confirm it answers 200 with an image content type."
                .to_string(),
            "Confirm every image is wrapped in the link with `?ref=readme` and carries alt text."
                .to_string(),
            "Confirm you changed nothing else: no reformatting, no reflowed prose, no reordered \
             badges beyond the one you inserted."
                .to_string(),
            format!("Report what you added and where, and link the full report: {report}"),
        ])
    ));

    document(&sections)
}

/// The prompt for a maintainer or organization profile README.
pub fn profile_agent_prompt(
    login: &str,
    site: &str,
    api: &str,
    total_stars: Option<i64>,
    repos: Option<i64>,
) -> String {
    let (site, api) = (origin(site), origin(api));
    let link = readme_link(site, &format!("/{login}"));
    let assets = profile_embed_assets(login);
    let asset = |id: &str| assets.iter().find(|asset| asset.id == id);

    let mut sections = vec![
        format!("# Add gitdebt profile analytics to {login}'s profile README"),
        format!(
            "gitdebt ({site}) renders aggregate public-repository statistics for \
             an account as plain image URLs. No account, token, or GitHub Action \
             is involved. A profile README lives in a repository named after the \
             account itself — `{login}/{login}` for a user; for an organization, \
             a repository named `.github` with the file at `profile/README.md`. \
             Create it if it does not exist."
        ),
    ];

    if let Some(total) = total_stars {
        sections.push(format!(
            "## What gitdebt has measured\n\n- {} stars across {login}'s public \
             repositories{}.\n\nRe-checkable at \
             {api}/api/users/{login}/stats.json.",
            thousands(total),
            match repos.filter(|count| *count > 0) {
                Some(count) => format!(" ({} repositories counted)", thousands(count)),
                None => String::new(),
            }
        ));
    }

    sections.push(
        "## What to add\n\nPaste these as-is; both carry light and dark variants.".to_string(),
    );
    if let Some(card) = asset("card") {
        sections.push(snippet_block(
            api,
            card,
            &link,
            &format!("### 1. Maintainer card — {}", card.placement),
        ));
    }
    if let Some(chart) = asset("chart") {
        sections.push(snippet_block(
            api,
            chart,
            &link,
            &format!("### 2. Aggregate star history — {}", chart.placement),
        ));
    }
    sections.push(format!(
        "### 3. Optional footprint charts\n\n{}",
        asset_bullets(
            api,
            assets
                .iter()
                .filter(|asset| asset.group == EmbedGroup::Health)
        )
    ));
    sections.push(format!("## Rules\n\n{}", bullet(EMBED_RULES)));
    sections.push(format!(
        "## Finish\n\n{}",
        numbered(&[
            "Request each URL and confirm it answers 200 with an image content type.".to_string(),
            "Confirm every image keeps its link wrapper and alt text.".to_string(),
            format!("Report what you added, and link the full report: {site}/{login}"),
        ])
    ));

    document(&sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "https://gitdebt.com";
    const API: &str = "https://api.gitdebt.com";

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    /// An unmeasured repository must not receive an invented figure — the
    /// prompt sends the agent to the live JSON instead.
    #[test]
    fn unmeasured_repository_prints_no_star_figure() {
        let prompt = repo_agent_prompt("owner/repo", SITE, API, None);
        assert!(!prompt.contains("GitHub stars"));
        assert!(!prompt.contains("## What gitdebt has measured"));
        assert!(prompt.contains("## Numbers"));
        assert!(
            prompt
                .contains("read it from https://api.gitdebt.com/api/repos/owner/repo/health.json.")
        );

        // An all-empty summary is still not a zero: nothing was measured.
        let empty = StarSummary {
            total_stars: None,
            gained_30: None,
            gained_90: None,
            trend: None,
            first_star_month: None,
            approximate: false,
        };
        let prompt = repo_agent_prompt("owner/repo", SITE, API, Some(&empty));
        assert!(!prompt.contains("GitHub stars"));
        assert!(prompt.contains("## Numbers"));
    }

    #[test]
    fn measured_repository_quotes_the_windows_and_the_pace() {
        let stars = StarSummary {
            total_stars: Some(12_043),
            gained_30: Some(410),
            gained_90: Some(1_204),
            trend: Some(StarTrend::Accelerating),
            first_star_month: Some(day(2013, 3, 9)),
            approximate: false,
        };
        let prompt = repo_agent_prompt("owner/repo", SITE, API, Some(&stars));
        assert!(prompt.contains(
            "- 12,043 GitHub stars (+1,204 in 90 days, +410 in 30), running ahead \
             of its lifetime pace."
        ));
        assert!(prompt.contains("- Star history begins Mar 2013."));
        assert!(!prompt.contains("## Numbers"));
    }

    /// A GH Archive series records star *actions*. Saying so is the difference
    /// between an agent publishing an attention signal and publishing a wrong
    /// net-star claim.
    #[test]
    fn approximate_series_is_never_described_as_net_stars() {
        let stars = StarSummary {
            total_stars: Some(4_210),
            gained_30: None,
            gained_90: None,
            trend: None,
            first_star_month: None,
            approximate: true,
        };
        let prompt = repo_agent_prompt("owner/repo", SITE, API, Some(&stars));
        assert!(prompt.contains("cannot see unstars"));
        assert!(prompt.contains("never as net stars"));

        // The caveat does not depend on a total having been resolved.
        let without_total = StarSummary {
            total_stars: None,
            ..stars
        };
        assert!(
            repo_agent_prompt("owner/repo", SITE, API, Some(&without_total))
                .contains("cannot see unstars")
        );
    }

    #[test]
    fn placeholder_slug_opens_by_resolving_the_remote() {
        let prompt = repo_agent_prompt(PLACEHOLDER_SLUG, SITE, API, None);
        assert!(prompt.contains("# Add gitdebt analytics to the project's README"));
        assert!(prompt.contains("Run `git remote get-url origin`"));
        assert!(prompt.contains("Replace every `OWNER/REPO` below"));
        // The snippets still resolve, so a human can eyeball the shape.
        assert!(prompt.contains("/api/repos/OWNER/REPO/chart.svg?theme=dark"));

        assert!(!repo_agent_prompt("owner/repo", SITE, API, None).contains("## Step 0"));
    }

    #[test]
    fn attribution_rides_the_link_and_never_an_image_url() {
        let prompt = repo_agent_prompt("owner/repo", SITE, API, None);
        assert!(prompt.contains("https://gitdebt.com/owner/repo?ref=readme"));
        for image in prompt
            .split(&['"', '(', ')', '`', ' ', '\n'][..])
            .filter(|part| part.starts_with(API))
        {
            for forbidden in ["ref=", "animate=", "render="] {
                assert!(!image.contains(forbidden), "{image} carries {forbidden}");
            }
        }
    }

    #[test]
    fn profile_prompt_reports_totals_and_snippets() {
        let prompt = profile_agent_prompt("owner", SITE, API, Some(90_120), Some(42));
        assert!(prompt.contains(
            "- 90,120 stars across owner's public repositories (42 repositories counted)."
        ));
        assert!(prompt.contains("/api/users/owner/card.svg?theme=dark"));
        assert!(prompt.contains(
            "Report what you added, and link the full report: https://gitdebt.com/owner"
        ));

        // Nothing measured, nothing claimed.
        let bare = profile_agent_prompt("owner", SITE, API, None, None);
        assert!(!bare.contains("## What gitdebt has measured"));

        // A zero repository count is a missing count, not a fact worth printing.
        let no_count = profile_agent_prompt("owner", SITE, API, Some(5), Some(0));
        assert!(no_count.contains("- 5 stars across owner's public repositories."));
    }

    /// This prompt is executed by a coding agent, so a wrong file location is a
    /// wrong `mkdir -p`. An organization profile README lives at
    /// `profile/README.md` inside a repository literally named `.github`; the
    /// path `.github/profile/README.md` names no repository at all.
    #[test]
    fn the_profile_prompt_locates_an_organization_readme_correctly() {
        let prompt = profile_agent_prompt("owner", SITE, API, None, None);
        assert!(prompt.contains(
            "`owner/owner` for a user; for an organization, a repository named \
             `.github` with the file at `profile/README.md`"
        ));
        assert!(!prompt.contains(".github/profile/README.md"));
    }

    #[test]
    fn identical_input_renders_identical_bytes() {
        let stars = StarSummary {
            total_stars: Some(7),
            gained_30: Some(1),
            gained_90: Some(2),
            trend: Some(StarTrend::Steady),
            first_star_month: Some(day(2020, 12, 1)),
            approximate: true,
        };
        assert_eq!(
            repo_agent_prompt("owner/repo", SITE, API, Some(&stars)),
            repo_agent_prompt("owner/repo", SITE, API, Some(&stars))
        );
        // Both origins are normalized upstream; a trailing slash on either must
        // still never reach a URL as `host//path`.
        assert_eq!(
            repo_agent_prompt(
                "owner/repo",
                "https://gitdebt.com/",
                "https://api.gitdebt.com/",
                None
            ),
            repo_agent_prompt("owner/repo", SITE, API, None)
        );
        assert_eq!(
            profile_agent_prompt(
                "owner",
                "https://gitdebt.com/",
                "https://api.gitdebt.com/",
                Some(3),
                None
            ),
            profile_agent_prompt("owner", SITE, API, Some(3), None)
        );
    }

    #[test]
    fn history_windows_anchor_on_the_last_data_point() {
        // Two years of one star per day, then a 90-day burst.
        let mut history = Vec::new();
        let start = day(2023, 1, 1);
        for index in 0..730i64 {
            history.push((start + Duration::days(index), index + 1));
        }
        let last = history.last().expect("history").0;
        for index in 1..=90i64 {
            history.push((last + Duration::days(index), 730 + index * 20));
        }

        let summary = StarSummary::from_history(&history, Some(2_530), false);
        assert_eq!(summary.gained_90, Some(1_800));
        assert_eq!(summary.gained_30, Some(600));
        assert_eq!(summary.trend, Some(StarTrend::Accelerating));
        assert_eq!(summary.first_month_label().as_deref(), Some("Jan 2023"));

        // Too short to judge honestly.
        let short = StarSummary::from_history(&history[..30], Some(30), false);
        assert_eq!(short.trend, None);
        // A window wider than the series counts the whole series, not zero.
        assert_eq!(short.gained_90, Some(30));

        // An empty history yields no figures at all, never zeros.
        let empty = StarSummary::from_history(&[], None, false);
        assert_eq!(empty.gained_30, None);
        assert_eq!(empty.gained_90, None);
        assert_eq!(empty.trend, None);
        assert_eq!(empty.first_month_label(), None);
    }
}
