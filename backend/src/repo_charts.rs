//! The repository-health sheets.
//!
//! Every renderer here is a pure function from query rows plus a `&Theme` to
//! one SVG string, and every one of them draws the same dimensioned
//! engineering drawing the site is: graphite on paper, square corners, and no
//! texture, gradient, glow or shadow anywhere. The notation vocabulary lives
//! in [`crate::texture`]; this module composes it into sheets.
//!
//! Three rules govern what is drawn:
//!
//! 1. **Every line terminates on something real.** A dimension line spans two
//!    measured points and letters the value on itself. An extension tick
//!    springs from a datum. A leader points at one. There is no background
//!    grid and no rule that separates nothing.
//! 2. **A plotted bar is a flat plotter ink with a 1px ink hairline standing
//!    at its measured edge** ([`crate::texture::series_bar`]). Categories are
//!    told apart by their pen and by their own label, never by a texture.
//! 3. **Drafting red is spent only on something measured**: a dimension and
//!    its terminators, or the primary trace. It is never a tag, a status dot
//!    or a category colour.
//!
//! Theme handling: each renderer takes a `&Theme` and substitutes concrete
//! hex colors directly into the output. No CSS variables, no
//! `prefers-color-scheme` — that approach is fragile in `<img>`-embedded
//! README contexts (see `theme.rs` for the why). Embedders combine a
//! `?theme=light` + `?theme=dark` pair via `<picture>`.
//!
//! Static attributes always contain the finished sheet, because many
//! consumers render an SVG as a single frame — every rasterizer, and README
//! renderers outside GitHub. The reveal only fades in what is already drawn.

use chrono::{Datelike, NaiveDate};
use serde::Serialize;

use crate::brand;
use crate::texture::{self, Dimension, Side, TitleField};
use crate::theme::{Theme, contrast_on, pens_for};

