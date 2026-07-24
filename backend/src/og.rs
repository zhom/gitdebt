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
use crate::cards::UserCardData;
use crate::chart::{Point, palette};
use crate::texture;
use crate::theme::Theme;

/// Fixed OG card dimensions. Declared `og:image:width`/`height` on the
/// frontend MUST equal these, and `raster::rasterize(.., 1.0)` of a
/// `WIDTH × HEIGHT` viewBox yields a PNG of exactly these dimensions.
pub const OG_WIDTH: u32 = 1200;
pub const OG_HEIGHT: u32 = 630;

/// Dark-card canvas and slightly lifted sparkline floor.
const CARD_BG: &str = "#0a0a0a";
const CARD_PANEL: &str = "#171717";

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
    let accent = texture::wave_ink(theme);

    let mut body = String::new();
    body.push_str(&wordmark(theme));

    // Eyebrow: the repo slug in mono, wave-accent ink.
    body.push_str(&format!(
        "  <text x=\"80\" y=\"196\" fill=\"{accent}\" font-family=\"{FONT_MONO}\" font-size=\"34\" font-weight=\"600\" letter-spacing=\"0.02em\">{}</text>\n",
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

    // Title: "a vs b" (vs c …). Slugs joined with " vs ", truncated with
    // an ellipsis to the canvas budget — a 12-slug compare would otherwise
    // run far past the 1200px edge.
    let title = entries
        .iter()
        .map(|e| short_slug(&e.slug))
        .collect::<Vec<_>>()
        .join(" vs ");
    let drawn_title = truncate_title(&title, TITLE_FONT_SIZE, TITLE_MAX_WIDTH);
    body.push_str(&format!(
        "  <text x=\"80\" y=\"212\" fill=\"{fg}\" font-family=\"{FONT_SANS}\" font-size=\"64\" font-weight=\"800\" letter-spacing=\"-0.01em\">{}</text>\n",
        escape_xml(&drawn_title),
    ));
    body.push_str(&format!(
        "  <text x=\"82\" y=\"256\" fill=\"{muted}\" font-family=\"{FONT_SANS}\" font-size=\"26\" font-weight=\"600\" letter-spacing=\"0.08em\">GITHUB STAR HISTORY</text>\n",
    ));

    // Per-repo star count, each in its series color. Only
    // `COMPARE_ROW_SLOTS` baselines fit above the sparkline panel (which
    // is painted AFTER the rows); when the compare lists more repos, the
    // last slot becomes a "+N more" line instead of painting rows on or
    // under the panel.
    let shown = if entries.len() <= COMPARE_ROW_SLOTS {
        entries.len()
    } else {
        COMPARE_ROW_SLOTS - 1
    };
    for (i, e) in entries.iter().take(shown).enumerate() {
        let color = pal[i % pal.len()];
        let row_y = COMPARE_ROW_TOP + i as f32 * COMPARE_ROW_STEP;
        body.push_str(&format!(
            "  <rect x=\"82\" y=\"{ry:.1}\" width=\"18\" height=\"18\" rx=\"4\" fill=\"{color}\" />\n  <text x=\"112\" y=\"{ty:.1}\" fill=\"{fg}\" font-family=\"{FONT_MONO}\" font-size=\"30\" font-weight=\"600\">{slug} — {stars}</text>\n",
            ry = row_y - 16.0,
            ty = row_y,
            slug = escape_xml(&short_slug(&e.slug)),
            stars = escape_xml(&format!("{} stars", fmt_count(e.stars))),
        ));
    }
    if entries.len() > shown {
        let row_y = COMPARE_ROW_TOP + shown as f32 * COMPARE_ROW_STEP;
        body.push_str(&format!(
            "  <text x=\"82\" y=\"{row_y:.1}\" fill=\"{muted}\" font-family=\"{FONT_MONO}\" font-size=\"26\" font-weight=\"600\">+{n} more</text>\n",
            n = entries.len() - shown,
        ));
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

    // Large robot mark + wordmark. 148px of mark width keeps the artwork
    // above the size where its dither pattern still resolves.
    body.push_str(&brand::logo_mark(84.0, 207.0, 148.0, fg));
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

// User profile card

/// Render the user-profile social card SVG (1200×630): mono eyebrow with
/// the profile handle, the persona line, a headline star total, a mono
/// footprint row, and a decorative Bayer density ramp across the lower
/// third (profiles have no single time series; the ramp is the texture
/// signature, not fake data). Deterministic; only Postgres-derived
/// [`UserCardData`] goes in.
pub fn render_user_og(data: &UserCardData, theme: &Theme) -> String {
    let bg = card_bg(theme);
    let fg = card_fg(theme);
    let muted = card_muted(theme);
    let accent = texture::wave_ink(theme);

    let mut body = String::new();
    body.push_str(&wordmark(theme));

    // Eyebrow: the handle in mono, wave-accent ink.
    body.push_str(&format!(
        "  <text x=\"80\" y=\"196\" fill=\"{accent}\" font-family=\"{FONT_MONO}\" font-size=\"34\" font-weight=\"600\" letter-spacing=\"0.02em\">@{}</text>\n",
        escape_xml(&data.login),
    ));

    // Headline: total stars across tracked repos.
    body.push_str(&format!(
        "  <text x=\"78\" y=\"320\" fill=\"{fg}\" font-family=\"{FONT_SANS}\" font-size=\"132\" font-weight=\"800\" letter-spacing=\"-0.02em\">{}</text>\n",
        escape_xml(&fmt_count(data.stars)),
    ));
    body.push_str(&format!(
        "  <text x=\"82\" y=\"364\" fill=\"{muted}\" font-family=\"{FONT_SANS}\" font-size=\"30\" font-weight=\"600\" letter-spacing=\"0.08em\">TOTAL STARS · {}</text>\n",
        escape_xml(crate::cards::user_persona(data)),
    ));

    // Footprint row in mono: commits, contributed repos, tracked repos.
    // These are lower bounds over tracked repos (the card's honesty rule).
    let mut parts: Vec<String> = Vec::new();
    if data.commits > 0 {
        parts.push(format!("{} commits", fmt_count(data.commits)));
    }
    if data.contribs > 0 {
        parts.push(format!("{} contributed", fmt_count(data.contribs)));
    }
    parts.push(format!("{} repos tracked", fmt_count(data.repos_tracked)));
    body.push_str(&format!(
        "  <text x=\"82\" y=\"420\" fill=\"{muted}\" font-family=\"{FONT_MONO}\" font-size=\"28\">{}</text>\n",
        escape_xml(&parts.join("  ·  ")),
    ));

    body.push_str(&density_ramp(theme));
    body.push_str(&footer(muted));

    wrap_svg(&body, bg, &format!("gitdebt · @{}", data.login))
}

/// 1200×630 placeholder for a login gitdebt knows nothing about — mirrors
/// the user-card "no data yet" behavior at OG dimensions so social embeds
/// self-heal on the API layer's short TTL.
pub fn render_user_empty_og(login: &str, theme: &Theme) -> String {
    let bg = card_bg(theme);
    let fg = card_fg(theme);
    let muted = card_muted(theme);
    let accent = texture::wave_ink(theme);

    let mut body = String::new();
    body.push_str(&wordmark(theme));
    body.push_str(&format!(
        "  <text x=\"80\" y=\"240\" fill=\"{accent}\" font-family=\"{FONT_MONO}\" font-size=\"34\" font-weight=\"600\" letter-spacing=\"0.02em\">@{}</text>\n",
        escape_xml(login),
    ));
    body.push_str(&format!(
        "  <text x=\"78\" y=\"330\" fill=\"{fg}\" font-family=\"{FONT_SANS}\" font-size=\"64\" font-weight=\"800\">no gitdebt data yet</text>\n",
    ));
    body.push_str(&format!(
        "  <text x=\"82\" y=\"388\" fill=\"{muted}\" font-family=\"{FONT_SANS}\" font-size=\"30\" font-weight=\"500\">analyze a repository at gitdebt.com to start tracking</text>\n",
    ));
    body.push_str(&density_ramp(theme));
    body.push_str(&footer(muted));

    wrap_svg(&body, bg, &format!("gitdebt · @{login}"))
}

/// Decorative lower-third Bayer density ramp: stacked horizontal bands of
/// increasing tier density (the pattern-quantized vertical gradient), one
/// ink, alpha-only. Purely geometric — carries no data.
fn density_ramp(theme: &Theme) -> String {
    let mut out = String::from("  <g data-gitdebt-ramp=\"true\">\n");
    // Six 18px bands from tier 3 up to tier 13, faint → present. (Tier 1
    // is already defined by the card envelope's grain wash; these ids must
    // stay disjoint from it.)
    const BANDS: [(usize, &str); 6] = [
        (3, "0.10"),
        (5, "0.14"),
        (7, "0.18"),
        (9, "0.22"),
        (11, "0.26"),
        (13, "0.30"),
    ];
    let top = SPARK_TOP;
    let band_h = (SPARK_BOTTOM - SPARK_TOP) / BANDS.len() as f32;
    out.push_str(&format!(
        "    <defs>{}</defs>\n",
        BANDS
            .iter()
            .map(|(tier, _)| texture::tier_pattern(theme.fg, 3.0, *tier))
            .collect::<Vec<_>>()
            .join(""),
    ));
    for (i, (tier, alpha)) in BANDS.iter().enumerate() {
        let y = top + i as f32 * band_h;
        out.push_str(&format!(
            "    <rect x=\"{SPARK_LEFT:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{band_h:.1}\" fill=\"{fill}\" fill-opacity=\"{alpha}\" />\n",
            w = SPARK_RIGHT - SPARK_LEFT,
            fill = texture::tier_fill(*tier),
        ));
    }
    out.push_str("  </g>\n");
    out
}

// Shared pieces

/// Y of the sparkline panel's top edge. The panel spans from here to
/// just above the footer.
const SPARK_TOP: f32 = 452.0;
const SPARK_BOTTOM: f32 = 560.0;
const SPARK_LEFT: f32 = 80.0;
const SPARK_RIGHT: f32 = OG_WIDTH as f32 - 80.0;

/// Compare-card per-repo row geometry: first text baseline + step.
const COMPARE_ROW_TOP: f32 = 320.0;
const COMPARE_ROW_STEP: f32 = 44.0;
/// Row baselines that fit between the subtitle and the sparkline panel:
/// 320 / 364 / 408. One more slot would land exactly on [`SPARK_TOP`]
/// (452) and the panel — painted after the rows — would cover it. Tied to
/// the geometry by `compare_row_slots_stay_above_sparkline_panel`.
const COMPARE_ROW_SLOTS: usize = 3;

/// Compare-title geometry: the 64px face at x=80 with a mirrored 80px
/// right margin.
const TITLE_X: f32 = 80.0;
const TITLE_FONT_SIZE: f32 = 64.0;
const TITLE_MAX_WIDTH: f32 = OG_WIDTH as f32 - 2.0 * TITLE_X;

/// Estimated advance (em) of one title glyph. The rasterizer resolves the
/// stack onto bundled Inter Regular (weight 800 falls back to it), whose
/// mixed-slug average is ~0.56em at this size. Estimation only has to be
/// safe, not exact: the wide/narrow classes err generous so an
/// adversarial all-'M' slug can't blow the budget the way a flat average
/// would, and over-estimation merely truncates a glyph early.
fn title_advance_em(c: char) -> f32 {
    match c {
        'm' | 'w' | 'M' | 'W' | '…' => 1.0,
        'i' | 'j' | 'l' | 'f' | 't' | 'r' | 'I' | '.' | '-' | '_' | ' ' => 0.45,
        'A'..='Z' => 0.80,
        '0'..='9' => 0.65,
        _ => 0.62,
    }
}

/// Estimated pixel width of a title glyph run at `font_size`.
fn title_width(text: &str, font_size: f32) -> f32 {
    text.chars().map(|c| title_advance_em(c) * font_size).sum()
}

/// Truncate `text` with a trailing ellipsis so its estimated width fits
/// `max_width` at `font_size`. Pure and deterministic; returns the input
/// unchanged when it already fits.
fn truncate_title(text: &str, font_size: f32, max_width: f32) -> String {
    if title_width(text, font_size) <= max_width {
        return text.to_string();
    }
    let budget = max_width - title_advance_em('…') * font_size;
    let mut out = String::new();
    let mut used = 0.0_f32;
    for c in text.chars() {
        let w = title_advance_em(c) * font_size;
        if used + w > budget {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// gitdebt wordmark top-left: monochrome robot mark + the wordmark in fg.
fn wordmark(theme: &Theme) -> String {
    let fg = card_fg(theme);
    format!(
        "{}  <text x=\"152\" y=\"108\" fill=\"{fg}\" font-family=\"{FONT_SANS}\" font-size=\"44\" font-weight=\"800\" letter-spacing=\"-0.01em\">gitdebt</text>\n",
        brand::logo_mark(84.0, 73.0, 56.0, fg),
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

    let single = active.len() == 1;
    for (i, (_, s)) in active.iter().enumerate() {
        let color = pal[i % pal.len()];
        let mut d = String::new();
        let mut first_x = plot_left;
        let mut last_x = plot_left;
        for (j, p) in s.iter().enumerate() {
            let x = plot_left + ((p.at.timestamp() as f32 - x_min) / x_span) * plot_w;
            let y = plot_bottom - (p.stars as f32 / y_max_f) * plot_h;
            if j == 0 {
                first_x = x;
                d.push_str(&format!("M {x:.1} {y:.1}"));
            } else {
                d.push_str(&format!(" L {x:.1} {y:.1}"));
            }
            last_x = x;
        }
        if single {
            // Dithered underfill — the chart system's pixel-fill signature.
            out.push_str(&format!(
                "  <path d=\"{d} L {last_x:.1} {plot_bottom:.1} L {first_x:.1} {plot_bottom:.1} Z\" fill=\"{fill}\" opacity=\"0.55\" />\n",
                fill = texture::FILL,
            ));
        } else {
            // Overlay: a dithered ghost under-stroke keeps each line's
            // texture without stacked fills fighting each other.
            out.push_str(&format!(
                "  <path d=\"{d}\" fill=\"none\" stroke=\"{fill}\" stroke-width=\"8\" opacity=\"0.38\" stroke-linecap=\"square\" stroke-linejoin=\"miter\" />\n",
                fill = texture::FILL,
            ));
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
    // The dark card carries the dark wave trio; the light card the light
    // trio. `bg` decides (wrap_svg predates a theme parameter here and the
    // two canvases are fixed hex).
    let theme = if bg == CARD_BG {
        &crate::theme::DARK
    } else {
        &crate::theme::LIGHT
    };
    // Sized texture defs (wave gradient spans the full 1200px) + the two
    // Bayer density tiers the card body uses. Pattern-based: no per-cell
    // rects at OG scale.
    let defs = format!(
        "{}\n  <defs>{}</defs>",
        texture::defs_sized(theme, OG_WIDTH as f32, OG_HEIGHT as f32),
        texture::tier_pattern(theme.fg, 3.0, 1),
    );
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-label="{label}">
  {defs}
  <rect x="0" y="0" width="{w}" height="{h}" fill="{bg}" />
  <rect x="0" y="0" width="{w}" height="{h}" fill="{grain}" fill-opacity="0.05" />
  <rect x="0" y="0" width="{w}" height="6" fill="url(#gd-dither-wave)" />
{body}</svg>"##,
        w = OG_WIDTH,
        h = OG_HEIGHT,
        bg = bg,
        grain = texture::tier_fill(1),
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
        assert!(svg.contains("#358ff3"));
        assert!(svg.contains("#966eff"));
    }

    /// Extract every `<text …>content</text>` element as `(x, y, content)`.
    /// Minimal deterministic scan for geometry assertions — the renderer
    /// always emits `x="…"` before `y="…"` on one line.
    fn text_elements(svg: &str) -> Vec<(f32, f32, String)> {
        let mut out = Vec::new();
        for (start, _) in svg.match_indices("<text ") {
            let rest = &svg[start..];
            let attrs_end = rest.find('>').expect("text tag closes");
            let attrs = &rest[..attrs_end];
            let attr = |name: &str| -> f32 {
                let marker = format!("{name}=\"");
                let from = attrs.find(&marker).expect("attr present") + marker.len();
                let to = attrs[from..].find('"').expect("attr closes") + from;
                attrs[from..to].parse().expect("numeric attr")
            };
            let content_end = rest.find("</text>").expect("text element closes");
            out.push((
                attr("x"),
                attr("y"),
                rest[attrs_end + 1..content_end].to_string(),
            ));
        }
        out
    }

    fn compare_entries(n: usize) -> Vec<CompareEntry> {
        (0..n)
            .map(|i| CompareEntry {
                slug: format!("owner/repo-{i:02}"),
                stars: 100 + i as u64,
                series: sample_series(5 + i as i64),
            })
            .collect()
    }

    /// The row-slot constant is derived from the panel geometry: every
    /// slot baseline sits a full step above `SPARK_TOP`, and one more
    /// slot would land exactly on the panel edge.
    #[test]
    fn compare_row_slots_stay_above_sparkline_panel() {
        let last = COMPARE_ROW_TOP + (COMPARE_ROW_SLOTS as f32 - 1.0) * COMPARE_ROW_STEP;
        assert!(
            last + COMPARE_ROW_STEP <= SPARK_TOP,
            "last slot baseline {last} must clear SPARK_TOP {SPARK_TOP} by a row step"
        );
        let next = COMPARE_ROW_TOP + COMPARE_ROW_SLOTS as f32 * COMPARE_ROW_STEP;
        assert!(next >= SPARK_TOP, "slot count is not derived from geometry");
    }

    /// A 12-repo compare must not paint rows on or under the sparkline
    /// panel (drawn after the rows): rows are capped, the omitted repos
    /// collapse into a "+N more" line, and every row/more baseline stays
    /// above `SPARK_TOP`.
    #[test]
    fn compare_card_caps_rows_and_adds_more_line() {
        let entries = compare_entries(12);
        let svg = render_compare_card(&entries, &DARK);
        // Two rows + the "+10 more" line occupy the three slots.
        assert!(svg.contains("repo-00 — "));
        assert!(svg.contains("repo-01 — "));
        assert!(!svg.contains("repo-02 — "), "row 3+ must be omitted");
        assert!(svg.contains(">+10 more<"));
        for (x, y, content) in text_elements(&svg) {
            let is_row = (x - 112.0).abs() < f32::EPSILON;
            let is_more = content.starts_with('+') && content.ends_with(" more");
            if is_row || is_more {
                assert!(
                    y < SPARK_TOP,
                    "baseline {y} of {content:?} reaches the sparkline panel at {SPARK_TOP}"
                );
            }
        }
    }

    /// Up to three repos fit without omission: all three rows draw at the
    /// slot baselines and no "+N more" line appears; a fourth repo trips
    /// the cap.
    #[test]
    fn compare_card_three_rows_fit_four_truncate() {
        let three = render_compare_card(&compare_entries(3), &DARK);
        for i in 0..3 {
            assert!(three.contains(&format!("repo-{i:02} — ")));
        }
        assert!(!three.contains(" more<"));
        for (x, y, _) in text_elements(&three) {
            if (x - 112.0).abs() < f32::EPSILON {
                assert!(y < SPARK_TOP);
            }
        }

        let four = render_compare_card(&compare_entries(4), &DARK);
        assert!(four.contains("repo-00 — "));
        assert!(four.contains("repo-01 — "));
        assert!(!four.contains("repo-02 — "));
        assert!(four.contains(">+2 more<"));
    }

    /// A many-slug title truncates with an ellipsis and its estimated
    /// glyph run never crosses the canvas edge (nor the mirrored right
    /// margin the budget encodes).
    #[test]
    fn compare_card_title_truncates_with_ellipsis_inside_canvas() {
        let entries: Vec<CompareEntry> = (0..12)
            .map(|i| CompareEntry {
                slug: format!("owner/some-rather-long-repository-name-{i:02}"),
                stars: 10,
                series: sample_series(4),
            })
            .collect();
        let svg = render_compare_card(&entries, &DARK);
        assert!(svg.contains('…'), "long title must be truncated");
        let (x, _, title) = text_elements(&svg)
            .into_iter()
            .find(|(_, _, content)| content.contains('…'))
            .expect("truncated title element");
        assert_eq!(x, TITLE_X);
        let end_x = x + title_width(&title, TITLE_FONT_SIZE);
        assert!(
            end_x <= TITLE_X + TITLE_MAX_WIDTH,
            "title run ends at {end_x}, past the {TITLE_MAX_WIDTH} budget"
        );
        assert!(end_x <= OG_WIDTH as f32, "title run crosses the canvas");
        // Deterministic under truncation too.
        assert_eq!(svg, render_compare_card(&entries, &DARK));
    }

    /// Short titles are untouched — no ellipsis, exact text preserved.
    #[test]
    fn compare_card_short_title_is_not_truncated() {
        let svg = render_compare_card(&compare_entries(2), &DARK);
        assert!(!svg.contains('…'));
        assert!(svg.contains("repo-00 vs repo-01"));
        assert_eq!(
            truncate_title("vue vs react", TITLE_FONT_SIZE, TITLE_MAX_WIDTH),
            "vue vs react"
        );
    }

    /// Raster-level verification of the title budget: adversarially wide
    /// glyphs ('M'/'w' runs — the widest classes) must leave the right
    /// gutter of the title band untouched in the rendered PNG. A flat
    /// average-advance estimate would let these run past the edge.
    #[test]
    fn compare_card_long_title_rasterizes_without_right_edge_overflow() {
        use crate::raster::{RasterFormat, rasterize};
        use resvg::tiny_skia::Pixmap;

        for slug_char in ['M', 'w', 'o'] {
            let name: String = std::iter::repeat_n(slug_char, 40).collect();
            let entries = vec![
                CompareEntry {
                    slug: format!("o/{name}"),
                    stars: 1,
                    series: sample_series(5),
                },
                CompareEntry {
                    slug: format!("o/{name}2"),
                    stars: 2,
                    series: sample_series(5),
                },
            ];
            let svg = render_compare_card(&entries, &DARK);
            let png = rasterize(&svg, RasterFormat::Png, 1.0).expect("rasterize compare card");
            let pixmap = Pixmap::decode_png(&png).expect("decode png");
            // Title band (64px face, baseline 212): the right gutter must
            // be pure background (dark canvas + ≤5% grain ≈ rgb(22,22,22)).
            for y in 150..=232 {
                for x in 1160..1200 {
                    let p = pixmap.pixel(x, y).expect("pixel in bounds");
                    assert!(
                        p.red() < 100 && p.green() < 100 && p.blue() < 100,
                        "glyph ink at ({x},{y}) for {slug_char:?} title — truncation budget too loose"
                    );
                }
            }
        }
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

    /// Social cards are the one surface large enough for the artwork's own
    /// dither fill, so they must carry it — and still be the same glyph.
    #[test]
    fn social_cards_carry_the_dithered_artwork() {
        for theme in [&LIGHT, &DARK] {
            let svg = render_default_card(theme);
            assert!(svg.contains("M320.5 110.5"));
            assert!(svg.contains("<pattern"), "the hero mark keeps its dither");
            assert!(
                !svg.contains("id=\"gitdebt-dither\""),
                "pattern id is scoped"
            );
        }
        // The user card's wordmark mark is small enough to go solid.
        let user = render_user_og(&sample_user_data(), &DARK);
        assert!(user.contains("M320.5 110.5"));
        let place = crate::brand::MarkBox::locate(&user, 1.0, card_fg(&DARK), card_bg(&DARK));
        let (mismatch, ink) = crate::brand::mark_fidelity(&user, place);
        assert!(mismatch < 0.05, "og wordmark drifted: {mismatch:.3}");
        assert!((0.25..0.75).contains(&ink));
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

    fn sample_user_data() -> UserCardData {
        UserCardData {
            login: "octocat".into(),
            stars: 12_345,
            commits: 987,
            contribs: 12,
            repos_tracked: 8,
            repos_analyzed: 8,
            forks: 456,
            since_year: Some(2015),
            langs: vec![("Rust".into(), 120_000)],
        }
    }

    #[test]
    fn user_og_is_deterministic_1200x630_and_dithered() {
        let a = render_user_og(&sample_user_data(), &DARK);
        let b = render_user_og(&sample_user_data(), &DARK);
        assert_eq!(a, b);
        assert!(a.contains("viewBox=\"0 0 1200 630\""));
        assert!(a.contains("width=\"1200\""));
        assert!(a.contains("height=\"630\""));
        assert!(a.contains("@octocat"));
        assert!(a.contains("12.3k")); // headline stars
        assert!(a.contains("987 commits"));
        assert!(a.contains("8 repos tracked"));
        // Dither language: wave accent + Bayer density ramp, no CSS vars.
        assert!(a.contains("#9b7bff"));
        assert!(a.contains("data-gitdebt-ramp=\"true\""));
        assert!(a.contains("url(#gd-t3)"));
        assert!(!a.contains("var(--"));
    }

    #[test]
    fn user_og_rasterizes_to_exact_dimensions() {
        use crate::raster::{RasterFormat, rasterize};
        let svg = render_user_og(&sample_user_data(), &DARK);
        let png = rasterize(&svg, RasterFormat::Png, 1.0).expect("user og png");
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!((w, h), (OG_WIDTH, OG_HEIGHT));
    }

    #[test]
    fn user_empty_og_renders_placeholder() {
        let svg = render_user_empty_og("ghost", &DARK);
        assert!(svg.contains("viewBox=\"0 0 1200 630\""));
        assert!(svg.contains("@ghost"));
        assert!(svg.contains("no gitdebt data yet"));
        assert_eq!(svg, render_user_empty_og("ghost", &DARK));
        // Escaping holds on the login path too.
        let evil = render_user_empty_og("<script>", &DARK);
        assert!(!evil.contains("<script>"));
    }

    #[test]
    fn og_cards_carry_the_wave_gradient_top_rule_and_grain() {
        for svg in [
            render_repo_card(
                &RepoCard {
                    slug: "o/r".into(),
                    stars: 5,
                    series: sample_series(10),
                    ..Default::default()
                },
                &DARK,
            ),
            render_default_card(&DARK),
            render_user_og(&sample_user_data(), &DARK),
        ] {
            assert!(svg.contains("fill=\"url(#gd-dither-wave)\""));
            assert!(svg.contains("id=\"gd-dither-wave\""));
            assert!(svg.contains("url(#gd-t1)"));
            // The gradient spans the real 1200px surface.
            assert!(svg.contains("x2=\"1200\""));
        }
        // Repo sparkline underfill is the dithered pixel fill.
        let repo = render_repo_card(
            &RepoCard {
                slug: "o/r".into(),
                stars: 5,
                series: sample_series(10),
                ..Default::default()
            },
            &DARK,
        );
        assert!(repo.contains("fill=\"url(#gd-pixel-fill)\""));
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
