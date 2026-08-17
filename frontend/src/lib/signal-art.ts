export type SignalArtMode = "mosaic" | "braille" | "contour";
export type InsightKind =
  | "repository"
  | "profile"
  | "comparison"
  | "category"
  | "general";

export type PageInsight = {
  kind: InsightKind;
  mode: SignalArtMode;
  seed: string;
  eyebrow: string;
  question: string;
  answer: string;
  labels: readonly string[];
  values: readonly number[];
};

const RESERVED = new Set([
  "",
  "404",
  "about",
  "badges",
  "compare",
  "leaderboard",
  "privacy",
  "profile",
  "report",
  "terms",
]);

export const clampUnit = (value: number) =>
  value < 0 ? 0 : value > 1 ? 1 : value;

/** FNV-1a gives page visuals a stable identity without storing user data. */
export function signalSeed(value: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

export function signalHash(seed: number, x: number, y: number): number {
  let hash =
    seed ^
    Math.imul((x | 0) + 0x51ed, 0x45d9f3b) ^
    Math.imul((y | 0) + 0x19b9, 0x119de1f3);
  hash = Math.imul(hash ^ (hash >>> 16), 0x7feb352d);
  hash = Math.imul(hash ^ (hash >>> 15), 0x846ca68b);
  return ((hash ^ (hash >>> 16)) >>> 0) / 4294967296;
}

export function normalizeSignals(values: readonly number[]): number[] {
  const finite = values.filter((value) => Number.isFinite(value));
  const max = Math.max(1, ...finite.map((value) => Math.abs(value)));
  const normalized = finite.map((value) => clampUnit(Math.abs(value) / max));
  return normalized.length > 0 ? normalized : [0.5];
}

/**
 * Low-resolution luminance source for the Neon Mirage mosaic.
 * The signal vector changes the wave profile; seed changes its phase.
 */
export function mosaicSignal(
  seed: number,
  x: number,
  y: number,
  cols: number,
  rows: number,
  phase: number,
  values: readonly number[],
): number {
  const u = (x + 0.5) / Math.max(1, cols);
  const v = (y + 0.5) / Math.max(1, rows);
  const signals = normalizeSignals(values);
  const primary = signals[x % signals.length] ?? 0.5;
  const secondary = signals[(x + 1) % signals.length] ?? primary;
  const offset = (seed & 1023) / 1023;
  const ridge =
    0.54 +
    0.2 * Math.sin(u * Math.PI * (2.4 + primary * 1.4) + phase + offset * 5) +
    0.09 * Math.sin(u * 13 - phase * 0.7 + secondary * 4);
  const distance = Math.abs(v - ridge);
  const glow = Math.exp(-distance * (9 + primary * 7));
  const horizon = clampUnit((v - 0.22) / 0.72) * (0.15 + primary * 0.18);
  const grain = (signalHash(seed, x, y) - 0.5) * 0.13;
  return clampUnit(glow * 0.84 + horizon + grain);
}

const BRAILLE = ["⠁", "⠃", "⠉", "⠙", "⠛", "⠟", "⠿", "⣿"] as const;

export function brailleGlyph(
  seed: number,
  column: number,
  row: number,
  phase: number,
  values: readonly number[],
): string {
  const signals = normalizeSignals(values);
  const strength = signals[column % signals.length] ?? 0.5;
  const drift = Math.floor(phase * (1.5 + strength * 2.5));
  const sample = signalHash(seed, column, row - drift);
  const index = Math.min(
    BRAILLE.length - 1,
    Math.floor(clampUnit(sample * 0.72 + strength * 0.35) * BRAILLE.length),
  );
  return BRAILLE[index];
}

/** Smooth scalar field used by marching contour lines and hex annotations. */
export function contourSignal(
  seed: number,
  u: number,
  v: number,
  phase: number,
  values: readonly number[],
): number {
  const signals = normalizeSignals(values);
  const a = signals[0] ?? 0.5;
  const b = signals[1] ?? a;
  const seedPhase = (seed & 2047) / 2047;
  const left =
    Math.sin((u * (2.2 + a) + v * 1.3 + phase * 0.08 + seedPhase) * Math.PI);
  const right = Math.cos(
    (u * 1.4 - v * (2.4 + b) - phase * 0.06 - seedPhase) * Math.PI,
  );
  const basin = Math.sin(
    Math.hypot(u - 0.34, v - 0.56) * Math.PI * (5 + a * 2) - phase * 0.12,
  );
  return clampUnit(0.5 + left * 0.2 + right * 0.18 + basin * 0.12);
}

/**
 * A bounded, data-shaped path for the repository background's moving ASCII
 * currents. Bands remain stable for a route but travel at distinct speeds.
 */
export function ambientWave(
  seed: number,
  u: number,
  band: number,
  phase: number,
  values: readonly number[],
): number {
  const signals = normalizeSignals(values);
  const strength = signals[band % signals.length] ?? 0.5;
  const offset = ((seed >>> (band * 3)) & 255) / 255;
  const center = 0.2 + band * 0.22;
  const primary = Math.sin(
    u * Math.PI * (2.2 + strength * 1.8) +
      phase * (0.2 + band * 0.045) +
      offset * Math.PI * 2,
  );
  const detail = Math.sin(
    u * Math.PI * (9 + band * 1.7) -
      phase * 0.12 +
      strength * Math.PI,
  );
  return clampUnit(center + primary * (0.07 + strength * 0.045) + detail * 0.018);
}

export type AmbientOrbitPoint = {
  x: number;
  y: number;
  energy: number;
};

/**
 * Elliptical braille packets for profile backgrounds. Returning normalized
 * coordinates keeps the painter responsive and makes the motion testable.
 */
export function ambientOrbitPoint(
  seed: number,
  index: number,
  total: number,
  ring: number,
  phase: number,
  values: readonly number[],
): AmbientOrbitPoint {
  const signals = normalizeSignals(values);
  const strength = signals[ring % signals.length] ?? 0.5;
  const seedPhase = ((seed >>> (ring * 4)) & 511) / 511;
  const angle =
    (index / Math.max(1, total)) * Math.PI * 2 +
    phase * (0.07 + ring * 0.018) * (ring % 2 === 0 ? 1 : -1) +
    seedPhase * Math.PI * 2;
  const radiusX = 0.13 + ring * 0.048 + strength * 0.02;
  const radiusY = radiusX * (0.44 + strength * 0.12);
  const ripple = 1 + Math.sin(angle * (3 + ring) - phase * 0.16) * 0.035;
  return {
    x: clampUnit(0.68 + Math.cos(angle) * radiusX * ripple),
    y: clampUnit(0.34 + Math.sin(angle) * radiusY * ripple),
    energy: clampUnit(
      0.28 +
        strength * 0.42 +
        signalHash(seed ^ ring, index, ring) * 0.3,
    ),
  };
}

function segmentValues(segments: readonly string[], seed: string): number[] {
  return [
    ...segments.map((segment) => Math.max(1, segment.length)),
    (signalSeed(seed) & 255) + 1,
  ];
}

/**
 * Selects a useful, visible query answer and the visual grammar for a route.
 * Callers may override this with richer page data later without changing the
 * canvas API.
 */
export function pageInsightForPath(pathname: string): PageInsight {
  const path = pathname.split(/[?#]/, 1)[0] ?? "/";
  const segments = path
    .split("/")
    .filter(Boolean)
    .map((segment) => decodeURIComponent(segment).toLowerCase());

  // Astro's file output may expose `/owner/repo.html` while prerendering.
  // Keep that implementation detail out of visible copy and visual seeds.
  const finalSegment = segments.at(-1);
  if (finalSegment === "index.html" || finalSegment === "index.htm") {
    segments.pop();
  } else if (finalSegment) {
    segments[segments.length - 1] = finalSegment.replace(/\.(?:html?|md)$/i, "");
  }
  const seed = segments.join("/") || "gitdebt";

  if (segments[0] === "vs" && segments.length >= 5) {
    const left = `${segments[1]}/${segments[2]}`;
    const right = `${segments[3]}/${segments[4]}`;
    return {
      kind: "comparison",
      mode: "contour",
      seed,
      eyebrow: "comparison signal // dual contour",
      question: `What should you compare between ${left} and ${right}?`,
      answer:
        "Compare the shape of star growth alongside maintenance cadence, contributor concentration, language footprint, file change frequency, and code signals. Popularity shows reach; repository-health evidence explains the maintenance context behind it.",
      labels: [left, right],
      values: segmentValues(segments.slice(1), seed),
    };
  }

  if (segments[0] === "compare") {
    const category = segments[1]?.replaceAll("-", " ");
    return {
      kind: category ? "category" : "comparison",
      mode: "contour",
      seed,
      eyebrow: "comparison signal // contour map",
      question: category
        ? `How should ${category} repositories be compared?`
        : "How should two GitHub repositories be compared?",
      answer:
        "Start with star-history shape, then test whether commit cadence, ownership depth, file change frequency, and dominant languages support the same conclusion. gitdebt keeps attention signals and maintenance evidence visible together.",
      labels: category ? [category, "maintenance evidence"] : ["attention", "maintenance"],
      values: segmentValues(segments, seed),
    };
  }

  if (
    segments.length === 2 &&
    !RESERVED.has(segments[0]) &&
    !RESERVED.has(segments[1])
  ) {
    const repository = `${segments[0]}/${segments[1]}`;
    return {
      kind: "repository",
      mode: "mosaic",
      seed,
      eyebrow: "repository signal // neon mosaic",
      question: `What does the ${repository} report measure?`,
      answer: `The report combines ${repository}'s cumulative GitHub star history with commit activity, contributor concentration, file change frequency, and code signals. Use the chart for attention over time and the maintenance panels to locate where future work is likely to concentrate.`,
      labels: [repository, "stars + code health"],
      values: segmentValues(segments, seed),
    };
  }

  if (segments.length === 1 && !RESERVED.has(segments[0])) {
    const login = segments[0];
    return {
      kind: "profile",
      mode: "braille",
      seed,
      eyebrow: "profile signal // braille activity",
      question: `What is included in @${login}'s GitHub profile report?`,
      answer: `The report aggregates star growth and public repository maintenance signals for @${login}. It highlights contribution footprint, languages, commit activity and README-ready assets without collecting stargazer identities.`,
      labels: [`@${login}`, "aggregate activity"],
      values: segmentValues(segments, seed),
    };
  }

  return {
    kind: "general",
    mode: "mosaic",
    seed,
    eyebrow: "gitdebt signal // live mosaic",
    question: "What can gitdebt tell you about a GitHub repository?",
    answer:
      "gitdebt charts star history and pairs it with repository-health evidence: maintenance cadence, file change frequency, change hotspots, and fix-labelled changes. Reports are designed for investigation first and README embedding second.",
    labels: ["star history", "repository health"],
    values: segmentValues(segments.length > 0 ? segments : ["gitdebt"], seed),
  };
}
