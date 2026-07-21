//! Profile + repo stat cards (`/api/users/:login/card.svg`,
//! `/api/repos/:owner/:repo/card.svg`, plus `.png` / `.webp` variants).
//!
//! The user card uses gitdebt's own maintainer-footprint composition. Legacy
//! query parameters remain accepted for stable URLs, but the visual system is
//! intentionally independent rather than imitating another stats-card project.
//!
//! Every render function here is pure (`data + options + &Theme → SVG
//! String`) and bytes-deterministic. Theme colors are baked hex — no CSS
//! vars, no `prefers-color-scheme` (see `theme.rs` for the why; use the
//! `<picture>` light/dark pattern for theme-aware embeds). Animation
//! follows the `badge.rs` SMIL discipline: `animate=0` (default) emits no
//! `<animate>` tags at all; `animate=1` uses `<animate … fill="freeze">`
//! so a SMIL-stripped embed (and `raster::freeze_svg_animations`) shows the
//! correct final frame.
//!
//! ## Deliberately-unsupported github-readme-stats params
//!
//! Accepted-and-ignored so pasted GRS URLs keep working, but never
//! honored (product decisions, not gaps): `bg_color`, `title_color`,
//! `text_color`, `icon_color`, `border_color`, `ring_color`,
//! `border_radius` (two baked themes only — free hex would fragment the
//! CDN cache and defeat the palette), `locale` (English-only v0),
//! `cache_seconds` (fixed CDN policy), `line_height` / `text_bold` /
//! `number_precision` (fixed typographic scale), and every per-user
//! GitHub-API stat gitdebt does not observe: PRs, issues, reviews,
//! discussions, followers, streaks/contribution calendars,
//! `include_all_commits` / `commits_year`, and the `repo=`/`owner=`/
//! `role=` affiliation filters. Our stars/commits/contribs are **lower
//! bounds over tracked repos**; the mandatory "N repos tracked" footer
//! and the "Repos Tracked" label are the honesty framing that makes that
//! OK. Nothing here reads stargazer *profiles* — the user card consumes
//! only `repo_author_stats` commit-authorship aggregates and `repos`
//! ownership rows.

use crate::badge::humanize;
use crate::brand;
use crate::chart::{Point, palette};
use crate::theme::Theme;

// Shared option plumbing

/// Maintainer cards need enough width for the mark, editorial header, and
/// paired metrics without collapsing into a borrowed single-column layout.
pub const USER_CARD_DEFAULT_WIDTH: u32 = 560;
pub const USER_CARD_NORANK_WIDTH: u32 = 420;
pub const REPO_CARD_DEFAULT_WIDTH: u32 = 400;

/// Clamp a requested user-card width. `hide_rank` is retained in the API as a
/// legacy spelling for hiding the coverage rail, not as a second composition.
pub fn clamp_user_width(requested: Option<u32>, _hide_rank: bool) -> u32 {
    requested.map_or(USER_CARD_DEFAULT_WIDTH, |width| width.clamp(420, 1000))
}

/// Clamp a requested repo-card width to [320, 800] (default 400).
pub fn clamp_repo_width(requested: Option<u32>) -> u32 {
    requested.map_or(REPO_CARD_DEFAULT_WIDTH, |w| w.clamp(320, 800))
}

/// `number_format=short|long`. Short is `badge::humanize` ("12.3k");
/// long is comma-grouped ("12,345"). Unknown → short (the GRS default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberFormat {
    Short,
    Long,
}

impl NumberFormat {
    pub fn parse(s: Option<&str>) -> Self {
        match s.map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("long") => NumberFormat::Long,
            _ => NumberFormat::Short,
        }
    }

    pub fn format(self, n: u64) -> String {
        match self {
            NumberFormat::Short => humanize(n),
            NumberFormat::Long => group_thousands(n),
        }
    }
}

/// Comma-group a number: 1234567 → "1,234,567". Deterministic.
pub fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Strict GitHub-username validation for the `/api/users/...` card path:
/// ASCII alphanumeric + `-`, 1–39 chars, no leading/trailing hyphen.
/// Stricter than `aggregate::is_valid_login` on purpose — the user-card
/// SQL uses a `repo LIKE login || '/%'` prefix query, and this charset
/// contains no LIKE metacharacters (`_` / `%`), so the bound value can
/// never widen the match.
pub fn is_valid_login(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 39
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

/// Truncate to at most `max` chars (appending `…` when cut). Char-safe.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Resolve the display title: a non-empty `custom_title` (escaped,
/// truncated at 64 chars per the GRS convention) or the entity default.
fn display_title(custom: Option<&str>, default: &str) -> String {
    match custom.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => escape_xml(&truncate_chars(t, 64)),
        None => escape_xml(default),
    }
}

/// Card background per theme — concrete hex, matching the GitHub-native
/// look (`#0a0a0a` is the product's dark canvas).
fn card_bg(theme: &Theme) -> &'static str {
    if theme.dark { "#0a0a0a" } else { "#ffffff" }
}

/// Brand ink accent per theme (categorical palette index 0).
fn brand(theme: &Theme) -> &'static str {
    palette(theme)[0]
}

/// Approx px/char at the 12px repo-card value font.
const VALUE_CHAR_W: f32 = 7.0;

/// One shared `<style>` block (fonts only — colors are always inline
/// baked hex; classes carry no color so no CSS-variable leakage).
const CARD_STYLE: &str = "  <style><![CDATA[ \
.t { font: 600 18px ui-sans-serif, system-ui, sans-serif; } \
.ey { font: 700 9px ui-monospace, SFMono-Regular, monospace; letter-spacing: 1.1px; } \
.ml { font: 600 9px ui-monospace, SFMono-Regular, monospace; letter-spacing: 0.7px; } \
.mv { font: 650 22px ui-sans-serif, system-ui, sans-serif; letter-spacing: -0.5px; } \
.rt { font: 600 15px ui-sans-serif, system-ui, sans-serif; } \
.l { font: 400 14px ui-sans-serif, system-ui, sans-serif; } \
.v { font: 600 14px ui-sans-serif, system-ui, sans-serif; } \
.rv { font: 600 12px ui-sans-serif, system-ui, sans-serif; } \
.c { font: 500 12px ui-sans-serif, system-ui, sans-serif; } \
.m { font: 500 10px ui-sans-serif, system-ui, sans-serif; } \
.g { font: 800 24px ui-sans-serif, system-ui, sans-serif; } \
.p { font: 700 16px ui-sans-serif, system-ui, sans-serif; } \
@media (prefers-reduced-motion: reduce) { .motion { display: none; } } \
]]></style>\n";

fn svg_open(w: f32, h: f32, label: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.0} {h:.0}\" role=\"img\" aria-label=\"{label}\">\n"
    )
}

/// Card chrome: rounded rect, 1px border (opacity 0 when hidden — the
/// geometry stays identical so `hide_border` never reflows anything).
fn chrome(w: f32, h: f32, theme: &Theme, hide_border: bool) -> String {
    let mut out = format!(
        "  <rect x=\"0.5\" y=\"0.5\" width=\"{:.1}\" height=\"{:.1}\" rx=\"4.5\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1\" stroke-opacity=\"{}\" />\n",
        w - 1.0,
        h - 1.0,
        card_bg(theme),
        theme.border,
        if hide_border { "0" } else { "1" },
    );
    out.push_str(&brand::themed_logo_mark(12.0, h - 19.0, 10.0, theme));
    out
}

fn anim_group(animate: bool, index: usize) -> (String, &'static str) {
    if animate {
        let delay = (index as f32 * 0.04).min(0.08);
        (
            format!(
                "  <g opacity=\"1\"><animate class=\"motion\" attributeName=\"opacity\" from=\"0\" to=\"1\" begin=\"{delay:.2}s\" dur=\"0.2s\" fill=\"freeze\" />\n"
            ),
            "  </g>\n",
        )
    } else {
        ("  <g opacity=\"1\">\n".to_string(), "  </g>\n")
    }
}

