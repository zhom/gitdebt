# Design

<!-- impeccable:design-schema 1 -->

The record of gitdebt's visual system, written from the built product. Where
this file and the code disagree, the code is the truth and this file is the bug.

## The world

gitdebt measures a repository. So every surface is **a dimensioned technical
drawing** of one: ink on plot paper, with the notation a draughtsman uses to
state a measured value about a thing.

This was chosen over the two ruts the category ships. The first is the dark
developer-tool page with a neon accent and a glowing chart in a rounded panel,
which is what every star-history tool looks like. The second is its predictable
opposite, the light geometric-sans minimal page, which is what the brief pinned
and which on its own is the default wearing better clothes. The drawing is what
makes the light surface specific: the notation carries the identity, so the page
could not be recolored onto another product.

### The governing rule

> **Every line terminates on something real.**

A dimension line spans two measured points and carries the value of that span. A
leader points at a datum. A frame encloses a sheet. A rule separates two real
regions.

A line that measures nothing, separates nothing and encloses nothing is deleted.
This is the single rule that keeps drafting notation from decaying into
ornament, and it is why there is **no page grid, no graph paper, no texture, no
gradient, no glow and no blurred blob** anywhere in the product. It is also why
`Separator` is documented as unavailable beside a label: that line measures
nothing.

## Colour

Held in oklch in `src/styles/globals.css` and as the same values in hex in
`backend/src/theme.rs`. **They must not drift** — if they do, a chart stops
belonging to the page that embeds it.

| Token | Light | Dark (assets only) | Role |
|---|---|---|---|
| `paper` | `#ffffff` | `#15171a` | the sheet |
| `table` | `#f6f7f9` | `#0c0f11` | the ground the sheet lies on |
| `ink` | `#111417` | `#e6e8ea` | graphite: the object line, primary text |
| `ink-2` | `#4f5357` | `#a8abae` | secondary text |
| `ink-3` | `#6c6f73` | `#828588` | captions, field labels, construction lines |
| `rule` | `#dcdee0` | `#2b2e31` | hairlines |
| `rule-strong` | `#c2c4c7` | `#3f4347` | frame and extension lines |
| `signal` | `#cc291f` | `#f0674e` | **drafting red** |

Contrast is measured, not assumed. On paper: ink 18.5, ink-2 7.8, ink-3 5.05,
signal 5.38, white-on-signal 5.38. On the table ground: ink 17.2, ink-3 4.71. On
the dark ground: ink 15.7, ink-3 5.18, signal 6.19. Every one clears AA.
`ink-3` sat at `oklch(0.575)` during the build and measured 4.37 on paper and
4.08 on the table — a failing caption colour, in the one place nobody checks.
It is `oklch(0.54)` for that reason; do not lighten it.

### How drafting red is spent

Only on: **a measured value and its terminators**, **the live or primary data
trace**, and **at most one primary action per surface**. Roughly 2% coverage.

It is never a tag, a status dot, a category colour, a section heading, or a
decorative accent. `theme.rs` reserves it mechanically: `SIGNAL_PEN` is pen 2
and `pen_for`/`pens_for` allocate from the other seven, so a category can never
hash onto the signal.

### The plotter pens

Multi-series charts only. Eight inks in one narrow lightness band so none
shouts, assigned per series key and never cycled by index. **Every series is
also labelled at its own line end and varied by dash pattern**, so hue is never
the sole carrier of meaning.

Language colours (`conventional_language_color`) are real brand colours and are
NOT part of this palette. Leave them alone.

## Type

Three voices, and there is no fourth.

- **`font-draft` — Format 1452.** Frank Adebiaye, SIL OFL, self-hosted, 11.7 KB.
  A DIN 1451 descendant built from modules with no optical corrections: the
  lettering of the industrial standard sheet, drawn by a machine rather than
  corrected by an eye. It letters the drawing — headings, field labels, measured
  values — and never prose.
- **`font-sans` — `system-ui`.** Every sentence a person reads. Genuinely
  neutral and free; it carries reading, not identity. Inter used to carry the
  old identity and no longer loads.
- **`font-mono` — Geist Mono.** Text that is genuinely a value: a repository
  slug, a figure in a column, a snippet.

