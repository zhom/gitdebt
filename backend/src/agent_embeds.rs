//! The catalog of everything gitdebt can put in somebody else's README, and
//! the rules for putting it there correctly.
//!
//! A faithful port of `frontend/src/lib/readme-embeds.ts`. Both copies exist on
//! purpose: the frontend renders the human `/badges` page and the "Ask an
//! agent" clipboard prompt in the browser, while every `.md` surface is now
//! answered by the API. `backend/tests/fixtures/embed-parity.md` is the golden
//! both languages assert against — from `backend/tests/parity.rs` here and
//! `frontend/scripts/embed-parity.test.mjs` there — so a snippet edited on one
//! side and not the other turns a test red instead of shipping two different
//! READMEs.
//!
//! Everything here is pure and deterministic given a slug and an API origin —
//! no wall clock, no database — so the snippet a visitor copies and the snippet
//! an agent fetches are byte-identical.

use crate::agent_markdown::{bullet, fence};

/// The surface an asset describes, which decides how it is grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedGroup {
    Headline,
    Health,
    Social,
}

/// One embeddable asset, already pointed at a concrete repository or account.
#[derive(Debug, Clone)]
pub struct EmbedAsset {
    pub id: &'static str,
    /// Display name, also the Markdown heading the snippet sits under.
    pub name: &'static str,
    /// One line: what a reader learns from it.
    pub purpose: &'static str,
    /// Path under the API origin, including any asset-defining query.
    pub path: String,
    /// Alt text. Descriptive, because README images are read by screen readers.
    pub alt: String,
    /// Whether light and dark variants are both worth publishing. False for
    /// assets that ship one baked appearance (social PNGs, the contributor
    /// grid).
    pub themed: bool,
    /// Every encoding the asset's route actually answers with, best first.
    pub formats: &'static [&'static str],
    pub group: EmbedGroup,
    /// Where the asset earns its place, in words an agent can act on.
    pub placement: &'static str,
    /// Set when the asset publishes a grid of rank-addressed images rather than
    /// the single image [`EmbedAsset::path`] names. [`EmbedAsset::path`] is
    /// still the first slot, so previews and format swaps keep working.
    pub grid: Option<EmbedGrid>,
}

/// A grid of rank-addressed images, each in its own link.
///
/// One `<a><img></a>` per slot rather than one linked image: an SVG embedded
/// through `<img>` renders as an image and not as a document, so an `<a>` drawn
/// inside it is inert. The only way to give twelve faces twelve destinations is
/// twelve elements.
#[derive(Debug, Clone)]
pub struct EmbedGrid {
    /// The rank-addressed route. `{route}/{rank}` redirects to whoever holds
    /// that rank right now, and `{route}/{rank}/avatar.svg` renders them.
    pub route: String,
    /// How many slots the pasted markup carries. Ranks past the end of the list
    /// render an empty image, so this is a ceiling and never a claim.
    pub slots: usize,
    /// Per-slot alt text, suffixed with the slot's 1-based rank.
    pub alt: String,
}

/// Routes that rasterize but do not animate.
const STILL_FORMATS: &[&str] = &["svg", "png", "webp"];
/// Routes wired through `animated_gif`: `chart`, `card`, and every `stats/*`.
const MOTION_FORMATS: &[&str] = &["svg", "png", "webp", "gif"];
/// `og` is a social preview; it has no vector form to serve.
const SOCIAL_FORMATS: &[&str] = &["png", "webp"];

const HEALTH_PLACEMENT: &str = "a Project health or Contributing section, where a prospective contributor \
     is already reading";

/// Slots the published contributor grid carries. The URLs address ranks and
/// never go stale, but the *number* of them is whatever the pasted markup says,
/// so a round dozen: two full rows at most README widths, and short enough that
/// a maintainer reads the block rather than scrolling past it.
const CONTRIBUTOR_GRID_SLOTS: usize = 12;
/// Rendered size of one avatar slot, in CSS pixels.
const AVATAR_SLOT_PX: u32 = 64;

