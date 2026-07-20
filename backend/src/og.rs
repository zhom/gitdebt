//! Social Open Graph card renderer → 1200×630 PNG.
//!
//! Why a dedicated module (not the chart renderers): social platforms
//! (Twitter/X, Slack, LinkedIn, Discord, Facebook) **reject SVG** as an
//! `og:image` and demand a real raster at the dimensions the page
//! declares. The chart SVGs are 1200×600 line charts tuned for README
//! embeds; an OG card is a different composition — a branded monochrome
//! background, a giant headline number, a sparkline across the lower
//! third, a footer lockup — sized at exactly **1200×630** so the
//! rasterized PNG matches the `og:image:width`/`height` the frontend
//! declares.
//!
//! Pipeline mirrors the chart raster path: build a deterministic SVG
//! here, then `raster::rasterize(svg, Png, 1.0)`. Scale **1.0** is
//! load-bearing: the viewBox is 1200×630 so scale 1.0 yields a 1200×630
//! PNG. (The chart endpoints rasterize at 2.0 for retina READMEs; OG
//! images must be exactly the declared size or crawlers letterbox /
//! reject them.)
//!
//! Fonts: we reuse the **exact** `font-family` strings the chart SVGs
//! use — `ui-sans-serif, system-ui, sans-serif` and `ui-monospace,
//! SFMono-Regular, Menlo, monospace`. `raster.rs` maps every generic
//! family (sans-serif / serif / monospace) onto the bundled Inter font,
//! so these resolve and text renders. Introducing a novel family here
//! would render blank glyph boxes in the PNG — don't.
//!
//! OG cards share the chart system's concrete light/dark colors. Social
//! previews don't theme-switch, so the default is the white-first light
//! card and callers may explicitly request the black dark variant.
//! Deterministic: same input → same bytes.

use crate::brand;
use crate::chart::{Point, palette};
use crate::theme::Theme;

/// Fixed OG card dimensions. Declared `og:image:width`/`height` on the
/// frontend MUST equal these, and `raster::rasterize(.., 1.0)` of a
/// `WIDTH × HEIGHT` viewBox yields a PNG of exactly these dimensions.
pub const OG_WIDTH: u32 = 1200;
pub const OG_HEIGHT: u32 = 630;

/// Dark-card canvas and slightly lifted sparkline floor.
const CARD_BG: &str = "#0a0a0a";
const CARD_PANEL: &str = "#171717";
/// The strongest white-first brand mark.
const BRAND_INK: &str = "#0a0a0a";

/// The same font stacks the chart/badge SVGs use. `raster.rs` resolves
/// every generic family to the bundled Inter, so these render in PNG.
const FONT_SANS: &str = "ui-sans-serif, system-ui, sans-serif";
const FONT_MONO: &str = "ui-monospace, SFMono-Regular, Menlo, monospace";

/// Inputs for a single-repo OG card. All the secondary fields are
/// best-effort — a missing piece is simply omitted from the card so the
/// renderer never blocks on data it doesn't have.
#[derive(Debug, Clone, Default)]
pub struct RepoCard {
    /// `owner/repo` slug (already lowercased by the caller).
    pub slug: String,
    /// Total stars — the headline number.
    pub stars: u64,
    /// Fork count, when known.
    pub forks: Option<u64>,
    /// Best resolved download total + its source label, e.g.
    /// `(2_100_000, "npm")` → "2.1M npm downloads". `None` omits the row.
    pub downloads: Option<(u64, String)>,
    /// Cumulative star-history points for the lower-third sparkline.
    /// Empty → the sparkline area is left blank (card still valid).
    pub series: Vec<Point>,
}

/// One repo entry on a compare card: slug, star count, and the series
/// for the overlay sparkline. The caller orders these to match the
/// categorical palette (index 0 = strongest contrast).
#[derive(Debug, Clone, Default)]
pub struct CompareEntry {
    pub slug: String,
    pub stars: u64,
    pub series: Vec<Point>,
}

// Repo card

