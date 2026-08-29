//! Profile + repo stat cards (`/api/users/:login/card.svg`,
//! `/api/repos/:owner/:repo/card.svg`, plus `.png` / `.webp` variants).
//!
//! ## What a card is
//!
//! A small drawing sheet. Paper, a 1px frame, the bottom-right corner cut at
//! 10px, and the interior divided into fields by rules that terminate on the
//! frame or on each other. Field labels are uppercase and tracked; values are
//! tabular and right-aligned so a column of numbers lines up on its digits.
//! The headline figure is the measured value of the sheet and it is the one
//! thing that takes drafting red — together with the star trace on the repo
//! sheet, which is the same measurement drawn as a line.
//!
//! Unlike a badge, a card paints its paper. A sheet is a sheet: a light card
//! dropped into a dark README used to letter graphite onto whatever was
//! behind it and disappear. `?theme=` picks which print you embed, and the
//! `<picture>` pattern in `theme.rs` picks it per viewer.
//!
//! Every render function here is pure (`data + options + &Theme → SVG
//! String`) and bytes-deterministic. Theme colors are baked hex — no CSS
//! vars, no `prefers-color-scheme` (see `theme.rs` for the why). Animation
//! follows the `badge.rs` SMIL discipline: `animate=0` (default) emits no
//! `<animate>` tags at all; `animate=1` uses `<animate … fill="freeze">`
//! so a SMIL-stripped embed (and `raster::freeze_svg_animations`) shows the
//! correct final frame. Nothing is ever hidden behind a reveal: every group
//! is authored at its resting state and the animation only plays it in.
//!
//! ## Deliberately-unsupported github-readme-stats params
//!
//! Accepted-and-ignored so pasted GRS URLs keep working, but never
//! honored (product decisions, not gaps): `bg_color`, `title_color`,
//! `text_color`, `icon_color`, `border_color`, `ring_color`,
//! `border_radius` (two baked themes only — free hex would fragment the
//! CDN cache and defeat the palette; and the drawing has exactly one
//! non-square corner, the 10px chamfer), `show_icons` (a drawing sheet
//! letters its fields, it does not illustrate them), `locale`
//! (English-only v0), `cache_seconds` (fixed CDN policy), `line_height` /
//! `text_bold` / `number_precision` (fixed typographic scale), and every
//! per-user GitHub-API stat gitdebt does not observe: PRs, issues, reviews,
//! discussions, followers, streaks/contribution calendars,
//! `include_all_commits` / `commits_year`, and the `repo=`/`owner=`/
//! `role=` affiliation filters. Our stars/commits/contribs are **lower
//! bounds over tracked repos**; the mandatory "N repos tracked" footer
//! and the "REPOS TRACKED" field are the honesty framing that makes that
//! OK. Nothing here reads stargazer *profiles* — the user card consumes
//! only `repo_author_stats` commit-authorship aggregates and `repos`
//! ownership rows.

use crate::badge::humanize;
use crate::brand;
use crate::chart::Point;
use crate::texture::{self, Dimension, Side};
use crate::theme::{Theme, pens_for};

// Shared option plumbing

/// Maintainer cards need enough width for the headline field and the field
/// stack beside it without either column collapsing.
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

/// Resolve the display title: a non-empty `custom_title`, or the entity
/// default, cut to whatever the sheet actually has room for and never past
/// the 64 chars GRS conventionally allows.
///
/// The default is fitted too, not just the custom one. A repository slug can
/// be 140 characters, and lettering it unfitted ran it straight off the right
/// edge of the sheet.
fn display_title(custom: Option<&str>, default: &str, max_chars: usize) -> String {
    let raw = custom
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default);
    texture::escape_xml(&truncate_chars(raw, max_chars.min(64)))
}

/// How many characters of a face with `char_w` advance fit in `span`.
fn fit_chars(span: f32, char_w: f32) -> usize {
    ((span / char_w).floor() as i64).max(4) as usize
}

/// Characters of the profile sheet's title that fit beside its persona.
fn user_title_budget(w: f32, persona: &str) -> usize {
    let span =
        (w - 2.0 * PAD - persona.chars().count() as f32 * LABEL_CHAR_W - FIELD_GAP).max(48.0);
    fit_chars(span, TITLE_CHAR_W)
}

// Sheet geometry

/// Margin from the frame to any lettering. Nothing on the sheet arrives at
/// an edge with less than this.
const PAD: f32 = 24.0;
/// Inset of a field label from the rule that opens its column.
const COL_INSET: f32 = 16.0;

/// Rendered advance of one uppercase character in the field-label style
/// (8px sans plus 0.09em tracking), deliberately rounded UP: an
/// over-estimate truncates a hair early, an under-estimate collides with
/// the value.
const LABEL_CHAR_W: f32 = 6.0;
/// Mono advance at the 13px field-value size. Exact, not an estimate.
const VALUE_CHAR_W: f32 = 7.8;
/// Rendered advance of one character of the 16px sheet title, and of the
/// 10px caption the compact header letters a login in. Both rounded up.
const TITLE_CHAR_W: f32 = 8.0;
const CAPTION_CHAR_W: f32 = 5.8;
/// Ceiling and floor for the headline figure. It shrinks to fit its own
/// box before it is ever allowed to run into the rule beside it.
const HEADLINE_MAX: f32 = 28.0;
const HEADLINE_MIN: f32 = 14.0;
/// Clearance held between a field label and the value it names.
const FIELD_GAP: f32 = 12.0;

/// One shared `<style>` block. Five classes, one for each role a card has:
/// sheet title, field label, field value, running note, caption. Colors are
/// always inline baked hex; classes carry no color so nothing can leak a CSS
/// variable into a README.
const CARD_STYLE: &str = "  <style><![CDATA[ \
.t { font: 600 16px ui-sans-serif, system-ui, sans-serif; } \
.k { font: 500 8px ui-sans-serif, system-ui, sans-serif; letter-spacing: 0.09em; } \
.v { font: 600 13px ui-monospace, SFMono-Regular, monospace; font-variant-numeric: tabular-nums; } \
.n { font: 400 12px ui-sans-serif, system-ui, sans-serif; } \
.m { font: 500 10px ui-sans-serif, system-ui, sans-serif; } \
@media (prefers-reduced-motion: reduce) { .motion { display: none; } } \
]]></style>\n";

fn svg_open(w: f32, h: f32, label: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.0} {h:.0}\" role=\"img\" aria-label=\"{label}\">\n"
    )
}