// Glyphs (baked 16×16 path data; no external icon fetches)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphKind {
    Star,
    Fork,
    Commit,
    Branch,
    Repo,
    Clock,
    Code,
    Pulse,
}

/// A 16×16 glyph translated to (`x`, `y`), drawn in `color`. The inner
/// path data is fixed, so bytes stay deterministic.
fn glyph_svg(kind: GlyphKind, x: f32, y: f32, color: &str) -> String {
    let inner = match kind {
        GlyphKind::Star => format!(
            "<path d=\"M8 1.5 L9.9 5.6 14.4 6.1 11.1 9.2 12 13.6 8 11.4 4 13.6 4.9 9.2 1.6 6.1 6.1 5.6 Z\" fill=\"{color}\" />"
        ),
        GlyphKind::Fork => format!(
            "<g stroke=\"{color}\" stroke-width=\"1.6\" fill=\"none\"><circle cx=\"4\" cy=\"3.5\" r=\"1.8\" /><circle cx=\"12\" cy=\"3.5\" r=\"1.8\" /><circle cx=\"8\" cy=\"12.5\" r=\"1.8\" /><path d=\"M4 5.3 V7 H12 V5.3 M8 7 V10.7\" /></g>"
        ),
        GlyphKind::Commit => format!(
            "<g stroke=\"{color}\" stroke-width=\"1.6\" fill=\"none\"><circle cx=\"8\" cy=\"8\" r=\"3\" /><path d=\"M0.5 8 H5 M11 8 H15.5\" /></g>"
        ),
        GlyphKind::Branch => format!(
            "<g stroke=\"{color}\" stroke-width=\"1.6\" fill=\"none\"><circle cx=\"4\" cy=\"3\" r=\"1.8\" /><circle cx=\"4\" cy=\"13\" r=\"1.8\" /><circle cx=\"12\" cy=\"5\" r=\"1.8\" /><path d=\"M4 4.8 V11.2 M12 6.8 C12 9.5 8.5 9.3 5.8 10.5\" /></g>"
        ),
        GlyphKind::Repo => format!(
            "<g stroke=\"{color}\" stroke-width=\"1.5\" fill=\"none\"><path d=\"M3 2.5 H11.5 A1.5 1.5 0 0 1 13 4 V13.5 H5 A2 2 0 0 1 3 11.5 Z\" /><path d=\"M3 11.5 A2 2 0 0 1 5 9.5 H13\" /></g>"
        ),
        GlyphKind::Clock => format!(
            "<g stroke=\"{color}\" stroke-width=\"1.5\" fill=\"none\"><circle cx=\"8\" cy=\"8\" r=\"6\" /><path d=\"M8 4.5 V8 L10.8 9.8\" /></g>"
        ),
        GlyphKind::Code => format!(
            "<g stroke=\"{color}\" stroke-width=\"1.6\" fill=\"none\" stroke-linecap=\"round\"><path d=\"M5.5 4.5 L2 8 L5.5 11.5 M10.5 4.5 L14 8 L10.5 11.5\" /></g>"
        ),
        GlyphKind::Pulse => format!(
            "<path d=\"M1 8.5 H4.5 L6.5 3.5 9.5 12.5 11.5 8.5 H15\" stroke=\"{color}\" stroke-width=\"1.6\" fill=\"none\" stroke-linejoin=\"round\" />"
        ),
    };
    format!("<g transform=\"translate({x:.1} {y:.1})\">{inner}</g>")
}

// Rank (the GRS signature ring, over gitdebt-observable inputs)

/// Fixed-CDF grade, GRS-style but over gitdebt-observable inputs (GRS
/// weighs followers; we substitute forks — gitdebt never fetches
/// follower data). `exp_cdf(x) = 1 - 2^-x`; `log_normal_cdf(x) =
/// x / (1 + x)`. No population query — the thresholds are fixed:
///
/// ```text
/// percentile = (1 - (4·lncdf(stars/50) + 3·ecdf(commits/250)
///                  + 2·ecdf(contribs/5) + 1·lncdf(forks/10)) / 10) · 100
/// ```
pub fn rank(stars: u64, commits: u64, contribs: u64, forks: u64) -> (&'static str, f64) {
    fn exp_cdf(x: f64) -> f64 {
        1.0 - 2f64.powf(-x)
    }
    fn log_normal_cdf(x: f64) -> f64 {
        x / (1.0 + x)
    }
    let score = 4.0 * log_normal_cdf(stars as f64 / 50.0)
        + 3.0 * exp_cdf(commits as f64 / 250.0)
        + 2.0 * exp_cdf(contribs as f64 / 5.0)
        + log_normal_cdf(forks as f64 / 10.0);
    let percentile = (1.0 - score / 10.0) * 100.0;
    (rank_level(percentile), percentile)
}

/// Grade letter for a percentile. Thresholds identical to GRS: S = top
/// 1%, then A+/A/A-/B+/B/B-/C+ at 12.5-point steps, C for the rest.
pub fn rank_level(percentile: f64) -> &'static str {
    const LEVELS: [(f64, &str); 8] = [
        (1.0, "S"),
        (12.5, "A+"),
        (25.0, "A"),
        (37.5, "A-"),
        (50.0, "B+"),
        (62.5, "B"),
        (75.0, "B-"),
        (87.5, "C+"),
    ];
    for (threshold, level) in LEVELS {
        if percentile <= threshold {
            return level;
        }
    }
    "C"
}

/// Ring geometry: `r=40`, `stroke-width=6` — the GRS look.
pub const RING_RADIUS: f32 = 40.0;

pub fn ring_circumference() -> f32 {
    2.0 * std::f32::consts::PI * RING_RADIUS
}

/// Static (== final) dashoffset for a percentile: 0 = full ring (top of
/// the distribution), full circumference = empty ring.
pub fn ring_dashoffset(percentile: f64) -> f32 {
    (ring_circumference() as f64 * percentile / 100.0) as f32
}

// User profile card

/// User-card metric keys, GRS names where meanings align. `Since` and
/// `Langs` are `show=`-only extras.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserMetric {
    Stars,
    Commits,
    Contribs,
    Repos,
    Forks,
    Since,
    Langs,
}

impl UserMetric {
    fn parse(tok: &str) -> Option<Self> {
        match tok.trim().to_ascii_lowercase().as_str() {
            "stars" | "star" => Some(UserMetric::Stars),
            "commits" | "commit" => Some(UserMetric::Commits),
            "contribs" | "contributions" => Some(UserMetric::Contribs),
            "repos" | "repositories" => Some(UserMetric::Repos),
            "forks" | "fork" => Some(UserMetric::Forks),
            "since" => Some(UserMetric::Since),
            "langs" | "languages" => Some(UserMetric::Langs),
            _ => None,
        }
    }
}

/// Resolve the ordered user metric list from `hide=` / `show=`. Defaults
/// are the five core rows; `show=` appends the opt-in extras; `hide=`
/// removes; unknown tokens (e.g. pasted GRS `prs`, `issues`) are
/// silently ignored.
pub fn select_user_metrics(hide: Option<&str>, show: Option<&str>) -> Vec<UserMetric> {
    let mut out = vec![
        UserMetric::Stars,
        UserMetric::Commits,
        UserMetric::Contribs,
        UserMetric::Repos,
        UserMetric::Forks,
    ];
    if let Some(show) = show {
        for tok in show.split(',') {
            if let Some(m) = UserMetric::parse(tok)
                && !out.contains(&m)
            {
                out.push(m);
            }
        }
    }
    if let Some(hide) = hide {
        for tok in hide.split(',') {
            if let Some(m) = UserMetric::parse(tok) {
                out.retain(|x| *x != m);
            }
        }
    }
    out
}