/// Render the single-repo social card SVG (1200×630). Headline total
/// stars, a secondary mono row (forks + downloads), a star-history
/// sparkline across the lower third, and the gitdebt footer lockup.
pub fn render_repo_card(card: &RepoCard, theme: &Theme) -> String {
    let bg = card_bg(theme);
    let fg = card_fg(theme);
    let muted = card_muted(theme);
    let accent = card_accent(theme);

    let mut body = String::new();
    body.push_str(&wordmark(theme));

    // Eyebrow: the repo slug in mono, accent-colored.
    body.push_str(&format!(
        "  <text x=\"80\" y=\"196\" fill=\"{accent}\" font-family=\"{FONT_MONO}\" font-size=\"34\" font-weight=\"600\">{}</text>\n",
        escape_xml(&card.slug),
    ));

    // Headline: the big star number.
    body.push_str(&format!(
        "  <text x=\"78\" y=\"320\" fill=\"{fg}\" font-family=\"{FONT_SANS}\" font-size=\"132\" font-weight=\"800\" letter-spacing=\"-0.02em\">{}</text>\n",
        escape_xml(&fmt_count(card.stars)),
    ));
    body.push_str(&format!(
        "  <text x=\"82\" y=\"364\" fill=\"{muted}\" font-family=\"{FONT_SANS}\" font-size=\"30\" font-weight=\"600\" letter-spacing=\"0.08em\">GITHUB STARS</text>\n",
    ));

    // Secondary mono row: forks + the best resolved download total. Each
    // piece is best-effort; we build the segments that exist and join.
    let secondary = secondary_line(card);
    if !secondary.is_empty() {
        body.push_str(&format!(
            "  <text x=\"82\" y=\"420\" fill=\"{muted}\" font-family=\"{FONT_MONO}\" font-size=\"28\">{}</text>\n",
            escape_xml(&secondary),
        ));
    }

    // Sparkline across the lower third (the star-history shape).
    body.push_str(&sparkline_panel(
        &[(card.slug.clone(), &card.series)],
        theme,
        SPARK_TOP,
    ));

    // Footer lockup.
    body.push_str(&footer(muted));

    wrap_svg(&body, bg, &format!("gitdebt · {}", card.slug))
}

/// Build the "45 forks · 2.1M npm downloads" style secondary row,
/// omitting whichever piece is missing. Empty string → no row drawn.
fn secondary_line(card: &RepoCard) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(forks) = card.forks {
        parts.push(format!("{} forks", fmt_count(forks)));
    }
    if let Some((total, ref label)) = card.downloads {
        parts.push(format!("{} {label} downloads", fmt_count(total)));
    }
    parts.join("  ·  ")
}

// Compare card

/// Render the multi-repo compare card SVG (1200×630): a "{a} vs {b}"
/// title, an overlay sparkline, and each repo's star count in its
/// series color. `entries` is in stable palette order.
pub fn render_compare_card(entries: &[CompareEntry], theme: &Theme) -> String {
    let bg = card_bg(theme);
    let fg = card_fg(theme);
    let muted = card_muted(theme);
    let pal = palette(theme);

    let mut body = String::new();
    body.push_str(&wordmark(theme));

    // Title: "a vs b" (vs c …). Slugs joined with " vs ".
    let title = entries
        .iter()
        .map(|e| short_slug(&e.slug))
        .collect::<Vec<_>>()
        .join(" vs ");
    body.push_str(&format!(
        "  <text x=\"80\" y=\"212\" fill=\"{fg}\" font-family=\"{FONT_SANS}\" font-size=\"64\" font-weight=\"800\" letter-spacing=\"-0.01em\">{}</text>\n",
        escape_xml(&title),
    ));
    body.push_str(&format!(
        "  <text x=\"82\" y=\"256\" fill=\"{muted}\" font-family=\"{FONT_SANS}\" font-size=\"26\" font-weight=\"600\" letter-spacing=\"0.08em\">GITHUB STAR HISTORY</text>\n",
    ));

    // Per-repo star count, each in its series color.
    let mut row_y = 320.0_f32;
    for (i, e) in entries.iter().enumerate() {
        let color = pal[i % pal.len()];
        body.push_str(&format!(
            "  <rect x=\"82\" y=\"{ry:.1}\" width=\"18\" height=\"18\" rx=\"4\" fill=\"{color}\" />\n  <text x=\"112\" y=\"{ty:.1}\" fill=\"{fg}\" font-family=\"{FONT_MONO}\" font-size=\"30\" font-weight=\"600\">{slug} — {stars}</text>\n",
            ry = row_y - 16.0,
            ty = row_y,
            slug = escape_xml(&short_slug(&e.slug)),
            stars = escape_xml(&format!("{} stars", fmt_count(e.stars))),
        ));
        row_y += 44.0;
        // Cap visible rows so a 12-repo compare doesn't overrun the card.
        if i >= 4 {
            break;
        }
    }

    // Overlay sparkline across the lower third.
    let series_refs: Vec<(String, &Vec<Point>)> = entries
        .iter()
        .map(|e| (e.slug.clone(), &e.series))
        .collect();
    body.push_str(&sparkline_panel(&series_refs, theme, SPARK_TOP));

    body.push_str(&footer(muted));

    wrap_svg(&body, bg, &format!("gitdebt · {title}"))
}

