//! The Markdown behind the non-repository pages: a maintainer profile, a
//! head-to-head comparison, and a curated category.
//!
//! These three used to be prerendered by the static site, one `.md` file per
//! emitted page. Everything Markdown now answers from the API instead, so these
//! renderers are the siblings of [`crate::agent_report`] and read as one
//! document family with it: the same header, the same table helper, the same
//! refusal to print a figure nobody measured.
//!
//! Ported from `frontend/src/lib/agent-markdown.ts` — the content and the
//! editorial voice, not the code. One behavioural change is deliberate: the
//! comparison there could print a confident `0` for a repository whose star
//! history had not landed, because a leg carried its readiness beside its
//! figures and the two could disagree. Here a withheld leg has no figures to
//! print at all ([`ComparisonLeg::stars`] is `None`), so the defect is not
//! reachable rather than merely unlikely.
//!
//! Pure and deterministic: every figure arrives in the argument, nothing reads
//! the clock or the database, so identical inputs render identical bytes.

use crate::agent_embeds::{
    EmbedAsset, EmbedGroup, asset_section, profile_embed_assets, readme_link, rules_section,
};
use crate::agent_markdown::{
    bullet, document, document_header, escape_html_attribute, origin, table, thousands,
};
use crate::agent_prompt::{StarSummary, StarTrend};
use crate::aggregate::MAX_AGGREGATE_REPOS;
use crate::api::MAX_OVERLAY_REPOS;
use crate::catalog::Category;

/// What a cell holds when the figure behind it was never measured. An em dash
/// rather than `0`, `n/a`, or an empty cell: it has to be unmistakable in a row
/// sitting next to a fully measured competitor.
const WITHHELD: &str = "—";

/// A maintainer or organization profile, already resolved by the handler.
#[derive(Debug, Clone)]
pub struct ProfileView {
    pub login: String,
    pub total_stars: Option<i64>,
    /// Repositories whose star history is complete and therefore summed above.
    pub repos_included: Option<i64>,
    /// Public repositories the account owns, when known. Larger than
    /// `repos_included` means the total above is a floor over a capped sample.
    pub repos_total: Option<i64>,
    /// Repositories still being measured.
    pub repos_pending: Option<i64>,
    pub first_year: Option<i32>,
}

/// One side of a comparison. `stars` is `None` when the star history is not
/// complete, and that withholds every figure in the row — an empty history
/// totals zero, and a zero printed beside a measured competitor is the most
/// damaging claim this document could make.
#[derive(Debug, Clone)]
pub struct ComparisonLeg {
    pub slug: String,
    pub stars: Option<StarSummary>,
}

pub fn render_profile(view: &ProfileView, site: &str, api: &str) -> String {
    let (site, api) = (origin(site), origin(api));
    let login = &view.login;
    let assets = profile_embed_assets(login);
    let link = readme_link(site, &format!("/{login}"));

    let mut sections = vec![document_header(
        &format!("{login} — public GitHub profile statistics"),
        &format!("{site}/{login}"),
    )];
    sections.push(format!(
        "Aggregate statistics across the public repositories owned by `{login}`. \
         Private repositories are never analyzed or counted."
    ));

    match snapshot_rows(view) {
        Some(rows) => {
            sections.push(format!(
                "## Snapshot\n\n{}",
                table(&["Metric", "Value"], &rows)
            ));
            // The sum is over whatever has landed, and a large account lands in
            // batches. Without this the same page prints a growing number every
            // few minutes and each one reads as final.
            if let Some(pending) = view.repos_pending.filter(|pending| *pending > 0) {
                sections.push(format!(
                    "{} of the repositories in that slice are still being \
                     measured, so the figures above are a floor over what has \
                     landed so far, not a settled total for the account. They \
                     grow as the remaining histories arrive.",
                    thousands(pending)
                ));
            }
        }
        None => sections.push(format!(
            "## Snapshot\n\nNo aggregate figures have been published for \
             `{login}` yet, and an absent figure here does not mean zero. \
             Request {api}/api/users/{login}/analyze to start the aggregation, \
             then request this URL again."
        )),
    }

    sections.push(format!(
        "## Put this in a profile README\n\nA profile README lives in a \
         repository named after the account itself — `{login}/{login}` for a \
         user; for an organization, a repository named `.github` with the file \
         at `profile/README.md`. Create it if it does not exist. Every URL \
         below is a plain public image: no account, token, build step, or \
         GitHub Action."
    ));
    for asset in assets
        .iter()
        .filter(|asset| asset.group == EmbedGroup::Headline)
    {
        sections.push(asset_section(api, asset, &link));
    }
    sections.push(format!(
        "### Footprint charts\n\nSame `<picture>` shape as above, different \
         path.\n\n{}",
        asset_table(api, &assets, EmbedGroup::Health)
    ));
    for asset in assets
        .iter()
        .filter(|asset| asset.group == EmbedGroup::Social)
    {
        sections.push(asset_section(api, asset, &link));
    }

    sections.push(rules_section());
    sections.push(format!(
        "## Live data\n\n{}",
        bullet(&[
            format!("GitHub profile: https://github.com/{login}"),
            format!("Aggregate analysis: {api}/api/users/{login}/analyze"),
            format!("Profile statistics JSON: {api}/api/users/{login}/stats.json"),
        ])
    ));

    document(&sections)
}

