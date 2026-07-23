//! Compact, configurable badge SVG renderer.
//!
//! Renders a small embeddable badge showing any subset of `stars`, `forks`,
//! and `downloads` for a repo, in one of four visual styles, plus the
//! evidence-backed signal badge. Pure + deterministic (same input → same
//! bytes) so the badge endpoint is upstream-cacheable; theme colors are
//! baked as concrete hex (no CSS vars) so the badge renders correctly as a
//! README `<img>` regardless of the viewer's OS/page theme — same rationale
//! as `chart.rs` / `theme.rs`.
//!
//! ## Layout discipline
//!
//! Badge text uses a mono stack with a fixed per-character advance
//! (`0.6em`), and every `<text>` is pinned with `textLength` +
//! `lengthAdjust="spacingAndGlyphs"` so browsers and the resvg raster path
//! produce identical geometry (verified by a raster unit test). Character
//! counts happen **before** XML escaping. After the width clamp, label and
//! detail are truncated with a real ellipsis and every x position is
//! recomputed from the final text widths, so content can never underlap
//! the trailing brand chip.
//!
//! ## Animation + GitHub's SMIL sanitizer
//!
//! `animate=1` adds tasteful SMIL animation using `<animate …
//! fill="freeze">`. GitHub strips `<animate>` from README `<img>` SVGs, so
//! on GitHub the viewer sees the **frozen final frame**. We guarantee that
//! frame is correct by authoring every animated attribute's static value
//! to already equal the animation's end state; `<animateTransform>` always
//! carries `additive="sum"` so the element's static transform survives
//! (SMIL's default replace semantics would discard it). `animate=0`
//! (default) emits no `<animate>` tags at all.

use crate::brand;
use crate::texture;
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
    /// shields.io-like: bordered chip, flat segments, leading accent strip.
    Flat,
    /// Rounded pill, soft shadow, leading accent dot.
    Modern,
    /// Translucent-style lifted panel with a top highlight.
    Glass,
    /// Prompt-and-cursor dark chip (always dark, theme-independent).
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

/// Uniform badge height across the whole family (metric styles + signal +
/// empty) so side-by-side README embeds align.
pub const HEIGHT: f32 = 28.0;
/// Reserved trailing zone for the brand chip: 5px gap + 12px chip + 5px
/// right margin.
const BRAND_W: f32 = 22.0;
const MARK_SIZE: f32 = 12.0;
/// Horizontal padding inside each segment.
const SEG_PAD_X: f32 = 9.0;
/// Width reserved for a metric glyph (icon).
const GLYPH_W: f32 = 14.0;
/// Gap between glyph and value text.
const GLYPH_GAP: f32 = 5.0;
/// Mono advance at 12px (0.6em) — pinned via `textLength`, so this is the
/// rendered width, not an estimate.
const CHAR_W: f32 = 7.2;
/// Mono advance at the signal badge's 11px text.
const SIGNAL_CHAR_W: f32 = 6.6;
const REVEAL_SECONDS: f32 = 0.2;
const STAGGER_SECONDS: f32 = 0.04;
const MAX_STAGGER_SECONDS: f32 = 0.08;
const MOTION_CSS: &str = "@media (prefers-reduced-motion: reduce) { .motion { display: none; } }";
/// The shared badge type stack. `raster.rs` maps the generic `monospace`
/// keyword onto the bundled font so PNG/WebP geometry matches the pinned
/// `textLength` advance.
const FONT_MONO: &str = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

fn reveal_delay(index: usize) -> f32 {
    (index as f32 * STAGGER_SECONDS).min(MAX_STAGGER_SECONDS)
}

/// A pinned `<text>` element: mono, fixed advance, `textLength` +
/// `lengthAdjust` so client and raster geometry agree. `chars` is counted
/// by the caller BEFORE escaping.
fn pinned_text(x: f32, y: f32, fill: &str, class: &str, text: &str, advance: f32) -> String {
    let class_attr = if class.is_empty() {
        String::new()
    } else {
        format!(" class=\"{class}\"")
    };
    format!(
        "<text{class_attr} x=\"{x:.1}\" y=\"{y:.1}\" fill=\"{fill}\" textLength=\"{advance:.1}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>",
        escape_xml(text),
    )
}

/// One laid-out segment with its x offset + width.
struct Placed {
    seg: Segment,
    x: f32,
    w: f32,
}