// Default site card

/// Render the default site card SVG (1200×630): the gitdebt lockup, the
/// tagline, and a subtle technical grid motif. Used for `/api/og.png` with no
/// repos.
pub fn render_default_card(theme: &Theme) -> String {
    let bg = card_bg(theme);
    let fg = card_fg(theme);
    let muted = card_muted(theme);

    let mut body = String::new();
    // Subtle grid motif behind everything (decorative, faint).
    body.push_str(&grid_motif(theme));

    // Large centered-ish robot mark + wordmark.
    body.push_str(&brand::logo_mark(80.0, 172.0, 140.0, fg, card_bg(theme)));
    body.push_str(&format!(
        "  <text x=\"248\" y=\"300\" fill=\"{fg}\" font-family=\"{FONT_SANS}\" font-size=\"120\" font-weight=\"800\" letter-spacing=\"-0.02em\">gitdebt</text>\n",
    ));
    // Tagline.
    body.push_str(&format!(
        "  <text x=\"86\" y=\"372\" fill=\"{muted}\" font-family=\"{FONT_SANS}\" font-size=\"40\" font-weight=\"500\">GitHub star history + repo-debt insights</text>\n",
    ));

    body.push_str(&footer(muted));

    wrap_svg(
        &body,
        bg,
        "gitdebt — GitHub star history + repo-debt insights",
    )
}

// Shared pieces

/// Y of the sparkline panel's top edge. The panel spans from here to
/// just above the footer.
const SPARK_TOP: f32 = 452.0;
const SPARK_BOTTOM: f32 = 560.0;
const SPARK_LEFT: f32 = 80.0;
const SPARK_RIGHT: f32 = OG_WIDTH as f32 - 80.0;

/// gitdebt wordmark top-left: monochrome robot mark + the wordmark in fg.
fn wordmark(theme: &Theme) -> String {
    let fg = card_fg(theme);
    format!(
        "{}  <text x=\"152\" y=\"108\" fill=\"{fg}\" font-family=\"{FONT_SANS}\" font-size=\"44\" font-weight=\"800\" letter-spacing=\"-0.01em\">gitdebt</text>\n",
        brand::logo_mark(80.0, 52.0, 64.0, fg, card_bg(theme)),
    )
}

/// Footer lockup, bottom-left.
fn footer(muted: &str) -> String {
    format!(
        "  <text x=\"80\" y=\"600\" fill=\"{muted}\" font-family=\"{FONT_SANS}\" font-size=\"24\" font-weight=\"600\" letter-spacing=\"0.02em\">gitdebt.com · GitHub star history &amp; repo-debt</text>\n",
    )
}