/// README assets are static by default and animation is opt-in, so no builder
/// here ever emits `animate=1`. Not because GitHub removes it — camo passes an
/// SVG through byte for byte, and an `<img>`-embedded SVG runs in secure
/// animated mode, where SMIL and CSS animation both play — but because motion
/// in somebody else's README should be their decision, and because a static
/// default keeps the SVG and the raster forms of an asset showing the same
/// frame.
pub const STATIC_BY_DEFAULT: &str = "Published snippets are static: motion nobody asked for is bad manners in \
     somebody else's README, and it keeps the SVG and raster forms of an asset \
     identical. Motion is an explicit opt-in — add `animate=1` to an SVG URL \
     and it plays in a GitHub README. The `.gif` variant is for the surfaces \
     that take raster alone: rasterizers, CSS `background-image`, and README \
     renderers outside GitHub such as npm, PyPI, and Docker Hub, which show an \
     SVG as a single static frame.";

/// Repository-health charts share a route shape, a caveat, and a placement.
struct HealthChart {
    id: &'static str,
    name: &'static str,
    purpose: &'static str,
    alt: &'static str,
}

const HEALTH_CHARTS: &[HealthChart] = &[
    HealthChart {
        id: "heatmap",
        name: "Commit calendar",
        purpose: "Daily commit density across the last 52 weeks.",
        alt: "commit activity calendar",
    },
    HealthChart {
        id: "commit-trend",
        name: "Maintenance pulse",
        purpose: "Commit volume over time, so a slowdown is visible rather than implied.",
        alt: "commit trend",
    },
    HealthChart {
        id: "contributors",
        name: "Contributors",
        purpose: "Who is actually landing commits, ranked, with avatars inlined.",
        alt: "contributors",
    },
    HealthChart {
        id: "bus-factor",
        name: "Ownership concentration",
        purpose: "How few people write half the commits.",
        alt: "bus factor",
    },
    HealthChart {
        id: "lines",
        name: "Language activity",
        purpose: "Lines of code by language across the analyzed history.",
        alt: "language activity",
    },
    HealthChart {
        id: "top-files",
        name: "File change frequency",
        purpose: "The files the most commits touch, dependency manifests excluded.",
        alt: "file change frequency",
    },
    HealthChart {
        id: "bug-magnets",
        name: "Fix-labelled changes",
        purpose: "Files most often touched by commits whose message reads like a fix.",
        alt: "fix-labelled changes",
    },
    HealthChart {
        id: "todo-trend",
        name: "TODO/FIXME movement",
        purpose: "Whether known debt markers are being added or paid down.",
        alt: "recent TODO and FIXME movement",
    },
];

