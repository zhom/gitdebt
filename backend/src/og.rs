//! Social Open Graph card renderer → 1200×630 PNG.
//!
//! Why a dedicated module (not the chart renderers): social platforms
//! (Twitter/X, Slack, LinkedIn, Discord, Facebook) **reject SVG** as an
//! `og:image` and demand a real raster at the dimensions the page declares.
//! The chart SVGs are README line charts; a card is a different composition,
//! sized at exactly **1200×630** so the rasterized PNG matches the
//! `og:image:width`/`height` the frontend declares.
//!
//! Pipeline mirrors the chart raster path: build a deterministic SVG here,
//! then `raster::rasterize(svg, Png, 1.0)`. Scale **1.0** is load-bearing:
//! the viewBox is 1200×630 so scale 1.0 yields a 1200×630 PNG. (The chart
//! endpoints rasterize at 2.0 for retina READMEs; OG images must be exactly
//! the declared size or crawlers letterbox them.)
//!
//! # The card is the drawing, plotted at 3:1
//!
//! Every gitdebt asset is a sheet of one dimensioned engineering drawing, and
//! a social card is the sheet with the LEAST notation on it: a framed sheet,
//! the subject lettered large in ink, the star trace across the lower band,
//! one dimensioned value in drafting red, and the mark small in the
//! bottom-left corner. Nothing else. A card is read at ~300px wide in a feed,
//! so the type is large and the notation is sparse — this is the one surface
//! where you draw *fewer* dimension lines, not more.
//!
//! The notation vocabulary in [`texture`] is authored at 1:1: a 1px object
//! line, a 5px terminator, a 6px extension tick. Rather than re-derive all of
//! those constants at poster size, a card plots the vocabulary at [`PLOT`]:1
//! inside one scaled group and hands it coordinates in drawing units (see
//! [`du`]). The sheet therefore carries exactly the three pen widths the
//! system has — 0.5, 1 and 2 — plotted three times larger, so the trace is
//! six real pixels and still reads in a 300px thumbnail.
//!
//! # Where drafting red is spent
//!
//! A single-repo sheet spends it twice: on the star trace, which is the
//! primary data trace, and on the one dimension that measures the total.
//! Nowhere else. A compare sheet has no primary series, so it draws in
//! plotter pens and carries no red at all; a profile sheet measures nothing
//! against a datum, so it carries none either. Red is attached to a
//! measurement or it is not spent.
//!
//! Fonts: the generic stacks from [`texture`]. `raster.rs` maps every generic
//! family onto the bundled Inter, so these resolve and text renders;
//! introducing a novel family here would rasterize as blank glyph boxes.
//!
//! Deterministic: same input → same bytes.

use crate::brand;
use crate::cards::UserCardData;
use crate::chart::Point;
use crate::texture::{self, Dimension, MONO, SANS, Side, coord, escape_xml};
use crate::theme::{self, Theme};

/// Fixed OG card dimensions. Declared `og:image:width`/`height` on the
/// frontend MUST equal these, and `raster::rasterize(.., 1.0)` of a
/// `WIDTH × HEIGHT` viewBox yields a PNG of exactly these dimensions.
pub const OG_WIDTH: u32 = 1200;
pub const OG_HEIGHT: u32 = 630;

/// The scale the drawing is plotted at on a card sheet. The notation
/// vocabulary is authored at 1:1; a card wraps it in one `scale(PLOT)` group
/// so the three pen widths stay 0.5/1/2 and land as 1.5/3/6 real pixels.
const PLOT: f32 = 3.0;

/// Card px → drawing units, for the coordinates handed to the notation
/// vocabulary inside the plotted group.
fn du(v: f32) -> f32 {
    v / PLOT
}

/// The sheet frame, inset from the trimmed edge. The one rule on the card
/// that measures nothing, and it earns its place by enclosing the sheet.
const FRAME_INSET: f32 = 30.0;

/// Left margin for body lettering and for every notation datum.
const CONTENT_X: f32 = 60.0;
/// Left margin for display lettering. Three pixels left of [`CONTENT_X`]:
/// an 82px glyph carries proportionally more left side bearing than a 28px
/// one, so aligning their boxes would leave the display line looking
/// indented against the lines beneath it.
const DISPLAY_X: f32 = 57.0;
/// Right edge available to any lettering, mirroring [`CONTENT_X`].
const CONTENT_RIGHT: f32 = OG_WIDTH as f32 - CONTENT_X;

/// The subject line: the repository slug, the compare title, the handle.
const SUBJECT_SIZE: f32 = 82.0;
const SUBJECT_BASELINE: f32 = 174.0;
/// The quiet mono line under the subject.
const SECONDARY_SIZE: f32 = 28.0;
const SECONDARY_BASELINE: f32 = 226.0;

/// The band the object line occupies: the lower two-thirds of the sheet.
const BAND_TOP: f32 = 292.0;
const BAND_BOTTOM: f32 = 528.0;
/// Right edge of a single-series plot, leaving the gutter its dimension
/// stands in.
const BAND_RIGHT: f32 = 1076.0;
/// Where the star dimension stands: clear of the trace, clear of the frame.
const DIM_X: f32 = 1116.0;
const DIM_SIZE: f32 = 39.0;

/// Right edge of a compare plot: the labels claim the rest of the band.
const COMPARE_BAND_RIGHT: f32 = 796.0;
/// Where a compare series' leader ends and its label begins.
const COMPARE_LABEL_X: f32 = 856.0;
const COMPARE_LABEL_SIZE: f32 = 30.0;
/// Vertical pitch between two series labels. Enough that two lines finishing
/// at the same height still each get a readable label.
const COMPARE_LABEL_GAP: f32 = 46.0;
/// Traces one sheet can carry and still be read. Past this the tail collapses
/// into a single "+N more" line: seven near-parallel labels in a 236px band
/// is not a comparison, it is a smear.
const COMPARE_MAX_TRACES: usize = 5;

/// The headline value on a sheet with no trace to dimension.
const VALUE_SIZE: f32 = 150.0;
const VALUE_BASELINE: f32 = 486.0;
const UNIT_SIZE: f32 = 26.0;
/// Gap between the value and the unit label riding on its baseline.
const UNIT_GAP: f32 = 26.0;

/// A profile's persona: its own line, well clear above the value, because it
/// classifies the account and is not a unit of the number under it.
const PERSONA_SIZE: f32 = 30.0;
const PERSONA_BASELINE: f32 = 310.0;

/// The signature lockup, bottom-left: the mark and the domain.
const SIG_MARK_X: f32 = 60.0;
const SIG_MARK_Y: f32 = 552.0;
const SIG_MARK_W: f32 = 30.0;
const SIG_TEXT_X: f32 = 102.0;
const SIG_TEXT_BASELINE: f32 = 571.0;
const SIG_TEXT_SIZE: f32 = 24.0;

/// Cap height of the display face as a fraction of its size. Inter's caps
/// and lining figures both sit at 0.727em; this is only used to centre a
/// lockup optically, never to lay out a line of text.
const CAP_HEIGHT: f32 = 0.727;

/// More points than this cannot show on a 1000px band, but a popular
/// repository carries hundreds of thousands of them and would turn one social
/// card into a multi-megabyte path. Sampling is by index, so it is
/// deterministic and always keeps the first and the last point.
const TRACE_SAMPLES: usize = 240;

