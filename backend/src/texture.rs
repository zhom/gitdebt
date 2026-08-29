//! Drafting notation shared by generated media.
//!
//! This module used to paint ordered-dither texture, wave gradients and
//! density-tier patterns. It paints none of those now. Every rendered asset
//! is a sheet of one dimensioned engineering drawing, and a drawing has no
//! texture, no gradient, no glow and no shadow: it has lines that measure
//! something and lettering that says what they measured.
//!
//! The governing rule, and the one thing to check a new helper against:
//! **every line terminates on something real.** A dimension line spans two
//! measured points and carries a value. A leader points at a datum. A frame
//! encloses a sheet. A rule that measures nothing, separates nothing and
//! encloses nothing does not belong here, so there is deliberately no
//! `hairline()` or `divider()` in this vocabulary.
//!
//! The module keeps its old file name so the renderers reconcile one thing at
//! a time. Nothing in it is a texture.
//!
//! # Determinism
//!
//! Every emitter is a pure function of its arguments. Coordinates go through
//! [`coord`], which rounds to two decimals, collapses `-0.0` to `0.0` and
//! pins a non-finite input to zero, so identical inputs produce identical
//! bytes and a bad float can never letter `NaN` into an attribute or panic on
//! a request path.
//!
//! # Typography
//!
//! Assets letter with the viewer's own system stack. Embedding a webfont
//! would bloat every SVG a README pulls, and the identity of these assets
//! travels through the line grammar, not the typeface.

use crate::theme::Theme;

/// The generic stacks every asset letters with. There is no webfont.
pub const SANS: &str = "ui-sans-serif, system-ui, sans-serif";
pub const MONO: &str = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

/// Construction and extension lines.
pub const W_CONSTRUCTION: f32 = 0.5;
/// The object, hairlines, dimension lines.
pub const W_OBJECT: f32 = 1.0;
/// A cut, an emphasis, the selected series.
pub const W_EMPHASIS: f32 = 2.0;

/// Length of a terminator triangle, from tip to base.
pub const TERMINATOR_LEN: f32 = 5.0;
/// Width of a terminator triangle across its base.
pub const TERMINATOR_BASE: f32 = 6.4;

/// How far an extension tick starts clear of the datum it measures. A tick
/// that touches its datum reads as part of the object instead of as notation.
pub const TICK_CLEARANCE: f32 = 2.0;
/// Default reach of an extension tick past that clearance.
pub const TICK_LEN: f32 = 6.0;

/// The one chamfer in the system: the bottom-right corner of a panel or a
/// title block. Everything else is square, and nothing is rounded.
pub const CHAMFER: f32 = 10.0;

/// Tracking on a field label. Labels are uppercase; values are tabular.
pub const LABEL_TRACKING: &str = "0.09em";

/// Width of the ground-coloured stroke that cuts a rule for its lettering,
/// as a fraction of the lettering size. See [`cut_text`] for why it is this
/// wide and not narrower.
pub const CUT_STROKE: f32 = 0.95;

/// Which way a piece of notation faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

/// A measured value, and the two colours it needs: the ink it letters in and
/// the ground the dimension line is cut back to behind the lettering.
#[derive(Debug, Clone, Copy)]
pub struct Dimension<'a> {
    /// The measured value, already formatted. Escaped on the way out.
    pub value: &'a str,
    /// Ink for the rule, the terminators and the lettering. Drafting red
    /// belongs here: a measured value is exactly what signal is spent on.
    pub ink: &'a str,
    /// The surface behind the line, which is what the lettering is stroked in
    /// so the rule opens a gap for its own text.
    pub ground: &'a str,
    /// Lettering size in px.
    pub size: f32,
}

/// A row of a title block: an uppercase field label and its value.
#[derive(Debug, Clone, Copy)]
pub struct TitleField<'a> {
    pub label: &'a str,
    pub value: &'a str,
}

/// Round to two decimals, collapse `-0.0`, and pin a non-finite input to
/// zero. Every coordinate this module emits goes through here.
pub fn coord(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let rounded = (v * 100.0).round() / 100.0;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{rounded:.2}")
}

