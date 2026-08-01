//! The Markdown documents that are not about one repository: the home page,
//! the site's static pages, and the complete README embed catalog.
//!
//! A port of `staticMarkdown` / `badgeCatalogMarkdown` in
//! `frontend/src/lib/agent-markdown.ts` plus `frontend/src/pages/index.md.ts`.
//! The static site no longer emits `.md` at all; `<page>.md` redirects here, so
//! these renderers are the only surface serving them and the wording is the
//! wording that shipped.
//!
//! `/badges` is the one document worth an agent's full attention: every asset
//! gitdebt can embed, each as a headed paste-ready snippet, the query reference,
//! and the generic agent prompt whole. Per-repository reports point here instead
//! of repeating it thousands of times.
//!
//! Pure and deterministic: origins in, bytes out. Nothing here reads the clock
//! or the database.

use crate::agent_embeds::{
    QUERY_REFERENCE, asset_section, profile_embed_assets, readme_link, repo_embed_assets,
    rules_section,
};
use crate::agent_markdown::{bullet, document, document_header, fence, origin, outer_fence, table};
use crate::agent_prompt::{PLACEHOLDER_SLUG, repo_agent_prompt};

/// The account name the catalog's profile snippets are written against, the
/// counterpart of [`PLACEHOLDER_SLUG`] for `/api/users/...` assets.
const PLACEHOLDER_LOGIN: &str = "LOGIN";

/// One page of the static site, in the words its HTML counterpart uses. The
/// list is the frontend's `STATIC_PAGES`, and the titles and descriptions are
/// the ones the HTML `<title>`/`<meta>` carry, so an agent that reads the
/// Markdown and an agent that reads the page agree on what the page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticPage {
    pub path: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

const STATIC_PAGES: &[StaticPage] = &[
    StaticPage {
        path: "404",
        title: "Page not found",
        description: "Search for a public GitHub repository report on gitdebt.",
    },
    StaticPage {
        path: "about",
        title: "About gitdebt",
        description: "How gitdebt collects and presents public GitHub repository analytics.",
    },
    StaticPage {
        path: "badges",
        title: "GitHub repository badges",
        description: "Evidence-backed badges and README media for public GitHub repositories.",
    },
    StaticPage {
        path: "compare",
        title: "Compare GitHub repositories",
        description: "Compare star history and growth for public GitHub repositories.",
    },
    StaticPage {
        path: "leaderboard",
        title: "GitHub repository leaderboard",
        description: "Public repositories ranked by stars and recent growth.",
    },
    StaticPage {
        path: "privacy",
        title: "gitdebt privacy policy",
        description: "The gitdebt privacy and public-data policy.",
    },
    StaticPage {
        path: "profile",
        title: "GitHub profile statistics",
        description: "Open aggregate statistics for a user's public GitHub repositories.",
    },
    StaticPage {
        path: "report",
        title: "GitHub repository report",
        description: "Open a live public-repository analysis.",
    },
    StaticPage {
        path: "terms",
        title: "gitdebt terms",
        description: "Terms for using gitdebt and its public analytics API.",
    },
];

/// The static page at a site path, if there is one.
///
/// Surrounding slashes are tolerated so the route can hand over whatever the
/// URL carried; nothing else is normalized, because a static path that differs
/// in case is a different URL and must not be answered from here.
pub fn static_page(path: &str) -> Option<&'static StaticPage> {
    let path = path.trim_matches('/');
    STATIC_PAGES.iter().find(|page| page.path == path)
}