/// Inputs for a single-repo OG card. All the secondary fields are
/// best-effort — a missing piece is simply omitted from the card so the
/// renderer never blocks on data it doesn't have.
#[derive(Debug, Clone, Default)]
pub struct RepoCard {
    /// `owner/repo` slug (already lowercased by the caller).
    pub slug: String,
    /// Total stars — the dimensioned value.
    pub stars: u64,
    /// Fork count, when known.
    pub forks: Option<u64>,
    /// Best resolved download total + its source label, e.g.
    /// `(2_100_000, "npm")` → "2.1M npm downloads". `None` omits the row.
    pub downloads: Option<(u64, String)>,
    /// Cumulative star-history points for the object line. Empty → the sheet
    /// letters the total instead of tracing it.
    pub series: Vec<Point>,
}

/// One repo entry on a compare card: slug, star count, and the series for its
/// trace. Pens are claimed by slug, never by position in this list.
#[derive(Debug, Clone, Default)]
pub struct CompareEntry {
    pub slug: String,
    pub stars: u64,
    pub series: Vec<Point>,
}

// Repo card

/// Render the single-repo social card SVG (1200×630): the slug lettered
/// large, a quiet mono line of secondary totals, the star trace across the
/// lower band in drafting red, and one vertical dimension measuring the total
/// up from the zero datum.
pub fn render_repo_card(card: &RepoCard, theme: &Theme) -> String {
    let mut ink = String::new();
    let mut notation = String::new();

    ink.push_str(&subject_slug(&card.slug, theme));
    let secondary = secondary_line(card);
    if !secondary.is_empty() {
        ink.push_str(&mono_line(&secondary, theme.muted));
    }

    let band = Band::single();
    // The vertical scale covers the reported total as well as the trace, so
    // the dimension measures against the same axis the object is drawn on.
    match Range::of(&[card.series.as_slice()], card.stars) {
        Some(range) => {
            notation.push_str(&zero_datum(&band, theme));
            notation.push_str(&trace(
                &card.series,
                &band,
                &range,
                theme.accent,
                texture::W_EMPHASIS,
            ));
            notation.push_str(&star_dimension(card.stars, &band, &range, theme));
        }
        // Nothing to trace, so nothing is measured: the total is lettered.
        None => ink.push_str(&value_block(&fmt_count(card.stars), "GITHUB STARS", theme)),
    }

    ink.push_str(&signature(theme));
    sheet(theme, &ink, &notation, &format!("gitdebt · {}", card.slug))
}

/// Build the "45 forks · 2.1M npm downloads" style secondary row, omitting
/// whichever piece is missing. Empty string → no row drawn.
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

/// Render the multi-repo compare card SVG (1200×630): an "{a} vs {b}" title
/// and one trace per repository, each in its own plotter pen and each
/// labelled at its own line end, so hue is never the only thing telling two
/// series apart. No series is primary here, so no drafting red is spent.
pub fn render_compare_card(entries: &[CompareEntry], theme: &Theme) -> String {
    let drawn = if entries.len() > COMPARE_MAX_TRACES {
        COMPARE_MAX_TRACES - 1
    } else {
        entries.len()
    };
    let shown = &entries[..drawn];
    let omitted = entries.len() - drawn;

    // The title names exactly the repositories the sheet draws, and so does
    // the alt text: a headline listing twelve slugs over four traces is
    // describing a different card.
    let title = shown
        .iter()
        .map(|entry| short_slug(&entry.slug))
        .collect::<Vec<_>>()
        .join(" vs ");

    let mut ink = String::new();
    ink.push_str(&display(
        DISPLAY_X,
        SUBJECT_BASELINE,
        SUBJECT_SIZE,
        theme.fg,
        &escape_xml(&fit(&title, SUBJECT_SIZE, CONTENT_RIGHT - DISPLAY_X)),
    ));

    let mut notation = String::new();
    let band = Band::compare();
    let all: Vec<&[Point]> = shown.iter().map(|entry| entry.series.as_slice()).collect();
    if let Some(range) = Range::of(&all, 0) {
        notation.push_str(&zero_datum(&band, theme));
        let keys: Vec<&str> = shown.iter().map(|entry| entry.slug.as_str()).collect();
        let pens = theme::pens_for(theme, &keys);

        // Where each trace actually finishes. Histories do not all end on
        // the same day, so a leader has to point at its own line's last
        // point rather than at the right edge of the band — a terminator
        // landing where no line is measures nothing.
        let ends: Vec<Option<(f32, f32)>> = shown
            .iter()
            .map(|entry| {
                entry.series.last().map(|point| {
                    (
                        range.x(&band, point.at.timestamp() as f32),
                        range.y(&band, point.stars as f32),
                    )
                })
            })
            .collect();
        // A series with no points has no line end to label; park its slot on
        // the floor so it cannot drag its neighbours' labels around.
        let heights: Vec<f32> = ends
            .iter()
            .map(|end| end.map_or(band.bottom, |(_, y)| y))
            .collect();
        let labels = spread(&heights, COMPARE_LABEL_GAP, BAND_TOP - 6.0, BAND_BOTTOM);

        for (index, entry) in shown.iter().enumerate() {
            let pen = pens[index];
            notation.push_str(&trace(&entry.series, &band, &range, pen, texture::W_OBJECT));
            let Some(datum) = ends[index] else {
                continue;
            };
            let label = fit(
                &format!("{} {}", short_slug(&entry.slug), fmt_count(entry.stars)),
                COMPARE_LABEL_SIZE,
                CONTENT_RIGHT - COMPARE_LABEL_X - 12.0,
            );
            notation.push_str(&texture::leader(
                (du(datum.0), du(datum.1)),
                (du(COMPARE_LABEL_X), du(labels[index])),
                &label,
                du(COMPARE_LABEL_SIZE),
                pen,
            ));
        }

        if omitted > 0 {
            let under = labels.iter().copied().fold(BAND_TOP, f32::max) + COMPARE_LABEL_GAP;
            ink.push_str(&sans_text(
                COMPARE_LABEL_X + 12.0,
                under.min(BAND_BOTTOM + COMPARE_LABEL_GAP),
                UNIT_SIZE,
                theme.ink_3,
                &escape_xml(&format!("+{omitted} more")),
            ));
        }
    }

    let aria = if omitted > 0 {
        format!("gitdebt · {title} +{omitted} more")
    } else {
        format!("gitdebt · {title}")
    };
    ink.push_str(&signature(theme));
    sheet(theme, &ink, &notation, &aria)
}

/// Spread label heights so no two land on top of each other.
///
/// Each label starts at its own line end and is pushed down only as far as
/// the one above forces it; if the stack overruns `bottom` the whole run
/// slides back up together, so the labels keep their relative spacing and
/// their order still matches the order the lines finish in. Pure: the same
/// ends always produce the same heights.
fn spread(ends: &[f32], gap: f32, top: f32, bottom: f32) -> Vec<f32> {
    let mut order: Vec<usize> = (0..ends.len()).collect();
    // Stable, and total_cmp keeps it a total order without assuming the band
    // handed us finite heights.
    order.sort_by(|a, b| ends[*a].total_cmp(&ends[*b]));

    let mut placed = vec![0.0_f32; ends.len()];
    let mut previous = f32::NEG_INFINITY;
    for index in &order {
        let y = ends[*index].max(top).max(previous + gap);
        placed[*index] = y;
        previous = y;
    }
    if let Some(last) = order.last() {
        let overrun = placed[*last] - bottom;
        if overrun > 0.0 {
            for y in &mut placed {
                *y -= overrun;
            }
        }
    }
    placed
}

