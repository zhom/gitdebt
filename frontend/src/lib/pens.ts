/**
 * Which plotter pen draws which series.
 *
 * The same rule, hash and probe as `theme::pens_for` in the backend, so a
 * repository takes the same ink on the page as in the README overlay drawn
 * from the same slugs. Pen 2 is the drafting red, reserved for a measured
 * value, so a comparison has seven category pens and never draws in red.
 */

/** Indices into the eight-pen set a category may claim. Pen 2 is absent. */
export const CATEGORY_SLOTS = [0, 2, 3, 4, 5, 6, 7] as const;

/** FNV-1a over the key's UTF-8 bytes, finished with a xorshift-multiply
 *  avalanche, reduced to an index into `CATEGORY_SLOTS`. */
export function penSlot(key: string): number {
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(key)) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  hash ^= hash >>> 16;
  hash = Math.imul(hash, 0x7feb352d) >>> 0;
  hash ^= hash >>> 15;
  hash = Math.imul(hash, 0x846ca68b) >>> 0;
  hash ^= hash >>> 16;
  return (hash >>> 0) % CATEGORY_SLOTS.length;
}

/**
 * Pen index per slug, depending on the slug set alone. Keys are ranked before
 * slots are handed out, each claims its preferred slot when free and otherwise
 * the next free one, so up to seven series are always distinct; past seven
 * they fall back to their preferred slot, as the backend does.
 */
export function comparisonPenIndices(slugs: string[]): Map<string, number> {
  const keys = [...new Set(slugs)];
  const ranked = [...keys].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const taken = new Array<boolean>(CATEGORY_SLOTS.length).fill(false);
  const pens = new Map<string, number>();
  for (const key of ranked) {
    const preferred = penSlot(key);
    let slot = preferred;
    for (let step = 0; step < CATEGORY_SLOTS.length; step += 1) {
      const probe = (preferred + step) % CATEGORY_SLOTS.length;
      if (!taken[probe]) {
        slot = probe;
        taken[probe] = true;
        break;
      }
    }
    pens.set(key, CATEGORY_SLOTS[slot]);
  }
  return pens;
}
