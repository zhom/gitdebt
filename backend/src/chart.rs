//! Star-history SVG charts: a single cumulative total-stars line, and a
//! multi-repo overlay that plots N repos' star histories on shared axes.
//!
//! Pure: a time series → string. No external SVG library — the markup is
//! short enough to write by hand and we want it deterministic for
//! caching/embedding (same input → same bytes).
//!
//! Two renderers:
//!   * [`render_svg`] — single repo, ONE total-stars line. Animates the
//!     draw-in only when [`ChartOpts::animate`] is explicitly enabled.
//!   * [`render_multi_svg`] — N repos overlaid with a legend. ALWAYS
//!     static — no `<animate>` — because a comparison is read, not
//!     watched, and motion would fight the legend for attention.
//!
//! Theme colors are baked as concrete hex (no CSS vars) so the SVG renders
//! correctly when embedded as an `<img>` regardless of OS / page theme.
//! Series colors come from a shared categorical palette, index 0 = brand
//! ink. See `theme.rs` for the per-element fg/muted/grid colors.

use std::borrow::Cow;

use chrono::{DateTime, Datelike, Utc};
use serde::Serialize;

use crate::brand;
use crate::theme::Theme;

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

/// Rendering knobs shared by both renderers.
#[derive(Debug, Clone)]
pub struct ChartOpts {
    /// X-axis alignment (absolute date vs. days-since-first-star).
    pub axis: TimeAxis,
    /// True → log-scaled y-axis. Useful when overlaying repos that span
    /// several orders of magnitude in star count.
    pub log_y: bool,
    /// Emit the brief line reveal. Embed URLs are static by default because
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
    /// "stars"; GH Archive WatchEvents use "public star actions".
    pub metric_label: String,
    /// Animation duration in seconds for the line drawing in. Only the
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

/// Categorical series palette shared by exports and the interactive web
/// charts. The first series is the product's star-history blue; subsequent
/// series use the established signal colors so comparisons remain legible
/// without collapsing into eight nearly-identical gray lines.
pub const PALETTE_LIGHT: [&str; 8] = [
    "#087fea", "#7c4dff", "#d729a9", "#0a8f55", "#d06a00", "#d33434", "#007f91", "#525252",
];
pub const PALETTE_DARK: [&str; 8] = [
    "#358ff3", "#966eff", "#f05abe", "#28d26e", "#ff9632", "#f04646", "#23b8c8", "#d4d4d4",
];

/// The categorical palette for `theme`. `palette(theme)[i % 8]` is the
/// color for the i-th series.
pub fn palette(theme: &Theme) -> &'static [&'static str; 8] {
    if theme.dark {
        &PALETTE_DARK
    } else {
        &PALETTE_LIGHT
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
    crate::texture::decorate(
        render_single_svg(&series, cfg, theme, opts, 1.0, opts.animate, None),
        theme,
    )
}

/// Render one static frame of the single-repo chart. Used by the GIF
/// encoder so every frame comes from the exact same chart geometry as the
/// SVG endpoint. `progress=0` hides the line, `1` is the completed chart.
pub(crate) fn render_svg_frame(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
    progress: f32,
) -> String {
    let series = bounded_render_series(series);
    crate::texture::decorate(
        render_single_svg(
            &series,
            cfg,
            theme,
            opts,
            progress.clamp(0.0, 1.0),
            false,
            None,
        ),
        theme,
    )
}

/// One frame of the looping `wave` motion: the fully drawn chart with the
/// dithered underfill's top edge displaced by layered sines and the Bayer
/// threshold phase advanced. Loop-periodic in `frame / frames`, phases
/// seeded deterministically by the caller.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WaveSpec {
    /// 0-based frame index within the cycle.
    pub frame: usize,
    /// Total frames in one seamless cycle.
    pub frames: usize,
    /// fnv1a-style seed (slug-derived) → per-repo stable phases.
    pub seed: u32,
}

pub(crate) fn render_svg_wave_frame(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
    wave: WaveSpec,
) -> String {
    let series = bounded_render_series(series);
    crate::texture::decorate(
        render_single_svg(&series, cfg, theme, opts, 1.0, false, Some(wave)),
        theme,
    )
}