/// Inputs for [`render_user_card`] — pre-aggregated by the API layer
/// from Postgres only (`repos` ownership rows + `repo_author_stats`
/// commit aggregates + `repo_lines`).
#[derive(Debug, Clone, Default)]
pub struct UserCardData {
    /// Lowercased, [`is_valid_login`]-validated login.
    pub login: String,
    pub stars: u64,
    pub commits: u64,
    /// Distinct tracked repos this login authored commits in.
    pub contribs: u64,
    /// Owned repos present in the `repos` table (the honesty headline).
    pub repos_tracked: u64,
    /// Owned tracked repos whose git history completed at least one pass.
    pub repos_analyzed: u64,
    pub forks: u64,
    /// Year of the earliest authored commit across tracked repos.
    pub since_year: Option<i32>,
    /// Top languages by `lines_code` across owned tracked repos.
    pub langs: Vec<(String, i64)>,
}

impl UserCardData {
    /// False when gitdebt has nothing at all for this login — the API
    /// layer serves the "no data yet" card instead.
    pub fn has_data(&self) -> bool {
        self.repos_tracked > 0 || self.commits > 0 || self.contribs > 0
    }

    /// Commit/contribution totals are still incomplete while this is true.
    pub fn analysis_pending(&self) -> bool {
        self.repos_tracked > self.repos_analyzed
    }
}

#[derive(Debug, Clone)]
pub struct UserCardOptions {
    pub metrics: Vec<UserMetric>,
    /// Already clamped via [`clamp_user_width`].
    pub width: u32,
    pub hide_border: bool,
    pub hide_title: bool,
    pub hide_rank: bool,
    /// `rank_icon=percentile` — numeric percentile instead of the grade.
    pub rank_icon_percentile: bool,
    pub custom_title: Option<String>,
    pub show_icons: bool,
    pub number_format: NumberFormat,
    pub animate: bool,
}

impl Default for UserCardOptions {
    fn default() -> Self {
        Self {
            metrics: select_user_metrics(None, None),
            width: USER_CARD_DEFAULT_WIDTH,
            hide_border: false,
            hide_title: false,
            hide_rank: false,
            rank_icon_percentile: false,
            custom_title: None,
            show_icons: true,
            number_format: NumberFormat::Short,
            animate: false,
        }
    }
}

enum UserRow {
    Stat {
        glyph: GlyphKind,
        label: &'static str,
        value: String,
    },
    Langs(Vec<String>),
}

fn user_rows(data: &UserCardData, opts: &UserCardOptions) -> Vec<UserRow> {
    let fmt = |n: u64| opts.number_format.format(n);
    let lower_bound = |n: u64| {
        if data.analysis_pending() {
            if n == 0 {
                "warming".to_string()
            } else {
                format!("{}+", fmt(n))
            }
        } else {
            fmt(n)
        }
    };
    let mut rows = Vec::new();
    for m in &opts.metrics {
        match m {
            UserMetric::Stars => rows.push(UserRow::Stat {
                glyph: GlyphKind::Star,
                label: "Total Stars Earned",
                value: fmt(data.stars),
            }),
            UserMetric::Commits => rows.push(UserRow::Stat {
                glyph: GlyphKind::Commit,
                label: "Commits Analyzed",
                value: lower_bound(data.commits),
            }),
            UserMetric::Contribs => rows.push(UserRow::Stat {
                glyph: GlyphKind::Branch,
                label: "Contributed To",
                value: lower_bound(data.contribs),
            }),
            UserMetric::Repos => rows.push(UserRow::Stat {
                glyph: GlyphKind::Repo,
                label: "Repos Tracked",
                value: fmt(data.repos_tracked),
            }),
            UserMetric::Forks => rows.push(UserRow::Stat {
                glyph: GlyphKind::Fork,
                label: "Total Forks",
                value: fmt(data.forks),
            }),
            UserMetric::Since => {
                if let Some(year) = data.since_year {
                    rows.push(UserRow::Stat {
                        glyph: GlyphKind::Clock,
                        label: "Contributing Since",
                        value: year.to_string(),
                    });
                }
            }
            UserMetric::Langs => {
                let names: Vec<String> = data
                    .langs
                    .iter()
                    .filter(|(_, lines)| *lines > 0)
                    .map(|(name, _)| name.clone())
                    .collect();
                if !names.is_empty() {
                    rows.push(UserRow::Langs(names));
                }
            }
        }
    }
    rows
}

/// Card height for the maintainer-footprint composition. Metrics form a
/// two-column grid; the optional legacy `hide_rank` switch hides the analysis
/// coverage rail rather than changing the card into a different layout.
pub fn user_card_height(rows: usize, hide_title: bool, hide_rank: bool) -> u32 {
    let header: u32 = if hide_title { 66 } else { 88 };
    let metric_rows = (rows as u32).div_ceil(2);
    let coverage: u32 = if hide_rank { 0 } else { 45 };
    (header + metric_rows * 58 + coverage + 34).max(184)
}