// Default site card

/// Render the default site card SVG (1200×630): the title sheet of the
/// drawing set. There is no subject and nothing to measure, so the lockup is
/// the sheet's object — the mark at full size beside the wordmark, the
/// tagline under it, and the colophon in the corner. This is the one card
/// where the mark is not small, and the only one whose corner carries the
/// domain alone: repeating the robot at two sizes on one sheet would be a
/// signature signing a signature. Used for `/api/og.png` with no repos.
pub fn render_default_card(theme: &Theme) -> String {
    const LOCKUP_MARK_W: f32 = 150.0;
    const WORDMARK_SIZE: f32 = 190.0;
    const WORDMARK_BASELINE: f32 = 330.0;

    let mut ink = String::new();
    // The mark centres on the wordmark's cap-to-baseline span, not on its
    // box: optical alignment, the way a lockup is actually set.
    let cap_top = WORDMARK_BASELINE - CAP_HEIGHT * WORDMARK_SIZE;
    ink.push_str(&brand::logo_mark(
        DISPLAY_X,
        (cap_top + WORDMARK_BASELINE) / 2.0 - brand::mark_height(LOCKUP_MARK_W) / 2.0,
        LOCKUP_MARK_W,
        theme.fg,
    ));
    ink.push_str(&display(
        DISPLAY_X + LOCKUP_MARK_W + 40.0,
        WORDMARK_BASELINE,
        WORDMARK_SIZE,
        theme.fg,
        "gitdebt",
    ));
    ink.push_str(&sans_text(
        CONTENT_X,
        436.0,
        40.0,
        theme.muted,
        "GitHub star history + repo-debt insights",
    ));
    ink.push_str(&colophon(theme, CONTENT_X));
    sheet(
        theme,
        &ink,
        "",
        "gitdebt — GitHub star history + repo-debt insights",
    )
}

// User profile card

/// Render the user-profile social card SVG (1200×630): the handle lettered
/// large, a mono footprint line, the persona on its own line, and the star
/// total as the headline value.
///
/// A profile has no time series, so this sheet has no object line and nothing
/// on it is dimensioned — which is exactly why it carries no drafting red.
/// Deterministic; only Postgres-derived [`UserCardData`] goes in.
pub fn render_user_og(data: &UserCardData, theme: &Theme) -> String {
    let mut ink = String::new();
    ink.push_str(&display(
        DISPLAY_X,
        SUBJECT_BASELINE,
        SUBJECT_SIZE,
        theme.fg,
        &escape_xml(&fit(
            &format!("@{}", data.login),
            SUBJECT_SIZE,
            CONTENT_RIGHT - DISPLAY_X,
        )),
    ));

    // Lower bounds over tracked repos — the card's honesty rule.
    let mut parts: Vec<String> = Vec::new();
    if data.commits > 0 {
        parts.push(format!("{} commits", fmt_count(data.commits)));
    }
    if data.contribs > 0 {
        parts.push(format!("{} contributed", fmt_count(data.contribs)));
    }
    parts.push(format!("{} repos tracked", fmt_count(data.repos_tracked)));
    ink.push_str(&mono_line(&parts.join("  ·  "), theme.muted));
    ink.push_str(&field_label(
        CONTENT_X,
        PERSONA_BASELINE,
        PERSONA_SIZE,
        theme.muted,
        &fit_label(
            crate::cards::user_persona(data),
            PERSONA_SIZE,
            CONTENT_RIGHT - CONTENT_X,
        ),
    ));
    ink.push_str(&value_block(&fmt_count(data.stars), "TOTAL STARS", theme));

    ink.push_str(&signature(theme));
    sheet(theme, &ink, "", &format!("gitdebt · @{}", data.login))
}

/// 1200×630 placeholder for a login gitdebt knows nothing about — mirrors the
/// user-card "no data yet" behavior at OG dimensions so social embeds
/// self-heal on the API layer's short TTL.
pub fn render_user_empty_og(login: &str, theme: &Theme) -> String {
    let mut ink = String::new();
    ink.push_str(&display(
        DISPLAY_X,
        SUBJECT_BASELINE,
        SUBJECT_SIZE,
        theme.fg,
        &escape_xml(&fit(
            &format!("@{login}"),
            SUBJECT_SIZE,
            CONTENT_RIGHT - DISPLAY_X,
        )),
    ));
    ink.push_str(&display(
        DISPLAY_X,
        VALUE_BASELINE - 60.0,
        64.0,
        theme.fg,
        "no gitdebt data yet",
    ));
    ink.push_str(&sans_text(
        CONTENT_X,
        VALUE_BASELINE,
        32.0,
        theme.muted,
        "analyze a repository at gitdebt.com to start tracking",
    ));
    ink.push_str(&signature(theme));
    sheet(theme, &ink, "", &format!("gitdebt · @{login}"))
}

// The plot

/// The band an object line is plotted in.
#[derive(Debug, Clone, Copy)]
struct Band {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl Band {
    fn single() -> Self {
        Self {
            left: CONTENT_X,
            right: BAND_RIGHT,
            top: BAND_TOP,
            bottom: BAND_BOTTOM,
        }
    }

    fn compare() -> Self {
        Self {
            right: COMPARE_BAND_RIGHT,
            ..Self::single()
        }
    }
}

/// The ranges a set of series is plotted against. Shared across every trace
/// on a sheet, so two repositories are compared on one scale.
#[derive(Debug, Clone, Copy)]
struct Range {
    x_min: f32,
    x_span: f32,
    y_max: f32,
}

impl Range {
    /// `None` when there is nothing to draw. `floor` lifts the vertical scale
    /// to at least that value, so a dimension measuring the reported total
    /// reads against the same axis the trace is drawn on.
    fn of(series: &[&[Point]], floor: u64) -> Option<Self> {
        let mut x_min = f32::INFINITY;
        let mut x_max = f32::NEG_INFINITY;
        let mut y_max = floor.max(1) as f32;
        let mut any = false;
        for points in series {
            for point in points.iter() {
                let x = point.at.timestamp() as f32;
                x_min = x_min.min(x);
                x_max = x_max.max(x);
                y_max = y_max.max(point.stars as f32);
                any = true;
            }
        }
        any.then(|| Self {
            x_min,
            x_span: (x_max - x_min).max(1.0),
            y_max,
        })
    }

    fn x(&self, band: &Band, at: f32) -> f32 {
        band.left + ((at - self.x_min) / self.x_span) * (band.right - band.left)
    }

    fn y(&self, band: &Band, stars: f32) -> f32 {
        band.bottom - (stars / self.y_max) * (band.bottom - band.top)
    }
}

/// The zero datum: the line the trace rises from and the dimension measures
/// up from. It spans exactly the plotted range and nothing more.
fn zero_datum(band: &Band, theme: &Theme) -> String {
    format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\" />",
        coord(du(band.left)),
        coord(du(band.bottom)),
        coord(du(band.right)),
        coord(du(band.bottom)),
        theme.grid,
        texture::W_OBJECT,
    )
}