/// XML text escaping. One copy, so a repository named `a&b` cannot break one
/// renderer's SVG and not another's.
pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A filled triangle landing ON `(x, y)`, pointing the way `points` says.
///
/// The tip is the datum. Nothing is offset off it: a terminator that stops
/// short of what it points at is measuring a different number.
pub fn terminator(x: f32, y: f32, points: Side, ink: &str) -> String {
    let half = TERMINATOR_BASE / 2.0;
    let (ax, ay, bx, by) = match points {
        Side::Right => (x - TERMINATOR_LEN, y - half, x - TERMINATOR_LEN, y + half),
        Side::Left => (x + TERMINATOR_LEN, y - half, x + TERMINATOR_LEN, y + half),
        Side::Down => (x - half, y - TERMINATOR_LEN, x + half, y - TERMINATOR_LEN),
        Side::Up => (x - half, y + TERMINATOR_LEN, x + half, y + TERMINATOR_LEN),
    };
    format!(
        "<path d=\"M{} {}L{} {}L{} {}Z\" fill=\"{ink}\" />",
        coord(x),
        coord(y),
        coord(ax),
        coord(ay),
        coord(bx),
        coord(by),
    )
}

/// A short perpendicular line springing from the datum at `(x, y)`, running
/// `len` in the direction of `toward`.
///
/// It starts [`TICK_CLEARANCE`] clear of the datum and never touches it.
pub fn extension_tick(x: f32, y: f32, toward: Side, len: f32, ink: &str) -> String {
    let len = if len.is_finite() { len.max(0.0) } else { 0.0 };
    let (x1, y1, x2, y2) = match toward {
        Side::Left => (x - TICK_CLEARANCE, y, x - TICK_CLEARANCE - len, y),
        Side::Right => (x + TICK_CLEARANCE, y, x + TICK_CLEARANCE + len, y),
        Side::Up => (x, y - TICK_CLEARANCE, x, y - TICK_CLEARANCE - len),
        Side::Down => (x, y + TICK_CLEARANCE, x, y + TICK_CLEARANCE + len),
    };
    format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{ink}\" stroke-width=\"{W_CONSTRUCTION}\" />",
        coord(x1),
        coord(y1),
        coord(x2),
        coord(y2),
    )
}

/// Lettering that cuts the rule it sits on.
///
/// The gap in the line is not a painted rectangle behind the text: the glyphs
/// are stroked in the ground colour and `paint-order="stroke"` puts that
/// stroke under the fill, so the rule is cut to exactly the shape of its own
/// value. That is how a drawing opens a gap in a rule for its lettering, and
/// because the stroke sits under the fill the glyphs keep their exact weight.
///
/// The stroke has to be wide enough to close the space between two words. A
/// space carries no glyph to stroke, so a narrow halo leaves the rule showing
/// through the gap and `380 files` reads as `380-files`. A monospace space
/// advances about 0.6em, so [`CUT_STROKE`] reaches a little over half of that
/// from each neighbour and the gap closes.
pub fn cut_text(x: f32, y: f32, text: &str, d: &Dimension<'_>) -> String {
    let size = if d.size.is_finite() {
        d.size.max(1.0)
    } else {
        10.0
    };
    format!(
        "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" dominant-baseline=\"central\" \
fill=\"{ink}\" stroke=\"{ground}\" stroke-width=\"{stroke}\" stroke-linejoin=\"round\" \
paint-order=\"stroke\" font-family=\"{MONO}\" font-size=\"{fs}\" \
font-variant-numeric=\"tabular-nums\">{value}</text>",
        coord(x),
        coord(y),
        ink = d.ink,
        ground = d.ground,
        stroke = coord(size * CUT_STROKE),
        fs = coord(size),
        value = escape_xml(text),
    )
}

/// A horizontal dimension line from `x1` to `x2` at height `y`, with a
/// terminator at each end and the measured value lettered on it.
///
/// Terminators point outward, at the extension ticks the caller has already
/// sprung from the two datums.
pub fn dimension_h(x1: f32, x2: f32, y: f32, d: &Dimension<'_>) -> String {
    format!(
        "<g>{rule}{left}{right}{value}</g>",
        rule = format_args!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{W_OBJECT}\" />",
            coord(x1),
            coord(y),
            coord(x2),
            coord(y),
            d.ink,
        ),
        left = terminator(x1, y, Side::Left, d.ink),
        right = terminator(x2, y, Side::Right, d.ink),
        value = cut_text((x1 + x2) / 2.0, y, d.value, d),
    )
}

