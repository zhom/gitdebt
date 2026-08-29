//! Star-history sheets: a single cumulative star trace, a multi-repo
//! overlay, and a stars-versus-usage dual-ordinate sheet.
//!
//! Pure: a time series → string. No external SVG library — the markup is
//! short enough to write by hand and we want it deterministic for
//! caching/embedding (same input → same bytes).
//!
//! # The drawing
//!
//! Every asset here is one sheet of a dimensioned engineering drawing, and
//! that is not a coat of paint: it decides what may be on the sheet at all.
//! **Every line terminates on something real.** The abscissa is the baseline
//! the trace is measured from; the ordinate is the axis it is measured
//! against; an extension tick springs from a datum; a dimension line spans
//! two measured points and letters its own value; a leader points at one
//! datum. There is no shaded region under the curve, no gradient, no dither,
//! no glow, no rule laid across the plot to be read against. A drawing plots
//! a line — it does not shade a region, and a horizontal rule at every
//! gradation is graph paper, which measures nothing.
//!
//! Three renderers:
//!   * [`render_svg`] — one repo, one graphite trace. Draws itself in only
//!     when [`ChartOpts::animate`] is explicitly enabled.
//!   * [`render_multi_svg`] — N repos on shared axes, each in its own
//!     plotter pen AND labelled at its own line end, so hue is never the
//!     sole carrier of meaning.
//!   * [`render_overlay_svg`] — stars against package downloads on two
//!     ordinates. Always static: an embed's motion is the reader's call.
//!
//! Drafting red is spent on exactly one thing per sheet: the measured value
//! on the final datum (or, in a looping frame, the travelling station that
//! replaces it). It is never a category color, which is why the multi-repo
//! pens come from [`theme::pens_for`] — that accessor cannot hand out the
//! reserved signal pen.
//!
//! Theme colors are baked as concrete hex (no CSS vars) so the SVG renders
//! correctly when embedded as an `<img>` regardless of OS / page theme.

use std::borrow::Cow;

use chrono::{DateTime, Datelike, Utc};
use serde::Serialize;

use crate::brand;
use crate::texture::{
    self, Dimension, MONO, SANS, Side, TitleField, W_EMPHASIS, W_OBJECT, escape_xml,
};
use crate::theme::{self, Theme};

#[derive(Debug, Clone, Serialize)]
pub struct Point {
    /// Serialized as `date` to match the `/analyze` JSON contract.
    #[serde(rename = "date")]
    pub at: DateTime<Utc>,
    /// Cumulative total stars up to (and including) this timestamp.
    pub stars: u32,
}

/// More than one point per horizontal pixel cannot add visible fidelity to
/// the 1200px chart, but it can turn popular repositories into multi-megabyte
/// SVGs. A little oversampling preserves sharp bursts without shipping every
/// archived event to the browser or rasterizer.
const MAX_RENDER_POINTS: usize = 1_600;

// Sheet layout. Every offset below is measured from the plot's own edges, so
// the notation keeps its spacing at any configured width or height.

/// How far the span dimension line sits below the baseline it measures.
const SPAN_DROP: f32 = 26.0;
/// Reach of the two extension ticks that carry the span dimension. Long
/// enough to cross the dimension line and stand a little past it, which is
/// what tells the reader the tick is an extension and not part of the object.
const SPAN_TICK: f32 = 30.0;
/// Reach of an interior gradation tick on the abscissa.
const GRAD_TICK: f32 = 6.0;
/// Baseline of an abscissa gradation's lettering, below the baseline.
const X_LABEL_DROP: f32 = 46.0;
/// Baseline of the sheet title, above the plot.
const TITLE_RISE: f32 = 22.0;
/// Baseline of the final value's leader label, above the plot.
const LEADER_RISE: f32 = 18.0;
/// Width of the title block in the bottom-right corner of the plot.
const TITLE_BLOCK_W: f32 = 268.0;
/// Clearance between the title block's lower edge and the baseline.
const TITLE_BLOCK_LIFT: f32 = 12.0;
/// Lettering sizes. Values are tabular and set in the mono stack; labels are
/// uppercase and tracked, which [`texture::title_block`] handles itself.
const GRAD_SIZE: f32 = 11.0;
const SPAN_VALUE_SIZE: f32 = 11.0;
const LEADER_SIZE: f32 = 12.0;
const SERIES_LABEL_SIZE: f32 = 11.0;
/// Gap between an axis tick and the value lettered outside it.
const GRAD_GUTTER: f32 = 5.0;
/// How far a line-end label sits above the datum it names, how far two of
/// them stand apart when their line ends are too close, and the highest a
/// stack of them may climb before its lettering would sit hard against the
/// sheet's own top edge.
const LABEL_RISE: f32 = 16.0;
const LABEL_STACK: f32 = 17.0;
const LABEL_FLOOR: f32 = 20.0;

/// X-axis alignment for the chart.
///   * `Date` — absolute x-axis by timestamp (default). Repos appear at
///     their real calendar positions.
///   * `Timeline` — x-axis is days-since-each-repo's-first-star, so repos
///     of different ages line up at day 0 and growth shapes can be
///     compared directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeAxis {
    Date,
    Timeline,
}

impl TimeAxis {
    /// Parse the `?type=` query value. Anything but `timeline` (incl.
    /// unset / "date" / garbage) → `Date`, matching the public default.
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some(v) if v.eq_ignore_ascii_case("timeline") => TimeAxis::Timeline,
            _ => TimeAxis::Date,
        }
    }
}

/// Rendering knobs shared by every renderer.
#[derive(Debug, Clone)]
pub struct ChartOpts {
    /// X-axis alignment (absolute date vs. days-since-first-star).
    pub axis: TimeAxis,
    /// True → log-scaled y-axis. Useful when overlaying repos that span
    /// several orders of magnitude in star count.
    pub log_y: bool,
    /// Emit the brief trace reveal. Embed URLs are static by default because
    /// motion in someone else's README is their call, not ours.
    pub animate: bool,
}

impl Default for ChartOpts {
    fn default() -> Self {
        Self {
            axis: TimeAxis::Date,
            log_y: false,
            animate: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChartConfig {
    pub width: u32,
    pub height: u32,
    pub padding: u32,
    pub repo: String,
    /// Human-readable cumulative metric. Exact GitHub snapshots use
    /// "stars"; archived watch events use "public star actions".
    pub metric_label: String,
    /// Animation duration in seconds for the trace drawing in. Only the
    /// single-repo [`render_svg`] uses this when animation is enabled.
    pub draw_seconds: f32,
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            width: 1200,
            height: 600,
            padding: 56,
            repo: String::new(),
            metric_label: "stars".to_string(),
            draw_seconds: 0.22,
        }
    }
}

/// The plotter pen set for `theme`, in its fixed order.
///
/// Kept as a thin accessor over [`theme::Theme::pens`] so surfaces that pin
/// their own series by position keep one source of truth for the ink. Note
/// that index [`theme::SIGNAL_PEN`] is drafting red and is reserved for a
/// measured value: a chart choosing pens for *categories* wants
/// [`theme::pens_for`], which can never hand it out.
pub fn palette(theme: &Theme) -> &'static [&'static str; 8] {
    if theme.dark {
        &theme::DARK.pens
    } else {
        &theme::LIGHT.pens
    }
}

/// Build a cumulative total-stars series from per-stargazer arrival
/// timestamps. `arrivals` must be ordered oldest-to-newest. One point per
/// arrival, with `stars` = running count.
pub fn cumulative_series(arrivals: &[DateTime<Utc>]) -> Vec<Point> {
    let mut out = Vec::with_capacity(arrivals.len());
    for (index, t) in arrivals.iter().enumerate() {
        out.push(Point {
            at: *t,
            stars: (index + 1) as u32,
        });
    }
    out
}

/// Downsample a series to at most `max_points` points by even index
/// sampling, always keeping the first and last point. Returns the input
/// unchanged when it already fits. Used so the API's `history` array and
/// the rendered paths stay bounded for huge repos.
pub fn downsample(series: &[Point], max_points: usize) -> Vec<Point> {
    downsample_by_index(series, max_points)
}

fn downsample_by_index<T: Clone>(series: &[T], max_points: usize) -> Vec<T> {
    if series.len() <= max_points || max_points < 2 {
        return series.to_vec();
    }
    let n = series.len();
    let mut out = Vec::with_capacity(max_points);
    // Sample `max_points - 1` evenly-spaced indices across [0, n-1), then
    // always append the true last point so the head and tail are exact.
    for i in 0..(max_points - 1) {
        let idx = i * (n - 1) / (max_points - 1);
        out.push(series[idx].clone());
    }
    out.push(series[n - 1].clone());
    out
}

fn bounded_render_series<T: Clone>(series: &[T]) -> Cow<'_, [T]> {
    if series.len() > MAX_RENDER_POINTS {
        Cow::Owned(downsample_by_index(series, MAX_RENDER_POINTS))
    } else {
        Cow::Borrowed(series)
    }
}

// Single-repo renderer (may animate)

pub fn render_svg(series: &[Point], cfg: &ChartConfig, theme: &Theme, opts: &ChartOpts) -> String {
    let series = bounded_render_series(series);
    render_single_svg(&series, cfg, theme, opts, 1.0, opts.animate)
}

/// Render one static frame of the single-repo sheet. Used by the GIF
/// encoder so every frame comes from the exact same geometry as the SVG
/// endpoint, and so a looping GIF is the sheet re-plotting itself rather
/// than a texture crawling under it. `progress=0` hides the trace, `1` is
/// the completed drawing.
pub(crate) fn render_svg_frame(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
    progress: f32,
) -> String {
    let series = bounded_render_series(series);
    render_single_svg(&series, cfg, theme, opts, progress.clamp(0.0, 1.0), false)
}