/// Blue ink is deliberately most solid at the baseline and resolves into a
/// sparse ordered-dither field at the data contour. Keeping this luminance
/// ramp separate from the moving cell pattern makes every animation frame
/// describe the exact same data while matching the interactive chart.
fn star_area_defs(color: &str, top: f32, bottom: f32) -> String {
    format!(
        r##"<linearGradient id="gd-star-base" gradientUnits="userSpaceOnUse" x1="0" y1="{top:.1}" x2="0" y2="{bottom:.1}">
      <stop offset="0" stop-color="{color}" stop-opacity="0.07" />
      <stop offset="0.48" stop-color="{color}" stop-opacity="0.27" />
      <stop offset="1" stop-color="{color}" stop-opacity="0.94" />
    </linearGradient>
    <linearGradient id="gd-star-dither-alpha" gradientUnits="userSpaceOnUse" x1="0" y1="{top:.1}" x2="0" y2="{bottom:.1}">
      <stop offset="0" stop-color="#fff" stop-opacity="0.98" />
      <stop offset="0.56" stop-color="#fff" stop-opacity="0.74" />
      <stop offset="1" stop-color="#fff" stop-opacity="0.14" />
    </linearGradient>
    <mask id="gd-star-dither-mask"><rect width="100%" height="100%" fill="url(#gd-star-dither-alpha)" /></mask>"##,
    )
}