/// Everything embeddable for one repository, in the order a README would want
/// it: the badge row first, then the chart most projects came for, then the
/// evidence a reader would ask for next.
pub fn repo_embed_assets(slug: &str) -> Vec<EmbedAsset> {
    let base = format!("/api/repos/{slug}");
    let mut assets = vec![
        EmbedAsset {
            id: "badge-metrics",
            name: "Metrics badge",
            purpose: "Stars and forks in one compact chip, served from gitdebt's cache.",
            path: format!("{base}/badge.svg?metrics=stars,forks"),
            alt: format!("{slug} stars and forks"),
            themed: true,
            formats: STILL_FORMATS,
            group: EmbedGroup::Headline,
            placement: "the badge row directly under the project title, alongside CI and license badges",
            grid: None,
        },
        EmbedAsset {
            id: "badge-signal",
            name: "Earned signal badge",
            purpose: "One evidence-backed claim — actively maintained, community powered, \
                      star momentum, or contributor readiness.",
            path: format!("{base}/badge.svg?signal=active"),
            alt: format!("{slug} actively maintained"),
            themed: true,
            formats: STILL_FORMATS,
            group: EmbedGroup::Headline,
            placement: "the badge row, but only for signals the repository has actually earned",
            grid: None,
        },
        EmbedAsset {
            id: "chart",
            name: "Star history",
            purpose: "The full cumulative star curve, served from Postgres.",
            path: format!("{base}/chart.svg"),
            alt: format!("{slug} star history"),
            themed: true,
            formats: MOTION_FORMATS,
            group: EmbedGroup::Headline,
            placement: "a `## Star history` section near the bottom of the README, above License",
            grid: None,
        },
        EmbedAsset {
            id: "card",
            name: "Repository card",
            purpose: "Stars, forks, contributors, languages, and a 90-day sparkline in one panel.",
            path: format!("{base}/card.svg"),
            alt: format!("{slug} repository statistics"),
            themed: true,
            formats: MOTION_FORMATS,
            group: EmbedGroup::Headline,
            placement: "an About or Project status section, or a docs-site sidebar",
            grid: None,
        },
        EmbedAsset {
            id: "usage",
            name: "Stars versus downloads",
            purpose: "Star growth against package-registry downloads, for projects that \
                      publish one.",
            path: format!("{base}/usage.svg"),
            alt: format!("{slug} stars versus package downloads"),
            themed: true,
            formats: STILL_FORMATS,
            group: EmbedGroup::Headline,
            placement: "next to the star-history chart, when the project ships a package",
            grid: None,
        },
        EmbedAsset {
            id: "contributor-grid",
            name: "Linked contributor grid",
            purpose: "The top twelve contributors as individually linked avatars. Each slot is \
                      its own `<a><img>` because an SVG embedded through `<img>` cannot carry a \
                      working link inside it, and each URL addresses a rank rather than a person, \
                      so the markup never needs regenerating: ranks past the end of the list \
                      render nothing, and a repository that grows past twelve keeps showing its \
                      top twelve until someone pastes more lines.",
            // Rank zero: the asset's own path is the first slot, so a preview
            // or a format swap resolves to a real image rather than a template.
            path: format!("{base}/contributors/0/avatar.svg"),
            alt: format!("{slug} contributor 1"),
            themed: false,
            formats: STILL_FORMATS,
            group: EmbedGroup::Headline,
            placement: "a Contributors or Thanks section, where a reader is looking for the \
                        people rather than the numbers",
            grid: Some(EmbedGrid {
                route: format!("{base}/contributors"),
                slots: CONTRIBUTOR_GRID_SLOTS,
                alt: format!("{slug} contributor"),
            }),
        },
    ];
    assets.extend(HEALTH_CHARTS.iter().map(|chart| EmbedAsset {
        id: chart.id,
        name: chart.name,
        purpose: chart.purpose,
        path: format!("{base}/stats/{}.svg", chart.id),
        alt: format!("{slug} {}", chart.alt),
        themed: true,
        formats: MOTION_FORMATS,
        group: EmbedGroup::Health,
        placement: HEALTH_PLACEMENT,
        grid: None,
    }));
    assets.push(EmbedAsset {
        id: "og",
        name: "Social preview",
        purpose: "A 1200x630 PNG for link unfurls on social platforms and chat apps.",
        path: format!("{base}/og.png"),
        alt: format!("{slug} on gitdebt"),
        themed: false,
        formats: SOCIAL_FORMATS,
        group: EmbedGroup::Social,
        placement: "a docs-site `og:image` meta tag — not the README, where it would be redundant",
        grid: None,
    });
    assets
}

/// Everything embeddable for one maintainer account or organization.
pub fn profile_embed_assets(login: &str) -> Vec<EmbedAsset> {
    let base = format!("/api/users/{login}");
    vec![
        EmbedAsset {
            id: "card",
            name: "Maintainer card",
            purpose: "Aggregate public-repository totals for the account in one compact panel.",
            path: format!("{base}/card.svg"),
            alt: format!("{login} maintainer statistics"),
            themed: true,
            formats: MOTION_FORMATS,
            group: EmbedGroup::Headline,
            placement: "the top of a profile README, under the introduction",
            grid: None,
        },
        EmbedAsset {
            id: "chart",
            name: "Aggregate star history",
            purpose: "One curve summing star growth across every public repository owned.",
            path: format!("{base}/chart.svg"),
            alt: format!("Aggregate star history across {login}'s public repositories"),
            themed: true,
            formats: MOTION_FORMATS,
            group: EmbedGroup::Headline,
            placement: "a profile README, below the card",
            grid: None,
        },
        EmbedAsset {
            id: "contributions",
            name: "Contribution footprint",
            purpose: "Authored work in owned projects versus other people's projects.",
            path: format!("{base}/stats/contributions.svg"),
            alt: format!("{login} contribution footprint"),
            themed: true,
            formats: MOTION_FORMATS,
            group: EmbedGroup::Health,
            placement: "a profile README, in place of a generic contribution-count widget",
            grid: None,
        },
        EmbedAsset {
            id: "languages",
            name: "Language footprint",
            purpose: "Lines of code by language across every analyzed owned repository.",
            path: format!("{base}/stats/languages.svg"),
            alt: format!("{login} language footprint"),
            themed: true,
            formats: MOTION_FORMATS,
            group: EmbedGroup::Health,
            placement: "a profile README, next to the contribution footprint",
            grid: None,
        },
        EmbedAsset {
            id: "commit-activity",
            name: "Commit activity",
            purpose: "Every commit landed in the last 52 weeks, summed across owned repos.",
            path: format!("{base}/stats/commit-activity.svg"),
            alt: format!("{login} commit activity"),
            themed: true,
            formats: MOTION_FORMATS,
            group: EmbedGroup::Health,
            placement: "a profile README, as the activity strip",
            grid: None,
        },
        EmbedAsset {
            id: "og",
            name: "Social preview",
            purpose: "A 1200x630 PNG for link unfurls.",
            path: format!("{base}/og.png"),
            alt: format!("{login} on gitdebt"),
            themed: false,
            formats: SOCIAL_FORMATS,
            group: EmbedGroup::Social,
            placement: "a personal site's `og:image` meta tag",
            grid: None,
        },
    ]
}