/// The sheet: paper, a 1px frame, and the drawing's one chamfer on the
/// bottom-right corner. `hide_border` drops the frame ink only — the
/// geometry is identical either way, so the flag never reflows anything.
fn sheet(w: f32, h: f32, theme: &Theme, hide_border: bool) -> String {
    format!(
        "  <path d=\"{d}\" fill=\"{paper}\" stroke=\"{frame}\" stroke-width=\"{weight}\" stroke-opacity=\"{opacity}\" />\n",
        d = texture::chamfered_rect_path(0.5, 0.5, w - 1.0, h - 1.0),
        paper = theme.bg,
        frame = theme.border,
        weight = texture::W_OBJECT,
        opacity = if hide_border { "0" } else { "1" },
    )
}

/// A rule dividing the sheet across its full interior width. Both ends land
/// on the frame; the y it is given is the boundary, and the line is snapped
/// to sit crisply just above it.
fn rule_h(x1: f32, x2: f32, boundary: f32, ink: &str) -> String {
    format!(
        "  <line x1=\"{:.1}\" y1=\"{y:.1}\" x2=\"{:.1}\" y2=\"{y:.1}\" stroke=\"{ink}\" stroke-width=\"{weight}\" />\n",
        x1,
        x2,
        y = boundary.round() - 0.5,
        weight = texture::W_OBJECT,
    )
}

/// A rule dividing one band into two columns. It runs between two horizontal
/// rules, so both of its ends terminate on something real.
fn rule_v(boundary: f32, y1: f32, y2: f32, ink: &str) -> String {
    format!(
        "  <line x1=\"{x:.1}\" y1=\"{:.1}\" x2=\"{x:.1}\" y2=\"{:.1}\" stroke=\"{ink}\" stroke-width=\"{weight}\" />\n",
        y1,
        y2,
        x = boundary.round() - 0.5,
        weight = texture::W_OBJECT,
    )
}

/// An uppercase, tracked field label.
fn label_text(x: f32, baseline: f32, text: &str, ink: &str) -> String {
    format!(
        "  <text class=\"k\" x=\"{x:.1}\" y=\"{baseline:.1}\" fill=\"{ink}\">{}</text>\n",
        texture::escape_xml(text),
    )
}

/// One title-block field: the label on the left of its column, the value
/// tabular and right-aligned, both sitting on the same optical centre. The
/// value is fitted first, then the label is given whatever is left, so a
/// long label can never run under its own number.
fn field_row(
    left: f32,
    right: f32,
    baseline: f32,
    label: &str,
    value: &str,
    label_ink: &str,
    value_ink: &str,
) -> String {
    let span = (right - left).max(0.0);
    let value_budget = ((span - 24.0) / VALUE_CHAR_W).floor().max(3.0) as usize;
    let value = truncate_chars(value, value_budget);
    let value_w = value.chars().count() as f32 * VALUE_CHAR_W;
    let label_budget = (((span - value_w - FIELD_GAP) / LABEL_CHAR_W).floor() as i64).max(1);
    let label = truncate_chars(&label.to_uppercase(), label_budget as usize);
    format!(
        "{}  <text class=\"v\" x=\"{right:.1}\" y=\"{baseline:.1}\" text-anchor=\"end\" fill=\"{value_ink}\">{}</text>\n",
        // 13px value against an 8px label: matching baselines would sit the
        // label low, so it is raised onto the value's cap centre.
        label_text(left, baseline - 1.8, &label, label_ink),
        texture::escape_xml(&value),
    )
}

/// A value is measured when it reads as a figure. `warming` is a state, not
/// a measurement, and a state must never take drafting red.
fn is_measured(value: &str) -> bool {
    matches!(value.chars().next(), Some(c) if c.is_ascii_digit() || c == '+')
}

/// Fit the headline figure to its own box.
///
/// It shrinks before it collides: the size falls until the value fits the
/// box, and only if the floor still will not hold it is the value cut.
/// Returns the value as it will be lettered and the size it will take, both
/// of which the caller needs to centre the field.
fn headline_fit(value: &str, box_w: f32) -> (String, f32) {
    let budget = (box_w / (HEADLINE_MIN * 0.6)).floor().max(2.0) as usize;
    let value = truncate_chars(value, budget);
    let chars = value.chars().count().max(1) as f32;
    let size = (box_w / (chars * 0.6)).clamp(HEADLINE_MIN, HEADLINE_MAX);
    (value, size)
}

/// Cap height of lettering at `size`, and the reach of its descenders. Both
/// are what the field is centred on; guessing them lands the block low.
const CAP_RATIO: f32 = 0.72;
const DESCENT_RATIO: f32 = 0.22;
/// Air between a field label's baseline and the cap line of the figure it
/// names.
const HEADLINE_LEAD: f32 = 10.0;