/// Render the user profile card. Pure + deterministic. `Err` when the
/// selection leaves nothing to draw (all stats hidden AND the analysis
/// coverage rail hidden).
pub fn render_user_card(
    data: &UserCardData,
    opts: &UserCardOptions,
    theme: &Theme,
) -> Result<String, &'static str> {
    let rows = user_rows(data, opts);
    if rows.is_empty() && opts.hide_rank {
        return Err("either metrics or analysis coverage are required");
    }
    let w = opts.width as f32;
    let h = user_card_height(rows.len(), opts.hide_title, opts.hide_rank) as f32;
    let pal0 = brand(theme);
    let login = escape_xml(&data.login);
    let title = display_title(opts.custom_title.as_deref(), &format!("@{}", data.login));

    let mut body = String::new();
    body.push_str(&format!(
        "  <rect x=\"0.5\" y=\"0.5\" width=\"{rw:.1}\" height=\"{rh:.1}\" rx=\"12\" fill=\"{bg}\" stroke=\"{border}\" stroke-width=\"1\" stroke-opacity=\"{stroke_opacity}\" />\n",
        rw = w - 1.0,
        rh = h - 1.0,
        bg = card_bg(theme),
        border = theme.border,
        stroke_opacity = if opts.hide_border { "0" } else { "1" },
    ));
    body.push_str(&brand::themed_logo_mark(24.0, 20.0, 34.0, theme));
    body.push_str(&format!(
        "  <text class=\"ey\" x=\"70\" y=\"32\" fill=\"{pal0}\">GITDEBT / MAINTAINER FOOTPRINT</text>\n",
    ));
    if !opts.hide_title {
        body.push_str(&format!(
            "  <text class=\"t\" x=\"70\" y=\"57\" fill=\"{}\">{title}</text>\n",
            theme.fg
        ));
    } else {
        body.push_str(&format!(
            "  <text class=\"m\" x=\"70\" y=\"52\" fill=\"{}\">@{login}</text>\n",
            theme.muted
        ));
    }

    let grid_y = if opts.hide_title { 66.0 } else { 88.0 };
    let gap = 12.0;
    let cell_w = (w - 48.0 - gap) / 2.0;
    for (i, row) in rows.iter().enumerate() {
        let col = (i % 2) as f32;
        let row_index = (i / 2) as f32;
        let x = 24.0 + col * (cell_w + gap);
        let y = grid_y + row_index * 58.0;
        let (open, close) = anim_group(opts.animate, i);
        body.push_str(&open);
        body.push_str(&format!(
            "    <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{cell_w:.1}\" height=\"46\" rx=\"7\" fill=\"{track}\" opacity=\"0.22\" />\n",
            track = theme.track,
        ));
        match row {
            UserRow::Stat {
                glyph,
                label,
                value,
            } => {
                let mut label_x = x + 12.0;
                if opts.show_icons {
                    body.push_str(&glyph_svg(*glyph, label_x, y + 7.0, pal0));
                    label_x += 21.0;
                }
                body.push_str(&format!(
                    "    <text class=\"ml\" x=\"{label_x:.1}\" y=\"{label_y:.1}\" fill=\"{muted}\">{label}</text><text class=\"mv\" x=\"{value_x:.1}\" y=\"{value_y:.1}\" text-anchor=\"end\" fill=\"{fg}\">{value}</text>\n",
                    label_y = y + 18.0,
                    value_x = x + cell_w - 12.0,
                    value_y = y + 36.0,
                    fg = theme.fg,
                    muted = theme.muted,
                    value = escape_xml(value),
                ));
            }
            UserRow::Langs(names) => {
                let mut label_x = x + 12.0;
                if opts.show_icons {
                    body.push_str(&glyph_svg(GlyphKind::Code, label_x, y + 7.0, pal0));
                    label_x += 21.0;
                }
                let joined = truncate_chars(
                    &names
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" · "),
                    24,
                );
                body.push_str(&format!(
                    "    <text class=\"ml\" x=\"{label_x:.1}\" y=\"{label_y:.1}\" fill=\"{muted}\">TOP LANGUAGES</text><text class=\"rv\" x=\"{value_x:.1}\" y=\"{value_y:.1}\" text-anchor=\"end\" fill=\"{fg}\">{joined}</text>\n",
                    label_y = y + 18.0,
                    value_x = x + cell_w - 12.0,
                    value_y = y + 36.0,
                    muted = theme.muted,
                    fg = theme.fg,
                    joined = escape_xml(&joined),
                ));
            }
        }
        body.push_str(close);
    }

    let metric_rows = rows.len().div_ceil(2) as f32;
    let mut footer_y = grid_y + metric_rows * 58.0;
    if !opts.hide_rank {
        let completion = if data.repos_tracked == 0 {
            0.0
        } else {
            (data.repos_analyzed as f64 / data.repos_tracked as f64).clamp(0.0, 1.0)
        };
        let rail_w = w - 48.0;
        let fill_w = rail_w * completion as f32;
        let status = if data.analysis_pending() {
            "ANALYSIS COVERAGE"
        } else {
            "ANALYSIS COMPLETE"
        };
        body.push_str(&format!(
            "  <g><text class=\"ml\" x=\"24\" y=\"{label_y:.1}\" fill=\"{muted}\">{status}</text><text class=\"ml\" x=\"{right:.1}\" y=\"{label_y:.1}\" text-anchor=\"end\" fill=\"{fg}\">{analyzed} / {tracked} REPOS</text><rect x=\"24\" y=\"{rail_y:.1}\" width=\"{rail_w:.1}\" height=\"5\" rx=\"2.5\" fill=\"{track}\" /><rect x=\"24\" y=\"{rail_y:.1}\" width=\"{fill_w:.1}\" height=\"5\" rx=\"2.5\" fill=\"{pal0}\" /></g>\n",
            label_y = footer_y + 13.0,
            rail_y = footer_y + 23.0,
            right = w - 24.0,
            muted = theme.muted,
            fg = theme.fg,
            analyzed = data.repos_analyzed,
            tracked = data.repos_tracked,
            track = theme.track,
        ));
        footer_y += 45.0;
    }

    body.push_str(&format!(
        "  <a href=\"https://gitdebt.com/u/{login}\" target=\"_blank\" rel=\"noopener\"><text class=\"m\" x=\"{x:.1}\" y=\"{y:.1}\" text-anchor=\"end\" fill=\"{muted}\">gitdebt.com/u/{login} ↗</text></a>\n",
        x = w - 25.0,
        y = footer_y + 20.0,
        muted = theme.muted,
    ));

    Ok(format!(
        "{open}{CARD_STYLE}{body}</svg>",
        open = svg_open(w, h, &format!("{login} gitdebt stats")),
    ))
}

/// Placeholder for a login gitdebt knows nothing about. Static; the API
/// layer serves it with a short TTL so the embed self-heals — and never
/// enqueues anything (there is no per-user fetch pipeline, on purpose).
pub fn render_user_empty_card(login: &str, theme: &Theme) -> String {
    notice_card(
        &format!("@{login}"),
        "no gitdebt data yet — analyze a repo at gitdebt.com",
        theme,
    )
}

// Repo stats card

/// Repo-card metric keys. Defaults: stars, forks, contributors, commits,
/// age. `show=` extras: lines, commits_30d, stars_30d.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoMetric {
    Stars,
    Forks,
    Contributors,
    Commits,
    Age,
    Lines,
    Commits30d,
    Stars30d,
}

impl RepoMetric {
    fn parse(tok: &str) -> Option<Self> {
        match tok.trim().to_ascii_lowercase().as_str() {
            "stars" | "star" => Some(RepoMetric::Stars),
            "forks" | "fork" => Some(RepoMetric::Forks),
            "contributors" | "contribs" => Some(RepoMetric::Contributors),
            "commits" | "commit" => Some(RepoMetric::Commits),
            "age" | "since" => Some(RepoMetric::Age),
            "lines" | "loc" => Some(RepoMetric::Lines),
            "commits_30d" => Some(RepoMetric::Commits30d),
            "stars_30d" => Some(RepoMetric::Stars30d),
            _ => None,
        }
    }
}

/// Resolve the ordered repo metric list from `hide=` / `show=` (same
/// semantics as [`select_user_metrics`]).
pub fn select_repo_metrics(hide: Option<&str>, show: Option<&str>) -> Vec<RepoMetric> {
    let mut out = vec![
        RepoMetric::Stars,
        RepoMetric::Forks,
        RepoMetric::Contributors,
        RepoMetric::Commits,
        RepoMetric::Age,
    ];
    if let Some(show) = show {
        for tok in show.split(',') {
            if let Some(m) = RepoMetric::parse(tok)
                && !out.contains(&m)
            {
                out.push(m);
            }
        }
    }
    if let Some(hide) = hide {
        for tok in hide.split(',') {
            if let Some(m) = RepoMetric::parse(tok) {
                out.retain(|x| *x != m);
            }
        }
    }
    out
}

/// One point of the 90-day cumulative star sparkline: unix seconds +
/// cumulative total. Produced by [`spark_window`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkPoint {
    pub t: i64,
    pub v: u64,
}

/// Windowed + downsampled sparkline points from a complete cumulative
/// star series (ascending). Takes the trailing `window_days` relative to
/// the series' **last** timestamp (not the wall clock — determinism), and
/// downsamples to at most `max_points`. When fewer than 2 points fall in
/// the window, the last pre-window point is prepended so a quiet repo
/// still draws a flat line. Empty input → empty output (sparkline
/// omitted; the caller must never approximate from a partial series).
pub fn spark_window(series: &[Point], window_days: i64, max_points: usize) -> Vec<SparkPoint> {
    let Some(last) = series.last() else {
        return Vec::new();
    };
    let start = last.at - chrono::Duration::days(window_days);
    let idx = series.partition_point(|p| p.at < start);
    let from = if series.len() - idx < 2 && idx > 0 {
        idx - 1
    } else {
        idx
    };
    let window: Vec<Point> = series[from..].to_vec();
    crate::chart::downsample(&window, max_points)
        .into_iter()
        .map(|p| SparkPoint {
            t: p.at.timestamp(),
            v: p.stars as u64,
        })
        .collect()
}

/// Normalized language shares (of the provided rows' total), dropping
/// non-positive rows. Input order preserved (callers pass lines-desc).
pub fn lang_shares(langs: &[(String, i64)]) -> Vec<(String, f64)> {
    let rows: Vec<&(String, i64)> = langs.iter().filter(|(_, l)| *l > 0).collect();
    let total: i64 = rows.iter().map(|(_, l)| *l).sum();
    if total <= 0 {
        return Vec::new();
    }
    rows.iter()
        .map(|(name, lines)| (name.clone(), *lines as f64 / total as f64))
        .collect()
}

/// Inputs for [`render_repo_card`]. `None` = unavailable → the cell is
/// dropped (never rendered as a fake zero). `spark` empty = no complete
/// star history → the sparkline is omitted entirely.
#[derive(Debug, Clone, Default)]
pub struct RepoCardData {
    /// Lowercased `owner/repo` slug (validated by the API layer).
    pub slug: String,
    pub stars: Option<u64>,
    pub forks: Option<u64>,
    pub contributors: Option<u64>,
    pub commits: Option<u64>,
    pub created_year: Option<i32>,
    pub lines_total: Option<u64>,
    pub commits_30d: Option<u64>,
    /// Only ever `Some` when the star history is complete.
    pub stars_30d: Option<u64>,
    /// Top languages by `lines_code` (desc).
    pub langs: Vec<(String, i64)>,
    pub spark: Vec<SparkPoint>,
}