fn render_single_svg(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
    progress: f32,
    animate: bool,
) -> String {
    if series.is_empty() {
        return empty_svg(cfg, theme);
    }
    let geom = Geometry::new(cfg);
    // X coordinates depend on the alignment mode; in timeline mode the
    // origin is shifted to the first star so day-0 sits at the left edge.
    let xs = x_values(series, opts.axis);
    let (x_min, x_span) = x_range(&xs);
    let y_max = series.last().unwrap().stars.max(1);
    let yscale = YScale::new(y_max, opts.log_y);

    let x_at = |x: f32| geom.pad + ((x - x_min) / x_span) * geom.plot_w;
    let y_at = |v: f32| geom.pad + geom.plot_h - yscale.frac(v) * geom.plot_h;

    // Graphite: the trace is the object this sheet draws.
    let ink = theme.fg;
    let path = build_path(&xs, series, &x_at, &y_at);
    let first_v = xs[0];
    let last_v = *xs.last().unwrap();
    let first_x = x_at(first_v);
    let last_x = x_at(last_v);
    let total = series.last().unwrap().stars;
    let last_y = y_at(total as f32);

    let dash = approximate_path_length(&xs, series, &x_at, &y_at);
    let dash_offset = ((dash as f32) * (1.0 - progress)).round() as u32;

    let ordinate = ordinate_axis(
        &geom,
        geom.left(),
        &value_gradations(&yscale.ticks()),
        &y_at,
        Side::Left,
        theme.border,
        theme.muted,
    );
    let abscissa = abscissa_axis(
        &geom,
        &time_gradations(&nice_x_ticks(first_v, last_v), opts.axis),
        &x_at,
        theme.border,
        theme.muted,
    );
    let span = span_dimension(
        &geom,
        first_x,
        last_x,
        &span_label(first_v, last_v, opts.axis),
        theme,
    );

    // Drafting red, spent once per sheet: on the value measured at the
    // final datum, and on nothing else.
    let measured = format!("{} {}", fmt_count(total), cfg.metric_label);
    let callout = value_leader(&geom, (last_x, last_y), &measured, theme);

    let total_text = fmt_count(total);
    let fields = [
        TitleField {
            label: "metric",
            value: &cfg.metric_label,
        },
        TitleField {
            label: "total",
            value: &total_text,
        },
        TitleField {
            label: "scale",
            value: scale_field(opts),
        },
        TitleField {
            label: "axis",
            value: axis_field(opts.axis),
        },
    ];

    // `<animate>` is emitted only for an explicit on-site opt-in. Public
    // embed URLs are static by default.
    //
    // CRITICAL (static-embed invariant): the STATIC attributes must encode
    // the *end* state (fully-drawn trace, `stroke-dashoffset="0"`), not the
    // start of the draw-in. Consumers that render the SVG as a still (every
    // rasterizer, npm/PyPI/Docker Hub READMEs, CSS `background-image`, and
    // our own PNG/WebP path) never run the `<animate>`, so if the static
    // offset were `{dash}` the whole trace would be dashed out of view and
    // the sheet would render blank. The `<animate>` still starts the on-site
    // draw-in from `{dash}` → `0` and freezes at the end; on a static render
    // the baked `0` offset already shows the complete trace.
    let motion = if animate {
        format!(
            r#"<animate class="motion" attributeName="stroke-dashoffset" from="{dash}" to="0" dur="{dur}s" fill="freeze" calcMode="spline" keySplines="0.23 1 0.32 1" />"#,
            dur = cfg.draw_seconds,
        )
    } else {
        String::new()
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="Cumulative {metric_label} for {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px {SANS}; }}
    .grad {{ font: {GRAD_SIZE}px {MONO}; font-variant-numeric: tabular-nums; }}
    .footer-link {{ fill: {muted}; font: 600 11px {SANS}; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
{motion_css}  ]]></style>
  <text class="title" x="{title_x:.1}" y="{title_y:.1}">{repo}</text>
{ordinate}{abscissa}{span}  <path d="{path}" fill="none" stroke="{ink}" stroke-width="{W_EMPHASIS}" stroke-linecap="round" stroke-linejoin="round" stroke-dasharray="{dash}" stroke-dashoffset="{dash_offset}">{motion}</path>
{callout}{block}{footer}
</svg>"##,
        w = geom.w,
        h = geom.h,
        repo = escape_xml(&cfg.repo),
        fg = theme.fg,
        muted = theme.muted,
        motion_css = reduced_motion_css(animate),
        title_x = geom.left(),
        title_y = geom.top() - TITLE_RISE,
        metric_label = escape_xml(&cfg.metric_label),
        block = sheet_block(&geom, &fields, theme),
        footer = brand::footer_lockup(geom.right(), geom.h - 8.0, theme),
    )
}

// Multi-repo overlay renderer

/// Plot several repos' star histories on shared axes. Each repo draws in its
/// own plotter pen AND carries a label on a leader at its own line end, so
/// the sheet stays readable when hue does not survive (print, a colorblind
/// reader, a monochrome rasterizer).
///
/// The semantic final frame is always baked into the paths. `animate=1` adds
/// a brief trace reveal; consumers that render a still frame still receive
/// the complete drawing. Determinism holds: same input → same bytes.
pub fn render_multi_svg(
    series_per_repo: &[(String, Vec<Point>)],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
) -> String {
    let bounded: Vec<(String, Vec<Point>)> = series_per_repo
        .iter()
        .map(|(repo, series)| (repo.clone(), bounded_render_series(series).into_owned()))
        .collect();
    render_multi_svg_inner(&bounded, cfg, theme, opts)
}

fn render_multi_svg_inner(
    series_per_repo: &[(String, Vec<Point>)],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
) -> String {
    // Drop empty series — a repo with no stars contributes no line and
    // would otherwise divide-by-zero the axis range.
    let active: Vec<&(String, Vec<Point>)> = series_per_repo
        .iter()
        .filter(|(_, s)| !s.is_empty())
        .collect();
    if active.is_empty() {
        return empty_svg(cfg, theme);
    }
    let geom = Geometry::new(cfg);

    // Shared axes: x-range is the union of every series' x-values; y-max
    // is the largest final star count across all repos.
    let per_xs: Vec<Vec<f32>> = active.iter().map(|(_, s)| x_values(s, opts.axis)).collect();
    let mut x_min = f32::INFINITY;
    let mut x_max = f32::NEG_INFINITY;
    for xs in &per_xs {
        for &x in xs {
            x_min = x_min.min(x);
            x_max = x_max.max(x);
        }
    }
    let x_span = (x_max - x_min).max(1.0);
    let y_max = active
        .iter()
        .map(|(_, s)| s.last().map(|p| p.stars).unwrap_or(0))
        .max()
        .unwrap_or(1)
        .max(1);
    let yscale = YScale::new(y_max, opts.log_y);

    let x_at = |x: f32| geom.pad + ((x - x_min) / x_span) * geom.plot_w;
    let y_at = |v: f32| geom.pad + geom.plot_h - yscale.frac(v) * geom.plot_h;

    // Pens are claimed by repository slug, never by list position, so adding
    // a repo cannot recolor its neighbours — and `pens_for` is the accessor
    // that can never hand a category the reserved drafting red.
    let slugs: Vec<&str> = active.iter().map(|(slug, _)| slug.as_str()).collect();
    let pens = theme::pens_for(theme, &slugs);

    let ordinate = ordinate_axis(
        &geom,
        geom.left(),
        &value_gradations(&yscale.ticks()),
        &y_at,
        Side::Left,
        theme.border,
        theme.muted,
    );
    // Anchor gradations to evenly-spaced values across the shared time range,
    // rather than evenly-spaced data indices. Star arrivals are commonly
    // clustered after a launch, so index sampling can stack several labels
    // into the same few pixels at the right edge.
    let abscissa = abscissa_axis(
        &geom,
        &time_gradations(&nice_x_ticks(x_min, x_max), opts.axis),
        &x_at,
        theme.border,
        theme.muted,
    );
    let span = span_dimension(
        &geom,
        x_at(x_min),
        x_at(x_max),
        &span_label(x_min, x_max, opts.axis),
        theme,
    );

    let ends: Vec<(f32, f32)> = active
        .iter()
        .zip(per_xs.iter())
        .map(|((_, series), xs)| {
            (
                x_at(*xs.last().unwrap()),
                y_at(series.last().unwrap().stars as f32),
            )
        })
        .collect();
    let label_ys = label_heights(&ends);

    let mut paths = String::new();
    let mut labels = String::new();
    for (index, ((slug, series), xs)) in active.iter().zip(per_xs.iter()).enumerate() {
        let pen = pens[index];
        let d = build_path(xs, series, &x_at, &y_at);
        let dash = approximate_path_length(xs, series, &x_at, &y_at);
        let motion = if opts.animate {
            format!(
                "<animate class=\"motion\" attributeName=\"stroke-dashoffset\" from=\"{dash}\" to=\"0\" begin=\"{begin:.2}s\" dur=\"{dur}s\" fill=\"freeze\" calcMode=\"spline\" keySplines=\"0.23 1 0.32 1\" />",
                begin = (index as f32 * 0.07).min(0.35),
                dur = cfg.draw_seconds,
            )
        } else {
            String::new()
        };
        paths.push_str(&format!(
            "  <path d=\"{d}\" fill=\"none\" stroke=\"{pen}\" stroke-width=\"{W_EMPHASIS}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-dasharray=\"{dash}\" stroke-dashoffset=\"0\">{motion}</path>\n",
        ));
        labels.push_str(&end_label(&geom, ends[index], label_ys[index], slug, pen));
    }

    let title = if active.len() == 1 {
        escape_xml(&active[0].0)
    } else {
        escape_xml(&format!("Star history · {} repos", active.len()))
    };

    let series_count = active.len().to_string();
    let fields = [
        TitleField {
            label: "series",
            value: &series_count,
        },
        TitleField {
            label: "metric",
            value: &cfg.metric_label,
        },
        TitleField {
            label: "scale",
            value: scale_field(opts),
        },
        TitleField {
            label: "axis",
            value: axis_field(opts.axis),
        },
    ];

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="Star history overlay">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px {SANS}; }}
    .grad {{ font: {GRAD_SIZE}px {MONO}; font-variant-numeric: tabular-nums; }}
    .footer-link {{ fill: {muted}; font: 600 11px {SANS}; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
{motion_css}  ]]></style>
  <text class="title" x="{title_x:.1}" y="{title_y:.1}">{title}</text>
{ordinate}{abscissa}{span}{paths}{labels}{block}{footer}
</svg>"##,
        w = geom.w,
        h = geom.h,
        fg = theme.fg,
        muted = theme.muted,
        motion_css = reduced_motion_css(opts.animate),
        title_x = geom.left(),
        title_y = geom.top() - TITLE_RISE,
        block = sheet_block(&geom, &fields, theme),
        footer = brand::footer_lockup(geom.right(), geom.h - 8.0, theme),
    )
}