/// Swap an asset path onto another format, preserving its query string. The
/// query is asset-defining (`?metrics=stars,forks` is a different badge), so
/// dropping it here would silently publish the wrong image.
fn with_format(path: &str, format: &str) -> String {
    let (file, query) = match path.find('?') {
        Some(index) => path.split_at(index),
        None => (path, ""),
    };
    match file.rsplit_once('.') {
        Some((stem, "svg" | "png" | "webp" | "gif")) => format!("{stem}.{format}{query}"),
        _ => format!("{file}{query}"),
    }
}

fn with_param(path: &str, key: &str, value: &str) -> String {
    let separator = if path.contains('?') { '&' } else { '?' };
    format!("{path}{separator}{key}={value}")
}

/// The absolute URL a README would carry: no cache-busting revision, no
/// attribution parameter. Attribution belongs on the surrounding link, never on
/// an image URL that a CDN has to key on.
pub fn asset_url(
    api: &str,
    asset: &EmbedAsset,
    theme: Option<&str>,
    format: Option<&str>,
) -> String {
    let format = format
        .or_else(|| asset.formats.first().copied())
        .unwrap_or("svg");
    let mut path = with_format(&asset.path, format);
    // An unthemed asset ships one baked appearance, so `theme` on its URL would
    // only fragment the CDN cache.
    if let Some(theme) = theme.filter(|_| asset.themed) {
        path = with_param(&path, "theme", theme);
    }
    format!("{api}{path}")
}

/// The gitdebt report an embed links back to, carrying README attribution.
pub fn readme_link(site: &str, path: &str) -> String {
    let origin = site.trim_end_matches('/');
    if path.starts_with('/') {
        format!("{origin}{path}?ref=readme")
    } else {
        format!("{origin}/{path}?ref=readme")
    }
}

/// `[![alt](url)](link)` against one baked theme.
pub fn markdown_embed(api: &str, asset: &EmbedAsset, link: &str, theme: &str) -> String {
    format!(
        "[![{}]({})]({link})",
        asset.alt,
        asset_url(api, asset, Some(theme), None)
    )
}

/// The theme-aware form. GitHub renders README images against the reader's OS
/// preference rather than the page, and an SVG cannot answer that itself
/// because its colors are baked, so both variants ship and `<picture>` chooses.
pub fn picture_embed(api: &str, asset: &EmbedAsset, link: &str) -> String {
    [
        format!("<a href=\"{link}\">"),
        "  <picture>".to_string(),
        format!(
            "    <source media=\"(prefers-color-scheme: dark)\" srcset=\"{}\" />",
            asset_url(api, asset, Some("dark"), None)
        ),
        format!(
            "    <img alt=\"{}\" src=\"{}\" />",
            asset.alt,
            asset_url(api, asset, Some("light"), None)
        ),
        "  </picture>".to_string(),
        "</a>".to_string(),
    ]
    .join("\n")
}