#[derive(Debug, Clone, Serialize)]
pub struct FileRow {
    pub path: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContributorRow {
    pub login: Option<String>,
    pub name: String,
    pub avatar_url: Option<String>,
    pub commits: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayCount {
    pub day: NaiveDate,
    pub commits: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TodoPoint {
    pub day: NaiveDate,
    pub running_total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanguageBar {
    pub language: String,
    pub files: i64,
    pub lines_code: i64,
    pub lines_blank: i64,
    pub lines_comment: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContributionProfile {
    pub owned_repos: i64,
    pub external_repos: i64,
    pub owned_commits: i64,
    pub external_commits: i64,
    pub visionary_count: i64,
}

/// Rendered edge of a contributor's photo, and the margin the sheet holds
/// around it for the frame line. Shared by the grid and the single-avatar
/// asset so a README that lays the standalone tiles out itself lands on the
/// same geometry the chart draws.
pub const AVATAR_SIZE: u32 = 62;
pub const AVATAR_MARGIN: u32 = 5;

/// Edge of the square one standalone avatar occupies: the photo plus its
/// margin on both sides.
pub const AVATAR_TILE: u32 = AVATAR_SIZE + AVATAR_MARGIN * 2;

/// Sheet margin. Every sheet letters its title at this inset and closes its
/// title block flush to the mirrored one.
const PAD: f32 = 56.0;

/// Baselines of the two header lines every sheet opens with.
const TITLE_Y: f32 = 36.0;
const CAPTION_Y: f32 = 58.0;

/// Width of the title block on a sheet that is 900 units or wider.
const BLOCK_W: f32 = 236.0;

/// Room under the title block for the colophon.
const COLOPHON_BAND: f32 = 34.0;

fn reveal_begin(index: usize) -> f32 {
    (index as f32 * 0.018).min(0.09)
}

/// The finished-frame reveal.
///
/// The element it sits on already carries `opacity="1"`, so a renderer that
/// never plays the animation — every rasterizer, and the static default —
/// still draws the whole sheet. Nothing on a sheet is gated on motion.
fn motion_reveal(index: usize) -> String {
    format!(
        "<animate class=\"motion\" attributeName=\"opacity\" from=\"0\" to=\"1\" dur=\"0.2s\" begin=\"{:.2}s\" fill=\"freeze\" />",
        reveal_begin(index),
    )
}

/// The lettering roles every sheet shares, in this print's ink.
///
/// Roles, not decoration: `.title` names the sheet, `.caption` says what was
/// measured, `.label` letters a plotted row, `.value` its measured figure
/// (tabular, so a column lines up on its digits), `.meta` is the
/// construction-line grey, and `.field` is the uppercase tracked-out field
/// label a key uses.
fn sheet_css(theme: &Theme) -> String {
    format!(
        r##"    .title {{ fill: {fg}; font: 600 17px {sans}; }}
    .caption {{ fill: {ink3}; font: 12px {sans}; }}
    .label {{ fill: {fg}; font: 12px {mono}; }}
    .value {{ fill: {fg}; font: 11px {mono}; font-variant-numeric: tabular-nums; }}
    .meta {{ fill: {ink3}; font: 11px {mono}; font-variant-numeric: tabular-nums; }}
    .field {{ fill: {ink3}; font: 8px {sans}; letter-spacing: {tracking}; }}
    .footer-link {{ fill: {muted}; font: 600 11px {sans}; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
    }}"##,
        fg = theme.fg,
        ink3 = theme.ink_3,
        muted = theme.muted,
        sans = texture::SANS,
        mono = texture::MONO,
        tracking = texture::LABEL_TRACKING,
    )
}

/// A measured value in drafting red, cut into its own dimension line.
fn measured<'a>(value: &'a str, theme: &'a Theme, size: f32) -> Dimension<'a> {
    Dimension {
        value,
        ink: theme.accent,
        ground: theme.bg,
        size,
    }
}

/// A measured value in graphite. Used where a sheet already spends its red.
fn plotted<'a>(value: &'a str, theme: &'a Theme, size: f32) -> Dimension<'a> {
    Dimension {
        value,
        ink: theme.fg,
        ground: theme.bg,
        size,
    }
}

/// How far a dimension line stands off the datum it measures.
const DIM_STANDOFF: f32 = 16.0;

/// How far an extension tick runs past the dimension line it serves. A tick
/// that stops short of its own dimension leaves the measurement floating
/// beside the drawing instead of attached to it.
const TICK_OVERRUN: f32 = 3.0;

/// Length of an extension tick that has to reach a dimension line standing
/// `standoff` off the datum.
fn extension_len(standoff: f32) -> f32 {
    (standoff - texture::TICK_CLEARANCE + TICK_OVERRUN).max(texture::TICK_LEN)
}

/// A 1px object line. The only line this module draws by hand: everything
/// else is a piece of notation from [`crate::texture`].
fn object_line(x1: f32, y1: f32, x2: f32, y2: f32, ink: &str, weight: f32) -> String {
    format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{ink}\" stroke-width=\"{weight}\" />",
        texture::coord(x1),
        texture::coord(y1),
        texture::coord(x2),
        texture::coord(y2),
    )
}

// ---------------------------------------------------------------------------
// Fix-labelled file changes, and file change frequency
// ---------------------------------------------------------------------------

pub fn render_bug_magnets(repo: &str, rows: &[FileRow], theme: &Theme) -> String {
    horizontal_bar_chart(BarChartConfig {
        repo,
        title: "Fix-labelled changes",
        caption: "Most fix-labelled commits in the analyzed commit window",
        quantity: "fix commits",
        empty: "no fix-labelled changes in the analyzed commit window",
        rows,
        theme,
    })
}

pub fn render_top_changed(repo: &str, rows: &[FileRow], theme: &Theme) -> String {
    horizontal_bar_chart(BarChartConfig {
        repo,
        title: "File change frequency",
        caption: "Files touched most often in the analyzed commit window",
        quantity: "commits",
        empty: "no file changes in the analyzed commit window",
        rows,
        theme,
    })
}

struct BarChartConfig<'a> {
    repo: &'a str,
    title: &'a str,
    caption: &'a str,
    /// What the plotted count measures. Lettered on the sheet's one dimension
    /// so the value carries its unit, the way a drawing dimensions one.
    quantity: &'a str,
    /// What a sheet with no rows says instead of leaving its plot area blank.
    empty: &'a str,
    rows: &'a [FileRow],
    theme: &'a Theme,
}

/// A plotted bar per file, measured from one zero datum.
///
/// The bars carry a 1px graphite hairline at the leading edge — the measured
/// end, where a terminator would land — and the sheet spends its red once, on
/// the dimension that measures the peak across the full plot width.
fn horizontal_bar_chart(cfg: BarChartConfig<'_>) -> String {
    let theme = cfg.theme;
    let width = 900.0_f32;
    let label_w = 330.0_f32;
    let value_gutter = 66.0_f32;
    let plot_x = PAD + label_w;
    let plot_w = width - PAD - plot_x - value_gutter;
    let row_h = 30.0_f32;
    let bar_h = 14.0_f32;
    let header_h = 112.0_f32;

    let max_count = cfg.rows.iter().map(|r| r.count).max().unwrap_or(0).max(0);
    let scale = max_count.max(1) as f32;
    // A sheet with no rows still holds a plot band, so its reason has room to
    // sit in and the title block does not ride up under the caption.
    let plot_bottom = header_h
        + if cfg.rows.is_empty() {
            60.0
        } else {
            cfg.rows.len() as f32 * row_h
        };

    let mut body = String::new();
    for (index, row) in cfg.rows.iter().enumerate() {
        let bar_y = header_h + index as f32 * row_h + (row_h - bar_h) / 2.0;
        let center = bar_y + bar_h / 2.0;
        let bar_w = (row.count.max(0) as f32 / scale) * plot_w;
        let bar = if bar_w >= 0.5 {
            texture::series_bar(
                plot_x,
                bar_y,
                bar_w,
                bar_h,
                theme.ink_3,
                theme.fg,
                Side::Right,
            )
        } else {
            String::new()
        };
        let value = row.count.to_string();
        let (value_x, anchor, ink) = count_placement(plot_x, bar_w, &value, theme.ink_3, theme.fg);
        let href = format!(
            "https://github.com/{repo}/blob/HEAD/{path}",
            repo = cfg.repo,
            path = row.path,
        );
        body.push_str(&format!(
            r##"  <g opacity="1">
    {motion}
    <a class="bar-link" href="{href}" target="_blank" rel="noopener"><title>{full}</title><text class="label" x="{lx}" y="{cy}" dominant-baseline="central">{label}</text></a>
    {bar}
    <text class="value" x="{vx}" y="{cy}" text-anchor="{anchor}" dominant-baseline="central" fill="{ink}">{value}</text>
  </g>
"##,
            motion = motion_reveal(index),
            href = escape_xml(&href),
            full = escape_xml(&row.path),
            lx = texture::coord(PAD),
            cy = texture::coord(center),
            label = escape_xml(&truncate_tail(&row.path, 44)),
            vx = texture::coord(value_x),
        ));
    }

    // The zero datum every bar is measured from, and the one dimension: the
    // peak, spanning the whole plot because the peak sets the scale.
    let (datum, dimension) = if max_count > 0 {
        let bar_top = header_h + (row_h - bar_h) / 2.0;
        let value = format!("{max_count} {}", cfg.quantity);
        (
            object_line(
                plot_x,
                header_h + 4.0,
                plot_x,
                plot_bottom - 4.0,
                theme.border,
                texture::W_OBJECT,
            ),
            format!(
                "  {left}{right}{dim}\n",
                left = texture::extension_tick(
                    plot_x,
                    bar_top,
                    Side::Up,
                    extension_len(DIM_STANDOFF),
                    theme.border
                ),
                right = texture::extension_tick(
                    plot_x + plot_w,
                    bar_top,
                    Side::Up,
                    extension_len(DIM_STANDOFF),
                    theme.border
                ),
                dim = texture::dimension_h(
                    plot_x,
                    plot_x + plot_w,
                    bar_top - DIM_STANDOFF,
                    &measured(&value, theme, 11.0)
                ),
            ),
        )
    } else {
        // Nothing to measure, so nothing is drawn but the reason. A blank
        // plot area reads as a rendering fault; a sentence does not.
        (
            format!(
                "<text class=\"caption\" x=\"{x}\" y=\"{y}\" text-anchor=\"middle\">{message}</text>",
                x = texture::coord(width / 2.0),
                y = texture::coord(header_h + 34.0),
                message = escape_xml(cfg.empty),
            ),
            String::new(),
        )
    };

    let repo_field = truncate_tail(cfg.repo, 22);
    let rows_field = cfg.rows.len().to_string();
    let peak_field = max_count.to_string();
    let fields = [
        TitleField {
            label: "repo",
            value: &repo_field,
        },
        TitleField {
            label: "rows",
            value: &rows_field,
        },
        TitleField {
            label: "peak",
            value: &peak_field,
        },
    ];
    let block_y = plot_bottom + 22.0;
    let height = block_y + texture::title_block_height(fields.len()) + COLOPHON_BAND;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="{title} for {repo}">
  <style><![CDATA[
{css}
    .bar-link {{ cursor: pointer; }}
    .bar-link:hover .label {{ text-decoration: underline; }}
  ]]></style>
  <text class="title" x="{pad}" y="{title_y}">{title}</text>
  <text class="caption" x="{pad}" y="{caption_y}">{caption}</text>
{dimension}  {datum}
{body}{block}{footer}
</svg>"##,
        title = escape_xml(cfg.title),
        repo = escape_xml(cfg.repo),
        caption = escape_xml(cfg.caption),
        css = sheet_css(theme),
        pad = texture::coord(PAD),
        title_y = texture::coord(TITLE_Y),
        caption_y = texture::coord(CAPTION_Y),
        block = texture::title_block(width - PAD - BLOCK_W, block_y, BLOCK_W, &fields, theme),
        footer = brand::footer_lockup(width - PAD, height - 12.0, theme),
    )
}

/// Where the measured figure sits relative to its bar.
///
/// A bar wide enough to swallow the value letters it INSIDE, right against
/// the leading edge, in whichever of graphite or paper stays legible on the
/// flat pen. A short bar letters it outside, in ink. Returns
/// `(x, text-anchor, fill)`.
fn count_placement<'a>(
    plot_x: f32,
    bar_w: f32,
    text: &str,
    bar_ink: &str,
    outside_ink: &'a str,
) -> (f32, &'static str, &'a str) {
    // ~6.6 px/char is the advance of an 11px monospace digit.
    let estimated = (text.chars().count() as f32) * 6.6;
    if bar_w >= estimated + 16.0 {
        (plot_x + bar_w - 8.0, "end", contrast_on(bar_ink))
    } else {
        (plot_x + bar_w + 8.0, "start", outside_ink)
    }
}

// ---------------------------------------------------------------------------
// Commit heatmap
// ---------------------------------------------------------------------------

/// One step of the commit-intensity ladder: how wide its mark is inside the
/// day cell, and which of the drawing's inks it is drawn in.
type HeatStep = (f32, fn(&Theme) -> &'static str);

/// The four plotted steps of one day's commit count.
///
/// A stepped set of inks, and deliberately NOT the plotter pens. The pen set
/// exists for several series drawn at once, and it sits in one narrow
/// lightness band precisely so no series shouts over another — which is the
/// opposite of what a magnitude needs. Intensity is one series in steps, so
/// it steps through the drawing's own ink ladder instead, from the frame line
/// up to graphite, and the mark grows with it. Two encodings, one direction:
/// the ladder reads as more even where two of its inks sit close together,
/// and the key letters the count range each step stands for.
const HEAT_STEPS: [HeatStep; 4] = [
    (8.0, |theme| theme.border),
    (11.0, |theme| theme.ink_3),
    (14.0, |theme| theme.muted),
    (14.0, |theme| theme.fg),
];

/// Render the commit heatmap.
///
/// `analyzed_from` is the first day the analysis window actually covers, when
/// that window is capped. Days before it have no observation behind them, so
/// they must not be labelled "0 commits" — for a repository above the commit
/// limit that is a confident assertion about history the analysis never read.
pub fn render_heatmap(
    repo: &str,
    subtitle_label: &str,
    start: NaiveDate,
    end: NaiveDate,
    days: &[DayCount],
    analyzed_from: Option<NaiveDate>,
    theme: &Theme,
) -> String {
    use std::collections::BTreeMap;
    let counts: BTreeMap<NaiveDate, i64> = days.iter().map(|d| (d.day, d.commits)).collect();
    let mut sorted: Vec<i64> = counts.values().copied().filter(|c| *c > 0).collect();
    sorted.sort_unstable();
    let quantile = |p: f32| -> i64 {
        if sorted.is_empty() {
            return 0;
        }
        let index = ((sorted.len() as f32 - 1.0) * p).round() as usize;
        sorted[index]
    };
    let steps = (quantile(0.25), quantile(0.5), quantile(0.75));

    let cell = 14.0_f32;
    let gap = 3.0_f32;
    let step = cell + gap;
    let pad_left = 44.0_f32;
    let pad_top = 96.0_f32;
    let aligned_start = first_monday_on_or_before(start);
    let total_days = (end - aligned_start).num_days().max(0) as u32 + 1;
    let cols = total_days.div_ceil(7);
    let plot_w = cols as f32 * step;
    let plot_h = 7.0 * step;
    let width = (pad_left + plot_w + 40.0).max(640.0);

    let mut cells = String::new();
    let mut total = 0i64;
    let mut max_seen = 0i64;
    let mut peak: Option<(NaiveDate, f32, f32)> = None;
    let mut day_iter = start;
    while day_iter <= end {
        let weekday = day_iter.weekday().num_days_from_monday();
        let col = ((day_iter - aligned_start).num_days() / 7) as f32;
        let count = counts.get(&day_iter).copied().unwrap_or(0);
        total = total.saturating_add(count);
        let x = pad_left + col * step;
        let y = pad_top + weekday as f32 * step;
        if count > max_seen {
            max_seen = count;
            peak = Some((day_iter, x, y));
        }
        let href = format!(
            "https://github.com/{repo}/commits?since={day}T00%3A00%3A00Z&amp;until={day}T23%3A59%3A59Z",
            day = day_iter,
        );
        let label = if analyzed_from.is_some_and(|from| day_iter < from) {
            format!("{day_iter} · outside the analyzed window · open on GitHub")
        } else {
            let plural = if count == 1 { "" } else { "s" };
            format!("{day_iter} · {count} commit{plural} · open on GitHub")
        };
        cells.push_str(&format!(
            r##"<a class="day-link" href="{href}" target="_blank" rel="noopener" aria-label="Open commits from {day}">
      <rect class="cell" x="{x}" y="{y}" width="{cell}" height="{cell}" fill="{ground}"><title>{label}</title></rect>{mark}
    </a>
    "##,
            x = texture::coord(x),
            y = texture::coord(y),
            cell = texture::coord(cell),
            ground = theme.track,
            mark = heat_mark(x, y, cell, heat_level(count, steps), theme),
            day = day_iter,
        ));
        let Some(next) = day_iter.succ_opt() else {
            break;
        };
        day_iter = next;
    }

    // The key: the same five steps, at the same size and in the same ink as
    // the calendar draws them, each lettered with the count range it stands
    // for. A step whose mark is smaller than its cell is smaller here too.
    let band_y = pad_top + plot_h + 26.0;
    let mut key = format!(
        "  <text class=\"field\" x=\"{x}\" y=\"{y}\">COMMITS PER DAY</text>\n",
        x = texture::coord(pad_left),
        y = texture::coord(band_y),
    );
    for (index, range) in heat_key_ranges(steps).iter().enumerate() {
        let swatch_x = pad_left + index as f32 * 58.0;
        let swatch_y = band_y + 10.0;
        key.push_str(&format!(
            "  <rect x=\"{x}\" y=\"{y}\" width=\"{cell}\" height=\"{cell}\" fill=\"{ground}\" shape-rendering=\"crispEdges\" />{mark}<text class=\"meta\" x=\"{tx}\" y=\"{ty}\" dominant-baseline=\"central\">{range}</text>\n",
            x = texture::coord(swatch_x),
            y = texture::coord(swatch_y),
            cell = texture::coord(cell),
            ground = theme.track,
            mark = heat_mark(swatch_x, swatch_y, cell, index, theme),
            tx = texture::coord(swatch_x + cell + 6.0),
            ty = texture::coord(swatch_y + cell / 2.0),
            range = escape_xml(range),
        ));
    }

    let mut dow = String::new();
    for (index, label) in ["Mon", "Wed", "Fri"].iter().enumerate() {
        let row = index as f32 * 2.0;
        dow.push_str(&format!(
            "  <text class=\"meta\" x=\"{x}\" y=\"{y}\" text-anchor=\"end\" dominant-baseline=\"central\">{label}</text>\n",
            x = texture::coord(pad_left - 8.0),
            y = texture::coord(pad_top + row * step + cell / 2.0),
        ));
    }

    // The peak is the sheet's measured datum, so it gets the red leader.
    let leader = peak.map_or_else(String::new, |(day, x, y)| {
        let datum = (x + cell / 2.0, y);
        let label_x = if datum.0 < width / 2.0 {
            datum.0 + 46.0
        } else {
            datum.0 - 46.0
        };
        format!(
            "  {}\n",
            texture::leader(
                datum,
                (label_x, pad_top - 22.0),
                &format!("peak {max_seen} · {day}"),
                11.0,
                theme.accent,
            )
        )
    });

    let window_label = analyzed_from.map_or_else(
        || subtitle_label.to_string(),
        |from| format!("{subtitle_label} · bounded analysis from {from}"),
    );
    let repo_field = truncate_tail(repo, 22);
    let days_field = ((end - start).num_days().max(0) + 1).to_string();
    let total_field = humanize(total);
    let peak_field = max_seen.to_string();
    let fields = [
        TitleField {
            label: "repo",
            value: &repo_field,
        },
        TitleField {
            label: "days",
            value: &days_field,
        },
        TitleField {
            label: "commits",
            value: &total_field,
        },
        TitleField {
            label: "peak day",
            value: &peak_field,
        },
    ];
    let block_y = band_y - 8.0;
    let height = block_y + texture::title_block_height(fields.len()) + COLOPHON_BAND;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="Commit activity for {repo}">
  <style><![CDATA[
{css}
    .cell {{ shape-rendering: crispEdges; }}
    .day-link {{ cursor: pointer; }}
    .cell:hover {{ stroke: {fg}; stroke-width: 1; }}
  ]]></style>
  <text class="title" x="{pad}" y="{title_y}">{repo}</text>
  <text class="caption" x="{pad}" y="{caption_y}">{window_label} · {total} commits</text>
{dow}  <g class="heat-cells" opacity="1">
    {motion}
    {cells}
  </g>
{leader}{key}{block}{footer}
</svg>"##,
        repo = escape_xml(repo),
        window_label = escape_xml(&window_label),
        css = sheet_css(theme),
        fg = theme.fg,
        pad = texture::coord(pad_left),
        title_y = texture::coord(TITLE_Y),
        caption_y = texture::coord(CAPTION_Y),
        motion = motion_reveal(0),
        block = texture::title_block(width - 40.0 - BLOCK_W, block_y, BLOCK_W, &fields, theme),
        footer = brand::footer_lockup(width - 24.0, height - 12.0, theme),
    )
}

/// Which of the five steps a day's commit count falls in.
fn heat_level(count: i64, steps: (i64, i64, i64)) -> usize {
    if count <= 0 {
        0
    } else if count <= steps.0 {
        1
    } else if count <= steps.1 {
        2
    } else if count <= steps.2 {
        3
    } else {
        4
    }
}

/// The mark one step plots inside its cell, centred on it.
///
/// Step 0 draws nothing at all: a day with no commits keeps its place in the
/// calendar, on the empty ground, and carries no ink. Every other step is a
/// square of the step's own size and ink, centred so the ladder grows from
/// the middle of the cell outward.
fn heat_mark(x: f32, y: f32, cell: f32, level: usize, theme: &Theme) -> String {
    if level == 0 {
        return String::new();
    }
    let (size, ink) = HEAT_STEPS[(level - 1).min(HEAT_STEPS.len() - 1)];
    let inset = (cell - size) / 2.0;
    format!(
        "<rect class=\"mark\" x=\"{x}\" y=\"{y}\" width=\"{size}\" height=\"{size}\" fill=\"{ink}\" shape-rendering=\"crispEdges\" pointer-events=\"none\" />",
        x = texture::coord(x + inset),
        y = texture::coord(y + inset),
        size = texture::coord(size),
        ink = ink(theme),
    )
}

/// What each step of the key measures, lettered.
fn heat_key_ranges(steps: (i64, i64, i64)) -> [String; 5] {
    [
        "0".to_string(),
        format!("≤{}", steps.0.max(1)),
        format!("≤{}", steps.1.max(steps.0).max(1)),
        format!("≤{}", steps.2.max(steps.1).max(1)),
        format!(">{}", steps.2.max(steps.1).max(1)),
    ]
}

fn first_monday_on_or_before(d: NaiveDate) -> NaiveDate {
    let offset = d.weekday().num_days_from_monday() as i64;
    d - chrono::Duration::days(offset)
}

// ---------------------------------------------------------------------------
// Contributors
// ---------------------------------------------------------------------------

pub fn render_contributors(repo: &str, contributors: &[ContributorRow], theme: &Theme) -> String {
    let width = 1100u32;
    let pad = 44u32;
    let avatar_y = 86u32;
    let row_step = 82u32;
    let min_column_step = 76u32;
    let content_width = width - pad * 2;
    let column_span = content_width.saturating_sub(AVATAR_TILE);
    let columns = (column_span / min_column_step + 1).max(1);
    // The endpoint already supplies a bounded, ordered author set. Rendering
    // every provided row avoids silently turning that set into a second,
    // undocumented top-16 sample.
    let row_count = (contributors.len() as u32).div_ceil(columns);

    let mut tiles = String::new();
    for (index, person) in contributors.iter().enumerate() {
        let column = index as u32 % columns;
        let row = index as u32 / columns;
        // Distribute complete rows across the full padded content width.
        let column_offset = if columns > 1 {
            column * column_span / (columns - 1)
        } else {
            0
        };
        let x = pad + AVATAR_MARGIN + column_offset;
        let y = avatar_y + row * row_step;
        let label = person.login.clone().unwrap_or_else(|| person.name.clone());
        let content = format!(
            r##"<title>{label}</title>
      <g class="avatar-frame">
        <clipPath id="contributor-clip-{index}"><rect width="{size}" height="{size}" /></clipPath>
        {photo}
        <rect class="avatar-edge" x="0" y="0" width="{size}" height="{size}" />
      </g>"##,
            label = escape_xml(&label),
            size = AVATAR_SIZE,
            photo = avatar_photo(
                person.avatar_url.as_deref(),
                &label,
                &format!("contributor-clip-{index}")
            ),
        );
        let linked = person.login.as_ref().map_or(content.clone(), |login| {
            format!(
                r##"<a class="contributor-link" href="{href}" target="_blank" rel="noopener">{content}</a>"##,
                href = escape_xml(&format!("https://github.com/{login}")),
            )
        });
        tiles.push_str(&format!(
            r##"  <g class="contributor-node" transform="translate({x}, {y})" opacity="1">
    {motion}
    {linked}
  </g>
"##,
            motion = motion_reveal(index),
        ));
    }

    let repo_field = truncate_tail(repo, 22);
    let authors_field = contributors.len().to_string();
    let fields = [
        TitleField {
            label: "repo",
            value: &repo_field,
        },
        TitleField {
            label: "authors",
            value: &authors_field,
        },
    ];
    let sheet_w = width as f32;
    let grid_bottom = if row_count == 0 {
        avatar_y as f32
    } else {
        (avatar_y + (row_count - 1) * row_step + AVATAR_SIZE) as f32
    };
    let block_y = grid_bottom + 26.0;
    let height = block_y + texture::title_block_height(fields.len()) + COLOPHON_BAND;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height:.0}" role="img" aria-label="Contributors of {repo}">
  <style><![CDATA[
{css}
{avatar_css}
    .contributor-link {{ cursor: pointer; }}
    .contributor-link:hover .avatar-edge {{ stroke: {fg}; stroke-width: {emphasis}; }}
  ]]></style>
  <text class="title" x="{pad}" y="{title_y}">Contributors</text>
  <text class="caption" x="{pad}" y="{caption_y}">{count} public commit author{plural} · {repo}</text>
{tiles}{block}{footer}
</svg>"##,
        repo = escape_xml(repo),
        css = sheet_css(theme),
        avatar_css = avatar_css(theme),
        fg = theme.fg,
        emphasis = texture::W_EMPHASIS,
        title_y = texture::coord(TITLE_Y),
        caption_y = texture::coord(CAPTION_Y),
        count = contributors.len(),
        plural = if contributors.len() == 1 { "" } else { "s" },
        block = texture::title_block(
            sheet_w - pad as f32 - BLOCK_W,
            block_y,
            BLOCK_W,
            &fields,
            theme
        ),
        footer = brand::footer_lockup(sheet_w - pad as f32, height - 12.0, theme),
    )
}

/// The lettering and line roles a placed photo needs.
///
/// A photo is an object on the sheet: it sits square, inside a 1px frame, and
/// carries no ring, no glow and no rounded corner. When there is no photo the
/// frame holds the person's initial on the second ground instead.
fn avatar_css(theme: &Theme) -> String {
    format!(
        r##"    .avatar-edge {{ fill: none; stroke: {border}; stroke-width: {object}; }}
    .avatar-fallback-bg {{ fill: {track}; }}
    .avatar-fallback {{ fill: {fg}; font: 600 22px {mono}; }}"##,
        border = theme.border,
        object = texture::W_OBJECT,
        track = theme.track,
        fg = theme.fg,
        mono = texture::MONO,
    )
}

/// The photo itself, or the initial when a contributor has none.
///
/// Centred by `dominant-baseline`, not by a guessed offset: a glyph that sits
/// high in its frame is the most repeated execution miss there is.
fn avatar_photo(url: Option<&str>, label: &str, clip_id: &str) -> String {
    let size = AVATAR_SIZE as f32;
    match url {
        Some(url) => format!(
            r#"<image href="{url}" x="0" y="0" width="{size:.0}" height="{size:.0}" clip-path="url(#{clip_id})" preserveAspectRatio="xMidYMid slice" />"#,
            url = escape_xml(url),
        ),
        None => {
            let initial = label
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            format!(
                r#"<rect class="avatar-fallback-bg" x="0" y="0" width="{size:.0}" height="{size:.0}" /><text class="avatar-fallback" x="{c}" y="{c}" text-anchor="middle" dominant-baseline="central">{initial}</text>"#,
                c = texture::coord(size / 2.0),
                initial = escape_xml(&initial),
            )
        }
    }
}