/// Compute per-segment widths + total content width for the given segments.
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

// Shared chrome

/// Panel tone per theme — a quiet dark-first surface one step off the
/// canvas, with a hairline border.
fn panel_tone(theme: &Theme) -> &'static str {
    if theme.dark { "#141414" } else { "#f5f5f5" }
}

/// The Bayer wash defs a badge needs: the used density tier in the theme's
/// fg ink (alpha carried by the consuming rect, one ink only).
fn wash_defs(theme: &Theme, tier: usize) -> String {
    format!(
        "  <defs>{}</defs>\n",
        texture::tier_pattern(theme.fg, 2.0, tier)
    )
}

/// The Bayer wash rect covering the panel interior.
fn wash_rect(x: f32, y: f32, w: f32, h: f32, rx: f32, tier: usize, opacity: &str) -> String {
    format!(
        "  <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"{rx:.1}\" fill=\"{}\" fill-opacity=\"{opacity}\" />\n",
        texture::tier_fill(tier),
    )
}

/// Trailing brand chip, right-aligned inside the reserved [`BRAND_W`] zone.
fn brand_chip(total: f32, ink: &str) -> String {
    brand::chip_mark(
        total - BRAND_W + 5.0,
        (HEIGHT - MARK_SIZE) / 2.0,
        MARK_SIZE,
        ink,
    )
}

/// Shared SVG header. `defs` lets a style inject filters or other defs.
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

fn badge_style_css() -> String {
    format!("  <style><![CDATA[ text {{ font: 600 12px {FONT_MONO}; }} {MOTION_CSS} ]]></style>\n")
}

/// Build the label string for accessibility (e.g. "stars: 12.3k, forks: 1.2k").
fn aria_label(placed: &[Placed]) -> String {
    placed
        .iter()
        .map(|p| format!("{}: {}", p.seg.metric.label(), p.seg.value))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Value text + glyph for a segment, with an optional reveal animation.
/// The static markup already shows the final state; the animate only
/// fades/slides it in so the FROZEN frame on GitHub is correct.
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
    let advance = p.seg.value.chars().count() as f32 * CHAR_W;

    // The animation is authored so the resting (post-freeze) state equals
    // the element's static attributes — required for the GitHub frozen
    // frame. We animate `opacity` (and, for fade-slide, an additive
    // transform so the static transform list survives SMIL).
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
                    "<animate class=\"motion\" attributeName=\"opacity\" from=\"0\" to=\"1\" begin=\"{delay:.2}s\" dur=\"{REVEAL_SECONDS:.1}s\" fill=\"freeze\" /><animateTransform class=\"motion\" attributeName=\"transform\" type=\"translate\" from=\"0 4\" to=\"0 0\" begin=\"{delay:.2}s\" dur=\"{REVEAL_SECONDS:.1}s\" fill=\"freeze\" additive=\"sum\" calcMode=\"spline\" keySplines=\"0.23 1 0.32 1\" />"
                ),
                " transform=\"translate(0 0)\"".to_string(),
            ),
        }
    } else {
        (String::new(), String::new())
    };

    format!(
        "  <g opacity=\"1\"{start_transform}>{g}{text}{anim_tag}</g>\n",
        text = pinned_text(text_x, text_y, text_color, "", &p.seg.value, advance),
    )
}

/// Which per-segment animation a style uses for its reveal.
#[derive(Clone, Copy)]
enum SegAnim {
    FadeIn,
    FadeSlide,
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

fn render_flat(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    let wave = texture::wave_ink(theme);
    let label = aria_label(placed);
    let total = width + BRAND_W;
    let mut body = String::new();
    // Bordered chip + Bayer wash.
    body.push_str(&format!(
        "  <rect x=\"0.5\" y=\"0.5\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"6\" fill=\"{bg}\" stroke=\"{border}\" stroke-width=\"1\" />\n",
        w = total - 1.0,
        h = HEIGHT - 1.0,
        bg = panel_tone(theme),
        border = theme.border,
    ));
    body.push_str(&wash_rect(
        1.0,
        1.0,
        total - 2.0,
        HEIGHT - 2.0,
        5.0,
        2,
        "0.10",
    ));
    // Left accent strip — the single chromatic element.
    body.push_str(&format!(
        "  <rect x=\"0\" y=\"5\" width=\"3\" height=\"{h:.1}\" rx=\"1.5\" fill=\"{wave}\" />\n",
        h = HEIGHT - 10.0,
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
            wave,
            animate,
            SegAnim::FadeIn,
            i,
        ));
    }
    body.push_str(&brand_chip(total, wave));
    format!(
        "{header}{css}{body}</svg>",
        header = svg_header(total, &label, &wash_defs(theme, 2)),
        css = badge_style_css(),
    )
}

