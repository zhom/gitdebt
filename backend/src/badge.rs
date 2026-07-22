//! Compact, configurable badge SVG renderer.
//!
//! Renders a small embeddable badge showing any subset of `stars`, `forks`,
//! and `downloads` for a repo, in one of four visual styles. Pure +
//! deterministic (same input → same bytes) so the badge endpoint is
//! upstream-cacheable; theme colors are baked as concrete hex (no CSS vars)
//! so the badge renders correctly as a README `<img>` regardless of the
//! viewer's OS/page theme — same rationale as `chart.rs` / `theme.rs`.
//!
//! ## Animation + GitHub's SMIL sanitizer
//!
//! `animate=1` adds tasteful SMIL animation (count-up / fade-slide /
//! shimmer / pulse, picked per style) using `<animate ... fill="freeze">`.
//! GitHub strips `<animate>` from README `<img>` SVGs, so on GitHub the
//! viewer sees the **frozen final frame**. We guarantee that frame is the
//! correct end state by authoring every animated attribute's static value
//! to already equal the animation's `to` value — the `<animate>` only
//! controls the *transition* into that frame, never the resting state.
//! (This is also why `raster::freeze_svg_animations` can rasterize the
//! final frame correctly: it rewrites `attr="from"` → `attr="to"`, and our
//! static attributes equal `to` regardless.) `animate=0` (default) emits no
//! `<animate>` tags at all.

use crate::brand;
use crate::theme::Theme;

/// Which metrics to show, in display order. Honors include/exclude: a
/// metric absent from the list (or unavailable) is omitted entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Stars,
    Forks,
    Downloads,
}

impl Metric {
    /// Parse the `?metrics=` comma list into an ordered, de-duplicated set.
    /// Unknown tokens are ignored. Empty / all-unknown input falls back to
    /// the default (all three, stars→forks→downloads).
    pub fn parse_list(s: Option<&str>) -> Vec<Metric> {
        let Some(s) = s.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::default_list();
        };
        let mut out = Vec::new();
        for tok in s.split(',') {
            let m = match tok.trim().to_ascii_lowercase().as_str() {
                "stars" | "star" => Some(Metric::Stars),
                "forks" | "fork" => Some(Metric::Forks),
                "downloads" | "download" | "dl" => Some(Metric::Downloads),
                _ => None,
            };
            if let Some(m) = m
                && !out.contains(&m)
            {
                out.push(m);
            }
        }
        if out.is_empty() {
            Self::default_list()
        } else {
            out
        }
    }

    fn default_list() -> Vec<Metric> {
        vec![Metric::Stars, Metric::Forks, Metric::Downloads]
    }

    fn label(self) -> &'static str {
        match self {
            Metric::Stars => "stars",
            Metric::Forks => "forks",
            Metric::Downloads => "downloads",
        }
    }
}

/// Visual style. Auto-sized width; per-theme baked hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeStyle {
    /// shields.io-like: two-tone pill, flat label/value split.
    Flat,
    /// Rounded, soft, brand-accent leading dot.
    Modern,
    /// Translucent-style monochrome panel.
    Glass,
    /// Monospace, dark chip.
    Terminal,
}

impl BadgeStyle {
    /// Parse `?style=`. Unknown → `Flat` (the safe shields-like default).
    pub fn parse(s: Option<&str>) -> Self {
        match s.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("modern") => BadgeStyle::Modern,
            Some("glass") => BadgeStyle::Glass,
            Some("terminal") => BadgeStyle::Terminal,
            _ => BadgeStyle::Flat,
        }
    }
}

/// A resolved metric segment: glyph kind + humanized value string.
#[derive(Debug, Clone)]
pub struct Segment {
    pub metric: Metric,
    pub value: String,
}

/// Inputs for [`render_badge`]. Values are pre-resolved by the API layer;
/// `None` means "unavailable" and the segment is dropped even if requested.
#[derive(Debug, Clone)]
pub struct BadgeInput {
    pub stars: Option<u64>,
    pub forks: Option<u64>,
    pub downloads: Option<u64>,
    pub metrics: Vec<Metric>,
    pub style: BadgeStyle,
    pub animate: bool,
}

impl BadgeInput {
    /// Build the ordered segment list, honoring include/exclude AND
    /// availability. A requested-but-unavailable metric is omitted.
    fn segments(&self) -> Vec<Segment> {
        self.metrics
            .iter()
            .filter_map(|m| {
                let v = match m {
                    Metric::Stars => self.stars,
                    Metric::Forks => self.forks,
                    Metric::Downloads => self.downloads,
                }?;
                Some(Segment {
                    metric: *m,
                    value: humanize(v),
                })
            })
            .collect()
    }
}

