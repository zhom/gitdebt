export type FileAgeRange =
  | "this_month"
  | "within_year"
  | "two_to_three_years"
  | "older";

export type FileAgeBand = {
  range: FileAgeRange;
  /** Files whose most recent observed change lands in this range. */
  files: number;
  /** Total observed file changes across those files. */
  changes: number;
};

export type FileCoupling = {
  source: string;
  target: string;
  /** Commits in which both files changed. */
  cochanges: number;
  /** Co-changing commits classified as fixes. */
  fix_commits: number;
};

export type AgeRing = {
  range: FileAgeRange;
  files: number;
  changes: number;
  fileShare: number;
  changeRate: number;
  changeIntensity: number;
  innerRadius: number;
  outerRadius: number;
};

export type CouplingNode = {
  id: string;
  label: string;
  cluster: string;
  x: number;
  y: number;
  weight: number;
  fixWeight: number;
};

export type CouplingEdge = {
  source: string;
  target: string;
  cochanges: number;
  fixCommits: number;
  strength: number;
  cluster: string;
};

export type CouplingCluster = {
  id: string;
  weight: number;
  fixWeight: number;
};

export type CouplingLayout = {
  nodes: CouplingNode[];
  edges: CouplingEdge[];
  clusters: CouplingCluster[];
};

export const AGE_ORDER: readonly FileAgeRange[] = [
  "this_month",
  "within_year",
  "two_to_three_years",
  "older",
];

export const AGE_LABEL: Record<FileAgeRange, string> = {
  this_month: "Changed this month",
  within_year: "Changed within 1 year",
  two_to_three_years: "Changed 1–3 years ago",
  older: "Older than 3 years",
};

const cleanCount = (value: number) =>
  Number.isFinite(value) ? Math.max(0, Math.round(value)) : 0;