/// A vertical dimension line from `y1` to `y2` at `x`. The value is lettered
/// along the line, reading upward, the way a drawing letters one.
pub fn dimension_v(y1: f32, y2: f32, x: f32, d: &Dimension<'_>) -> String {
    let mid = (y1 + y2) / 2.0;
    format!(
        "<g>{rule}{up}{down}<g transform=\"rotate(-90 {cx} {cy})\">{value}</g></g>",
        rule = format_args!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{W_OBJECT}\" />",
            coord(x),
            coord(y1),
            coord(x),
            coord(y2),
            d.ink,
        ),
        up = terminator(x, y1, Side::Up, d.ink),
        down = terminator(x, y2, Side::Down, d.ink),
        cx = coord(x),
        cy = coord(mid),
        value = cut_text(x, mid, d.value, d),
    )
}

/// A leader: a line from a datum out to a label, with a terminator at the
/// datum end only.
///
/// The triangle is built from the leader's own direction, so a leader may run
/// at any angle. A zero-length leader points at nothing and gets no
/// terminator rather than a triangle full of `NaN`.
pub fn leader(
    datum: (f32, f32),
    label_at: (f32, f32),
    label: &str,
    size: f32,
    ink: &str,
) -> String {
    let (dx, dy) = (datum.0 - label_at.0, datum.1 - label_at.1);
    let len = (dx * dx + dy * dy).sqrt();
    let head = if len.is_finite() && len > f32::EPSILON {
        let half = TERMINATOR_BASE / 2.0;
        // Unit vector from the label toward the datum, and its normal.
        let (ux, uy) = (dx / len, dy / len);
        let (base_x, base_y) = (datum.0 - ux * TERMINATOR_LEN, datum.1 - uy * TERMINATOR_LEN);
        format!(
            "<path d=\"M{} {}L{} {}L{} {}Z\" fill=\"{ink}\" />",
            coord(datum.0),
            coord(datum.1),
            coord(base_x - uy * half),
            coord(base_y + ux * half),
            coord(base_x + uy * half),
            coord(base_y - ux * half),
        )
    } else {
        String::new()
    };

    // The label sits on the far side of its own end, never overlapping the
    // leader it belongs to.
    let trailing = label_at.0 >= datum.0;
    let (anchor, text_x) = if trailing {
        ("start", label_at.0 + 4.0)
    } else {
        ("end", label_at.0 - 4.0)
    };
    let size = if size.is_finite() {
        size.max(1.0)
    } else {
        10.0
    };
    format!(
        "<g><line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{ink}\" \
stroke-width=\"{W_OBJECT}\" />{head}<text x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" \
dominant-baseline=\"central\" fill=\"{ink}\" font-family=\"{SANS}\" font-size=\"{fs}\">{text}</text></g>",
        coord(datum.0),
        coord(datum.1),
        coord(label_at.0),
        coord(label_at.1),
        coord(text_x),
        coord(label_at.1),
        fs = coord(size),
        text = escape_xml(label),
    )
}

/// The `d` of a closed rectangle whose bottom-right corner is cut at
/// [`CHAMFER`]. The only non-square corner in the system.
pub fn chamfered_rect_path(x: f32, y: f32, w: f32, h: f32) -> String {
    let w = if w.is_finite() { w.max(0.0) } else { 0.0 };
    let h = if h.is_finite() { h.max(0.0) } else { 0.0 };
    // A chamfer larger than the box would fold the path inside out.
    let cut = CHAMFER.min(w).min(h);
    format!(
        "M{} {}H{}V{}L{} {}H{}Z",
        coord(x),
        coord(y),
        coord(x + w),
        coord(y + h - cut),
        coord(x + w - cut),
        coord(y + h),
        coord(x),
    )
}

/// A panel: a chamfered box with a frame line around it.
pub fn panel(x: f32, y: f32, w: f32, h: f32, fill: &str, stroke: &str) -> String {
    format!(
        "<path d=\"{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{W_OBJECT}\" />",
        chamfered_rect_path(x, y, w, h),
    )
}

/// Padding inside a title block, and the step between its rows.
const TITLE_PAD: f32 = 11.0;
const TITLE_ROW: f32 = 15.0;
/// Baseline of a row, measured from the top of its band. Chosen so the last
/// row clears the chamfer with room to spare; `title_block_clears_the_cut`
/// is what holds that true.
const TITLE_BASELINE: f32 = 11.0;
const TITLE_LABEL_SIZE: f32 = 8.0;
const TITLE_VALUE_SIZE: f32 = 10.0;