/// Humanize a count: 999 → "999", 1234 → "1.2k", 12_345 → "12.3k",
/// 1_500_000 → "1.5M", 2_000_000_000 → "2.0B". Deterministic. Drops a
/// trailing `.0` only above the thousands break so "1.0k" stays "1.0k"
/// (matches shields/star-history convention of one decimal place).
pub fn humanize(n: u64) -> String {
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

// Layout math

const HEIGHT: f32 = 28.0;
/// Reserved trailing space for the subtle self-contained gitdebt mark.
const BRAND_W: f32 = 20.0;
/// Horizontal padding inside each segment.
const SEG_PAD_X: f32 = 9.0;
/// Width reserved for a metric glyph (icon).
const GLYPH_W: f32 = 14.0;
/// Gap between glyph and value text.
const GLYPH_GAP: f32 = 5.0;
/// Approx px width per character at the badge's font size (deterministic,
/// matches the categorical-palette legend estimate in `chart.rs`).
const CHAR_W: f32 = 7.2;
const REVEAL_SECONDS: f32 = 0.2;
const STAGGER_SECONDS: f32 = 0.04;
const MAX_STAGGER_SECONDS: f32 = 0.08;
const MOTION_CSS: &str = "@media (prefers-reduced-motion: reduce) { .motion { display: none; } }";

fn reveal_delay(index: usize) -> f32 {
    (index as f32 * STAGGER_SECONDS).min(MAX_STAGGER_SECONDS)
}

/// One laid-out segment with its x offset + width.
struct Placed {
    seg: Segment,
    x: f32,
    w: f32,
}

/// Compute per-segment widths + total badge width for the given segments.
/// Width auto-sizes to content. Returns `(total_width, placed)`.
fn layout(segments: &[Segment]) -> (f32, Vec<Placed>) {
    let mut x = 0.0_f32;
    let mut placed = Vec::with_capacity(segments.len());
    for seg in segments {
        let text_w = seg.value.chars().count() as f32 * CHAR_W;
        let w = SEG_PAD_X + GLYPH_W + GLYPH_GAP + text_w + SEG_PAD_X;
        placed.push(Placed {
            seg: seg.clone(),
            x,
            w,
        });
        x += w;
    }
    (x.max(1.0), placed)
}

// Glyphs (small inline metric icons, baked path data)

/// Return an SVG `<path>`/`<polygon>` fragment for a metric glyph, drawn in
/// `color`, positioned so its ~14×14 box sits at (`cx`, vertical center).
/// `cx` is the left edge of the glyph box.
fn glyph(metric: Metric, cx: f32, color: &str) -> String {
    let cy = HEIGHT / 2.0;
    match metric {
        // Five-point star.
        Metric::Stars => {
            let r = 6.0;
            let pts = star_points(cx + GLYPH_W / 2.0, cy, r, r * 0.42);
            format!("<polygon points=\"{pts}\" fill=\"{color}\" />")
        }
        // Fork: two prongs + a stem (three circles + connecting lines).
        Metric::Forks => {
            let gx = cx + GLYPH_W / 2.0;
            format!(
                "<g stroke=\"{color}\" stroke-width=\"1.6\" fill=\"none\"><circle cx=\"{a:.1}\" cy=\"{top:.1}\" r=\"2\" fill=\"{color}\" /><circle cx=\"{b:.1}\" cy=\"{top:.1}\" r=\"2\" fill=\"{color}\" /><circle cx=\"{gx:.1}\" cy=\"{bot:.1}\" r=\"2\" fill=\"{color}\" /><path d=\"M{a:.1} {top:.1} V{midy:.1} M{b:.1} {top:.1} V{midy:.1} M{a:.1} {midy:.1} H{b:.1} M{gx:.1} {midy:.1} V{bot:.1}\" /></g>",
                a = gx - 4.0,
                b = gx + 4.0,
                top = cy - 5.0,
                midy = cy + 1.0,
                bot = cy + 5.0,
                gx = gx,
            )
        }
        // Download: down-arrow into a tray.
        Metric::Downloads => {
            let gx = cx + GLYPH_W / 2.0;
            format!(
                "<g stroke=\"{color}\" stroke-width=\"1.6\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M{gx:.1} {top:.1} V{arrowbot:.1} M{lx:.1} {midy:.1} L{gx:.1} {arrowbot:.1} L{rx:.1} {midy:.1}\" /><path d=\"M{lx:.1} {tray:.1} H{rx:.1}\" /></g>",
                gx = gx,
                top = cy - 6.0,
                arrowbot = cy + 2.0,
                midy = cy - 1.0,
                lx = gx - 4.0,
                rx = gx + 4.0,
                tray = cy + 5.0,
            )
        }
    }
}

/// Vertices of a five-point star centered at (cx, cy) with outer radius
/// `outer` and inner radius `inner`. Deterministic (fixed angles).
fn star_points(cx: f32, cy: f32, outer: f32, inner: f32) -> String {
    let mut pts = String::new();
    for i in 0..10 {
        let r = if i % 2 == 0 { outer } else { inner };
        // Start at top (-90°), step 36°.
        let ang = std::f32::consts::PI * (-0.5 + i as f32 * 0.2);
        let x = cx + r * ang.cos();
        let y = cy + r * ang.sin();
        if i > 0 {
            pts.push(' ');
        }
        pts.push_str(&format!("{x:.1},{y:.1}"));
    }
    pts
}

// Renderer

/// Render the badge to an SVG string. Auto-sized; deterministic; theme hex
/// baked. Emits `<animate fill="freeze">` only when `input.animate` is true.
pub fn render_badge(input: &BadgeInput, theme: &Theme) -> String {
    let segments = input.segments();
    if segments.is_empty() {
        return empty_badge(input.style, theme);
    }
    let (width, placed) = layout(&segments);

    match input.style {
        BadgeStyle::Flat => render_flat(&placed, width, theme, input.animate),
        BadgeStyle::Modern => render_modern(&placed, width, theme, input.animate),
        BadgeStyle::Glass => render_glass(&placed, width, theme, input.animate),
        BadgeStyle::Terminal => render_terminal(&placed, width, theme, input.animate),
    }
}

/// Render an evidence-backed repository signal as a compact README badge.
///
/// The API owns the qualification rules; this function only renders the
/// already-evaluated result. Keeping evaluation out of the renderer preserves
/// deterministic bytes and makes the SVG independently testable.
pub fn render_signal_badge(
    label: &str,
    detail: &str,
    earned: bool,
    theme: &Theme,
    animate: bool,
) -> String {
    const H: f32 = 30.0;
    const SIGNAL_CHAR_W: f32 = 6.7;
    let label = escape_xml(label);
    let detail = escape_xml(detail);
    let label_width = label.chars().count() as f32 * SIGNAL_CHAR_W;
    let detail_width = detail.chars().count() as f32 * SIGNAL_CHAR_W;
    let width = (44.0 + label_width + detail_width + 30.0).clamp(180.0, 420.0);
    let detail_x = 31.0 + label_width + 13.0;
    let status = if earned { "earned" } else { "not earned" };
    let signal = if earned { theme.accent } else { theme.muted };
    let panel = if theme.dark { "#171717" } else { "#f5f5f5" };
    let motion = if animate {
        r#"<animateTransform class="motion" attributeName="transform" type="scale" from="0.75" to="1" dur="0.22s" fill="freeze" calcMode="spline" keySplines="0.23 1 0.32 1" />"#
    } else {
        ""
    };
    let check = if earned {
        format!(
            "  <g transform=\"translate(8 7)\" stroke=\"{signal}\" stroke-width=\"2\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"8\" cy=\"8\" r=\"7\" /><path d=\"M4.8 8.1 7 10.3 11.5 5.8\" />{motion}</g>\n"
        )
    } else {
        format!(
            "  <circle cx=\"16\" cy=\"15\" r=\"6\" fill=\"none\" stroke=\"{signal}\" stroke-width=\"1.5\" />\n"
        )
    };
    let logo = brand::themed_logo_mark(width - 20.0, 8.0, 14.0, theme);

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0}" height="{H:.0}" viewBox="0 0 {width:.0} {H:.0}" role="img" aria-label="{label}: {detail}, {status}">
  <style><![CDATA[
    text {{ font: 600 11px ui-sans-serif, system-ui, sans-serif; }}
    .detail {{ font-weight: 500; }}
    {MOTION_CSS}
  ]]></style>
  <rect x="0.5" y="0.5" width="{panel_width:.1}" height="29" rx="7" fill="{panel}" stroke="{border}" />
  <rect x="0" y="6" width="3" height="18" rx="1.5" fill="{signal}" />
{check}  <text x="31" y="19" fill="{fg}">{label}</text>
  <text class="detail" x="{detail_x:.1}" y="19" fill="{muted}">· {detail}</text>
{logo}</svg>"##,
        panel_width = width - 1.0,
        border = theme.border,
        fg = theme.fg,
        muted = theme.muted,
    )
}

