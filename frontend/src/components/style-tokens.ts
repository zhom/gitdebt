/**
 * The drawing's type and surface scale.
 *
 * Three voices, and there is no fourth:
 *
 *   DRAFT  — the drawing's own hand (Format 1452). Headings, field labels, and
 *            measured values. It letters the drawing, never the prose.
 *   PROSE  — system-ui. Every sentence a person reads.
 *   DATUM  — Geist Mono. Text that is genuinely a value: a repository slug, a
 *            figure in a column, a snippet.
 *
 * Choosing a voice is choosing what a string IS, not how large it should look.
 * That is why there is no `SMALL` and no `LABEL` here: a caption is prose set
 * small, and a field label is the drawing's hand. They are different things and
 * they are never interchangeable, which is what stops one tracked-out costume
 * from landing on every short string on the page.
 */

/* ── The drawing's hand ──────────────────────────────────────────────────── */

/** Sheet title. One per page, and it is the drawing's subject. */
export const TITLE = "font-draft text-[2.25rem] leading-[0.95] sm:text-[3rem]";

/** Section heading. The only step below TITLE. */
export const HEADING = "font-draft text-[1.375rem] leading-[1.1]";

/** A field label on the drawing: it names a measured quantity. */
export const FIELD = "drafted";

/** A measured value: drafting red, tabular, never present without its subject. */
export const VALUE = "measured";

/** A headline figure. The drawing's hand, set at the scale of a dimension. */
export const FIGURE =
  "font-draft text-[1.75rem] leading-none tabular-nums tracking-tight";

/* ── Prose ──────────────────────────────────────────────────────────────── */

/** Body prose. */
export const BODY = "text-[0.875rem] leading-[1.65] text-ink-2 [text-wrap:pretty]";

/** The one paragraph under a sheet title. */
export const LEAD = "text-[1.0625rem] leading-[1.55] text-ink-2 [text-wrap:pretty]";

/** Caption and footnote: prose, set small. Not a label, and not the draft face. */
export const CAPTION = "text-[0.75rem] leading-[1.5] text-ink-3 [text-wrap:pretty]";

/** Reading measure. There is one, and it applies to every block of prose. */
export const MEASURE = "max-w-[64ch]";

/* ── Data ───────────────────────────────────────────────────────────────── */

/** A repository slug, a figure in a column, an identifier. */
export const DATUM = "font-mono text-[0.8125rem] tabular-nums";

/* ── Surfaces ───────────────────────────────────────────────────────────── */

/**
 * A panel on the sheet. It is defined by a drawn edge and a step in ground,
 * never by a shadow: the paper is white and the table beneath it is not, and
 * that one step is the whole elevation model.
 */
export const PANEL = "cut-edge [--pad-x:1rem] [--pad-y:1rem] p-4";

/**
 * A navigable row. The affordance is the ground stepping to paper under the
 * pointer — nothing lifts, nothing glows, and nothing grows an underline.
 */
export const ROW =
  "group relative flex min-h-11 items-center gap-3 px-3 text-ink-2 outline-none transition-colors duration-[--duration-ui] hover:bg-paper hover:text-ink focus-visible:outline-2 focus-visible:outline-signal aria-[current=page]:bg-paper aria-[current=page]:text-ink";

/**
 * A section header. A baseline row: the heading and its one action share a
 * baseline, so the action reads as part of the heading rather than as chrome
 * floating beside it.
 */
export const SECTION_HEADER = "flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2";

/**
 * The quiet action that closes a section header.
 *
 * It is text and ink, and nothing else: the heading's own baseline holds it, so
 * it reads as part of the heading rather than as chrome floating beside it, and
 * under the pointer it takes the signal. It carries no arrow. The leader mark
 * belongs to `ButtonLink`, where there is a control for it to travel out of;
 * bolted onto a bare text link it is an ornament, and all seventeen call sites
 * ship without one. (This string used to declare `group` for a leader that no
 * call site has ever rendered — a hover scope with nothing inside it to drive.)
 */
export const SECTION_ACTION =
  "inline-flex items-baseline gap-1.5 text-[0.8125rem] text-ink-3 outline-none transition-colors duration-[--duration-ui] hover:text-signal focus-visible:outline-2 focus-visible:outline-signal";

/* ── Navigation ─────────────────────────────────────────────────────────── */

/**
 * The location line: which sheet in the set this one is.
 *
 * It is a path, so it is set in the data face rather than the drawing's hand —
 * `gitdebt / compare / build-tools` is an address, not a label, and lettering
 * it in tracked-out caps would put the drawing's one small-text costume on a
 * fourth kind of string.
 */
export const BREADCRUMB = "font-mono text-[0.75rem] text-ink-3";

/** A crumb that is still somewhere you can go. Ink is the only thing that moves. */
export const CRUMB =
  "outline-none transition-colors duration-[--duration-ui] hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal";

/**
 * A link inside a sentence.
 *
 * It carries its underline at rest, because a prose link that only reveals
 * itself under the pointer is a link nobody finds — and the underline is drawn
 * in rule, not ink, so it reads as the drawing's own hairline rather than as a
 * heavier second line under the words. Nothing about it grows on hover; the ink
 * changes and that is all.
 */
export const PROSE_LINK =
  "underline decoration-rule-strong underline-offset-[3px] outline-none transition-colors duration-[--duration-ui] hover:text-signal hover:decoration-signal focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal";