/// Height a title block needs for `rows` fields.
pub fn title_block_height(rows: usize) -> f32 {
    TITLE_PAD * 2.0 + rows as f32 * TITLE_ROW
}

/// A title block: a bordered box with its outer corner cut at [`CHAMFER`],
/// carrying field labels and values in two columns.
///
/// Labels are uppercase and tracked out in ink-3; values are tabular and set
/// in ink, right-aligned so a column of numbers lines up on its digits. The
/// caller places it, conventionally bottom-right on the sheet.
pub fn title_block(x: f32, y: f32, width: f32, fields: &[TitleField<'_>], theme: &Theme) -> String {
    let height = title_block_height(fields.len());
    let mut out = String::with_capacity(160 + fields.len() * 220);
    out.push_str("<g>");
    out.push_str(&panel(x, y, width, height, theme.track, theme.border));
    for (index, field) in fields.iter().enumerate() {
        let baseline = y + TITLE_PAD + index as f32 * TITLE_ROW + TITLE_BASELINE;
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" fill=\"{ink3}\" font-family=\"{SANS}\" \
font-size=\"{ls}\" letter-spacing=\"{LABEL_TRACKING}\">{label}</text>",
            coord(x + TITLE_PAD),
            coord(baseline),
            ink3 = theme.ink_3,
            ls = coord(TITLE_LABEL_SIZE),
            label = escape_xml(&field.label.to_uppercase()),
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"end\" fill=\"{ink}\" font-family=\"{MONO}\" \
font-size=\"{vs}\" font-variant-numeric=\"tabular-nums\">{value}</text>",
            coord(x + width - TITLE_PAD),
            coord(baseline),
            ink = theme.fg,
            vs = coord(TITLE_VALUE_SIZE),
            value = escape_xml(field.value),
        ));
    }
    out.push_str("</g>");
    out
}