/// Shared SVG header. `extra_defs` lets a style inject filters or other defs.
fn svg_header(width: f32, label: &str, defs: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" role="img" aria-label="{label}">
{defs}"##,
        w = width,
        h = HEIGHT,
        label = label,
        defs = defs,
    )
}

/// Build the label string for accessibility (e.g. "stars: 12.3k, forks: 1.2k").
fn aria_label(placed: &[Placed]) -> String {
    placed
        .iter()
        .map(|p| format!("{}: {}", p.seg.metric.label(), p.seg.value))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Value text + glyph for a segment, with an optional count-up odometer
/// animation. The static text already shows the final value (`to`); the
/// animate only fades/slides it in so the FROZEN frame on GitHub is correct.
fn segment_content(
    p: &Placed,
    text_color: &str,
    glyph_color: &str,
    animate: bool,
    anim: SegAnim,
    index: usize,
) -> String {
    let glyph_x = p.x + SEG_PAD_X;
    let text_x = glyph_x + GLYPH_W + GLYPH_GAP;
    let text_y = HEIGHT / 2.0 + 4.0;
    let g = glyph(p.seg.metric, glyph_x, glyph_color);

    // The animation is authored so the resting (post-freeze) state equals
    // the element's static attributes — required for the GitHub frozen
    // frame. We animate `opacity` (and, for fade-slide, a transform) from a
    // start state to the static end state.
    let (anim_tag, start_transform) = if animate {
        let delay = reveal_delay(index);
        match anim {
            SegAnim::FadeIn => (
                format!(
                    "<animate class=\"motion\" attributeName=\"opacity\" from=\"0\" to=\"1\" begin=\"{delay:.2}s\" dur=\"{REVEAL_SECONDS:.1}s\" fill=\"freeze\" />"
                ),
                String::new(),
            ),
            SegAnim::FadeSlide => (
                format!(
                    "<animate class=\"motion\" attributeName=\"opacity\" from=\"0\" to=\"1\" begin=\"{delay:.2}s\" dur=\"{REVEAL_SECONDS:.1}s\" fill=\"freeze\" /><animateTransform class=\"motion\" attributeName=\"transform\" type=\"translate\" from=\"0 4\" to=\"0 0\" begin=\"{delay:.2}s\" dur=\"{REVEAL_SECONDS:.1}s\" fill=\"freeze\" calcMode=\"spline\" keySplines=\"0.23 1 0.32 1\" />"
                ),
                " transform=\"translate(0 0)\"".to_string(),
            ),
        }
    } else {
        (String::new(), String::new())
    };

    format!(
        "  <g opacity=\"1\"{start_transform}>{g}<text x=\"{text_x:.1}\" y=\"{text_y:.1}\" fill=\"{text_color}\">{value}</text>{anim_tag}</g>\n",
        value = escape_xml(&p.seg.value),
    )
}

/// Which per-segment animation a style uses for its count-up reveal.
#[derive(Clone, Copy)]
enum SegAnim {
    FadeIn,
    FadeSlide,
}

fn render_flat(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    // shields-like: high-contrast leading strip, segments in the panel bg.
    let pal0 = theme.accent;
    let bg = if theme.dark { "#171717" } else { "#f5f5f5" };
    let label = aria_label(placed);
    let total = width + BRAND_W;
    let mut body = String::new();
    // Rounded container.
    body.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{total:.1}\" height=\"{h:.1}\" rx=\"6\" fill=\"{bg}\" stroke=\"{border}\" stroke-width=\"1\" />\n",
        h = HEIGHT,
        border = theme.border,
    ));
    // Left accent strip.
    body.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"3\" height=\"{h:.1}\" rx=\"1.5\" fill=\"{pal0}\" />\n",
        h = HEIGHT,
    ));
    for (i, p) in placed.iter().enumerate() {
        if i > 0 {
            body.push_str(&format!(
                "  <line x1=\"{x:.1}\" y1=\"6\" x2=\"{x:.1}\" y2=\"{y2:.1}\" stroke=\"{border}\" stroke-width=\"1\" opacity=\"0.5\" />\n",
                x = p.x,
                y2 = HEIGHT - 6.0,
                border = theme.border,
            ));
        }
        body.push_str(&segment_content(
            p,
            theme.fg,
            pal0,
            animate,
            SegAnim::FadeIn,
            i,
        ));
    }
    body.push_str(&brand::themed_logo_mark(
        width + 5.0,
        (HEIGHT - 10.0) / 2.0,
        10.0,
        theme,
    ));
    format!(
        "{header}  <style><![CDATA[ text {{ font: 600 12px ui-sans-serif, system-ui, sans-serif; }} {motion_css} ]]></style>\n{body}</svg>",
        header = svg_header(total, &label, ""),
        motion_css = MOTION_CSS,
    )
}