**Display headings carry no terminal punctuation.** Format 1452 sets its full
stop as an open ring, which is authentic to a modular face and unmistakable for
a CJK full stop at the end of a sentence. A line break separates display
clauses instead. Prose keeps normal punctuation because prose is system-ui.

Choosing a voice is choosing what a string **is**, not how large it should look.
A caption is prose set small (`CAPTION`); a field label is the drawing's hand
(`FIELD`). They are never interchangeable — that distinction is what stops one
tracked-out costume landing on every short string on the page.

## Shape and elevation

Everything is square. The one exception is the **cut corner**: a 10px chamfer on
the bottom-right of a panel and of the primary action. Nothing else on the site
is chamfered and nothing is rounded. It is the one shape that belongs to
gitdebt.

Depth is the step from `paper` to `table` plus line weight. There is exactly one
shadow in the system, the `lifted` utility, and it belongs to a layer that
genuinely floats above the sheet — a menu, a popover. Nothing else casts.

### `cut-edge` is self-contained — do not also apply `cut`

Two traps, and the first build fell into both, which turned every panel on the
site into a solid grey slab:

1. A `clip-path` shears a border, so the edge cannot be a `border`.
2. A `z-index: -1` pseudo-element does **not** paint behind its own parent's
   background. Painting order puts the stacking-context element's background
   first and negative-z descendants immediately after, so the pseudo covers the
   very background it was meant to sit behind.

So a `cut-edge` element paints nothing itself: `::before` is the edge at full
size, `::after` is the paper inset by one line width, and content paints above
both. `cut` remains, separately, for solid-filled things that clip themselves.

## Motion

**The drawing draws itself.** That is the one gesture the site owns.

- `inks-in` — a trace along its own length, via `stroke-dashoffset`. The length
  is summed in render by `polylineLength`, never measured with
  `getTotalLength()` in an effect: a wrong length at first paint is exactly how
  a stroke animation ends up filling only part of its track.
- `extends` — a dimension line growing from its datum.
- `lands` — a terminator arriving with a half-pixel overshoot.

The absolute rule: **nothing is invisible until an animation runs.** No
`opacity: 0` initial state, anywhere. Content is in the markup and painted at
first paint; the motion happens over a finished drawing. Reduced motion removes
the travel and keeps the mark.

Charts letter at 1:1 — the viewBox width is the **measured** width
(`usePlotWidth`). A fixed viewBox stretched to fit condenses every label to a
third of its width on a phone.

## Actions

Four, because the product has four kinds of action. There were nine, of which
two were byte-identical and three differed only in canvas texture.

- `primary` — the one action on a surface. Drafting red, cut corner, the
  drawing's own lettering. At most one per page.
- `quiet` — a working control. A drawn edge, paper under the pointer. It is
  **not** the outlined half of a filled/outlined pair; where a `primary` shares
  the row, the other action is a `link`.
- `link` — a text action carrying the leader arrow. It leaves.
- `danger` — red ink on paper, never a red fill, so it cannot be mistaken for
  the primary.

Nothing lifts, scales, glows, or grows an underline on hover. A control changes
state by changing ink and ground, in one frame budget.

## Iconography

Five house marks in `ui/marks.tsx`, drawn from the drawing's own vocabulary:
`Leader` (an up-and-out arrow, because that action leaves), `Terminator`,
`Index` (three rules of unequal length — the menu), `Cut` (the section-cut mark
— close), `Tick`. `lucide-react` is not a dependency; a redrawn icon-pack shape
is still the generic outline set.

## What renders where

The site and the README assets are one drawing, but they letter differently and
that is deliberate: an SVG embedded in a README letters with the **viewer's**
system stack, and embedding a webfont would bloat every asset. So the shared
identity travels through the **line grammar**, which survives every renderer,
never through the typeface.

Backend notation lives in `backend/src/texture.rs` — `terminator`,
`extension_tick`, `dimension_h`/`dimension_v`, `leader`, `cut_text`,
`chamfered_rect_path`, `title_block`, `series_bar`. The module keeps its old
name so `crate::texture::` paths resolve; it no longer paints any texture, and
renaming it to `notation` is a clean follow-up.

## Line weights

Three, and no others: `0.5px` construction and extension lines, `1px` the object
and dimension lines, `2px` a cut or an emphasis.