// Dual-ordinate "stars vs. usage" renderer (always static — NO <animate>)

/// A cumulative download series for the overlay's right-hand ordinate. `at`
/// is the timestamp, `total` is the running cumulative download count up to
/// that point. Kept `u64` because lifetime downloads run into the billions
/// (well past `u32`).
#[derive(Debug, Clone)]
pub struct DownloadCumPoint {
    pub at: DateTime<Utc>,
    pub total: u64,
}

/// Label for the downloads line's end label and right-ordinate values (e.g.
/// "npm downloads", "crates downloads").
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Left-ordinate series label (always "stars").
    pub repo: String,
    /// Right-ordinate series label, e.g. "npm downloads". `None` →
    /// stars-only.
    pub downloads_label: Option<String>,
}

/// Render the "stars vs. real usage" sheet: cumulative **stars** on the left
/// ordinate and cumulative **downloads** on a **right-hand second ordinate**,
/// sharing the time abscissa. Each ordinate is drawn and lettered in its own
/// series' pen, and each line carries a label on a leader at its own end, so
/// the reader knows which trace maps to which scale without decoding hue.
///
/// Static by design (no `<animate>`) — this backs an embeddable README
/// surface, where motion is the reader's call. Deterministic: same input →
/// same bytes.
///
/// When `downloads` is empty (or `cfg.downloads_label` is `None`), renders a
/// stars-only sheet plus a small "no package downloads found" note, so the
/// endpoint always returns a usable image.
pub fn render_overlay_svg(
    stars: &[Point],
    downloads: &[DownloadCumPoint],
    cfg: &ChartConfig,
    overlay: &OverlayConfig,
    theme: &Theme,
    opts: &ChartOpts,
) -> String {
    let stars = bounded_render_series(stars);
    let downloads = bounded_render_series(downloads);
    render_overlay_svg_inner(&stars, &downloads, cfg, overlay, theme, opts)
}

fn render_overlay_svg_inner(
    stars: &[Point],
    downloads: &[DownloadCumPoint],
    cfg: &ChartConfig,
    overlay: &OverlayConfig,
    theme: &Theme,
    opts: &ChartOpts,
) -> String {
    if stars.is_empty() {
        return empty_svg(cfg, theme);
    }
    let geom = Geometry::new(cfg);

    let has_downloads = overlay.downloads_label.is_some() && !downloads.is_empty();

    // Shared x range: the union of both series' x-values so the two traces
    // share a single calendar/timeline abscissa.
    let star_xs = x_values(stars, opts.axis);
    let dl_xs: Vec<f32> = if has_downloads {
        download_x_values(downloads, opts.axis)
    } else {
        Vec::new()
    };
    let mut x_min = f32::INFINITY;
    let mut x_max = f32::NEG_INFINITY;
    for &x in star_xs.iter().chain(dl_xs.iter()) {
        x_min = x_min.min(x);
        x_max = x_max.max(x);
    }
    if !x_min.is_finite() {
        x_min = 0.0;
        x_max = 1.0;
    }
    let x_span = (x_max - x_min).max(1.0);
    let x_at = |x: f32| geom.pad + ((x - x_min) / x_span) * geom.plot_w;

    // Left ordinate: stars.
    let star_max = stars.last().map(|p| p.stars).unwrap_or(1).max(1);
    let star_scale = YScale::new(star_max, opts.log_y);
    let star_y_at = |v: f32| geom.pad + geom.plot_h - star_scale.frac(v) * geom.plot_h;

    // Right ordinate: cumulative downloads. We reuse the linear/log fraction
    // mapping by scaling against the download max. u64 → f32 for the axis
    // math (precision past 2^24 is irrelevant at chart resolution).
    let dl_max = downloads.last().map(|p| p.total).unwrap_or(0).max(1);
    // Map an actual (possibly > u32) download value through the same
    // fraction curve by normalizing on the f32 max directly.
    let dl_max_f = dl_max as f32;
    let dl_frac = move |v: f32| {
        if opts.log_y {
            let denom = (dl_max_f + 1.0).ln();
            if denom <= 0.0 {
                0.0
            } else {
                (v.max(0.0) + 1.0).ln() / denom
            }
        } else {
            v / dl_max_f
        }
    };
    let dl_y_at = |v: f32| geom.pad + geom.plot_h - dl_frac(v) * geom.plot_h;

    // Two named series, two pens claimed by role. The roles are fixed
    // strings, not the caller's label, so the stars trace keeps one ink
    // whether or not a registry answered.
    let pens = theme::pens_for(theme, &["stars", "downloads"]);
    let star_pen = pens[0];
    let dl_pen = pens[1];

    let mut axes = ordinate_axis(
        &geom,
        geom.left(),
        &value_gradations(&star_scale.ticks()),
        &star_y_at,
        Side::Left,
        theme.border,
        star_pen,
    );
    if has_downloads {
        axes.push_str(&ordinate_axis(
            &geom,
            geom.right(),
            &download_gradations(dl_max, opts.log_y),
            &dl_y_at,
            Side::Right,
            theme.border,
            dl_pen,
        ));
    }
    axes.push_str(&abscissa_axis(
        &geom,
        &time_gradations(&nice_x_ticks(x_min, x_max), opts.axis),
        &x_at,
        theme.border,
        theme.muted,
    ));
    let span = span_dimension(
        &geom,
        x_at(x_min),
        x_at(x_max),
        &span_label(x_min, x_max, opts.axis),
        theme,
    );

    // Traces. The downloads trace is the only dashed one on the sheet: a
    // measured-but-secondary quantity, drawn the way a drawing draws a
    // hidden or referenced edge.
    let star_path = build_path(&star_xs, stars, &x_at, &star_y_at);
    let mut paths = format!(
        "  <path d=\"{star_path}\" fill=\"none\" stroke=\"{star_pen}\" stroke-width=\"{W_EMPHASIS}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" />\n",
    );
    let star_end = (
        x_at(*star_xs.last().unwrap()),
        star_y_at(stars.last().unwrap().stars as f32),
    );
    // Each trace is normalised to its own maximum, so both always end at the
    // very top of the sheet and their two labels always want the same spot.
    // The stacking pass is what keeps them off each other.
    let dl_end = if has_downloads {
        Some((
            x_at(*dl_xs.last().unwrap()),
            dl_y_at(downloads.last().unwrap().total as f32),
        ))
    } else {
        None
    };
    let ends: Vec<(f32, f32)> = std::iter::once(star_end).chain(dl_end).collect();
    let label_ys = label_heights(&ends);

    let mut labels = end_label(&geom, star_end, label_ys[0], "stars", star_pen);
    if let Some(dl_end) = dl_end {
        let dl_path = build_download_path(&dl_xs, downloads, &x_at, &dl_y_at);
        paths.push_str(&format!(
            "  <path d=\"{dl_path}\" fill=\"none\" stroke=\"{dl_pen}\" stroke-width=\"{W_EMPHASIS}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-dasharray=\"6 4\" />\n",
        ));
        if let Some(dl_label) = overlay.downloads_label.as_ref() {
            labels.push_str(&end_label(&geom, dl_end, label_ys[1], dl_label, dl_pen));
        }
    }

    let title = escape_xml(&format!("{} · stars vs. usage", overlay.repo));
    let mut fields = vec![
        TitleField {
            label: "metric",
            value: &cfg.metric_label,
        },
        TitleField {
            label: "scale",
            value: scale_field(opts),
        },
        TitleField {
            label: "axis",
            value: axis_field(opts.axis),
        },
    ];
    // When no registry answered, the sheet says so in the title block rather
    // than floating a loose note over the drawing. A title block is where a
    // drawing states what it is, and that includes what it does not carry.
    if !has_downloads {
        fields.push(TitleField {
            label: "downloads",
            value: "no package downloads found",
        });
    }

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="Stars vs. usage for {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px {SANS}; }}
    .grad {{ font: {GRAD_SIZE}px {MONO}; font-variant-numeric: tabular-nums; }}
    .footer-link {{ fill: {muted}; font: 600 11px {SANS}; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
  ]]></style>
  <text class="title" x="{title_x:.1}" y="{title_y:.1}">{title}</text>
{axes}{span}{paths}{labels}{block}{footer}
</svg>"##,
        w = geom.w,
        h = geom.h,
        repo = escape_xml(&overlay.repo),
        fg = theme.fg,
        muted = theme.muted,
        title_x = geom.left(),
        title_y = geom.top() - TITLE_RISE,
        block = sheet_block(&geom, &fields, theme),
        footer = brand::footer_lockup(geom.right(), geom.h - 8.0, theme),
    )
}