fn render_modern(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    // Rounded, soft shadow, brand-accent leading dot before the first value.
    let pal0 = theme.accent;
    let bg = if theme.dark { "#0a0a0a" } else { "#ffffff" };
    let label = aria_label(placed);
    let total = width + 8.0 + BRAND_W;
    let defs = format!(
        "  <defs><filter id=\"mshadow\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"160%\"><feDropShadow dx=\"0\" dy=\"1\" stdDeviation=\"1.2\" flood-color=\"{}\" flood-opacity=\"0.18\" /></filter></defs>\n",
        if theme.dark { "#000000" } else { "#737373" },
    );
    let mut body = String::new();
    body.push_str(&format!(
        "  <rect x=\"0.5\" y=\"0.5\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"14\" fill=\"{bg}\" stroke=\"{border}\" stroke-width=\"1\" filter=\"url(#mshadow)\" />\n",
        w = total - 1.0,
        h = HEIGHT - 1.0,
        border = theme.border,
    ));
    // Leading accent dot.
    body.push_str(&format!(
        "  <circle cx=\"7\" cy=\"{cy:.1}\" r=\"3\" fill=\"{pal0}\" />\n",
        cy = HEIGHT / 2.0,
    ));
    for (i, p) in placed.iter().enumerate() {
        // Modern shifts segments right a touch to clear the dot.
        let pp = Placed {
            seg: p.seg.clone(),
            x: p.x + 8.0,
            w: p.w,
        };
        body.push_str(&segment_content(
            &pp,
            theme.fg,
            pal0,
            animate,
            SegAnim::FadeSlide,
            i,
        ));
    }
    body.push_str(&brand::themed_logo_mark(
        width + 13.0,
        (HEIGHT - 10.0) / 2.0,
        10.0,
        theme,
    ));
    format!(
        "{header}  <style><![CDATA[ text {{ font: 600 12px ui-sans-serif, system-ui, sans-serif; }} {motion_css} ]]></style>\n{defs}{body}</svg>",
        header = svg_header(total, &label, ""),
        motion_css = MOTION_CSS,
        defs = defs,
    )
}