/// The object line: the cumulative star trace, in drawing units.
fn trace(series: &[Point], band: &Band, range: &Range, ink: &str, weight: f32) -> String {
    let mut d = String::with_capacity(TRACE_SAMPLES * 18);
    for (nth, point) in sample(series).into_iter().enumerate() {
        d.push_str(if nth == 0 { "M" } else { "L" });
        d.push_str(&coord(du(range.x(band, point.at.timestamp() as f32))));
        d.push(' ');
        d.push_str(&coord(du(range.y(band, point.stars as f32))));
    }
    if d.is_empty() {
        return String::new();
    }
    format!(
        "<path d=\"{d}\" fill=\"none\" stroke=\"{ink}\" stroke-width=\"{weight}\" \
stroke-linejoin=\"miter\" stroke-linecap=\"butt\" />",
    )
}

/// Every point, or an evenly indexed sample of them that always keeps the
/// first and the last.
fn sample(series: &[Point]) -> Vec<&Point> {
    if series.len() <= TRACE_SAMPLES {
        return series.iter().collect();
    }
    let last = series.len() - 1;
    (0..TRACE_SAMPLES)
        .map(|i| &series[i * last / (TRACE_SAMPLES - 1)])
        .collect()
}

/// The one measured value on a single-repo sheet: a vertical dimension from
/// the zero datum up to the total, with an extension tick springing from each
/// of the two heights it spans.
///
/// The rule and its terminators are drafting red because they carry a
/// measurement. The extension lines are not a measurement, so they stay in
/// rule-strong.
fn star_dimension(stars: u64, band: &Band, range: &Range, theme: &Theme) -> String {
    let top = range.y(band, stars as f32);
    // Reach from the plot edge to a hair past the dimension line, the way an
    // extension line runs on a drawing.
    let reach = du(DIM_X - band.right) - texture::TICK_CLEARANCE + 1.0;
    let value = fmt_count(stars);
    let dimension = Dimension {
        value: &value,
        ink: theme.accent,
        ground: theme.bg,
        size: du(DIM_SIZE),
    };
    format!(
        "{}{}{}",
        texture::extension_tick(du(band.right), du(top), Side::Right, reach, theme.border),
        texture::extension_tick(
            du(band.right),
            du(band.bottom),
            Side::Right,
            reach,
            theme.border,
        ),
        texture::dimension_v(du(top), du(band.bottom), du(DIM_X), &dimension),
    )
}

// Lettering

/// The headline value on a sheet with no trace: the number lettered very
/// large in ink with its unit riding on the same baseline. A value and its
/// unit, never a label stacked over a heading.
///
/// The unit's room is reserved before the number is set, so the pair can
/// never run off the sheet: an absurd figure loses a digit to an ellipsis
/// rather than pushing its own unit past the margin.
fn value_block(value: &str, unit: &str, theme: &Theme) -> String {
    let unit_width = label_width(unit, UNIT_SIZE);
    let room = (CONTENT_RIGHT - DISPLAY_X - UNIT_GAP - unit_width).max(VALUE_SIZE);
    let value = fit_with(value, room, |run| figure_width(run, VALUE_SIZE));
    let x = DISPLAY_X + figure_width(&value, VALUE_SIZE) + UNIT_GAP;
    format!(
        "{}{}",
        display(
            DISPLAY_X,
            VALUE_BASELINE,
            VALUE_SIZE,
            theme.fg,
            &escape_xml(&value),
        ),
        field_label(x, VALUE_BASELINE, UNIT_SIZE, theme.ink_3, unit),
    )
}

/// The subject line of a repository sheet. The owner is the address and the
/// repository is the subject, so the owner drops a tonal step to ink-2 — an
/// emphasis made of value, never of colour.
fn subject_slug(slug: &str, theme: &Theme) -> String {
    let shown = fit(slug, SUBJECT_SIZE, CONTENT_RIGHT - DISPLAY_X);
    let body = match shown.split_once('/') {
        Some((owner, name)) => format!(
            "<tspan fill=\"{}\">{}/</tspan>{}",
            theme.muted,
            escape_xml(owner),
            escape_xml(name),
        ),
        None => escape_xml(&shown),
    };
    display(DISPLAY_X, SUBJECT_BASELINE, SUBJECT_SIZE, theme.fg, &body)
}

/// Display lettering. `body` is already escaped, so a caller may hand it
/// tspans.
fn display(x: f32, baseline: f32, size: f32, fill: &str, body: &str) -> String {
    format!(
        "  <text x=\"{}\" y=\"{}\" fill=\"{fill}\" font-family=\"{SANS}\" font-size=\"{}\" \
font-weight=\"700\" letter-spacing=\"-0.015em\">{body}</text>\n",
        coord(x),
        coord(baseline),
        coord(size),
    )
}

/// Body lettering in the sans stack. `body` is already escaped.
fn sans_text(x: f32, baseline: f32, size: f32, fill: &str, body: &str) -> String {
    format!(
        "  <text x=\"{}\" y=\"{}\" fill=\"{fill}\" font-family=\"{SANS}\" \
font-size=\"{}\">{body}</text>\n",
        coord(x),
        coord(baseline),
        coord(size),
    )
}

/// An uppercase, tracked-out field label.
fn field_label(x: f32, baseline: f32, size: f32, fill: &str, label: &str) -> String {
    format!(
        "  <text x=\"{}\" y=\"{}\" fill=\"{fill}\" font-family=\"{SANS}\" font-size=\"{}\" \
letter-spacing=\"{}\">{}</text>\n",
        coord(x),
        coord(baseline),
        coord(size),
        texture::LABEL_TRACKING,
        escape_xml(&label.to_uppercase()),
    )
}

/// The quiet mono line under the subject: tabular, so two cards' figures line
/// up when they sit next to each other in a feed.
fn mono_line(body: &str, fill: &str) -> String {
    let shown = fit_mono(body, SECONDARY_SIZE, CONTENT_RIGHT - CONTENT_X);
    format!(
        "  <text x=\"{}\" y=\"{}\" fill=\"{fill}\" font-family=\"{MONO}\" font-size=\"{}\" \
font-variant-numeric=\"tabular-nums\">{}</text>\n",
        coord(CONTENT_X),
        coord(SECONDARY_BASELINE),
        coord(SECONDARY_SIZE),
        escape_xml(&shown),
    )
}

/// The domain alone, in the bottom-left corner. `x` is the margin it hangs
/// from: beside the mark on a subject sheet, on the content margin when it
/// stands by itself.
fn colophon(theme: &Theme, x: f32) -> String {
    format!(
        "  <text x=\"{}\" y=\"{}\" fill=\"{}\" font-family=\"{MONO}\" font-size=\"{}\" \
letter-spacing=\"0.02em\">gitdebt.com</text>\n",
        coord(x),
        coord(SIG_TEXT_BASELINE),
        theme.muted,
        coord(SIG_TEXT_SIZE),
    )
}

/// The signature: the mark small in the bottom-left corner with the domain
/// beside it. The one place the brand appears on a subject sheet.
fn signature(theme: &Theme) -> String {
    format!(
        "{}{}",
        brand::logo_mark(SIG_MARK_X, SIG_MARK_Y, SIG_MARK_W, theme.muted),
        colophon(theme, SIG_TEXT_X),
    )
}