/// The headline field — label over figure — centred inside the box
/// `(x, top)`/`(box_w, box_h)`. A short field floating at the head of a tall
/// box is the loudest kind of layout accident, so it is measured and placed,
/// never pinned to the top and hoped for.
fn headline_field(
    at: (f32, f32),
    size_of_box: (f32, f32),
    label: &str,
    value: &str,
    inks: (&str, &str),
) -> String {
    let ((x, top), (box_w, box_h), (label_ink, value_ink)) = (at, size_of_box, inks);
    let (value, size) = headline_fit(value, box_w);
    let cap = size * CAP_RATIO;
    let label_cap = 8.0 * CAP_RATIO;
    let block = label_cap + HEADLINE_LEAD + cap + size * DESCENT_RATIO;
    let label_baseline = top + ((box_h - block) / 2.0).max(0.0) + label_cap;
    let value_baseline = label_baseline + HEADLINE_LEAD + cap;
    let label_budget = (box_w / LABEL_CHAR_W).floor().max(1.0) as usize;
    format!(
        "{}  <text x=\"{x:.1}\" y=\"{value_baseline:.1}\" fill=\"{value_ink}\" font-family=\"{mono}\" font-size=\"{size:.1}\" font-weight=\"600\" font-variant-numeric=\"tabular-nums\">{}</text>\n",
        label_text(
            x,
            label_baseline,
            &truncate_chars(&label.to_uppercase(), label_budget),
            label_ink,
        ),
        texture::escape_xml(&value),
        mono = texture::MONO,
    )
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

// Rank (retained pure math; the ring it once drove is not part of the
// drawing, and nothing on a sheet renders it)

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

/// Ring geometry retained for the rank API surface. Nothing draws a ring.
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
    /// Accepted for URL stability and ignored: a drawing sheet letters its
    /// fields rather than illustrating them.
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

struct UserRow {
    label: &'static str,
    value: String,
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
            UserMetric::Stars => rows.push(UserRow {
                label: "Total Stars Earned",
                value: fmt(data.stars),
            }),
            UserMetric::Commits => rows.push(UserRow {
                label: "Commits Found",
                value: lower_bound(data.commits),
            }),
            UserMetric::Contribs => rows.push(UserRow {
                label: "Contributed To",
                value: lower_bound(data.contribs),
            }),
            UserMetric::Repos => rows.push(UserRow {
                label: "Repos Tracked",
                value: fmt(data.repos_tracked),
            }),
            UserMetric::Forks => rows.push(UserRow {
                label: "Total Forks",
                value: fmt(data.forks),
            }),
            UserMetric::Since => {
                if let Some(year) = data.since_year {
                    rows.push(UserRow {
                        label: "Contributing Since",
                        value: year.to_string(),
                    });
                }
            }
            UserMetric::Langs => {
                let names: Vec<&str> = data
                    .langs
                    .iter()
                    .filter(|(_, lines)| *lines > 0)
                    .map(|(name, _)| name.as_str())
                    .collect();
                if !names.is_empty() {
                    rows.push(UserRow {
                        label: "Top Languages",
                        value: names
                            .iter()
                            .take(3)
                            .copied()
                            .collect::<Vec<_>>()
                            .join(" · "),
                    });
                }
            }
        }
    }
    rows
}

/// Band heights of the profile sheet: title strip, the body that carries the
/// headline field beside the field stack, and the footer strip.
const U_HEADER_H: u32 = 46;
const U_HEADER_COMPACT_H: u32 = 34;
const U_HEADLINE_BOX_H: u32 = 80;
const U_ROW_H: u32 = 24;
const U_BODY_PAD: u32 = 20;
const U_FOOTER_H: u32 = 30;

/// Height of the profile sheet. The body is the taller of the headline field
/// and the stack of remaining fields beside it, so neither can be clipped by
/// the other. The legacy `hide_rank` parameter is layout-neutral now that the
/// sheet carries no coverage rail.
pub fn user_card_height(rows: usize, hide_title: bool, hide_rank: bool) -> u32 {
    let _ = hide_rank;
    let header = if hide_title {
        U_HEADER_COMPACT_H
    } else {
        U_HEADER_H
    };
    let stack = rows.saturating_sub(1) as u32 * U_ROW_H + U_BODY_PAD;
    let body = std::cmp::max(U_HEADLINE_BOX_H, stack);
    (header + body + U_FOOTER_H).max(140)
}

/// A deterministic, playful profile title derived only from the card's
/// visible activity totals. It is a classification field, not a
/// measurement, so it letters in the field-label style and never in red.
pub fn user_persona(data: &UserCardData) -> &'static str {
    if data.stars >= 100_000 {
        "OSS WIZARD"
    } else if data.commits >= 1_000 && data.stars < 500 {
        "PRODUCTIVE PROCRASTINATOR"
    } else if data.forks >= 10_000 {
        "ECOSYSTEM ARCHITECT"
    } else if data.repos_tracked >= 10 && data.commits >= 300 {
        "REPO GARDENER"
    } else if data.contribs >= 20 {
        "CODE NOMAD"
    } else {
        "OPEN SOURCE BUILDER"
    }
}

/// Render the user profile card. Pure + deterministic. `Err` when the
/// selection leaves nothing to draw.
pub fn render_user_card(
    data: &UserCardData,
    opts: &UserCardOptions,
    theme: &Theme,
) -> Result<String, &'static str> {
    let rows = user_rows(data, opts);
    if rows.is_empty() {
        return Err("at least one profile metric is required");
    }
    let w = opts.width as f32;
    let h = user_card_height(rows.len(), opts.hide_title, opts.hide_rank) as f32;
    let header = if opts.hide_title {
        U_HEADER_COMPACT_H
    } else {
        U_HEADER_H
    } as f32;
    let stack_h = rows.len().saturating_sub(1) as f32 * U_ROW_H as f32;
    let body_h = h - header - U_FOOTER_H as f32;
    let persona = user_persona(data);
    // The title and the classification share one baseline, so the title's
    // room is what the classification leaves it, never the whole width.
    let title_span =
        (w - 2.0 * PAD - persona.chars().count() as f32 * LABEL_CHAR_W - FIELD_GAP).max(48.0);
    let title = display_title(
        opts.custom_title.as_deref(),
        &format!("@{}", data.login),
        user_title_budget(w, persona),
    );
    let login = texture::escape_xml(&truncate_chars(
        &data.login,
        fit_chars(title_span - CAPTION_CHAR_W, CAPTION_CHAR_W),
    ));

    let mut body = sheet(w, h, theme, opts.hide_border);

    // Title strip: the entity on the left, its classification on the right,
    // both on one baseline, closed by a rule that lands on the frame.
    let title_baseline = header - 17.0;
    let strip_baseline = if opts.hide_title {
        header - 12.0
    } else {
        title_baseline
    };
    if opts.hide_title {
        body.push_str(&format!(
            "  <text class=\"m\" x=\"{PAD:.1}\" y=\"{strip_baseline:.1}\" fill=\"{ink}\">@{login}</text>\n",
            ink = theme.muted,
        ));
    } else {
        body.push_str(&format!(
            "  <text class=\"t\" x=\"{PAD:.1}\" y=\"{title_baseline:.1}\" fill=\"{ink}\">{title}</text>\n",
            ink = theme.fg,
        ));
    }
    body.push_str(&format!(
        "  <text class=\"k\" x=\"{x:.1}\" y=\"{strip_baseline:.1}\" text-anchor=\"end\" fill=\"{ink}\">{persona}</text>\n",
        x = w - PAD,
        ink = theme.ink_3,
    ));
    body.push_str(&rule_h(1.0, w - 1.0, header, theme.grid));

    // Body: the headline field, then the stack of the rest beside it. With
    // nothing to stack the sheet is not divided at all — a rule with an empty
    // box behind it encloses nothing, and the headline takes the whole width.
    let split = if rows.len() > 1 {
        (w * 0.40).clamp(150.0, 260.0)
    } else {
        w - COL_INSET
    };
    if rows.len() > 1 {
        body.push_str(&rule_v(split, header, header + body_h, theme.grid));
    }

    let (hero_open, hero_close) = anim_group(opts.animate, 0);
    body.push_str(&hero_open);
    body.push_str(&headline_field(
        (PAD, header),
        (split - PAD - COL_INSET, body_h),
        rows[0].label,
        &rows[0].value,
        (
            theme.ink_3,
            if is_measured(&rows[0].value) {
                theme.accent
            } else {
                theme.fg
            },
        ),
    ));
    body.push_str(hero_close);

    // The stack is centred in the body box: a short stack floating at the
    // top of a tall box reads as a layout accident.
    let stack_top = header + (body_h - stack_h) / 2.0;
    for (index, row) in rows.iter().skip(1).enumerate() {
        let (open, close) = anim_group(opts.animate, index + 1);
        body.push_str(&open);
        body.push_str(&field_row(
            split + COL_INSET,
            w - PAD,
            stack_top + index as f32 * U_ROW_H as f32 + 16.0,
            row.label,
            &row.value,
            theme.ink_3,
            theme.fg,
        ));
        body.push_str(close);
    }

    // Footer strip.
    let footer_top = header + body_h;
    body.push_str(&rule_h(1.0, w - 1.0, footer_top, theme.grid));
    body.push_str(&format!(
        "  <a href=\"https://gitdebt.com/{login}\" target=\"_blank\" rel=\"noopener\"><text class=\"m\" x=\"{PAD:.1}\" y=\"{y:.1}\" fill=\"{ink}\">gitdebt.com/{login} ↗</text></a>\n",
        y = h - 12.0,
        ink = theme.muted,
    ));
    body.push_str(&brand::footer_lockup(w - PAD, h - 12.0, theme));

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

