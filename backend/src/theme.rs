//! Concrete-color themes baked at render time.
//!
//! Why not CSS `prefers-color-scheme` + variables? Because SVGs served
//! through `<img>` (the way READMEs render them via GitHub's camo proxy)
//! evaluate `prefers-color-scheme` against the user's **OS preference**,
//! not the embedding page's theme. A user with light OS reading GitHub
//! in dark mode would still see a light-themed chart inside a dark page.
//!
//! Star-history's solution — and ours — is to bake the theme directly:
//! emit fixed hex colors per-element. Embedders pick a theme via
//! `?theme=light|dark`, and combine the two SVGs at the embed site:
//!
//! ```html
//! <picture>
//!   <source media="(prefers-color-scheme: dark)"
//!           srcset="https://gitdebt.com/.../chart.svg?theme=dark" />
//!   <img src="https://gitdebt.com/.../chart.svg?theme=light" />
//! </picture>
//! ```
//!
//! The default is `light`. Every rendered asset is one sheet of the same
//! dimensioned engineering drawing the site is, and that drawing is graphite
//! on paper, so a bare URL returns the light print. Embedders that want the
//! second print of the same drawing opt in with `?theme=dark`.
//!
//! # Field names to drafting roles
//!
//! The struct field names predate the drawing and are kept so the renderers
//! do not all churn at once. What each one now holds:
//!
//! | field    | LIGHT role  | hex       | DARK role | hex       |
//! |----------|-------------|-----------|-----------|-----------|
//! | `bg`     | paper       | `#ffffff` | bg        | `#0c0f11` |
//! | `track`  | table       | `#f6f7f9` | panel     | `#15171a` |
//! | `fg`     | ink         | `#111417` | ink       | `#e6e8ea` |
//! | `muted`  | ink-2       | `#4f5357` | ink-2     | `#a8abae` |
//! | `ink_3`  | ink-3       | `#6c6f73` | ink-3     | `#828588` |
//! | `grid`   | rule        | `#dcdee0` | rule      | `#2b2e31` |
//! | `border` | rule-strong | `#c2c4c7` | rule-str  | `#3f4347` |
//! | `accent` | signal      | `#cc291f` | signal    | `#f0674e` |
//!
//! `bg` is the paper the ink is designed against, not a canvas that gets
//! painted: shareable SVG/PNG/WebP surfaces paint no background so they
//! composite onto the embedder's own page. It is what GIF frames are
//! flattened onto (GIF carries one bit of alpha) and what raster fidelity
//! checks composite over before judging luminance. `track` is the second
//! ground, the one step of depth in the system; depth never comes from a
//! shadow.
//!
//! `accent` is drafting red and is spent ONLY on a measured value and its
//! terminators, on the live data trace, and on at most one primary action.
//! It is never a tag, a status dot, a category color, or a heading.
//!
//! # These are the site's values, in hex
//!
//! `frontend/src/styles/globals.css` holds this exact palette in oklch; the
//! hexes here are the sRGB of those same coordinates, so a chart never draws
//! a different graphite from the page it sits on. Change one and change both.
//!
//! # Measured contrast
//!
//! Every color here has a measured WCAG contrast ratio against the ground it
//! is used on. Nothing is assumed.
//!
//! LIGHT, on paper `#ffffff`:
//!
//! - ink `#111417` — 18.5
//! - ink-2 `#4f5357` — 7.8
//! - ink-3 `#6c6f73` — 5.05
//! - signal `#cc291f` — 5.38, and white lettered on signal is also 5.38
//!
//! DARK, on bg `#0c0f11`:
//!
//! - ink `#e6e8ea` — 15.7
//! - ink-3 `#828588` — 5.18
//! - signal `#f0674e` — 6.19
//!
//! ink-3 is the floor of the system and it clears AA on both grounds by
//! design: it sat at oklch lightness 0.575 during design and measured 4.37,
//! which is exactly where a caption color never gets checked.

use serde::Deserialize;