fn render_modern(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    let wave = texture::wave_ink(theme);
    let label = aria_label(placed);
    let total = width + 8.0 + BRAND_W;
    let defs = format!(
        "  <defs><filter id=\"mshadow\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"160%\"><feDropShadow dx=\"0\" dy=\"1\" stdDeviation=\"1.2\" flood-color=\"{}\" flood-opacity=\"0.18\" /></filter>{}</defs>\n",
        if theme.dark { "#000000" } else { "#737373" },
        texture::tier_pattern(theme.fg, 2.0, 1),
    );
    let mut body = String::new();
    body.push_str(&format!(
        "  <rect x=\"0.5\" y=\"0.5\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"14\" fill=\"{bg}\" stroke=\"{border}\" stroke-width=\"1\" filter=\"url(#mshadow)\" />\n",
        w = total - 1.0,
        h = HEIGHT - 1.0,
        bg = if theme.dark { "#0f0f0f" } else { "#ffffff" },
        border = theme.border,
    ));
    body.push_str(&wash_rect(
        2.0,
        2.0,
        total - 4.0,
        HEIGHT - 4.0,
        12.0,
        1,
        "0.08",
    ));
    // Leading accent dot.
    body.push_str(&format!(
        "  <circle cx=\"9\" cy=\"{cy:.1}\" r=\"2.5\" fill=\"{wave}\" />\n",
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
            wave,
            animate,
            SegAnim::FadeSlide,
            i,
        ));
    }
    body.push_str(&brand_chip(total, wave));
    format!(
        "{header}{css}{body}</svg>",
        header = svg_header(total, &label, &defs),
        css = badge_style_css(),
    )
}

fn render_glass(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    let wave = texture::wave_ink(theme);
    let label = aria_label(placed);
    let panel = if theme.dark { "#1c1c1c" } else { "#f0f0f0" };
    let total = width + BRAND_W;
    let mut body = String::new();
    body.push_str(&format!(
        "  <rect x=\"0.5\" y=\"0.5\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\" fill=\"{panel}\" stroke=\"{border}\" stroke-width=\"1\" />\n",
        w = total - 1.0,
        h = HEIGHT - 1.0,
        panel = panel,
        border = theme.border,
    ));
    body.push_str(&wash_rect(
        1.0,
        1.0,
        total - 2.0,
        HEIGHT - 2.0,
        9.0,
        3,
        "0.10",
    ));
    // Top glass highlight.
    body.push_str(&format!(
        "  <rect x=\"3\" y=\"2\" width=\"{w:.1}\" height=\"8\" rx=\"5\" fill=\"#ffffff\" opacity=\"{op}\" />\n",
        w = total - 6.0,
        op = if theme.dark { "0.05" } else { "0.5" },
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
            wave,
            animate,
            SegAnim::FadeIn,
            i,
        ));
    }
    // Highlight sweep: a thin bar that slides left→right and freezes
    // off-screen-right (so the frozen GitHub frame shows a clean panel).
    // `additive="sum"` keeps the static transform authoritative once frozen.
    if animate {
        body.push_str(&format!(
            "  <rect x=\"{end:.1}\" y=\"0\" width=\"12\" height=\"{h:.1}\" fill=\"#ffffff\" opacity=\"0.18\" transform=\"translate(0 0)\"><animateTransform class=\"motion\" attributeName=\"transform\" type=\"translate\" from=\"-{distance:.1} 0\" to=\"0 0\" dur=\"0.6s\" begin=\"0s\" fill=\"freeze\" additive=\"sum\" calcMode=\"linear\" /></rect>\n",
            h = HEIGHT,
            end = total,
            distance = total + 12.0,
        ));
    }
    body.push_str(&brand_chip(total, wave));
    format!(
        "{header}{css}{body}</svg>",
        header = svg_header(total, &label, &wash_defs(theme, 3)),
        css = badge_style_css(),
    )
}