fn render_single_svg(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
    progress: f32,
    animate: bool,
    wave: Option<WaveSpec>,
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

    let color = palette(theme)[0];
    let path = build_path(&xs, series, &x_at, &y_at);
    let baseline = y_at(0.0);
    let first_x = x_at(xs[0]);
    let last_x = x_at(*xs.last().unwrap_or(&xs[0]));
    // The dithered underfill always uses the exact data path as its contour.
    // Motion belongs to the ink density/pattern phase, never the geometry:
    // separating the fill edge from the line creates a visible black seam and
    // makes the shaded area appear to describe different data.
    let (area, area_fill, wave_defs) = if let Some(w) = wave {
        let area = format!("{path} L {last_x:.1} {baseline:.1} L {first_x:.1} {baseline:.1} Z");
        // The 32-column cell strip advances a seeded sine threshold, matching
        // the interactive chart's density wave without moving the contour.
        let dense = crate::texture::wave_cells_with(color, w.frame, w.frames, w.seed);
        let defs = format!(
            "  <defs><pattern id=\"gd-wave-fill\" width=\"64\" height=\"8\" patternUnits=\"userSpaceOnUse\" patternTransform=\"translate(.5 .5)\"><g shape-rendering=\"crispEdges\" opacity=\"0.96\" transform=\"scale(2)\">{dense}</g></pattern>{}</defs>\n",
            star_area_defs(color, geom.pad, baseline),
        );
        (area, "url(#gd-wave-fill)".to_string(), defs)
    } else if animate {
        // March the dither one full 8px tile per cycle. Consumers that strip
        // SMIL retain the complete phase-0 fill and exact data contour.
        let area = format!("{path} L {last_x:.1} {baseline:.1} L {first_x:.1} {baseline:.1} Z");
        let dense = crate::texture::dense_cells_with(color);
        let defs = format!(
            "  <defs><pattern id=\"gd-wave-fill\" width=\"8\" height=\"8\" patternUnits=\"userSpaceOnUse\" patternTransform=\"translate(.5 .5)\"><g shape-rendering=\"crispEdges\" opacity=\"0.96\" transform=\"scale(2)\">{dense}</g><animateTransform class=\"motion\" attributeName=\"patternTransform\" type=\"translate\" from=\"0.5 0.5\" to=\"8.5 0.5\" dur=\"0.8s\" repeatCount=\"indefinite\" /></pattern>{}</defs>\n",
            star_area_defs(color, geom.pad, baseline),
        );
        (area, "url(#gd-wave-fill)".to_string(), defs)
    } else {
        let defs = format!(
            "  <defs><pattern id=\"gd-wave-fill\" width=\"8\" height=\"8\" patternUnits=\"userSpaceOnUse\" patternTransform=\"translate(.5 .5)\"><g shape-rendering=\"crispEdges\" opacity=\"0.96\" transform=\"scale(2)\">{}</g></pattern>{}</defs>\n",
            crate::texture::dense_cells_with(color),
            star_area_defs(color, geom.pad, baseline),
        );
        (
            format!("{path} L {last_x:.1} {baseline:.1} L {first_x:.1} {baseline:.1} Z"),
            "url(#gd-wave-fill)".to_string(),
            defs,
        )
    };
    let dash = approximate_path_length(&xs, series, &x_at, &y_at);
    let dash_offset = ((dash as f32) * (1.0 - progress)).round() as u32;

    let y_ticks = yscale.ticks();
    let x_ticks = nice_x_ticks(
        xs.first().copied().unwrap_or(x_min),
        xs.last().copied().unwrap_or(x_min),
    );
    let axis_lines = render_axes(cfg, &x_ticks, &y_ticks, opts.axis, &x_at, &y_at, theme);

    let total = series.last().unwrap().stars;
    let subtitle_text = format!("{} {}", fmt_count(total), cfg.metric_label);

    // `<animate>` is emitted only for an explicit on-site opt-in. Public
    // embed URLs are static by default.
    //
    // CRITICAL (static-embed invariant): the STATIC attributes must encode
    // the *end* state (fully-drawn line, `stroke-dashoffset="0"`), not the
    // start of the draw-in. Consumers that render the SVG as a still (every
    // rasterizer, npm/PyPI/Docker Hub READMEs, CSS `background-image`, and
    // our own PNG/WebP path) never run the `<animate>`, so if the static
    // offset were `{dash}` the whole line would be dashed out of view and
    // the chart would render blank. The `<animate>` still starts the on-site
    // draw-in from `{dash}` → `0` and freezes at the end; on a static render
    // the baked `0` offset already shows the complete line.
    let motion = if animate {
        format!(
            r#"    <animate class="motion" attributeName="stroke-dashoffset" from="{dash}" to="0" dur="{dur}s" fill="freeze" calcMode="spline" keySplines="0.23 1 0.32 1" />"#,
            dur = cfg.draw_seconds,
        )
    } else {
        String::new()
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="Cumulative {metric_label} for {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .subtitle {{ fill: {muted}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .axis-label {{ fill: {muted}; font: 12px ui-sans-serif, system-ui, sans-serif; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
    }}
  ]]></style>
{wave_defs}  <text class="title" x="{title_x}" y="{title_y}">{repo}</text>
  <text class="subtitle" x="{title_x}" y="{subtitle_y}">{subtitle}</text>
{axis_lines}
  <path d="{area}" fill="url(#gd-star-base)" />
  <path d="{area}" fill="{area_fill}" mask="url(#gd-star-dither-mask)" opacity="0.94" />
  <path d="{path}" fill="none" stroke="{color}" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" stroke-dasharray="{dash}" stroke-dashoffset="{dash_offset}">
{motion}
  </path>
{footer}
</svg>"##,
        w = geom.w,
        h = geom.h,
        repo = escape_xml(&cfg.repo),
        fg = theme.fg,
        muted = theme.muted,
        title_x = geom.pad,
        title_y = geom.pad - 22.0,
        subtitle_y = geom.pad - 6.0,
        path = path,
        area = area,
        area_fill = area_fill,
        wave_defs = wave_defs,
        dash = dash,
        color = color,
        dash_offset = dash_offset,
        motion = motion,
        axis_lines = axis_lines,
        subtitle = escape_xml(&subtitle_text),
        metric_label = escape_xml(&cfg.metric_label),
        footer = brand::footer_lockup(geom.w - geom.pad, geom.h - 8.0, theme),
    )
}

// Multi-repo overlay renderer (always static — NO <animate>)