/// How many plotter pens there are. There are eight and no more; a chart that
/// needs a ninth series needs fewer series.
pub const PEN_COUNT: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// True for dark, false for light. Lets per-chart code pick the
    /// correct categorical palette (and any non-palette-tracked colors)
    /// without a pointer-equality dance.
    pub dark: bool,
    /// The sheet: `paper` in light, `bg` in dark. See the module docs for why
    /// this is a design ground and not a rect that gets painted.
    pub bg: &'static str,
    /// The second ground the sheet lies on: `table` in light, `panel` in
    /// dark. One step of tone, and it is the only depth in the system.
    pub track: &'static str,
    /// Graphite: the object line, and all primary text.
    pub fg: &'static str,
    /// ink-2: secondary text.
    pub muted: &'static str,
    /// ink-3: captions, field labels, construction lines.
    pub ink_3: &'static str,
    /// Hairlines.
    pub grid: &'static str,
    /// Frame and extension lines.
    pub border: &'static str,
    /// Drafting red. Attached to something measured, or it is wrong.
    pub accent: &'static str,
    /// The plotter pen set, for multi-series charts and nothing else. The
    /// order is fixed; see [`pen_for`] for how a series claims one. Pen 1 is
    /// the graphite pen and pen 2 is the reserved signal ([`SIGNAL_PEN`]); the
    /// seven chromatic pens sit in one narrow lightness band so that none of
    /// them shouts, which is also why every series must carry a label at its
    /// own line end: hue is never the sole carrier of meaning.
    pub pens: [&'static str; PEN_COUNT],
}

// `static` (not `const`) so callers see one canonical address — `const`
// items get inlined per use-site, breaking pointer-equality checks like
// `std::ptr::eq(theme_for(...), &LIGHT)`.
pub static LIGHT: Theme = Theme {
    dark: false,
    bg: "#ffffff",
    track: "#f6f7f9",
    fg: "#111417",
    muted: "#4f5357",
    ink_3: "#6c6f73",
    grid: "#dcdee0",
    border: "#c2c4c7",
    accent: "#cc291f",
    pens: [
        "#282c2f", "#cc291f", "#1a609e", "#607c42", "#a25e2b", "#6a588a", "#1e7777", "#7b6f66",
    ],
};

pub static DARK: Theme = Theme {
    dark: true,
    bg: "#0c0f11",
    track: "#15171a",
    fg: "#e6e8ea",
    muted: "#a8abae",
    ink_3: "#828588",
    grid: "#2b2e31",
    border: "#3f4347",
    accent: "#f0674e",
    pens: [
        "#d5d8da", "#f0674e", "#5ca5e1", "#90ba6c", "#e29d5e", "#a88fd6", "#5db5b5", "#afa297",
    ],
};

pub fn theme_for(name: Option<&str>) -> &'static Theme {
    match name {
        Some(s) if s.eq_ignore_ascii_case("dark") => &DARK,
        // Anything else (including unset, "light", garbage) → light. The
        // drawing is graphite on paper; dark is the second print of it.
        _ => &LIGHT,
    }
}

/// The pen drafting red sits in, and the one pen a category may never have.
///
/// Signal is spent on the live or primary data trace, so a renderer reaches
/// for this pen deliberately, by name, for the one series that is the point
/// of the chart. Neither [`pen_for`] nor [`pens_for`] will ever hand it out,
/// because "whichever category the hash happened to land on" is exactly the
/// decorative use the palette forbids.
pub const SIGNAL_PEN: usize = 1;

/// The slots a category may claim, in order. [`SIGNAL_PEN`] is not among
/// them, which is why a chart has seven category pens and not eight.
const CATEGORY_SLOTS: [usize; PEN_COUNT - 1] = [0, 2, 3, 4, 5, 6, 7];

/// The pen at `index`, wrapped into range. Use this when a chart knows its
/// own series up front and pins them explicitly, and it is the only way to
/// reach [`SIGNAL_PEN`].
pub fn pen(theme: &Theme, index: usize) -> &'static str {
    theme.pens[index % PEN_COUNT]
}

/// The category pen assigned to a series `key`.
///
/// Assignment is by key, never by the series' position in whatever list this
/// particular chart happened to build, so `stars` is the same pen on every
/// chart and every re-render and adding a series does not recolor its
/// neighbours. No RNG, no wall clock, no map iteration: identical inputs
/// produce identical bytes.
///
/// Seven category pens means two keys *can* land on the same one. This is the
/// right call for a single series that wants a stable color of its own; a
/// chart drawing several at once should use [`pens_for`], which guarantees
/// distinct pens for up to seven series.
pub fn pen_for(theme: &Theme, key: &str) -> &'static str {
    theme.pens[CATEGORY_SLOTS[pen_slot(key)]]
}