// Notation

/// The ordinate: one 1px axis line standing on the baseline, a 0.5px
/// extension tick springing from every gradation, and that gradation's value
/// lettered outside the tick.
///
/// There is deliberately no rule laid across the plot at each gradation. A
/// horizontal line spanning the whole sheet at a round number measures
/// nothing, encloses nothing and separates nothing: it is graph paper. What
/// the reader actually needs measured is measured — the axis itself, the
/// span below it, and the leader that lands on the final datum.
fn ordinate_axis(
    geom: &Geometry,
    x: f32,
    gradations: &[(f32, String)],
    y_at: &impl Fn(f32) -> f32,
    toward: Side,
    rule: &str,
    ink: &str,
) -> String {
    let baseline = geom.baseline();
    // The axis spans the full measured height, from the sheet's own top down
    // to the baseline, not merely up to the last round gradation. A series
    // whose maximum lands between two gradations still has its whole extent
    // measured, which is the point of drawing an axis at all.
    let top = geom.top();
    let mut out = format!(
        "  <line x1=\"{x:.1}\" y1=\"{top:.1}\" x2=\"{x:.1}\" y2=\"{baseline:.1}\" stroke=\"{rule}\" stroke-width=\"{W_OBJECT}\" />\n",
    );
    let reach = texture::TICK_CLEARANCE + texture::TICK_LEN + GRAD_GUTTER;
    let (anchor, text_x) = match toward {
        Side::Left => ("end", x - reach),
        _ => ("start", x + reach),
    };
    for (value, label) in gradations {
        let y = y_at(*value);
        out.push_str("  ");
        out.push_str(&texture::extension_tick(
            x,
            y,
            toward,
            texture::TICK_LEN,
            rule,
        ));
        out.push_str(&format!(
            "<text class=\"grad\" x=\"{text_x:.1}\" y=\"{y:.1}\" text-anchor=\"{anchor}\" dominant-baseline=\"central\" fill=\"{ink}\">{}</text>\n",
            escape_xml(label),
        ));
    }
    out
}

/// The abscissa: the 1px baseline the whole trace is measured from, plus a
/// 0.5px gradation tick and its lettering at each interior time step.
///
/// The two end gradations carry no lettering. The span dimension underneath
/// already letters where the series starts and where it ends, and printing
/// either twice would be a second answer to a question that has one — and
/// the last of them would sit hard against the sheet's right edge.
fn abscissa_axis(
    geom: &Geometry,
    gradations: &[(f32, String)],
    x_at: &impl Fn(f32) -> f32,
    rule: &str,
    ink: &str,
) -> String {
    let baseline = geom.baseline();
    let mut out = format!(
        "  <line x1=\"{l:.1}\" y1=\"{baseline:.1}\" x2=\"{r:.1}\" y2=\"{baseline:.1}\" stroke=\"{rule}\" stroke-width=\"{W_OBJECT}\" />\n",
        l = geom.left(),
        r = geom.right(),
    );
    let interior = gradations.len().saturating_sub(1);
    for (value, label) in gradations.iter().take(interior).skip(1) {
        let x = x_at(*value);
        out.push_str("  ");
        out.push_str(&texture::extension_tick(
            x,
            baseline,
            Side::Down,
            GRAD_TICK,
            rule,
        ));
        out.push_str(&format!(
            "<text class=\"grad\" x=\"{x:.1}\" y=\"{y:.1}\" text-anchor=\"middle\" fill=\"{ink}\">{}</text>\n",
            escape_xml(label),
            y = baseline + X_LABEL_DROP,
        ));
    }
    out
}

/// The overall dimension below the baseline: an extension tick at each end
/// of the plotted span and a dimension line between them carrying the time
/// range, with the rule cut for its own lettering.
///
/// Skipped when the span is too short to dimension. Two terminators and a
/// value crammed into twenty pixels measure nothing legibly.
fn span_dimension(geom: &Geometry, x0: f32, x1: f32, value: &str, theme: &Theme) -> String {
    let width = x1 - x0;
    if !width.is_finite() || width < 96.0 {
        return String::new();
    }
    let baseline = geom.baseline();
    format!(
        "  {left}{right}{dimension}\n",
        left = texture::extension_tick(x0, baseline, Side::Down, SPAN_TICK, theme.border),
        right = texture::extension_tick(x1, baseline, Side::Down, SPAN_TICK, theme.border),
        dimension = texture::dimension_h(
            x0,
            x1,
            baseline + SPAN_DROP,
            &Dimension {
                value,
                ink: theme.muted,
                ground: theme.bg,
                size: SPAN_VALUE_SIZE,
            },
        ),
    )
}

/// The leader that spends this sheet's drafting red: a line from the final
/// datum out to the value measured there, with a filled terminator landing
/// on the point itself.
///
/// The label sits in the header band above the plot, which is empty on the
/// right on every sheet, so the leader is short and can never be crossed by
/// the trace it points at.
fn value_leader(geom: &Geometry, datum: (f32, f32), value: &str, theme: &Theme) -> String {
    let label_x = (datum.0 - 10.0).max(geom.left() + 96.0);
    format!(
        "  {}\n",
        texture::leader(
            datum,
            (label_x, geom.top() - LEADER_RISE),
            value,
            LEADER_SIZE,
            theme.accent,
        )
    )
}

/// A series' own name, on a leader landing on that series' last datum.
///
/// Every multi-series sheet carries these: the pen set sits in one narrow
/// lightness band on purpose, so hue is never the only thing telling two
/// traces apart. `label_y` comes from [`label_heights`], which is what keeps
/// two of them off each other.
fn end_label(geom: &Geometry, datum: (f32, f32), label_y: f32, text: &str, pen: &str) -> String {
    let label_x = (datum.0 - 14.0).max(geom.left() + 40.0);
    format!(
        "  {}\n",
        texture::leader(datum, (label_x, label_y), text, SERIES_LABEL_SIZE, pen)
    )
}

/// Where each line-end label sits, so two of them never land on each other.
///
/// Every label wants to sit [`LABEL_RISE`] above its own line end, in the
/// clear space over a rising trace. When two ends are too close to give both
/// labels that, the pass works bottom-up and lifts the higher one further
/// rather than dropping it onto the trace it belongs to. Two series always
/// collide on the stars-versus-usage sheet, where each trace is normalised
/// to its own maximum and therefore always ends at the very top.
fn label_heights(ends: &[(f32, f32)]) -> Vec<f32> {
    let mut order: Vec<usize> = (0..ends.len()).collect();
    // Bottom-most first, tie-broken by index, so the pass is deterministic
    // whatever order the caller built its series in.
    order.sort_by(|a, b| {
        ends[*b]
            .1
            .partial_cmp(&ends[*a].1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(b))
    });
    let mut out = vec![0.0_f32; ends.len()];
    let mut ceiling = f32::INFINITY;
    for index in order {
        let y = (ends[index].1 - LABEL_RISE).min(ceiling).max(LABEL_FLOOR);
        out[index] = y;
        ceiling = y - LABEL_STACK;
    }
    out
}

/// The title block, in the plot's bottom-right corner with its outer corner
/// cut. It carries only fields the caller already supplies: what was
/// measured, how much of it there is, and the two parameters that decide
/// what the drawing means (its scale and its axis).
fn sheet_block(geom: &Geometry, fields: &[TitleField<'_>], theme: &Theme) -> String {
    let height = texture::title_block_height(fields.len());
    format!(
        "  {}\n",
        texture::title_block(
            geom.right() - TITLE_BLOCK_W,
            geom.baseline() - TITLE_BLOCK_LIFT - height,
            TITLE_BLOCK_W,
            fields,
            theme,
        )
    )
}

/// Gradations for a value axis: each tick and its compact lettering.
fn value_gradations(ticks: &[i32]) -> Vec<(f32, String)> {
    ticks
        .iter()
        .map(|v| (*v as f32, fmt_count(*v as u32)))
        .collect()
}

/// Gradations for the downloads ordinate, worked in `u64`.
///
/// Downloads run into the billions, well past `u32`. Deriving these from a
/// `u32`-clamped scale and then plotting them against the true maximum piles
/// every gradation into the bottom inch of the axis, which is exactly the
/// kind of unreadable notation a drawing is supposed to prevent.
fn download_gradations(max: u64, log: bool) -> Vec<(f32, String)> {
    let max = max.max(1);
    let mut ticks: Vec<u64> = Vec::new();
    if log {
        ticks.push(0);
        ticks.push(1);
        let mut v = 10u64;
        while v < max {
            ticks.push(v);
            v = match v.checked_mul(10) {
                Some(next) => next,
                None => break,
            };
        }
        ticks.push(max);
        ticks.dedup();
    } else {
        // The same "~5 round gradations" rule the star ordinate uses.
        let mut magnitude = 1u64;
        while magnitude.saturating_mul(10) <= max / 4 {
            magnitude = magnitude.saturating_mul(10);
        }
        let step = [1u64, 2, 5, 10]
            .iter()
            .map(|c| c.saturating_mul(magnitude))
            .find(|s| *s > 0 && max / *s <= 5)
            .unwrap_or(magnitude.max(1));
        let mut v = 0u64;
        while v <= max {
            ticks.push(v);
            v = match v.checked_add(step) {
                Some(next) => next,
                None => break,
            };
        }
    }
    ticks
        .iter()
        .map(|v| (*v as f32, fmt_count_u64(*v)))
        .collect()
}

/// Gradations for the time abscissa.
fn time_gradations(ticks: &[f32], axis: TimeAxis) -> Vec<(f32, String)> {
    ticks
        .iter()
        .map(|v| (*v, format_x_tick(*v, axis)))
        .collect()
}

/// The value a span dimension letters: where the series starts and where it
/// ends, in the same lettering the abscissa uses.
fn span_label(first: f32, last: f32, axis: TimeAxis) -> String {
    format!(
        "{} - {}",
        format_x_tick(first, axis),
        format_x_tick(last, axis)
    )
}

fn scale_field(opts: &ChartOpts) -> &'static str {
    if opts.log_y { "LOG" } else { "LINEAR" }
}