/// `/` — the entry document. Reached at `/api/md/`, so it names the one URL
/// shape that answers for any public repository rather than only the curated
/// links the HTML home page offers.
pub fn render_home(site: &str, api: &str) -> String {
    let (site, api) = (origin(site), origin(api));
    let sections = [
        document_header("gitdebt", &format!("{site}/")),
        "> Star history and repository-health analytics for public GitHub repositories."
            .to_string(),
        "Gitdebt shows star history, growth, contributors, ownership risk, \
         language activity, file change frequency, fix-labelled changes, \
         maintenance cadence, and README-ready media. Private repositories are \
         never analyzed or counted."
            .to_string(),
        "The homepage carries a live health scorecard: four readings taken from a \
         repository's own commit history — maintenance (commits in the trailing \
         90 days against the 90 before), ownership (how many contributors write \
         half the commits), repair load (the fix-labelled share of file changes), \
         and debt markers (TODO/FIXME movement)."
            .to_string(),
        bullet([
            format!("[Analyze a repository]({site}/report)"),
            format!("[Repository leaderboard]({site}/leaderboard)"),
            format!("[Compare repositories]({site}/compare)"),
            format!("[Badge catalog]({site}/badges)"),
            format!("[About and API behavior]({site}/about)"),
            format!(
                "[Live Markdown report for any public repository]({api}/api/repos/{PLACEHOLDER_SLUG}/report.md)"
            ),
        ]),
    ];
    document(&sections)
}

/// One static page. `/badges` is the catalog rather than a stub, exactly as the
/// prerendered site served it.
pub fn render_static(page: &StaticPage, site: &str, api: &str) -> String {
    if page.path == "badges" {
        return render_badge_catalog(site, api);
    }

    let site = origin(site);
    let sections = [
        document_header(page.title, &format!("{site}/{}", page.path)),
        format!("> {}", page.description),
        "gitdebt reports star history, growth, contributors, ownership \
         concentration, language activity, file change frequency, fix-labelled \
         changes, maintenance cadence, and README-ready media for public GitHub \
         repositories."
            .to_string(),
        bullet([
            format!("Repository report: {site}/report"),
            format!("Repository leaderboard: {site}/leaderboard"),
            format!("Compare repositories: {site}/compare"),
            format!("README asset catalog: {site}/badges.md"),
            format!("API behaviour and methodology: {site}/about"),
            format!("Agent index: {site}/llms.txt"),
        ]),
    ];
    document(&sections)
}

/// `/badges` — every asset, every snippet, in a form an agent can act on
/// without a second request.
pub fn render_badge_catalog(site: &str, api: &str) -> String {
    let (site, api) = (origin(site), origin(api));
    let repo_link = readme_link(site, &format!("/{PLACEHOLDER_SLUG}"));
    let profile_link = readme_link(site, &format!("/{PLACEHOLDER_LOGIN}"));

    let mut sections = vec![
        document_header(
            "Everything gitdebt can embed in a README",
            &format!("{site}/badges"),
        ),
        "> Star-history charts, a metrics badge, evidence-backed signal badges, \
         repository and maintainer cards, eight repository-health charts, and a \
         social preview. Every asset is a plain public image URL."
            .to_string(),
        format!(
            "Replace `{PLACEHOLDER_SLUG}` with a lowercased `owner/repo` slug, \
             and `{PLACEHOLDER_LOGIN}` with a GitHub account name. Nothing else \
             needs to change: no account, no token, no build step, no GitHub \
             Action."
        ),
        rules_section(),
        "## Repository assets".to_string(),
    ];

    for asset in repo_embed_assets(PLACEHOLDER_SLUG) {
        sections.push(asset_section(api, &asset, &repo_link));
    }

    sections.push("## Profile assets".to_string());
    for asset in profile_embed_assets(PLACEHOLDER_LOGIN) {
        sections.push(asset_section(api, &asset, &profile_link));
    }

    sections.push(format!(
        "## Multi-repository overlay\n\nOne chart, several series, for a \
         comparison table or a docs page.\n\n{}",
        fence(
            "markdown",
            &format!(
                "![Star history comparison]({api}/api/chart.svg?repos=owner%2Frepo%2Cother%2Frepo&rebase=1&theme=dark)"
            )
        )
    ));
    sections.push(parameter_section());
    sections.push(
        "## Ready-made agent prompt\n\nThe `/badges` page and every repository \
         report carry an *Ask an agent* button that copies this prompt, filled in \
         for the repository being viewed. The generic form:"
            .to_string(),
    );
    // The prompt carries fenced snippets of its own, so it needs a fence one
    // backtick longer than anything inside it or the catalog ends mid-document.
    sections.push(outer_fence(
        "markdown",
        repo_agent_prompt(PLACEHOLDER_SLUG, site, api, None).trim_end(),
    ));

    document(&sections)
}