/// Plot several repos' star histories on shared axes, with a legend
/// (slug + color swatch). `series_per_repo` is `(slug, points)` in the
/// order the caller wants them treated — index 0 gets the strongest ink.
///
/// The semantic final frame is always baked into the paths. `animate=1` adds
/// a brief line reveal plus a looping ordered-dither phase; consumers that
/// render a still frame still receive the complete chart. Determinism holds: same
/// input → same bytes.
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
    crate::texture::decorate(render_multi_svg_inner(&bounded, cfg, theme, opts), theme)
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

    let pal = palette(theme);
    let y_ticks = yscale.ticks();
    // Anchor labels to evenly-spaced values across the shared time range,
    // rather than evenly-spaced data indices. Star arrivals are commonly
    // clustered after a launch, so index sampling can stack several labels
    // into the same few pixels at the right edge.
    let x_ticks = nice_x_ticks(x_min, x_max);
    let axis_lines = render_axes(cfg, &x_ticks, &y_ticks, opts.axis, &x_at, &y_at, theme);

    // One path per repo, plus a legend row.
    let mut paths = String::new();
    let mut legend = String::new();
    let mut series_defs = String::new();
    let legend_x = geom.pad;
    let legend_y = geom.h - 12.0;
    let mut lx = legend_x;
    for (i, ((slug, series), xs)) in active.iter().zip(per_xs.iter()).enumerate() {
        let color = pal[i % pal.len()];
        let d = build_path(xs, series, &x_at, &y_at);
        let dash = approximate_path_length(xs, series, &x_at, &y_at);
        let motion = if opts.animate {
            format!(
                "<animate class=\"motion\" attributeName=\"stroke-dashoffset\" from=\"{dash}\" to=\"0\" begin=\"{begin:.2}s\" dur=\"{dur}s\" fill=\"freeze\" calcMode=\"spline\" keySplines=\"0.23 1 0.32 1\" />",
                begin = (i as f32 * 0.07).min(0.35),
                dur = cfg.draw_seconds,
            )
        } else {
            String::new()
        };
        let pattern_motion = if opts.animate {
            "<animateTransform class=\"motion\" attributeName=\"patternTransform\" type=\"translate\" from=\"0.5 0.5\" to=\"8.5 0.5\" dur=\"1.6s\" repeatCount=\"indefinite\" />"
        } else {
            ""
        };
        series_defs.push_str(&format!(
            "    <pattern id=\"gd-series-{i}\" width=\"8\" height=\"8\" patternUnits=\"userSpaceOnUse\" patternTransform=\"translate(.5 .5)\"><g shape-rendering=\"crispEdges\" opacity=\"0.96\" transform=\"scale(2)\">{cells}</g>{pattern_motion}</pattern>\n",
            cells = crate::texture::dense_cells_with(color),
        ));
        paths.push_str(&format!(
            "  <path d=\"{d}\" fill=\"none\" stroke=\"url(#gd-series-{i})\" stroke-width=\"8\" opacity=\"0.34\" stroke-linecap=\"square\" stroke-linejoin=\"miter\" />\n  <path d=\"{d}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-dasharray=\"{dash}\" stroke-dashoffset=\"0\">{motion}</path>\n",
        ));
        // Legend swatch + label. Advance `lx` by an estimate of the label
        // width so entries don't overlap (deterministic: 6.5px/char).
        legend.push_str(&format!(
            "    <rect x=\"{:.1}\" y=\"{:.1}\" width=\"14\" height=\"3\" rx=\"1.5\" fill=\"{color}\" />\n    <text class=\"legend\" x=\"{:.1}\" y=\"{:.1}\">{label}</text>\n",
            lx,
            legend_y - 10.0,
            lx + 20.0,
            legend_y,
            label = escape_xml(slug),
        ));
        lx += 20.0 + slug.chars().count() as f32 * 6.5 + 24.0;
    }

    let title = if active.len() == 1 {
        escape_xml(&active[0].0)
    } else {
        escape_xml(&format!("Star history · {} repos", active.len()))
    };

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="Star history overlay">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .axis-label {{ fill: {muted}; font: 12px ui-sans-serif, system-ui, sans-serif; }}
    .legend {{ fill: {fg}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{ .motion {{ display: none; }} }}
  ]]></style>
  <defs>
{series_defs}  </defs>
  <text class="title" x="{title_x}" y="{title_y}">{title}</text>
{axis_lines}
{paths}  <g class="legend-row">
{legend}  </g>
{footer}
</svg>"##,
        w = geom.w,
        h = geom.h,
        fg = theme.fg,
        muted = theme.muted,
        series_defs = series_defs,
        title_x = geom.pad,
        title_y = geom.pad - 14.0,
        title = title,
        axis_lines = axis_lines,
        paths = paths,
        legend = legend,
        footer = brand::footer_lockup(geom.w - geom.pad, geom.h - 8.0, theme),
    )
}

// Dual-axis "stars vs. usage" overlay renderer (always static — NO <animate>)

/// A cumulative download series for the overlay's right-hand axis. `at` is
/// the timestamp, `total` is the running cumulative download count up to
/// that point. Kept `u64` because lifetime downloads run into the billions
/// (well past `u32`).
#[derive(Debug, Clone)]
pub struct DownloadCumPoint {
    pub at: DateTime<Utc>,
    pub total: u64,
}