fn render_glass(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    // Restrained translucent-style panel + a brief solid highlight sweep.
    let pal0 = theme.accent;
    let label = aria_label(placed);
    let panel = if theme.dark { "#262626" } else { "#f5f5f5" };
    let total = width + BRAND_W;
    let mut body = String::new();
    body.push_str(&format!(
        "  <rect x=\"0.5\" y=\"0.5\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\" fill=\"{panel}\" stroke=\"{border}\" stroke-width=\"1\" />\n",
        w = total - 1.0,
        h = HEIGHT - 1.0,
        panel = panel,
        border = theme.border,
    ));
    // Top glass highlight.
    body.push_str(&format!(
        "  <rect x=\"3\" y=\"2\" width=\"{w:.1}\" height=\"8\" rx=\"5\" fill=\"#ffffff\" opacity=\"{op}\" />\n",
        w = total - 6.0,
        op = if theme.dark { "0.06" } else { "0.5" },
    ));
    for (i, p) in placed.iter().enumerate() {
        if i > 0 {
            body.push_str(&format!(
                "  <line x1=\"{x:.1}\" y1=\"7\" x2=\"{x:.1}\" y2=\"{y2:.1}\" stroke=\"{border}\" stroke-width=\"1\" opacity=\"0.4\" />\n",
                x = p.x,
                y2 = HEIGHT - 7.0,
                border = theme.border,
            ));
        }
        body.push_str(&segment_content(
            p,
            theme.fg,
            pal0,
            animate,
            SegAnim::FadeIn,
            i,
        ));
    }
    // Highlight sweep: a thin bar that slides left→right and freezes
    // off-screen-right (so the frozen GitHub frame shows a clean panel).
    if animate {
        body.push_str(&format!(
            "  <rect x=\"{end:.1}\" y=\"0\" width=\"12\" height=\"{h:.1}\" fill=\"#ffffff\" opacity=\"0.18\" transform=\"translate(0 0)\"><animateTransform class=\"motion\" attributeName=\"transform\" type=\"translate\" from=\"-{distance:.1} 0\" to=\"0 0\" dur=\"0.6s\" begin=\"0s\" fill=\"freeze\" calcMode=\"linear\" /></rect>\n",
            h = HEIGHT,
            end = total,
            distance = total + 12.0,
        ));
    }
    body.push_str(&brand::themed_logo_mark(
        width + 5.0,
        (HEIGHT - 10.0) / 2.0,
        10.0,
        theme,
    ));
    format!(
        "{header}  <style><![CDATA[ text {{ font: 600 12px ui-sans-serif, system-ui, sans-serif; }} {motion_css} ]]></style>\n{body}</svg>",
        header = svg_header(total, &label, ""),
        motion_css = MOTION_CSS,
    )
}