/// Distinct category pens for a set of series keys, in the caller's own order.
///
/// The assignment depends on the key *set* and nothing else: the keys are
/// ranked before slots are handed out, so building the list in a different
/// order cannot recolor the chart. Each key claims its [`pen_for`] slot when
/// that slot is free and otherwise takes the next free one, which is why up
/// to seven series always come back distinct. Past seven the pens have to
/// repeat, and keys fall back to their preferred slot.
pub fn pens_for(theme: &Theme, keys: &[&str]) -> Vec<&'static str> {
    // `sort_by` is stable, so two identical keys keep their input order and
    // the whole function stays deterministic.
    let mut ranked: Vec<usize> = (0..keys.len()).collect();
    ranked.sort_by(|a, b| keys[*a].cmp(keys[*b]));

    let mut taken = [false; CATEGORY_SLOTS.len()];
    let mut out = vec![theme.pens[CATEGORY_SLOTS[0]]; keys.len()];
    for index in ranked {
        let preferred = pen_slot(keys[index]);
        let mut slot = preferred;
        for step in 0..CATEGORY_SLOTS.len() {
            let probe = (preferred + step) % CATEGORY_SLOTS.len();
            if !taken[probe] {
                slot = probe;
                taken[probe] = true;
                break;
            }
        }
        out[index] = theme.pens[CATEGORY_SLOTS[slot]];
    }
    out
}

/// FNV-1a over the key's bytes, finished with a xorshift-multiply avalanche,
/// reduced to an index into [`CATEGORY_SLOTS`].
///
/// The finalizer is not decoration. Raw FNV-1a puts `stars` and `forks` in
/// the same slot, and those two share a chart more often than any other pair
/// in this product.
fn pen_slot(key: &str) -> usize {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in key.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x7feb_352d);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x846c_a68b);
    hash ^= hash >> 16;
    (hash % CATEGORY_SLOTS.len() as u32) as usize
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ThemeQuery {
    pub theme: Option<String>,
}

/// Graphite, for lettering that sits on a filled shape. This is `LIGHT.fg`,
/// used in both prints: it is the darkest ink in the system, so it is the
/// right one whenever a fill is light enough to want dark lettering at all.
const LETTERING_INK: &str = "#111417";

/// Paper, for lettering that sits on a filled shape. This is `LIGHT.bg`.
const LETTERING_PAPER: &str = "#ffffff";

/// YIQ luminance at or above which graphite letters better than paper.
///
/// The crossover between `contrast(ink, C)` and `contrast(paper, C)` for a
/// neutral `C` is a linear luminance of 0.192, which is sRGB 121.3, so the
/// threshold sits one step above at 122. That sorts every color in both
/// prints and both pen sets correctly: the highest YIQ that still wants paper
/// is light pen 8 `#7b6f66` at 113, and the lowest that wants graphite is
/// dark ink-3 `#828588` at 132. The old threshold of 145 sat above both and
/// lettered dark signal `#f0674e` in white at 3.1, under AA.
const INK_OVER_PAPER_YIQ: u32 = 122;

/// Pick graphite or paper, whichever letters legibly on `hex`.
///
/// Used for a count printed inside a filled bar, where the fill is a pen or a
/// language color rather than one of the two grounds. An unparseable color
/// falls back to graphite: the drawing is light-first, so an unknown fill is
/// far likelier to be a light one.
pub fn contrast_on(hex: &str) -> &'static str {
    match parse_hex_rgb(hex) {
        Some((r, g, b)) => {
            let yiq = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
            if yiq >= INK_OVER_PAPER_YIQ {
                LETTERING_INK
            } else {
                LETTERING_PAPER
            }
        }
        None => LETTERING_INK,
    }
}