fn render_terminal(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    // Prompt-style dark chip with a blinking cursor (the cursor "pulse" is
    // the animation; it freezes visible).
    let bg = "#0a0a0a";
    let fg = "#fafafa";
    // Terminal is intentionally theme-independent (always a dark chip), so
    // the accent is the dark wave ink regardless of the requested theme.
    let _ = theme;
    let accent = "#9b7bff";
    let label = aria_label(placed);
    let total = width + 14.0 + BRAND_W;
    let defs = format!("  <defs>{}</defs>\n", texture::tier_pattern(fg, 2.0, 1));
    let mut body = String::new();
    body.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"5\" fill=\"{bg}\" />\n",
        w = total,
        h = HEIGHT,
    ));
    body.push_str(&wash_rect(0.0, 0.0, total, HEIGHT, 5.0, 1, "0.10"));
    // Leading `›` prompt marker.
    body.push_str(&format!(
        "  <text x=\"7\" y=\"{ty:.1}\" fill=\"{accent}\" textLength=\"{advance:.1}\" lengthAdjust=\"spacingAndGlyphs\">&#8250;</text>\n",
        ty = HEIGHT / 2.0 + 4.0,
        advance = CHAR_W,
    ));
    for (i, p) in placed.iter().enumerate() {
        let pp = Placed {
            seg: p.seg.clone(),
            x: p.x + 14.0,
            w: p.w,
        };
        body.push_str(&segment_content(
            &pp,
            fg,
            accent,
            animate,
            SegAnim::FadeIn,
            i,
        ));
    }
    // Blinking cursor block at the end of the content run.
    let cursor_x = width + 14.0 - 8.0;
    if animate {
        body.push_str(&format!(
            "  <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"6\" height=\"12\" fill=\"{fg}\" opacity=\"1\"><animate class=\"motion\" attributeName=\"opacity\" values=\"1;0.2;1\" dur=\"0.8s\" repeatCount=\"2\" fill=\"freeze\" /></rect>\n",
            x = cursor_x,
            y = HEIGHT / 2.0 - 6.0,
        ));
    } else {
        body.push_str(&format!(
            "  <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"6\" height=\"12\" fill=\"{fg}\" />\n",
            x = cursor_x,
            y = HEIGHT / 2.0 - 6.0,
        ));
    }
    body.push_str(&brand_chip(total, fg));
    format!(
        "{header}{css}{body}</svg>",
        header = svg_header(total, &label, &defs),
        css = badge_style_css(),
    )
}

fn empty_badge(style: BadgeStyle, theme: &Theme) -> String {
    let (bg, fg, chip_ink) = match style {
        BadgeStyle::Terminal => ("#0a0a0a", "#fafafa", "#fafafa"),
        _ => (panel_tone(theme), theme.muted, texture::wave_ink(theme)),
    };
    let text = "no metrics";
    let advance = text.chars().count() as f32 * SIGNAL_CHAR_W;
    let content_w = SEG_PAD_X + advance + SEG_PAD_X;
    let total = content_w + BRAND_W;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total:.0}" height="{h:.0}" viewBox="0 0 {total:.0} {h:.0}" role="img" aria-label="no metrics">
  <style><![CDATA[ text {{ font: 600 11px {FONT_MONO}; }} ]]></style>
  <rect x="0.5" y="0.5" width="{rw:.1}" height="{rh:.1}" rx="6" fill="{bg}" stroke="{border}" stroke-width="1" />
  {text_el}
{chip}</svg>"##,
        h = HEIGHT,
        rw = total - 1.0,
        rh = HEIGHT - 1.0,
        border = theme.border,
        text_el = pinned_text(SEG_PAD_X, HEIGHT / 2.0 + 4.0, fg, "", text, advance),
        chip = brand_chip(total, chip_ink),
    )
}

// Signal badge

/// Deterministic signal-badge geometry, computed once and shared by the
/// renderer and the geometry tests. All x positions derive from the FINAL
/// (possibly truncated) text widths.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SignalLayout {
    pub width: f32,
    pub label: String,
    pub label_x: f32,
    pub label_w: f32,
    pub sep_x: f32,
    pub detail: String,
    pub detail_x: f32,
    pub detail_w: f32,
    pub mark_x: f32,
}