#[derive(Debug, Clone)]
pub struct RepoCardOptions {
    pub metrics: Vec<RepoMetric>,
    /// Already clamped via [`clamp_repo_width`].
    pub width: u32,
    pub hide_border: bool,
    pub custom_title: Option<String>,
    pub show_icons: bool,
    pub number_format: NumberFormat,
    pub animate: bool,
}

impl Default for RepoCardOptions {
    fn default() -> Self {
        Self {
            metrics: select_repo_metrics(None, None),
            width: REPO_CARD_DEFAULT_WIDTH,
            hide_border: false,
            custom_title: None,
            show_icons: true,
            number_format: NumberFormat::Short,
            animate: false,
        }
    }
}

struct RepoCell {
    glyph: GlyphKind,
    value: String,
    label: &'static str,
}

fn repo_cells(data: &RepoCardData, opts: &RepoCardOptions) -> Vec<RepoCell> {
    let fmt = |n: u64| opts.number_format.format(n);
    let mut cells = Vec::new();
    for m in &opts.metrics {
        let cell = match m {
            RepoMetric::Stars => data.stars.map(|v| RepoCell {
                glyph: GlyphKind::Star,
                value: fmt(v),
                label: "stars",
            }),
            RepoMetric::Forks => data.forks.map(|v| RepoCell {
                glyph: GlyphKind::Fork,
                value: fmt(v),
                label: "forks",
            }),
            RepoMetric::Contributors => data.contributors.map(|v| RepoCell {
                glyph: GlyphKind::Branch,
                value: fmt(v),
                label: "contributors",
            }),
            RepoMetric::Commits => data.commits.map(|v| RepoCell {
                glyph: GlyphKind::Commit,
                value: fmt(v),
                label: "commits analyzed",
            }),
            RepoMetric::Age => data.created_year.map(|y| RepoCell {
                glyph: GlyphKind::Clock,
                value: y.to_string(),
                label: "since",
            }),
            RepoMetric::Lines => data.lines_total.map(|v| RepoCell {
                glyph: GlyphKind::Code,
                value: fmt(v),
                label: "lines of code",
            }),
            RepoMetric::Commits30d => data.commits_30d.map(|v| RepoCell {
                glyph: GlyphKind::Pulse,
                value: fmt(v),
                label: "commits · 30d",
            }),
            RepoMetric::Stars30d => data.stars_30d.map(|v| RepoCell {
                glyph: GlyphKind::Star,
                value: format!("+{}", fmt(v)),
                label: "stars · 30d",
            }),
        };
        if let Some(cell) = cell {
            cells.push(cell);
        }
    }
    cells
}

/// Repo-card height: header + the taller of (metric grid, sparkline) +
/// optional language strip + footer. Grows 24px per grid row.
pub fn repo_card_height(grid_rows: usize, has_spark: bool, has_langs: bool) -> u32 {
    let content = std::cmp::max(grid_rows as u32 * 24, if has_spark { 52 } else { 0 });
    let mut h = 44 + content + 22;
    if has_langs {
        h += 38;
    }
    h.max(110)
}

/// Render the repo stats card. Pure + deterministic. `Err` only when the
/// `hide=` selection removed every metric key (→ 400 upstream);
/// data-unavailable cells are silently dropped instead.
pub fn render_repo_card(
    data: &RepoCardData,
    opts: &RepoCardOptions,
    theme: &Theme,
) -> Result<String, &'static str> {
    if opts.metrics.is_empty() {
        return Err("no metrics selected");
    }
    let cells = repo_cells(data, opts);
    let has_spark = data.spark.len() >= 2;
    let shares = lang_shares(&data.langs);
    let has_langs = !shares.is_empty();
    let grid_rows = cells.len().div_ceil(2);
    let w = opts.width as f32;
    let h = repo_card_height(grid_rows, has_spark, has_langs) as f32;
    let pal0 = brand(theme);
    let slug = escape_xml(&data.slug);
    let title = display_title(opts.custom_title.as_deref(), &data.slug);

    let mut body = String::new();
    body.push_str(&chrome(w, h, theme, opts.hide_border));

    // Header: repo glyph + full slug, linked to GitHub.
    body.push_str(&format!(
        "  <a href=\"https://github.com/{slug}\" target=\"_blank\" rel=\"noopener\">{glyph}<text class=\"rt\" x=\"47\" y=\"28\" fill=\"{fg}\">{title}</text></a>\n",
        glyph = glyph_svg(GlyphKind::Repo, 25.0, 14.0, pal0),
        fg = theme.fg,
    ));

    // Metric grid (2 columns, left zone).
    let grid_w = if has_spark { w - 190.0 } else { w - 50.0 };
    let col_w = grid_w / 2.0;
    for (i, cell) in cells.iter().enumerate() {
        let col = (i % 2) as f32;
        let row = (i / 2) as f32;
        let x = 25.0 + col * col_w;
        let y = 64.0 + row * 24.0;
        let (open, close) = anim_group(opts.animate, i);
        body.push_str(&open);
        let mut tx = x;
        if opts.show_icons {
            body.push_str(&glyph_svg(cell.glyph, x, y - 12.0, pal0));
            tx += 22.0;
        }
        let label_x = tx + cell.value.chars().count() as f32 * VALUE_CHAR_W + 6.0;
        body.push_str(&format!(
            "<text class=\"rv\" x=\"{tx:.1}\" y=\"{y:.1}\" fill=\"{fg}\">{value}</text><text class=\"m\" x=\"{label_x:.1}\" y=\"{y:.1}\" fill=\"{muted}\">{label}</text>",
            fg = theme.fg,
            muted = theme.muted,
            value = escape_xml(&cell.value),
            label = cell.label,
        ));
        body.push_str(close);
    }

    // 90-day star sparkline (right zone; only when history is complete).
    if has_spark {
        body.push_str(&spark_svg(&data.spark, w - 155.0, 48.0, 130.0, 42.0, pal0));
    }

    // Language strip: stacked bar + up to 3 legend chips.
    if has_langs {
        let content = std::cmp::max(grid_rows as u32 * 24, if has_spark { 52 } else { 0 }) as f32;
        let bar_y = 44.0 + content + 6.0;
        body.push_str(&lang_strip(&shares, 25.0, bar_y, w - 50.0, theme));
    }

    body.push_str(&format!(
        "  <a href=\"https://gitdebt.com/{slug}\" target=\"_blank\" rel=\"noopener\"><text class=\"m\" x=\"{x:.1}\" y=\"{y:.1}\" text-anchor=\"end\" fill=\"{muted}\">via gitdebt</text></a>\n",
        x = w - 25.0,
        y = h - 10.0,
        muted = theme.muted,
    ));

    Ok(format!(
        "{open}{CARD_STYLE}{body}</svg>",
        open = svg_open(w, h, &format!("{slug} gitdebt stats")),
    ))
}

/// Cumulative-star sparkline: line + 12%-opacity area fill inside the
/// (`x`, `y`, `w`, `h`) box. Requires ≥2 points (callers gate on that).
fn spark_svg(points: &[SparkPoint], x: f32, y: f32, w: f32, h: f32, color: &str) -> String {
    let t0 = points.first().map(|p| p.t).unwrap_or(0);
    let t1 = points.last().map(|p| p.t).unwrap_or(0);
    let span = (t1 - t0).max(1) as f64;
    let vmin = points.iter().map(|p| p.v).min().unwrap_or(0);
    let vmax = points.iter().map(|p| p.v).max().unwrap_or(0);
    let vspan = (vmax.saturating_sub(vmin)).max(1) as f64;
    let flat = vmax == vmin;
    let mut line = String::new();
    for (i, p) in points.iter().enumerate() {
        let px = x + ((p.t - t0) as f64 / span) as f32 * w;
        let py = if flat {
            y + h / 2.0
        } else {
            y + h - ((p.v - vmin) as f64 / vspan) as f32 * h
        };
        line.push_str(if i == 0 { "M" } else { " L" });
        line.push_str(&format!("{px:.1} {py:.1}"));
    }
    let last_x = x + w;
    format!(
        "  <g class=\"spark\"><path d=\"{line} L{last_x:.1} {bottom:.1} L{x:.1} {bottom:.1} Z\" fill=\"{color}\" opacity=\"0.12\" stroke=\"none\" /><path d=\"{line}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.6\" stroke-linejoin=\"round\" /></g>\n",
        bottom = y + h,
    )
}