/// Render the lower-third sparkline panel for one or more series. Each
/// series is drawn as a polyline in its palette color over a shared
/// x/y range (union of all series). Empty input → just the panel floor.
///
/// Mirrors the chart's coordinate math (`(x - x_min)/x_span * plot_w`)
/// rather than reaching into `chart.rs`'s private `Geometry`/`build_path`
/// — the geometry differs (this is a compact sparkline, not the full
/// axes chart) so a local, equally-deterministic mapping is correct here.
fn sparkline_panel(series: &[(String, &Vec<Point>)], theme: &Theme, top: f32) -> String {
    let pal = palette(theme);
    let plot_left = SPARK_LEFT;
    let plot_right = SPARK_RIGHT;
    let plot_top = top;
    let plot_bottom = SPARK_BOTTOM;
    let plot_w = plot_right - plot_left;
    let plot_h = plot_bottom - plot_top;

    // A subtly-lifted panel floor + a baseline rule, so an empty-data
    // card still reads as "a chart goes here" rather than blank space.
    let mut out = format!(
        "  <rect x=\"{plot_left:.1}\" y=\"{plot_top:.1}\" width=\"{plot_w:.1}\" height=\"{plot_h:.1}\" rx=\"10\" fill=\"{panel}\" opacity=\"0.5\" />\n  <line x1=\"{plot_left:.1}\" y1=\"{plot_bottom:.1}\" x2=\"{plot_right:.1}\" y2=\"{plot_bottom:.1}\" stroke=\"{grid}\" stroke-width=\"1.5\" opacity=\"0.6\" />\n",
        panel = card_panel(theme),
        grid = card_grid(theme),
    );

    // Shared ranges across every non-empty series.
    let active: Vec<&(String, &Vec<Point>)> =
        series.iter().filter(|(_, s)| !s.is_empty()).collect();
    if active.is_empty() {
        return out;
    }
    let mut x_min = f32::INFINITY;
    let mut x_max = f32::NEG_INFINITY;
    let mut y_max = 1u64;
    for (_, s) in &active {
        for p in s.iter() {
            let x = p.at.timestamp() as f32;
            x_min = x_min.min(x);
            x_max = x_max.max(x);
            y_max = y_max.max(p.stars as u64);
        }
    }
    let x_span = (x_max - x_min).max(1.0);
    let y_max_f = y_max.max(1) as f32;

    for (i, (_, s)) in active.iter().enumerate() {
        let color = pal[i % pal.len()];
        let mut d = String::new();
        for (j, p) in s.iter().enumerate() {
            let x = plot_left + ((p.at.timestamp() as f32 - x_min) / x_span) * plot_w;
            let y = plot_bottom - (p.stars as f32 / y_max_f) * plot_h;
            if j == 0 {
                d.push_str(&format!("M {x:.1} {y:.1}"));
            } else {
                d.push_str(&format!(" L {x:.1} {y:.1}"));
            }
        }
        out.push_str(&format!(
            "  <path d=\"{d}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"3\" stroke-linecap=\"round\" stroke-linejoin=\"round\" />\n",
        ));
    }
    out
}

/// Faint monochrome grid motif for the default card. Deterministic set of
/// vertical + horizontal hairlines at a fixed spacing.
fn grid_motif(theme: &Theme) -> String {
    let grid = card_grid(theme);
    let mut out = String::new();
    let step = 60;
    let mut x = step;
    while x < OG_WIDTH as i32 {
        out.push_str(&format!(
            "  <line x1=\"{x}\" y1=\"0\" x2=\"{x}\" y2=\"{h}\" stroke=\"{grid}\" stroke-width=\"1\" opacity=\"0.25\" />\n",
            h = OG_HEIGHT,
        ));
        x += step;
    }
    let mut y = step;
    while y < OG_HEIGHT as i32 {
        out.push_str(&format!(
            "  <line x1=\"0\" y1=\"{y}\" x2=\"{w}\" y2=\"{y}\" stroke=\"{grid}\" stroke-width=\"1\" opacity=\"0.25\" />\n",
            w = OG_WIDTH,
        ));
        y += step;
    }
    out
}

/// Wrap a card body in the 1200×630 SVG envelope. A thin contrast rule
/// along the top edge ties it to the
/// brand. `aria_label` describes the card for accessibility tooling.
fn wrap_svg(body: &str, bg: &str, aria_label: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-label="{label}">
  <rect x="0" y="0" width="{w}" height="{h}" fill="{bg}" />
  <rect x="0" y="0" width="{w}" height="6" fill="{accent}" />
{body}</svg>"##,
        w = OG_WIDTH,
        h = OG_HEIGHT,
        bg = bg,
        accent = if bg == CARD_BG { "#fafafa" } else { BRAND_INK },
        label = escape_xml(aria_label),
        body = body,
    )
}

/// Trim the owner from `owner/repo` to keep compare titles short; falls
/// back to the full slug when there's no slash.
fn short_slug(slug: &str) -> String {
    match slug.split_once('/') {
        Some((_, repo)) => repo.to_string(),
        None => slug.to_string(),
    }
}

// Card palette accessors. The default light card and explicit dark card
// use the same white/ink system as every other share surface.
fn card_bg(theme: &Theme) -> &'static str {
    if theme.dark { CARD_BG } else { "#ffffff" }
}
fn card_panel(theme: &Theme) -> &'static str {
    if theme.dark { CARD_PANEL } else { "#f5f5f5" }
}
fn card_fg(theme: &Theme) -> &'static str {
    theme.fg
}
fn card_muted(theme: &Theme) -> &'static str {
    theme.muted
}
fn card_grid(theme: &Theme) -> &'static str {
    theme.grid
}
fn card_accent(theme: &Theme) -> &'static str {
    theme.accent
}