/// Parse `#rrggbb`.
///
/// Byte-wise on purpose. `s.len()` is a *byte* length, so a seven-byte string
/// that is not seven ASCII characters — `#` followed by an emoji and two
/// letters, say — used to slice through the middle of a code point and panic.
/// `contrast_on` is reachable from a request path, so it parses bytes and
/// rejects anything that is not seven ASCII ones.
fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let bytes = s.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' || !s.is_ascii() {
        return None;
    }
    let nibble = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let channel = |hi: u8, lo: u8| Some(nibble(hi)? * 16 + nibble(lo)?);
    Some((
        channel(bytes[1], bytes[2])?,
        channel(bytes[3], bytes[4])?,
        channel(bytes[5], bytes[6])?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linear-light relative luminance, the real WCAG one. The tests judge
    /// contrast with this; `contrast_on` uses the cheap YIQ approximation and
    /// these tests are what hold the two in agreement.
    fn relative_luminance(hex: &str) -> f64 {
        let (r, g, b) = parse_hex_rgb(hex).expect("hex");
        let channel = |v: u8| {
            let c = v as f64 / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    fn contrast_ratio(a: &str, b: &str) -> f64 {
        let (la, lb) = (relative_luminance(a), relative_luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.05,
            "expected {expected}, measured {actual:.3}"
        );
    }

    /// The exact figures the module docs and `globals.css` both claim. If a
    /// hex moves, this fails before the docs go quietly stale.
    #[test]
    fn documented_contrast_ratios_are_the_measured_ones() {
        close(contrast_ratio(LIGHT.fg, LIGHT.bg), 18.48);
        close(contrast_ratio(LIGHT.muted, LIGHT.bg), 7.76);
        close(contrast_ratio(LIGHT.ink_3, LIGHT.bg), 5.05);
        close(contrast_ratio(LIGHT.accent, LIGHT.bg), 5.38);
        close(contrast_ratio("#ffffff", LIGHT.accent), 5.38);

        close(contrast_ratio(DARK.fg, DARK.bg), 15.65);
        close(contrast_ratio(DARK.ink_3, DARK.bg), 5.18);
        close(contrast_ratio(DARK.accent, DARK.bg), 6.19);
    }

    /// ink-3 is the floor. It has to clear AA on the second ground too, not
    /// just on the sheet, because that is where captions actually land.
    #[test]
    fn every_ink_clears_aa_on_both_grounds() {
        for theme in [&LIGHT, &DARK] {
            for ink in [theme.fg, theme.muted, theme.ink_3] {
                for ground in [theme.bg, theme.track] {
                    let ratio = contrast_ratio(ink, ground);
                    assert!(ratio >= 4.5, "{ink} on {ground} is only {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn palette_is_the_drafting_palette() {
        assert_eq!(
            (LIGHT.bg, LIGHT.track, LIGHT.fg, LIGHT.muted, LIGHT.ink_3),
            ("#ffffff", "#f6f7f9", "#111417", "#4f5357", "#6c6f73")
        );
        assert_eq!(
            (LIGHT.grid, LIGHT.border, LIGHT.accent),
            ("#dcdee0", "#c2c4c7", "#cc291f")
        );
        assert_eq!(
            (DARK.bg, DARK.track, DARK.fg, DARK.muted, DARK.ink_3),
            ("#0c0f11", "#15171a", "#e6e8ea", "#a8abae", "#828588")
        );
        assert_eq!(
            (DARK.grid, DARK.border, DARK.accent),
            ("#2b2e31", "#3f4347", "#f0674e")
        );
        // Nothing in the drawing is pure black or pure white ink.
        for theme in [&LIGHT, &DARK] {
            assert_ne!(theme.fg, "#000000");
            assert_ne!(theme.fg, "#ffffff");
        }
    }

    #[test]
    fn pen_sets_are_fixed_and_distinct() {
        assert_eq!(
            LIGHT.pens,
            [
                "#282c2f", "#cc291f", "#1a609e", "#607c42", "#a25e2b", "#6a588a", "#1e7777",
                "#7b6f66",
            ]
        );
        assert_eq!(
            DARK.pens,
            [
                "#d5d8da", "#f0674e", "#5ca5e1", "#90ba6c", "#e29d5e", "#a88fd6", "#5db5b5",
                "#afa297",
            ]
        );
        for theme in [&LIGHT, &DARK] {
            let mut seen = theme.pens.to_vec();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), PEN_COUNT, "pens must not repeat");
            // The reserved pen is the signal: the live trace draws in
            // drafting red, and nothing else in the pen set is red.
            assert_eq!(theme.pens[SIGNAL_PEN], theme.accent);
        }
    }

    /// Drafting red is never a category color. Neither key-driven accessor
    /// may hand out the reserved pen, however the hash falls.
    #[test]
    fn a_category_can_never_claim_drafting_red() {
        assert!(!CATEGORY_SLOTS.contains(&SIGNAL_PEN));
        assert_eq!(CATEGORY_SLOTS.len(), PEN_COUNT - 1);
        for theme in [&LIGHT, &DARK] {
            for key in [
                "stars",
                "forks",
                "issues",
                "Rust",
                "TypeScript",
                "",
                "a very long series key that hashes somewhere",
            ] {
                assert_ne!(pen_for(theme, key), theme.accent, "{key} claimed signal");
            }
            let many: Vec<String> = (0..200).map(|i| format!("series-{i}")).collect();
            let keys: Vec<&str> = many.iter().map(String::as_str).collect();
            for assigned in pens_for(theme, &keys) {
                assert_ne!(assigned, theme.accent);
            }
        }
        // It stays reachable on purpose, for the one trace that earns it.
        assert_eq!(pen(&LIGHT, SIGNAL_PEN), LIGHT.accent);
        assert_eq!(pen(&DARK, SIGNAL_PEN), DARK.accent);
    }

    /// The whole point of the band: no pen shouts over another, so hue can
    /// never be the sole carrier of meaning and every series must be labelled.
    #[test]
    fn pens_sit_in_one_narrow_lightness_band() {
        for theme in [&LIGHT, &DARK] {
            // Pen 1 is the graphite pen and is deliberately outside the band
            // of the seven chromatic ones.
            let lums: Vec<f64> = theme.pens[1..]
                .iter()
                .map(|p| relative_luminance(p))
                .collect();
            let lo = lums.iter().cloned().fold(f64::MAX, f64::min);
            let hi = lums.iter().cloned().fold(f64::MIN, f64::max);
            assert!(
                hi / lo < 3.0,
                "pens span {lo:.3}..{hi:.3}, too wide to read as one set"
            );
        }
    }

    #[test]
    fn every_pen_clears_aa_on_its_own_ground() {
        for theme in [&LIGHT, &DARK] {
            for pen in theme.pens {
                let ratio = contrast_ratio(pen, theme.bg);
                assert!(ratio >= 4.5, "pen {pen} on {} is {ratio:.2}", theme.bg);
            }
        }
    }

    #[test]
    fn pen_for_is_stable_per_key_across_both_prints() {
        assert_eq!(pen_for(&LIGHT, "stars"), pen_for(&LIGHT, "stars"));
        for key in ["stars", "forks", "issues", "commits", ""] {
            let index = CATEGORY_SLOTS[pen_slot(key)];
            assert_eq!(pen_for(&LIGHT, key), LIGHT.pens[index]);
            assert_eq!(pen_for(&DARK, key), DARK.pens[index]);
        }
        // The flagship pair of this product shares a chart constantly, so it
        // gets two different pens without leaning on `pens_for` to rescue it.
        assert_ne!(pen_for(&LIGHT, "stars"), pen_for(&LIGHT, "forks"));
        assert_eq!(pen(&LIGHT, 0), LIGHT.pens[0]);
        assert_eq!(pen(&LIGHT, PEN_COUNT + 3), LIGHT.pens[3]);
    }

    /// The groups that actually share one chart in this product come back
    /// distinct. Seven slots and four keys will collide on a raw hash sooner
    /// or later, which is precisely the job `pens_for` exists to do.
    #[test]
    fn the_series_that_share_a_chart_get_different_pens() {
        for group in [
            &["stars", "forks"][..],
            &["opened", "closed", "merged"],
            &["additions", "deletions"],
            &["code", "comments", "blank"],
            &["owned", "external", "visionary"],
            &["stars", "forks", "issues", "prs"],
        ] {
            let mut pens = pens_for(&LIGHT, group);
            let count = pens.len();
            pens.sort_unstable();
            pens.dedup();
            assert_eq!(pens.len(), count, "{group:?} share a pen");
        }
    }

    #[test]
    fn pens_for_is_distinct_and_independent_of_list_order() {
        let langs = [
            "Rust",
            "TypeScript",
            "Python",
            "Go",
            "JavaScript",
            "C",
            "Shell",
        ];
        let assigned = pens_for(&LIGHT, &langs);
        assert_eq!(assigned.len(), langs.len());
        let mut distinct = assigned.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            CATEGORY_SLOTS.len(),
            "seven series, seven category pens"
        );

        // Reversing the caller's list must not recolor a single series.
        let mut reversed: Vec<&str> = langs.to_vec();
        reversed.reverse();
        let by_reversed = pens_for(&LIGHT, &reversed);
        for (index, name) in langs.iter().enumerate() {
            let mirrored = reversed.iter().position(|k| k == name).expect("key");
            assert_eq!(assigned[index], by_reversed[mirrored], "{name} moved");
        }

        // Deterministic, and the same slots in both prints.
        assert_eq!(assigned, pens_for(&LIGHT, &langs));
        for (light, dark) in assigned.iter().zip(pens_for(&DARK, &langs)) {
            let slot = LIGHT.pens.iter().position(|p| p == light).expect("slot");
            assert_eq!(dark, DARK.pens[slot]);
        }

        // Past seven the pens have to repeat, but nothing panics and the
        // length still matches the input.
        let many: Vec<&str> = (0..20).map(|i| ["a", "b", "c", "d", "e"][i % 5]).collect();
        assert_eq!(pens_for(&LIGHT, &many).len(), 20);
        assert!(pens_for(&LIGHT, &[]).is_empty());
    }

    #[test]
    fn contrast_picks_paper_on_dark_fills() {
        assert_eq!(contrast_on(LIGHT.fg), "#ffffff");
        assert_eq!(contrast_on("#000000"), "#ffffff");
        // Light drafting red carries white lettering at 5.38.
        assert_eq!(contrast_on(LIGHT.accent), "#ffffff");
    }

    #[test]
    fn contrast_picks_graphite_on_light_fills() {
        assert_eq!(contrast_on(LIGHT.bg), "#111417");
        assert_eq!(contrast_on(LIGHT.track), "#111417");
        assert_eq!(contrast_on(DARK.fg), "#111417");
        // The regression the old 145 threshold shipped: dark drafting red is
        // YIQ 141, and white on it is only 3.1.
        assert_eq!(contrast_on(DARK.accent), "#111417");
    }

    /// The cheap YIQ threshold has to agree with real WCAG contrast for every
    /// color the drawing can hand it, or `contrast_on` is just a guess.
    #[test]
    fn contrast_on_agrees_with_wcag_across_the_whole_palette() {
        let mut candidates: Vec<&'static str> = Vec::new();
        for theme in [&LIGHT, &DARK] {
            candidates.extend([
                theme.bg,
                theme.track,
                theme.fg,
                theme.muted,
                theme.ink_3,
                theme.grid,
                theme.border,
                theme.accent,
            ]);
            candidates.extend(theme.pens);
        }
        for fill in candidates {
            let picked = contrast_on(fill);
            let ink = contrast_ratio(LETTERING_INK, fill);
            let paper = contrast_ratio(LETTERING_PAPER, fill);
            let best = if ink > paper {
                LETTERING_INK
            } else {
                LETTERING_PAPER
            };
            assert_eq!(picked, best, "on {fill}: ink {ink:.2}, paper {paper:.2}");
            assert!(
                ink.max(paper) >= 4.5,
                "no legible lettering exists on {fill}"
            );
        }
    }

    /// The two lettering constants are the light print's own ink and paper.
    /// Kept as literals because a `const` cannot read a `static`.
    #[test]
    fn lettering_constants_track_the_light_print() {
        assert_eq!(LETTERING_INK, LIGHT.fg);
        assert_eq!(LETTERING_PAPER, LIGHT.bg);
    }

    #[test]
    fn contrast_on_falls_back_to_graphite_without_panicking() {
        for bad in [
            "",
            "#fff",
            "not a color",
            "#gggggg",
            "#1114177",
            "111417",
            "#11 417",
            // Seven BYTES but not seven characters. Slicing this by byte
            // index used to split a code point and panic.
            "#😀ab",
            "#éée",
            "🎨🎨",
        ] {
            assert_eq!(contrast_on(bad), "#111417", "on {bad:?}");
        }
        // Case is not significant, and every valid channel round-trips.
        assert_eq!(contrast_on("#FFFFFF"), contrast_on("#ffffff"));
        assert_eq!(parse_hex_rgb("#0aF3c9"), Some((10, 243, 201)));
    }

    #[test]
    fn theme_for_defaults_to_light() {
        assert!(std::ptr::eq(theme_for(None), &LIGHT));
        assert!(std::ptr::eq(theme_for(Some("garbage")), &LIGHT));
        assert!(std::ptr::eq(theme_for(Some("light")), &LIGHT));
    }

    #[test]
    fn theme_for_dark_case_insensitive() {
        assert!(std::ptr::eq(theme_for(Some("dark")), &DARK));
        assert!(std::ptr::eq(theme_for(Some("DARK")), &DARK));
        assert!(std::ptr::eq(theme_for(Some("Dark")), &DARK));
    }
}