/// Label for the downloads line's legend + right-axis caption (e.g.
/// "npm downloads", "crates downloads").
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Left-axis series label (always "stars").
    pub repo: String,
    /// Right-axis series label, e.g. "npm downloads". `None` → stars-only.
    pub downloads_label: Option<String>,
}

/// Render the "stars vs. real usage" overlay: cumulative **stars** on the
/// left y-axis (primary ink, palette index 0) and cumulative **downloads**
/// on a **right-hand secondary y-axis** (palette index 1), sharing the x
/// time-axis. Dual-axis legend; both axis scales are labeled in their
/// series color so the reader knows which line maps to which scale.
///
/// Static by design (no `<animate>`) — this backs an embeddable README
/// surface, where motion is the reader's call. Deterministic: same input →
/// same bytes.
///
/// When `downloads` is empty (or `cfg.downloads_label` is `None`), renders a
/// stars-only chart plus a small "no package downloads found" note, so the
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
    crate::texture::decorate(
        render_overlay_svg_inner(&stars, &downloads, cfg, overlay, theme, opts),
        theme,
    )
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

    // Shared x range: the union of both series' x-values so the two lines
    // share a single calendar/timeline axis.
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

    // Left axis: stars.
    let star_max = stars.last().map(|p| p.stars).unwrap_or(1).max(1);
    let star_scale = YScale::new(star_max, opts.log_y);
    let star_y_at = |v: f32| geom.pad + geom.plot_h - star_scale.frac(v) * geom.plot_h;

    // Right axis: cumulative downloads. We reuse the linear/log fraction
    // mapping by scaling against the download max. u64 → f32 for the axis
    // math (precision past 2^24 is irrelevant at chart resolution).
    let dl_max = downloads.last().map(|p| p.total).unwrap_or(0).max(1);
    let dl_scale = YScale::new(dl_max.min(u32::MAX as u64) as u32, opts.log_y);
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

    let pal = palette(theme);
    let star_color = pal[0];
    let dl_color = pal[1];

    // Grid + left (stars) axis labels.
    let star_ticks = star_scale.ticks();
    let x_ticks = nice_x_ticks(x_min, x_max);
    let mut axis = String::new();
    axis.push_str("  <g class=\"axes\">\n");
    for y in &star_ticks {
        let yp = star_y_at(*y as f32);
        axis.push_str(&format!(
            "    <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{grid}\" stroke-width=\"1\" opacity=\"0.6\" />\n",
            geom.pad,
            yp,
            geom.pad + geom.plot_w,
            yp,
            grid = theme.grid,
        ));
        // Left-axis labels in the stars color.
        axis.push_str(&format!(
            "    <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\" fill=\"{star_color}\" font-family=\"ui-sans-serif, system-ui, sans-serif\" font-size=\"12\">{}</text>\n",
            geom.pad - 8.0,
            yp + 4.0,
            fmt_count(*y as u32),
        ));
    }
    // Right-axis (downloads) labels, only when there's a downloads line.
    if has_downloads {
        let dl_ticks = dl_scale.ticks();
        for y in &dl_ticks {
            let yv = *y as f32;
            // Skip ticks beyond the real download max (the u32-clamped
            // scale can emit a top tick a hair above dl_max).
            if yv > dl_max_f * 1.001 {
                continue;
            }
            let yp = dl_y_at(yv);
            axis.push_str(&format!(
                "    <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"start\" fill=\"{dl_color}\" font-family=\"ui-sans-serif, system-ui, sans-serif\" font-size=\"12\">{}</text>\n",
                geom.pad + geom.plot_w + 8.0,
                yp + 4.0,
                fmt_count_u64(*y as u64),
            ));
        }
    }
    // X-axis labels (shared), evenly spaced across the full union range.
    if x_ticks.len() >= 2 {
        for &tick in x_ticks.iter().skip(1) {
            let xp = x_at(tick);
            let label = format_x_tick(tick, opts.axis);
            axis.push_str(&format!(
                "    <text class=\"axis-label\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>\n",
                xp,
                geom.h - geom.pad + 18.0,
                escape_xml(&label),
            ));
        }
    }
    axis.push_str("  </g>\n");

    // Lines.
    let star_path = build_path(&star_xs, stars, &x_at, &star_y_at);
    let mut paths = format!(
        "  <path d=\"{star_path}\" fill=\"none\" stroke=\"{pixel_fill}\" stroke-width=\"8\" opacity=\"0.38\" stroke-linecap=\"square\" />\n  <path d=\"{star_path}\" fill=\"none\" stroke=\"{star_color}\" stroke-width=\"2.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\" />\n",
        pixel_fill = crate::texture::FILL,
    );
    if has_downloads {
        let dl_path = build_download_path(&dl_xs, downloads, &x_at, &dl_y_at);
        paths.push_str(&format!(
            "  <path d=\"{dl_path}\" fill=\"none\" stroke=\"{pixel_fill}\" stroke-width=\"7\" opacity=\"0.3\" stroke-linecap=\"square\" />\n  <path d=\"{dl_path}\" fill=\"none\" stroke=\"{dl_color}\" stroke-width=\"2.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-dasharray=\"6 4\" />\n",
            pixel_fill = crate::texture::FILL,
        ));
    }

    // Legend: stars swatch + label, downloads swatch + label.
    let legend_y = geom.h - 12.0;
    let star_label = "stars".to_string();
    let mut legend = format!(
        "    <rect x=\"{lx:.1}\" y=\"{ly:.1}\" width=\"14\" height=\"3\" rx=\"1.5\" fill=\"{star_color}\" />\n    <text class=\"legend\" x=\"{tx:.1}\" y=\"{ty:.1}\">{label}</text>\n",
        lx = geom.pad,
        ly = legend_y - 10.0,
        tx = geom.pad + 20.0,
        ty = legend_y,
        label = escape_xml(&star_label),
    );
    let mut lx = geom.pad + 20.0 + star_label.chars().count() as f32 * 6.5 + 24.0;
    if let Some(dl_label) = overlay.downloads_label.as_ref().filter(|_| has_downloads) {
        legend.push_str(&format!(
            "    <rect x=\"{lx:.1}\" y=\"{ly:.1}\" width=\"14\" height=\"3\" rx=\"1.5\" fill=\"{dl_color}\" />\n    <text class=\"legend\" x=\"{tx:.1}\" y=\"{ty:.1}\">{label}</text>\n",
            lx = lx,
            ly = legend_y - 10.0,
            tx = lx + 20.0,
            ty = legend_y,
            label = escape_xml(dl_label),
        ));
        lx += 20.0 + dl_label.chars().count() as f32 * 6.5 + 24.0;
    }
    let _ = lx;

    // A small note when there's nothing to plot on the right axis.
    let note = if has_downloads {
        String::new()
    } else {
        format!(
            "  <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\" fill=\"{muted}\" font-family=\"ui-sans-serif, system-ui, sans-serif\" font-size=\"12\">no package downloads found</text>\n",
            geom.pad + geom.plot_w,
            geom.pad - 6.0,
            muted = theme.muted,
        )
    };

    let title = escape_xml(&format!("{} · stars vs. usage", overlay.repo));

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="Stars vs. usage for {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .axis-label {{ fill: {muted}; font: 12px ui-sans-serif, system-ui, sans-serif; }}
    .legend {{ fill: {fg}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
  ]]></style>
  <text class="title" x="{title_x}" y="{title_y}">{title}</text>
{note}{axis}{paths}  <g class="legend-row">
{legend}  </g>
{footer}
</svg>"##,
        w = geom.w,
        h = geom.h,
        repo = escape_xml(&overlay.repo),
        fg = theme.fg,
        muted = theme.muted,
        title_x = geom.pad,
        title_y = geom.pad - 14.0,
        title = title,
        note = note,
        axis = axis,
        paths = paths,
        legend = legend,
        footer = brand::footer_lockup(geom.w - geom.pad, geom.h - 8.0, theme),
    )
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
/// reserved for the footer link (and, on the overlay, the legend row).
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
/// tick set in one place so both renderers agree.
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

    /// Integer y-axis tick values (always includes 0 and y_max).
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
    // Pick a step that yields ~5 ticks at "nice" round values.
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