/// Assemble a sheet: the paper, the frame, and the notation plotted at
/// [`PLOT`]:1 under the lettering.
///
/// `notation` is authored in drawing units and goes inside the scaled group;
/// `ink` is lettering authored in card units and stays outside it, so a
/// glyph's size is stated once, in the units the card is measured in. Each
/// renderer signs its own sheet, because the title sheet signs itself
/// differently from a subject sheet.
fn sheet(theme: &Theme, ink: &str, notation: &str, aria_label: &str) -> String {
    let frame = format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{}\" \
stroke-width=\"{}\" />",
        coord(du(FRAME_INSET)),
        coord(du(FRAME_INSET)),
        coord(du(OG_WIDTH as f32 - 2.0 * FRAME_INSET)),
        coord(du(OG_HEIGHT as f32 - 2.0 * FRAME_INSET)),
        theme.border,
        texture::W_OBJECT,
    );
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-label="{label}">
  <rect x="0" y="0" width="{w}" height="{h}" fill="{paper}" />
  <g transform="scale({plot})">{frame}{notation}</g>
{ink}</svg>"##,
        w = OG_WIDTH,
        h = OG_HEIGHT,
        paper = theme.bg,
        plot = coord(PLOT),
        label = escape_xml(aria_label),
    )
}

/// Trim the owner from `owner/repo` to keep compare labels short; falls back
/// to the full slug when there's no slash.
fn short_slug(slug: &str) -> String {
    match slug.split_once('/') {
        Some((_, repo)) => repo.to_string(),
        None => slug.to_string(),
    }
}

// Fitting

/// Estimated advance (em) of one glyph in the display face. The rasterizer
/// resolves the stack onto bundled Inter, whose mixed-slug average is
/// ~0.56em. Estimation only has to be safe, not exact: the wide classes err
/// generous so an adversarial all-'M' slug cannot blow the budget the way a
/// flat average would, and over-estimation merely truncates a glyph early.
fn advance_em(c: char) -> f32 {
    match c {
        'm' | 'w' | 'M' | 'W' | '…' => 1.0,
        'i' | 'j' | 'l' | 'f' | 't' | 'r' | 'I' | '.' | '-' | '_' | ' ' | '/' => 0.45,
        'A'..='Z' => 0.80,
        '0'..='9' => 0.65,
        _ => 0.62,
    }
}

/// Estimated pixel width of a display glyph run at `size`.
fn display_width(text: &str, size: f32) -> f32 {
    text.chars().map(|c| advance_em(c) * size).sum()
}

/// Estimated width of a compact figure — the only run [`value_block`]
/// letters. The glyph set is known (digits, at most one period, at most one
/// k/M/B suffix), so this measures far closer than [`display_width`], which
/// stays deliberately generous because a truncation budget must never
/// under-shoot. A figure is *placed*, and placement wants the true width: an
/// over-estimate here is a visible gap between a number and its unit.
fn figure_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| match c {
            '0'..='9' => 0.60,
            '.' => 0.25,
            'M' | 'B' => 0.72,
            _ => 0.55,
        })
        .sum::<f32>()
        * size
}

/// Estimated width of a tracked-out field label: the glyph run plus the
/// tracking the label carries between every pair.
fn label_width(text: &str, size: f32) -> f32 {
    display_width(text, size) + text.chars().count() as f32 * size * 0.09
}

/// Estimated width of a monospace run. One advance for every character.
fn mono_width(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.6
}