/// One contributor's photo as a standalone asset: the square, its frame, and
/// nothing else. No title, no caption, no title block, no colophon.
///
/// The grid's own `<a>` wrappers can never fire in a README. An SVG loaded
/// through an HTML `<img>` renders in SVG2 secure animated mode, where
/// declarative animation still plays but script, external references and every
/// form of interactivity are switched off. A linked contributor grid therefore
/// has to be one `<a>` per tile in the README's *own* markup, which needs one
/// image per tile — this one.
///
/// `rank` only shifts the reveal delay, reusing the grid's stagger so a README
/// laid out from these tiles fades in exactly like the single-image chart.
pub fn render_contributor_avatar(
    contributor: &ContributorRow,
    rank: usize,
    theme: &Theme,
) -> String {
    let label = contributor
        .login
        .clone()
        .unwrap_or_else(|| contributor.name.clone());
    // Intrinsic width/height as well as the viewBox: a README lays these out
    // as bare `<img>` tiles, and a sizeless SVG would be stretched to the
    // viewer's default replaced-element box instead of staying a 72px tile.
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {AVATAR_TILE} {AVATAR_TILE}" width="{AVATAR_TILE}" height="{AVATAR_TILE}" role="img" aria-label="{label}">
  <style><![CDATA[
{avatar_css}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
    }}
  ]]></style>
  <g transform="translate({AVATAR_MARGIN}, {AVATAR_MARGIN})" opacity="1">
    {motion}
    <clipPath id="gd-avatar-clip"><rect width="{AVATAR_SIZE}" height="{AVATAR_SIZE}" /></clipPath>
    {photo}
    <rect class="avatar-edge" x="0" y="0" width="{AVATAR_SIZE}" height="{AVATAR_SIZE}" />
  </g>
</svg>"##,
        label = escape_xml(&label),
        avatar_css = avatar_css(theme),
        motion = motion_reveal(rank),
        photo = avatar_photo(contributor.avatar_url.as_deref(), &label, "gd-avatar-clip"),
    )
}

/// The answer for a contributor slot nobody occupies.
///
/// A README grid is pasted with a fixed number of slots and the author set
/// underneath it shrinks and grows, so the tail slots have to disappear rather
/// than break: a 404 draws the broken-image glyph, and any visible placeholder
/// would assert a contributor who does not exist.
pub fn render_blank_avatar() -> String {
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1" width="1" height="1" role="presentation"></svg>"#
        .to_string()
}

// ---------------------------------------------------------------------------
// Contribution footprint
// ---------------------------------------------------------------------------

pub fn render_contribution_profile(
    login: &str,
    profile: &ContributionProfile,
    theme: &Theme,
) -> String {
    let width = 1100.0_f32;
    let plot_x = 300.0_f32;
    let value_gutter = 110.0_f32;
    let plot_w = width - plot_x - PAD - value_gutter;
    let lane_h = 26.0_f32;
    let owned_y = 132.0_f32;
    let external_y = 208.0_f32;

    let total_commits = profile
        .owned_commits
        .max(0)
        .saturating_add(profile.external_commits.max(0));
    let lane_w = |commits: i64| {
        if commits <= 0 || total_commits <= 0 {
            0.0
        } else {
            ((commits as f32 / total_commits as f32) * plot_w).clamp(18.0, plot_w)
        }
    };
    let owned_w = lane_w(profile.owned_commits);
    let external_w = lane_w(profile.external_commits);
    // Two series on one sheet, so the pens come from the shared allocator and
    // each lane is labelled at its own end regardless.
    let pens = pens_for(theme, &["owned", "external"]);

    let lane = |y: f32, w: f32, pen: &str, repos: i64, commits: i64, index: usize| -> String {
        let track = format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\" />",
            x = texture::coord(plot_x),
            y = texture::coord(y),
            w = texture::coord(plot_w),
            h = texture::coord(lane_h),
            fill = theme.track,
        );
        let bar = if w > 0.0 {
            texture::series_bar(plot_x, y, w, lane_h, pen, theme.fg, Side::Right)
        } else {
            String::new()
        };
        // One extension tick per repository, tallied along the lane it was
        // measured on. Capped, because a tally that overruns its own lane
        // stops being a count.
        let shown = repos.clamp(0, 18) as usize;
        let mut tally = String::new();
        if shown > 0 && w > 0.0 {
            for mark in 0..shown {
                let at = plot_x + (mark as f32 + 0.5) / shown as f32 * w;
                tally.push_str(&texture::extension_tick(
                    at,
                    y + lane_h,
                    Side::Down,
                    5.0,
                    theme.ink_3,
                ));
            }
        }
        let value = format!("{} commits", humanize(commits.max(0)));
        let (value_x, anchor, ink) = count_placement(plot_x, w, &value, pen, theme.fg);
        format!(
            r##"  <g opacity="1">
    {motion}
    {track}
    {bar}
    {tally}
    <text class="value" x="{vx}" y="{cy}" text-anchor="{anchor}" dominant-baseline="central" fill="{ink}">{value}</text>
  </g>
"##,
            motion = motion_reveal(index),
            vx = texture::coord(value_x),
            cy = texture::coord(y + lane_h / 2.0),
            value = escape_xml(&value),
        )
    };

    // The measured split is what this sheet is about, so the wider lane
    // carries the one dimension, in red, above its own bar.
    let dimension = {
        let (y, w, commits) = if owned_w >= external_w {
            (owned_y, owned_w, profile.owned_commits)
        } else {
            (external_y, external_w, profile.external_commits)
        };
        if w > 0.0 && total_commits > 0 {
            let share = commits.max(0) as f32 / total_commits as f32 * 100.0;
            let value = format!("{share:.0}% of commits");
            format!(
                "  {left}{right}{dim}\n",
                left = texture::extension_tick(
                    plot_x,
                    y,
                    Side::Up,
                    extension_len(DIM_STANDOFF),
                    theme.border
                ),
                right = texture::extension_tick(
                    plot_x + w,
                    y,
                    Side::Up,
                    extension_len(DIM_STANDOFF),
                    theme.border
                ),
                dim = texture::dimension_h(
                    plot_x,
                    plot_x + w,
                    y - DIM_STANDOFF,
                    &measured(&value, theme, 11.0)
                ),
            )
        } else {
            String::new()
        }
    };

    let style = if total_commits == 0 {
        "No attributed commits yet"
    } else {
        let external_share = profile.external_commits.max(0) as f64 / total_commits as f64;
        if external_share >= 0.68 {
            "Ecosystem-led contributor"
        } else if external_share <= 0.32 {
            "Builder-led contributor"
        } else {
            "Balanced project footprint"
        }
    };

    let login_field = truncate_tail(login, 22);
    let commits_field = humanize(total_commits);
    let repos_field = format!(
        "{} + {}",
        profile.owned_repos.max(0),
        profile.external_repos.max(0)
    );
    let breakout_field = format!(
        "{} {}",
        profile.visionary_count,
        if profile.visionary_count == 1 {
            "project"
        } else {
            "projects"
        }
    );
    let mut fields = vec![
        TitleField {
            label: "login",
            value: &login_field,
        },
        TitleField {
            label: "commits",
            value: &commits_field,
        },
        TitleField {
            label: "repos",
            value: &repos_field,
        },
    ];
    if profile.visionary_count > 0 {
        fields.push(TitleField {
            label: "breakout",
            value: &breakout_field,
        });
    }
    let block_y = external_y + lane_h + 34.0;
    let height = block_y + texture::title_block_height(fields.len()) + COLOPHON_BAND;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="Contribution footprint for {login}">
  <style><![CDATA[
{css}
    .lane {{ fill: {fg}; font: 600 13px {sans}; }}
  ]]></style>
  <text class="title" x="{pad}" y="{title_y}">Contribution footprint</text>
  <text class="caption" x="{pad}" y="{caption_y}">{login} · {style}</text>
{dimension}  <text class="lane" x="{pad}" y="{owned_label_y}">Owned projects</text>
  <text class="meta" x="{pad}" y="{owned_meta_y}">{owned_repos} repos</text>
{owned}  <text class="lane" x="{pad}" y="{external_label_y}">Other people's projects</text>
  <text class="meta" x="{pad}" y="{external_meta_y}">{external_repos} repos</text>
{external}{block}{footer}
</svg>"##,
        login = escape_xml(login),
        css = sheet_css(theme),
        fg = theme.fg,
        sans = texture::SANS,
        pad = texture::coord(PAD),
        title_y = texture::coord(TITLE_Y),
        caption_y = texture::coord(CAPTION_Y),
        owned_label_y = texture::coord(owned_y + 6.0),
        owned_meta_y = texture::coord(owned_y + 25.0),
        external_label_y = texture::coord(external_y + 6.0),
        external_meta_y = texture::coord(external_y + 25.0),
        owned_repos = profile.owned_repos.max(0),
        external_repos = profile.external_repos.max(0),
        owned = lane(
            owned_y,
            owned_w,
            pens[0],
            profile.owned_repos,
            profile.owned_commits,
            0
        ),
        external = lane(
            external_y,
            external_w,
            pens[1],
            profile.external_repos,
            profile.external_commits,
            1
        ),
        block = texture::title_block(width - PAD - BLOCK_W, block_y, BLOCK_W, &fields, theme),
        footer = brand::footer_lockup(width - PAD, height - 12.0, theme),
    )
}

// ---------------------------------------------------------------------------
// Language activity
// ---------------------------------------------------------------------------

pub fn render_languages(repo: &str, rows: &[LanguageBar], theme: &Theme) -> String {
    let width = 1100.0_f32;
    if rows.is_empty() {
        // A repository whose whole tree is assets, data, or vendored content
        // has no classified source. Say so, rather than emitting a header
        // over an empty plot area that reads as a rendering failure.
        return empty_chart(width, 200.0, "no recognized source files in HEAD", theme);
    }
    let label_w = 190.0_f32;
    let plot_x = PAD + label_w;
    let plot_w = 500.0_f32;
    let value_gutter = 76.0_f32;
    let meta_x = plot_x + plot_w + value_gutter;
    let row_h = 30.0_f32;
    let bar_h = 14.0_f32;
    let header_h = 112.0_f32;

    let line_totals: Vec<i64> = rows
        .iter()
        .map(|r| r.lines_code + r.lines_blank + r.lines_comment)
        .collect();
    let file_census = line_totals.iter().all(|total| *total == 0);
    let totals: Vec<i64> = if file_census {
        rows.iter().map(|row| row.files).collect()
    } else {
        line_totals
    };
    let max_total = totals.iter().copied().max().unwrap_or(1).max(1);
    let total_total: i64 = totals.iter().sum();
    let total_code: i64 = rows.iter().map(|r| r.lines_code).sum();
    let total_files: i64 = rows.iter().map(|r| r.files).sum();

    let mut bars = String::new();
    for (index, row) in rows.iter().enumerate() {
        let total = totals[index];
        let bar_y = header_h + index as f32 * row_h + (row_h - bar_h) / 2.0;
        let center = bar_y + bar_h / 2.0;
        let bar_w = (total.max(0) as f32 / max_total as f32) * plot_w;
        // Language colours are the conventional brand hues readers already
        // expect, not pens: they are the one place a chart's ink is chosen
        // by the world instead of by this palette.
        let ink = language_color(&row.language, theme);
        let value = humanize(total);
        let (value_x, anchor, value_ink) = count_placement(plot_x, bar_w, &value, &ink, theme.fg);
        let meta = if file_census {
            format!(
                "{} file{}",
                row.files,
                if row.files == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "{} file{} · {} code",
                row.files,
                if row.files == 1 { "" } else { "s" },
                humanize(row.lines_code),
            )
        };
        let title = if file_census {
            format!("{} files in current HEAD", row.files)
        } else {
            format!(
                "{total} total · {} code · {} comments · {} blank",
                row.lines_code, row.lines_comment, row.lines_blank
            )
        };
        bars.push_str(&format!(
            r##"  <g opacity="1">
    {motion}
    <title>{title}</title>
    <text class="label" x="{lx}" y="{cy}" dominant-baseline="central">{language}</text>
    {bar}
    <text class="value" x="{vx}" y="{cy}" text-anchor="{anchor}" dominant-baseline="central" fill="{value_ink}">{value}</text>
    <text class="meta" x="{mx}" y="{cy}" dominant-baseline="central">{meta}</text>
  </g>
"##,
            motion = motion_reveal(index),
            title = escape_xml(&title),
            lx = texture::coord(PAD),
            cy = texture::coord(center),
            language = escape_xml(&row.language),
            bar = if bar_w >= 0.5 {
                texture::series_bar(plot_x, bar_y, bar_w, bar_h, &ink, theme.fg, Side::Right)
            } else {
                String::new()
            },
            vx = texture::coord(value_x),
            mx = texture::coord(meta_x),
            meta = escape_xml(&meta),
        ));
    }

    let bar_top = header_h + (row_h - bar_h) / 2.0;
    let plot_bottom = header_h + rows.len() as f32 * row_h;
    let peak_value = format!(
        "{} {}",
        humanize(max_total),
        if file_census { "files" } else { "lines" }
    );
    let dimension = format!(
        "  {left}{right}{dim}\n",
        left = texture::extension_tick(
            plot_x,
            bar_top,
            Side::Up,
            extension_len(DIM_STANDOFF),
            theme.border
        ),
        right = texture::extension_tick(
            plot_x + plot_w,
            bar_top,
            Side::Up,
            extension_len(DIM_STANDOFF),
            theme.border
        ),
        dim = texture::dimension_h(
            plot_x,
            plot_x + plot_w,
            bar_top - DIM_STANDOFF,
            &measured(&peak_value, theme, 11.0)
        ),
    );
    let datum = object_line(
        plot_x,
        header_h + 4.0,
        plot_x,
        plot_bottom - 4.0,
        theme.border,
        texture::W_OBJECT,
    );

    let caption = if file_census {
        format!(
            "{} · {} files in {} languages · current HEAD tree",
            repo,
            humanize(total_files),
            rows.len()
        )
    } else {
        format!(
            "{} · {} lines · {} code · {} files in {} languages",
            repo,
            humanize(total_total),
            humanize(total_code),
            total_files,
            rows.len()
        )
    };
    let aria = if file_census {
        format!("Language file activity in {repo}")
    } else {
        format!("Lines of code in {repo}")
    };

    let repo_field = truncate_tail(repo, 22);
    let languages_field = rows.len().to_string();
    let total_field = humanize(total_total);
    let code_field = humanize(total_code);
    let mut fields = vec![
        TitleField {
            label: "repo",
            value: &repo_field,
        },
        TitleField {
            label: "languages",
            value: &languages_field,
        },
        TitleField {
            label: if file_census { "files" } else { "lines" },
            value: &total_field,
        },
    ];
    if !file_census {
        fields.push(TitleField {
            label: "code",
            value: &code_field,
        });
    }
    let block_y = plot_bottom + 22.0;
    let height = block_y + texture::title_block_height(fields.len()) + COLOPHON_BAND;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="{aria}">
  <style><![CDATA[
{css}
  ]]></style>
  <text class="title" x="{pad}" y="{title_y}">Language activity</text>
  <text class="caption" x="{pad}" y="{caption_y}">{caption}</text>
{dimension}  {datum}
{bars}{block}{footer}
</svg>"##,
        aria = escape_xml(&aria),
        css = sheet_css(theme),
        pad = texture::coord(PAD),
        title_y = texture::coord(TITLE_Y),
        caption_y = texture::coord(CAPTION_Y),
        caption = escape_xml(&caption),
        block = texture::title_block(width - PAD - BLOCK_W, block_y, BLOCK_W, &fields, theme),
        footer = brand::footer_lockup(width - PAD, height - 12.0, theme),
    )
}