/// The snapshot table, or `None` when there is nothing measured to put in it.
///
/// A zero repository count collapses the whole table rather than printing a
/// `0`: an account the aggregation has not reached and an account with no
/// public repositories are indistinguishable here, and every other row is a
/// sum over those same zero repositories.
///
/// Where the sum covers only part of the account, every row says so. The
/// aggregate is capped at [`MAX_AGGREGATE_REPOS`] by stars and grows as
/// histories land, so an organization with two hundred repositories and three
/// measured ones would otherwise publish a bare, confident figure that is off
/// by two orders of magnitude.
fn snapshot_rows(view: &ProfileView) -> Option<Vec<Vec<String>>> {
    let included = view.repos_included?;
    if included == 0 {
        return None;
    }
    let uncounted = view.repos_total.filter(|total| *total > included);
    let partial = uncounted.is_some() || view.repos_pending.is_some_and(|pending| pending > 0);

    let mut rows = Vec::new();
    if let Some(stars) = view.total_stars {
        rows.push(vec![
            if partial {
                "Stars across counted repositories"
            } else {
                "Stars across public repositories"
            }
            .to_string(),
            thousands(stars),
        ]);
    }
    rows.push(vec![
        "Repositories counted".to_string(),
        match uncounted {
            Some(total) => format!(
                "{} of {} (top {MAX_AGGREGATE_REPOS} by stars)",
                thousands(included),
                thousands(total)
            ),
            None => thousands(included),
        },
    ]);
    if let Some(year) = view.first_year {
        rows.push(vec!["Active since".to_string(), year.to_string()]);
    }
    Some(rows)
}

pub fn render_comparison(
    first: &ComparisonLeg,
    second: &ComparisonLeg,
    site: &str,
    api: &str,
) -> String {
    let (site, api) = (origin(site), origin(api));
    let path = format!("/vs/{}/{}", first.slug, second.slug);
    let link = readme_link(site, &path);

    let mut sections = vec![document_header(
        &format!(
            "{} versus {} — GitHub star history compared",
            first.slug, second.slug
        ),
        &format!("{site}{path}"),
    )];
    sections.push(
        "Star history and growth for two public GitHub repositories on one \
         timeline, from gitdebt's Postgres cache. Private repositories are \
         never analyzed or counted."
            .to_string(),
    );
    sections.push(format!(
        "## Star comparison\n\n{}",
        table(
            &[
                "Repository",
                "Stars",
                "Trailing 90d",
                "Trailing 30d",
                "Pace",
                "History from",
            ],
            &[comparison_row(first), comparison_row(second)]
        )
    ));

    let withheld: Vec<&ComparisonLeg> = [first, second]
        .into_iter()
        .filter(|leg| leg.stars.is_none())
        .collect();
    if !withheld.is_empty() {
        sections.push(format!(
            "A row of `{WITHHELD}` means gitdebt has not published a complete \
             star history for that repository yet. It is a missing measurement, \
             not a zero, and the two rows are not comparable until it lands. \
             Re-check live — that request queues the analysis if nobody has \
             asked for it yet:\n\n{}",
            bullet(
                withheld
                    .iter()
                    .map(|leg| format!("{}: {api}/api/md/{}", leg.slug, leg.slug))
                    .collect::<Vec<_>>()
            )
        ));
    }

    // Comparing a GH Archive activity curve against an exact stargazer snapshot
    // is comparing two different measurements, and the reader has to be told.
    if [first, second]
        .iter()
        .any(|leg| leg.stars.as_ref().is_some_and(|stars| stars.approximate))
    {
        sections.push(
            "At least one series is public GH Archive star *activity*: it \
             records star actions and cannot see unstars, so it is an attention \
             signal rather than a net-star curve. Say so before comparing it to \
             an exact stargazer count."
                .to_string(),
        );
    }

    let query = format!(
        "repos={}",
        encode_query_value(&format!("{},{}", first.slug, second.slug))
    );
    sections.push(format!(
        "## Overlay chart\n\nOne chart, both series. Append `&rebase=1` to start \
         each series at zero when the projects are different ages, or \
         `&from=`/`&to=` for a window.\n\n```html\n{}\n```",
        overlay_snippet(
            api,
            &query,
            &link,
            &format!("Star history of {} versus {}", first.slug, second.slug)
        )
    ));

    sections.push(format!(
        "## Individual reports\n\n{}",
        bullet([first, second].iter().map(|leg| format!(
            "{}: {site}/{} (Markdown: {api}/api/md/{})",
            leg.slug, leg.slug, leg.slug
        )))
    ));
    sections.push(rules_section());

    document(&sections)
}