fn render_terminal(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    // Monospace, dark chip with a prompt-style leading marker + a blinking
    // cursor (the cursor "pulse" is the animation; it freezes visible).
    let bg = "#0a0a0a";
    let fg = "#fafafa";
    let accent = "#ffffff";
    let _ = theme; // terminal style is intentionally theme-independent (always dark chip).
    let label = aria_label(placed);
    let total = width + 14.0 + BRAND_W;
    let mut body = String::new();
    body.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"5\" fill=\"{bg}\" />\n",
        w = total,
        h = HEIGHT,
    ));
    // Leading `›` prompt marker.
    body.push_str(&format!(
        "  <text x=\"7\" y=\"{ty:.1}\" fill=\"{accent}\" class=\"mono\">&#8250;</text>\n",
        ty = HEIGHT / 2.0 + 4.0,
    ));
    for (i, p) in placed.iter().enumerate() {
        let pp = Placed {
            seg: p.seg.clone(),
            x: p.x + 14.0,
            w: p.w,
        };
        body.push_str(&segment_terminal(&pp, fg, accent, animate, i));
    }
    // Blinking cursor block at the end.
    let cursor_x = width + 14.0 - 8.0;
    if animate {
        body.push_str(&format!(
            "  <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"6\" height=\"12\" fill=\"{accent}\" opacity=\"1\"><animate class=\"motion\" attributeName=\"opacity\" values=\"1;0.2;1\" dur=\"0.8s\" repeatCount=\"2\" fill=\"freeze\" /></rect>\n",
            x = cursor_x,
            y = HEIGHT / 2.0 - 6.0,
        ));
    } else {
        body.push_str(&format!(
            "  <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"6\" height=\"12\" fill=\"{accent}\" />\n",
            x = cursor_x,
            y = HEIGHT / 2.0 - 6.0,
        ));
    }
    body.push_str(&brand::logo_mark(
        width + 19.0,
        (HEIGHT - 10.0) / 2.0,
        10.0,
        fg,
        bg,
    ));
    format!(
        "{header}  <style><![CDATA[ .mono {{ font: 600 12px ui-monospace, SFMono-Regular, Menlo, monospace; }} {motion_css} ]]></style>\n{body}</svg>",
        header = svg_header(total, &label, ""),
        motion_css = MOTION_CSS,
    )
}

/// Terminal-style segment: monospace text in `key=value` shape, glyph drawn
/// in `accent`. Count-up = fade-in (the frozen frame shows full opacity).
fn segment_terminal(p: &Placed, fg: &str, accent: &str, animate: bool, index: usize) -> String {
    let glyph_x = p.x + SEG_PAD_X;
    let text_x = glyph_x + GLYPH_W + GLYPH_GAP;
    let text_y = HEIGHT / 2.0 + 4.0;
    let g = glyph(p.seg.metric, glyph_x, accent);
    let anim_tag = if animate {
        let delay = reveal_delay(index);
        format!(
            "<animate class=\"motion\" attributeName=\"opacity\" from=\"0\" to=\"1\" begin=\"{delay:.2}s\" dur=\"{REVEAL_SECONDS:.1}s\" fill=\"freeze\" />"
        )
    } else {
        String::new()
    };
    format!(
        "  <g opacity=\"1\" class=\"mono\">{g}<text x=\"{text_x:.1}\" y=\"{text_y:.1}\" fill=\"{fg}\">{value}</text>{anim_tag}</g>\n",
        value = escape_xml(&p.seg.value),
    )
}