/// One category bar.
///
/// This is what replaced the density-tier patterns. Categories are told apart
/// by their plotter pen and by the label at the bar's own end, never by a
/// texture, and the bar carries a 1px ink hairline standing at its leading
/// edge: the measured end, which is where a terminator would land if the bar
/// were dimensioned. `grows` says which edge that is.
pub fn series_bar(x: f32, y: f32, w: f32, h: f32, pen: &str, ink: &str, grows: Side) -> String {
    let w = if w.is_finite() { w.max(0.0) } else { 0.0 };
    let h = if h.is_finite() { h.max(0.0) } else { 0.0 };
    let (ex1, ey1, ex2, ey2) = match grows {
        Side::Right => (x + w, y, x + w, y + h),
        Side::Left => (x, y, x, y + h),
        Side::Up => (x, y, x + w, y),
        Side::Down => (x, y + h, x + w, y + h),
    };
    format!(
        "<g><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{pen}\" />\
<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{ink}\" stroke-width=\"{W_OBJECT}\" /></g>",
        coord(x),
        coord(y),
        coord(w),
        coord(h),
        coord(ex1),
        coord(ey1),
        coord(ex2),
        coord(ey2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn dim<'a>(value: &'a str, theme: &'a Theme) -> Dimension<'a> {
        Dimension {
            value,
            ink: theme.accent,
            ground: theme.bg,
            size: 10.0,
        }
    }

    /// Everything here is a pure function of its arguments. Nothing samples a
    /// clock, an RNG, or a map's iteration order.
    #[test]
    fn every_emitter_is_deterministic() {
        let d = dim("1,240", &theme::LIGHT);
        let fields = [
            TitleField {
                label: "source",
                value: "historical data",
            },
            TitleField {
                label: "state",
                value: "complete",
            },
        ];
        for _ in 0..3 {
            assert_eq!(
                terminator(10.0, 20.0, Side::Right, "#111417"),
                terminator(10.0, 20.0, Side::Right, "#111417")
            );
            assert_eq!(
                dimension_h(10.0, 90.0, 40.0, &d),
                dimension_h(10.0, 90.0, 40.0, &d)
            );
            assert_eq!(
                dimension_v(10.0, 90.0, 40.0, &d),
                dimension_v(10.0, 90.0, 40.0, &d)
            );
            assert_eq!(
                leader((10.0, 10.0), (60.0, 30.0), "peak", 11.0, "#111417"),
                leader((10.0, 10.0), (60.0, 30.0), "peak", 11.0, "#111417")
            );
            assert_eq!(
                title_block(0.0, 0.0, 200.0, &fields, &theme::LIGHT),
                title_block(0.0, 0.0, 200.0, &fields, &theme::LIGHT)
            );
        }
    }

    /// The old module's whole job is gone: no dither field, no wave gradient,
    /// no density tiers, no def ids for any of them.
    #[test]
    fn the_notation_carries_no_texture() {
        let d = dim("42", &theme::DARK);
        let fields = [TitleField {
            label: "sheet",
            value: "1 of 1",
        }];
        let emitted = [
            terminator(4.0, 4.0, Side::Up, "#e6e8ea"),
            extension_tick(4.0, 4.0, Side::Down, TICK_LEN, "#828588"),
            dimension_h(0.0, 50.0, 12.0, &d),
            dimension_v(0.0, 50.0, 12.0, &d),
            leader((0.0, 0.0), (40.0, 20.0), "note", 10.0, "#e6e8ea"),
            title_block(0.0, 0.0, 180.0, &fields, &theme::DARK),
            series_bar(0.0, 0.0, 60.0, 12.0, "#5ca5e1", "#e6e8ea", Side::Right),
        ];
        for svg in emitted {
            for banned in [
                "gd-dither-wave",
                "gd-pixel-fill",
                "gd-pixel-field",
                "gd-pixel-fade",
                "gd-heat",
                "gd-t",
                "<pattern",
                "linearGradient",
                "radialGradient",
                "filter=",
                "feGaussianBlur",
                "opacity=",
                "var(--",
                "<animate",
                "rx=",
                "ry=",
                "box-shadow",
            ] {
                assert!(!svg.contains(banned), "{banned} survived in {svg}");
            }
        }
    }

    /// Three weights and no others, and each piece of notation uses the right
    /// one: 0.5 for a construction line, 1 for the object and the dimension.
    #[test]
    fn there_are_exactly_three_line_weights() {
        assert_eq!((W_CONSTRUCTION, W_OBJECT, W_EMPHASIS), (0.5, 1.0, 2.0));
        let tick = extension_tick(0.0, 0.0, Side::Up, TICK_LEN, "#111417");
        assert!(tick.contains("stroke-width=\"0.5\""));
        let d = dim("8", &theme::LIGHT);
        assert!(dimension_h(0.0, 40.0, 0.0, &d).contains("stroke-width=\"1\""));
        assert!(
            series_bar(0.0, 0.0, 10.0, 4.0, "#1a609e", "#111417", Side::Right)
                .contains("stroke-width=\"1\"")
        );
    }

    /// A terminator lands ON its datum, and an extension tick never touches
    /// one. Those two rules are what make the notation readable as measured.
    #[test]
    fn terminators_land_on_the_datum_and_ticks_stand_clear() {
        // Tip first in the path data, at the datum exactly.
        assert!(
            terminator(120.0, 40.0, Side::Right, "#111417")
                .starts_with("<path d=\"M120.00 40.00L115.00")
        );
        assert!(
            terminator(120.0, 40.0, Side::Left, "#111417")
                .starts_with("<path d=\"M120.00 40.00L125.00")
        );
        assert!(
            terminator(120.0, 40.0, Side::Down, "#111417")
                .starts_with("<path d=\"M120.00 40.00L116.80 35.00")
        );
        assert!(
            terminator(120.0, 40.0, Side::Up, "#111417")
                .starts_with("<path d=\"M120.00 40.00L116.80 45.00")
        );

        // The tick starts TICK_CLEARANCE clear of the datum, never on it.
        let up = extension_tick(50.0, 80.0, Side::Up, 6.0, "#6c6f73");
        assert!(up.contains("y1=\"78.00\"") && up.contains("y2=\"72.00\""));
        let right = extension_tick(50.0, 80.0, Side::Right, 6.0, "#6c6f73");
        assert!(right.contains("x1=\"52.00\"") && right.contains("x2=\"58.00\""));
    }

    /// The rule is cut for its own lettering by stroking the glyphs in the
    /// ground colour under the fill. Not by a rectangle parked behind them.
    #[test]
    fn a_dimension_cuts_its_rule_for_the_value() {
        let d = dim("1,240 stars", &theme::LIGHT);
        let svg = dimension_h(20.0, 220.0, 60.0, &d);
        assert!(svg.contains("paint-order=\"stroke\""));
        assert!(svg.contains(&format!("stroke=\"{}\"", theme::LIGHT.bg)));
        assert!(svg.contains("dominant-baseline=\"central\""));
        assert!(svg.contains("text-anchor=\"middle\""));
        assert!(svg.contains("font-variant-numeric=\"tabular-nums\""));
        // Centred on the span, and both terminators present.
        assert!(svg.contains("x=\"120.00\""));
        assert_eq!(svg.matches("<path d=\"M").count(), 2);
        // Drafting red is spent on a measured value: that is the one place it
        // belongs.
        assert!(svg.contains(theme::LIGHT.accent));
    }

    #[test]
    fn a_leader_carries_one_terminator_at_the_datum_end() {
        let svg = leader((100.0, 50.0), (160.0, 20.0), "peak", 11.0, "#111417");
        assert_eq!(svg.matches("<path d=\"M").count(), 1);
        // The triangle's tip is the datum.
        assert!(svg.contains("<path d=\"M100.00 50.00L"));
        // Label trails to the right of its own end, clear of the line.
        assert!(svg.contains("text-anchor=\"start\"") && svg.contains("x=\"164.00\""));
        let back = leader((100.0, 50.0), (40.0, 20.0), "peak", 11.0, "#111417");
        assert!(back.contains("text-anchor=\"end\"") && back.contains("x=\"36.00\""));
        // A leader pointing at itself emits no NaN triangle.
        let degenerate = leader((10.0, 10.0), (10.0, 10.0), "x", 10.0, "#111417");
        assert!(!degenerate.contains("<path"));
        assert!(!degenerate.contains("NaN"));
    }

    #[test]
    fn the_only_chamfer_is_the_bottom_right_corner() {
        let d = chamfered_rect_path(0.0, 0.0, 200.0, 80.0);
        assert_eq!(d, "M0.00 0.00H200.00V70.00L190.00 80.00H0.00Z");
        // Square everywhere else, and never rounded.
        assert!(!d.contains('A') && !d.contains('Q') && !d.contains('C'));
        // A chamfer bigger than the box cannot fold the path inside out.
        let tiny = chamfered_rect_path(0.0, 0.0, 4.0, 3.0);
        assert_eq!(tiny, "M0.00 0.00H4.00V0.00L1.00 3.00H0.00Z");
        assert!(!panel(0.0, 0.0, 10.0, 10.0, "#fff", "#000").contains("rx="));
    }

    #[test]
    fn a_title_block_letters_two_columns() {
        let fields = [
            TitleField {
                label: "source",
                value: "historical data",
            },
            TitleField {
                label: "coverage date",
                value: "2026-08-29",
            },
            TitleField {
                label: "state",
                value: "complete",
            },
        ];
        let svg = title_block(20.0, 400.0, 240.0, &fields, &theme::LIGHT);
        // Labels uppercase and tracked; values tabular and right-aligned.
        assert!(svg.contains(">SOURCE<") && svg.contains(">COVERAGE DATE<"));
        assert!(svg.contains(&format!("letter-spacing=\"{LABEL_TRACKING}\"")));
        assert!(svg.contains("text-anchor=\"end\""));
        assert_eq!(
            svg.matches("font-variant-numeric=\"tabular-nums\"").count(),
            3
        );
        // Two columns: labels on the left inset, values on the right inset.
        assert_eq!(svg.matches("x=\"31.00\"").count(), 3);
        assert_eq!(svg.matches("x=\"249.00\"").count(), 3);
        assert!(svg.contains(theme::LIGHT.ink_3) && svg.contains(theme::LIGHT.fg));
        assert_eq!(title_block_height(3), 67.0);
        // XML from a value can never break the sheet.
        let nasty = [TitleField {
            label: "repo",
            value: "a&b<c>",
        }];
        let escaped = title_block(0.0, 0.0, 100.0, &nasty, &theme::DARK);
        assert!(escaped.contains("a&amp;b&lt;c&gt;") && !escaped.contains("<c>"));
    }

    /// Clear the cut. The chamfer removes the bottom-right corner, so the
    /// last row's value has to sit fully inside what survives.
    #[test]
    fn title_block_clears_the_cut() {
        for rows in 1..8usize {
            let height = title_block_height(rows);
            let last_baseline = TITLE_PAD + (rows - 1) as f32 * TITLE_ROW + TITLE_BASELINE;
            // Descenders reach roughly a quarter of the size below baseline.
            let ink_bottom = last_baseline + TITLE_VALUE_SIZE * 0.25;
            assert!(
                height - ink_bottom > CHAMFER,
                "{rows} rows: lettering ends {:.1} above the edge, chamfer takes {CHAMFER}",
                height - ink_bottom
            );
        }
    }

    /// The replacement for the density-tier patterns: a flat pen and a
    /// hairline at the measured end.
    #[test]
    fn a_series_bar_is_a_flat_pen_and_a_leading_hairline() {
        let bar = series_bar(
            40.0,
            10.0,
            120.0,
            14.0,
            theme::LIGHT.pens[2],
            theme::LIGHT.fg,
            Side::Right,
        );
        assert!(bar.contains(&format!("fill=\"{}\"", theme::LIGHT.pens[2])));
        // Hairline stands on the right edge, the end that carries the value.
        assert!(bar.contains("x1=\"160.00\"") && bar.contains("x2=\"160.00\""));
        assert!(bar.contains("y1=\"10.00\"") && bar.contains("y2=\"24.00\""));
        assert!(!bar.contains("fill-opacity") && !bar.contains("url(#"));

        // A column grows upward, so its hairline caps the top.
        let column = series_bar(
            40.0,
            10.0,
            14.0,
            120.0,
            theme::DARK.pens[3],
            theme::DARK.fg,
            Side::Up,
        );
        assert!(column.contains("y1=\"10.00\"") && column.contains("y2=\"10.00\""));
        assert!(column.contains("x1=\"40.00\"") && column.contains("x2=\"54.00\""));
    }

    /// A bad float must not letter `NaN` into an attribute or panic. A
    /// request path renders something valid or it renders nothing.
    #[test]
    fn non_finite_coordinates_degrade_to_zero() {
        assert_eq!(coord(f32::NAN), "0.00");
        assert_eq!(coord(f32::INFINITY), "0.00");
        assert_eq!(coord(f32::NEG_INFINITY), "0.00");
        assert_eq!(coord(-0.0), "0.00");
        assert_eq!(coord(-0.001), "0.00");
        assert_eq!(coord(12.345), "12.35");

        let d = Dimension {
            value: "?",
            ink: "#cc291f",
            ground: "#ffffff",
            size: f32::NAN,
        };
        for svg in [
            dimension_h(f32::NAN, 10.0, f32::INFINITY, &d),
            dimension_v(0.0, f32::NAN, 5.0, &d),
            leader((f32::NAN, 0.0), (10.0, 10.0), "x", f32::NAN, "#111417"),
            series_bar(0.0, 0.0, f32::NAN, -8.0, "#111417", "#111417", Side::Right),
            chamfered_rect_path(0.0, 0.0, f32::NAN, f32::INFINITY),
            extension_tick(0.0, 0.0, Side::Up, f32::NAN, "#111417"),
        ] {
            assert!(!svg.contains("NaN") && !svg.contains("inf"), "{svg}");
        }
    }

    #[test]
    fn text_is_escaped_everywhere_it_is_emitted() {
        assert_eq!(escape_xml("a&b<c>\"d\""), "a&amp;b&lt;c&gt;&quot;d&quot;");
        let d = dim("<1 & 2>", &theme::LIGHT);
        let svg = dimension_h(0.0, 10.0, 0.0, &d);
        assert!(svg.contains("&lt;1 &amp; 2&gt;"));
        let led = leader((0.0, 0.0), (9.0, 9.0), "a<b", 10.0, "#111417");
        assert!(led.contains("a&lt;b"));
    }

    /// No webfont travels with a README asset.
    #[test]
    fn assets_letter_with_the_system_stack() {
        let fields = [TitleField {
            label: "sheet",
            value: "1",
        }];
        let svg = title_block(0.0, 0.0, 120.0, &fields, &theme::LIGHT);
        assert!(svg.contains(SANS) && svg.contains(MONO));
        for banned in ["@font-face", "https://", "data:font", ".woff"] {
            assert!(!svg.contains(banned));
        }
    }
}