/// Fixed lead-in before the label: strip + check zone.
const SIGNAL_TEXT_X: f32 = 31.0;
/// Gap on each side of the separator dot.
const SIGNAL_SEP_GAP: f32 = 8.0;
/// Clearance between the detail text and the brand chip zone.
const SIGNAL_TAIL_GAP: f32 = 8.0;
const SIGNAL_MIN_W: f32 = 180.0;
const SIGNAL_MAX_W: f32 = 420.0;

/// Truncate to `max_chars` with a real ellipsis. Char-safe; counts happen
/// on the raw string (before XML escaping).
fn truncate_ellipsis(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    while out.ends_with(' ') {
        out.pop();
    }
    out.push('…');
    out
}

pub(crate) fn signal_layout(label: &str, detail: &str) -> SignalLayout {
    let char_w = SIGNAL_CHAR_W;
    let fixed = SIGNAL_TEXT_X + SIGNAL_SEP_GAP * 2.0 + SIGNAL_TAIL_GAP + BRAND_W;
    let mut label = label.to_string();
    let mut detail = detail.to_string();
    let natural = |label: &str, detail: &str| {
        fixed + (label.chars().count() + detail.chars().count()) as f32 * char_w
    };
    // Truncate to fit the max width: the detail yields first (down to a
    // floor), then the label.
    if natural(&label, &detail) > SIGNAL_MAX_W {
        let budget_chars = ((SIGNAL_MAX_W - fixed) / char_w).floor() as usize;
        let label_len = label.chars().count();
        let detail_floor = 8usize;
        if label_len + detail_floor <= budget_chars {
            detail = truncate_ellipsis(&detail, budget_chars - label_len);
        } else {
            detail = truncate_ellipsis(&detail, detail_floor.min(budget_chars.saturating_sub(1)));
            let detail_len = detail.chars().count();
            label = truncate_ellipsis(&label, budget_chars.saturating_sub(detail_len).max(1));
        }
    }
    let label_w = label.chars().count() as f32 * char_w;
    let detail_w = detail.chars().count() as f32 * char_w;
    let width = natural(&label, &detail).clamp(SIGNAL_MIN_W, SIGNAL_MAX_W);
    let sep_x = SIGNAL_TEXT_X + label_w + SIGNAL_SEP_GAP;
    let detail_x = sep_x + SIGNAL_SEP_GAP;
    SignalLayout {
        width,
        label,
        label_x: SIGNAL_TEXT_X,
        label_w,
        sep_x,
        detail,
        detail_x,
        detail_w,
        mark_x: width - BRAND_W + 5.0,
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
    let l = signal_layout(label, detail);
    let status = if earned { "earned" } else { "not earned" };
    let wave = texture::wave_ink(theme);
    let signal = if earned { wave } else { theme.muted };
    let panel = panel_tone(theme);
    let text_y = HEIGHT / 2.0 + 4.0;
    // `additive="sum"` keeps the static translate; the check scales in
    // around the group's local origin and rests at the authored position.
    let motion = if animate {
        r#"<animateTransform class="motion" attributeName="transform" type="scale" from="0.75" to="1" dur="0.22s" fill="freeze" additive="sum" calcMode="spline" keySplines="0.23 1 0.32 1" />"#
    } else {
        ""
    };
    let check = if earned {
        format!(
            "  <g transform=\"translate(9.5 7.5)\" stroke=\"{signal}\" stroke-width=\"1.8\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"6.5\" cy=\"6.5\" r=\"6\" /><path d=\"M3.8 6.6 5.8 8.6 9.6 4.6\" />{motion}</g>\n"
        )
    } else {
        format!(
            "  <circle cx=\"16\" cy=\"14\" r=\"6\" fill=\"none\" stroke=\"{signal}\" stroke-width=\"1.5\" />\n"
        )
    };
    let aria = format!(
        "{}: {}, {status}",
        escape_xml(&l.label),
        escape_xml(&l.detail)
    );

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0}" height="{h:.0}" viewBox="0 0 {width:.0} {h:.0}" role="img" aria-label="{aria}">
  <defs>{tier}</defs>
  <style><![CDATA[
    text {{ font: 600 11px {FONT_MONO}; }}
    .detail {{ font-weight: 500; }}
    {MOTION_CSS}
  ]]></style>
  <rect x="0.5" y="0.5" width="{panel_w:.1}" height="{panel_h:.1}" rx="7" fill="{panel}" stroke="{border}" />
{wash}  <rect x="0" y="5" width="3" height="{strip_h:.1}" rx="1.5" fill="{signal}" />
{check}  {label_text}
  <circle cx="{sep_x:.1}" cy="{sep_y:.1}" r="1.3" fill="{muted}" opacity="0.7" />
  {detail_text}
{chip}</svg>"##,
        width = l.width,
        h = HEIGHT,
        tier = texture::tier_pattern(theme.fg, 2.0, 2),
        panel_w = l.width - 1.0,
        panel_h = HEIGHT - 1.0,
        strip_h = HEIGHT - 10.0,
        border = theme.border,
        wash = wash_rect(1.0, 1.0, l.width - 2.0, HEIGHT - 2.0, 6.0, 2, "0.10"),
        label_text = pinned_text(l.label_x, text_y, theme.fg, "", &l.label, l.label_w),
        sep_x = l.sep_x,
        sep_y = HEIGHT / 2.0,
        muted = theme.muted,
        detail_text = pinned_text(
            l.detail_x,
            text_y,
            theme.muted,
            "detail",
            &l.detail,
            l.detail_w
        ),
        chip = brand::chip_mark(l.mark_x, (HEIGHT - MARK_SIZE) / 2.0, MARK_SIZE, signal),
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
        // Earned ink is the wave accent, not plain fg.
        assert!(first.contains("#5b2cff"));
        assert!(first.contains("<animateTransform"));
        assert!(first.contains("additive=\"sum\""));
        assert!(!first.contains("var("));

        let dark = render_signal_badge("community powered", "8 contributors", false, &DARK, false);
        assert!(dark.contains("not earned"));
        assert!(!dark.contains("<animate"));
        assert!(dark.contains(DARK.fg));
    }

    #[test]
    fn signal_badge_has_no_baked_middot_prefix() {
        let svg = render_signal_badge("star momentum", "+279 stars / 30d", true, &LIGHT, false);
        // The separator is an explicit positioned element, never text.
        assert!(!svg.contains(">· "));
        assert!(svg.contains("+279 stars / 30d"));
    }

    #[test]
    fn signal_layout_orders_label_separator_detail_mark() {
        let l = signal_layout("star momentum", "+279 stars / 30d");
        assert!(
            l.label_x + l.label_w <= l.sep_x,
            "label must end before the separator"
        );
        assert!(l.sep_x <= l.detail_x, "separator sits before the detail");
        assert!(
            l.detail_x + l.detail_w + SIGNAL_TAIL_GAP <= l.mark_x + 0.01,
            "detail must clear the brand chip: {l:?}"
        );
        assert!(l.mark_x + MARK_SIZE <= l.width);
        assert!((SIGNAL_MIN_W..=SIGNAL_MAX_W).contains(&l.width));
    }

    #[test]
    fn signal_layout_truncates_with_ellipsis_instead_of_underlapping() {
        let long_detail = "bus factor 3 / 1847 contributors across every tracked module";
        let l = signal_layout("community powered", long_detail);
        assert!(l.width <= SIGNAL_MAX_W);
        assert!(
            l.detail.ends_with('…'),
            "clamped detail gets a real ellipsis: {}",
            l.detail
        );
        assert!(
            l.detail_x + l.detail_w + SIGNAL_TAIL_GAP <= l.mark_x + 0.01,
            "truncated content must still clear the chip: {l:?}"
        );
        // Extreme: even the label yields when both are enormous.
        let l2 = signal_layout(&"x".repeat(80), &"y".repeat(80));
        assert!(l2.label.ends_with('…'));
        assert!(l2.width <= SIGNAL_MAX_W);
        assert!(l2.detail_x + l2.detail_w + SIGNAL_TAIL_GAP <= l2.mark_x + 0.01);
    }

    #[test]
    fn char_counting_happens_before_xml_escaping() {
        // "&" is ONE char of layout even though it escapes to five.
        let a = signal_layout("a & b", "one & two");
        let b = signal_layout("a x b", "one x two");
        assert_eq!(a.width, b.width);
        assert_eq!(a.detail_x, b.detail_x);
        let svg = render_signal_badge("a & b", "one & two", true, &LIGHT, false);
        assert!(svg.contains("&amp;"));
        assert!(!svg.contains(" & "));
    }

    #[test]
    fn every_text_is_pinned_with_text_length() {
        for style in [
            BadgeStyle::Flat,
            BadgeStyle::Modern,
            BadgeStyle::Glass,
            BadgeStyle::Terminal,
        ] {
            let svg = render_badge(&full_input(style, false), &LIGHT);
            let texts = svg.matches("<text").count();
            let pinned = svg.matches("textLength=").count();
            assert_eq!(texts, pinned, "{style:?}: every <text> must be pinned");
            assert!(svg.contains("lengthAdjust=\"spacingAndGlyphs\""));
            assert!(svg.contains(FONT_MONO));
        }
        let signal = render_signal_badge("star momentum", "+12 stars / 30d", true, &DARK, false);
        assert_eq!(
            signal.matches("<text").count(),
            signal.matches("textLength=").count()
        );
        let empty = render_badge(
            &BadgeInput {
                stars: None,
                forks: None,
                downloads: None,
                metrics: vec![Metric::Stars],
                style: BadgeStyle::Flat,
                animate: false,
            },
            &LIGHT,
        );
        assert_eq!(
            empty.matches("<text").count(),
            empty.matches("textLength=").count()
        );
    }

    #[test]
    fn all_badges_share_one_uniform_height() {
        let expect = format!("height=\"{:.0}\"", HEIGHT);
        for style in [
            BadgeStyle::Flat,
            BadgeStyle::Modern,
            BadgeStyle::Glass,
            BadgeStyle::Terminal,
        ] {
            let svg = render_badge(&full_input(style, false), &DARK);
            assert!(svg.contains(&expect), "{style:?} must be {HEIGHT}px tall");
        }
        let signal = render_signal_badge("star momentum", "+9 stars / 30d", false, &DARK, false);
        assert!(
            signal.contains(&expect),
            "signal badge must match the family height"
        );
        let empty = render_badge(
            &BadgeInput {
                stars: None,
                forks: None,
                downloads: None,
                metrics: vec![Metric::Stars],
                style: BadgeStyle::Glass,
                animate: false,
            },
            &DARK,
        );
        assert!(empty.contains(&expect));
    }

    #[test]
    fn metric_badges_never_render_the_robot_at_chip_scale() {
        for style in [
            BadgeStyle::Flat,
            BadgeStyle::Modern,
            BadgeStyle::Glass,
            BadgeStyle::Terminal,
        ] {
            let svg = render_badge(&full_input(style, false), &LIGHT);
            assert!(svg.contains("data-gitdebt-logo=\"true\""));
            // The 512px robot path data must not appear at badge scale.
            assert!(
                !svg.contains("scale(0.0"),
                "{style:?} scales the robot down"
            );
            assert!(
                !svg.contains("gitdebt-dither"),
                "{style:?} leaks the logo pattern"
            );
        }
    }

    #[test]
    fn metric_segments_end_before_the_brand_chip() {
        let (width, placed) = layout(
            &BadgeInput {
                stars: Some(123_456),
                forks: Some(9_999),
                downloads: Some(123_456_789),
                metrics: Metric::parse_list(None),
                style: BadgeStyle::Flat,
                animate: false,
            }
            .segments(),
        );
        let last = placed.last().unwrap();
        assert!(last.x + last.w <= width + 0.01);
        // Flat total = width + BRAND_W; the chip starts at total-BRAND_W+5.
        let total = width + BRAND_W;
        let chip_x = total - BRAND_W + 5.0;
        assert!(
            last.x + last.w <= chip_x,
            "content must clear the chip zone"
        );
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
            // Every animateTransform must be additive so static transforms
            // survive SMIL playback (the pennant bug class).
            for frag in on.split("<animateTransform").skip(1) {
                let tag_end = frag.find("/>").unwrap_or(frag.len());
                let tag = &frag[..tag_end];
                assert!(
                    tag.contains("additive=\"sum\""),
                    "{style:?}: animateTransform must be additive: {tag}"
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
        // Ink + the theme's wave accent, no CSS variables.
        assert!(light.contains("#0a0a0a"));
        assert!(light.contains("#5b2cff"));
        assert!(dark.contains("#fafafa"));
        assert!(dark.contains("#9b7bff"));
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
        let signal = render_signal_badge("star momentum", "+279 stars / 30d", true, &DARK, true);
        let png = crate::raster::rasterize(&signal, crate::raster::RasterFormat::Png, 2.0)
            .expect("signal raster");
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }
}