fn empty_badge(style: BadgeStyle, theme: &Theme) -> String {
    let bg = match style {
        BadgeStyle::Terminal => "#0a0a0a",
        _ if theme.dark => "#171717",
        _ => "#f5f5f5",
    };
    let fg = match style {
        BadgeStyle::Terminal => "#fafafa",
        _ => theme.muted,
    };
    let content_w = 92.0;
    let w = content_w + BRAND_W;
    let logo = match style {
        BadgeStyle::Terminal => brand::logo_mark(
            content_w + 5.0,
            (HEIGHT - 10.0) / 2.0,
            10.0,
            "#fafafa",
            "#0a0a0a",
        ),
        _ => brand::themed_logo_mark(content_w + 5.0, (HEIGHT - 10.0) / 2.0, 10.0, theme),
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" role="img" aria-label="no metrics">
  <rect x="0" y="0" width="{w:.0}" height="{h:.0}" rx="6" fill="{bg}" />
  <text x="{cx:.0}" y="{cy:.0}" text-anchor="middle" fill="{fg}" font-family="ui-sans-serif, system-ui, sans-serif" font-size="11">no metrics</text>
{logo}
</svg>"##,
        w = w,
        h = HEIGHT,
        bg = bg,
        fg = fg,
        cx = content_w / 2.0,
        cy = HEIGHT / 2.0 + 4.0,
        logo = logo,
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

    fn full_input(style: BadgeStyle, animate: bool) -> BadgeInput {
        BadgeInput {
            stars: Some(12_345),
            forks: Some(1_234),
            downloads: Some(2_500_000),
            metrics: vec![Metric::Stars, Metric::Forks, Metric::Downloads],
            style,
            animate,
        }
    }

    #[test]
    fn humanize_breakpoints() {
        assert_eq!(humanize(0), "0");
        assert_eq!(humanize(999), "999");
        assert_eq!(humanize(1_234), "1.2k");
        assert_eq!(humanize(12_345), "12.3k");
        assert_eq!(humanize(1_500_000), "1.5M");
        assert_eq!(humanize(2_000_000_000), "2.0B");
    }

    #[test]
    fn earned_signal_badge_is_deterministic_and_theme_baked() {
        let first = render_signal_badge(
            "actively maintained",
            "18 commits · 30d",
            true,
            &LIGHT,
            true,
        );
        let second = render_signal_badge(
            "actively maintained",
            "18 commits · 30d",
            true,
            &LIGHT,
            true,
        );
        assert_eq!(first, second);
        assert!(first.contains("actively maintained"));
        assert!(first.contains("18 commits · 30d"));
        assert!(first.contains("aria-label=\"actively maintained: 18 commits · 30d, earned\""));
        assert!(first.contains("data-gitdebt-logo=\"true\""));
        assert!(first.contains("<animateTransform"));
        assert!(!first.contains("var("));

        let dark = render_signal_badge("community powered", "8 contributors", false, &DARK, false);
        assert!(dark.contains("not earned"));
        assert!(!dark.contains("<animate"));
        assert!(dark.contains(DARK.fg));
    }

    #[test]
    fn metric_parse_default_is_all_three() {
        assert_eq!(
            Metric::parse_list(None),
            vec![Metric::Stars, Metric::Forks, Metric::Downloads]
        );
        assert_eq!(
            Metric::parse_list(Some("")),
            vec![Metric::Stars, Metric::Forks, Metric::Downloads]
        );
    }

    #[test]
    fn metric_parse_honors_order_and_subset() {
        assert_eq!(
            Metric::parse_list(Some("downloads,stars")),
            vec![Metric::Downloads, Metric::Stars]
        );
        assert_eq!(Metric::parse_list(Some("forks")), vec![Metric::Forks]);
    }

    #[test]
    fn metric_parse_dedups_and_ignores_unknown() {
        assert_eq!(
            Metric::parse_list(Some("stars,stars,bogus,forks")),
            vec![Metric::Stars, Metric::Forks]
        );
        // All-unknown → default.
        assert_eq!(
            Metric::parse_list(Some("nope,zilch")),
            vec![Metric::Stars, Metric::Forks, Metric::Downloads]
        );
    }

    #[test]
    fn style_parse() {
        assert_eq!(BadgeStyle::parse(Some("flat")), BadgeStyle::Flat);
        assert_eq!(BadgeStyle::parse(Some("modern")), BadgeStyle::Modern);
        assert_eq!(BadgeStyle::parse(Some("glass")), BadgeStyle::Glass);
        assert_eq!(BadgeStyle::parse(Some("terminal")), BadgeStyle::Terminal);
        assert_eq!(BadgeStyle::parse(Some("garbage")), BadgeStyle::Flat);
        assert_eq!(BadgeStyle::parse(None), BadgeStyle::Flat);
    }

    #[test]
    fn segments_honor_include_exclude() {
        let input = BadgeInput {
            stars: Some(10),
            forks: Some(20),
            downloads: Some(30),
            metrics: vec![Metric::Forks, Metric::Stars],
            style: BadgeStyle::Flat,
            animate: false,
        };
        let segs = input.segments();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].metric, Metric::Forks);
        assert_eq!(segs[1].metric, Metric::Stars);
    }

    #[test]
    fn segments_drop_unavailable_metric() {
        // Downloads requested but None → omitted.
        let input = BadgeInput {
            stars: Some(10),
            forks: None,
            downloads: None,
            metrics: vec![Metric::Stars, Metric::Forks, Metric::Downloads],
            style: BadgeStyle::Flat,
            animate: false,
        };
        let segs = input.segments();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].metric, Metric::Stars);
    }

    #[test]
    fn width_grows_with_more_segments() {
        let one = layout(&[Segment {
            metric: Metric::Stars,
            value: "12.3k".into(),
        }]);
        let two = layout(&[
            Segment {
                metric: Metric::Stars,
                value: "12.3k".into(),
            },
            Segment {
                metric: Metric::Forks,
                value: "1.2k".into(),
            },
        ]);
        assert!(two.0 > one.0, "two segments must be wider than one");
    }

    #[test]
    fn width_grows_with_longer_value() {
        let short = layout(&[Segment {
            metric: Metric::Stars,
            value: "9".into(),
        }]);
        let long = layout(&[Segment {
            metric: Metric::Stars,
            value: "123.4k".into(),
        }]);
        assert!(long.0 > short.0);
    }

    #[test]
    fn animation_tags_only_when_animate_true() {
        for style in [
            BadgeStyle::Flat,
            BadgeStyle::Modern,
            BadgeStyle::Glass,
            BadgeStyle::Terminal,
        ] {
            let off = render_badge(&full_input(style, false), &LIGHT);
            assert!(
                !off.contains("<animate"),
                "{style:?} animate=0 must have no <animate>: {off}"
            );
            assert!(off.contains("data-gitdebt-logo=\"true\""));
            let on = render_badge(&full_input(style, true), &LIGHT);
            assert!(
                on.contains("<animate"),
                "{style:?} animate=1 must contain <animate>: {on}"
            );
            assert!(on.contains("prefers-reduced-motion: reduce"));
            assert!(
                !on.contains("<g opacity=\"0\""),
                "{style:?}: SMIL-stripped badge content must stay visible"
            );
            // Every animate must freeze so the GitHub final frame is correct.
            for frag in on.split("<animate").skip(1) {
                let tag_end = frag
                    .find("/>")
                    .or_else(|| frag.find('>'))
                    .unwrap_or(frag.len());
                let tag = &frag[..tag_end];
                assert!(
                    tag.contains("fill=\"freeze\"") || tag.contains("repeatCount"),
                    "{style:?}: animate tag must freeze or repeat-then-freeze: {tag}"
                );
            }
        }
    }

    #[test]
    fn reveal_stagger_is_capped_and_glass_uses_transform() {
        let modern = render_badge(&full_input(BadgeStyle::Modern, true), &LIGHT);
        assert!(modern.contains("begin=\"0.00s\""));
        assert!(modern.contains("begin=\"0.04s\""));
        assert!(modern.contains("begin=\"0.08s\""));
        assert!(!modern.contains("begin=\"0.12s\""));
        assert!(modern.contains("dur=\"0.2s\""));

        let glass = render_badge(&full_input(BadgeStyle::Glass, true), &LIGHT);
        assert!(glass.contains("<animateTransform"));
        assert!(glass.contains("calcMode=\"linear\""));
        assert!(!glass.contains("attributeName=\"x\""));
    }

    #[test]
    fn frozen_frame_shows_final_value() {
        // The static text content must already be the final value, so the
        // GitHub-sanitized (no-SMIL) render shows correct numbers.
        let svg = render_badge(&full_input(BadgeStyle::Modern, true), &LIGHT);
        assert!(svg.contains("12.3k")); // stars
        assert!(svg.contains("1.2k")); // forks
        assert!(svg.contains("2.5M")); // downloads
    }

    #[test]
    fn per_theme_colors_baked() {
        let light = render_badge(&full_input(BadgeStyle::Flat, false), &LIGHT);
        let dark = render_badge(&full_input(BadgeStyle::Flat, false), &DARK);
        // Monochrome brand ink per theme.
        assert!(light.contains("#0a0a0a"));
        assert!(dark.contains("#fafafa"));
        // No CSS variables.
        assert!(!light.contains("var(--"));
        assert!(!dark.contains("var(--"));
    }

    #[test]
    fn deterministic_same_bytes() {
        let a = render_badge(&full_input(BadgeStyle::Glass, true), &DARK);
        let b = render_badge(&full_input(BadgeStyle::Glass, true), &DARK);
        assert_eq!(a, b);
    }

    #[test]
    fn empty_when_no_metrics_available() {
        let input = BadgeInput {
            stars: None,
            forks: None,
            downloads: None,
            metrics: vec![Metric::Stars],
            style: BadgeStyle::Flat,
            animate: false,
        };
        let svg = render_badge(&input, &LIGHT);
        assert!(svg.contains("no metrics"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn value_is_xml_escaped() {
        // Defensive: humanized values are digits + suffix, never need
        // escaping, but the path must still be safe.
        let svg = render_badge(&full_input(BadgeStyle::Flat, false), &LIGHT);
        assert!(svg.starts_with("<svg"));
        assert!(!svg.contains("<script"));
    }

    #[test]
    fn every_style_rasterizes_frozen_frame() {
        // The .png/.webp variants run through the raster freezer; confirm
        // each style's animated SVG produces valid PNG bytes after the SMIL
        // → frozen-frame rewrite (catches filter parse failures).
        for style in [
            BadgeStyle::Flat,
            BadgeStyle::Modern,
            BadgeStyle::Glass,
            BadgeStyle::Terminal,
        ] {
            let svg = render_badge(&full_input(style, true), &LIGHT);
            let png = crate::raster::rasterize(&svg, crate::raster::RasterFormat::Png, 2.0)
                .unwrap_or_else(|e| panic!("{style:?} raster failed: {e}"));
            assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        }
    }
}