/// Placeholder for a profile chart whose owned repos are tracked but not
/// yet analyzed. Distinct from [`render_user_empty_card`]: there IS data
/// coming, so the message must not read as "nothing here".
pub fn render_user_pending_card(login: &str, theme: &Theme) -> String {
    notice_card(
        &format!("@{login}"),
        "code analysis is still running — refresh shortly",
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

/// One point of the cumulative star trace: unix seconds + cumulative
/// total. Produced by [`spark_window`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkPoint {
    pub t: i64,
    pub v: u64,
}

/// Windowed + downsampled trace points from a complete cumulative
/// star series (ascending). Takes the trailing `window_days` relative to
/// the series' **last** timestamp (not the wall clock — determinism), and
/// downsamples to at most `max_points`. When fewer than 2 points fall in
/// the window, the last pre-window point is prepended so a quiet repo
/// still draws a flat line. Empty input → empty output (trace
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

/// Inputs for [`render_repo_card`]. `None` = unavailable → the field is
/// dropped (never rendered as a fake zero). `spark` empty = no complete
/// star history → the trace is omitted entirely.
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
    /// Accepted for URL stability and ignored; see [`UserCardOptions`].
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
    value: String,
    label: &'static str,
}

fn repo_cells(data: &RepoCardData, opts: &RepoCardOptions) -> Vec<RepoCell> {
    let fmt = |n: u64| opts.number_format.format(n);
    let mut cells = Vec::new();
    for m in &opts.metrics {
        let cell = match m {
            RepoMetric::Stars => data.stars.map(|v| RepoCell {
                value: fmt(v),
                label: "stars",
            }),
            RepoMetric::Forks => data.forks.map(|v| RepoCell {
                value: fmt(v),
                label: "forks",
            }),
            RepoMetric::Contributors => data.contributors.map(|v| RepoCell {
                value: fmt(v),
                label: "contributors",
            }),
            RepoMetric::Commits => data.commits.map(|v| RepoCell {
                value: fmt(v),
                label: "commits",
            }),
            RepoMetric::Age => data.created_year.map(|y| RepoCell {
                value: y.to_string(),
                label: "since",
            }),
            RepoMetric::Lines => data.lines_total.map(|v| RepoCell {
                value: fmt(v),
                label: "lines of code",
            }),
            RepoMetric::Commits30d => data.commits_30d.map(|v| RepoCell {
                value: fmt(v),
                label: "commits · 30d",
            }),
            RepoMetric::Stars30d => data.stars_30d.map(|v| RepoCell {
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

/// Band heights of the repository sheet.
const R_HEADER_H: u32 = 40;
const R_HEADLINE_BOX_H: u32 = 74;
const R_ROW_H: u32 = 20;
const R_BODY_PAD: u32 = 18;
const R_TRACE_H: u32 = 64;
const R_LANG_H: u32 = 38;
const R_FOOTER_H: u32 = 26;

/// Repo-sheet height: title strip + the body (headline field beside the
/// field stack) + the optional trace and language bands + footer strip.
/// `grid_rows` is the number of fields BESIDE the headline.
pub fn repo_card_height(grid_rows: usize, has_spark: bool, has_langs: bool) -> u32 {
    let stack = grid_rows as u32 * R_ROW_H + R_BODY_PAD;
    let body = std::cmp::max(R_HEADLINE_BOX_H, stack);
    let trace = if has_spark { R_TRACE_H } else { 0 };
    let langs = if has_langs { R_LANG_H } else { 0 };
    (R_HEADER_H + body + trace + langs + R_FOOTER_H).max(150)
}

/// Render the repo stats card. Pure + deterministic. `Err` only when the
/// `hide=` selection removed every metric key (→ 400 upstream);
/// data-unavailable fields are silently dropped instead.
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
    let grid_rows = cells.len().saturating_sub(1);
    let w = opts.width as f32;
    let h = repo_card_height(grid_rows, has_spark, has_langs) as f32;
    let slug = texture::escape_xml(&data.slug);
    let title = display_title(
        opts.custom_title.as_deref(),
        &data.slug,
        fit_chars(w - 2.0 * PAD, TITLE_CHAR_W),
    );

    let header = R_HEADER_H as f32;
    let footer_top = h - R_FOOTER_H as f32;
    let lang_top = footer_top - if has_langs { R_LANG_H as f32 } else { 0.0 };
    let trace_top = lang_top - if has_spark { R_TRACE_H as f32 } else { 0.0 };
    let body_h = trace_top - header;
    let stack_h = grid_rows as f32 * R_ROW_H as f32;

    let mut body = sheet(w, h, theme, opts.hide_border);

    // Title strip: the full slug, linked to GitHub, closed by a rule.
    body.push_str(&format!(
        "  <a href=\"https://github.com/{slug}\" target=\"_blank\" rel=\"noopener\"><text class=\"t\" x=\"{PAD:.1}\" y=\"{y:.1}\" fill=\"{ink}\">{title}</text></a>\n",
        y = header - 14.0,
        ink = theme.fg,
    ));
    body.push_str(&rule_h(1.0, w - 1.0, header, theme.grid));

    // Body: headline field, then the remaining fields stacked beside it.
    // With nothing to stack beside it the headline takes the whole width; a
    // rule with an empty box behind it encloses nothing.
    let split = if grid_rows > 0 {
        (w * 0.40).clamp(140.0, 260.0)
    } else {
        w - COL_INSET
    };
    if grid_rows > 0 {
        body.push_str(&rule_v(split, header, header + body_h, theme.grid));
    }

    if let Some(hero) = cells.first() {
        let (open, close) = anim_group(opts.animate, 0);
        body.push_str(&open);
        body.push_str(&headline_field(
            (PAD, header),
            (split - PAD - COL_INSET, body_h),
            hero.label,
            &hero.value,
            (
                theme.ink_3,
                if is_measured(&hero.value) {
                    theme.accent
                } else {
                    theme.fg
                },
            ),
        ));
        body.push_str(close);
    }

    let stack_top = header + (body_h - stack_h) / 2.0;
    for (index, cell) in cells.iter().skip(1).enumerate() {
        let (open, close) = anim_group(opts.animate, index + 1);
        body.push_str(&open);
        body.push_str(&field_row(
            split + COL_INSET,
            w - PAD,
            stack_top + index as f32 * R_ROW_H as f32 + 14.0,
            cell.label,
            &cell.value,
            theme.ink_3,
            theme.fg,
        ));
        body.push_str(close);
    }

    // Trace band: the star history is the live measurement of this sheet,
    // so it draws in drafting red, and the window it covers is dimensioned
    // from the trace's own timestamps rather than assumed.
    if has_spark {
        body.push_str(&rule_h(1.0, w - 1.0, trace_top, theme.grid));
        body.push_str(&label_text(
            PAD,
            trace_top + 15.0,
            "STAR HISTORY",
            theme.ink_3,
        ));
        let (tx, tw) = (PAD, w - 2.0 * PAD);
        let (ty, th) = (trace_top + 20.0, 22.0);
        body.push_str(&spark_svg(&data.spark, tx, ty, tw, th, theme.accent));
        let days = trace_span_days(&data.spark);
        body.push_str(&texture::extension_tick(
            tx,
            ty + th,
            Side::Down,
            texture::TICK_LEN,
            theme.ink_3,
        ));
        body.push_str(&texture::extension_tick(
            tx + tw,
            ty + th,
            Side::Down,
            texture::TICK_LEN,
            theme.ink_3,
        ));
        body.push_str(&texture::dimension_h(
            tx,
            tx + tw,
            ty + th + 12.0,
            &Dimension {
                value: &format!("{days}D"),
                ink: theme.ink_3,
                ground: theme.bg,
                size: 8.0,
            },
        ));
        body.push('\n');
    }

    // Language band: one bar of plotter pens, each segment closed by an ink
    // hairline at its measured edge, and each named in its own pen so hue is
    // never the only thing telling two of them apart.
    if has_langs {
        body.push_str(&rule_h(1.0, w - 1.0, lang_top, theme.grid));
        body.push_str(&lang_strip(
            &shares,
            PAD,
            lang_top + 10.0,
            w - 2.0 * PAD,
            theme,
        ));
    }

    body.push_str(&rule_h(1.0, w - 1.0, footer_top, theme.grid));
    body.push_str(&brand::footer_lockup(w - PAD, h - 10.0, theme));

    Ok(format!(
        "{open}{CARD_STYLE}{body}</svg>",
        open = svg_open(w, h, &format!("{slug} gitdebt stats")),
    ))
}

/// Whole days the trace actually spans, from its own timestamps. Never the
/// wall clock, and never a number the renderer assumed.
fn trace_span_days(points: &[SparkPoint]) -> i64 {
    match (points.first(), points.last()) {
        (Some(a), Some(b)) => ((b.t - a.t) / 86_400).max(1),
        _ => 1,
    }
}

/// Cumulative-star trace inside the (`x`, `y`, `w`, `h`) box: one line, no
/// fill, no wash. Requires ≥2 points (callers gate on that).
fn spark_svg(points: &[SparkPoint], x: f32, y: f32, w: f32, h: f32, ink: &str) -> String {
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
        line.push_str(&format!("{} {}", texture::coord(px), texture::coord(py)));
    }
    format!(
        "  <g class=\"spark\"><path d=\"{line}\" fill=\"none\" stroke=\"{ink}\" stroke-width=\"{weight}\" stroke-linejoin=\"round\" stroke-linecap=\"round\" /></g>\n",
        weight = texture::W_OBJECT,
    )
}

/// The language bar and its legend.
///
/// Segments are plotter pens assigned by language name, so the same set of
/// languages is the same set of pens on every render and no series can claim
/// drafting red. Each segment carries the ink hairline `series_bar` puts on
/// its measured edge, which is what tells two adjacent pens apart at 8px.
fn lang_strip(shares: &[(String, f64)], x: f32, y: f32, w: f32, theme: &Theme) -> String {
    let names: Vec<&str> = shares.iter().map(|(n, _)| n.as_str()).collect();
    let pens = pens_for(theme, &names);
    let mut out = String::from("  <g class=\"langs\">");
    let mut sx = x;
    for ((_, share), pen) in shares.iter().zip(&pens) {
        let seg_w = (*share as f32) * w;
        out.push_str(&texture::series_bar(
            sx,
            y,
            seg_w,
            8.0,
            pen,
            theme.fg,
            Side::Right,
        ));
        sx += seg_w;
    }
    let legend_y = y + 20.0;
    let mut lx = x;
    for ((name, share), pen) in shares.iter().zip(&pens).take(3) {
        let text = format!("{name} {:.1}%", share * 100.0);
        // Approximate advance at the 10px caption size. A name that would
        // carry the row past the sheet's own margin is dropped rather than
        // lettered into the edge.
        let advance = text.chars().count() as f32 * 5.4;
        if lx + advance > x + w {
            break;
        }
        out.push_str(&format!(
            "<text class=\"m\" x=\"{lx:.1}\" y=\"{legend_y:.1}\" fill=\"{pen}\">{}</text>",
            texture::escape_xml(&text),
        ));
        lx += advance + 14.0;
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

/// Shared minimal notice sheet (400×108): the same three bands every card
/// has — title strip, body, footer strip — with one line of running text in
/// place of the fields. Nothing here was measured, so nothing here is red.
/// The title is cut to what the 16px face fits between the two margins.
fn notice_card(title: &str, message: &str, theme: &Theme) -> String {
    let (w, h) = (400.0_f32, 108.0_f32);
    format!(
        "{open}{CARD_STYLE}{sheet}{head}  <text class=\"t\" x=\"{PAD:.1}\" y=\"26\" fill=\"{ink}\">{title}</text>\n  <text class=\"n\" x=\"{PAD:.1}\" y=\"62\" fill=\"{muted}\">{message}</text>\n{foot}{footer}</svg>",
        open = svg_open(w, h, "gitdebt"),
        sheet = sheet(w, h, theme, false),
        head = rule_h(1.0, w - 1.0, 40.0, theme.grid),
        foot = rule_h(1.0, w - 1.0, 78.0, theme.grid),
        ink = theme.fg,
        muted = theme.muted,
        title = texture::escape_xml(&truncate_chars(
            title,
            fit_chars(w - 2.0 * PAD, TITLE_CHAR_W)
        )),
        message = texture::escape_xml(message),
        footer = brand::footer_lockup(w - PAD, h - 12.0, theme),
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
        assert!(svg.contains("COMMITS FOUND"));
        assert!(svg.contains(">warming</text>"));
        assert!(!svg.contains("ANALYSIS COVERAGE"));
        assert!(!svg.contains(">0</text>"));
    }

    /// A state is not a measurement, so a headline that reads `warming`
    /// letters in ink while a real figure takes drafting red.
    #[test]
    fn only_a_measured_headline_takes_drafting_red() {
        assert!(is_measured("12.3k") && is_measured("+1.2k") && is_measured("2015"));
        assert!(!is_measured("warming") && !is_measured("") && !is_measured("Rust · Go"));

        let measured = render_user_card(&sample_user(), &UserCardOptions::default(), &LIGHT)
            .expect("card renders");
        assert!(measured.contains(&format!("fill=\"{}\"", LIGHT.accent)));

        // Commits are a lower bound over tracked repos, so a profile still
        // warming has no figure to letter at all.
        let mut pending = sample_user();
        pending.commits = 0;
        pending.repos_analyzed = 0;
        let opts = UserCardOptions {
            metrics: select_user_metrics(Some("stars,contribs,repos,forks"), None),
            ..UserCardOptions::default()
        };
        let stateful = render_user_card(&pending, &opts, &LIGHT).expect("card renders");
        assert!(stateful.contains(">warming</text>"));
        assert!(
            !stateful.contains(&format!("fill=\"{}\"", LIGHT.accent)),
            "a state must never spend the signal"
        );
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
        assert!(light.contains("#111417"));
        assert!(dark.contains("#e6e8ea"));
        assert!(!light.contains("var(--"));
        assert!(!dark.contains("var(--"));
        let rlight = render_repo_card(&sample_repo(), &full_repo_opts(false), &LIGHT).unwrap();
        let rdark = render_repo_card(&sample_repo(), &full_repo_opts(false), &DARK).unwrap();
        assert!(rlight.contains("#111417"));
        assert!(rdark.contains("#e6e8ea"));
        assert!(!rlight.contains("var(--"));
        assert!(!rdark.contains("var(--"));
    }

    #[test]
    fn hide_removes_row_from_svg() {
        let all = render_user_card(&sample_user(), &UserCardOptions::default(), &LIGHT).unwrap();
        assert!(all.contains("TOTAL FORKS"));
        let opts = UserCardOptions {
            metrics: select_user_metrics(Some("forks"), None),
            ..UserCardOptions::default()
        };
        let hidden = render_user_card(&sample_user(), &opts, &LIGHT).unwrap();
        assert!(!hidden.contains("TOTAL FORKS"));
        assert!(hidden.contains("TOTAL STARS EARNED"));
    }

    #[test]
    fn hiding_everything_errors_regardless_of_legacy_rank_flag() {
        let opts = UserCardOptions {
            metrics: select_user_metrics(Some("stars,commits,contribs,repos,forks"), None),
            hide_rank: true,
            ..UserCardOptions::default()
        };
        assert!(render_user_card(&sample_user(), &opts, &LIGHT).is_err());
        // The old rank flag no longer supplies a replacement coverage rail.
        let with_rank = UserCardOptions {
            metrics: select_user_metrics(Some("stars,commits,contribs,repos,forks"), None),
            hide_rank: false,
            ..UserCardOptions::default()
        };
        assert!(render_user_card(&sample_user(), &with_rank, &LIGHT).is_err());
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
        // Header + the headline box + the footer strip, with no fields beside
        // the headline at all.
        assert_eq!(user_card_height(0, false, false), 156);
    }

    /// Every band the sheet reserves is a band the renderer actually fills,
    /// and the sum is exactly the sheet's own height.
    #[test]
    fn the_sheet_bands_add_up_to_its_height() {
        for (spark, langs) in [(true, true), (true, false), (false, true), (false, false)] {
            let mut data = sample_repo();
            if !spark {
                data.spark = Vec::new();
                data.stars_30d = None;
            }
            if !langs {
                data.langs = Vec::new();
            }
            let opts = full_repo_opts(false);
            let svg = render_repo_card(&data, &opts, &LIGHT).unwrap();
            let cells = repo_cells(&data, &opts).len();
            let h = repo_card_height(cells - 1, spark, langs);
            assert!(
                svg.contains(&format!("height=\"{h}\"")),
                "spark={spark} langs={langs} sheet height drifted from its bands"
            );
        }
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
        assert!(svg.contains("OPEN SOURCE BUILDER"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(!svg.contains("ANALYSIS COVERAGE"));
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
        assert!(svg.contains("OPEN SOURCE BUILDER"));
        assert!(!svg.contains("ANALYSIS"));
        assert!(!svg.contains("class=\"g\""));
    }

    /// `show_icons` is accepted for URL stability and changes nothing: a
    /// drawing sheet letters its fields rather than illustrating them.
    #[test]
    fn show_icons_is_accepted_and_ignored() {
        let with = UserCardOptions {
            show_icons: true,
            ..UserCardOptions::default()
        };
        let without = UserCardOptions {
            show_icons: false,
            ..UserCardOptions::default()
        };
        assert_eq!(
            render_user_card(&sample_user(), &with, &LIGHT).unwrap(),
            render_user_card(&sample_user(), &without, &LIGHT).unwrap()
        );
        let ropts = |show_icons| RepoCardOptions {
            show_icons,
            ..full_repo_opts(false)
        };
        assert_eq!(
            render_repo_card(&sample_repo(), &ropts(true), &LIGHT).unwrap(),
            render_repo_card(&sample_repo(), &ropts(false), &LIGHT).unwrap()
        );
    }

    #[test]
    fn profile_personas_follow_visible_activity() {
        let mut data = sample_user();
        data.stars = 100_000;
        assert_eq!(user_persona(&data), "OSS WIZARD");
        data.stars = 10;
        data.commits = 2_000;
        assert_eq!(user_persona(&data), "PRODUCTIVE PROCRASTINATOR");
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
        // Cut to what the strip leaves it beside the persona, with a real
        // ellipsis — never the flat 64 the GRS convention allows, which would
        // have run under the classification on the same baseline.
        let budget =
            user_title_budget(USER_CARD_DEFAULT_WIDTH as f32, user_persona(&sample_user()));
        assert!(budget < 64);
        assert!(svg.contains(&format!("{}…", "x".repeat(budget - 1))));
        assert!(!svg.contains(&"x".repeat(budget + 1)));
        // Repo card path too.
        let ropts = RepoCardOptions {
            custom_title: Some("\"><script>".into()),
            ..RepoCardOptions::default()
        };
        let svg = render_repo_card(&sample_repo(), &ropts, &LIGHT).unwrap();
        assert!(!svg.contains("<script"));
    }

    /// The regression this guards: a repository slug can be 140 characters
    /// and the sheet lettered it in full, straight off its own right edge.
    /// Every title is now cut to what fits between the two margins.
    #[test]
    fn a_long_title_is_cut_to_the_sheet_it_is_lettered_on() {
        let long_slug = "extremely-long-owner-name/an-extremely-long-repository-name";
        for width in [320u32, 400, 800] {
            let data = RepoCardData {
                slug: long_slug.into(),
                ..sample_repo()
            };
            let opts = RepoCardOptions {
                width,
                ..RepoCardOptions::default()
            };
            let svg = render_repo_card(&data, &opts, &LIGHT).unwrap();
            let budget = fit_chars(width as f32 - 2.0 * PAD, TITLE_CHAR_W).min(64);
            let lettered = svg
                .split("class=\"t\"")
                .nth(1)
                .and_then(|f| f.split_once('>'))
                .and_then(|(_, rest)| rest.split_once('<'))
                .map(|(text, _)| text.chars().count())
                .expect("the sheet letters a title");
            assert!(
                lettered <= budget,
                "{width}-wide sheet lettered {lettered} characters into a {budget}-character strip"
            );
            assert!(
                lettered as f32 * TITLE_CHAR_W <= width as f32 - 2.0 * PAD,
                "the title runs past the margin on a {width}-wide sheet"
            );
        }
        // The notice sheet is fitted the same way.
        let notice = render_repo_missing_card(long_slug, &LIGHT);
        assert!(notice.contains('…'));
        assert!(!notice.contains(long_slug));
    }

    #[test]
    fn trace_omitted_without_complete_history() {
        let mut data = sample_repo();
        data.spark = Vec::new();
        data.stars_30d = None;
        let svg = render_repo_card(&data, &full_repo_opts(false), &LIGHT).unwrap();
        assert!(!svg.contains("class=\"spark\""));
        assert!(!svg.contains("STARS · 30D"));
        assert!(!svg.contains("STAR HISTORY"));
        let with = render_repo_card(&sample_repo(), &full_repo_opts(false), &LIGHT).unwrap();
        assert!(with.contains("class=\"spark\""));
        assert!(with.contains("STAR HISTORY"));
    }

    /// The window under the trace is dimensioned from the trace's own
    /// timestamps. Assuming 90 would letter a wrong number the moment a
    /// caller passed a different window.
    #[test]
    fn the_trace_window_is_measured_not_assumed() {
        assert_eq!(trace_span_days(&spark_pts(30)), 29);
        assert_eq!(trace_span_days(&spark_pts(1)), 1);
        assert_eq!(trace_span_days(&[]), 1);
        let svg = render_repo_card(&sample_repo(), &full_repo_opts(false), &LIGHT).unwrap();
        assert!(svg.contains(">29D</text>"), "{svg}");
        // A dimension is a rule between two terminators, cut for its value.
        assert!(svg.contains("paint-order=\"stroke\""));
    }

    #[test]
    fn unavailable_cells_are_dropped_not_zeroed() {
        let data = RepoCardData {
            slug: "a/b".into(),
            stars: Some(10),
            ..RepoCardData::default()
        };
        let svg = render_repo_card(&data, &RepoCardOptions::default(), &LIGHT).unwrap();
        assert!(svg.contains(">STARS<"));
        assert!(!svg.contains(">FORKS<"));
        assert!(!svg.contains(">CONTRIBUTORS<"));
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

    /// Languages are plotter pens, assigned by name so the set is stable,
    /// and no category can ever claim the reserved signal pen.
    #[test]
    fn language_segments_are_plotter_pens_and_never_drafting_red() {
        for theme in [&LIGHT, &DARK] {
            let mut data = sample_repo();
            data.langs = vec![
                ("Rust".into(), 500),
                ("TypeScript".into(), 300),
                ("Python".into(), 200),
            ];
            let svg = render_repo_card(&data, &full_repo_opts(false), theme).unwrap();
            let shares = lang_shares(&data.langs);
            let names: Vec<&str> = shares.iter().map(|(n, _)| n.as_str()).collect();
            let pens = pens_for(theme, &names);
            for pen in &pens {
                assert_ne!(*pen, theme.accent, "a language claimed drafting red");
                assert!(svg.contains(pen), "{pen} missing from the bar");
            }
            // Each language is named in its own pen, so hue is never the only
            // thing separating two segments.
            assert!(svg.contains(&format!("fill=\"{}\">Rust 50.0%<", pens[0])));
        }
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
        // All are static, escaped, and spend no signal on an unmeasured state.
        for svg in [empty, missing, pending, pending_cold] {
            assert!(!svg.contains("<animate"));
            assert!(svg.starts_with("<svg"));
            assert!(!svg.contains(LIGHT.accent) && !svg.contains(DARK.accent));
        }
    }

    /// Every card is a sheet: paper, one 1px frame, one chamfer, and no
    /// texture, gradient, glow or rounded corner anywhere.
    #[test]
    fn every_card_is_a_drawing_sheet() {
        let user = render_user_card(&sample_user(), &UserCardOptions::default(), &DARK).unwrap();
        let repo = render_repo_card(&sample_repo(), &full_repo_opts(true), &DARK).unwrap();
        let notice = render_repo_pending_card("a/b", None, &DARK);
        for svg in [&user, &repo, &notice] {
            for banned in [
                "rx=",
                "ry=",
                "<pattern",
                "Gradient",
                "url(#",
                "gd-t",
                "gd-pixel",
                "gd-dither",
                "filter=",
                "fill-opacity",
                "var(--",
            ] {
                assert!(!svg.contains(banned), "{banned} survived: {svg}");
            }
            // The one chamfer, on the sheet's bottom-right corner.
            assert_eq!(svg.matches("  <path d=\"M0.50 0.50H").count(), 1);
            assert!(svg.contains(&format!("fill=\"{}\"", DARK.bg)), "paper");
            assert!(
                svg.contains(&format!("stroke=\"{}\"", DARK.border)),
                "frame"
            );
        }
    }

    /// Drafting red is rationed. On a whole sheet it lands on the headline
    /// figure and, on the repo sheet, on the star trace — the same
    /// measurement drawn as a line — and nowhere else.
    #[test]
    fn the_signal_is_spent_only_on_the_measurement() {
        let user = render_user_card(&sample_user(), &UserCardOptions::default(), &LIGHT).unwrap();
        assert_eq!(user.matches(LIGHT.accent).count(), 1);
        let repo = render_repo_card(&sample_repo(), &full_repo_opts(false), &LIGHT).unwrap();
        assert_eq!(repo.matches(LIGHT.accent).count(), 2);
        // The trace is the line, not a filled wash under it.
        assert!(repo.contains("fill=\"none\""));
    }

    #[test]
    fn user_card_raster_text_stays_at_card_scale() {
        // Regression: resvg parses only multiple-of-100 weights inside the
        // `font:` shorthand; a `font: 650 22px …` class made it read `650`
        // as the SIZE and blast 650px glyphs across the card. Guard by
        // bounding the lit-pixel share of the dark raster — card-scale
        // text lights a few percent, runaway glyphs light most of it.
        let svg = render_user_card(&sample_user(), &UserCardOptions::default(), &DARK).unwrap();
        let (rgba, w, h) = crate::raster::rasterize_rgba(&svg, 1.0).expect("raster");
        // The sheet paints its own paper, so the dark print's ground is
        // #0c0f11 and only lettering clears this threshold.
        let lit = rgba
            .chunks_exact(4)
            .filter(|px| {
                let a = px[3] as u32;
                (px[0] as u32 * a + 0x0c * (255 - a)) / 255 > 160
            })
            .count();
        let share = lit as f64 / (w * h) as f64;
        assert!(
            share < 0.10,
            "bright-pixel share {share:.3} — text is rendering far larger than authored"
        );
        assert!(share > 0.001, "text must actually render");
    }

    #[test]
    fn footer_links_to_matching_pages() {
        let user = render_user_card(&sample_user(), &UserCardOptions::default(), &LIGHT).unwrap();
        assert!(user.contains("https://gitdebt.com/octocat"));
        assert!(user.contains("data-gitdebt-logo=\"true\""));
        let repo = render_repo_card(&sample_repo(), &full_repo_opts(false), &LIGHT).unwrap();
        assert!(repo.contains("https://gitdebt.com"));
        assert!(repo.contains("https://github.com/rust-lang/rust"));
    }

    /// The profile card's footer lockup once carried a hand-drawn stand-in.
    /// Rasterize the real card and hold its mark against the asset.
    #[test]
    fn card_footer_lockups_carry_the_canonical_logo() {
        for theme in [&LIGHT, &DARK] {
            let user =
                render_user_card(&sample_user(), &UserCardOptions::default(), theme).unwrap();
            let repo = render_repo_card(&sample_repo(), &full_repo_opts(false), theme).unwrap();
            for svg in [&user, &repo] {
                assert!(svg.contains("M320.5 110.5"), "footer mark is the artwork");
            }

            for svg in [&user, &repo] {
                let place = crate::brand::MarkBox::locate(svg, 2.0, theme.muted, theme.bg);
                let (mismatch, ink) = crate::brand::mark_fidelity(svg, place);
                assert!(mismatch < 0.05, "card lockup drifted: {mismatch:.3}");
                assert!(
                    (0.25..0.75).contains(&ink),
                    "card lockup coverage {ink:.3} reads as a block"
                );
            }
        }
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

    /// Clear the cut. The chamfer removes the sheet's bottom-right corner,
    /// so the margin every right-anchored string is set against has to be
    /// wider than the cut, at every width the sheet can be asked for.
    #[test]
    fn nothing_is_lettered_into_the_chamfer() {
        const { assert!(PAD >= texture::CHAMFER) };
        for width in [320u32, 400, 800] {
            let ropts = RepoCardOptions {
                width,
                ..full_repo_opts(false)
            };
            let uopts = UserCardOptions {
                width: width.max(420),
                metrics: select_user_metrics(None, Some("since,langs")),
                ..UserCardOptions::default()
            };
            for (svg, w) in [
                (
                    render_repo_card(&sample_repo(), &ropts, &LIGHT).unwrap(),
                    width as f32,
                ),
                (
                    render_user_card(&sample_user(), &uopts, &LIGHT).unwrap(),
                    width.max(420) as f32,
                ),
            ] {
                for frag in svg.split("<text").skip(1) {
                    let head = &frag[..frag.find('>').unwrap_or(frag.len())];
                    if !head.contains("text-anchor=\"end\"") {
                        continue;
                    }
                    let x: f32 = head
                        .split(" x=\"")
                        .nth(1)
                        .and_then(|rest| rest.split('"').next())
                        .and_then(|v| v.parse().ok())
                        .expect("a right-anchored string carries an x");
                    assert!(
                        x <= w - texture::CHAMFER,
                        "a string ends at {x} on a {w}-wide sheet, inside the cut"
                    );
                }
            }
        }
    }
}