/// Compact integer formatting (1234 → "1.2k", 1_500_000 → "1.5M",
/// 2_000_000_000 → "2.0B"). Matches `chart::fmt_count_u64` so card
/// numbers read identically to the chart axes. Local copy because the
/// chart helper is private.
fn fmt_count(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::cumulative_series;
    use crate::theme::{DARK, LIGHT};
    use chrono::{DateTime, TimeZone, Utc};

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn sample_series(n: i64) -> Vec<Point> {
        cumulative_series(&(0..n).map(|i| at(i * 86_400)).collect::<Vec<_>>())
    }

    #[test]
    fn repo_card_has_correct_dimensions() {
        // The declared OG dims and the rasterized PNG both depend on a
        // 1200×630 viewBox — if this drifts, social previews letterbox.
        let card = RepoCard {
            slug: "owner/repo".into(),
            stars: 12_345,
            forks: Some(678),
            downloads: Some((2_100_000, "npm".into())),
            series: sample_series(20),
        };
        let svg = render_repo_card(&card, &DARK);
        assert!(svg.contains("viewBox=\"0 0 1200 630\""));
        assert!(svg.contains("width=\"1200\""));
        assert!(svg.contains("height=\"630\""));
    }

    #[test]
    fn repo_card_contains_slug_and_star_count() {
        let card = RepoCard {
            slug: "facebook/react".into(),
            stars: 234_567,
            forks: Some(48_000),
            downloads: Some((21_000_000, "npm".into())),
            series: sample_series(30),
        };
        let svg = render_repo_card(&card, &DARK);
        // Slug present (eyebrow).
        assert!(svg.contains("facebook/react"));
        // Humanized star count present (headline).
        assert!(svg.contains("234.6k"));
        // Secondary row: forks + downloads.
        assert!(svg.contains("48.0k forks"));
        assert!(svg.contains("21.0M npm downloads"));
        // Footer lockup.
        assert!(svg.contains("gitdebt.com"));
    }

    #[test]
    fn repo_card_is_deterministic() {
        let card = RepoCard {
            slug: "a/b".into(),
            stars: 100,
            forks: Some(2),
            downloads: None,
            series: sample_series(15),
        };
        let a = render_repo_card(&card, &DARK);
        let b = render_repo_card(&card, &DARK);
        assert_eq!(a, b);
    }

    #[test]
    fn repo_card_omits_missing_pieces_gracefully() {
        // No forks, no downloads, no series — must still be a valid card.
        let card = RepoCard {
            slug: "lonely/repo".into(),
            stars: 7,
            forks: None,
            downloads: None,
            series: Vec::new(),
        };
        let svg = render_repo_card(&card, &DARK);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("lonely/repo"));
        assert!(svg.contains(">7<")); // headline number, no suffix
        // No forks/downloads text leaked.
        assert!(!svg.contains("forks"));
        assert!(!svg.contains("downloads"));
    }

    #[test]
    fn repo_card_bakes_dark_monochrome_palette() {
        let card = RepoCard {
            slug: "o/r".into(),
            stars: 1,
            ..Default::default()
        };
        let svg = render_repo_card(&card, &DARK);
        assert!(svg.contains(CARD_BG));
        assert!(svg.contains("#fafafa"));
        // No CSS vars / theme leakage.
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn repo_card_reuses_chart_font_families() {
        // The PNG only renders text if the rasterizer can resolve the
        // font. We reuse the exact stacks chart.rs uses (resvg maps the
        // generic families onto bundled Inter).
        let card = RepoCard {
            slug: "o/r".into(),
            stars: 5,
            ..Default::default()
        };
        let svg = render_repo_card(&card, &DARK);
        assert!(svg.contains("ui-sans-serif, system-ui, sans-serif"));
        assert!(svg.contains("ui-monospace"));
    }

    #[test]
    fn compare_card_has_title_and_per_repo_counts() {
        let entries = vec![
            CompareEntry {
                slug: "vuejs/vue".into(),
                stars: 207_000,
                series: sample_series(25),
            },
            CompareEntry {
                slug: "facebook/react".into(),
                stars: 234_000,
                series: sample_series(20),
            },
        ];
        let svg = render_compare_card(&entries, &DARK);
        assert!(svg.contains("viewBox=\"0 0 1200 630\""));
        // Title "vue vs react".
        assert!(svg.contains("vue vs react"));
        // Each repo's star count rendered.
        assert!(svg.contains("207.0k stars"));
        assert!(svg.contains("234.0k stars"));
        // Both series colors (dark palette index 0 + 1).
        assert!(svg.contains("#fafafa"));
        assert!(svg.contains("#e5e5e5"));
    }

    #[test]
    fn compare_card_is_deterministic() {
        let entries = vec![
            CompareEntry {
                slug: "o/a".into(),
                stars: 10,
                series: sample_series(8),
            },
            CompareEntry {
                slug: "o/b".into(),
                stars: 20,
                series: sample_series(12),
            },
        ];
        let a = render_compare_card(&entries, &DARK);
        let b = render_compare_card(&entries, &DARK);
        assert_eq!(a, b);
    }

    #[test]
    fn default_card_has_lockup_and_tagline() {
        let svg = render_default_card(&DARK);
        assert!(svg.contains("viewBox=\"0 0 1200 630\""));
        assert!(svg.contains("gitdebt"));
        assert!(svg.contains("M320.5 110.5"));
        assert!(!svg.contains("<image"));
        assert!(svg.contains("GitHub star history + repo-debt insights"));
        assert!(svg.contains(CARD_BG));
        // Deterministic.
        assert_eq!(render_default_card(&DARK), svg);
    }

    #[test]
    fn light_theme_card_uses_white_bg() {
        let svg = render_default_card(&LIGHT);
        assert!(svg.contains("#ffffff"));
        // Light card still renders (no panic, valid svg).
        assert!(svg.starts_with("<svg"));
        // The logo stays strictly monochrome on every card theme.
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.contains("fill=\"#0a0a0a\""));
        assert!(svg.contains("fill=\"#ffffff\""));
        assert!(svg.contains(r##"<rect x="0" y="0" width="1200" height="630" fill="#ffffff" />"##));
    }

    #[test]
    fn logo_reverses_for_dark_social_cards() {
        let svg = render_default_card(&DARK);
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.contains("fill=\"#fafafa\""));
        assert!(svg.contains("fill=\"#0a0a0a\""));
        assert!(!svg.contains("<image"));
    }

    #[test]
    fn light_theme_repo_card_uses_light_panel_tone() {
        // The repo card draws the sparkline panel; the light variant
        // bakes the light panel tone, exercising `card_panel`.
        let card = RepoCard {
            slug: "o/r".into(),
            stars: 5,
            series: sample_series(10),
            ..Default::default()
        };
        let svg = render_repo_card(&card, &LIGHT);
        assert!(svg.contains("#f5f5f5"));
    }

    #[test]
    fn xml_escaped_in_slug() {
        let card = RepoCard {
            slug: "<script>/x".into(),
            stars: 1,
            ..Default::default()
        };
        let svg = render_repo_card(&card, &DARK);
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;"));
    }

    #[test]
    fn fmt_count_humanizes() {
        assert_eq!(fmt_count(7), "7");
        assert_eq!(fmt_count(12_345), "12.3k");
        assert_eq!(fmt_count(1_500_000), "1.5M");
        assert_eq!(fmt_count(2_000_000_000), "2.0B");
    }

    #[test]
    fn repo_card_rasterizes_to_exactly_1200x630_png_with_text() {
        use crate::raster::{RasterFormat, rasterize};

        let card = RepoCard {
            slug: "owner/repo".into(),
            stars: 12_345,
            forks: Some(678),
            downloads: Some((2_100_000, "npm".into())),
            series: sample_series(40),
        };
        let svg = render_repo_card(&card, &DARK);
        // Scale 1.0 → PNG is exactly the SVG's 1200×630 viewBox.
        let png = rasterize(&svg, RasterFormat::Png, 1.0).expect("rasterize og png");

        // PNG magic.
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        // IHDR width/height are big-endian u32 at byte offsets 16 and 20.
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!(
            (w, h),
            (OG_WIDTH, OG_HEIGHT),
            "OG PNG must be exactly 1200×630"
        );

        // Text-renders check without an optional PNG decoder: a card whose
        // body actually rasterized glyphs + a sparkline compresses to a
        // materially larger PNG than a same-size flat fill. If the
        // font failed to resolve (blank glyph boxes) and the body dropped
        // out, the PNG would collapse toward the flat-fill size.
        let blank = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="{bg}" /></svg>"##,
            w = OG_WIDTH,
            h = OG_HEIGHT,
            bg = CARD_BG,
        );
        let blank_png = rasterize(&blank, RasterFormat::Png, 1.0).expect("rasterize blank");
        assert!(
            png.len() > blank_png.len() * 3,
            "rendered card ({}) should be much larger than a flat fill ({}) — text/sparkline must have rendered",
            png.len(),
            blank_png.len(),
        );
    }
}