/// Power-of-ten ticks for the log y-axis: 0, 1, 10, 100, … up to y_max.
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
/// most of its stars in one launch-week cluster, and index-based ticks would
/// then overlap at the chart's right edge.
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

#[allow(clippy::too_many_arguments)]
fn render_axes(
    cfg: &ChartConfig,
    x_ticks: &[f32],
    y_ticks: &[i32],
    axis: TimeAxis,
    x_at: &impl Fn(f32) -> f32,
    y_at: &impl Fn(f32) -> f32,
    theme: &Theme,
) -> String {
    let pad = cfg.padding as f32;
    let plot_w = cfg.width as f32 - pad * 2.0;
    let mut s = String::new();
    s.push_str("  <g class=\"axes\">\n");
    for y in y_ticks {
        let yp = y_at(*y as f32);
        s.push_str(&format!(
            "    <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{grid}\" stroke-width=\"1\" opacity=\"0.6\" />\n",
            pad,
            yp,
            pad + plot_w,
            yp,
            grid = theme.grid,
        ));
        s.push_str(&format!(
            "    <text class=\"axis-label\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\">{}</text>\n",
            pad - 8.0,
            yp + 4.0,
            fmt_count(*y as u32),
        ));
    }
    if x_ticks.len() >= 2 {
        for &tick in x_ticks.iter().skip(1) {
            let xp = x_at(tick);
            let label = format_x_tick(tick, axis);
            s.push_str(&format!(
                "    <text class=\"axis-label\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>\n",
                xp,
                cfg.height as f32 - pad + 18.0,
                escape_xml(&label),
            ));
        }
    }
    s.push_str("  </g>\n");
    s
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
/// style age. Keeps labels short on the shared axis.
fn format_tick_days(secs: f32) -> String {
    let days = (secs / 86_400.0).round() as i64;
    if days >= 365 {
        let years = days as f32 / 365.0;
        format!("{years:.1}y")
    } else {
        format!("{days}d")
    }
}

/// Compact integer formatting for axis labels and the subtitle count:
/// 1234 → "1.2k", 1_500_000 → "1.5M". Deterministic.
fn fmt_count(n: u32) -> String {
    fmt_count_u64(n as u64)
}

/// `u64` variant for the downloads axis, which runs into the billions:
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
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
  ]]></style>
  <text x="{cx}" y="{cy}" text-anchor="middle" fill="{muted}"
        font-family="ui-sans-serif, system-ui, sans-serif" font-size="14">No star history available</text>
{footer}
</svg>"##,
        w = cfg.width,
        h = cfg.height,
        cx = cfg.width / 2,
        cy = cfg.height / 2,
        muted = theme.muted,
        footer = brand::footer_lockup(
            cfg.width as f32 - cfg.padding as f32,
            cfg.height as f32 - 8.0,
            theme,
        ),
    )
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

    #[test]
    fn render_svg_one_line_with_brand_ink_and_animation() {
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
        // Light-theme brand ink is index 0 of the light palette.
        assert!(svg.contains("#0a0a0a"));
        // Exactly one line → exactly one <animate>.
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
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn animated_svg_waves_the_dither_fill_and_stays_static_by_default() {
        let arrivals: Vec<_> = (0..24).map(|i| at(i * 86_400)).collect();
        let series = cumulative_series(&arrivals);
        let cfg = ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        };

        // On-site animate: the underfill swaps to the wave pattern and marches
        // its dither, while the line keeps its single draw-in <animate>.
        let anim = render_svg(&series, &cfg, &DARK, &animated_opts());
        assert!(anim.contains("id=\"gd-wave-fill\""));
        assert!(anim.contains("url(#gd-wave-fill)"));
        assert!(anim.contains("<animateTransform"));
        assert!(anim.contains("attributeName=\"patternTransform\""));
        assert!(anim.contains("class=\"motion\""));
        // Exactly one line-draw <animate> (the space-terminated tag), unchanged.
        assert_eq!(anim.matches("<animate ").count(), 1);
        // SMIL-stripped fallback is a fully-painted chart, never blank.
        assert!(anim.contains("stroke-dashoffset=\"0\""));
        assert!(anim.contains("prefers-reduced-motion: reduce"));
        // Deterministic bytes.
        assert_eq!(anim, render_svg(&series, &cfg, &DARK, &animated_opts()));

        // Default (static) chart keeps the same blue ordered-dither treatment,
        // frozen at phase zero, and never emits a marching transform.
        let stat = render_svg(&series, &cfg, &DARK, &ChartOpts::default());
        assert!(!stat.contains("animateTransform"));
        assert!(stat.contains("url(#gd-wave-fill)"));
        assert!(stat.contains("#358ff3"));
        assert!(stat.contains("gd-star-dither-mask"));
    }

    #[test]
    fn render_svg_static_frame_is_full_line_not_blank() {
        // The static (SMIL-stripped) frame MUST show the fully-drawn line:
        // the line <path> carries `stroke-dashoffset="0"` (end state), never
        // `stroke-dashoffset="{dash}"` (start state, line hidden). A consumer
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
        // The drawable line's static offset must be exactly 0.
        assert!(
            svg.contains(r#"stroke-dashoffset="0""#),
            "line path must bake the end-state offset (0 = fully drawn)"
        );
        // The animate still starts hidden (from={dash}) and freezes drawn.
        assert!(svg.contains(r#"from="#) && svg.contains(r#"to="0""#));
        assert!(svg.contains(r#"fill="freeze""#));
        // Guard the exact bug: the path element itself must NOT statically
        // offset by the dash length (which would hide the whole line). Derive
        // the dash and assert it isn't used as the static dashoffset.
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
        assert!(dash > 1, "sanity: a real line has non-trivial length");
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
        // Neither theme paints its canvas: the chart has to sit on whatever
        // README background the reader has, which the baked-per-theme
        // `<picture>` embed already guarantees is the matching one.
        assert!(!light.contains(r##"fill="#ffffff""##));
        assert!(!dark.contains(r##"fill="#0a0a0a""##));
        assert!(!light.contains("data-gitdebt-canvas"));
        assert!(light.contains("data-gitdebt-texture=\"true\""));
        assert!(dark.contains("data-gitdebt-texture=\"true\""));
        assert!(light.contains(r#"stroke-dashoffset="0""#));
        assert!(dark.contains(r#"stroke-dashoffset="0""#));
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
        assert!(svg.contains("#fafafa"));
        assert!(!svg.contains("#65a30d"));
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
        // Legend carries both slugs.
        assert!(svg1.contains("o/a"));
        assert!(svg1.contains("o/b"));
        assert!(svg1.contains("data-gitdebt-logo=\"true\""));
    }

    #[test]
    fn render_multi_svg_palette_per_theme() {
        let a = cumulative_series(&[at(1), at(2), at(3)]);
        let b = cumulative_series(&[at(1), at(2)]);
        let series = vec![("o/a".to_string(), a), ("o/b".to_string(), b)];
        let light = render_multi_svg(
            &series,
            &ChartConfig::default(),
            &crate::theme::LIGHT,
            &ChartOpts::default(),
        );
        // First two light palette colors must appear (series 0 and 1).
        assert!(light.contains("#087fea"));
        assert!(light.contains("#7c4dff"));
        let dark = render_multi_svg(
            &series,
            &ChartConfig::default(),
            &crate::theme::DARK,
            &ChartOpts::default(),
        );
        assert!(dark.contains("#358ff3"));
        assert!(dark.contains("#966eff"));
    }

    #[test]
    fn animated_multi_svg_moves_colored_dither_without_hiding_data() {
        let a = cumulative_series(&[at(1), at(2), at(3)]);
        let b = cumulative_series(&[at(1), at(2)]);
        let series = vec![("o/a".to_string(), a), ("o/b".to_string(), b)];
        let svg = render_multi_svg(&series, &ChartConfig::default(), &DARK, &animated_opts());
        assert!(svg.contains("id=\"gd-series-0\""));
        assert!(svg.contains("id=\"gd-series-1\""));
        assert!(svg.contains("#358ff3"));
        assert!(svg.contains("#966eff"));
        assert!(svg.contains("<animateTransform"));
        assert!(svg.contains("stroke-dashoffset=\"0\""));
        assert!(svg.contains("prefers-reduced-motion: reduce"));
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
        // Both categorical series colors are present.
        assert!(a.contains("#087fea"));
        assert!(a.contains("#7c4dff"));
        // Dual-axis legend carries both labels.
        assert!(a.contains("stars"));
        assert!(a.contains("npm downloads"));
        // Downloads axis humanizes into the millions.
        assert!(a.contains("2.0M"));
        assert!(a.contains("data-gitdebt-logo=\"true\""));
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
        // Stars line still drawn.
        assert!(svg.contains("#0a0a0a"));
        // No downloads line color when there's no downloads series.
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn overlay_dark_theme_bakes_dark_palette() {
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
        assert!(svg.contains("#358ff3")); // dark stars
        assert!(svg.contains("#966eff")); // dark downloads (index 1)
        assert!(!svg.contains("var(--"));
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