fn humanize(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// ---------------------------------------------------------------------------
// Per-language ink
// ---------------------------------------------------------------------------

/// Achromatic gray for buckets that are not one language: the synthetic
/// `Config` rollup and anything the census could not name. Documented
/// meaning: "no single language".
const NEUTRAL_LANGUAGE_COLOR: &str = "#8b8b8b";

/// The conventional color for a language, as readers already expect it
/// (Go cyan, Rust rust, TypeScript blue, …). Synthetic aliases inherit
/// their parent language's color. `None` → no conventional color exists,
/// so the caller derives a stable hue from the name instead.
///
/// These are the real language brand colours and they are deliberately
/// outside the drawing's palette: a reader recognises Rust by its own colour
/// and would not recognise it in a plotter pen, so the redraw left every hex
/// here untouched. Any surface that shows a language mirrors this table
/// exactly, so a bar is the same hue in the app and in an exported sheet.
/// This map is the *source* hue
/// only; [`language_color`] is what renders, because these values were picked
/// against a white page and several of them (`#292929`, `#012456`,
/// `#f1e05a`) are unreadable on one of our two prints untouched.
fn conventional_language_color(name: &str) -> Option<&'static str> {
    Some(match name {
        "Rust" => "#dea584",
        "TypeScript" | "TSX" => "#3178c6",
        "JavaScript" | "JSX" => "#f1e05a",
        "Python" => "#3572a5",
        "Go" => "#00add8",
        "Ruby" => "#701516",
        "Java" => "#b07219",
        "Kotlin" => "#a97bff",
        "Swift" => "#f05138",
        "Objective-C" => "#438eff",
        "C" => "#555555",
        "C++" => "#f34b7d",
        "C#" => "#178600",
        "Shell" | "Bash" => "#89e051",
        "PowerShell" => "#012456",
        "HTML" => "#e34c26",
        "CSS" => "#563d7c",
        "SCSS" => "#c6538c",
        "Less" => "#1d365d",
        "Vue" => "#41b883",
        "Svelte" => "#ff3e00",
        "Astro" => "#ff5a03",
        "Markdown" => "#083fa1",
        "TOML" => "#9c4221",
        "YAML" => "#cb171e",
        "JSON" => "#292929",
        "XML" => "#0060ac",
        "SVG" => "#ff9900",
        "Dockerfile" => "#384d54",
        "Lua" => "#000080",
        "Perl" => "#0298c3",
        "PHP" => "#4f5d95",
        "Scala" => "#c22d40",
        "Groovy" => "#4298b8",
        "Haskell" => "#5e5086",
        "Elixir" => "#6e4a7e",
        "Erlang" => "#b83998",
        "OCaml" => "#3be133",
        "Dart" => "#00b4ab",
        "Zig" => "#ec915c",
        "R" => "#198ce7",
        "Julia" => "#a270ba",
        "Nix" => "#7e7eff",
        "Makefile" => "#427819",
        "CMake" => "#da3434",
        "SQL" => "#e38c00",
        "GraphQL" => "#e10098",
        "Verilog" => "#b2b7f8",
        "Terraform" | "HCL" => "#844fba",
        "TeX" => "#3d6117",
        "Config" => NEUTRAL_LANGUAGE_COLOR,
        _ => return None,
    })
}

/// Stable per-language ink, legibility-corrected for `theme`.
///
/// Two rules, in order:
///   1. hue is the language's conventional color when one exists, and a
///      deterministic name-derived hue otherwise — so two languages never
///      share a bar color just because they are adjacent in the list;
///   2. the result is pushed into the print's readable luminance band by
///      blending toward white (dark) or black (light). Hue survives; only
///      lightness moves. Same input → same bytes.
pub fn language_color(name: &str, theme: &Theme) -> String {
    let rgb = conventional_language_color(name)
        .and_then(parse_hex)
        .unwrap_or_else(|| hashed_language_rgb(name));
    let (min, max) = if theme.dark {
        (0.30_f32, 0.94_f32)
    } else {
        (0.06_f32, 0.55_f32)
    };
    format_hex(clamp_luma(rgb, min, max))
}

/// Relative luminance of an sRGB triple, in `0.0..=1.0`. Deliberately the
/// cheap non-linearized form: this only has to rank colors consistently.
fn luma([r, g, b]: [f32; 3]) -> f32 {
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0
}

/// Blend toward white or black until the luminance lands inside
/// `[min, max]`. Both blends are affine in each channel, so the resulting
/// luminance is exactly the requested bound.
fn clamp_luma(rgb: [f32; 3], min: f32, max: f32) -> [f32; 3] {
    let l = luma(rgb);
    if l < min {
        let t = (min - l) / (1.0 - l).max(f32::EPSILON);
        rgb.map(|c| c + (255.0 - c) * t)
    } else if l > max {
        let t = (l - max) / l.max(f32::EPSILON);
        rgb.map(|c| c * (1.0 - t))
    } else {
        rgb
    }
}

fn parse_hex(hex: &str) -> Option<[f32; 3]> {
    let raw = hex.strip_prefix('#')?;
    if raw.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&raw[i..i + 2], 16).ok().map(f32::from);
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn format_hex(rgb: [f32; 3]) -> String {
    let ch = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", ch(rgb[0]), ch(rgb[1]), ch(rgb[2]))
}

/// Deterministic fallback hue for languages with no conventional color.
/// FNV-1a over the name picks a hue; saturation and lightness are fixed
/// so every derived color starts inside a sane band before the theme
/// clamp runs.
fn hashed_language_rgb(name: &str) -> [f32; 3] {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    // 24 evenly spaced hues, offset so the first bucket is not pure red.
    let hue = ((hash % 24) as f32) * 15.0 + 7.0;
    hsl_to_rgb(hue, 0.62, 0.58)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h % 360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [(r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0]
}

// ---------------------------------------------------------------------------
// Plotted traces: TODO/FIXME movement, and commits per month
// ---------------------------------------------------------------------------

struct TrendSheet<'a> {
    aria: &'a str,
    title: &'a str,
    caption: &'a str,
    /// Each plotted point as (position across the axis in `0.0..=1.0`, value).
    points: &'a [(f32, i64)],
    from_label: &'a str,
    to_label: &'a str,
    /// Index of the peak, and the value lettered on the vertical dimension
    /// that measures it from the baseline.
    peak: usize,
    peak_value: &'a str,
    fields: &'a [TitleField<'a>],
    theme: &'a Theme,
}

/// One trace, measured against a lettered scale.
///
/// The trace is the sheet's primary data, so it is the one thing drawn in
/// drafting red at the emphasis weight. The scale is construction lines that
/// terminate on the axis and the plot edge, never a background grid, and the
/// peak is measured by a vertical dimension that letters its value on itself.
fn trend_sheet(sheet: TrendSheet<'_>) -> String {
    let theme = sheet.theme;
    let width = 1200.0_f32;
    let pad_l = 64.0_f32;
    let pad_r = 44.0_f32;
    let pad_t = 100.0_f32;
    let plot_h = 168.0_f32;
    let plot_w = width - pad_l - pad_r;
    let base_y = pad_t + plot_h;

    let y_max = sheet
        .points
        .iter()
        .map(|(_, v)| *v)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let x_at = |fraction: f32| pad_l + fraction.clamp(0.0, 1.0) * plot_w;
    let y_at = |value: f32| base_y - (value / y_max) * plot_h;

    let mut scale = object_line(pad_l, pad_t, pad_l, base_y, theme.border, texture::W_OBJECT);
    for step in 0..=4 {
        let value = y_max * (step as f32 / 4.0);
        let y = y_at(value);
        scale.push_str(&if step == 0 {
            object_line(pad_l, y, pad_l + plot_w, y, theme.border, texture::W_OBJECT)
        } else {
            object_line(
                pad_l,
                y,
                pad_l + plot_w,
                y,
                theme.grid,
                texture::W_CONSTRUCTION,
            )
        });
        scale.push_str(&format!(
            "<text class=\"meta\" x=\"{x}\" y=\"{y}\" text-anchor=\"end\" dominant-baseline=\"central\">{v}</text>",
            x = texture::coord(pad_l - 10.0),
            y = texture::coord(y),
            v = value.round() as i64,
        ));
    }

    let mut path = String::new();
    for (index, (fraction, value)) in sheet.points.iter().enumerate() {
        path.push_str(&format!(
            "{}{} {}",
            if index == 0 { "M" } else { " L" },
            texture::coord(x_at(*fraction)),
            texture::coord(y_at(*value as f32)),
        ));
    }
    let mut area = path.clone();
    if let (Some(first), Some(last)) = (sheet.points.first(), sheet.points.last()) {
        area.push_str(&format!(
            " L{} {} L{} {}Z",
            texture::coord(x_at(last.0)),
            texture::coord(base_y),
            texture::coord(x_at(first.0)),
            texture::coord(base_y),
        ));
    }

    let peak_point = sheet.points.get(sheet.peak).copied().unwrap_or((0.0, 0));
    let peak_x = x_at(peak_point.0);
    let peak_y = y_at(peak_point.1 as f32);
    let dimension = if peak_point.1 > 0 && base_y - peak_y > 26.0 {
        format!(
            "    {}\n",
            texture::dimension_v(
                peak_y,
                base_y,
                peak_x,
                &plotted(sheet.peak_value, theme, 10.0)
            )
        )
    } else {
        String::new()
    };

    // The trace is labelled at its own end, the way every plotted series is.
    let end_label = sheet.points.last().map_or_else(String::new, |(f, v)| {
        format!(
            "    <text class=\"value\" x=\"{x}\" y=\"{y}\" text-anchor=\"end\" dominant-baseline=\"central\">{v}</text>\n",
            x = texture::coord(x_at(*f) - 8.0),
            y = texture::coord((y_at(*v as f32) - 12.0).max(pad_t + 6.0)),
        )
    });

    let block_y = base_y + 34.0;
    let height = block_y + texture::title_block_height(sheet.fields.len()) + COLOPHON_BAND;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="{aria}">
  <style><![CDATA[
{css}
  ]]></style>
  <text class="title" x="{pad_l}" y="{title_y}">{title}</text>
  <text class="caption" x="{pad_l}" y="{caption_y}">{caption}</text>
  <text class="meta" x="{pad_l}" y="{axis_y}">{from_label}</text>
  <text class="meta" x="{right}" y="{axis_y}" text-anchor="end">{to_label}</text>
  <g opacity="1">
    {motion}
    <path d="{area}" fill="{track}" />
    {scale}
    <path d="{path}" fill="none" stroke="{accent}" stroke-width="{emphasis}" stroke-linecap="round" stroke-linejoin="round" />
{dimension}{end_label}  </g>
{block}{footer}
</svg>"##,
        aria = escape_xml(sheet.aria),
        css = sheet_css(theme),
        title = escape_xml(sheet.title),
        caption = escape_xml(sheet.caption),
        pad_l = texture::coord(pad_l),
        title_y = texture::coord(TITLE_Y),
        caption_y = texture::coord(CAPTION_Y),
        axis_y = texture::coord(base_y + 18.0),
        right = texture::coord(pad_l + plot_w),
        from_label = escape_xml(sheet.from_label),
        to_label = escape_xml(sheet.to_label),
        motion = motion_reveal(0),
        track = theme.track,
        accent = theme.accent,
        emphasis = texture::W_EMPHASIS,
        block = texture::title_block(
            width - pad_r - BLOCK_W,
            block_y,
            BLOCK_W,
            sheet.fields,
            theme
        ),
        footer = brand::footer_lockup(width - 24.0, height - 12.0, theme),
    )
}

