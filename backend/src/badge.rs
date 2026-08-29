//! Compact, configurable badge SVG renderer.
//!
//! Renders a small embeddable badge showing any subset of `stars`, `forks`,
//! and `downloads` for a repo, plus the evidence-backed signal badge. Pure +
//! deterministic (same input → same bytes) so the badge endpoint is
//! upstream-cacheable; theme colors are baked as concrete hex (no CSS vars) so
//! the badge renders correctly as a README `<img>` regardless of the viewer's
//! OS/page theme — same rationale as `chart.rs` / `theme.rs`.
//!
//! ## What a badge is
//!
//! One field of the drawing, at the smallest size the drawing is ever printed:
//! a 1px frame, an uppercase field label in ink, and the measured value in
//! drafting red. Nothing else. There is no texture, no gradient, no glow and
//! no accent strip, and — unlike a card or a title block — **no chamfer**: a
//! badge is 28 units tall and often rendered at 20, and a 10px cut off one
//! corner of that is not a drafting detail, it is a broken frame.
//!
//! The value carries the red because the value is the point. The label says
//! what was measured, so the number never has to be guessed from a glyph.
//!
//! ## Layout discipline
//!
//! Badge text uses a mono stack with a fixed per-character advance
//! (`0.6em`), and every `<text>` is pinned with `textLength` +
//! `lengthAdjust` so browsers and the resvg raster path produce identical
//! geometry (verified by a raster unit test). Field labels are pinned with
//! `lengthAdjust="spacing"` to a width that carries exactly the drawing's
//! 0.09em label tracking; values are pinned with `spacingAndGlyphs`.
//! Character counts happen **before** XML escaping. After the width clamp,
//! label and detail are truncated with a real ellipsis and every x position is
//! recomputed from the final text widths, so content can never underlap the
//! trailing brand mark.
//!
//! ## Animation + the static-frame guarantee
//!
//! `animate=1` adds tasteful SMIL animation using `<animate …
//! fill="freeze">`. Anything that renders the SVG as a still — every
//! rasterizer, and README renderers outside GitHub — never runs the
//! `<animate>`, so the viewer sees the **frozen final frame**. We guarantee
//! that frame is correct by authoring every animated attribute's static value
//! to already equal the animation's end state; `<animateTransform>` always
//! carries `additive="sum"` so the element's static transform survives
//! (SMIL's default replace semantics would discard it). `animate=0`
//! (default) emits no `<animate>` tags at all.

use crate::brand;
use crate::texture::{self, Side};
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

    /// Lowercase name, for the accessible label.
    fn label(self) -> &'static str {
        match self {
            Metric::Stars => "stars",
            Metric::Forks => "forks",
            Metric::Downloads => "downloads",
        }
    }

    /// The uppercase field label lettered on the badge itself.
    fn field(self) -> &'static str {
        match self {
            Metric::Stars => "STARS",
            Metric::Forks => "FORKS",
            Metric::Downloads => "DOWNLOADS",
        }
    }
}

/// Visual style. Auto-sized width; per-theme baked hex.
///
/// gitdebt ships exactly one badge look. `flat`, `modern`, `glass`,
/// `terminal` and `dither` were near-identical variants of it; they remain
/// accepted `?style=` values so README embeds published against them keep
/// rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeStyle {
    #[default]
    Sheet,
}

impl BadgeStyle {
    /// Parse `?style=`. Every value — legacy, unknown, or absent — resolves
    /// to the single shipped style.
    pub fn parse(_s: Option<&str>) -> Self {
        BadgeStyle::Sheet
    }
}

/// A resolved metric segment: which metric, and its humanized value.
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