/// One repository's star evidence. Built from the leg's own figures or from
/// nothing at all — there is no path here that reads a number out of an absent
/// summary, which is what makes a fabricated `0` unreachable.
fn comparison_row(leg: &ComparisonLeg) -> Vec<String> {
    let mut row = vec![format!("`{}`", leg.slug)];
    let Some(stars) = &leg.stars else {
        row.extend(std::iter::repeat_n(WITHHELD.to_string(), 5));
        return row;
    };
    let withheld = || WITHHELD.to_string();
    row.push(stars.total_stars.map(thousands).unwrap_or_else(withheld));
    row.push(
        stars
            .gained_90
            .map(|gained| format!("+{}", thousands(gained)))
            .unwrap_or_else(withheld),
    );
    row.push(
        stars
            .gained_30
            .map(|gained| format!("+{}", thousands(gained)))
            .unwrap_or_else(withheld),
    );
    row.push(
        stars
            .trend
            .map(|trend| {
                match trend {
                    StarTrend::Accelerating => "ahead of lifetime pace",
                    StarTrend::Steady => "in line with lifetime pace",
                    StarTrend::Slowing => "below lifetime pace",
                }
                .to_string()
            })
            .unwrap_or_else(withheld),
    );
    row.push(stars.first_month_label().unwrap_or_else(withheld));
    row
}

pub fn render_category(category: &Category, site: &str, api: &str) -> String {
    let (site, api) = (origin(site), origin(api));
    let path = format!("/compare/{}", category.slug);
    let link = readme_link(site, &path);

    let mut sections = vec![document_header(
        &format!("{} — GitHub repository comparison", category.name),
        &format!("{site}{path}"),
    )];
    sections.push(category.short.clone());

    if category.repos.is_empty() {
        sections.push(
            "This category carries no repositories. Nothing is being compared, \
             and no figures are implied."
                .to_string(),
        );
        return document(&sections);
    }

    // No per-repository figures: none were supplied, and a comparison page that
    // invents them is worse than one that links out for them.
    sections.push(format!(
        "## Repositories in this category\n\n{}",
        table(
            &["Repository", "Report", "Markdown"],
            &category
                .repos
                .iter()
                .map(|slug| vec![
                    format!("`{slug}`"),
                    format!("{site}/{slug}"),
                    format!("{api}/api/md/{slug}"),
                ])
                .collect::<Vec<_>>()
        )
    ));

    // `/api/chart.svg` dedups the slug list *before* it caps it, so capping
    // first would let a duplicate inside the first `MAX_OVERLAY_REPOS` cost the
    // chart a series the prose below has already counted. Authored order is
    // editorial and survives; only the repeat is dropped.
    let mut seen = std::collections::HashSet::new();
    let distinct: Vec<&str> = category
        .repos
        .iter()
        .filter(|slug| seen.insert(slug.to_ascii_lowercase()))
        .map(String::as_str)
        .collect();
    let charted = &distinct[..distinct.len().min(MAX_OVERLAY_REPOS)];
    let query = format!("repos={}&rebase=1", encode_query_value(&charted.join(",")));
    sections.push(format!(
        "## Overlay every repository on one chart\n\nOne chart, {} series, \
         rebased so projects of different ages start together.{}\n\n```html\n{}\n```",
        charted.len(),
        if charted.len() < distinct.len() {
            format!(
                " The overlay endpoint accepts at most {MAX_OVERLAY_REPOS} \
                 repositories, so it carries the first {} of this category's {}.",
                charted.len(),
                distinct.len()
            )
        } else {
            String::new()
        },
        overlay_snippet(
            api,
            &query,
            &link,
            &format!("{} star history comparison", category.name)
        )
    ));
    sections.push(rules_section());
    sections.push(
        "Public repositories only. Open the canonical HTML page for the \
         interactive timeline and the per-repository health columns."
            .to_string(),
    );

    document(&sections)
}