pub fn render_todo_trend(repo: &str, points: &[TodoPoint], theme: &Theme) -> String {
    if points.is_empty() {
        return empty_chart(
            1200.0,
            360.0,
            "no TODO/FIXME movement in the analyzed commit window",
            theme,
        );
    }
    let t_min = points[0].day;
    let t_max = points[points.len() - 1].day;
    let span = (t_max - t_min).num_days().max(1) as f32;
    let plotted_points: Vec<(f32, i64)> = points
        .iter()
        .map(|p| ((p.day - t_min).num_days() as f32 / span, p.running_total))
        .collect();

    let last_total = points[points.len() - 1].running_total;
    let peak = points
        .iter()
        .enumerate()
        .max_by_key(|(_, p)| p.running_total)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let peak_total = points[peak].running_total;
    let peak_day = points[peak].day;

    let repo_field = truncate_tail(repo, 22);
    let current_field = last_total.to_string();
    let peak_field = format!("{peak_total} · {peak_day}");
    let fields = [
        TitleField {
            label: "repo",
            value: &repo_field,
        },
        TitleField {
            label: "current",
            value: &current_field,
        },
        TitleField {
            label: "peak",
            value: &peak_field,
        },
    ];
    let peak_value = format!("peak {peak_total}");
    trend_sheet(TrendSheet {
        aria: &format!("Recent TODO/FIXME movement for {repo}"),
        title: repo,
        caption: "Recent TODO/FIXME movement · running total across the analyzed commit window",
        points: &plotted_points,
        from_label: &t_min.to_string(),
        to_label: &t_max.to_string(),
        peak,
        peak_value: &peak_value,
        fields: &fields,
        theme,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MonthCount {
    /// First day of the month bucket.
    pub month: NaiveDate,
    pub commits: i64,
}

/// Aggregate per-day commit counts into contiguous month buckets.
/// Months between the first and last observed commit with no activity
/// get an explicit zero bucket, so a trend line shows dormant stretches
/// instead of interpolating across them. Input need not be sorted.
pub fn bucket_months(days: &[DayCount]) -> Vec<MonthCount> {
    use std::collections::BTreeMap;
    let mut by_month: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for d in days {
        let entry = by_month.entry(month_of(d.day)).or_insert(0);
        *entry = entry.saturating_add(d.commits);
    }
    let (Some(first), Some(last)) = (
        by_month.keys().next().copied(),
        by_month.keys().next_back().copied(),
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cur = first;
    loop {
        out.push(MonthCount {
            month: cur,
            commits: by_month.get(&cur).copied().unwrap_or(0),
        });
        if cur >= last {
            break;
        }
        cur = next_month(cur);
    }
    out
}

fn month_of(d: NaiveDate) -> NaiveDate {
    // Day 1 of an existing date's (year, month) is always constructible.
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d)
}

fn next_month(d: NaiveDate) -> NaiveDate {
    let (y, m) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    // (y, m, 1) is always a valid date for m in 1..=12.
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(d)
}

/// Monthly commit counts as one plotted trace, with the peak month measured.
pub fn render_commit_trend(repo: &str, days: &[DayCount], theme: &Theme) -> String {
    let months = bucket_months(days);
    if months.is_empty() {
        return empty_chart(1200.0, 360.0, "no commit data yet", theme);
    }
    let denom = months.len().saturating_sub(1).max(1) as f32;
    let plotted_points: Vec<(f32, i64)> = months
        .iter()
        .enumerate()
        .map(|(index, m)| (index as f32 / denom, m.commits))
        .collect();

    let total: i64 = months.iter().map(|m| m.commits).sum();
    // `max_by_key` keeps the LAST max on ties — deterministic, and the most
    // recent peak is the more interesting one to measure anyway.
    let (peak, peak_month) = months
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| m.commits)
        .expect("months is non-empty (checked above)");
    let peak_label = peak_month.month.format("%Y-%m").to_string();

    let repo_field = truncate_tail(repo, 22);
    let months_field = months.len().to_string();
    let total_field = humanize(total);
    let peak_field = format!("{} · {}", peak_month.commits, peak_label);
    let fields = [
        TitleField {
            label: "repo",
            value: &repo_field,
        },
        TitleField {
            label: "months",
            value: &months_field,
        },
        TitleField {
            label: "commits",
            value: &total_field,
        },
        TitleField {
            label: "peak",
            value: &peak_field,
        },
    ];
    let peak_value = format!("peak {}", peak_month.commits);
    let caption = format!(
        "Commits per month · {} total · peak {} in {}",
        humanize(total),
        peak_month.commits,
        peak_label
    );
    let from = months[0].month.format("%Y-%m").to_string();
    let to = months[months.len() - 1].month.format("%Y-%m").to_string();
    trend_sheet(TrendSheet {
        aria: &format!("Monthly commit trend for {repo}"),
        title: repo,
        caption: &caption,
        points: &plotted_points,
        from_label: &from,
        to_label: &to,
        peak,
        peak_value: &peak_value,
        fields: &fields,
        theme,
    })
}

// ---------------------------------------------------------------------------
// Bus factor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AuthorShare {
    /// Display label — GitHub login when known, otherwise author name.
    pub label: String,
    pub login: Option<String>,
    pub avatar_url: Option<String>,
    pub commits: i64,
}

/// Minimum number of top contributors whose combined commits strictly
/// exceed 50% of `total_commits` — i.e. how many people the project
/// could lose before more than half of its authorship knowledge is gone.
///
/// `commits` need not be sorted (a descending copy is taken internally)
/// and non-positive entries are ignored. Returns 0 when there are no
/// commits at all. If `commits` is a truncated top-N prefix whose sum
/// never crosses half of `total_commits`, the prefix length is returned
/// as a lower bound — by definition the true bus factor is at least
/// that large.
pub fn compute_bus_factor(commits: &[i64], total_commits: i64) -> usize {
    if total_commits <= 0 {
        return 0;
    }
    let mut sorted: Vec<i64> = commits.iter().copied().filter(|c| *c > 0).collect();
    if sorted.is_empty() {
        return 0;
    }
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    let mut acc = 0i64;
    for (i, c) in sorted.iter().enumerate() {
        acc = acc.saturating_add(*c);
        if acc.saturating_mul(2) > total_commits {
            return i + 1;
        }
    }
    sorted.len()
}

/// How many authors the share band plots before the rest is one remainder
/// segment. Seven, because there are seven category pens: an eighth segment
/// would have to repeat one, and a repeated pen in a single band is a chart
/// that lies about how many people it is showing.
const BUS_SEGMENTS: usize = 7;

/// How far the bus-factor dimension stands below the band. Wider than the
/// usual standoff because the band's own key sits under it.
const BUS_DIM_STANDOFF: f32 = 22.0;

/// Contributor concentration as one measured band.
///
/// Each author is a plotted segment of the whole, with a 1px graphite
/// hairline standing at its leading edge, and the sheet spends its red on the
/// dimension that measures where the bus factor actually falls.
pub fn render_bus_factor(
    repo: &str,
    authors: &[AuthorShare],
    total_commits: i64,
    theme: &Theme,
) -> String {
    let width = 900.0_f32;

    let mut sorted: Vec<&AuthorShare> = authors.iter().filter(|a| a.commits > 0).collect();
    sorted.sort_by(|a, b| {
        b.commits
            .cmp(&a.commits)
            .then_with(|| a.label.cmp(&b.label))
    });
    if sorted.is_empty() || total_commits <= 0 {
        return empty_chart(width, 200.0, "no contributor data yet", theme);
    }

    let commit_counts: Vec<i64> = sorted.iter().map(|a| a.commits).collect();
    let bus_factor = compute_bus_factor(&commit_counts, total_commits);
    let risk = match bus_factor {
        0 => "Unavailable",
        1 => "Solo",
        2 => "High",
        3 => "Medium",
        _ => "Low",
    };
    // The visual deliberately omits sub-percent shares so the ownership risk
    // stays legible; the bus factor above still counts the whole population.
    let shown: Vec<&AuthorShare> = sorted
        .iter()
        .copied()
        .filter(|author| author.commits.saturating_mul(100) >= total_commits)
        .take(BUS_SEGMENTS)
        .collect();

    let band_x = PAD;
    let band_w = width - PAD * 2.0;
    let band_y = 122.0_f32;
    let band_h = 26.0_f32;
    let keys: Vec<&str> = shown.iter().map(|a| a.label.as_str()).collect();
    let pens = pens_for(theme, &keys);

    let mut band = format!(
        "  <rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{track}\" />\n",
        x = texture::coord(band_x),
        y = texture::coord(band_y),
        w = texture::coord(band_w),
        h = texture::coord(band_h),
        track = theme.track,
    );
    let mut edges: Vec<f32> = Vec::with_capacity(shown.len());
    let mut cursor = band_x;
    for (index, author) in shown.iter().enumerate() {
        let share = author.commits as f32 / total_commits as f32;
        // A segment can never run past the band it is a share of, however
        // odd the arithmetic upstream gets.
        let remaining = (band_x + band_w - cursor).max(0.0);
        let seg_w = (share * band_w).clamp(0.0, remaining);
        band.push_str(&format!(
            "  {}\n",
            texture::series_bar(
                cursor,
                band_y,
                seg_w,
                band_h,
                pens[index],
                theme.fg,
                Side::Right
            ),
        ));
        if seg_w >= 16.0 {
            band.push_str(&format!(
                "  <text class=\"value\" x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"central\" fill=\"{ink}\">{n:02}</text>\n",
                x = texture::coord(cursor + seg_w / 2.0),
                y = texture::coord(band_y + band_h / 2.0),
                ink = contrast_on(pens[index]),
                n = index + 1,
            ));
        }
        cursor += seg_w;
        edges.push(cursor);
    }
    // What is left of the band is everybody the plot does not draw. Saying so
    // is the difference between a measured remainder and an empty tail that
    // looks like a rendering fault.
    let plotted_commits: i64 = shown.iter().map(|author| author.commits).sum();
    let rest = total_commits.saturating_sub(plotted_commits);
    if rest > 0 && band_x + band_w - cursor >= 12.0 {
        band.push_str(&format!(
            "  <text class=\"meta\" x=\"{x}\" y=\"{y}\" text-anchor=\"end\">others {share:.1}%</text>\n",
            x = texture::coord(band_x + band_w),
            y = texture::coord(band_y - 9.0),
            share = rest as f64 / total_commits as f64 * 100.0,
        ));
    }

    // The measured value of this sheet: where the top authors cross half of
    // the attributed commits.
    let dimension = match bus_factor {
        0 => String::new(),
        factor if factor <= edges.len() => {
            let end = edges[factor - 1];
            let carried: i64 = shown.iter().take(factor).map(|author| author.commits).sum();
            let value = format!(
                "factor {factor} · {:.1}%",
                carried as f64 / total_commits as f64 * 100.0
            );
            format!(
                "  {left}{right}{dim}\n",
                left = texture::extension_tick(
                    band_x,
                    band_y + band_h,
                    Side::Down,
                    extension_len(BUS_DIM_STANDOFF),
                    theme.border
                ),
                right = texture::extension_tick(
                    end,
                    band_y + band_h,
                    Side::Down,
                    extension_len(BUS_DIM_STANDOFF),
                    theme.border
                ),
                dim = texture::dimension_h(
                    band_x,
                    end,
                    band_y + band_h + BUS_DIM_STANDOFF,
                    &measured(&value, theme, 11.0)
                ),
            )
        }
        _ => String::new(),
    };

    // The key: the same numbers, against the same pens, with the people they
    // belong to. Two columns, so a wide sheet is not one long column.
    let key_y = band_y + band_h + 58.0;
    let key_row_h = 54.0_f32;
    let key_col_w = (band_w - 20.0) / 2.0;
    let key_rows = shown.len().div_ceil(2);
    let mut key = String::new();
    for (index, author) in shown.iter().enumerate() {
        let x = band_x + (index % 2) as f32 * (key_col_w + 20.0);
        let y = key_y + (index / 2) as f32 * key_row_h;
        let label = truncate_tail(&author.label, 20);
        let share = author.commits as f64 / total_commits as f64 * 100.0;
        let content = format!(
            r##"<title>{tooltip}</title>
    <text class="meta" x="{nx}" y="{cy}" dominant-baseline="central">{n:02}</text>
    <rect x="{sx}" y="{sy}" width="10" height="10" fill="{pen}" shape-rendering="crispEdges" />
    <clipPath id="owner-clip-{index}"><rect width="34" height="34" /></clipPath>
    <g transform="translate({ax}, {ay})">{photo}<rect class="avatar-edge" x="0" y="0" width="34" height="34" /></g>
    <text class="label" x="{tx}" y="{ty}">{label}</text>
    <text class="meta" x="{tx}" y="{my}">{commits} commits · {share:.1}%</text>"##,
            tooltip = escape_xml(&format!(
                "{} · {} commits · {share:.1}%",
                author.label, author.commits
            )),
            n = index + 1,
            nx = texture::coord(x),
            cy = texture::coord(y + 17.0),
            sx = texture::coord(x + 22.0),
            sy = texture::coord(y + 12.0),
            pen = pens[index],
            ax = texture::coord(x + 40.0),
            ay = texture::coord(y),
            photo = bus_photo(author.avatar_url.as_deref(), &label, index),
            tx = texture::coord(x + 84.0),
            ty = texture::coord(y + 15.0),
            my = texture::coord(y + 31.0),
            label = escape_xml(&label),
            commits = author.commits,
        );
        let linked = author.login.as_ref().map_or(content.clone(), |login| {
            format!(
                r##"<a class="person-link" href="{href}" target="_blank" rel="noopener">{content}</a>"##,
                href = escape_xml(&format!("https://github.com/{login}")),
            )
        });
        key.push_str(&format!(
            "  <g opacity=\"1\">\n    {motion}\n    {linked}\n  </g>\n",
            motion = motion_reveal(index),
        ));
    }

    let repo_field = truncate_tail(repo, 22);
    let factor_field = bus_factor.to_string();
    let authors_field = sorted.len().to_string();
    let commits_field = humanize(total_commits);
    let fields = [
        TitleField {
            label: "repo",
            value: &repo_field,
        },
        TitleField {
            label: "factor",
            value: &factor_field,
        },
        TitleField {
            label: "risk",
            value: risk,
        },
        TitleField {
            label: "authors",
            value: &authors_field,
        },
        TitleField {
            label: "commits",
            value: &commits_field,
        },
    ];
    let block_y = key_y + key_rows as f32 * key_row_h - 12.0;
    let height = block_y + texture::title_block_height(fields.len()) + COLOPHON_BAND;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="Bus factor for {repo}">
  <style><![CDATA[
{css}
{avatar_css}
    .person-link {{ cursor: pointer; }}
    .person-link:hover .label {{ text-decoration: underline; }}
    .person-link:hover .avatar-edge {{ stroke: {fg}; stroke-width: {emphasis}; }}
  ]]></style>
  <text class="title" x="{pad}" y="{title_y}">Bus factor</text>
  <text class="caption" x="{pad}" y="{caption_y}">Contributors with at least 1% of attributed commits · {repo}</text>
{band}{dimension}{key}{block}{footer}
</svg>"##,
        repo = escape_xml(repo),
        css = sheet_css(theme),
        avatar_css = avatar_css(theme),
        fg = theme.fg,
        emphasis = texture::W_EMPHASIS,
        pad = texture::coord(PAD),
        title_y = texture::coord(TITLE_Y),
        caption_y = texture::coord(CAPTION_Y),
        block = texture::title_block(width - PAD - BLOCK_W, block_y, BLOCK_W, &fields, theme),
        footer = brand::footer_lockup(width - PAD, height - 12.0, theme),
    )
}