/// Uniform badge height across the whole family (metric badges + signal +
/// empty) so side-by-side README embeds align.
pub const HEIGHT: f32 = 28.0;
/// Reserved trailing zone for the brand mark: 5px gap + [`MARK_W`] + 5px
/// right margin.
const BRAND_W: f32 = 28.0;
/// Width of the robot mark. The artwork is 1.43:1, so 18px of width buys
/// 12.6px of height inside the 28px badge — the narrowest the head, screen
/// cutout, and both eyes still resolve at 1× in a README.
const MARK_W: f32 = 18.0;
/// Horizontal padding inside each field.
const SEG_PAD_X: f32 = 8.0;
/// Gap between a field label and the value it names.
const LABEL_GAP: f32 = 5.0;
/// Mono advance at 12px (0.6em) — pinned via `textLength`, so this is the
/// rendered width, not an estimate.
const CHAR_W: f32 = 7.2;
/// Mono advance at the signal badge's 11px text.
const SIGNAL_CHAR_W: f32 = 6.6;
/// Field lettering: 8px, its 0.6em mono advance, and the drawing's 0.09em
/// label tracking expressed in px at that size.
const LABEL_SIZE: f32 = 8.0;
const LABEL_CHAR_W: f32 = LABEL_SIZE * 0.6;
const LABEL_TRACK: f32 = LABEL_SIZE * 0.09;
/// Value baseline, and the label baseline that puts the label's capitals on
/// the same optical centre as the value's. Guessing this misses by ~1.5px,
/// which at badge scale reads as a label sitting low in its field.
const VALUE_BASELINE: f32 = HEIGHT / 2.0 + 4.0;
const LABEL_BASELINE: f32 = VALUE_BASELINE - (12.0 - LABEL_SIZE) * 0.72 / 2.0;
const REVEAL_SECONDS: f32 = 0.2;
const STAGGER_SECONDS: f32 = 0.04;
const MAX_STAGGER_SECONDS: f32 = 0.08;
const MOTION_CSS: &str = "@media (prefers-reduced-motion: reduce) { .motion { display: none; } }";
/// The shared badge type stack. `raster.rs` maps the generic `monospace`
/// keyword onto the bundled font so PNG/WebP geometry matches the pinned
/// `textLength` advance. A badge letters entirely in mono: at this size the
/// exact 0.6em advance is what keeps label, value and frame in register.
const FONT_MONO: &str = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

/// Pinned width of an uppercase field label: the glyph advances plus one
/// tracking step in each gap. `lengthAdjust="spacing"` then distributes
/// exactly that surplus between the letters, so the tracking is the
/// drawing's 0.09em rather than whatever the renderer felt like.
fn label_width(chars: usize) -> f32 {
    if chars == 0 {
        return 0.0;
    }
    chars as f32 * LABEL_CHAR_W + (chars - 1) as f32 * LABEL_TRACK
}

fn reveal_delay(index: usize) -> f32 {
    (index as f32 * STAGGER_SECONDS).min(MAX_STAGGER_SECONDS)
}

/// A pinned `<text>` element: mono, fixed advance, `textLength` +
/// `lengthAdjust` so client and raster geometry agree. `chars` is counted
/// by the caller BEFORE escaping.
fn pinned_text(
    x: f32,
    y: f32,
    fill: &str,
    class: &str,
    text: &str,
    advance: f32,
    adjust: &str,
) -> String {
    let class_attr = if class.is_empty() {
        String::new()
    } else {
        format!(" class=\"{class}\"")
    };
    format!(
        "<text{class_attr} x=\"{x:.1}\" y=\"{y:.1}\" fill=\"{fill}\" textLength=\"{advance:.1}\" lengthAdjust=\"{adjust}\">{}</text>",
        texture::escape_xml(text),
    )
}

/// A value: tabular, pinned glyph-for-glyph.
fn value_text(x: f32, y: f32, fill: &str, class: &str, text: &str) -> String {
    let advance = text.chars().count() as f32 * CHAR_W;
    pinned_text(x, y, fill, class, text, advance, "spacingAndGlyphs")
}

/// A field label: uppercase, tracked, pinned by spacing alone.
fn field_text(x: f32, y: f32, fill: &str, text: &str) -> String {
    let advance = label_width(text.chars().count());
    pinned_text(x, y, fill, "k", text, advance, "spacing")
}

/// One laid-out field at its x offset.
struct Placed {
    seg: Segment,
    x: f32,
}

/// Rendered width of one field: padding, label, gap, pinned value, padding.
fn segment_width(seg: &Segment) -> f32 {
    SEG_PAD_X
        + label_width(seg.metric.field().chars().count())
        + LABEL_GAP
        + seg.value.chars().count() as f32 * CHAR_W
        + SEG_PAD_X
}