/// Truncate `text` with a trailing ellipsis until `width` says it fits
/// `budget`. Pure and deterministic; returns the input unchanged when it
/// already fits, and the bare ellipsis when not even one glyph does.
fn fit_with(text: &str, budget: f32, width: impl Fn(&str) -> f32) -> String {
    if width(text) <= budget {
        return text.to_string();
    }
    let mut out = String::new();
    let mut probe = String::new();
    for c in text.chars() {
        probe.clear();
        probe.push_str(&out);
        probe.push(c);
        probe.push('…');
        if width(&probe) > budget {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

fn fit(text: &str, size: f32, budget: f32) -> String {
    fit_with(text, budget, |run| display_width(run, size))
}

fn fit_label(text: &str, size: f32, budget: f32) -> String {
    fit_with(text, budget, |run| label_width(run, size))
}

fn fit_mono(text: &str, size: f32, budget: f32) -> String {
    fit_with(text, budget, |run| mono_width(run, size))
}

/// Compact integer formatting (1234 → "1.2k", 1_500_000 → "1.5M",
/// 2_000_000_000 → "2.0B"). Matches `chart::fmt_count_u64` so card numbers
/// read identically to the chart axes. Local copy because the chart helper is
/// private.
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

    fn repo(series: Vec<Point>) -> RepoCard {
        RepoCard {
            slug: "facebook/react".into(),
            stars: 234_567,
            forks: Some(48_000),
            downloads: Some((21_000_000, "npm".into())),
            series,
        }
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

    fn compare_entries(n: usize) -> Vec<CompareEntry> {
        (0..n)
            .map(|i| CompareEntry {
                slug: format!("owner/repo-{i:02}"),
                stars: 100 + i as u64,
                series: sample_series(5 + i as i64),
            })
            .collect()
    }

    /// The byte range the plotted notation group occupies, found by counting
    /// nested groups — the notation vocabulary emits groups of its own, so
    /// the first `</g>` is not the one that closes the plot.
    fn plotted_span(svg: &str) -> std::ops::Range<usize> {
        let start = svg.find("<g transform=\"scale(").expect("plotted group");
        let bytes = svg.as_bytes();
        let mut depth = 0i32;
        let mut i = start;
        while i < bytes.len() {
            if bytes[i..].starts_with(b"<g") {
                depth += 1;
                i += 2;
            } else if bytes[i..].starts_with(b"</g>") {
                depth -= 1;
                i += 4;
                if depth == 0 {
                    return start..i;
                }
            } else {
                i += 1;
            }
        }
        panic!("plotted group never closes");
    }

    /// Every `<text>` on the sheet as `(x, y, content)` in CARD units.
    /// Notation letters inside the plotted group, so its coordinates are
    /// lifted back out of drawing units before anything compares them with
    /// the sheet's own geometry.
    fn text_elements(svg: &str) -> Vec<(f32, f32, String)> {
        let plotted = plotted_span(svg);
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
            let factor = if plotted.contains(&start) { PLOT } else { 1.0 };
            let content_end = rest.find("</text>").expect("text element closes");
            out.push((
                attr("x") * factor,
                attr("y") * factor,
                rest[attrs_end + 1..content_end].to_string(),
            ));
        }
        out
    }

    #[test]
    fn repo_card_has_correct_dimensions() {
        // The declared OG dims and the rasterized PNG both depend on a
        // 1200×630 viewBox — if this drifts, social previews letterbox.
        let svg = render_repo_card(&repo(sample_series(20)), &DARK);
        assert!(svg.contains("viewBox=\"0 0 1200 630\""));
        assert!(svg.contains("width=\"1200\""));
        assert!(svg.contains("height=\"630\""));
    }

    #[test]
    fn repo_card_letters_the_slug_and_measures_the_total() {
        let svg = render_repo_card(&repo(sample_series(30)), &DARK);
        // The slug is the subject, with the owner a tonal step back.
        assert!(svg.contains(">facebook/</tspan>react<"));
        // The total is lettered on its dimension line and nowhere else.
        assert_eq!(svg.matches("234.6k").count(), 1);
        assert!(svg.contains("48.0k forks"));
        assert!(svg.contains("21.0M npm downloads"));
        assert!(svg.contains("gitdebt.com"));
    }

    #[test]
    fn cards_are_deterministic() {
        let card = repo(sample_series(15));
        for theme in [&LIGHT, &DARK] {
            assert_eq!(
                render_repo_card(&card, theme),
                render_repo_card(&card, theme)
            );
            assert_eq!(
                render_compare_card(&compare_entries(3), theme),
                render_compare_card(&compare_entries(3), theme),
            );
            assert_eq!(
                render_user_og(&sample_user_data(), theme),
                render_user_og(&sample_user_data(), theme),
            );
            assert_eq!(render_default_card(theme), render_default_card(theme));
        }
    }

    #[test]
    fn repo_card_omits_missing_pieces_gracefully() {
        // No forks, no downloads, no series — still a valid sheet, and with
        // nothing to trace the total is lettered instead of measured.
        let card = RepoCard {
            slug: "lonely/repo".into(),
            stars: 7,
            forks: None,
            downloads: None,
            series: Vec::new(),
        };
        let svg = render_repo_card(&card, &LIGHT);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains(">lonely/</tspan>repo<"));
        assert!(svg.contains(">7</text>"));
        assert!(svg.contains(">GITHUB STARS<"));
        assert!(!svg.contains("forks"));
        assert!(!svg.contains("downloads"));
        // No notation was invented for data that does not exist: the plotted
        // group holds the sheet frame and nothing else.
        let plotted = &svg[plotted_span(&svg)];
        assert_eq!(plotted.matches("<rect").count(), 1);
        assert!(!plotted.contains("<path") && !plotted.contains("<line"));
    }

    /// The exact drafting palette, baked. A card never ships a CSS variable:
    /// social previews do not theme-switch.
    #[test]
    fn cards_bake_the_drafting_palette() {
        let light = render_repo_card(&repo(sample_series(10)), &LIGHT);
        assert!(
            light.contains(r##"<rect x="0" y="0" width="1200" height="630" fill="#ffffff" />"##)
        );
        assert!(light.contains("fill=\"#111417\""), "graphite lettering");
        assert!(light.contains("fill=\"#4f5357\""), "ink-2 for the owner");
        assert!(light.contains("stroke=\"#c2c4c7\""), "rule-strong frame");
        assert!(light.contains("stroke=\"#dcdee0\""), "the zero datum");
        assert!(light.contains("#cc291f"), "the trace is drafting red");

        let dark = render_repo_card(&repo(sample_series(10)), &DARK);
        assert!(
            dark.contains(r##"<rect x="0" y="0" width="1200" height="630" fill="#0c0f11" />"##)
        );
        assert!(dark.contains("fill=\"#e6e8ea\""));
        assert!(dark.contains("#f0674e"));

        for svg in [light, dark] {
            assert!(!svg.contains("var(--"));
        }
    }

    #[test]
    fn cards_reuse_the_shared_font_stacks() {
        // The PNG only renders text if the rasterizer can resolve the font.
        // These are the exact generic stacks resvg maps onto bundled Inter.
        let svg = render_repo_card(&repo(sample_series(4)), &DARK);
        assert!(svg.contains("ui-sans-serif, system-ui, sans-serif"));
        assert!(svg.contains("ui-monospace"));
        for banned in ["@font-face", "https://fonts", ".woff", "data:font"] {
            assert!(!svg.contains(banned));
        }
    }

    /// The drawing has three pen widths and a card plots them at 3:1, so
    /// every stroke on the sheet is authored inside the scaled group and
    /// nothing is hand-thickened to compensate.
    ///
    /// Lettering is exempt: `cut_text` strokes its glyphs in the ground
    /// colour to cut the rule it sits on, which is a halo, not a pen.
    #[test]
    fn every_drawn_stroke_is_one_of_the_three_pen_widths() {
        for svg in [
            render_repo_card(&repo(sample_series(30)), &LIGHT),
            render_compare_card(&compare_entries(3), &LIGHT),
            render_user_og(&sample_user_data(), &DARK),
            render_default_card(&DARK),
        ] {
            assert!(svg.contains("<g transform=\"scale(3.00)\">"));
            for (index, _) in svg.match_indices("stroke-width=\"") {
                let tag = svg[..index].rfind('<').expect("attribute inside a tag");
                if svg[tag..].starts_with("<text") {
                    continue;
                }
                let from = index + "stroke-width=\"".len();
                let to = svg[from..].find('"').expect("attr closes") + from;
                assert!(
                    ["0.5", "1", "2"].contains(&&svg[from..to]),
                    "pen width {:?} is not one of the three",
                    &svg[from..to],
                );
            }
        }
    }

    /// The trace is the primary data line, so it is drafting red at the
    /// emphasis weight; the zero datum it rises from is a hairline.
    #[test]
    fn the_trace_is_the_emphasis_pen_in_drafting_red() {
        let svg = render_repo_card(&repo(sample_series(40)), &LIGHT);
        assert!(svg.contains(&format!("stroke=\"{}\" stroke-width=\"2\"", LIGHT.accent)));
        assert!(svg.contains(&format!("stroke=\"{}\" stroke-width=\"1\"", LIGHT.grid)));
    }

    /// One measured value, dimensioned up from the zero datum, with a
    /// terminator at each end and the value cutting its own rule.
    #[test]
    fn the_total_is_the_one_dimensioned_value() {
        let svg = render_repo_card(&repo(sample_series(40)), &LIGHT);
        assert!(svg.contains("paint-order=\"stroke\""));
        assert!(svg.contains("rotate(-90"));
        // The trace plus exactly two terminators: this sheet measures one
        // thing. (The canonical mark's own path writes `fill` before `d`.)
        assert_eq!(svg.matches("<path d=\"M").count(), 3);
        // Both extension lines are rule-strong construction, never red.
        assert_eq!(
            svg.matches(&format!("stroke=\"{}\" stroke-width=\"0.5\"", LIGHT.border))
                .count(),
            2
        );
        // The value is cut back to the paper it sits on, not to a box.
        assert!(svg.contains(&format!("stroke=\"{}\"", LIGHT.bg)));
    }

    /// Nothing on the sheet may be rounded, glowing, textured or filled with
    /// a paint server. The whole visual world, in one assertion.
    #[test]
    fn the_sheet_carries_no_texture_gradient_or_glow() {
        for svg in [
            render_repo_card(&repo(sample_series(20)), &DARK),
            render_compare_card(&compare_entries(4), &LIGHT),
            render_user_og(&sample_user_data(), &LIGHT),
            render_user_empty_og("ghost", &DARK),
            render_default_card(&LIGHT),
        ] {
            for banned in [
                "gd-dither-wave",
                "gd-pixel-fill",
                "gd-pixel-field",
                "gd-t1",
                "data-gitdebt-ramp",
                "<pattern",
                "linearGradient",
                "radialGradient",
                "<filter",
                "feGaussianBlur",
                "opacity=",
                "url(#",
                "rx=",
                "ry=",
                "<animate",
            ] {
                assert!(!svg.contains(banned), "{banned} survived");
            }
        }
    }

    #[test]
    fn compare_card_labels_every_trace_at_its_own_line_end() {
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
        assert!(svg.contains("vue vs react"));
        // Each series is labelled at its own end, so hue is never the sole
        // carrier of meaning.
        assert!(svg.contains("vue 207.0k"));
        assert!(svg.contains("react 234.0k"));
        // Two distinct plotter pens, and neither of them is drafting red.
        let pens = theme::pens_for(&DARK, &["vuejs/vue", "facebook/react"]);
        assert_ne!(pens[0], pens[1]);
        for pen in &pens {
            assert!(svg.contains(*pen));
            assert_ne!(*pen, DARK.accent);
        }
        assert!(!svg.contains(DARK.accent), "a compare sheet spends no red");
    }

    /// One leader per trace, each with a single terminator at its datum end.
    /// A second line would be pointing at nothing.
    #[test]
    fn each_compare_trace_gets_one_leader() {
        let svg = render_compare_card(&compare_entries(3), &LIGHT);
        assert_eq!(svg.matches("<path d=\"M").count(), 3 + 3);
    }

    /// Labels that would land on each other are pushed apart, in order, and
    /// the run stays inside the band.
    #[test]
    fn compare_labels_never_collide() {
        let placed = spread(&[400.0; 5], COMPARE_LABEL_GAP, BAND_TOP, BAND_BOTTOM);
        let mut sorted = placed.clone();
        sorted.sort_by(f32::total_cmp);
        for pair in sorted.windows(2) {
            assert!(
                pair[1] - pair[0] >= COMPARE_LABEL_GAP - 0.01,
                "labels {pair:?} overlap"
            );
        }
        assert!(sorted[4] <= BAND_BOTTOM + 0.01);
        // A lone label sits exactly at its own line end.
        assert_eq!(
            spread(&[400.0], COMPARE_LABEL_GAP, 100.0, 500.0),
            vec![400.0]
        );
        assert!(spread(&[], COMPARE_LABEL_GAP, 0.0, 1.0).is_empty());
        // Order is preserved: the line that finishes highest labels highest.
        let placed = spread(&[500.0, 300.0, 505.0], COMPARE_LABEL_GAP, 100.0, 900.0);
        assert!(placed[1] < placed[0] && placed[0] < placed[2]);
    }

    /// A 12-repo compare cannot draw twelve legible traces, so the tail
    /// collapses into one "+N more" line and every label stays on the sheet.
    #[test]
    fn compare_card_caps_traces_and_adds_a_more_line() {
        let svg = render_compare_card(&compare_entries(12), &DARK);
        for i in 0..COMPARE_MAX_TRACES - 1 {
            assert!(svg.contains(&format!("repo-{i:02} ")));
        }
        assert!(!svg.contains("repo-04 "), "past the cap must be omitted");
        assert!(svg.contains(">+8 more<"));
        assert_eq!(svg, render_compare_card(&compare_entries(12), &DARK));
    }

    #[test]
    fn compare_card_five_traces_fit_six_truncate() {
        let five = render_compare_card(&compare_entries(5), &DARK);
        for i in 0..5 {
            assert!(five.contains(&format!("repo-{i:02} ")));
        }
        assert!(!five.contains(" more<"));

        let six = render_compare_card(&compare_entries(6), &DARK);
        assert!(six.contains("repo-03 "));
        assert!(!six.contains("repo-04 "));
        assert!(six.contains(">+2 more<"));
    }

    /// A many-slug title truncates with an ellipsis and its estimated glyph
    /// run never crosses the mirrored right margin.
    #[test]
    fn compare_title_truncates_with_ellipsis_inside_the_sheet() {
        let entries: Vec<CompareEntry> = (0..12)
            .map(|i| CompareEntry {
                slug: format!("owner/some-rather-long-repository-name-{i:02}"),
                stars: 10,
                series: sample_series(4),
            })
            .collect();
        let svg = render_compare_card(&entries, &DARK);
        let (x, _, title) = text_elements(&svg)
            .into_iter()
            .find(|(_, y, _)| (*y - SUBJECT_BASELINE).abs() < 0.01)
            .expect("the subject line");
        assert!(
            title.contains('…'),
            "long title must be truncated: {title:?}"
        );
        assert_eq!(x, DISPLAY_X);
        let end = x + display_width(&title, SUBJECT_SIZE);
        assert!(end <= CONTENT_RIGHT, "title run ends at {end}");
        assert_eq!(svg, render_compare_card(&entries, &DARK));
    }

    #[test]
    fn short_runs_are_never_truncated() {
        let svg = render_compare_card(&compare_entries(2), &DARK);
        assert!(!svg.contains('…'));
        assert!(svg.contains("repo-00 vs repo-01"));
        assert_eq!(fit("vue vs react", SUBJECT_SIZE, 1000.0), "vue vs react");
        assert_eq!(fit_label("TOTAL STARS", UNIT_SIZE, 400.0), "TOTAL STARS");
        assert_eq!(
            fit_mono("987 commits", SECONDARY_SIZE, 400.0),
            "987 commits"
        );
        // Nothing fits: the ellipsis alone, never a panic.
        assert_eq!(fit("react", SUBJECT_SIZE, 1.0), "…");
    }

    /// Raster-level verification of the subject budget: adversarially wide
    /// glyphs ('M'/'w' runs — the widest classes) must leave the right gutter
    /// of the subject band untouched in the rendered PNG. A flat
    /// average-advance estimate would let these run past the edge.
    #[test]
    fn a_long_subject_rasterizes_without_right_edge_overflow() {
        use crate::raster::{RasterFormat, rasterize};
        use resvg::tiny_skia::Pixmap;

        for glyph in ['M', 'w', 'o'] {
            let name: String = std::iter::repeat_n(glyph, 40).collect();
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
            let svg = render_compare_card(&entries, &LIGHT);
            let png = rasterize(&svg, RasterFormat::Png, 1.0).expect("rasterize compare card");
            let pixmap = Pixmap::decode_png(&png).expect("decode png");
            // The subject band's right gutter, inside the frame: bare paper.
            for y in 110..=190 {
                for x in 1145..1166 {
                    let p = pixmap.pixel(x, y).expect("pixel in bounds");
                    assert!(
                        p.red() > 240 && p.green() > 240 && p.blue() > 240,
                        "ink at ({x},{y}) for {glyph:?} — the fit budget is too loose"
                    );
                }
            }
        }
    }

    #[test]
    fn default_card_is_the_title_sheet() {
        let svg = render_default_card(&DARK);
        assert!(svg.contains("viewBox=\"0 0 1200 630\""));
        assert!(svg.contains(">gitdebt</text>"));
        assert!(svg.contains("GitHub star history + repo-debt insights"));
        assert!(svg.contains("M320.5 110.5"), "the canonical mark travels");
        assert!(!svg.contains("<image"));
    }

    #[test]
    fn light_and_dark_are_two_prints_of_one_drawing() {
        let light = render_default_card(&LIGHT);
        let dark = render_default_card(&DARK);
        assert!(light.starts_with("<svg") && dark.starts_with("<svg"));
        assert!(light.contains("fill=\"#ffffff\"") && light.contains("fill=\"#111417\""));
        assert!(dark.contains("fill=\"#0c0f11\"") && dark.contains("fill=\"#e6e8ea\""));
        assert_ne!(light, dark);
    }

    /// The mark in the signature is the repository's own artwork, not a
    /// redrawn glyph and not a filled chip.
    #[test]
    fn the_signature_carries_the_canonical_mark() {
        for theme in [&LIGHT, &DARK] {
            let svg = render_user_og(&sample_user_data(), theme);
            assert!(svg.contains("data-gitdebt-logo=\"true\""));
            assert!(svg.contains("M320.5 110.5"));
            let place = crate::brand::MarkBox::locate(&svg, 2.0, theme.muted, theme.bg);
            let (mismatch, ink) = crate::brand::mark_fidelity(&svg, place);
            assert!(mismatch < 0.05, "signature mark drifted: {mismatch:.3}");
            assert!((0.25..0.75).contains(&ink));
        }
    }

    #[test]
    fn xml_is_escaped_on_every_text_path() {
        let card = RepoCard {
            slug: "<script>/x&y".into(),
            stars: 1,
            ..Default::default()
        };
        let svg = render_repo_card(&card, &DARK);
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;") && svg.contains("x&amp;y"));

        let compare = render_compare_card(
            &[CompareEntry {
                slug: "o/<b>".into(),
                stars: 2,
                series: sample_series(3),
            }],
            &LIGHT,
        );
        assert!(!compare.contains("<b>") && compare.contains("&lt;b&gt;"));
    }

    #[test]
    fn fmt_count_humanizes() {
        assert_eq!(fmt_count(7), "7");
        assert_eq!(fmt_count(12_345), "12.3k");
        assert_eq!(fmt_count(1_500_000), "1.5M");
        assert_eq!(fmt_count(2_000_000_000), "2.0B");
    }

    /// A huge series must not turn one card into a multi-megabyte path, and
    /// the sample must keep the two points that matter.
    #[test]
    fn a_long_series_is_sampled_down_keeping_both_ends() {
        let long = sample_series(4_000);
        let picked = sample(&long);
        assert_eq!(picked.len(), TRACE_SAMPLES);
        assert_eq!(picked[0].stars, long[0].stars);
        assert_eq!(picked[TRACE_SAMPLES - 1].stars, long[long.len() - 1].stars);
        for pair in picked.windows(2) {
            assert!(pair[0].at <= pair[1].at, "the trace must not double back");
        }
        assert_eq!(sample(&sample_series(9)).len(), 9);

        let svg = render_repo_card(&repo(long), &LIGHT);
        assert!(svg.len() < 40_000, "card svg is {} bytes", svg.len());
    }

    #[test]
    fn user_og_letters_the_total_and_measures_nothing() {
        let svg = render_user_og(&sample_user_data(), &DARK);
        assert!(svg.contains("viewBox=\"0 0 1200 630\""));
        assert!(svg.contains("width=\"1200\"") && svg.contains("height=\"630\""));
        assert!(svg.contains("@octocat"));
        assert!(svg.contains(">12.3k</text>") && svg.contains(">TOTAL STARS<"));
        assert!(svg.contains("987 commits") && svg.contains("8 repos tracked"));
        // The persona is a classification of the account, so it sits on its
        // own line and never gets crushed against the number.
        assert!(svg.contains(">OPEN SOURCE BUILDER<"));
        assert_eq!(
            text_elements(&svg)
                .into_iter()
                .filter(|(_, y, _)| (*y - PERSONA_BASELINE).abs() < 0.01)
                .count(),
            1
        );
        // A profile measures nothing against a datum, so it spends no red and
        // draws no notation but the frame.
        assert!(!svg.contains(DARK.accent));
        let plotted = &svg[plotted_span(&svg)];
        assert!(!plotted.contains("<path") && !plotted.contains("<line"));
    }

    /// The unit label rides on the value's baseline and never lands on the
    /// numeral, whatever the numeral's width, and never leaves the sheet.
    #[test]
    fn the_unit_label_clears_the_value() {
        for stars in [7_u64, 999, 12_345, 999_999_999, u64::MAX] {
            let data = UserCardData {
                stars,
                ..sample_user_data()
            };
            let svg = render_user_og(&data, &DARK);
            let on_baseline: Vec<(f32, f32, String)> = text_elements(&svg)
                .into_iter()
                .filter(|(_, y, _)| (*y - VALUE_BASELINE).abs() < 0.01)
                .collect();
            let numeral = on_baseline
                .iter()
                .find(|(x, _, _)| *x == DISPLAY_X)
                .expect("the value");
            let unit = on_baseline
                .iter()
                .find(|(x, _, _)| *x > DISPLAY_X)
                .expect("the unit label");
            assert!(
                unit.0 >= numeral.0 + figure_width(&numeral.2, VALUE_SIZE),
                "unit at {} lands on a {}-wide value",
                unit.0,
                figure_width(&numeral.2, VALUE_SIZE),
            );
            assert!(unit.0 + label_width(&unit.2, UNIT_SIZE) <= CONTENT_RIGHT + 0.01);
        }
    }

    #[test]
    fn user_empty_og_renders_a_placeholder_sheet() {
        let svg = render_user_empty_og("ghost", &DARK);
        assert!(svg.contains("viewBox=\"0 0 1200 630\""));
        assert!(svg.contains("@ghost"));
        assert!(svg.contains("no gitdebt data yet"));
        assert_eq!(svg, render_user_empty_og("ghost", &DARK));
        let evil = render_user_empty_og("<script>", &DARK);
        assert!(!evil.contains("<script>"));
    }

    #[test]
    fn og_rasterizes_to_exactly_1200x630_png_with_text() {
        use crate::raster::{RasterFormat, rasterize};

        let svg = render_repo_card(&repo(sample_series(40)), &LIGHT);
        // Scale 1.0 → PNG is exactly the SVG's 1200×630 viewBox.
        let png = rasterize(&svg, RasterFormat::Png, 1.0).expect("rasterize og png");
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        // IHDR width/height are big-endian u32 at byte offsets 16 and 20.
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!((w, h), (OG_WIDTH, OG_HEIGHT), "OG PNG must be 1200×630");

        // Text-renders check without an optional PNG decoder: a sheet whose
        // lettering actually rasterized compresses to a materially larger PNG
        // than bare paper. If the font failed to resolve (blank glyph boxes)
        // the body would collapse toward it.
        let blank = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="{paper}" /></svg>"##,
            w = OG_WIDTH,
            h = OG_HEIGHT,
            paper = LIGHT.bg,
        );
        let blank_png = rasterize(&blank, RasterFormat::Png, 1.0).expect("rasterize blank");
        assert!(
            png.len() > blank_png.len() * 3,
            "rendered sheet ({}) should dwarf bare paper ({})",
            png.len(),
            blank_png.len(),
        );

        let user = rasterize(
            &render_user_og(&sample_user_data(), &DARK),
            RasterFormat::Png,
            1.0,
        )
        .expect("user og png");
        let w = u32::from_be_bytes([user[16], user[17], user[18], user[19]]);
        let h = u32::from_be_bytes([user[20], user[21], user[22], user[23]]);
        assert_eq!((w, h), (OG_WIDTH, OG_HEIGHT));
    }

    /// Nothing is lettered against the trimmed edge: the frame clears every
    /// glyph on every sheet, in card units.
    #[test]
    fn every_glyph_sits_inside_the_frame() {
        for svg in [
            render_repo_card(&repo(sample_series(30)), &LIGHT),
            render_compare_card(&compare_entries(5), &LIGHT),
            render_compare_card(&compare_entries(9), &LIGHT),
            render_user_og(&sample_user_data(), &LIGHT),
            render_user_empty_og("ghost", &LIGHT),
            render_default_card(&LIGHT),
        ] {
            for (x, y, content) in text_elements(&svg) {
                assert!(
                    x >= FRAME_INSET && x <= OG_WIDTH as f32 - FRAME_INSET,
                    "{content:?} starts at x={x}"
                );
                assert!(
                    y >= FRAME_INSET && y <= OG_HEIGHT as f32 - FRAME_INSET,
                    "{content:?} sits at y={y}"
                );
            }
        }
    }
}