/// Full-width 8px stacked language bar + up to 3 `name pct%` chips.
fn lang_strip(shares: &[(String, f64)], x: f32, y: f32, w: f32, theme: &Theme) -> String {
    let pal = palette(theme);
    let mut out = String::from("  <g class=\"langs\">");
    let mut sx = x;
    for (i, (_, share)) in shares.iter().enumerate() {
        let seg_w = (*share as f32) * w;
        out.push_str(&format!(
            "<rect x=\"{sx:.1}\" y=\"{y:.1}\" width=\"{seg_w:.1}\" height=\"8\" rx=\"2\" fill=\"{color}\" />",
            color = pal[i % pal.len()],
        ));
        sx += seg_w;
    }
    let legend_y = y + 21.0;
    let mut lx = x;
    for (i, (name, share)) in shares.iter().take(3).enumerate() {
        let text = format!("{name} {:.1}%", share * 100.0);
        out.push_str(&format!(
            "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"3.5\" fill=\"{color}\" /><text class=\"m\" x=\"{tx:.1}\" y=\"{ty:.1}\" fill=\"{muted}\">{text}</text>",
            cx = lx + 3.5,
            cy = legend_y - 3.5,
            color = pal[i % pal.len()],
            tx = lx + 11.0,
            ty = legend_y,
            muted = theme.muted,
            text = escape_xml(&text),
        ));
        lx += 11.0 + text.chars().count() as f32 * 5.4 + 12.0;
    }
    out.push_str("</g>\n");
    out
}

/// Tombstoned (404) repo → a plain, honest "not found" card. Cacheable
/// at the standard TTL (the tombstone is terminal).
pub fn render_repo_missing_card(slug: &str, theme: &Theme) -> String {
    notice_card(slug, "repo not found on GitHub", theme)
}

/// Cold repo (no star history AND no clone analysis yet). The API layer
/// serves it with a short TTL so README embeds self-heal once the
/// existing queues catch up — no new enqueue path.
pub fn render_repo_pending_card(slug: &str, stars: Option<u64>, theme: &Theme) -> String {
    let msg = match stars {
        Some(n) => format!("{} stars · analysis pending — check back soon", humanize(n)),
        None => "analysis pending — check back soon".to_string(),
    };
    notice_card(slug, &msg, theme)
}