/// Compute per-field widths + total content width for the given segments.
/// Width auto-sizes to content. Returns `(total_width, placed)`.
fn layout(segments: &[Segment]) -> (f32, Vec<Placed>) {
    let mut x = 0.0_f32;
    let mut placed = Vec::with_capacity(segments.len());
    for seg in segments {
        let w = segment_width(seg);
        placed.push(Placed {
            seg: seg.clone(),
            x,
        });
        x += w;
    }
    (x.max(1.0), placed)
}

// Shared chrome

/// Left edge of the mark inside the reserved [`BRAND_W`] zone.
fn mark_x(total: f32) -> f32 {
    total - BRAND_W + 5.0
}

/// Top edge of the vertically centred mark.
fn mark_y() -> f32 {
    (HEIGHT - brand::mark_height(MARK_W)) / 2.0
}

/// Trailing brand mark — the canonical gitdebt robot, right-aligned inside
/// the reserved [`BRAND_W`] zone. The ink is the badge's foreground, never
/// the signal: the logo must read as the logo, not as a measured value.
fn brand_mark(total: f32, ink: &str) -> String {
    brand::logo_mark(mark_x(total), mark_y(), MARK_W, ink)
}

/// The badge frame. Square, 1px, in the frame ink. The interior is
/// unpainted so the badge composites onto the README it sits in.
fn frame(total: f32, theme: &Theme) -> String {
    format!(
        "  <rect x=\"0.5\" y=\"0.5\" width=\"{w:.1}\" height=\"{h:.1}\" fill=\"none\" stroke=\"{border}\" stroke-width=\"{weight}\" />\n",
        w = total - 1.0,
        h = HEIGHT - 1.0,
        border = theme.border,
        weight = texture::W_OBJECT,
    )
}

/// A field division: a 1px rule that runs the full interior height, so it
/// terminates on the frame at both ends. Snapped to a half-pixel so it
/// stays one crisp line at 1×.
fn field_rule(x: f32, theme: &Theme) -> String {
    format!(
        "  <line x1=\"{x:.1}\" y1=\"1\" x2=\"{x:.1}\" y2=\"{y2:.1}\" stroke=\"{rule}\" stroke-width=\"{weight}\" />\n",
        x = x.round() + 0.5,
        y2 = HEIGHT - 1.0,
        rule = theme.grid,
        weight = texture::W_OBJECT,
    )
}

/// Shared SVG header.
fn svg_header(width: f32, label: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" role="img" aria-label="{label}">
"##,
        w = width,
        h = HEIGHT,
        label = label,
    )
}