fn parameter_section() -> String {
    format!(
        "## Query parameters\n\n{}",
        table(
            &["Parameter", "Applies to", "Effect"],
            &QUERY_REFERENCE
                .iter()
                .map(|entry| vec![
                    format!("`{}`", entry.param),
                    entry.applies.to_string(),
                    entry.effect.to_string(),
                ])
                .collect::<Vec<_>>()
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_embeds::{EmbedGroup, asset_url};

    const SITE: &str = "https://gitdebt.com";
    const API: &str = "https://api.gitdebt.com";

    #[test]
    fn static_pages_resolve_by_exact_path() {
        assert_eq!(
            static_page("about").map(|page| page.title),
            Some("About gitdebt")
        );
        // The route may hand over the path with its slashes still attached.
        assert_eq!(static_page("/terms/"), static_page("terms"));
        // A repository slug must fall through to the repository renderer.
        assert!(static_page("facebook/react").is_none());
        assert!(static_page("About").is_none());
        assert!(static_page("").is_none());
    }

    #[test]
    fn home_document_is_stable() {
        let expected = r##"# gitdebt

Canonical HTML: https://gitdebt.com/

> Star history and repository-health analytics for public GitHub repositories.

Gitdebt shows star history, growth, contributors, ownership risk, language activity, file change frequency, fix-labelled changes, maintenance cadence, and README-ready media. Private repositories are never analyzed or counted.

The homepage carries a live health scorecard: four readings taken from a repository's own commit history — maintenance (commits in the trailing 90 days against the 90 before), ownership (how many contributors write half the commits), repair load (the fix-labelled share of file changes), and debt markers (TODO/FIXME movement).

- [Analyze a repository](https://gitdebt.com/report)
- [Repository leaderboard](https://gitdebt.com/leaderboard)
- [Compare repositories](https://gitdebt.com/compare)
- [Badge catalog](https://gitdebt.com/badges)
- [About and API behavior](https://gitdebt.com/about)
- [Live Markdown report for any public repository](https://api.gitdebt.com/api/repos/OWNER/REPO/report.md)
"##;
        assert_eq!(render_home(SITE, API), expected);
        // A configured origin with a trailing slash must not double it, and
        // that holds for the API origin as much as for the site's.
        assert_eq!(
            render_home("https://gitdebt.com/", "https://api.gitdebt.com/"),
            expected
        );
    }

    #[test]
    fn static_document_is_stable() {
        let expected = r##"# About gitdebt

Canonical HTML: https://gitdebt.com/about

> How gitdebt collects and presents public GitHub repository analytics.

gitdebt reports star history, growth, contributors, ownership concentration, language activity, file change frequency, fix-labelled changes, maintenance cadence, and README-ready media for public GitHub repositories.

- Repository report: https://gitdebt.com/report
- Repository leaderboard: https://gitdebt.com/leaderboard
- Compare repositories: https://gitdebt.com/compare
- README asset catalog: https://gitdebt.com/badges.md
- API behaviour and methodology: https://gitdebt.com/about
- Agent index: https://gitdebt.com/llms.txt
"##;
        let page = static_page("about").expect("about page");
        assert_eq!(render_static(page, SITE, API), expected);
    }

    /// `/badges` is the catalog, not a stub, exactly as the prerendered site
    /// served it — the route dispatches on the page list alone.
    #[test]
    fn badges_static_page_renders_the_catalog() {
        let page = static_page("badges").expect("badges page");
        assert_eq!(
            render_static(page, SITE, API),
            render_badge_catalog(SITE, API)
        );
    }

    /// The catalog's own bytes. The embedded prompt is asserted against
    /// [`repo_agent_prompt`] rather than duplicated into this golden: it must be
    /// that renderer's output verbatim, and a second copy here would only make
    /// one edit fail twice.
    #[test]
    fn badge_catalog_document_is_stable() {
        let rendered = render_badge_catalog(SITE, API);
        let (frame, prompt) = rendered
            .split_once("````markdown\n")
            .expect("the prompt is wrapped in a four-backtick fence");
        assert_eq!(
            frame,
            r##"# Everything gitdebt can embed in a README

Canonical HTML: https://gitdebt.com/badges

> Star-history charts, a metrics badge, evidence-backed signal badges, repository and maintainer cards, eight repository-health charts, and a social preview. Every asset is a plain public image URL.

Replace `OWNER/REPO` with a lowercased `owner/repo` slug, and `LOGIN` with a GitHub account name. Nothing else needs to change: no account, no token, no build step, no GitHub Action.

## Embedding rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static. Motion is opt-in: add `animate=1` to an SVG URL, or use the `.gif` variant where one exists, because GitHub strips SVG animation from README images in several contexts.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

## Repository assets

### Metrics badge

Stars and forks in one compact chip, served from gitdebt's cache.

Goes in the badge row directly under the project title, alongside CI and license badges.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?metrics=stars,forks&theme=dark" />
    <img alt="OWNER/REPO stars and forks" src="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?metrics=stars,forks&theme=light" />
  </picture>
</a>
```

### Earned signal badge

One evidence-backed claim — actively maintained, community powered, star momentum, or contributor readiness.

Goes in the badge row, but only for signals the repository has actually earned.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=active&theme=dark" />
    <img alt="OWNER/REPO actively maintained" src="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=active&theme=light" />
  </picture>
</a>
```

### Star history

The full cumulative star curve, served from Postgres.

Goes in a `## Star history` section near the bottom of the README, above License.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark" />
    <img alt="OWNER/REPO star history" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light" />
  </picture>
</a>
```

### Repository card

Stars, forks, contributors, languages, and a 90-day sparkline in one panel.

Goes in an About or Project status section, or a docs-site sidebar.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=dark" />
    <img alt="OWNER/REPO repository statistics" src="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=light" />
  </picture>
</a>
```

### Stars versus downloads

Star growth against package-registry downloads, for projects that publish one.

Goes in next to the star-history chart, when the project ships a package.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/usage.svg?theme=dark" />
    <img alt="OWNER/REPO stars versus package downloads" src="https://api.gitdebt.com/api/repos/OWNER/REPO/usage.svg?theme=light" />
  </picture>
</a>
```

### Commit calendar

Daily commit density across the last 52 weeks.

Goes in a Project health or Contributing section, where a prospective contributor is already reading.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg?theme=dark" />
    <img alt="OWNER/REPO commit activity calendar" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg?theme=light" />
  </picture>
</a>
```

### Maintenance pulse

Commit volume over time, so a slowdown is visible rather than implied.

Goes in a Project health or Contributing section, where a prospective contributor is already reading.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.svg?theme=dark" />
    <img alt="OWNER/REPO commit trend" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.svg?theme=light" />
  </picture>
</a>
```

### Contributors

Who is actually landing commits, ranked, with avatars inlined.

Goes in a Project health or Contributing section, where a prospective contributor is already reading.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.svg?theme=dark" />
    <img alt="OWNER/REPO contributors" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.svg?theme=light" />
  </picture>
</a>
```

### Ownership concentration

How few people write half the commits.

Goes in a Project health or Contributing section, where a prospective contributor is already reading.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.svg?theme=dark" />
    <img alt="OWNER/REPO bus factor" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.svg?theme=light" />
  </picture>
</a>
```

### Language activity

Lines of code by language across the analyzed history.

Goes in a Project health or Contributing section, where a prospective contributor is already reading.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.svg?theme=dark" />
    <img alt="OWNER/REPO language activity" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.svg?theme=light" />
  </picture>
</a>
```

### File change frequency

The files the most commits touch, dependency manifests excluded.

Goes in a Project health or Contributing section, where a prospective contributor is already reading.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.svg?theme=dark" />
    <img alt="OWNER/REPO file change frequency" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.svg?theme=light" />
  </picture>
</a>
```

### Fix-labelled changes

Files most often touched by commits whose message reads like a fix.

Goes in a Project health or Contributing section, where a prospective contributor is already reading.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.svg?theme=dark" />
    <img alt="OWNER/REPO fix-labelled changes" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.svg?theme=light" />
  </picture>
</a>
```

### TODO/FIXME movement

Whether known debt markers are being added or paid down.

Goes in a Project health or Contributing section, where a prospective contributor is already reading.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.svg?theme=dark" />
    <img alt="OWNER/REPO recent TODO and FIXME movement" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.svg?theme=light" />
  </picture>
</a>
```

### Social preview

A 1200x630 PNG for link unfurls on social platforms and chat apps.

Goes in a docs-site `og:image` meta tag — not the README, where it would be redundant.

```markdown
[![OWNER/REPO on gitdebt](https://api.gitdebt.com/api/repos/OWNER/REPO/og.png)](https://gitdebt.com/OWNER/REPO?ref=readme)
```

## Profile assets

### Maintainer card

Aggregate public-repository totals for the account in one compact panel.

Goes in the top of a profile README, under the introduction.

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=dark" />
    <img alt="LOGIN maintainer statistics" src="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=light" />
  </picture>
</a>
```

### Aggregate star history

One curve summing star growth across every public repository owned.

Goes in a profile README, below the card.

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/chart.svg?theme=dark" />
    <img alt="Aggregate star history across LOGIN's public repositories" src="https://api.gitdebt.com/api/users/LOGIN/chart.svg?theme=light" />
  </picture>
</a>
```

### Contribution footprint

Authored work in owned projects versus other people's projects.

Goes in a profile README, in place of a generic contribution-count widget.

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/stats/contributions.svg?theme=dark" />
    <img alt="LOGIN contribution footprint" src="https://api.gitdebt.com/api/users/LOGIN/stats/contributions.svg?theme=light" />
  </picture>
</a>
```

### Language footprint

Lines of code by language across every analyzed owned repository.

Goes in a profile README, next to the contribution footprint.

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/stats/languages.svg?theme=dark" />
    <img alt="LOGIN language footprint" src="https://api.gitdebt.com/api/users/LOGIN/stats/languages.svg?theme=light" />
  </picture>
</a>
```

### Commit activity

Every commit landed in the last 52 weeks, summed across owned repos.

Goes in a profile README, as the activity strip.

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.svg?theme=dark" />
    <img alt="LOGIN commit activity" src="https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.svg?theme=light" />
  </picture>
</a>
```

### Social preview

A 1200x630 PNG for link unfurls.

Goes in a personal site's `og:image` meta tag.

```markdown
[![LOGIN on gitdebt](https://api.gitdebt.com/api/users/LOGIN/og.png)](https://gitdebt.com/LOGIN?ref=readme)
```

## Multi-repository overlay

One chart, several series, for a comparison table or a docs page.

```markdown
![Star history comparison](https://api.gitdebt.com/api/chart.svg?repos=owner%2Frepo%2Cother%2Frepo&rebase=1&theme=dark)
```

## Query parameters

| Parameter | Applies to | Effect |
| --- | --- | --- |
| `theme=light\|dark` | every SVG and raster asset | Bakes that palette into the output. Default is light. |
| `animate=1` | SVG charts, cards, and badges | Opts into motion. Off by default; use the `.gif` variant where GitHub strips SVG animation. |
| `from=YYYY-MM-DD&to=YYYY-MM-DD` | star-history charts | Inclusive date window. An invalid or inverted range is a 400. |
| `rebase=1` | star-history charts | Starts every series at zero, so projects of different ages compare fairly. |
| `type=date\|timeline` | star-history charts | Calendar dates, or days-since-first-star. |
| `log=1` | star-history charts | Logarithmic y axis. |
| `repos=owner/repo,owner/repo` | /api/chart.svg | Overlays several repositories on one chart. |
| `metrics=stars,forks,downloads` | /badge.svg | Chooses the chips and their order. `downloads` needs a published package. |
| `signal=active\|community\|momentum\|contributor-ready` | /badge.svg | Renders one evidence-backed claim instead of raw metrics. |
| `hide_border=1, hide_title=1, card_width=N` | /card.svg | Trims the card for tight layouts. |

## Ready-made agent prompt

The `/badges` page and every repository report carry an *Ask an agent* button that copies this prompt, filled in for the repository being viewed. The generic form:

"##
        );
        assert_eq!(
            prompt,
            format!(
                "{}\n````\n",
                repo_agent_prompt(PLACEHOLDER_SLUG, SITE, API, None).trim_end()
            )
        );
    }

    /// The embedded prompt carries fenced snippets of its own. If its wrapper
    /// were a three-backtick fence the prompt's first snippet would close it and
    /// the rest of the catalog would render as prose — silently, for every agent
    /// that reads it.
    #[test]
    fn badge_catalog_wraps_the_prompt_in_a_longer_fence() {
        let rendered = render_badge_catalog(SITE, API);
        let outer: Vec<&str> = rendered
            .lines()
            .filter(|line| line.starts_with("````"))
            .collect();
        assert_eq!(outer, vec!["````markdown", "````"]);
        assert!(rendered.ends_with("\n````\n"));

        let prompt = rendered
            .split_once("````markdown\n")
            .expect("outer fence")
            .1;
        // The prompt's three `<picture>` snippets keep their own fences intact.
        assert_eq!(prompt.matches("```html\n<a href=").count(), 3);
        assert_eq!(prompt.matches("\n```\n").count(), 3);
    }

    /// Every asset the catalog is the catalog *of* has to appear in it: a
    /// dropped asset is invisible in a 20-section document.
    #[test]
    fn every_asset_is_published_with_its_snippet() {
        let rendered = render_badge_catalog(SITE, API);
        let assets = repo_embed_assets(PLACEHOLDER_SLUG)
            .into_iter()
            .chain(profile_embed_assets(PLACEHOLDER_LOGIN));
        for asset in assets {
            assert!(
                rendered.contains(&format!("Goes in {}.", asset.placement)),
                "{} lost its placement",
                asset.id
            );
            let theme = if asset.themed { Some("dark") } else { None };
            assert!(
                rendered.contains(&asset_url(API, &asset, theme, None)),
                "{} lost its URL",
                asset.id
            );
            // Attribution rides the link, never the image URL.
            assert!(!asset.path.contains("ref=readme"));
        }
        // Health charts are catalogued in full here, not summarized as a table.
        assert!(
            repo_embed_assets(PLACEHOLDER_SLUG)
                .iter()
                .any(|asset| asset.group == EmbedGroup::Health)
        );
    }

    /// Half the documented parameters carry a pipe in their own syntax, so the
    /// query reference is the one table in this repository whose real data
    /// depends on the shared escaping.
    #[test]
    fn the_query_reference_escapes_its_own_pipes() {
        let rendered = parameter_section();
        assert!(rendered.contains("| `theme=light\\|dark` |"));
        assert!(rendered.contains("| `type=date\\|timeline` |"));
        assert!(!rendered.contains("`theme=light|dark`"));
    }

    #[test]
    fn identical_input_renders_identical_bytes() {
        assert_eq!(render_home(SITE, API), render_home(SITE, API));
        // Both origins are normalized upstream; neither may produce `host//path`.
        assert_eq!(
            render_badge_catalog(SITE, API),
            render_badge_catalog("https://gitdebt.com/", "https://api.gitdebt.com/")
        );
        let page = static_page("privacy").expect("privacy page");
        assert_eq!(
            render_static(page, SITE, API),
            render_static(page, SITE, API)
        );
    }

    /// No origin is baked in: both come from `ApiState`.
    #[test]
    fn origins_come_from_the_caller() {
        let rendered =
            render_badge_catalog("https://staging.example", "https://api.staging.example");
        assert!(!rendered.contains("gitdebt.com"));
        assert!(rendered.contains("Canonical HTML: https://staging.example/badges"));
        assert!(
            render_home("https://staging.example", "https://api.staging.example")
                .contains("https://api.staging.example/api/repos/OWNER/REPO/report.md")
        );
    }
}