/// Shared minimal notice card (400×100): title line + muted message.
fn notice_card(title: &str, message: &str, theme: &Theme) -> String {
    let (w, h) = (400.0_f32, 100.0_f32);
    format!(
        "{open}{CARD_STYLE}{chrome}  <text class=\"rt\" x=\"25\" y=\"42\" fill=\"{fg}\">{title}</text>\n  <text class=\"c\" x=\"25\" y=\"66\" fill=\"{muted}\">{message}</text>\n  <a href=\"https://gitdebt.com\" target=\"_blank\" rel=\"noopener\"><text class=\"m\" x=\"{fx:.1}\" y=\"{fy:.1}\" text-anchor=\"end\" fill=\"{muted}\">via gitdebt</text></a>\n</svg>",
        open = svg_open(w, h, "gitdebt"),
        chrome = chrome(w, h, theme, false),
        fg = theme.fg,
        muted = theme.muted,
        title = escape_xml(&truncate_chars(title, 48)),
        message = escape_xml(message),
        fx = w - 25.0,
        fy = h - 12.0,
    )
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};
    use chrono::{DateTime, Duration, TimeZone, Utc};

    fn sample_user() -> UserCardData {
        UserCardData {
            login: "octocat".into(),
            stars: 12_345,
            commits: 987,
            contribs: 12,
            repos_tracked: 8,
            repos_analyzed: 8,
            forks: 456,
            since_year: Some(2015),
            langs: vec![
                ("Rust".into(), 120_000),
                ("TypeScript".into(), 30_000),
                ("Go".into(), 0),
            ],
        }
    }

    fn spark_pts(n: usize) -> Vec<SparkPoint> {
        (0..n)
            .map(|i| SparkPoint {
                t: 1_700_000_000 + i as i64 * 86_400,
                v: (i * i) as u64,
            })
            .collect()
    }

    fn sample_repo() -> RepoCardData {
        RepoCardData {
            slug: "rust-lang/rust".into(),
            stars: Some(95_000),
            forks: Some(12_000),
            contributors: Some(4_800),
            commits: Some(250_000),
            created_year: Some(2010),
            lines_total: Some(3_000_000),
            commits_30d: Some(900),
            stars_30d: Some(1_200),
            langs: vec![("Rust".into(), 2_500_000), ("Python".into(), 100_000)],
            spark: spark_pts(30),
        }
    }

    fn full_repo_opts(animate: bool) -> RepoCardOptions {
        RepoCardOptions {
            metrics: select_repo_metrics(None, Some("lines,commits_30d,stars_30d")),
            animate,
            ..RepoCardOptions::default()
        }
    }

    #[test]
    fn login_validation_is_strict() {
        assert!(is_valid_login("torvalds"));
        assert!(is_valid_login("rust-lang"));
        assert!(is_valid_login("a"));
        assert!(is_valid_login(&"a".repeat(39)));
        // Rejected: LIKE metacharacters, dots, traversal, hyphen edges.
        assert!(!is_valid_login(""));
        assert!(!is_valid_login(&"a".repeat(40)));
        assert!(!is_valid_login("a_b")); // `_` is a LIKE metachar
        assert!(!is_valid_login("a%b")); // `%` is a LIKE metachar
        assert!(!is_valid_login("a.b"));
        assert!(!is_valid_login(".."));
        assert!(!is_valid_login("-lead"));
        assert!(!is_valid_login("trail-"));
        assert!(!is_valid_login("a/b"));
        assert!(!is_valid_login("héllo"));
    }

    #[test]
    fn number_format_short_and_long() {
        assert_eq!(NumberFormat::Short.format(12_345), "12.3k");
        assert_eq!(NumberFormat::Long.format(12_345), "12,345");
        assert_eq!(NumberFormat::Long.format(999), "999");
        assert_eq!(NumberFormat::Long.format(1_234_567), "1,234,567");
        assert_eq!(NumberFormat::parse(Some("long")), NumberFormat::Long);
        assert_eq!(NumberFormat::parse(Some("LONG")), NumberFormat::Long);
        assert_eq!(NumberFormat::parse(Some("garbage")), NumberFormat::Short);
        assert_eq!(NumberFormat::parse(None), NumberFormat::Short);
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate_chars("short", 64), "short");
        let cut = truncate_chars(&"é".repeat(100), 10);
        assert_eq!(cut.chars().count(), 10); // 9 chars + ellipsis
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn width_clamps() {
        assert_eq!(clamp_user_width(None, false), 560);
        assert_eq!(clamp_user_width(None, true), 560);
        assert_eq!(clamp_user_width(Some(100), false), 420);
        assert_eq!(clamp_user_width(Some(100), true), 420);
        assert_eq!(clamp_user_width(Some(5_000), false), 1000);
        assert_eq!(clamp_repo_width(None), 400);
        assert_eq!(clamp_repo_width(Some(10)), 320);
        assert_eq!(clamp_repo_width(Some(9_999)), 800);
    }

    #[test]
    fn rank_all_zero_is_c_at_100() {
        let (level, pct) = rank(0, 0, 0, 0);
        assert_eq!(level, "C");
        assert!((pct - 100.0).abs() < 1e-9);
    }

    #[test]
    fn rank_huge_inputs_reach_s() {
        let (level, pct) = rank(10_000_000, 10_000_000, 10_000_000, 10_000_000);
        assert_eq!(level, "S");
        assert!(pct <= 1.0, "percentile {pct} should be top-1%");
    }

    #[test]
    fn rank_is_monotonic_in_each_argument() {
        let base = rank(100, 100, 5, 10).1;
        assert!(rank(1_000, 100, 5, 10).1 < base);
        assert!(rank(100, 1_000, 5, 10).1 < base);
        assert!(rank(100, 100, 50, 10).1 < base);
        assert!(rank(100, 100, 5, 100).1 < base);
    }

    #[test]
    fn rank_level_threshold_edges() {
        assert_eq!(rank_level(0.0), "S");
        assert_eq!(rank_level(1.0), "S");
        assert_eq!(rank_level(1.01), "A+");
        assert_eq!(rank_level(12.5), "A+");
        assert_eq!(rank_level(25.0), "A");
        assert_eq!(rank_level(37.5), "A-");
        assert_eq!(rank_level(50.0), "B+");
        assert_eq!(rank_level(62.5), "B");
        assert_eq!(rank_level(75.0), "B-");
        assert_eq!(rank_level(87.5), "C+");
        assert_eq!(rank_level(100.0), "C");
    }

    #[test]
    fn ring_math_is_exact() {
        let circ = ring_circumference();
        assert!((circ - 251.327).abs() < 0.01, "circumference {circ}");
        assert!((ring_dashoffset(0.0) - 0.0).abs() < 1e-6);
        assert!((ring_dashoffset(100.0) - circ).abs() < 1e-3);
        assert!((ring_dashoffset(50.0) - circ / 2.0).abs() < 1e-3);
    }

    #[test]
    fn user_metric_defaults_and_extras() {
        let d = select_user_metrics(None, None);
        assert_eq!(
            d,
            vec![
                UserMetric::Stars,
                UserMetric::Commits,
                UserMetric::Contribs,
                UserMetric::Repos,
                UserMetric::Forks
            ]
        );
        let with_extras = select_user_metrics(None, Some("since,langs"));
        assert!(with_extras.contains(&UserMetric::Since));
        assert!(with_extras.contains(&UserMetric::Langs));
        let hidden = select_user_metrics(Some("forks,repos"), None);
        assert!(!hidden.contains(&UserMetric::Forks));
        assert!(!hidden.contains(&UserMetric::Repos));
        // Unknown keys (pasted GRS URLs) are silently ignored.
        let grs = select_user_metrics(Some("prs,issues"), Some("reviews,discussions_started"));
        assert_eq!(grs, d);
    }

    #[test]
    fn repo_metric_defaults_and_extras() {
        let d = select_repo_metrics(None, None);
        assert_eq!(d.len(), 5);
        assert!(!d.contains(&RepoMetric::Lines));
        let with = select_repo_metrics(None, Some("lines,commits_30d,stars_30d,bogus"));
        assert!(with.contains(&RepoMetric::Lines));
        assert!(with.contains(&RepoMetric::Commits30d));
        assert!(with.contains(&RepoMetric::Stars30d));
        let none = select_repo_metrics(Some("stars,forks,contributors,commits,age"), None);
        assert!(none.is_empty());
    }

    #[test]
    fn user_card_deterministic() {
        let opts = UserCardOptions {
            animate: true,
            metrics: select_user_metrics(None, Some("since,langs")),
            ..UserCardOptions::default()
        };
        let a = render_user_card(&sample_user(), &opts, &DARK).unwrap();
        let b = render_user_card(&sample_user(), &opts, &DARK).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn pending_user_analysis_never_presents_zero_authorship_as_final() {
        let mut data = sample_user();
        data.commits = 0;
        data.contribs = 0;
        data.repos_analyzed = 0;
        let svg = render_user_card(&data, &UserCardOptions::default(), &LIGHT).unwrap();
        assert!(svg.contains("Commits Analyzed"));
        assert!(svg.contains(">warming</text>"));
        assert!(svg.contains("0 / 8 REPOS"));
        assert!(!svg.contains(">0</text>"));
    }

    #[test]
    fn repo_card_deterministic() {
        let a = render_repo_card(&sample_repo(), &full_repo_opts(true), &LIGHT).unwrap();
        let b = render_repo_card(&sample_repo(), &full_repo_opts(true), &LIGHT).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn theme_colors_are_baked() {
        let light = render_user_card(&sample_user(), &UserCardOptions::default(), &LIGHT).unwrap();
        let dark = render_user_card(&sample_user(), &UserCardOptions::default(), &DARK).unwrap();
        assert!(light.contains("#0a0a0a"));
        assert!(dark.contains("#fafafa"));
        assert!(!light.contains("var(--"));
        assert!(!dark.contains("var(--"));
        let rlight = render_repo_card(&sample_repo(), &full_repo_opts(false), &LIGHT).unwrap();
        let rdark = render_repo_card(&sample_repo(), &full_repo_opts(false), &DARK).unwrap();
        assert!(rlight.contains("#0a0a0a"));
        assert!(rdark.contains("#fafafa"));
        assert!(!rlight.contains("var(--"));
        assert!(!rdark.contains("var(--"));
    }

    #[test]
    fn hide_removes_row_from_svg() {
        let all = render_user_card(&sample_user(), &UserCardOptions::default(), &LIGHT).unwrap();
        assert!(all.contains("Total Forks"));
        let opts = UserCardOptions {
            metrics: select_user_metrics(Some("forks"), None),
            ..UserCardOptions::default()
        };
        let hidden = render_user_card(&sample_user(), &opts, &LIGHT).unwrap();
        assert!(!hidden.contains("Total Forks"));
        assert!(hidden.contains("Total Stars Earned"));
    }

    #[test]
    fn hiding_everything_plus_hide_rank_errors() {
        let opts = UserCardOptions {
            metrics: select_user_metrics(Some("stars,commits,contribs,repos,forks"), None),
            hide_rank: true,
            ..UserCardOptions::default()
        };
        assert!(render_user_card(&sample_user(), &opts, &LIGHT).is_err());
        // Same selection with the rank visible still renders.
        let with_rank = UserCardOptions {
            metrics: select_user_metrics(Some("stars,commits,contribs,repos,forks"), None),
            hide_rank: false,
            ..UserCardOptions::default()
        };
        assert!(render_user_card(&sample_user(), &with_rank, &LIGHT).is_ok());
        // Empty repo-card selection errors too.
        let ropts = RepoCardOptions {
            metrics: Vec::new(),
            ..RepoCardOptions::default()
        };
        assert!(render_repo_card(&sample_repo(), &ropts, &LIGHT).is_err());
    }

    #[test]
    fn height_grows_with_rows() {
        assert!(user_card_height(6, false, false) > user_card_height(3, false, false));
        assert!(user_card_height(3, true, true) < user_card_height(3, false, true));
        assert!(user_card_height(3, true, false) < user_card_height(3, false, false));
        assert!(repo_card_height(4, false, false) > repo_card_height(1, false, false));
        assert!(repo_card_height(1, false, true) > repo_card_height(1, false, false));
        assert_eq!(user_card_height(0, false, false), 184);
    }

    #[test]
    fn animation_tags_only_when_animate_true() {
        let render_all = |animate: bool| {
            let uopts = UserCardOptions {
                animate,
                ..UserCardOptions::default()
            };
            vec![
                render_user_card(&sample_user(), &uopts, &LIGHT).unwrap(),
                render_repo_card(&sample_repo(), &full_repo_opts(animate), &LIGHT).unwrap(),
            ]
        };
        for svg in render_all(false) {
            assert!(!svg.contains("<animate"), "animate=0 must be static: {svg}");
        }
        for svg in render_all(true) {
            assert!(svg.contains("<animate"), "animate=1 must animate");
            assert!(
                !svg.contains("<g opacity=\"0\""),
                "SMIL-stripped cards must retain visible content"
            );
            for frag in svg.split("<animate").skip(1) {
                let end = frag
                    .find("/>")
                    .or_else(|| frag.find('>'))
                    .unwrap_or(frag.len());
                let tag = &frag[..end];
                assert!(
                    tag.contains("fill=\"freeze\"") || tag.contains("repeatCount"),
                    "animate tag must freeze: {tag}"
                );
            }
        }
    }

    #[test]
    fn profile_card_uses_native_footprint_layout_and_brief_reveal() {
        let opts = UserCardOptions {
            animate: true,
            ..UserCardOptions::default()
        };
        let svg = render_user_card(&sample_user(), &opts, &LIGHT).unwrap();
        assert!(svg.contains("MAINTAINER FOOTPRINT"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.contains("ANALYSIS COMPLETE"));
        assert!(!svg.contains("stroke-dashoffset"));
        assert!(svg.contains("dur=\"0.2s\""));
        assert!(svg.contains("begin=\"0.08s\""));
        assert!(svg.contains("prefers-reduced-motion: reduce"));
    }

    #[test]
    fn legacy_rank_icon_does_not_change_native_layout() {
        let opts = UserCardOptions {
            rank_icon_percentile: true,
            ..UserCardOptions::default()
        };
        let svg = render_user_card(&sample_user(), &opts, &LIGHT).unwrap();
        assert!(svg.contains("MAINTAINER FOOTPRINT"));
        assert!(svg.contains("ANALYSIS COMPLETE"));
        assert!(!svg.contains("class=\"g\""));
    }

    #[test]
    fn custom_title_is_escaped_and_truncated() {
        let opts = UserCardOptions {
            custom_title: Some("<script>alert(1)</script>".into()),
            ..UserCardOptions::default()
        };
        let svg = render_user_card(&sample_user(), &opts, &LIGHT).unwrap();
        assert!(!svg.contains("<script"));
        assert!(svg.contains("&lt;script&gt;"));
        let long = UserCardOptions {
            custom_title: Some("x".repeat(200)),
            ..UserCardOptions::default()
        };
        let svg = render_user_card(&sample_user(), &long, &LIGHT).unwrap();
        assert!(svg.contains(&format!("{}…", "x".repeat(63))));
        assert!(!svg.contains(&"x".repeat(65)));
        // Repo card path too.
        let ropts = RepoCardOptions {
            custom_title: Some("\"><script>".into()),
            ..RepoCardOptions::default()
        };
        let svg = render_repo_card(&sample_repo(), &ropts, &LIGHT).unwrap();
        assert!(!svg.contains("<script"));
    }

    #[test]
    fn sparkline_omitted_without_complete_history() {
        let mut data = sample_repo();
        data.spark = Vec::new();
        data.stars_30d = None;
        let svg = render_repo_card(&data, &full_repo_opts(false), &LIGHT).unwrap();
        assert!(!svg.contains("class=\"spark\""));
        assert!(!svg.contains("stars · 30d"));
        let with = render_repo_card(&sample_repo(), &full_repo_opts(false), &LIGHT).unwrap();
        assert!(with.contains("class=\"spark\""));
    }

    #[test]
    fn unavailable_cells_are_dropped_not_zeroed() {
        let data = RepoCardData {
            slug: "a/b".into(),
            stars: Some(10),
            ..RepoCardData::default()
        };
        let svg = render_repo_card(&data, &RepoCardOptions::default(), &LIGHT).unwrap();
        assert!(svg.contains(">stars<"));
        assert!(!svg.contains(">forks<"));
        assert!(!svg.contains(">contributors<"));
    }

    fn pt(day: i64, stars: u32) -> Point {
        Point {
            at: Utc.timestamp_opt(1_700_000_000 + day * 86_400, 0).unwrap(),
            stars,
        }
    }

    #[test]
    fn spark_window_filters_and_downsamples() {
        let series: Vec<Point> = (0..200).map(|i| pt(i, i as u32 + 1)).collect();
        let out = spark_window(&series, 90, 120);
        assert!(out.len() <= 120);
        // Window is relative to the LAST point, not the wall clock.
        let last: DateTime<Utc> = series.last().unwrap().at;
        let start = last - Duration::days(90);
        assert!(out.first().unwrap().t >= start.timestamp());
        assert_eq!(out.last().unwrap().v, 200);
    }

    #[test]
    fn spark_window_quiet_repo_gets_flat_anchor() {
        // All stars ancient except the implicit last point → prepend the
        // pre-window point so there are ≥2 points to draw.
        let series = vec![pt(0, 1), pt(1, 2), pt(500, 3)];
        let out = spark_window(&series, 90, 120);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].v, 2);
        assert_eq!(out[1].v, 3);
        // Empty in → empty out.
        assert!(spark_window(&[], 90, 120).is_empty());
    }

    #[test]
    fn lang_shares_normalize_and_drop_nonpositive() {
        let shares = lang_shares(&[("Rust".into(), 75), ("Go".into(), 25), ("Junk".into(), 0)]);
        assert_eq!(shares.len(), 2);
        let total: f64 = shares.iter().map(|(_, s)| s).sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!((shares[0].1 - 0.75).abs() < 1e-9);
        assert!(lang_shares(&[]).is_empty());
    }

    #[test]
    fn notice_cards_render_expected_messages() {
        let empty = render_user_empty_card("ghost", &LIGHT);
        assert!(empty.contains("@ghost"));
        assert!(empty.contains("no gitdebt data yet"));
        let missing = render_repo_missing_card("a/b", &DARK);
        assert!(missing.contains("repo not found"));
        let pending = render_repo_pending_card("a/b", Some(1_234), &LIGHT);
        assert!(pending.contains("1.2k stars"));
        assert!(pending.contains("analysis pending"));
        let pending_cold = render_repo_pending_card("a/b", None, &LIGHT);
        assert!(pending_cold.contains("analysis pending"));
        // All are static + escaped.
        for svg in [empty, missing, pending, pending_cold] {
            assert!(!svg.contains("<animate"));
            assert!(svg.starts_with("<svg"));
        }
    }

    #[test]
    fn footer_links_to_matching_pages() {
        let user = render_user_card(&sample_user(), &UserCardOptions::default(), &LIGHT).unwrap();
        assert!(user.contains("https://gitdebt.com/u/octocat"));
        assert!(user.contains("8 / 8 REPOS"));
        let repo = render_repo_card(&sample_repo(), &full_repo_opts(false), &LIGHT).unwrap();
        assert!(repo.contains("https://gitdebt.com/rust-lang/rust"));
        assert!(repo.contains("https://github.com/rust-lang/rust"));
    }

    #[test]
    fn cards_rasterize_including_animated_variants() {
        const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        for animate in [false, true] {
            let uopts = UserCardOptions {
                animate,
                metrics: select_user_metrics(None, Some("since,langs")),
                ..UserCardOptions::default()
            };
            let user = render_user_card(&sample_user(), &uopts, &DARK).unwrap();
            let png = crate::raster::rasterize(&user, crate::raster::RasterFormat::Png, 2.0)
                .unwrap_or_else(|e| panic!("user card raster (animate={animate}): {e}"));
            assert_eq!(&png[..8], &PNG_MAGIC);

            let repo = render_repo_card(&sample_repo(), &full_repo_opts(animate), &LIGHT).unwrap();
            let png = crate::raster::rasterize(&repo, crate::raster::RasterFormat::Png, 2.0)
                .unwrap_or_else(|e| panic!("repo card raster (animate={animate}): {e}"));
            assert_eq!(&png[..8], &PNG_MAGIC);
        }
        let empty = render_user_empty_card("ghost", &LIGHT);
        let png = crate::raster::rasterize(&empty, crate::raster::RasterFormat::Png, 2.0).unwrap();
        assert_eq!(&png[..8], &PNG_MAGIC);
    }
}