function stableHash(value: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/**
 * Complete, ordered rings. Missing ranges remain present with zero values so
 * the radial position never changes from repository to repository.
 */
export function layoutAgeRings(bands: readonly FileAgeBand[]): AgeRing[] {
  const grouped = new Map<
    FileAgeRange,
    { files: number; changes: number }
  >();
  for (const band of bands) {
    if (!AGE_ORDER.includes(band.range)) continue;
    const current = grouped.get(band.range) ?? { files: 0, changes: 0 };
    current.files += cleanCount(band.files);
    current.changes += cleanCount(band.changes);
    grouped.set(band.range, current);
  }

  const rows = AGE_ORDER.map((range) => ({
    range,
    ...(grouped.get(range) ?? { files: 0, changes: 0 }),
  }));
  const totalFiles = Math.max(
    1,
    rows.reduce((sum, row) => sum + row.files, 0),
  );
  const rates = rows.map((row) =>
    row.files > 0 ? row.changes / row.files : 0,
  );
  const maxRate = Math.max(1, ...rates);
  const innerStart = 0.18;
  const gap = 0.035;
  const width = (0.96 - innerStart - gap * (rows.length - 1)) / rows.length;

  return rows.map((row, index) => {
    const innerRadius = innerStart + index * (width + gap);
    const rate = rates[index];
    return {
      ...row,
      fileShare: row.files / totalFiles,
      changeRate: rate,
      changeIntensity: rate / maxRate,
      innerRadius,
      outerRadius: innerRadius + width,
    };
  });
}

/** Resolve a normalized pointer to a ring, excluding its inter-band gaps. */
export function ageRingAtPoint(
  x: number,
  y: number,
  width: number,
  height: number,
  rings: readonly AgeRing[],
): number | null {
  if (width <= 0 || height <= 0) return null;
  const dx = x - width / 2;
  const dy = y - height / 2;
  const radius = Math.hypot(dx, dy) / (Math.min(width, height) * 0.5);
  const index = rings.findIndex(
    (ring) => radius >= ring.innerRadius && radius <= ring.outerRadius,
  );
  return index >= 0 ? index : null;
}

function fileCluster(path: string): string {
  const clean = path.replace(/^\.?\//, "");
  const slash = clean.indexOf("/");
  return slash > 0 ? clean.slice(0, slash) : "(root)";
}

function fileLabel(path: string): string {
  const clean = path.replace(/^\.?\//, "");
  const parts = clean.split("/");
  const tail = parts.at(-1) || clean;
  return tail.length > 18 ? `${tail.slice(0, 15)}…` : tail;
}

type CleanEdge = {
  source: string;
  target: string;
  cochanges: number;
  fixCommits: number;
  score: number;
};

function cleanCouplings(couplings: readonly FileCoupling[]): CleanEdge[] {
  const merged = new Map<string, CleanEdge>();
  for (const edge of couplings) {
    const source = edge.source.trim();
    const target = edge.target.trim();
    if (!source || !target || source === target) continue;
    const [left, right] =
      source.localeCompare(target, "en") <= 0
        ? [source, target]
        : [target, source];
    const key = `${left}\0${right}`;
    const cochanges = cleanCount(edge.cochanges);
    const fixCommits = cleanCount(edge.fix_commits);
    if (cochanges === 0 && fixCommits === 0) continue;
    const current = merged.get(key);
    if (current) {
      current.cochanges += cochanges;
      current.fixCommits += fixCommits;
      current.score =
        current.cochanges + current.fixCommits * 1.75;
    } else {
      merged.set(key, {
        source: left,
        target: right,
        cochanges,
        fixCommits,
        score: cochanges + fixCommits * 1.75,
      });
    }
  }
  return [...merged.values()].sort(
    (a, b) =>
      b.score - a.score ||
      b.fixCommits - a.fixCommits ||
      a.source.localeCompare(b.source, "en") ||
      a.target.localeCompare(b.target, "en"),
  );
}

/**
 * Deterministic, bounded graph layout. It keeps only the strongest evidence,
 * then gives top-level path clusters stable centers and stable local orbits.
 */
export function layoutFileCouplings(
  couplings: readonly FileCoupling[],
  maxNodes = 14,
  maxEdges = 20,
): CouplingLayout {
  const candidates = cleanCouplings(couplings).slice(0, Math.max(1, maxEdges));
  const nodeScore = new Map<string, number>();
  for (const edge of candidates) {
    nodeScore.set(
      edge.source,
      (nodeScore.get(edge.source) ?? 0) + edge.score,
    );
    nodeScore.set(
      edge.target,
      (nodeScore.get(edge.target) ?? 0) + edge.score,
    );
  }
  const allowed = new Set(
    [...nodeScore]
      .sort(
        (a, b) =>
          b[1] - a[1] || a[0].localeCompare(b[0], "en"),
      )
      .slice(0, Math.max(2, maxNodes))
      .map(([path]) => path),
  );
  const kept = candidates.filter(
    (edge) => allowed.has(edge.source) && allowed.has(edge.target),
  );
  if (kept.length === 0) return { nodes: [], edges: [], clusters: [] };

  const maxStrength = Math.max(1, ...kept.map((edge) => edge.score));
  const clusterTotals = new Map<
    string,
    { weight: number; fixWeight: number }
  >();
  const nodeTotals = new Map<
    string,
    { weight: number; fixWeight: number; cluster: string }
  >();
  for (const edge of kept) {
    for (const path of [edge.source, edge.target]) {
      const cluster = fileCluster(path);
      const node = nodeTotals.get(path) ?? {
        weight: 0,
        fixWeight: 0,
        cluster,
      };
      node.weight += edge.cochanges;
      node.fixWeight += edge.fixCommits;
      nodeTotals.set(path, node);

      const total = clusterTotals.get(cluster) ?? {
        weight: 0,
        fixWeight: 0,
      };
      total.weight += edge.cochanges;
      total.fixWeight += edge.fixCommits;
      clusterTotals.set(cluster, total);
    }
  }

  const clusters = [...clusterTotals]
    .map(([id, totals]) => ({ id, ...totals }))
    .sort(
      (a, b) =>
        b.fixWeight - a.fixWeight ||
        b.weight - a.weight ||
        a.id.localeCompare(b.id, "en"),
    );
  const clusterIndex = new Map(
    clusters.map((cluster, index) => [cluster.id, index]),
  );
  const byCluster = new Map<string, string[]>();
  for (const [path, node] of nodeTotals) {
    const paths = byCluster.get(node.cluster) ?? [];
    paths.push(path);
    byCluster.set(node.cluster, paths);
  }
  for (const paths of byCluster.values()) {
    paths.sort(
      (a, b) =>
        (nodeTotals.get(b)?.fixWeight ?? 0) -
          (nodeTotals.get(a)?.fixWeight ?? 0) ||
        (nodeTotals.get(b)?.weight ?? 0) -
          (nodeTotals.get(a)?.weight ?? 0) ||
        a.localeCompare(b, "en"),
    );
  }

  const nodes: CouplingNode[] = [];
  for (const cluster of clusters) {
    const index = clusterIndex.get(cluster.id) ?? 0;
    const clusterAngle =
      -Math.PI / 2 + (index / Math.max(1, clusters.length)) * Math.PI * 2;
    const clusterRadius = clusters.length === 1 ? 0 : 0.25;
    const centerX = 0.5 + Math.cos(clusterAngle) * clusterRadius;
    const centerY = 0.5 + Math.sin(clusterAngle) * clusterRadius * 0.68;
    const paths = byCluster.get(cluster.id) ?? [];
    paths.forEach((path, localIndex) => {
      const angleSeed = stableHash(path) / 4294967295;
      const angle =
        angleSeed * Math.PI * 2 +
        (localIndex / Math.max(1, paths.length)) * Math.PI * 2;
      const orbit =
        paths.length === 1 ? 0 : 0.075 + 0.035 * (localIndex % 3);
      const totals = nodeTotals.get(path)!;
      nodes.push({
        id: path,
        label: fileLabel(path),
        cluster: cluster.id,
        x: Math.max(0.08, Math.min(0.92, centerX + Math.cos(angle) * orbit)),
        y: Math.max(
          0.12,
          Math.min(0.88, centerY + Math.sin(angle) * orbit * 0.8),
        ),
        weight: totals.weight,
        fixWeight: totals.fixWeight,
      });
    });
  }
  nodes.sort((a, b) => a.id.localeCompare(b.id, "en"));

  const edges = kept
    .map((edge) => {
      const sourceCluster = nodeTotals.get(edge.source)!.cluster;
      const targetCluster = nodeTotals.get(edge.target)!.cluster;
      const sourceTotals = clusterTotals.get(sourceCluster)!;
      const targetTotals = clusterTotals.get(targetCluster)!;
      const crossCluster =
        sourceTotals.fixWeight !== targetTotals.fixWeight
          ? sourceTotals.fixWeight > targetTotals.fixWeight
            ? sourceCluster
            : targetCluster
          : sourceTotals.weight >= targetTotals.weight
            ? sourceCluster
            : targetCluster;
      return {
        source: edge.source,
        target: edge.target,
        cochanges: edge.cochanges,
        fixCommits: edge.fixCommits,
        strength: edge.score / maxStrength,
        cluster:
          sourceCluster === targetCluster
            ? sourceCluster
            : crossCluster,
      };
    })
    .sort(
      (a, b) =>
        b.fixCommits - a.fixCommits ||
        b.strength - a.strength ||
        a.source.localeCompare(b.source, "en") ||
        a.target.localeCompare(b.target, "en"),
    );

  return { nodes, edges, clusters };
}
