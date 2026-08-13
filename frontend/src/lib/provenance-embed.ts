/**
 * The README block that publishes a star-history chart with its provenance
 * stated beside it.
 *
 * Provenance is TEXT in the README, never a parameter on the image. There is no
 * provenance-stamped image route and no `provenance=1` chart parameter — baking
 * the coverage sentence into `chart.svg` would be a backend change (renderer,
 * cache key, parity test), so nothing here may pretend otherwise. Keeping it
 * out of the URL also keeps the CDN cache key and the `ref=readme` contract
 * exactly as they are: `?ref=readme` rides the two anchors, both image URLs
 * stay plain, and `MEDIA_RENDER_REVISION` — an on-page preview concern — never
 * reaches a copied snippet.
 *
 * The picture half is `bestEmbed()` verbatim, so what a reader copies here is
 * byte-identical to what /badges shows and what `/api/md` serves.
 *
 * Relative specifiers with their extension, as in `readme-embeds.ts`: this
 * module is covered by the Node test runner in `scripts/`, which resolves
 * neither the `@/` alias nor an extensionless specifier.
 */

import {
  coverageLabel,
  historyFreshness,
  sourceLabel,
  stateLabel,
  type HistorySnapshot,
} from "./history-freshness.ts";
import { bestEmbed, readmeLink, repoEmbedAssets } from "./readme-embeds.ts";

export type ProvenanceReadmeBlock = {
  snippet: string;
  language: "html";
};

export type ProvenanceReadmeInput = {
  /** Origin serving the media routes, e.g. https://api.gitdebt.dev. */
  apiBase: string;
  /** Origin serving the reports, e.g. https://gitdebt.dev. */
  siteOrigin: string;
  /** owner/repo. */
  slug: string;
  /** Subset of the analyze payload. Anything unclassifiable yields null. */
  snapshot?: HistorySnapshot | null;
};

/**
 * The chart embed plus one provenance line, or null.
 *
 * Null when the source is not established. Publishing "source not established"
 * into somebody's README would be an unestablished claim standing in a
 * permanent place, and publishing nothing is strictly better than that.
 */
export function provenanceReadmeBlock({
  apiBase,
  siteOrigin,
  slug,
  snapshot,
}: ProvenanceReadmeInput): ProvenanceReadmeBlock | null {
  const freshness = historyFreshness(snapshot);
  if (freshness.state === "unknown") return null;

  const asset = repoEmbedAssets(slug).find((item) => item.id === "chart");
  if (!asset) return null;

  const link = readmeLink(siteOrigin, `/${slug}`);
  const picture = bestEmbed(apiBase, asset, link);
  // Source, date, state. No count, no share, no score — the same ban the
  // notice copy is held to, and for the same reason.
  const provenance = `<sub>Star history source: ${sourceLabel(freshness)} · ${coverageLabel(freshness)} · ${stateLabel(freshness)} — <a href="${link}">gitdebt</a></sub>`;

  return { snippet: `${picture}\n\n${provenance}`, language: "html" };
}