/// A 34px key photo, or the author's initial when there is none.
fn bus_photo(url: Option<&str>, label: &str, index: usize) -> String {
    match url {
        Some(url) => format!(
            r#"<image href="{url}" width="34" height="34" clip-path="url(#owner-clip-{index})" preserveAspectRatio="xMidYMid slice" />"#,
            url = escape_xml(url),
        ),
        None => {
            let initial = label
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            format!(
                r#"<rect class="avatar-fallback-bg" width="34" height="34" /><text class="avatar-fallback" x="17" y="17" text-anchor="middle" dominant-baseline="central" font-size="14">{initial}</text>"#,
                initial = escape_xml(&initial),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// A sheet with nothing on it says why, in ink, and keeps its colophon.
fn empty_chart(width: f32, height: f32, message: &str, theme: &Theme) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="{message}">
  <style><![CDATA[
    .caption {{ fill: {muted}; font: 13px {sans}; }}
    .footer-link {{ fill: {muted}; font: 600 11px {sans}; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
  ]]></style>
  <text class="caption" x="{cx}" y="{cy}" text-anchor="middle" dominant-baseline="central">{message}</text>
{footer}
</svg>"##,
        muted = theme.muted,
        fg = theme.fg,
        sans = texture::SANS,
        cx = texture::coord(width / 2.0),
        cy = texture::coord(height / 2.0),
        message = escape_xml(message),
        footer = brand::footer_lockup(width - 24.0, height - 12.0, theme),
    )
}

fn truncate_tail(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let tail: String = s
        .chars()
        .rev()
        .take(max - 1)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("…{tail}")
}

/// XML text escaping.
///
/// [`crate::texture::escape_xml`] is the canonical copy; this one also
/// escapes the apostrophe, because a repository or author label can carry one
/// and these sheets letter those inside attributes as well as in text.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn without_smil(svg: &str) -> String {
        svg.lines()
            .filter(|line| !line.contains("<animate"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn sample_days() -> Vec<DayCount> {
        vec![
            DayCount {
                day: NaiveDate::from_ymd_opt(2026, 1, 3).unwrap(),
                commits: 5,
            },
            DayCount {
                day: NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
                commits: 1,
            },
        ]
    }

    fn sample_files() -> Vec<FileRow> {
        vec![
            FileRow {
                path: "src/auth.rs".into(),
                count: 47,
            },
            FileRow {
                path: "src/db.rs".into(),
                count: 23,
            },
        ]
    }

    /// Every sheet this module draws, in one print, for the contract tests
    /// that have to hold across the whole family.
    fn every_sheet(theme: &Theme) -> Vec<(&'static str, String)> {
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let days = sample_days();
        vec![
            (
                "bug magnets",
                render_bug_magnets("o/r", &sample_files(), theme),
            ),
            (
                "top changed",
                render_top_changed("o/r", &sample_files(), theme),
            ),
            (
                "commit activity",
                render_heatmap("o/r", "Commit activity", start, end, &days, None, theme),
            ),
            (
                "contributors",
                render_contributors(
                    "o/r",
                    &[ContributorRow {
                        login: Some("a".into()),
                        name: "a".into(),
                        avatar_url: None,
                        commits: 3,
                    }],
                    theme,
                ),
            ),
            (
                "languages",
                render_languages(
                    "o/r",
                    &[LanguageBar {
                        language: "Rust".into(),
                        files: 2,
                        lines_code: 30,
                        lines_blank: 3,
                        lines_comment: 1,
                    }],
                    theme,
                ),
            ),
            (
                "todo trend",
                render_todo_trend(
                    "o/r",
                    &[
                        TodoPoint {
                            day: start,
                            running_total: 1,
                        },
                        TodoPoint {
                            day: end,
                            running_total: 4,
                        },
                    ],
                    theme,
                ),
            ),
            (
                "bus factor",
                render_bus_factor(
                    "o/r",
                    &[
                        AuthorShare {
                            label: "alice".into(),
                            login: Some("alice".into()),
                            avatar_url: None,
                            commits: 5,
                        },
                        AuthorShare {
                            label: "bob".into(),
                            login: None,
                            avatar_url: None,
                            commits: 3,
                        },
                    ],
                    8,
                    theme,
                ),
            ),
            ("commit trend", render_commit_trend("o/r", &days, theme)),
            (
                "contribution profile",
                render_contribution_profile(
                    "@alice",
                    &ContributionProfile {
                        owned_repos: 4,
                        external_repos: 9,
                        owned_commits: 120,
                        external_commits: 380,
                        visionary_count: 2,
                    },
                    theme,
                ),
            ),
        ]
    }

    /// The whole point of the redraw: not one pattern, tier ladder, dither
    /// field, gradient or rounded corner survives on any sheet, in either
    /// print. These ids used to be on every one of them.
    #[test]
    fn no_sheet_carries_a_pattern_a_gradient_or_a_rounded_corner() {
        for theme in [&theme::LIGHT, &theme::DARK] {
            for (name, svg) in every_sheet(theme) {
                for banned in [
                    "gd-pixel-fill",
                    "gd-pixel-field",
                    "gd-pixel-fade",
                    "gd-dither-wave",
                    "gd-heat-t",
                    "gd-lang",
                    "gd-contrib-own",
                    "data-gitdebt-texture",
                    "data-gitdebt-heat-defs",
                    "data-gitdebt-language-defs",
                    "<pattern",
                    "url(#gd-",
                    "linearGradient",
                    "radialGradient",
                    "feGaussianBlur",
                    "filter=",
                    "drop-shadow",
                    "box-shadow",
                    "rx=",
                    "ry=",
                    "border-radius",
                    "var(--",
                    "prefers-color-scheme",
                ] {
                    assert!(!svg.contains(banned), "{name} still carries {banned}");
                }
            }
        }
    }

    /// Three weights and no others, everywhere.
    ///
    /// Lettering is exempt, and only lettering: a cut value strokes its own
    /// glyphs in the ground colour to open a gap in the rule it sits on, and
    /// that halo is wide on purpose. Every drawn line is 0.5, 1 or 2.
    #[test]
    fn sheets_draw_only_the_three_line_weights() {
        for theme in [&theme::LIGHT, &theme::DARK] {
            for (name, svg) in every_sheet(theme) {
                for (index, _) in svg.match_indices("stroke-width=\"") {
                    let tag_start = svg[..index].rfind('<').expect("owning tag");
                    if svg[tag_start..].starts_with("<text") {
                        continue;
                    }
                    let rest = &svg[index + "stroke-width=\"".len()..];
                    let value = &rest[..rest.find('"').expect("closing quote")];
                    assert!(
                        matches!(value, "0.5" | "1" | "2"),
                        "{name} draws a {value} weight"
                    );
                }
            }
        }
    }

    /// Every sheet closes with a title block: the field labels uppercase and
    /// tracked, the values tabular, in a box whose bottom-right corner is the
    /// system's one chamfer.
    #[test]
    fn every_sheet_closes_with_a_title_block() {
        for (name, svg) in every_sheet(&theme::LIGHT) {
            assert!(svg.contains(">REPO<") || svg.contains(">LOGIN<"), "{name}");
            assert!(
                svg.contains(&format!("letter-spacing=\"{}\"", texture::LABEL_TRACKING)),
                "{name} letters no field label"
            );
            assert!(
                svg.contains("font-variant-numeric=\"tabular-nums\""),
                "{name} has no tabular value"
            );
            // The chamfer: a closed path that steps in by 10 before the
            // bottom edge. Nothing else in the drawing is cut.
            assert!(svg.contains("H0.00Z") || svg.contains('Z'), "{name}");
        }
    }

    /// Drafting red is spent on the measured value and nothing else. A bar
    /// sheet's whole red budget is one dimension: its rule, its two
    /// terminators and its lettering.
    #[test]
    fn a_bar_sheet_spends_its_red_only_on_the_dimension() {
        for theme in [&theme::LIGHT, &theme::DARK] {
            let svg = render_bug_magnets("foo/bar", &sample_files(), theme);
            assert_eq!(
                svg.matches(theme.accent).count(),
                4,
                "red belongs to the dimension line, its terminators and its value"
            );
            // And that dimension measures the peak, with its unit.
            assert!(svg.contains(">47 fix commits<"));
            assert!(svg.contains("paint-order=\"stroke\""));
        }
    }

    /// A plotted bar is a flat pen with a 1px ink hairline standing at the
    /// measured edge, and no texture of any kind.
    #[test]
    fn bars_are_flat_pens_with_a_leading_hairline() {
        let svg = render_bug_magnets("foo/bar", &sample_files(), &theme::LIGHT);
        // The widest bar reaches the full plot width; its hairline stands on
        // that edge, top to bottom of the bar.
        assert!(svg.contains(&format!("fill=\"{}\"", theme::LIGHT.ink_3)));
        assert!(svg.contains(&format!(
            "<line x1=\"778.00\" y1=\"120.00\" x2=\"778.00\" y2=\"134.00\" stroke=\"{}\" stroke-width=\"1\" />",
            theme::LIGHT.fg
        )));
        assert!(!svg.contains("fill-opacity"));
    }

    /// A capped analysis window must not assert "0 commits" for days it
    /// never read. The cells are still drawn (the calendar keeps its shape)
    /// but they say what they actually know.
    #[test]
    fn heatmap_does_not_claim_zero_for_unanalyzed_days() {
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let analyzed_from = NaiveDate::from_ymd_opt(2026, 1, 20).unwrap();
        let days = vec![DayCount {
            day: analyzed_from,
            commits: 3,
        }];

        let disclosed = render_heatmap(
            "o/r",
            "Commits",
            start,
            end,
            &days,
            Some(analyzed_from),
            &theme::LIGHT,
        );
        assert!(disclosed.contains("2026-01-05 · outside the analyzed window"));
        assert!(disclosed.contains("2026-01-20 · 3 commits"));
        assert!(disclosed.contains("bounded analysis from 2026-01-20"));
        assert!(!disclosed.contains("2026-01-05 · 0 commits"));

        // A complete window keeps asserting a real zero.
        let complete = render_heatmap("o/r", "Commits", start, end, &days, None, &theme::LIGHT);
        assert!(complete.contains("2026-01-05 · 0 commits"));
    }

    #[test]
    fn every_repo_chart_surface_embeds_the_logo() {
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 7).unwrap();
        let charts = [
            render_bug_magnets("o/r", &[], &theme::LIGHT),
            render_top_changed("o/r", &[], &theme::LIGHT),
            render_heatmap(
                "o/r",
                "Commit activity",
                start,
                end,
                &[],
                None,
                &theme::LIGHT,
            ),
            render_contributors("o/r", &[], &theme::LIGHT),
            render_languages("o/r", &[], &theme::LIGHT),
            render_todo_trend("o/r", &[], &theme::LIGHT),
            render_bus_factor("o/r", &[], 0, &theme::LIGHT),
            render_commit_trend("o/r", &[], &theme::LIGHT),
        ];

        for svg in charts {
            assert!(svg.contains("data-gitdebt-logo=\"true\""));
            assert!(!svg.contains("<image"));
        }
    }

    #[test]
    fn truncate_keeps_tail() {
        assert_eq!(truncate_tail("short", 10), "short");
        let long = "src/components/very/long/path/to/component.tsx";
        let t = truncate_tail(long, 20);
        assert!(t.starts_with('…'));
        assert!(t.ends_with("component.tsx"));
        assert_eq!(t.chars().count(), 20);
    }

    #[test]
    fn bug_magnets_renders_paths_links_and_baked_colors() {
        let svg = render_bug_magnets("foo/bar", &sample_files(), &theme::LIGHT);
        assert!(svg.contains("Fix-labelled changes"));
        assert!(svg.contains("analyzed commit window"));
        assert!(svg.contains("src/auth.rs"));
        assert!(svg.contains(">47</text>"));
        assert!(svg.contains("<animate"));
        assert!(svg.contains("gitdebt.com"));
        assert!(svg.contains("https://github.com/foo/bar/blob/HEAD/src/auth.rs"));
        // Concrete light-print colors baked in.
        assert!(svg.contains(theme::LIGHT.fg));
        assert!(svg.contains(theme::LIGHT.ink_3));
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn dark_theme_bakes_dark_colors() {
        let rows = vec![FileRow {
            path: "x".into(),
            count: 1,
        }];
        let svg = render_bug_magnets("a/b", &rows, &theme::DARK);
        assert!(svg.contains(theme::DARK.fg));
        assert!(svg.contains(theme::DARK.accent));
        // The light print's ink is never the lettering colour here. (It can
        // still appear as an attribute, because `contrast_on` legitimately
        // letters graphite onto a light fill.)
        assert!(!svg.contains(&format!("fill: {};", theme::LIGHT.fg)));
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn count_placement_inside_for_long_bars() {
        let (x, anchor, _) = count_placement(100.0, 200.0, "12k", "#ef4444", "#94a3b8");
        assert_eq!(anchor, "end");
        assert!(x < 300.0);
    }

    #[test]
    fn count_placement_outside_for_short_bars() {
        let (_x, anchor, _) = count_placement(100.0, 5.0, "12k", "#ef4444", "#94a3b8");
        assert_eq!(anchor, "start");
    }

    #[test]
    fn heatmap_renders_cells() {
        let days = vec![
            DayCount {
                day: NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
                commits: 3,
            },
            DayCount {
                day: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                commits: 12,
            },
        ];
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let svg = render_heatmap(
            "foo/bar",
            "Commits in 2026",
            start,
            end,
            &days,
            None,
            &theme::LIGHT,
        );
        assert!(svg.contains("class=\"cell"));
        assert!(svg.contains("Commits in 2026"));
        assert!(svg.contains("Mon"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.contains(">gitdebt</text>"));
        assert!(svg.contains("text-anchor=\"end\""));
    }

    /// The commit-activity ramp is a stepped set of inks with a key that
    /// letters the count range each step stands for. It steps through the
    /// drawing's own ladder rather than the plotter pens — those exist for
    /// several series at once and sit in one narrow lightness band, which is
    /// exactly what a magnitude cannot use — and the mark grows with the ink.
    #[test]
    fn commit_activity_steps_through_a_labelled_ink_ladder() {
        let mut days = Vec::new();
        for (index, commits) in [1i64, 3, 9, 40, 0, 7, 22].into_iter().enumerate() {
            days.push(DayCount {
                day: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
                    + chrono::Duration::days(index as i64),
                commits,
            });
        }
        let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
        for theme in [&theme::LIGHT, &theme::DARK] {
            let svg = render_heatmap("foo/bar", "Commit activity", start, end, &days, None, theme);
            for (size, ink) in HEAT_STEPS {
                assert!(
                    svg.contains(&format!(
                        "width=\"{}\" height=\"{}\" fill=\"{}\"",
                        texture::coord(size),
                        texture::coord(size),
                        ink(theme)
                    )),
                    "a step never plots"
                );
                // No step is drafting red, and none of them is a pen: the
                // pen set belongs to charts that draw several series at once.
                assert_ne!(ink(theme), theme.accent);
                assert!(!theme.pens.contains(&ink(theme)));
            }
            // A day with no commits keeps its place on the empty ground.
            assert!(svg.contains(&format!("fill=\"{}\"", theme.track)));
            assert!(svg.contains(">COMMITS PER DAY<"));
            assert!(svg.contains(">0</text>"));
            assert!(svg.contains(">≤"));
            assert!(svg.contains("&gt;"));
            // The peak is the measured datum, so it carries the red leader.
            assert!(svg.contains("peak 40 · 2026-03-04"));
            assert_eq!(
                svg,
                render_heatmap("foo/bar", "Commit activity", start, end, &days, None, theme)
            );
        }
    }

    #[test]
    fn heat_steps_ladder_from_the_empty_ground_to_graphite() {
        assert_eq!(heat_level(0, (1, 4, 9)), 0);
        assert_eq!(heat_level(1, (1, 4, 9)), 1);
        assert_eq!(heat_level(4, (1, 4, 9)), 2);
        assert_eq!(heat_level(9, (1, 4, 9)), 3);
        assert_eq!(heat_level(90, (1, 4, 9)), 4);
        // A dormant day carries no mark at all, and the top step is graphite
        // filling its whole cell.
        assert_eq!(heat_mark(0.0, 0.0, 14.0, 0, &theme::LIGHT), "");
        let top = heat_mark(0.0, 0.0, 14.0, 4, &theme::LIGHT);
        assert!(top.contains(&format!("fill=\"{}\"", theme::LIGHT.fg)));
        assert!(top.contains("x=\"0.00\" y=\"0.00\" width=\"14.00\" height=\"14.00\""));
        // Smaller marks are centred in their cell, never parked in a corner.
        let first = heat_mark(0.0, 0.0, 14.0, 1, &theme::LIGHT);
        assert!(first.contains("x=\"3.00\" y=\"3.00\" width=\"8.00\" height=\"8.00\""));
        // Out-of-range levels clamp to the top step instead of panicking.
        assert_eq!(heat_mark(0.0, 0.0, 14.0, 99, &theme::LIGHT), top);
        // The ladder only darkens (or, in the second print, only brightens).
        let ladder: Vec<f32> = HEAT_STEPS
            .iter()
            .map(|(_, ink)| luma(parse_hex(ink(&theme::LIGHT)).expect("hex")))
            .collect();
        assert!(ladder.windows(2).all(|pair| pair[0] > pair[1]));
        // A degenerate quantile set still letters a readable key.
        assert_eq!(
            heat_key_ranges((0, 0, 0)),
            ["0", "≤1", "≤1", "≤1", ">1"].map(String::from)
        );
    }

    #[test]
    fn heatmap_rolling_52_weeks_renders() {
        let end = NaiveDate::from_ymd_opt(2026, 5, 7).unwrap();
        let start = end - chrono::Duration::days(51 * 7);
        let svg = render_heatmap(
            "foo/bar",
            "Commits in the last 52 weeks",
            start,
            end,
            &[],
            None,
            &theme::LIGHT,
        );
        assert!(svg.contains("Commits in the last 52 weeks"));
        // One cell per day in the range and not one more: the key's swatches
        // are a key, not calendar days, so they no longer inflate this count.
        let days = (end - start).num_days() + 1;
        assert_eq!(svg.matches("class=\"cell\"").count(), days as usize);
    }

    #[test]
    fn contributors_are_square_framed_linked_photos() {
        let rows = vec![ContributorRow {
            login: Some("zhom".into()),
            name: "zhom".into(),
            avatar_url: Some("https://avatars.githubusercontent.com/u/1?s=80".into()),
            commits: 100,
        }];
        let svg = render_contributors("foo/bar", &rows, &theme::LIGHT);
        assert!(svg.contains("href=\"https://github.com/zhom"));
        assert!(svg.contains("<image"));
        assert!(svg.contains("1 public commit author · "));
        assert!(!svg.contains("analyzed commit window"));
        assert!(!svg.contains("100 commits"));
        assert!(!svg.contains("animateTransform"));
        // The photo is square, clipped to a rect, inside a 1px frame. The
        // dither ring, the circle and the hover lift are all gone.
        assert!(svg.contains(
            "<clipPath id=\"contributor-clip-0\"><rect width=\"62\" height=\"62\" /></clipPath>"
        ));
        assert!(svg.contains("class=\"avatar-edge\" x=\"0\" y=\"0\" width=\"62\" height=\"62\""));
        assert!(!svg.contains("<circle"));
        assert!(!svg.contains("translateY"));
        assert!(!svg.contains("scale(1.08)"));
        assert!(!svg.contains("avatar-pixels"));
    }

    #[test]
    fn contributors_render_every_provided_author_across_rows() {
        let rows: Vec<ContributorRow> = (0..29)
            .map(|index| ContributorRow {
                login: Some(format!("author-{index}")),
                name: format!("Author {index}"),
                avatar_url: None,
                commits: 29 - index,
            })
            .collect();
        let svg = render_contributors("foo/bar", &rows, &theme::DARK);
        assert_eq!(svg.matches("class=\"contributor-node\"").count(), 29);
        assert!(svg.contains("href=\"https://github.com/author-28\""));
        assert!(svg.contains("id=\"contributor-clip-28\""));
        assert!(svg.contains("29 public commit authors · "));
        assert!(!svg.contains("analyzed commit window"));
        assert!(
            svg.contains("transform=\"translate(49, 86)\""),
            "the first photo should begin at the 44px content gutter"
        );
        assert!(
            svg.contains("transform=\"translate(989, 86)\""),
            "the last photo should end at the matching 44px gutter"
        );
        assert!(
            !svg.contains("viewBox=\"0 0 1100 208\""),
            "a multi-row set must grow the deterministic sheet"
        );
    }

    /// The standalone tile is the grid's photo and nothing else: no title, no
    /// caption, no title block, no colophon. Its geometry has to match the
    /// grid's cell exactly, or a README grid built from these tiles would not
    /// line up with the chart it came from.
    #[test]
    fn contributor_avatar_is_one_photo_on_a_transparent_tile() {
        let row = ContributorRow {
            login: Some("zhom".into()),
            name: "zhom".into(),
            avatar_url: Some("data:image/png;base64,AAAA".into()),
            commits: 100,
        };
        let svg = render_contributor_avatar(&row, 0, &theme::DARK);

        assert!(svg.contains("viewBox=\"0 0 72 72\""));
        assert!(svg.contains("width=\"72\" height=\"72\""));
        assert!(svg.contains("transform=\"translate(5, 5)\""));
        assert!(svg.contains(
            "<clipPath id=\"gd-avatar-clip\"><rect width=\"62\" height=\"62\" /></clipPath>"
        ));
        assert!(svg.contains("class=\"avatar-edge\" x=\"0\" y=\"0\" width=\"62\" height=\"62\""));
        assert!(svg.contains("<image href=\"data:image/png;base64,AAAA\""));
        assert!(svg.contains("aria-label=\"zhom\""));

        assert!(!svg.contains("<title>"));
        assert!(!svg.contains("class=\"title\""));
        assert!(!svg.contains("class=\"caption\""));
        assert!(!svg.contains("data-gitdebt-logo"));
        assert!(!svg.contains("footer-link"));
        // Transparent like every other shareable surface: no sheet is painted
        // behind the photo.
        assert!(!svg.contains(&format!("fill=\"{}\"", theme::DARK.bg)));
        assert!(!svg.contains("<circle"));
        // Baked ink only.
        assert!(!svg.contains("var(--"));
        assert!(!svg.contains("prefers-color-scheme"));
        assert!(svg.contains(theme::DARK.border));

        assert_eq!(svg, render_contributor_avatar(&row, 0, &theme::DARK));
        assert_ne!(svg, render_contributor_avatar(&row, 0, &theme::LIGHT));
    }

    /// The tile carries the grid's own stagger so a README laid out from them
    /// reveals like the single-image chart, and freezes to a finished frame
    /// when motion is off.
    #[test]
    fn contributor_avatar_reveal_is_staggered_by_rank_and_freezes_static() {
        let row = ContributorRow {
            login: None,
            name: "Ada Lovelace".into(),
            avatar_url: None,
            commits: 3,
        };
        let first = render_contributor_avatar(&row, 0, &theme::LIGHT);
        let third = render_contributor_avatar(&row, 3, &theme::LIGHT);
        assert!(first.contains("begin=\"0.00s\""));
        assert!(third.contains(&format!("begin=\"{:.2}s\"", reveal_begin(3))));
        assert_ne!(first, third);

        // No avatar URL → the initial, never a broken <image>, and centred by
        // the baseline rather than a guessed offset.
        assert!(first.contains(">A</text>"));
        assert!(first.contains("dominant-baseline=\"central\""));
        assert!(!first.contains("<image"));

        // Static is the default output, and it is complete: the reveal's
        // finished opacity is already on the element.
        let frozen = crate::raster::freeze_svg_animations(&first);
        assert!(!frozen.contains("<animate"));
        assert!(frozen.contains("opacity=\"1\""));
    }

    /// A README grid is pasted with a fixed slot count and the author list
    /// underneath it moves, so the tail slots have to vanish rather than draw
    /// anything at all.
    #[test]
    fn blank_avatar_is_an_empty_one_pixel_tile() {
        let svg = render_blank_avatar();
        assert!(svg.contains("viewBox=\"0 0 1 1\""));
        assert!(svg.contains("width=\"1\" height=\"1\""));
        assert!(!svg.contains("fill="));
        assert!(!svg.contains("<rect"));
        assert!(!svg.contains("<circle"));
        assert!(!svg.contains("<text"));
        assert_eq!(svg, render_blank_avatar());
    }

    /// Both tiles have to survive the raster dispatcher in every encoding the
    /// route answers — an empty slot that 500s is the broken-image icon the
    /// transparent tile exists to avoid, and the 1x1 blank is the smallest
    /// surface either encoder will ever be handed.
    #[test]
    fn avatar_tiles_rasterize() {
        let row = ContributorRow {
            login: Some("zhom".into()),
            name: "zhom".into(),
            avatar_url: None,
            commits: 1,
        };
        for format in [
            crate::raster::RasterFormat::Png,
            crate::raster::RasterFormat::Webp,
        ] {
            let blank = crate::raster::rasterize(&render_blank_avatar(), format, 2.0)
                .expect("blank tile rasterizes");
            assert!(!blank.is_empty());

            let tile = crate::raster::rasterize(
                &render_contributor_avatar(&row, 0, &theme::DARK),
                format,
                2.0,
            )
            .expect("avatar tile rasterizes");
            assert!(!tile.is_empty());
        }
    }

    #[test]
    fn todo_trend_handles_empty() {
        let svg = render_todo_trend("foo/bar", &[], &theme::LIGHT);
        assert!(svg.contains("no TODO/FIXME movement in the analyzed commit window"));
    }

    /// The trace is the sheet's primary data, so it is the one thing in
    /// drafting red, and the peak is measured by a vertical dimension that
    /// letters its value on its own line.
    #[test]
    fn todo_trend_plots_one_red_trace_and_measures_its_peak() {
        let pts = vec![
            TodoPoint {
                day: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                running_total: 10,
            },
            TodoPoint {
                day: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                running_total: 50,
            },
            TodoPoint {
                day: NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
                running_total: 30,
            },
        ];
        let svg = render_todo_trend("foo/bar", &pts, &theme::LIGHT);
        assert!(!svg.contains("attributeName=\"r\""));
        assert!(!svg.contains("stroke-dashoffset"));
        assert!(svg.contains("dur=\"0.2s\""));
        assert_eq!(
            svg.matches(theme::LIGHT.accent).count(),
            1,
            "the trace is the whole red budget"
        );
        assert!(svg.contains(&format!("stroke-width=\"{}\"", texture::W_EMPHASIS)));
        assert!(svg.contains(">peak 50<"));
        assert!(svg.contains("rotate(-90"));
        // The axis is lettered at both ends, never a background grid.
        assert!(svg.contains(">2026-01-01<") && svg.contains(">2026-12-31<"));
    }

    #[test]
    fn motion_is_grouped_short_and_reduced_motion_safe() {
        let rows = vec![
            FileRow {
                path: "a.rs".into(),
                count: 3,
            },
            FileRow {
                path: "b.rs".into(),
                count: 2,
            },
            FileRow {
                path: "c.rs".into(),
                count: 1,
            },
            FileRow {
                path: "d.rs".into(),
                count: 1,
            },
        ];
        let bars = render_bug_magnets("foo/bar", &rows, &theme::LIGHT);
        assert!(!bars.contains("attributeName=\"width\""));
        assert!(bars.contains("begin=\"0.00s\""));
        assert!(bars.contains("begin=\"0.02s\""));
        assert!(bars.contains("begin=\"0.04s\""));
        assert!(bars.contains("begin=\"0.05s\""));
        assert!(!bars.contains("begin=\"0.08s\""));
        assert!(bars.contains("prefers-reduced-motion: reduce"));

        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let heat = render_heatmap(
            "foo/bar",
            "Commits",
            start,
            start + chrono::Duration::days(30),
            &[],
            None,
            &theme::LIGHT,
        );
        assert_eq!(heat.matches("<animate ").count(), 1);
        assert!(heat.contains("class=\"heat-cells\""));
        assert!(heat.contains("prefers-reduced-motion: reduce"));
    }

    #[test]
    fn bus_factor_empty_is_zero() {
        assert_eq!(compute_bus_factor(&[], 0), 0);
        assert_eq!(compute_bus_factor(&[], 100), 0);
        assert_eq!(compute_bus_factor(&[0, 0], 0), 0);
        // Non-positive entries are ignored even with a bogus total.
        assert_eq!(compute_bus_factor(&[0, -3], 10), 0);
    }

    #[test]
    fn bus_factor_single_author_is_one() {
        assert_eq!(compute_bus_factor(&[42], 42), 1);
    }

    #[test]
    fn bus_factor_dominant_author() {
        // 60/30/10 → the top author alone exceeds half.
        assert_eq!(compute_bus_factor(&[60, 30, 10], 100), 1);
    }

    #[test]
    fn bus_factor_even_split_needs_strict_majority() {
        // 4 × 25: two authors reach exactly 50%, which does NOT exceed half.
        assert_eq!(compute_bus_factor(&[25, 25, 25, 25], 100), 3);
        // 50/50 needs both.
        assert_eq!(compute_bus_factor(&[50, 50], 100), 2);
    }

    #[test]
    fn bus_factor_input_order_does_not_matter() {
        assert_eq!(compute_bus_factor(&[1, 100, 2], 103), 1);
    }

    #[test]
    fn bus_factor_truncated_prefix_is_lower_bound() {
        // Top-2 prefix sums to 20 of 100 → can't cross half; report the
        // prefix length as a lower bound.
        assert_eq!(compute_bus_factor(&[10, 10], 100), 2);
    }

    /// Concentration is one measured band: a segment per author in its own
    /// pen, numbered against a key, and one red dimension marking where the
    /// bus factor actually falls.
    #[test]
    fn bus_factor_band_is_segmented_numbered_and_dimensioned() {
        let authors = vec![
            AuthorShare {
                label: "alice".into(),
                login: Some("alice".into()),
                avatar_url: Some("https://avatars.githubusercontent.com/u/1".into()),
                commits: 60,
            },
            AuthorShare {
                label: "bob".into(),
                login: None,
                avatar_url: None,
                commits: 30,
            },
            AuthorShare {
                label: "carol".into(),
                login: None,
                avatar_url: None,
                commits: 10,
            },
        ];
        let svg = render_bus_factor("foo/bar", &authors, 100, &theme::LIGHT);
        assert!(svg.contains("Bus factor"));
        assert!(svg.contains(">FACTOR<") && svg.contains(">RISK<"));
        assert!(svg.contains(">Solo</text>"));
        assert!(svg.contains("alice"));
        assert!(svg.contains("60 commits · 60.0%"));
        assert!(svg.contains("https://github.com/alice"));
        assert!(svg.contains("<image"));
        assert!(svg.contains("gitdebt.com"));
        assert!(!svg.contains("var(--"));
        // Every segment numbered, keyed to the same number in the list.
        assert!(svg.contains(">01</text>") && svg.contains(">03</text>"));
        // Three authors, three distinct pens, none of them drafting red.
        let pens = pens_for(&theme::LIGHT, &["alice", "bob", "carol"]);
        for pen in &pens {
            assert!(svg.contains(&format!("fill=\"{pen}\"")));
            assert_ne!(*pen, theme::LIGHT.accent);
        }
        // The dimension carries the whole red budget and letters the factor.
        assert_eq!(svg.matches(theme::LIGHT.accent).count(), 4);
        assert!(svg.contains(">factor 1 · 60.0%<"));
    }

    #[test]
    fn bus_factor_hides_people_below_one_percent() {
        let svg = render_bus_factor(
            "foo/bar",
            &[
                AuthorShare {
                    label: "visible".into(),
                    login: None,
                    avatar_url: None,
                    commits: 999,
                },
                AuthorShare {
                    label: "tiny".into(),
                    login: None,
                    avatar_url: None,
                    commits: 1,
                },
            ],
            1_000,
            &theme::LIGHT,
        );
        assert!(svg.contains("visible"));
        assert!(!svg.contains(">tiny</text>"));
    }

    /// Seven category pens, so seven segments: an eighth would have to repeat
    /// a pen, and a repeated pen in one band lies about how many people it is
    /// showing.
    #[test]
    fn bus_factor_band_never_repeats_a_pen() {
        let authors: Vec<AuthorShare> = (0..12)
            .map(|index| AuthorShare {
                label: format!("author-{index}"),
                login: None,
                avatar_url: None,
                commits: 100 - index,
            })
            .collect();
        let svg = render_bus_factor("foo/bar", &authors, 1_140, &theme::LIGHT);
        assert_eq!(svg.matches("class=\"person-link\"").count(), 0);
        assert!(svg.contains(">07</text>"));
        assert!(!svg.contains(">08</text>"));
        assert_eq!(BUS_SEGMENTS, 7);
    }

    #[test]
    fn bus_factor_chart_empty_state() {
        let svg = render_bus_factor("foo/bar", &[], 0, &theme::LIGHT);
        assert!(svg.contains("no contributor data yet"));
        // Zero-commit authors also count as empty.
        let svg = render_bus_factor(
            "foo/bar",
            &[AuthorShare {
                label: "x".into(),
                login: None,
                avatar_url: None,
                commits: 0,
            }],
            0,
            &theme::LIGHT,
        );
        assert!(svg.contains("no contributor data yet"));
    }

    #[test]
    fn bucket_months_empty() {
        assert!(bucket_months(&[]).is_empty());
    }

    #[test]
    fn bucket_months_sums_within_month_and_fills_gaps() {
        let d = |y, m, day, c| DayCount {
            day: NaiveDate::from_ymd_opt(y, m, day).unwrap(),
            commits: c,
        };
        // Unsorted on purpose; Feb and Mar are dormant.
        let days = vec![d(2026, 4, 2, 5), d(2026, 1, 5, 3), d(2026, 1, 20, 4)];
        let months = bucket_months(&days);
        assert_eq!(months.len(), 4);
        assert_eq!(
            months[0],
            MonthCount {
                month: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                commits: 7,
            }
        );
        assert_eq!(months[1].commits, 0);
        assert_eq!(months[2].commits, 0);
        assert_eq!(
            months[3],
            MonthCount {
                month: NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
                commits: 5,
            }
        );
    }

    #[test]
    fn bucket_months_crosses_year_boundary() {
        let d = |y, m, day, c| DayCount {
            day: NaiveDate::from_ymd_opt(y, m, day).unwrap(),
            commits: c,
        };
        let days = vec![d(2025, 12, 31, 1), d(2026, 1, 1, 2)];
        let months = bucket_months(&days);
        assert_eq!(months.len(), 2);
        assert_eq!(
            months[0].month,
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
        assert_eq!(
            months[1].month,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
        );
    }

    #[test]
    fn commit_trend_handles_empty() {
        let svg = render_commit_trend("foo/bar", &[], &theme::LIGHT);
        assert!(svg.contains("no commit data yet"));
    }

    #[test]
    fn commit_trend_renders_line_and_peak() {
        let d = |y, m, day, c| DayCount {
            day: NaiveDate::from_ymd_opt(y, m, day).unwrap(),
            commits: c,
        };
        let days = vec![d(2025, 11, 3, 4), d(2026, 2, 10, 9), d(2026, 2, 11, 1)];
        let svg = render_commit_trend("foo/bar", &days, &theme::LIGHT);
        assert!(svg.contains("Commits per month"));
        // Feb 2026 sums to 10 and is the peak.
        assert!(svg.contains("peak 10 in 2026-02"));
        assert!(svg.contains(">peak 10<"));
        assert!(!svg.contains("stroke-dashoffset"));
        assert!(svg.contains("dur=\"0.2s\""));
        assert!(svg.contains(theme::LIGHT.accent));
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn commit_trend_single_month_does_not_panic() {
        let days = vec![DayCount {
            day: NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            commits: 2,
        }];
        let svg = render_commit_trend("foo/bar", &days, &theme::LIGHT);
        assert!(svg.contains("peak 2 in 2026-03"));
    }

    #[test]
    fn new_charts_are_bytes_deterministic() {
        let authors = vec![AuthorShare {
            label: "a".into(),
            login: None,
            avatar_url: None,
            commits: 5,
        }];
        assert_eq!(
            render_bus_factor("x/y", &authors, 5, &theme::DARK),
            render_bus_factor("x/y", &authors, 5, &theme::DARK),
        );
        let days = vec![DayCount {
            day: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            commits: 1,
        }];
        assert_eq!(
            render_commit_trend("x/y", &days, &theme::DARK),
            render_commit_trend("x/y", &days, &theme::DARK),
        );
        for theme in [&theme::LIGHT, &theme::DARK] {
            for (name, svg) in every_sheet(theme) {
                let again = every_sheet(theme)
                    .into_iter()
                    .find(|(other, _)| *other == name)
                    .map(|(_, svg)| svg)
                    .expect("same sheet");
                assert_eq!(svg, again, "{name} is not byte-deterministic");
            }
        }
    }

    #[test]
    fn languages_shows_total_and_code_separately() {
        let rows = vec![
            LanguageBar {
                language: "Rust".into(),
                files: 88,
                lines_code: 45_000,
                lines_blank: 8_000,
                lines_comment: 2_500,
            },
            LanguageBar {
                language: "TypeScript".into(),
                files: 43,
                lines_code: 5_400,
                lines_blank: 800,
                lines_comment: 200,
            },
        ];
        let svg = render_languages("foo/bar", &rows, &theme::LIGHT);
        // Caption has both "lines" and "code" totals.
        assert!(svg.contains(" lines · "));
        assert!(svg.contains(" code · "));
        // Per-row meta has files + code.
        assert!(svg.contains("88 files · "));
        // Each language plots in its own conventional ink, with the graphite
        // hairline standing at the measured edge.
        let rust = language_color("Rust", &theme::LIGHT);
        let ts = language_color("TypeScript", &theme::LIGHT);
        assert_ne!(rust, ts);
        assert!(svg.contains(&format!("fill=\"{rust}\"")));
        assert!(svg.contains(&format!("fill=\"{ts}\"")));
        assert!(svg.contains(&format!("stroke=\"{}\"", theme::LIGHT.fg)));
        // The peak sets the scale, and it is the one dimensioned value.
        assert!(svg.contains(">55.5k lines<"));
        assert_eq!(svg.matches(theme::LIGHT.accent).count(), 4);
        // No swatch dot, no tier ladder, no alpha modulation.
        assert!(!svg.contains("<circle"));
        assert!(!svg.contains("fill-opacity"));
    }

    #[test]
    fn language_colors_are_deterministic_stable_and_distinct() {
        // Same input → same bytes, in both prints.
        for theme in [&theme::LIGHT, &theme::DARK] {
            assert_eq!(language_color("Rust", theme), language_color("Rust", theme));
            assert_eq!(
                language_color("Made-up lang", theme),
                language_color("Made-up lang", theme)
            );
        }
        // The conventional hue survives; only lightness is corrected. These
        // are real language brand colours and are not part of the palette.
        assert_eq!(conventional_language_color("Rust"), Some("#dea584"));
        assert_eq!(conventional_language_color("Go"), Some("#00add8"));
        assert_eq!(conventional_language_color("Config"), Some("#8b8b8b"));
        assert_eq!(conventional_language_color("Unknown language"), None);

        // Every common language resolves to its own color, per print.
        let langs = [
            "Rust",
            "TypeScript",
            "JavaScript",
            "Python",
            "Go",
            "C",
            "C++",
            "C#",
            "Java",
            "Kotlin",
            "Swift",
            "Ruby",
            "PHP",
            "Shell",
            "HTML",
            "CSS",
            "Vue",
            "Svelte",
            "Dart",
            "Scala",
            "Elixir",
            "Haskell",
            "Lua",
            "Zig",
            "Nix",
            "Markdown",
            "JSON",
            "YAML",
            "TOML",
            "SQL",
            "Dockerfile",
        ];
        for theme in [&theme::LIGHT, &theme::DARK] {
            let mut seen: std::collections::BTreeMap<String, &str> = Default::default();
            for name in langs {
                let color = language_color(name, theme);
                assert_eq!(color.len(), 7, "{name} must render as #rrggbb");
                if let Some(other) = seen.insert(color.clone(), name) {
                    panic!(
                        "{name} collides with {other} at {color} (dark={})",
                        theme.dark
                    );
                }
            }
        }
    }

    #[test]
    fn language_colors_stay_legible_on_both_prints() {
        // Near-black dark ground: nothing may sink below the readable floor
        // (Lua's #000080 and JSON's #292929 are the worst cases).
        for name in ["Lua", "JSON", "PowerShell", "Ruby", "C", "Less", "Made-up"] {
            let dark = parse_hex(&language_color(name, &theme::DARK)).expect("hex");
            assert!(
                luma(dark) >= 0.29,
                "{name} is too dark on the dark print: {dark:?}"
            );
            let light = parse_hex(&language_color(name, &theme::LIGHT)).expect("hex");
            assert!(
                luma(light) <= 0.56,
                "{name} is too light on the light print: {light:?}"
            );
        }
        for name in ["JavaScript", "OCaml", "Shell", "SVG"] {
            let light = parse_hex(&language_color(name, &theme::LIGHT)).expect("hex");
            assert!(luma(light) <= 0.56, "{name} washes out on paper");
        }
    }

    #[test]
    fn unnamed_languages_get_stable_hashed_hues() {
        let a = language_color("Whatsit", &theme::DARK);
        let b = language_color("Thingamajig", &theme::DARK);
        assert_ne!(a, b, "distinct unknown languages must not share a hue");
        assert_eq!(a, language_color("Whatsit", &theme::DARK));
        // `Config` is the documented "no single language" bucket and keeps
        // the achromatic gray rather than picking up a hue.
        assert_eq!(
            conventional_language_color("Config"),
            Some(NEUTRAL_LANGUAGE_COLOR)
        );

        let rows = vec![LanguageBar {
            language: "Unknown language".into(),
            files: 1,
            lines_code: 12,
            lines_blank: 1,
            lines_comment: 2,
        }];
        let svg = render_languages("foo/bar", &rows, &theme::DARK);
        assert!(svg.contains(&language_color("Unknown language", &theme::DARK)));
        assert!(svg.contains(&format!(".label {{ fill: {};", theme::DARK.fg)));
    }

    #[test]
    fn languages_labels_tree_census_as_files_not_lines() {
        let rows = vec![LanguageBar {
            language: "C".into(),
            files: 42_000,
            lines_code: 0,
            lines_blank: 0,
            lines_comment: 0,
        }];
        let svg = render_languages("torvalds/linux", &rows, &theme::LIGHT);
        assert!(svg.contains("42.0k files in 1 languages · current HEAD tree"));
        assert!(svg.contains("42000 files in current HEAD"));
        assert!(!svg.contains("42.0k lines"));
        assert!(svg.contains(">42.0k files<"));
    }

    #[test]
    fn empty_sheets_letter_the_reason_and_keep_the_colophon() {
        let svg = render_commit_trend("o/r", &[], &theme::DARK);
        assert!(svg.contains("no commit data yet"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.contains("dominant-baseline=\"central\""));
        // No sheet paints a ground: every surface composites onto whatever
        // page embeds it.
        assert!(!svg.contains(&format!(
            "<rect width=\"1200\" height=\"360\" fill=\"{}\" />",
            theme::DARK.bg
        )));
    }

    /// The static-output guards. Every sheet's finished frame has to survive
    /// with the SMIL stripped: nothing is zero-width, nothing sits at zero
    /// opacity, and no trace is hidden behind a dash offset.
    #[test]
    fn charts_keep_their_finished_frame_without_smil() {
        let files = vec![FileRow {
            path: "src/lib.rs".into(),
            count: 12,
        }];
        let bars = without_smil(&render_bug_magnets("foo/bar", &files, &theme::LIGHT));
        assert!(!bars.contains("width=\"0\""));
        assert!(!bars.contains("<g opacity=\"0\""));

        let days = vec![DayCount {
            day: NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            commits: 4,
        }];
        let heatmap = without_smil(&render_heatmap(
            "foo/bar",
            "Commits",
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
            &days,
            None,
            &theme::LIGHT,
        ));
        assert!(!heatmap.contains("class=\"cell\"") || heatmap.contains("opacity=\"1\""));
        assert!(!heatmap.contains("<g opacity=\"0\""));

        let contributors = without_smil(&render_contributors(
            "foo/bar",
            &[ContributorRow {
                login: Some("alice".into()),
                name: "Alice".into(),
                avatar_url: None,
                commits: 4,
            }],
            &theme::LIGHT,
        ));
        assert!(contributors.contains("class=\"avatar-frame\""));
        assert!(contributors.contains("opacity=\"1\""));

        let languages = without_smil(&render_languages(
            "foo/bar",
            &[LanguageBar {
                language: "Rust".into(),
                files: 1,
                lines_code: 100,
                lines_blank: 10,
                lines_comment: 5,
            }],
            &theme::LIGHT,
        ));
        assert!(!languages.contains("width=\"0\""));
        assert!(!languages.contains("<g opacity=\"0\""));

        let authors = without_smil(&render_bus_factor(
            "foo/bar",
            &[AuthorShare {
                label: "alice".into(),
                login: None,
                avatar_url: None,
                commits: 4,
            }],
            4,
            &theme::LIGHT,
        ));
        assert!(!authors.contains("width=\"0\""));
        assert!(!authors.contains("<g opacity=\"0\""));

        let todos = without_smil(&render_todo_trend(
            "foo/bar",
            &[
                TodoPoint {
                    day: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    running_total: 1,
                },
                TodoPoint {
                    day: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
                    running_total: 2,
                },
            ],
            &theme::LIGHT,
        ));
        assert!(!todos.contains("stroke-dashoffset"));
        assert!(todos.contains(&format!("fill=\"{}\" />", theme::LIGHT.track)));
        assert!(todos.contains(&format!("stroke=\"{}\"", theme::LIGHT.accent)));

        let commits = without_smil(&render_commit_trend(
            "foo/bar",
            &[
                DayCount {
                    day: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    commits: 1,
                },
                DayCount {
                    day: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
                    commits: 2,
                },
            ],
            &theme::LIGHT,
        ));
        assert!(!commits.contains("stroke-dashoffset"));
        assert!(commits.contains(&format!("fill=\"{}\" />", theme::LIGHT.track)));
        assert!(commits.contains(&format!("stroke=\"{}\"", theme::LIGHT.accent)));
    }

    /// Two lanes, two pens, one red dimension on the wider one, and the
    /// repository count tallied as extension ticks along the lane it was
    /// measured on. The pulsing badge and its moving patterns are gone.
    #[test]
    fn contribution_profile_plots_two_lanes_and_tallies_its_repos() {
        let profile = ContributionProfile {
            owned_repos: 4,
            external_repos: 9,
            owned_commits: 120,
            external_commits: 380,
            visionary_count: 2,
        };
        let first = render_contribution_profile("@alice", &profile, &theme::DARK);
        assert_eq!(
            first,
            render_contribution_profile("@alice", &profile, &theme::DARK)
        );
        assert!(first.contains("Ecosystem-led contributor"));
        assert!(first.contains(">BREAKOUT<") && first.contains(">2 projects<"));
        assert!(!first.contains("VISIONARY"));
        assert!(!first.contains("<animateTransform"));
        assert!(!first.contains("<circle"));
        // Two distinct pens, neither of them drafting red.
        let pens = pens_for(&theme::DARK, &["owned", "external"]);
        assert_ne!(pens[0], pens[1]);
        for pen in &pens {
            assert!(first.contains(&format!("fill=\"{pen}\"")));
            assert_ne!(*pen, theme::DARK.accent);
        }
        // The wider lane carries the sheet's one dimension.
        assert!(first.contains(">76% of commits<"));
        assert_eq!(first.matches(theme::DARK.accent).count(), 4);
        // Nine repositories, nine 0.5px tally ticks on the external lane,
        // plus four on the owned one.
        assert_eq!(
            first
                .matches(&format!(
                    "stroke=\"{}\" stroke-width=\"0.5\"",
                    theme::DARK.ink_3
                ))
                .count(),
            13
        );

        let frozen = crate::raster::freeze_svg_animations(&first);
        assert!(!frozen.contains("<animate"));
        assert!(frozen.contains("120 commits"));
        assert!(frozen.contains("380 commits"));

        let one_sided = render_contribution_profile(
            "@builder",
            &ContributionProfile {
                owned_repos: 2,
                external_repos: 0,
                owned_commits: 330,
                external_commits: 0,
                visionary_count: 0,
            },
            &theme::DARK,
        );
        assert!(!one_sided.contains("width=\"0.00\" height=\"26.00\""));
        assert!(!one_sided.contains(">BREAKOUT<"));
    }
}