fn axis_field(axis: TimeAxis) -> &'static str {
    match axis {
        TimeAxis::Date => "DATE",
        TimeAxis::Timeline => "TIMELINE",
    }
}

/// The reduced-motion rule, emitted only on a sheet that actually moves.
fn reduced_motion_css(animate: bool) -> &'static str {
    if animate {
        "    @media (prefers-reduced-motion: reduce) { .motion { display: none; } }\n"
    } else {
        ""
    }
}

/// Per-point x-values for a cumulative-download series under the given
/// alignment. Mirrors [`x_values`] for `DownloadCumPoint`.
fn download_x_values(series: &[DownloadCumPoint], axis: TimeAxis) -> Vec<f32> {
    match axis {
        TimeAxis::Date => series.iter().map(|p| p.at.timestamp() as f32).collect(),
        TimeAxis::Timeline => {
            let origin = series.first().map(|p| p.at.timestamp()).unwrap_or(0);
            series
                .iter()
                .map(|p| (p.at.timestamp() - origin) as f32)
                .collect()
        }
    }
}

fn build_download_path(
    xs: &[f32],
    series: &[DownloadCumPoint],
    x_at: &impl Fn(f32) -> f32,
    y_at: &impl Fn(f32) -> f32,
) -> String {
    let mut s = String::new();
    for (i, p) in series.iter().enumerate() {
        let x = x_at(xs[i]);
        let y = y_at(p.total as f32);
        if i == 0 {
            s.push_str(&format!("M {x:.1} {y:.1}"));
        } else {
            s.push_str(&format!(" L {x:.1} {y:.1}"));
        }
    }
    s
}

// Shared geometry / scaling helpers

/// Plot-area geometry derived once from the config. The bottom band is
/// reserved for the abscissa's lettering and the footer lockup.
struct Geometry {
    w: f32,
    h: f32,
    pad: f32,
    plot_w: f32,
    plot_h: f32,
}

impl Geometry {
    fn new(cfg: &ChartConfig) -> Self {
        let w = cfg.width as f32;
        let h = cfg.height as f32;
        let pad = cfg.padding as f32;
        let footer = 24.0_f32;
        Self {
            w,
            h,
            pad,
            plot_w: w - pad * 2.0,
            plot_h: h - pad * 2.0 - footer,
        }
    }

    fn left(&self) -> f32 {
        self.pad
    }

    fn right(&self) -> f32 {
        self.pad + self.plot_w
    }

    fn top(&self) -> f32 {
        self.pad
    }

    /// The datum line every trace is measured from: y for a value of zero,
    /// on both the linear and the log scale.
    fn baseline(&self) -> f32 {
        self.pad + self.plot_h
    }
}

/// Per-point x-values for a series under the given alignment.
///   * `Date` — the raw unix timestamp (seconds).
///   * `Timeline` — seconds elapsed since this series' first star, so
///     every series starts at x = 0 and ages are comparable.
fn x_values(series: &[Point], axis: TimeAxis) -> Vec<f32> {
    match axis {
        TimeAxis::Date => series.iter().map(|p| p.at.timestamp() as f32).collect(),
        TimeAxis::Timeline => {
            let origin = series.first().map(|p| p.at.timestamp()).unwrap_or(0);
            series
                .iter()
                .map(|p| (p.at.timestamp() - origin) as f32)
                .collect()
        }
    }
}

fn x_range(xs: &[f32]) -> (f32, f32) {
    let min = xs.iter().copied().fold(f32::INFINITY, f32::min);
    let max = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let min = if min.is_finite() { min } else { 0.0 };
    let max = if max.is_finite() { max } else { 1.0 };
    (min, (max - min).max(1.0))
}

/// Y-axis scaling — linear or log10. Keeps the fraction mapping and the
/// gradation set in one place so every renderer agrees.
struct YScale {
    y_max: u32,
    log: bool,
}

impl YScale {
    fn new(y_max: u32, log: bool) -> Self {
        Self {
            y_max: y_max.max(1),
            log,
        }
    }

    /// Map a value to a 0..=1 fraction of the plot height.
    fn frac(&self, v: f32) -> f32 {
        if self.log {
            // log1p so a value of 0 maps to 0 rather than -inf.
            let denom = (self.y_max as f32 + 1.0).ln();
            if denom <= 0.0 {
                0.0
            } else {
                (v.max(0.0) + 1.0).ln() / denom
            }
        } else {
            v / self.y_max as f32
        }
    }

    /// Integer gradation values (always includes 0 and y_max).
    fn ticks(&self) -> Vec<i32> {
        if self.log {
            log_y_ticks(self.y_max)
        } else {
            nice_y_ticks(self.y_max as f32)
        }
    }
}

fn build_path(
    xs: &[f32],
    series: &[Point],
    x_at: &impl Fn(f32) -> f32,
    y_at: &impl Fn(f32) -> f32,
) -> String {
    let mut s = String::new();
    for (i, p) in series.iter().enumerate() {
        let x = x_at(xs[i]);
        let y = y_at(p.stars as f32);
        if i == 0 {
            s.push_str(&format!("M {x:.1} {y:.1}"));
        } else {
            s.push_str(&format!(" L {x:.1} {y:.1}"));
        }
    }
    s
}

fn approximate_path_length(
    xs: &[f32],
    series: &[Point],
    x_at: &impl Fn(f32) -> f32,
    y_at: &impl Fn(f32) -> f32,
) -> u32 {
    let mut total = 0.0_f32;
    let mut prev: Option<(f32, f32)> = None;
    for (i, p) in series.iter().enumerate() {
        let x = x_at(xs[i]);
        let y = y_at(p.stars as f32);
        if let Some((px, py)) = prev {
            let dx = x - px;
            let dy = y - py;
            total += (dx * dx + dy * dy).sqrt();
        }
        prev = Some((x, y));
    }
    (total.ceil() as u32).max(1)
}

fn nice_y_ticks(y_max: f32) -> Vec<i32> {
    if y_max <= 0.0 {
        return vec![0];
    }
    // Pick a step that yields ~5 gradations at "nice" round values.
    let raw_step = y_max / 4.0;
    let mag = 10f32.powf(raw_step.log10().floor());
    let candidates = [1.0, 2.0, 5.0, 10.0];
    let step = candidates
        .iter()
        .map(|c| c * mag)
        .find(|&s| (y_max / s) <= 5.0)
        .unwrap_or(mag);
    let mut out = Vec::new();
    let mut v = 0.0_f32;
    while v <= y_max + 0.5 {
        out.push(v.round() as i32);
        v += step;
    }
    out
}

/// Power-of-ten gradations for the log ordinate: 0, 1, 10, 100, … up to
/// y_max.
fn log_y_ticks(y_max: u32) -> Vec<i32> {
    let mut out = vec![0, 1];
    let mut v = 10i64;
    while (v as u32) < y_max {
        out.push(v as i32);
        v *= 10;
    }
    out.push(y_max as i32);
    out.dedup();
    out
}