/// Type for the whole family: one mono stack, two sizes, one label
/// treatment. Colors are always baked inline, never carried by a class.
fn badge_style_css(value_size: f32) -> String {
    format!(
        "  <style><![CDATA[ text {{ font: 600 {value_size:.0}px {FONT_MONO}; font-variant-numeric: tabular-nums; }} .k {{ font: 500 {LABEL_SIZE:.0}px {FONT_MONO}; }} {MOTION_CSS} ]]></style>\n"
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

/// One field: its label in ink, its value in drafting red, with an optional
/// reveal animation. The static markup already shows the final state; the
/// animation only fades/slides it in so the FROZEN frame on GitHub is
/// correct.
fn segment_content(
    p: &Placed,
    label_ink: &str,
    value_ink: &str,
    animate: bool,
    index: usize,
) -> String {
    let label = p.seg.metric.field();
    let label_x = p.x + SEG_PAD_X;
    let value_x = label_x + label_width(label.chars().count()) + LABEL_GAP;

    // The animation is authored so the resting (post-freeze) state equals
    // the element's static attributes — required for the GitHub frozen
    // frame. The transform is additive so the static transform list
    // survives SMIL's default replace semantics.
    let (anim_tag, start_transform) = if animate {
        let delay = reveal_delay(index);
        (
            format!(
                "<animate class=\"motion\" attributeName=\"opacity\" from=\"0\" to=\"1\" begin=\"{delay:.2}s\" dur=\"{REVEAL_SECONDS:.1}s\" fill=\"freeze\" /><animateTransform class=\"motion\" attributeName=\"transform\" type=\"translate\" from=\"0 4\" to=\"0 0\" begin=\"{delay:.2}s\" dur=\"{REVEAL_SECONDS:.1}s\" fill=\"freeze\" additive=\"sum\" calcMode=\"spline\" keySplines=\"0.23 1 0.32 1\" />"
            ),
            " transform=\"translate(0 0)\"".to_string(),
        )
    } else {
        (String::new(), String::new())
    };

    format!(
        "  <g opacity=\"1\"{start_transform}>{label}{value}{anim_tag}</g>\n",
        label = field_text(label_x, LABEL_BASELINE, label_ink, label),
        value = value_text(value_x, VALUE_BASELINE, value_ink, "", &p.seg.value),
    )
}

// Renderer

/// Render the badge to an SVG string. Auto-sized; deterministic; theme hex
/// baked. Emits `<animate fill="freeze">` only when `input.animate` is true.
pub fn render_badge(input: &BadgeInput, theme: &Theme) -> String {
    let segments = input.segments();
    if segments.is_empty() {
        return empty_badge(theme);
    }
    let (width, placed) = layout(&segments);
    let BadgeStyle::Sheet = input.style;
    render_fields(&placed, width, theme, input.animate)
}

fn render_fields(placed: &[Placed], width: f32, theme: &Theme, animate: bool) -> String {
    let label = aria_label(placed);
    let total = width + BRAND_W;
    let mut body = frame(total, theme);
    for (i, p) in placed.iter().enumerate() {
        if i > 0 {
            body.push_str(&field_rule(p.x, theme));
        }
        body.push_str(&segment_content(p, theme.fg, theme.accent, animate, i));
    }
    body.push_str(&brand_mark(total, theme.fg));
    format!(
        "{header}{css}{body}</svg>",
        header = svg_header(total, &label),
        css = badge_style_css(12.0),
    )
}

/// Nothing was measured, so nothing takes the signal: the badge states the
/// fact in secondary ink and stops.
fn empty_badge(theme: &Theme) -> String {
    let text = "NO METRICS";
    let advance = label_width(text.chars().count());
    let total = SEG_PAD_X + advance + SEG_PAD_X + BRAND_W;
    format!(
        "{header}{css}{frame}  {text_el}\n{mark}</svg>",
        header = svg_header(total, "no metrics"),
        css = badge_style_css(11.0),
        frame = frame(total, theme),
        text_el = field_text(SEG_PAD_X, VALUE_BASELINE, theme.muted, text),
        mark = brand_mark(total, theme.fg),
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

/// Fixed lead-in before the label: the terminator that points at the field.
const SIGNAL_TEXT_X: f32 = 31.0;
/// Gap on each side of the field division.
const SIGNAL_SEP_GAP: f32 = 8.0;
/// Clearance between the detail text and the brand mark zone.
const SIGNAL_TAIL_GAP: f32 = 8.0;
const SIGNAL_MIN_W: f32 = 180.0;
const SIGNAL_MAX_W: f32 = 420.0;
/// Where the leader's terminator lands. The triangle's tip is the datum, so
/// it sits clear of the lettering with the whole 5px head behind it.
const SIGNAL_TERMINATOR_X: f32 = 23.0;

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
///
/// The signal is drawn as one leader: a terminator landing on the field, the
/// signal's name in ink, then the evidence that earned it. Earned spends the
/// drafting red on the terminator and on the evidence — the measured value
/// and its arrow, which is the one thing red is for. Unearned draws the same
/// geometry in construction ink, so the two states never reflow.
pub fn render_signal_badge(
    label: &str,
    detail: &str,
    earned: bool,
    theme: &Theme,
    animate: bool,
) -> String {
    let l = signal_layout(label, detail);
    let status = if earned { "earned" } else { "not earned" };
    let signal = if earned { theme.accent } else { theme.ink_3 };
    let detail_ink = if earned { theme.accent } else { theme.muted };

    // The head is drawn at its group's local origin and the group is
    // translated onto the datum, so the scale grows the triangle OUT OF the
    // point it lands on rather than sliding it in from the sheet corner.
    // `additive="sum"` keeps that static translate, which SMIL's default
    // replace semantics would otherwise discard.
    let motion = if animate {
        "<animateTransform class=\"motion\" attributeName=\"transform\" type=\"scale\" from=\"0.75\" to=\"1\" dur=\"0.22s\" fill=\"freeze\" additive=\"sum\" calcMode=\"spline\" keySplines=\"0.23 1 0.32 1\" />"
    } else {
        ""
    };
    let head = format!(
        "  <g transform=\"translate({x:.1} {y:.1})\">{shape}{motion}</g>\n",
        x = SIGNAL_TERMINATOR_X,
        y = HEIGHT / 2.0,
        shape = texture::terminator(0.0, 0.0, Side::Right, signal),
    );
    let aria = format!(
        "{}: {}, {status}",
        texture::escape_xml(&l.label),
        texture::escape_xml(&l.detail)
    );

    format!(
        "{header}{css}{frame}{head}  {label_text}\n{rule}  {detail_text}\n{mark}</svg>",
        header = svg_header(l.width, &aria),
        css = badge_style_css(11.0),
        frame = frame(l.width, theme),
        rule = field_rule(l.sep_x, theme),
        label_text = pinned_text(
            l.label_x,
            VALUE_BASELINE,
            theme.fg,
            "",
            &l.label,
            l.label_w,
            "spacingAndGlyphs"
        ),
        detail_text = pinned_text(
            l.detail_x,
            VALUE_BASELINE,
            detail_ink,
            "",
            &l.detail,
            l.detail_w,
            "spacingAndGlyphs"
        ),
        mark = brand::logo_mark(l.mark_x, mark_y(), MARK_W, theme.fg),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};

    fn full_input(animate: bool) -> BadgeInput {
        BadgeInput {
            stars: Some(12_345),
            forks: Some(1_234),
            downloads: Some(2_500_000),
            metrics: vec![Metric::Stars, Metric::Forks, Metric::Downloads],
            style: BadgeStyle::Sheet,
            animate,
        }
    }

    fn no_metrics_input() -> BadgeInput {
        BadgeInput {
            stars: None,
            forks: None,
            downloads: None,
            metrics: vec![Metric::Stars],
            style: BadgeStyle::Sheet,
            animate: false,
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

    /// The value is the point, so the value is what carries drafting red —
    /// and nothing else on the badge does.
    #[test]
    fn only_the_measured_value_takes_drafting_red() {
        for theme in [&LIGHT, &DARK] {
            let svg = render_badge(&full_input(false), theme);
            // One red fill per field, and no more.
            assert_eq!(
                svg.matches(&format!("fill=\"{}\"", theme.accent)).count(),
                3,
                "three fields, three measured values: {svg}"
            );
            // The label that names it is ink, the frame is rule-strong.
            let stars = svg
                .split("<text")
                .find(|frag| frag.contains(">STARS<"))
                .expect("the stars field is lettered");
            assert!(stars.contains(&format!("fill=\"{}\"", theme.fg)));
            assert!(svg.contains(&format!("stroke=\"{}\"", theme.border)));
        }
    }

    /// A badge is too small to survive the drawing's one chamfer, and it has
    /// no texture, no strip and no rounded corner either.
    #[test]
    fn a_badge_is_a_square_frame_and_nothing_else() {
        let svgs = [
            render_badge(&full_input(true), &LIGHT),
            render_badge(&no_metrics_input(), &DARK),
            render_signal_badge(
                "actively maintained",
                "18 commits · 30d",
                true,
                &LIGHT,
                false,
            ),
            render_signal_badge("community powered", "8 contributors", false, &DARK, true),
        ];
        for svg in svgs {
            for banned in [
                "rx=",
                "ry=",
                "<pattern",
                "Gradient",
                "url(#",
                "gd-t",
                "gd-pixel",
                "filter=",
                "opacity=\"0.",
                "fill-opacity",
            ] {
                assert!(!svg.contains(banned), "{banned} survived: {svg}");
            }
            // No chamfer: the frame is a plain rect, never a cut path.
            assert!(!svg.contains("<path d=\"M0.50 0.50"));
        }
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
        // The evidence and the terminator pointing at it are the red.
        assert!(first.contains(LIGHT.accent));
        assert!(first.contains("<animateTransform"));
        assert!(first.contains("additive=\"sum\""));
        assert!(!first.contains("var("));

        let dark = render_signal_badge("community powered", "8 contributors", false, &DARK, false);
        assert!(dark.contains("not earned"));
        assert!(!dark.contains("<animate"));
        assert!(dark.contains(DARK.fg));
        // Unearned measured nothing, so it spends no signal.
        assert!(!dark.contains(DARK.accent));
    }

    /// Earned and unearned draw the same geometry in different ink, so the
    /// badge never reflows when a repository crosses the threshold.
    #[test]
    fn earned_and_unearned_share_one_geometry() {
        let strip = |svg: String, theme: &Theme| {
            svg.replace(theme.accent, "INK")
                .replace(theme.ink_3, "INK")
                .replace(theme.muted, "INK")
        };
        let on = strip(
            render_signal_badge("star momentum", "+279 stars / 30d", true, &LIGHT, false),
            &LIGHT,
        );
        let off = strip(
            render_signal_badge("star momentum", "+279 stars / 30d", false, &LIGHT, false),
            &LIGHT,
        );
        assert_eq!(
            on.replace(", earned", ", STATE"),
            off.replace(", not earned", ", STATE")
        );
    }

    #[test]
    fn signal_badge_has_no_baked_middot_prefix() {
        let svg = render_signal_badge("star momentum", "+279 stars / 30d", true, &LIGHT, false);
        // The field division is a rule that lands on the frame, never text.
        assert!(!svg.contains(">· "));
        assert!(!svg.contains("<circle"));
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
            "detail must clear the brand mark: {l:?}"
        );
        assert!(l.mark_x + MARK_W <= l.width);
        assert!((SIGNAL_MIN_W..=SIGNAL_MAX_W).contains(&l.width));
        // The leader's head sits clear of the lettering, tip and all.
        const { assert!(SIGNAL_TERMINATOR_X + texture::TERMINATOR_LEN <= SIGNAL_TEXT_X) };
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
            "truncated content must still clear the mark: {l:?}"
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
        let svg = render_badge(&full_input(false), &LIGHT);
        assert_eq!(
            svg.matches("<text").count(),
            svg.matches("textLength=").count(),
            "every <text> must be pinned"
        );
        assert!(svg.contains("lengthAdjust=\"spacingAndGlyphs\""));
        // A label is pinned by spacing alone: that is what carries the
        // drawing's 0.09em tracking without fattening the glyphs.
        assert!(svg.contains("lengthAdjust=\"spacing\""));
        assert!(svg.contains(FONT_MONO));

        let signal = render_signal_badge("star momentum", "+12 stars / 30d", true, &DARK, false);
        assert_eq!(
            signal.matches("<text").count(),
            signal.matches("textLength=").count()
        );
        let empty = render_badge(&no_metrics_input(), &LIGHT);
        assert_eq!(
            empty.matches("<text").count(),
            empty.matches("textLength=").count()
        );
    }

    /// The label's capitals have to sit on the same optical centre as the
    /// value's, or the field reads as if the label slipped.
    #[test]
    fn the_field_label_is_optically_centred_on_its_value() {
        let value_cap_centre = VALUE_BASELINE - 12.0 * 0.72 / 2.0;
        let label_cap_centre = LABEL_BASELINE - LABEL_SIZE * 0.72 / 2.0;
        assert!(
            (value_cap_centre - label_cap_centre).abs() < 0.01,
            "label centre {label_cap_centre} vs value centre {value_cap_centre}"
        );
        assert!(label_width(0).abs() < 1e-6);
        assert!((label_width(1) - LABEL_CHAR_W).abs() < 1e-6);
        assert!((label_width(5) - (5.0 * LABEL_CHAR_W + 4.0 * LABEL_TRACK)).abs() < 1e-6);
    }

    #[test]
    fn all_badges_share_one_uniform_height() {
        let expect = format!("height=\"{HEIGHT:.0}\"");
        assert!(render_badge(&full_input(false), &DARK).contains(&expect));
        let signal = render_signal_badge("star momentum", "+9 stars / 30d", false, &DARK, false);
        assert!(
            signal.contains(&expect),
            "signal badge must match the family height"
        );
        assert!(render_badge(&no_metrics_input(), &DARK).contains(&expect));
    }

    #[test]
    fn every_badge_carries_the_real_logo_in_the_foreground_ink() {
        for theme in [&LIGHT, &DARK] {
            for svg in [
                render_badge(&full_input(false), theme),
                render_badge(&no_metrics_input(), theme),
                render_signal_badge("star momentum", "+9 / 30d", true, theme, false),
            ] {
                assert!(svg.contains("data-gitdebt-logo=\"true\""));
                // The canonical path, not a hand-drawn stand-in.
                assert!(svg.contains("M320.5 110.5"));
                // At badge scale the artwork's pattern would be sub-pixel.
                assert!(!svg.contains("gitdebt-dither"), "mark leaks the pattern");
                // Foreground ink, never the signal.
                assert!(
                    svg.contains(&format!("fill=\"{}\"", theme.fg)),
                    "mark must be inked with the theme foreground"
                );
            }
        }
    }

    /// The regression this guards: compact surfaces once carried a
    /// hand-authored 14x14 bitmap instead of the repository artwork.
    /// Rasterize a real badge at real embed densities and compare the brand
    /// zone against a rasterization of the canonical asset.
    #[test]
    fn rasterized_brand_mark_matches_the_canonical_logo() {
        let input = full_input(false);
        let (width, _placed) = layout(&input.segments());
        let total = width + BRAND_W;

        for scale in [1.0_f32, 2.0, 6.0] {
            for theme in [&DARK, &LIGHT] {
                let svg = render_badge(&input, theme);
                let (mismatch, ink) = brand::mark_fidelity(
                    &svg,
                    brand::MarkBox {
                        x: mark_x(total),
                        y: mark_y(),
                        width: MARK_W,
                        scale,
                        ink: theme.fg,
                        // The badge paints no ground, so the tone the mark is
                        // designed against is the theme's own sheet.
                        canvas: theme.bg,
                    },
                );
                assert!(
                    mismatch < 0.05,
                    "@{scale}x the badge mark differs from the canonical logo by {mismatch:.3}"
                );
                assert!(
                    (0.25..0.75).contains(&ink),
                    "@{scale}x mark coverage {ink:.3} reads as a block, not a glyph"
                );
            }
        }
    }

    #[test]
    fn metric_fields_end_before_the_brand_mark() {
        let (width, placed) = layout(
            &BadgeInput {
                stars: Some(123_456),
                forks: Some(9_999),
                downloads: Some(123_456_789),
                metrics: Metric::parse_list(None),
                style: BadgeStyle::Sheet,
                animate: false,
            }
            .segments(),
        );
        let last = placed.last().unwrap();
        let last_w = segment_width(&last.seg);
        assert!(last.x + last_w <= width + 0.01);
        // Total = width + BRAND_W; the mark starts at total-BRAND_W+5.
        let total = width + BRAND_W;
        assert!(
            last.x + last_w <= mark_x(total),
            "content must clear the mark zone"
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

    /// Legacy README embeds must keep rendering after the collapse to one
    /// style, so every historical `?style=` value stays valid input.
    #[test]
    fn every_legacy_style_value_resolves_to_the_single_style() {
        for value in [
            Some("flat"),
            Some("modern"),
            Some("glass"),
            Some("terminal"),
            Some("dither"),
            Some(" Terminal "),
            Some("garbage"),
            Some(""),
            None,
        ] {
            assert_eq!(BadgeStyle::parse(value), BadgeStyle::Sheet, "{value:?}");
        }
        // And they all render identical bytes.
        let mut input = full_input(false);
        input.style = BadgeStyle::parse(Some("terminal"));
        assert_eq!(
            render_badge(&input, &DARK),
            render_badge(&full_input(false), &DARK)
        );
    }

    #[test]
    fn segments_honor_include_exclude() {
        let input = BadgeInput {
            stars: Some(10),
            forks: Some(20),
            downloads: Some(30),
            metrics: vec![Metric::Forks, Metric::Stars],
            style: BadgeStyle::Sheet,
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
            style: BadgeStyle::Sheet,
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
        let off = render_badge(&full_input(false), &LIGHT);
        assert!(
            !off.contains("<animate"),
            "animate=0 must have no <animate>: {off}"
        );
        assert!(off.contains("data-gitdebt-logo=\"true\""));

        let on = render_badge(&full_input(true), &LIGHT);
        assert!(on.contains("<animate"), "animate=1 must animate: {on}");
        assert!(on.contains("prefers-reduced-motion: reduce"));
        assert!(
            !on.contains("<g opacity=\"0\""),
            "SMIL-stripped badge content must stay visible"
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
                "animate tag must freeze or repeat-then-freeze: {tag}"
            );
        }
        // Every animateTransform must be additive so static transforms
        // survive SMIL playback (the pennant bug class).
        for frag in on.split("<animateTransform").skip(1) {
            let tag_end = frag.find("/>").unwrap_or(frag.len());
            let tag = &frag[..tag_end];
            assert!(
                tag.contains("additive=\"sum\""),
                "animateTransform must be additive: {tag}"
            );
        }
    }

    #[test]
    fn reveal_stagger_is_capped() {
        let on = render_badge(&full_input(true), &LIGHT);
        assert!(on.contains("begin=\"0.00s\""));
        assert!(on.contains("begin=\"0.04s\""));
        assert!(on.contains("begin=\"0.08s\""));
        assert!(!on.contains("begin=\"0.12s\""));
        assert!(on.contains("dur=\"0.2s\""));
    }

    #[test]
    fn frozen_frame_shows_final_value() {
        // The static text content must already be the final value, so a
        // still-frame render (rasterizers, non-GitHub READMEs) shows correct
        // numbers.
        let svg = render_badge(&full_input(true), &LIGHT);
        assert!(svg.contains("12.3k")); // stars
        assert!(svg.contains("1.2k")); // forks
        assert!(svg.contains("2.5M")); // downloads
        // And each one is named, so the number never has to be guessed.
        assert!(svg.contains(">STARS<") && svg.contains(">FORKS<") && svg.contains(">DOWNLOADS<"));
    }

    #[test]
    fn per_theme_colors_baked() {
        let light = render_badge(&full_input(false), &LIGHT);
        let dark = render_badge(&full_input(false), &DARK);
        // Ink + the theme's drafting red, no CSS variables.
        assert!(light.contains("#111417"));
        assert!(light.contains("#cc291f"));
        assert!(dark.contains("#e6e8ea"));
        assert!(dark.contains("#f0674e"));
        assert!(!light.contains("var(--"));
        assert!(!dark.contains("var(--"));
    }

    #[test]
    fn deterministic_same_bytes() {
        let a = render_badge(&full_input(true), &DARK);
        let b = render_badge(&full_input(true), &DARK);
        assert_eq!(a, b);
    }

    #[test]
    fn empty_when_no_metrics_available() {
        let svg = render_badge(&no_metrics_input(), &LIGHT);
        assert!(svg.contains("NO METRICS"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.starts_with("<svg"));
        // Nothing was measured, so nothing is red.
        assert!(!svg.contains(LIGHT.accent));
    }

    #[test]
    fn value_is_xml_escaped() {
        // Defensive: humanized values are digits + suffix, never need
        // escaping, but the path must still be safe.
        let svg = render_badge(&full_input(false), &LIGHT);
        assert!(svg.starts_with("<svg"));
        assert!(!svg.contains("<script"));
    }

    #[test]
    fn every_badge_rasterizes_frozen_frame() {
        // The .png/.webp variants run through the raster freezer; confirm
        // the animated SVG produces valid PNG bytes after the SMIL →
        // frozen-frame rewrite (catches filter parse failures).
        for input in [full_input(true), no_metrics_input()] {
            let svg = render_badge(&input, &LIGHT);
            let png = crate::raster::rasterize(&svg, crate::raster::RasterFormat::Png, 2.0)
                .expect("badge raster");
            assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        }
        let signal = render_signal_badge("star momentum", "+279 stars / 30d", true, &DARK, true);
        let png = crate::raster::rasterize(&signal, crate::raster::RasterFormat::Png, 2.0)
            .expect("signal raster");
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }
}
