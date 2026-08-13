/**
 * Shared layout and typography tokens.
 *
 * One heading scale sections the whole product, one body scale carries prose,
 * and every navigable surface uses either the tile pattern or the row pattern.
 * These constants exist so the same decision is not re-made in thirty files.
 */

/** Section heading. Weight stays at 400 everywhere but `<th>` and the wordmark. */
export const HEADING = "text-lg font-normal tracking-tight";

/** Page title. The only step above `HEADING`. */
export const TITLE =
  "text-2xl font-normal tracking-tight text-balance sm:text-3xl";

/** Eyebrow. It replaces a title, it never stacks above one. */
export const EYEBROW =
  "font-mono text-[10px] font-normal tracking-[0.25em] text-muted-foreground/70 uppercase";

/** Body prose. */
export const BODY =
  "text-[13px] leading-relaxed text-muted-foreground [text-wrap:pretty]";

/** Page-header prose: the one paragraph directly under a page `h1`. */
export const LEAD = "max-w-[62ch] text-[15px]";

/** Section prose: every other block of running text. There is no third measure. */
export const MEASURE = "max-w-[68ch]";

/** Caption, footnote, and inline status text. */
export const CAPTION = "text-[11px] leading-relaxed text-muted-foreground";

/** Headline number. */
export const KPI = "text-[19px] leading-none tracking-tight tabular-nums";

/** In-flow panel. No shadow: shadows belong to floating layers only. */
export const PANEL = "rounded-lg border border-border/60 bg-background/40";

/** Panel with the single shared padding value. */
export const PANEL_PADDED = `${PANEL} p-3.5`;

/**
 * Navigable tile. Borderless and backgroundless: the preview carries the
 * affordance, and `-m-2 p-2` grows the hit target without growing the box.
 */
export const TILE =
  "group block -m-2 rounded-md p-2 outline-none focus-visible:ring-2 focus-visible:ring-accent/30";

/** Tile title. */
export const TILE_TITLE =
  "mt-5 text-[13px] font-normal text-foreground/90 transition-colors duration-150 group-hover:text-foreground";

/** Tile description. */
export const TILE_DESC =
  "mt-1.5 text-[11px] leading-relaxed text-muted-foreground [text-wrap:pretty]";

/** Navigable list row. `aria-current="page"` marks the active one. */
export const ROW =
  "group relative flex min-h-10 items-center gap-2.5 rounded-md px-2.5 font-mono text-[12px] text-muted-foreground outline-none transition-colors duration-150 hover:bg-card/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-accent/30 aria-[current=page]:bg-card aria-[current=page]:text-foreground";

/** Trailing count on a row. */
export const ROW_BADGE =
  "rounded border border-border/60 px-1 text-[10px] tabular-nums";

/** Section header: a baseline row, never centered. */
export const SECTION_HEADER = "flex items-baseline justify-between gap-4";

/** The quiet 11px action that closes a section header. */
export const SECTION_ACTION =
  "group inline-flex items-center gap-1.5 text-[11px] text-muted-foreground outline-none transition-colors duration-150 hover:text-foreground focus-visible:ring-2 focus-visible:ring-accent/30 rounded";