/// Five evenly-spaced x-axis values across the time domain.
///
/// Using time values instead of data indices is important: a repo may earn
/// most of its stars in one launch-week cluster, and index-based gradations
/// would then overlap at the sheet's right edge.
fn nice_x_ticks(min: f32, max: f32) -> Vec<f32> {
    if !min.is_finite() || !max.is_finite() || max <= min {
        return Vec::new();
    }
    const COUNT: usize = 5;
    let span = max - min;
    // A sub-pixel temporal range cannot produce useful labels.
    if span <= f32::EPSILON {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(COUNT);
    for i in 0..COUNT {
        out.push(min + span * i as f32 / (COUNT - 1) as f32);
    }
    out
}

fn format_x_tick(value: f32, axis: TimeAxis) -> String {
    match axis {
        TimeAxis::Date => DateTime::<Utc>::from_timestamp(value.round() as i64, 0)
            .map(format_tick_date)
            .unwrap_or_default(),
        TimeAxis::Timeline => format_tick_days(value),
    }
}

fn format_tick_date(t: DateTime<Utc>) -> String {
    format!(
        "{} {}",
        match t.month() {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            _ => "Dec",
        },
        t.year()
    )
}

/// Timeline-mode x label: seconds-since-origin → a "day N" / "year N"
/// style age. Keeps labels short on the shared abscissa.
fn format_tick_days(secs: f32) -> String {
    let days = (secs / 86_400.0).round() as i64;
    if days >= 365 {
        let years = days as f32 / 365.0;
        format!("{years:.1}y")
    } else {
        format!("{days}d")
    }
}

/// Compact integer formatting for gradations and measured values:
/// 1234 → "1.2k", 1_500_000 → "1.5M". Deterministic.
fn fmt_count(n: u32) -> String {
    fmt_count_u64(n as u64)
}

/// `u64` variant for the downloads ordinate, which runs into the billions:
/// 1234 → "1.2k", 1_500_000 → "1.5M", 2_000_000_000 → "2.0B".
fn fmt_count_u64(n: u64) -> String {
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

fn empty_svg(cfg: &ChartConfig, theme: &Theme) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}">
  <style><![CDATA[
    .footer-link {{ fill: {muted}; font: 600 11px {SANS}; text-decoration: none; letter-spacing: 0.02em; }}
  ]]></style>
  <text x="{cx}" y="{cy}" text-anchor="middle" dominant-baseline="central" fill="{ink3}"
        font-family="{SANS}" font-size="14">No star history available</text>
{footer}
</svg>"##,
        w = cfg.width,
        h = cfg.height,
        cx = cfg.width / 2,
        cy = cfg.height / 2,
        muted = theme.muted,
        ink3 = theme.ink_3,
        footer = brand::footer_lockup(
            cfg.width as f32 - cfg.padding as f32,
            cfg.height as f32 - 8.0,
            theme,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn animated_opts() -> ChartOpts {
        ChartOpts {
            animate: true,
            ..ChartOpts::default()
        }
    }

    /// Nothing a drawing forbids may appear in a rendered sheet. Checked on
    /// every renderer, in both prints, so a gradient or a pattern cannot
    /// creep back in through one code path.
    fn assert_is_a_drawing(svg: &str) {
        for banned in [
            "linearGradient",
            "radialGradient",
            "<pattern",
            "<mask",
            "url(#",
            "filter=",
            "feGaussianBlur",
            "opacity=",
            "box-shadow",
            "var(--",
            "rx=",
            "ry=",
            "gd-star-base",
            "gd-wave-fill",
            "gd-pixel",
            "data-gitdebt-texture",
        ] {
            assert!(!svg.contains(banned), "{banned} survived in the drawing");
        }
    }

    #[test]
    fn cumulative_series_counts_monotonically() {
        let arrivals = vec![at(1), at(2), at(3), at(4)];
        let s = cumulative_series(&arrivals);
        assert_eq!(s.len(), 4);
        for (i, p) in s.iter().enumerate() {
            assert_eq!(p.stars, (i + 1) as u32);
        }
        assert_eq!(s.last().unwrap().stars, 4);
    }

    #[test]
    fn cumulative_series_empty_is_empty() {
        assert!(cumulative_series(&[]).is_empty());
    }

    #[test]
    fn downsample_keeps_first_and_last() {
        let arrivals: Vec<_> = (0..1000).map(at).collect();
        let s = cumulative_series(&arrivals);
        let ds = downsample(&s, 400);
        assert!(ds.len() <= 400);
        assert_eq!(ds.first().unwrap().stars, s.first().unwrap().stars);
        assert_eq!(ds.last().unwrap().stars, s.last().unwrap().stars);
    }

    #[test]
    fn downsample_noop_when_small() {
        let s = cumulative_series(&[at(1), at(2), at(3)]);
        assert_eq!(downsample(&s, 400).len(), 3);
    }

    #[test]
    fn renderer_bounds_large_paths_and_preserves_the_exact_total() {
        let arrivals: Vec<_> = (0..100_000).map(at).collect();
        let series = cumulative_series(&arrivals);
        let bounded = bounded_render_series(&series);
        assert_eq!(bounded.len(), MAX_RENDER_POINTS);
        assert_eq!(bounded.first().unwrap().stars, 1);
        assert_eq!(bounded.last().unwrap().stars, 100_000);

        let svg = render_svg(
            &series,
            &ChartConfig::default(),
            &LIGHT,
            &ChartOpts::default(),
        );
        // The measured value rides the leader on the final datum.
        assert!(svg.contains("100.0k stars"));
        assert!(svg.len() < 150_000, "bounded chart was {} bytes", svg.len());
    }

    #[test]
    fn x_ticks_are_evenly_spaced_in_time_not_by_star_index() {
        let ticks = nice_x_ticks(0.0, 100.0);
        assert_eq!(ticks, vec![0.0, 25.0, 50.0, 75.0, 100.0]);

        // Degenerate one-point domains intentionally render no date labels
        // instead of stacking several identical labels on one coordinate.
        assert!(nice_x_ticks(42.0, 42.0).is_empty());
    }

    #[test]
    fn render_svg_handles_empty_series() {
        let svg = render_svg(
            &[],
            &ChartConfig::default(),
            &crate::theme::LIGHT,
            &ChartOpts::default(),
        );
        assert!(svg.contains("No star history available"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.starts_with("<svg"));
    }

    /// The sheet is graphite on paper: the trace is the theme's ink, and the
    /// one measured value on it is the only drafting red anywhere.
    #[test]
    fn the_trace_is_graphite_and_only_the_measured_value_is_red() {
        let arrivals: Vec<_> = (0..10).map(|i| at(i * 86_400)).collect();
        let series = cumulative_series(&arrivals);
        let svg = render_svg(
            &series,
            &ChartConfig {
                repo: "owner/repo".into(),
                ..ChartConfig::default()
            },
            &crate::theme::LIGHT,
            &animated_opts(),
        );
        assert!(svg.contains(&format!("stroke=\"{}\"", LIGHT.fg)));
        assert_eq!(LIGHT.fg, "#111417");
        // Drafting red appears only on the leader: its line, its terminator
        // and its lettering. Nothing else on the sheet is signal.
        assert_eq!(svg.matches(LIGHT.accent).count(), 3);
        // Exactly one trace → exactly one <animate>.
        assert_eq!(svg.matches("<animate ").count(), 1);
        assert!(svg.contains(r#"dur="0.22s""#));
        assert!(svg.contains(r#"keySplines="0.23 1 0.32 1""#));
        assert!(svg.contains("prefers-reduced-motion: reduce"));
        assert!(svg.contains("owner/repo"));
        assert!(svg.contains("stars"));
        assert!(svg.contains("gitdebt.com"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        // No detection language anywhere.
        assert!(!svg.contains("suspicious"));
        assert!(!svg.contains("fake"));
        assert_is_a_drawing(&svg);
    }

    /// The notation is what makes this a drawing rather than a chart with a
    /// theme: a baseline, extension ticks that stand clear of their datums,
    /// a dimension cut for its own value, a leader, and a title block.
    #[test]
    fn the_sheet_carries_its_notation() {
        let series = cumulative_series(&(0..40).map(|i| at(i * 86_400)).collect::<Vec<_>>());
        let cfg = ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        };
        let svg = render_svg(&series, &cfg, &LIGHT, &ChartOpts::default());
        let geom = Geometry::new(&cfg);

        // The baseline runs the full plot width at y = baseline().
        assert!(svg.contains(&format!(
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\"",
            geom.left(),
            geom.baseline(),
            geom.right(),
            geom.baseline()
        )));
        // The span dimension is cut for its lettering, not boxed behind it.
        assert!(svg.contains("paint-order=\"stroke\""));
        assert!(svg.contains(&format!("stroke=\"{}\"", LIGHT.bg)));
        // The dimension carries the time range in the sheet's own lettering.
        assert!(svg.contains("Jan 1970 - Feb 1970"));
        // A title block, with its one chamfered corner and its fields.
        assert!(svg.contains(">METRIC<") && svg.contains(">TOTAL<"));
        assert!(svg.contains(">SCALE<") && svg.contains(">AXIS<"));
        assert!(svg.contains(">LINEAR<") && svg.contains(">DATE<"));
        assert!(svg.contains(&format!("letter-spacing=\"{}\"", texture::LABEL_TRACKING)));
        // Three line weights and no others.
        for width in [
            texture::W_CONSTRUCTION,
            texture::W_OBJECT,
            texture::W_EMPHASIS,
        ] {
            assert!(svg.contains(&format!("stroke-width=\"{width}\"")));
        }
        assert!(!svg.contains("stroke-width=\"2.5\""));
        assert!(!svg.contains("stroke-width=\"8\""));
        assert_is_a_drawing(&svg);
    }

    /// Extension ticks stand clear of the datum they measure, and the span's
    /// terminators land exactly on it.
    #[test]
    fn extension_ticks_stand_clear_of_the_baseline() {
        let series = cumulative_series(&(0..30).map(|i| at(i * 86_400)).collect::<Vec<_>>());
        let cfg = ChartConfig::default();
        let geom = Geometry::new(&cfg);
        let svg = render_svg(&series, &cfg, &LIGHT, &ChartOpts::default());
        let baseline = geom.baseline();
        // A tick springs from baseline + TICK_CLEARANCE, never from the
        // baseline itself.
        assert!(svg.contains(&format!("y1=\"{:.2}\"", baseline + texture::TICK_CLEARANCE)));
        assert!(svg.contains(&format!(
            "y2=\"{:.2}\"",
            baseline + texture::TICK_CLEARANCE + SPAN_TICK
        )));
        // The dimension line sits between the two ticks' ends.
        assert!(svg.contains(&format!("y1=\"{:.2}\" x2=", baseline + SPAN_DROP)));
    }

    /// The GIF encoder's frames are the same sheet re-plotting itself: only
    /// the trace's own dash offset moves, and every frame is otherwise the
    /// completed drawing. Frame 1 is deliberately still a drawing, not a
    /// half-erased one.
    #[test]
    fn plot_frames_move_only_the_trace_and_the_default_never_moves() {
        let arrivals: Vec<_> = (0..24).map(|i| at(i * 86_400)).collect();
        let series = cumulative_series(&arrivals);
        let cfg = ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        };
        let opts = ChartOpts::default();
        let start = render_svg_frame(&series, &cfg, &DARK, &opts, 0.0);
        let middle = render_svg_frame(&series, &cfg, &DARK, &opts, 0.5);
        let end = render_svg_frame(&series, &cfg, &DARK, &opts, 1.0);
        assert_ne!(start, middle, "the trace has to actually re-plot");
        assert_ne!(middle, end);
        assert_eq!(end, render_svg_frame(&series, &cfg, &DARK, &opts, 1.0));
        // A frame is a still: never SMIL, whatever the progress.
        for frame in [&start, &middle, &end] {
            assert!(!frame.contains("<animate"));
            assert_is_a_drawing(frame);
        }
        // The completed frame is byte-identical to the default sheet apart
        // from nothing at all: progress 1 IS the default sheet.
        assert_eq!(end, render_svg(&series, &cfg, &DARK, &opts));
        // Progress is clamped, so a caller cannot drive the offset negative.
        assert_eq!(render_svg_frame(&series, &cfg, &DARK, &opts, 4.0), end);
        assert_eq!(render_svg_frame(&series, &cfg, &DARK, &opts, -1.0), start);
    }

    #[test]
    fn render_svg_static_frame_is_full_line_not_blank() {
        // The static (SMIL-stripped) frame MUST show the fully-drawn trace:
        // the trace <path> carries `stroke-dashoffset="0"` (end state), never
        // `stroke-dashoffset="{dash}"` (start state, trace hidden). A consumer
        // that ignores the <animate> would otherwise render blank.
        let arrivals: Vec<_> = (0..12).map(|i| at(i * 86_400)).collect();
        let series = cumulative_series(&arrivals);
        let svg = render_svg(
            &series,
            &ChartConfig {
                repo: "owner/repo".into(),
                ..ChartConfig::default()
            },
            &crate::theme::LIGHT,
            &animated_opts(),
        );
        // The drawable trace's static offset must be exactly 0.
        assert!(
            svg.contains(r#"stroke-dashoffset="0""#),
            "trace path must bake the end-state offset (0 = fully drawn)"
        );
        // The animate still starts hidden (from={dash}) and freezes drawn.
        assert!(svg.contains(r#"from="#) && svg.contains(r#"to="0""#));
        assert!(svg.contains(r#"fill="freeze""#));
        // Guard the exact bug: the path element itself must NOT statically
        // offset by the dash length (which would hide the whole trace).
        // Derive the dash and assert it isn't used as the static dashoffset.
        let xs = x_values(&series, TimeAxis::Date);
        let geom = Geometry::new(&ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        });
        let y_max = series.last().unwrap().stars.max(1);
        let yscale = YScale::new(y_max, false);
        let (x_min, x_span) = x_range(&xs);
        let x_at = |x: f32| geom.pad + ((x - x_min) / x_span) * geom.plot_w;
        let y_at = |v: f32| geom.pad + geom.plot_h - yscale.frac(v) * geom.plot_h;
        let dash = approximate_path_length(&xs, &series, &x_at, &y_at);
        assert!(dash > 1, "sanity: a real trace has non-trivial length");
        assert!(
            !svg.contains(&format!(r#"stroke-dashoffset="{dash}""#)),
            "static dashoffset must be 0, not the dash length (blank-frame bug)"
        );
    }

    #[test]
    fn render_svg_is_static_by_default_and_leaves_the_canvas_transparent() {
        let series = cumulative_series(&[at(1), at(2)]);
        let light = render_svg(
            &series,
            &ChartConfig::default(),
            &LIGHT,
            &ChartOpts::default(),
        );
        let dark = render_svg(
            &series,
            &ChartConfig::default(),
            &DARK,
            &ChartOpts::default(),
        );
        assert!(!light.contains("<animate"));
        assert!(!dark.contains("<animate"));
        // Neither print paints its sheet: the drawing has to sit on whatever
        // README background the reader has, which the baked-per-theme
        // `<picture>` embed already guarantees is the matching one. The one
        // filled surface is the title block, which is the system's single
        // step of tone and never the full canvas.
        assert!(!light.contains(&format!("fill=\"{}\"", LIGHT.bg)));
        assert!(!dark.contains(&format!("fill=\"{}\"", DARK.bg)));
        assert!(!light.contains("data-gitdebt-canvas"));
        assert!(light.contains(r#"stroke-dashoffset="0""#));
        assert!(dark.contains(r#"stroke-dashoffset="0""#));
        assert_is_a_drawing(&light);
        assert_is_a_drawing(&dark);
    }

    /// The product-level proof: transparency survives all the way to the
    /// bytes a README actually loads, not just to the SVG markup.
    #[test]
    fn chart_rasterizes_onto_a_transparent_canvas() {
        let series = cumulative_series(&[at(1), at(2), at(3)]);
        let svg = render_svg(
            &series,
            &ChartConfig::default(),
            &DARK,
            &ChartOpts::default(),
        );
        let png = crate::raster::rasterize(&svg, crate::raster::RasterFormat::Png, 1.0)
            .expect("chart png");
        let pixmap = resvg::tiny_skia::Pixmap::decode_png(&png).expect("decode png");
        let at_px = |x: u32, y: u32| pixmap.pixels()[(y * pixmap.width() + x) as usize].alpha();
        assert_eq!(at_px(0, 0), 0, "the corner must stay fully transparent");
        assert!(
            pixmap.pixels().iter().any(|px| px.alpha() > 0),
            "a transparent canvas must not mean an empty chart"
        );
    }

    #[test]
    fn render_svg_dark_theme_uses_light_ink() {
        let series = cumulative_series(&[at(1), at(2)]);
        let svg = render_svg(
            &series,
            &ChartConfig::default(),
            &crate::theme::DARK,
            &ChartOpts::default(),
        );
        assert!(svg.contains("#e6e8ea"));
        assert!(!svg.contains("#111417"));
    }

    #[test]
    fn render_svg_is_deterministic() {
        let series = cumulative_series(&(0..20).map(|i| at(i * 3600)).collect::<Vec<_>>());
        let cfg = ChartConfig {
            repo: "a/b".into(),
            ..ChartConfig::default()
        };
        let a = render_svg(&series, &cfg, &crate::theme::LIGHT, &ChartOpts::default());
        let b = render_svg(&series, &cfg, &crate::theme::LIGHT, &ChartOpts::default());
        assert_eq!(a, b);
    }

    #[test]
    fn render_multi_svg_static_no_animate_and_deterministic() {
        let a = cumulative_series(&(0..15).map(|i| at(i * 86_400)).collect::<Vec<_>>());
        let b = cumulative_series(&(0..8).map(|i| at(i * 86_400)).collect::<Vec<_>>());
        let series = vec![("o/a".to_string(), a), ("o/b".to_string(), b)];
        let svg1 = render_multi_svg(
            &series,
            &ChartConfig::default(),
            &crate::theme::LIGHT,
            &ChartOpts::default(),
        );
        let svg2 = render_multi_svg(
            &series,
            &ChartConfig::default(),
            &crate::theme::LIGHT,
            &ChartOpts::default(),
        );
        // Embeds MUST be static — motion is opt-in, never a default.
        assert!(!svg1.contains("<animate"));
        // Same input → same bytes.
        assert_eq!(svg1, svg2);
        // Every series is labelled at its own line end.
        assert!(svg1.contains("o/a"));
        assert!(svg1.contains("o/b"));
        assert!(svg1.contains("data-gitdebt-logo=\"true\""));
        assert_is_a_drawing(&svg1);
    }

    /// Pens are claimed by slug, so a series keeps one ink across renders and
    /// across both prints — and no category can ever claim drafting red.
    #[test]
    fn render_multi_svg_pens_per_theme_and_never_the_signal() {
        let a = cumulative_series(&[at(1), at(2), at(3)]);
        let b = cumulative_series(&[at(1), at(2)]);
        let series = vec![("o/a".to_string(), a), ("o/b".to_string(), b)];
        let light = render_multi_svg(
            &series,
            &ChartConfig::default(),
            &crate::theme::LIGHT,
            &ChartOpts::default(),
        );
        assert!(light.contains("#6a588a"));
        assert!(light.contains("#1e7777"));
        assert!(!light.contains(LIGHT.accent), "no category is drafting red");
        let dark = render_multi_svg(
            &series,
            &ChartConfig::default(),
            &crate::theme::DARK,
            &ChartOpts::default(),
        );
        assert!(dark.contains("#a88fd6"));
        assert!(dark.contains("#5db5b5"));
        assert!(!dark.contains(DARK.accent));
    }

    #[test]
    fn animated_multi_svg_reveals_every_trace_without_hiding_data() {
        let a = cumulative_series(&[at(1), at(2), at(3)]);
        let b = cumulative_series(&[at(1), at(2)]);
        let series = vec![("o/a".to_string(), a), ("o/b".to_string(), b)];
        let svg = render_multi_svg(&series, &ChartConfig::default(), &DARK, &animated_opts());
        // One reveal per trace, and every trace bakes its end state.
        assert_eq!(svg.matches("<animate ").count(), 2);
        assert_eq!(svg.matches(r#"stroke-dashoffset="0""#).count(), 2);
        assert!(svg.contains("#a88fd6"));
        assert!(svg.contains("#5db5b5"));
        assert!(svg.contains("prefers-reduced-motion: reduce"));
        assert!(!svg.contains("animateTransform"));
        assert_is_a_drawing(&svg);
    }

    fn cum_dl(days_vals: &[(i64, u64)]) -> Vec<DownloadCumPoint> {
        days_vals
            .iter()
            .map(|(d, v)| DownloadCumPoint {
                at: at(d * 86_400),
                total: *v,
            })
            .collect()
    }

    #[test]
    fn overlay_dual_axis_static_and_deterministic() {
        let stars = cumulative_series(&(0..20).map(|i| at(i * 86_400)).collect::<Vec<_>>());
        let dl = cum_dl(&[(0, 100), (5, 5_000), (10, 50_000), (19, 2_000_000)]);
        let cfg = ChartConfig::default();
        let overlay = OverlayConfig {
            repo: "o/r".into(),
            downloads_label: Some("npm downloads".into()),
        };
        let a = render_overlay_svg(&stars, &dl, &cfg, &overlay, &LIGHT, &ChartOpts::default());
        let b = render_overlay_svg(&stars, &dl, &cfg, &overlay, &LIGHT, &ChartOpts::default());
        // Embeddable → no SMIL.
        assert!(!a.contains("<animate"));
        // Deterministic.
        assert_eq!(a, b);
        // Both series pens are present, and neither is drafting red.
        assert!(a.contains("#1e7777"));
        assert!(a.contains("#282c2f"));
        assert!(!a.contains(LIGHT.accent));
        // Both traces are labelled at their own line ends.
        assert!(a.contains("stars"));
        assert!(a.contains("npm downloads"));
        // The downloads ordinate letters into the millions.
        assert!(a.contains("2.0M"));
        assert!(a.contains("data-gitdebt-logo=\"true\""));
        assert_is_a_drawing(&a);
        // The downloads trace is the only dashed line on the sheet.
        assert_eq!(a.matches("stroke-dasharray").count(), 1);
    }

    #[test]
    fn overlay_stars_only_shows_note() {
        let stars = cumulative_series(&(0..10).map(|i| at(i * 86_400)).collect::<Vec<_>>());
        let svg = render_overlay_svg(
            &stars,
            &[],
            &ChartConfig::default(),
            &OverlayConfig {
                repo: "o/r".into(),
                downloads_label: None,
            },
            &LIGHT,
            &ChartOpts::default(),
        );
        assert!(svg.contains("no package downloads found"));
        // The stars trace keeps its own pen whether or not a registry
        // answered, because the pen is claimed by role, not by list index.
        assert!(svg.contains("#1e7777"));
        assert!(!svg.contains("stroke-dasharray"));
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn overlay_dark_theme_bakes_dark_pens() {
        let stars = cumulative_series(&[at(1), at(2), at(3)]);
        let dl = cum_dl(&[(0, 10), (1, 20), (2, 30)]);
        let svg = render_overlay_svg(
            &stars,
            &dl,
            &ChartConfig::default(),
            &OverlayConfig {
                repo: "o/r".into(),
                downloads_label: Some("crates downloads".into()),
            },
            &DARK,
            &ChartOpts::default(),
        );
        assert!(svg.contains("#5db5b5")); // dark stars pen
        assert!(svg.contains("#d5d8da")); // dark downloads pen
        assert_is_a_drawing(&svg);
    }

    /// The downloads ordinate is worked in `u64`. Deriving its gradations
    /// from a `u32`-clamped scale and then plotting them against the true
    /// maximum piled every one of them into the bottom inch of the axis.
    #[test]
    fn the_downloads_ordinate_spreads_past_u32() {
        let max = 24_300_000_000_u64; // well past u32::MAX
        let gradations = download_gradations(max, false);
        assert!(gradations.len() >= 4 && gradations.len() <= 7);
        assert_eq!(gradations.first().unwrap().1, "0");
        // The gradations reach the measured maximum instead of stopping at a
        // twentieth of it, and they are evenly spread.
        let top = gradations.last().unwrap().0;
        assert!(
            top >= max as f32 * 0.8,
            "top gradation {top} nowhere near {max}"
        );
        for pair in gradations.windows(2) {
            assert!(pair[1].0 > pair[0].0);
        }
        assert!(gradations.iter().any(|(_, label)| label.ends_with('B')));

        // The log ordinate walks powers of ten and finishes on the maximum.
        let log = download_gradations(max, true);
        assert_eq!(log.first().unwrap().1, "0");
        assert_eq!(log.last().unwrap().0, max as f32);
        // Neither form can loop forever or overflow on an absurd maximum.
        assert!(!download_gradations(u64::MAX, true).is_empty());
        assert!(!download_gradations(u64::MAX, false).is_empty());
        assert_eq!(download_gradations(0, false).first().unwrap().1, "0");
    }

    /// Two line-end labels never sit on each other, and a stack of them
    /// never climbs off the top of the sheet.
    #[test]
    fn line_end_labels_stack_instead_of_colliding() {
        // The stars-versus-usage case: both traces are normalised to their
        // own maximum, so both always end at the very top.
        let tied = label_heights(&[(1144.0, 56.0), (1144.0, 56.0)]);
        assert!((tied[0] - tied[1]).abs() >= LABEL_STACK - 0.01);
        assert!(tied.iter().all(|y| *y >= LABEL_FLOOR));

        // Well-separated ends each keep their own natural rise.
        let apart = label_heights(&[(0.0, 100.0), (0.0, 300.0), (0.0, 500.0)]);
        assert_eq!(
            apart,
            vec![100.0 - LABEL_RISE, 300.0 - LABEL_RISE, 500.0 - LABEL_RISE]
        );

        // The pass does not depend on the caller's ordering.
        let one_way = label_heights(&[(0.0, 200.0), (0.0, 208.0), (0.0, 500.0)]);
        let other_way = label_heights(&[(0.0, 500.0), (0.0, 208.0), (0.0, 200.0)]);
        assert_eq!(one_way[0], other_way[2]);
        assert_eq!(one_way[1], other_way[1]);
        assert_eq!(one_way[2], other_way[0]);
        assert!(label_heights(&[]).is_empty());
    }

    /// The span dimension letters both ends of the plotted range, so the
    /// abscissa letters neither — printing either twice would answer one
    /// question twice, and the last would sit hard against the sheet edge.
    #[test]
    fn the_abscissa_letters_only_its_interior_gradations() {
        let series = cumulative_series(
            &(0..600)
                .map(|i| at(1_650_000_000 + i * 86_400))
                .collect::<Vec<_>>(),
        );
        let svg = render_svg(
            &series,
            &ChartConfig::default(),
            &LIGHT,
            &ChartOpts::default(),
        );
        let ticks = nice_x_ticks(
            series.first().unwrap().at.timestamp() as f32,
            series.last().unwrap().at.timestamp() as f32,
        );
        let first = format_x_tick(ticks[0], TimeAxis::Date);
        let last = format_x_tick(*ticks.last().unwrap(), TimeAxis::Date);
        // Each end appears exactly once on the sheet: inside the dimension.
        assert_eq!(svg.matches(&first).count(), 1);
        assert_eq!(svg.matches(&last).count(), 1);
        assert!(svg.contains(&format!("{first} - {last}")));
        // The three interior gradations are lettered under the baseline.
        for interior in &ticks[1..ticks.len() - 1] {
            let label = format_x_tick(*interior, TimeAxis::Date);
            assert!(svg.contains(&format!(">{label}<")), "{label} missing");
        }
    }

    #[test]
    fn fmt_count_u64_billions() {
        assert_eq!(fmt_count_u64(0), "0");
        assert_eq!(fmt_count_u64(999), "999");
        assert_eq!(fmt_count_u64(12_345), "12.3k");
        assert_eq!(fmt_count_u64(1_500_000), "1.5M");
        assert_eq!(fmt_count_u64(13_000_000_000), "13.0B");
    }

    #[test]
    fn timeline_alignment_shifts_x_origin() {
        // Two repos with stars at very different absolute dates but the
        // same shape. In timeline mode both start at day 0, so their
        // first-point x must coincide; in date mode they must not.
        let early = cumulative_series(&[at(0), at(86_400), at(2 * 86_400)]);
        let late = cumulative_series(&[at(100 * 86_400), at(101 * 86_400), at(102 * 86_400)]);
        let date_xs_early = x_values(&early, TimeAxis::Date);
        let date_xs_late = x_values(&late, TimeAxis::Date);
        assert_ne!(date_xs_early[0], date_xs_late[0]);

        let tl_xs_early = x_values(&early, TimeAxis::Timeline);
        let tl_xs_late = x_values(&late, TimeAxis::Timeline);
        assert_eq!(tl_xs_early[0], 0.0);
        assert_eq!(tl_xs_late[0], 0.0);
        // And the relative shape is preserved.
        assert_eq!(tl_xs_early, tl_xs_late);
    }

    /// Timeline mode letters ages, and the title block says which axis the
    /// sheet was drawn against so the reader is never guessing.
    #[test]
    fn timeline_sheets_declare_their_axis_and_scale() {
        let series = cumulative_series(&(0..400).map(|i| at(i * 86_400)).collect::<Vec<_>>());
        let svg = render_svg(
            &series,
            &ChartConfig::default(),
            &LIGHT,
            &ChartOpts {
                axis: TimeAxis::Timeline,
                log_y: true,
                animate: false,
            },
        );
        assert!(svg.contains(">TIMELINE<") && svg.contains(">LOG<"));
        assert!(svg.contains("0d - 1.1y"));
        assert_is_a_drawing(&svg);
    }

    #[test]
    fn log_axis_renders_without_panicking() {
        let series = cumulative_series(&(0..50).map(|i| at(i * 3600)).collect::<Vec<_>>());
        let svg = render_svg(
            &series,
            &ChartConfig::default(),
            &crate::theme::LIGHT,
            &ChartOpts {
                axis: TimeAxis::Date,
                log_y: true,
                animate: false,
            },
        );
        assert!(svg.starts_with("<svg"));
    }

    /// A one-point series has no span to dimension and no room for a leader
    /// label to its left. Neither may render broken notation.
    #[test]
    fn a_single_datum_dimensions_nothing_and_stays_on_the_sheet() {
        let series = cumulative_series(&[at(1_700_000_000)]);
        let cfg = ChartConfig::default();
        let svg = render_svg(&series, &cfg, &LIGHT, &ChartOpts::default());
        let geom = Geometry::new(&cfg);
        // No span dimension: two terminators and a value in zero pixels
        // would measure nothing.
        assert!(!svg.contains("paint-order=\"stroke\""));
        // The leader's label stays inside the sheet rather than running off
        // the left edge.
        assert!(svg.contains(&format!("x=\"{:.2}\"", geom.left() + 96.0 + 4.0)));
        assert!(!svg.contains("NaN"));
        assert_is_a_drawing(&svg);
    }

    #[test]
    fn xml_escaped_in_repo_name() {
        let svg = render_svg(
            &cumulative_series(&[at(1), at(2)]),
            &ChartConfig {
                repo: "<script>".into(),
                ..ChartConfig::default()
            },
            &crate::theme::LIGHT,
            &ChartOpts::default(),
        );
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;"));
    }
}