/// One `<a><img></a>` per rank, then the attributed link.
///
/// Every anchor above belongs to a contributor, so this is the one published
/// snippet with no single wrapper to hang `?ref=readme` on; it goes on a line of
/// its own underneath instead. The image URLs stay plain.
///
/// The slots carry no `theme`: an avatar is a photograph inside a ring, the
/// ring is the only themed ink on it, and a `<picture>` per slot would triple a
/// block somebody else has to read in their own README.
pub fn linked_grid_embed(api: &str, grid: &EmbedGrid, link: &str) -> String {
    let mut lines: Vec<String> = (0..grid.slots)
        .map(|rank| {
            format!(
                "<a href=\"{api}{route}/{rank}\"><img src=\"{api}{route}/{rank}/avatar.svg\" \
                 width=\"{AVATAR_SLOT_PX}\" height=\"{AVATAR_SLOT_PX}\" alt=\"{alt} {position}\" \
                 /></a>",
                route = grid.route,
                alt = grid.alt,
                position = rank + 1,
            )
        })
        .collect();
    lines.push(format!(
        "<a href=\"{link}\">Contributor ranking on gitdebt</a>"
    ));
    lines.join("\n")
}

/// The snippet to publish: a grid where the asset is one, theme-aware where
/// that is meaningful, Markdown otherwise.
pub fn best_embed(api: &str, asset: &EmbedAsset, link: &str) -> String {
    match &asset.grid {
        Some(grid) => linked_grid_embed(api, grid, link),
        None if asset.themed => picture_embed(api, asset, link),
        None => markdown_embed(api, asset, link, "dark"),
    }
}

/// The dialect [`best_embed`] returned, for fencing it in a code block.
pub fn best_embed_language(asset: &EmbedAsset) -> &'static str {
    if asset.themed || asset.grid.is_some() {
        "html"
    } else {
        "markdown"
    }
}

/// One asset as a heading, why it earns its place, and its paste-ready snippet.
/// Every document that publishes a snippet publishes it in this shape.
pub(crate) fn asset_section(api: &str, asset: &EmbedAsset, link: &str) -> String {
    format!(
        "### {}\n\n{}\n\nGoes in {}.\n\n{}",
        asset.name,
        asset.purpose,
        asset.placement,
        fence(best_embed_language(asset), &best_embed(api, asset, link))
    )
}

/// The rules block, identical wherever embedding is documented.
pub(crate) fn rules_section() -> String {
    format!("## Embedding rules\n\n{}", bullet(EMBED_RULES))
}

/// The rules that make a published embed correct rather than merely present.
pub const EMBED_RULES: &[&str] = &[
    "No account, token, or API key is involved. Every URL is a plain public image.",
    "Themes are baked into each asset because GitHub renders README images \
     against the reader's OS preference, not the page. Publish both variants \
     with an HTML `<picture>` element, or pick one explicitly with \
     `theme=light` / `theme=dark`. There is no `theme=auto`.",
    STATIC_BY_DEFAULT,
    "Keep the surrounding link and its `?ref=readme` parameter. Attribution \
     lives on the link; the image URL stays plain so CDNs can cache it.",
    "Do not add cache-busting query parameters. Media is edge-cached for a few \
     hours by design and refreshes on its own.",
    "Alt text is not optional. Say what the image shows, not \"chart\".",
    "A repository nobody has analyzed yet renders a placeholder frame and queues \
     the work instead of failing. Load the page once, or wait a few minutes, \
     and the real chart replaces it at the same URL.",
];

/// One query parameter that changes what an asset renders.
pub struct QueryParam {
    pub param: &'static str,
    pub applies: &'static str,
    pub effect: &'static str,
}

/// Query parameters that change what an asset renders.
pub const QUERY_REFERENCE: &[QueryParam] = &[
    QueryParam {
        param: "theme=light|dark",
        applies: "every SVG and raster asset",
        effect: "Bakes that palette into the output. Default is light.",
    },
    QueryParam {
        param: "animate=1",
        applies: "SVG charts, cards, and badges",
        effect: "Opts into motion, which plays in a GitHub README. Off by default; use the \
                 `.gif` variant for surfaces that show an SVG as a static frame.",
    },
    QueryParam {
        param: "from=YYYY-MM-DD&to=YYYY-MM-DD",
        applies: "star-history charts",
        effect: "Inclusive date window. An invalid or inverted range is a 400.",
    },
    QueryParam {
        param: "rebase=1",
        applies: "star-history charts",
        effect: "Starts every series at zero, so projects of different ages compare fairly.",
    },
    QueryParam {
        param: "type=date|timeline",
        applies: "star-history charts",
        effect: "Calendar dates, or days-since-first-star.",
    },
    QueryParam {
        param: "log=1",
        applies: "star-history charts",
        effect: "Logarithmic y axis.",
    },
    QueryParam {
        param: "repos=owner/repo,owner/repo",
        applies: "/api/chart.svg",
        effect: "Overlays several repositories on one chart.",
    },
    QueryParam {
        param: "metrics=stars,forks,downloads",
        applies: "/badge.svg",
        effect: "Chooses the chips and their order. `downloads` needs a published package.",
    },
    QueryParam {
        param: "signal=active|community|momentum|contributor-ready",
        applies: "/badge.svg",
        effect: "Renders one evidence-backed claim instead of raw metrics.",
    },
    QueryParam {
        param: "hide_border=1, hide_title=1, card_width=N",
        applies: "/card.svg",
        effect: "Trims the card for tight layouts.",
    },
];

