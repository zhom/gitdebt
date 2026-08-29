/**
 * The robot path, lifted out of the canonical mark.
 *
 * `Brand.astro` and the footer both need the silhouette as a bare `d` string so
 * they can draw it at their own size with their own ink. They each used to
 * carry their own regex keyed on the fill the artwork happened to ship with,
 * which meant that restyling the mark threw at build time from two different
 * files for the same reason.
 *
 * This matches on the path's own data instead of on its fill, so it holds
 * whatever the mark is filled with — do not "simplify" it back to a fill
 * lookup. The `M320.5` prefix is the canonical
 * starting point of the artwork and is asserted on the Rust side too
 * (`backend/src/brand.rs`), which makes it the one stable handle in the file.
 */

const ROBOT_D = /\sd="(M320\.5[^"]+)"/;

/** Ink bounds of the robot inside its 512x512 artboard. */
export const MARK_VIEWBOX = "41.436 108.392 429.115 299.305";

export function robotPath(source: string): string {
  const match = source.match(ROBOT_D);
  if (!match) {
    throw new Error(
      "assets/gitdebt-logo.svg no longer contains the canonical robot path (expected a d attribute starting M320.5)",
    );
  }
  return match[1];
}