/// The assets in one group as a reference table rather than snippets, for the
/// long tail nobody pastes all of.
fn asset_table(api: &str, assets: &[EmbedAsset], group: EmbedGroup) -> String {
    table(
        &["Chart", "URL", "Shows"],
        &assets
            .iter()
            .filter(|asset| asset.group == group)
            .map(|asset| {
                vec![
                    asset.name.to_string(),
                    format!("`{api}{}`", asset.path),
                    asset.purpose.to_string(),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

/// The multi-repository overlay, in the same `<picture>` shape as every other
/// published snippet. `/api/chart.svg` has no [`EmbedAsset`] because it is not
/// tied to one entity, but a README embedding it still needs both themes, alt
/// text, and the attributed link around it.
///
/// `alt` is caller-supplied — a hand-authored category name, four of which
/// carry an ampersand today — so it is escaped on its way into the attribute.
/// This snippet is published under prose telling a reader to paste it into a
/// README; invalid HTML here is invalid HTML there.
fn overlay_snippet(api: &str, query: &str, link: &str, alt: &str) -> String {
    let url = format!("{api}/api/chart.svg?{query}");
    let alt = escape_html_attribute(alt);
    [
        format!("<a href=\"{link}\">"),
        "  <picture>".to_string(),
        format!(
            "    <source media=\"(prefers-color-scheme: dark)\" srcset=\"{url}&theme=dark\" />"
        ),
        format!("    <img alt=\"{alt}\" src=\"{url}&theme=light\" />"),
        "  </picture>".to_string(),
        "</a>".to_string(),
    ]
    .join("\n")
}

/// Percent-encode a query value. Slugs reaching here are already validated, but
/// the encoder is total rather than a `,`-and-`/` replacement so a surprising
/// byte can never terminate the query it sits in.
fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    const SITE: &str = "https://gitdebt.com";
    const API: &str = "https://api.gitdebt.com";

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    fn measured() -> StarSummary {
        StarSummary {
            total_stars: Some(12_043),
            gained_30: Some(410),
            gained_90: Some(1_204),
            trend: Some(StarTrend::Accelerating),
            first_star_month: Some(day(2013, 3, 9)),
            approximate: false,
        }
    }

    fn category() -> Category {
        Category {
            slug: "frontend-frameworks".to_string(),
            name: "Frontend frameworks".to_string(),
            short: "React, Vue, Svelte, and the rest, on one star-history timeline.".to_string(),
            repos: vec!["facebook/react".to_string(), "vuejs/vue".to_string()],
        }
    }

    #[test]
    fn profile_renders_the_snapshot_snippets_and_surfaces() {
        let rendered = render_profile(
            &ProfileView {
                login: "owner".to_string(),
                total_stars: Some(90_120),
                repos_included: Some(42),
                repos_total: Some(42),
                repos_pending: Some(0),
                first_year: Some(2013),
            },
            SITE,
            API,
        );
        let expected = r##"# owner — public GitHub profile statistics

Canonical HTML: https://gitdebt.com/owner

Aggregate statistics across the public repositories owned by `owner`. Private repositories are never analyzed or counted.

## Snapshot

| Metric | Value |
| --- | --- |
| Stars across public repositories | 90,120 |
| Repositories counted | 42 |
| Active since | 2013 |

## Put this in a profile README

A profile README lives in a repository named after the account itself — `owner/owner` for a user; for an organization, a repository named `.github` with the file at `profile/README.md`. Create it if it does not exist. Every URL below is a plain public image: no account, token, build step, or GitHub Action.

### Maintainer card

Aggregate public-repository totals for the account in one compact panel.

Goes in the top of a profile README, under the introduction.

```html
<a href="https://gitdebt.com/owner?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/owner/card.svg?theme=dark" />
    <img alt="owner maintainer statistics" src="https://api.gitdebt.com/api/users/owner/card.svg?theme=light" />
  </picture>
</a>
```

### Aggregate star history

One curve summing star growth across every public repository owned.

Goes in a profile README, below the card.

```html
<a href="https://gitdebt.com/owner?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/owner/chart.svg?theme=dark" />
    <img alt="Aggregate star history across owner's public repositories" src="https://api.gitdebt.com/api/users/owner/chart.svg?theme=light" />
  </picture>
</a>
```

### Footprint charts

Same `<picture>` shape as above, different path.

| Chart | URL | Shows |
| --- | --- | --- |
| Contribution footprint | `https://api.gitdebt.com/api/users/owner/stats/contributions.svg` | Authored work in owned projects versus other people's projects. |
| Language footprint | `https://api.gitdebt.com/api/users/owner/stats/languages.svg` | Lines of code by language across every analyzed owned repository. |
| Commit activity | `https://api.gitdebt.com/api/users/owner/stats/commit-activity.svg` | Every commit landed in the last 52 weeks, summed across owned repos. |

### Social preview

A 1200x630 PNG for link unfurls.

Goes in a personal site's `og:image` meta tag.

```markdown
[![owner on gitdebt](https://api.gitdebt.com/api/users/owner/og.png)](https://gitdebt.com/owner?ref=readme)
```

## Embedding rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static. Motion is opt-in: add `animate=1` to an SVG URL, or use the `.gif` variant where one exists, because GitHub strips SVG animation from README images in several contexts.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

## Live data

- GitHub profile: https://github.com/owner
- Aggregate analysis: https://api.gitdebt.com/api/users/owner/analyze
- Profile statistics JSON: https://api.gitdebt.com/api/users/owner/stats.json
"##;
        assert_eq!(rendered, expected);
    }

    /// Nothing aggregated, nothing claimed — and a count of zero repositories
    /// collapses the whole table rather than publishing a sum over nothing.
    #[test]
    fn profile_without_figures_prints_no_zero() {
        for view in [
            ProfileView {
                login: "owner".to_string(),
                total_stars: None,
                repos_included: None,
                repos_total: None,
                repos_pending: None,
                first_year: None,
            },
            // Nothing measured yet, and repositories still draining: the
            // denominators must not conjure a table with no figures in it.
            ProfileView {
                login: "owner".to_string(),
                total_stars: Some(0),
                repos_included: Some(0),
                repos_total: Some(200),
                repos_pending: Some(50),
                first_year: None,
            },
        ] {
            let rendered = render_profile(&view, SITE, API);
            assert!(rendered.contains("No aggregate figures have been published"));
            assert!(rendered.contains("an absent figure here does not mean zero"));
            assert!(!rendered.contains("Stars across public repositories"));
            assert!(!rendered.contains("| 0 |"));
        }
    }

    /// The aggregate is a top-[`MAX_AGGREGATE_REPOS`] slice that fills in over
    /// several requests, so a large organization's sum is a floor. A bare
    /// `Repositories counted | 42` under a lead about "the public repositories
    /// owned by X" is the same confident-number defect as printing 0 stars for
    /// an unfetched repository, one surface over.
    #[test]
    fn a_partially_covered_profile_never_prints_a_bare_count() {
        let rendered = render_profile(
            &ProfileView {
                login: "bigcorp".to_string(),
                total_stars: Some(1_204),
                repos_included: Some(42),
                repos_total: Some(2_913),
                repos_pending: Some(8),
                first_year: Some(2013),
            },
            SITE,
            API,
        );
        assert!(rendered.contains("| Repositories counted | 42 of 2,913 (top 50 by stars) |"));
        assert!(!rendered.contains("| Repositories counted | 42 |"));
        // The star row must not read as a settled account-wide total.
        assert!(rendered.contains("| Stars across counted repositories | 1,204 |"));
        assert!(!rendered.contains("Stars across public repositories"));
        assert!(rendered.contains(
            "8 of the repositories in that slice are still being measured, so \
             the figures above are a floor"
        ));

        // Pending work alone withholds the settled label, even for an account
        // small enough that the cap never bites.
        let draining = render_profile(
            &ProfileView {
                login: "owner".to_string(),
                total_stars: Some(90),
                repos_included: Some(3),
                repos_total: Some(3),
                repos_pending: Some(1),
                first_year: None,
            },
            SITE,
            API,
        );
        assert!(draining.contains("| Repositories counted | 3 |"));
        assert!(draining.contains("| Stars across counted repositories | 90 |"));
        assert!(draining.contains("1 of the repositories in that slice"));

        // Full coverage keeps the settled wording and adds no floor sentence.
        let complete = render_profile(
            &ProfileView {
                login: "owner".to_string(),
                total_stars: Some(90),
                repos_included: Some(3),
                repos_total: Some(3),
                repos_pending: Some(0),
                first_year: None,
            },
            SITE,
            API,
        );
        assert!(complete.contains("| Stars across public repositories | 90 |"));
        assert!(!complete.contains("still being measured"));
    }

    #[test]
    fn comparison_renders_both_legs_and_the_overlay() {
        let rendered = render_comparison(
            &ComparisonLeg {
                slug: "facebook/react".to_string(),
                stars: Some(measured()),
            },
            &ComparisonLeg {
                slug: "vuejs/vue".to_string(),
                stars: Some(StarSummary {
                    total_stars: Some(207_900),
                    gained_30: Some(120),
                    gained_90: Some(300),
                    trend: Some(StarTrend::Slowing),
                    first_star_month: Some(day(2014, 2, 11)),
                    approximate: true,
                }),
            },
            SITE,
            API,
        );
        let expected = r##"# facebook/react versus vuejs/vue — GitHub star history compared

Canonical HTML: https://gitdebt.com/vs/facebook/react/vuejs/vue

Star history and growth for two public GitHub repositories on one timeline, from gitdebt's Postgres cache. Private repositories are never analyzed or counted.

## Star comparison

| Repository | Stars | Trailing 90d | Trailing 30d | Pace | History from |
| --- | --- | --- | --- | --- | --- |
| `facebook/react` | 12,043 | +1,204 | +410 | ahead of lifetime pace | Mar 2013 |
| `vuejs/vue` | 207,900 | +300 | +120 | below lifetime pace | Feb 2014 |

At least one series is public GH Archive star *activity*: it records star actions and cannot see unstars, so it is an attention signal rather than a net-star curve. Say so before comparing it to an exact stargazer count.

## Overlay chart

One chart, both series. Append `&rebase=1` to start each series at zero when the projects are different ages, or `&from=`/`&to=` for a window.

```html
<a href="https://gitdebt.com/vs/facebook/react/vuejs/vue?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/chart.svg?repos=facebook%2Freact%2Cvuejs%2Fvue&theme=dark" />
    <img alt="Star history of facebook/react versus vuejs/vue" src="https://api.gitdebt.com/api/chart.svg?repos=facebook%2Freact%2Cvuejs%2Fvue&theme=light" />
  </picture>
</a>
```

## Individual reports

- facebook/react: https://gitdebt.com/facebook/react (Markdown: https://api.gitdebt.com/api/md/facebook/react)
- vuejs/vue: https://gitdebt.com/vuejs/vue (Markdown: https://api.gitdebt.com/api/md/vuejs/vue)

## Embedding rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static. Motion is opt-in: add `animate=1` to an SVG URL, or use the `.gif` variant where one exists, because GitHub strips SVG animation from README images in several contexts.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.
"##;
        assert_eq!(rendered, expected);
    }

    /// The defect this whole change set exists to remove: the TypeScript
    /// comparison printed a measured-looking `0` for a repository whose star
    /// history had not landed. A withheld leg holds no figures at all.
    #[test]
    fn withheld_comparison_leg_never_prints_a_zero() {
        let rendered = render_comparison(
            &ComparisonLeg {
                slug: "facebook/react".to_string(),
                stars: Some(measured()),
            },
            &ComparisonLeg {
                slug: "owner/queued".to_string(),
                stars: None,
            },
            SITE,
            API,
        );
        assert!(rendered.contains("| `owner/queued` | — | — | — | — | — |"));
        for cell in ["| 0 |", "| +0 |", "| 0.", "| 0 ", " 0 |"] {
            assert!(!rendered.contains(cell), "withheld leg printed {cell}");
        }
        assert!(rendered.contains("It is a missing measurement, not a zero"));
        assert!(rendered.contains("- owner/queued: https://api.gitdebt.com/api/md/owner/queued"));
        // Only the withheld leg is named for a re-check.
        assert!(!rendered.contains("- facebook/react: https://api.gitdebt.com/api/md/"));
        // A measured leg is still fully reported beside it.
        assert!(rendered.contains("| `facebook/react` | 12,043 |"));
    }

    /// A summary that exists but has measured nothing is not a zero either.
    #[test]
    fn empty_summary_withholds_every_figure() {
        let rendered = render_comparison(
            &ComparisonLeg {
                slug: "owner/one".to_string(),
                stars: Some(StarSummary {
                    total_stars: None,
                    gained_30: None,
                    gained_90: None,
                    trend: None,
                    first_star_month: None,
                    approximate: false,
                }),
            },
            &ComparisonLeg {
                slug: "owner/two".to_string(),
                stars: None,
            },
            SITE,
            API,
        );
        assert!(rendered.contains("| `owner/one` | — | — | — | — | — |"));
        assert!(!rendered.contains("| 0 |"));
        // The approximate caveat belongs only to a series that is approximate.
        assert!(!rendered.contains("GH Archive"));
    }

    #[test]
    fn category_lists_members_and_one_overlay() {
        let rendered = render_category(&category(), SITE, API);
        let expected = r##"# Frontend frameworks — GitHub repository comparison

Canonical HTML: https://gitdebt.com/compare/frontend-frameworks

React, Vue, Svelte, and the rest, on one star-history timeline.

## Repositories in this category

| Repository | Report | Markdown |
| --- | --- | --- |
| `facebook/react` | https://gitdebt.com/facebook/react | https://api.gitdebt.com/api/md/facebook/react |
| `vuejs/vue` | https://gitdebt.com/vuejs/vue | https://api.gitdebt.com/api/md/vuejs/vue |

## Overlay every repository on one chart

One chart, 2 series, rebased so projects of different ages start together.

```html
<a href="https://gitdebt.com/compare/frontend-frameworks?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/chart.svg?repos=facebook%2Freact%2Cvuejs%2Fvue&rebase=1&theme=dark" />
    <img alt="Frontend frameworks star history comparison" src="https://api.gitdebt.com/api/chart.svg?repos=facebook%2Freact%2Cvuejs%2Fvue&rebase=1&theme=light" />
  </picture>
</a>
```

## Embedding rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static. Motion is opt-in: add `animate=1` to an SVG URL, or use the `.gif` variant where one exists, because GitHub strips SVG animation from README images in several contexts.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

Public repositories only. Open the canonical HTML page for the interactive timeline and the per-repository health columns.
"##;
        assert_eq!(rendered, expected);
    }

    /// The overlay endpoint drops slugs past its ceiling, so the snippet says
    /// what it actually renders instead of naming series it will not draw.
    #[test]
    fn oversized_category_truncates_the_overlay_and_says_so() {
        let mut oversized = category();
        oversized.repos = (0..15).map(|index| format!("owner/repo{index}")).collect();
        let rendered = render_category(&oversized, SITE, API);
        assert!(rendered.contains("One chart, 12 series"));
        assert!(rendered.contains("carries the first 12 of this category's 15"));
        assert!(rendered.contains("owner%2Frepo11"));
        assert!(!rendered.contains("owner%2Frepo12"));
        // Every member is still listed and linked.
        assert!(rendered.contains("| `owner/repo14` |"));
    }

    /// A category with no members states that and stops, rather than emitting a
    /// `repos=` URL that would 400.
    #[test]
    fn empty_category_publishes_no_overlay() {
        let mut empty = category();
        empty.repos.clear();
        let rendered = render_category(&empty, SITE, API);
        assert!(rendered.contains("This category carries no repositories."));
        assert!(!rendered.contains("/api/chart.svg"));
    }

    #[test]
    fn identical_input_renders_identical_bytes() {
        let view = ProfileView {
            login: "owner".to_string(),
            total_stars: Some(7),
            repos_included: Some(2),
            repos_total: Some(2),
            repos_pending: Some(0),
            first_year: Some(2020),
        };
        // Both origins are normalized upstream; a trailing slash on either must
        // still never reach a URL as `host//path`.
        assert_eq!(
            render_profile(&view, SITE, API),
            render_profile(&view, "https://gitdebt.com/", "https://api.gitdebt.com/")
        );
        let leg = |slug: &str| ComparisonLeg {
            slug: slug.to_string(),
            stars: Some(measured()),
        };
        assert_eq!(
            render_comparison(&leg("a/b"), &leg("c/d"), SITE, API),
            render_comparison(&leg("a/b"), &leg("c/d"), SITE, API)
        );
        assert_eq!(
            render_category(&category(), SITE, API),
            render_category(&category(), SITE, API)
        );
    }

    /// Attribution rides the link; an image URL that carries it, or motion
    /// nobody asked for, breaks CDN caching or publishes the wrong asset.
    #[test]
    fn no_image_url_carries_attribution_or_motion() {
        let documents = [
            render_profile(
                &ProfileView {
                    login: "owner".to_string(),
                    total_stars: Some(1),
                    repos_included: Some(1),
                    repos_total: Some(1),
                    repos_pending: Some(0),
                    first_year: Some(2020),
                },
                SITE,
                API,
            ),
            render_comparison(
                &ComparisonLeg {
                    slug: "a/b".to_string(),
                    stars: Some(measured()),
                },
                &ComparisonLeg {
                    slug: "c/d".to_string(),
                    stars: None,
                },
                SITE,
                API,
            ),
            render_category(&category(), SITE, API),
        ];
        for document in &documents {
            for image in document
                .split(['"', '(', ')', '`', ' ', '\n'])
                .filter(|part| part.starts_with(API))
            {
                for forbidden in ["ref=", "animate=", "render=", "v="] {
                    assert!(!image.contains(forbidden), "{image} carries {forbidden}");
                }
            }
            assert!(document.contains("?ref=readme"));
        }
    }

    /// A category name is hand-authored editorial text and reaches an `alt`
    /// attribute in a snippet the document tells the reader to paste into a
    /// README. Four live categories carry an ampersand; a quote would break out
    /// of the attribute and take the rest of the snippet with it.
    #[test]
    fn a_category_name_never_breaks_the_snippet_it_names() {
        let mut awkward = category();
        awkward.name = r#"Terminals & "multiplexers""#.to_string();
        let rendered = render_category(&awkward, SITE, API);
        assert!(rendered.contains(
            r#"<img alt="Terminals &amp; &quot;multiplexers&quot; star history comparison""#
        ));
        // The heading is Markdown, not an attribute, and stays readable.
        assert!(rendered.starts_with(r#"# Terminals & "multiplexers" — GitHub"#));
    }

    /// `/api/chart.svg` dedups before it caps. Capping first would publish a
    /// URL drawing eleven series under prose promising twelve.
    #[test]
    fn a_duplicate_slug_does_not_cost_the_overlay_a_series() {
        /// The slug list the published chart URL actually carries.
        fn charted_slugs(rendered: &str) -> Vec<&str> {
            rendered
                .split("repos=")
                .nth(1)
                .expect("an overlay query")
                .split(['&', '"'])
                .next()
                .expect("the repos value")
                .split("%2C")
                .collect()
        }

        // The repeat sits inside the cap, which is the only place capping
        // before deduping can cost the chart a series. `/api/chart.svg`
        // lowercases before it compares, so a differently cased repeat counts.
        let mut duplicated = category();
        duplicated.repos = ["owner/dup".to_string(), "OWNER/Dup".to_string()]
            .into_iter()
            .chain((0..12).map(|index| format!("owner/repo{index}")))
            .collect();
        let rendered = render_category(&duplicated, SITE, API);
        assert_eq!(charted_slugs(&rendered).len(), 12);
        assert!(rendered.contains("One chart, 12 series"));
        assert!(rendered.contains("carries the first 12 of this category's 13"));
        // The freed slot went to a real member, not to the repeat.
        assert!(rendered.contains("owner%2Frepo10"));
        assert!(!rendered.contains("owner%2Frepo11"));

        duplicated.repos = vec!["owner/a".to_string(), "owner/a".to_string()];
        let rendered = render_category(&duplicated, SITE, API);
        assert_eq!(charted_slugs(&rendered), vec!["owner%2Fa"]);
        assert!(rendered.contains("One chart, 1 series"));
        // Authored order is editorial, so the member list keeps every row.
        assert_eq!(rendered.matches("| `owner/a` |").count(), 2);
    }
}