/// Star-history widgets a project may already carry. An agent should replace
/// these in place rather than stacking a second chart underneath.
pub const EXISTING_STAR_HISTORY_MARKERS: &[&str] = &[
    "star-history.com",
    "api.star-history.com",
    "starchart.cc",
    "stars.medv.io",
    "seladb/starhistory",
];

/// Files worth checking beyond `README.md`, in the order they usually pay off.
pub const CANDIDATE_FILES: &[&str] = &[
    "README.md at the repository root",
    "docs/index.md, docs/README.md, or a docs-site landing page",
    "website/ or site/ landing content, if the project publishes one",
    "CONTRIBUTING.md, where repository-health charts tell a contributor what they are joining",
    // An organization profile README is `profile/README.md` inside a repository
    // literally named `.github`. `.github/profile/README.md` is a path that
    // exists in no other checkout, so an agent told to look there finds nothing.
    "profile/README.md, when the checkout is the account's `.github` repository, \
     which is where an organization profile README lives",
];

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "https://gitdebt.com";
    const API: &str = "https://api.gitdebt.com";

    #[test]
    fn every_asset_targets_the_requested_entity_with_a_unique_id() {
        for (assets, base) in [
            (repo_embed_assets("owner/repo"), "/api/repos/owner/repo/"),
            (profile_embed_assets("owner"), "/api/users/owner/"),
        ] {
            assert!(!assets.is_empty());
            let mut ids: Vec<&str> = assets.iter().map(|asset| asset.id).collect();
            ids.sort_unstable();
            let unique = ids.len();
            ids.dedup();
            assert_eq!(ids.len(), unique, "duplicate asset id in {base}");
            for asset in &assets {
                assert!(
                    asset.path.starts_with(base),
                    "{} escapes {base}",
                    asset.path
                );
                assert!(!asset.formats.is_empty());
                assert!(!asset.alt.is_empty());
                assert!(!asset.placement.is_empty());
            }
        }
    }

    /// Only the routes wired through `animated_gif` may advertise `.gif`;
    /// promising motion from `badge`, `usage`, or `og` publishes a 404.
    #[test]
    fn only_animating_routes_advertise_gif() {
        for asset in repo_embed_assets("owner/repo") {
            let animates =
                asset.id == "chart" || asset.id == "card" || asset.path.contains("/stats/");
            assert_eq!(
                asset.formats.contains(&"gif"),
                animates,
                "{} advertises the wrong motion support",
                asset.id
            );
        }
    }

    #[test]
    fn format_swap_preserves_the_asset_defining_query() {
        let assets = repo_embed_assets("owner/repo");
        let badge = assets
            .iter()
            .find(|asset| asset.id == "badge-metrics")
            .expect("metrics badge");
        assert_eq!(
            asset_url(API, badge, Some("light"), Some("png")),
            "https://api.gitdebt.com/api/repos/owner/repo/badge.png?metrics=stars,forks&theme=light"
        );
        assert_eq!(
            asset_url(API, badge, None, None),
            "https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks"
        );
    }

    /// An unthemed asset bakes one appearance, so a `theme` on its URL would
    /// fragment the CDN cache for no visual difference.
    #[test]
    fn theme_only_rides_themed_assets() {
        let assets = repo_embed_assets("owner/repo");
        let og = assets.iter().find(|asset| asset.id == "og").expect("og");
        assert_eq!(
            asset_url(API, og, Some("dark"), None),
            "https://api.gitdebt.com/api/repos/owner/repo/og.png"
        );
        assert_eq!(best_embed_language(og), "markdown");
        assert_eq!(
            best_embed(API, og, "https://gitdebt.com/owner/repo?ref=readme"),
            "[![owner/repo on gitdebt](https://api.gitdebt.com/api/repos/owner/repo/og.png)]\
             (https://gitdebt.com/owner/repo?ref=readme)"
        );
    }

    #[test]
    fn themed_assets_publish_both_variants() {
        let assets = repo_embed_assets("owner/repo");
        let chart = assets
            .iter()
            .find(|asset| asset.id == "chart")
            .expect("chart");
        let snippet = best_embed(API, chart, "https://gitdebt.com/owner/repo?ref=readme");
        assert_eq!(best_embed_language(chart), "html");
        assert!(snippet.contains("prefers-color-scheme: dark"));
        assert!(snippet.contains("chart.svg?theme=dark"));
        assert!(snippet.contains("chart.svg?theme=light"));
        assert!(snippet.contains("alt=\"owner/repo star history\""));
    }

    /// The grid's whole promise is that it never needs regenerating: every URL
    /// in it addresses a rank, so nothing in the markup names a person and
    /// nothing has to be renumbered when the ranking changes.
    #[test]
    fn the_contributor_grid_addresses_ranks_and_never_a_person() {
        let assets = repo_embed_assets("owner/repo");
        let asset = assets
            .iter()
            .find(|asset| asset.id == "contributor-grid")
            .expect("contributor grid");
        let grid = asset.grid.as_ref().expect("the grid");
        assert_eq!(grid.slots, CONTRIBUTOR_GRID_SLOTS);
        assert_eq!(best_embed_language(asset), "html");
        // The asset's own path is slot one, so its alt describes slot one too.
        assert_eq!(asset.path, format!("{}/0/avatar.svg", grid.route));
        assert_eq!(asset.alt, format!("{} 1", grid.alt));

        let link = readme_link(SITE, "/owner/repo");
        let snippet = best_embed(API, asset, &link);
        let lines: Vec<&str> = snippet.lines().collect();
        assert_eq!(lines.len(), grid.slots + 1);
        for (rank, line) in lines[..grid.slots].iter().enumerate() {
            assert_eq!(
                *line,
                format!(
                    "<a href=\"{API}/api/repos/owner/repo/contributors/{rank}\">\
                     <img src=\"{API}/api/repos/owner/repo/contributors/{rank}/avatar.svg\" \
                     width=\"64\" height=\"64\" alt=\"owner/repo contributor {}\" /></a>",
                    rank + 1
                )
            );
        }
        // Attribution is the last line, because every anchor above it belongs to
        // a contributor rather than to gitdebt.
        assert_eq!(
            lines[grid.slots],
            format!("<a href=\"{link}\">Contributor ranking on gitdebt</a>")
        );
        assert!(!snippet.contains("github.com"));
    }

    #[test]
    fn readme_link_normalizes_the_origin_and_carries_attribution() {
        assert_eq!(
            readme_link("https://gitdebt.com//", "/owner/repo"),
            "https://gitdebt.com/owner/repo?ref=readme"
        );
        assert_eq!(
            readme_link(SITE, "owner/repo"),
            "https://gitdebt.com/owner/repo?ref=readme"
        );
    }

    /// The whole point of keeping attribution on the link: an image URL that
    /// carries `ref=`, `animate=`, or `render=` breaks CDN caching or publishes
    /// motion nobody asked for.
    #[test]
    fn no_image_url_in_any_snippet_carries_a_forbidden_parameter() {
        let link = readme_link(SITE, "/owner/repo");
        let assets = repo_embed_assets("owner/repo")
            .into_iter()
            .chain(profile_embed_assets("owner"));
        for asset in assets {
            let snippet = best_embed(API, &asset, &link);
            for image in snippet
                .split(&['"', '(', ')'][..])
                .filter(|part| part.starts_with(API))
            {
                for forbidden in ["ref=", "animate=", "render="] {
                    assert!(
                        !image.contains(forbidden),
                        "{} embeds {forbidden} in {image}",
                        asset.id
                    );
                }
            }
            // Attribution still has to be present, on the link.
            assert!(snippet.contains("?ref=readme"));
        }
    }

    #[test]
    fn identical_input_renders_identical_bytes() {
        let link = readme_link(SITE, "/owner/repo");
        let once = repo_embed_assets("owner/repo");
        let twice = repo_embed_assets("owner/repo");
        for (first, second) in once.iter().zip(twice.iter()) {
            assert_eq!(
                best_embed(API, first, &link),
                best_embed(API, second, &link)
            );
        }
    }
}
