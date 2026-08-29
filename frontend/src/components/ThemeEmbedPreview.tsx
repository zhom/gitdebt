import { CodeBlock } from "@/components/CodeBlock";
import { CAPTION } from "@/components/style-tokens";
import {
  bestEmbed,
  readmeLink,
  repoEmbedAssets,
} from "@/lib/readme-embeds";

/**
 * One snippet, two drawings — shown as a callout, not as a toggle.
 *
 * The old version put a segmented switch above a single mock README and asked
 * the reader to flip between two states to learn one fact. A drawing does not
 * hide half of itself behind a control: it shows both specimens at once and
 * names each one after the line of markup that selects it, so the snippet below
 * and the two panels above read as one figure with nothing to click.
 *
 * The specimens carry the SAME trace geometry in two inks, because that is the
 * fact being stated: one measurement, published twice so GitHub can pick.
 *
 * Nothing here needs JavaScript. The snippet is composed by `readme-embeds.ts`,
 * the one catalog the API and the /badges page also read, so the markup on this
 * page cannot drift from the markup gitdebt serves.
 */

const SPECIMEN_SLUG = "facebook/react";
const SPECIMEN_API = "https://api.gitdebt.com";
const SPECIMEN_SITE = "https://gitdebt.com";

const CHART = repoEmbedAssets(SPECIMEN_SLUG).find((asset) => asset.id === "chart");
const SNIPPET = CHART
  ? bestEmbed(
      SPECIMEN_API,
      CHART,
      readmeLink(SPECIMEN_SITE, `/${SPECIMEN_SLUG}`),
    )
  : "";

/** The illustrative trace, written once and inked twice. */
const TRACE = [
  [4, 44],
  [40, 41],
  [78, 35],
  [116, 30],
  [154, 21],
  [196, 14],
  [236, 4],
] as const;

const TRACE_D = TRACE.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x} ${y}`).join(
  " ",
);

/** Summed here so the stroke draws to its true end, never short of it. */
const TRACE_LENGTH = Math.ceil(
  TRACE.reduce(
    (total, point, index) =>
      index === 0
        ? 0
        : total +
          Math.hypot(point[0] - TRACE[index - 1][0], point[1] - TRACE[index - 1][1]),
    0,
  ),
);

type SpecimenProps = {
  /** The line of the snippet that selects this asset. */
  selector: string;
  /** The condition under which GitHub renders it. */
  condition: string;
  dark: boolean;
  delay: number;
};

function Specimen({ selector, condition, dark, delay }: SpecimenProps) {
  return (
    <div className={dark ? "bg-ink p-4" : "bg-paper p-4"}>
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        {/* The element name, set exactly as the snippet below sets it. It is a
            code identifier, so it is lettered in the mono face and never
            tracked out into a label — the literal match is the whole point. */}
        <code
          className={
            dark
              ? "font-mono text-[0.8125rem] text-paper"
              : "font-mono text-[0.8125rem] text-ink"
          }
        >
          {selector}
        </code>
        <span
          className={
            dark
              ? "font-mono text-[0.6875rem] text-rule-strong"
              : "font-mono text-[0.6875rem] text-ink-3"
          }
        >
          {condition}
        </span>
      </div>

      <p
        className={
          dark
            ? "mt-5 font-mono text-[0.8125rem] text-rule-strong"
            : "mt-5 font-mono text-[0.8125rem] text-ink-2"
        }
      >
        {SPECIMEN_SLUG}
      </p>

      <svg
        viewBox="0 0 240 56"
        width="100%"
        height="56"
        preserveAspectRatio="none"
        aria-hidden="true"
        focusable="false"
        className="mt-3 block"
        fill="none"
      >
        <path
          d="M4 50H236"
          stroke={dark ? "var(--ink-3)" : "var(--rule-strong)"}
          strokeWidth="1"
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
        />
        <path
          d={TRACE_D}
          stroke={dark ? "var(--paper)" : "var(--ink)"}
          strokeWidth="1.5"
          strokeLinejoin="round"
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
          className="inks-in"
          style={{
            ["--draw-length" as string]: String(TRACE_LENGTH),
            ["--draw-delay" as string]: `${delay}ms`,
          }}
        />
      </svg>
    </div>
  );
}

export function ThemeEmbedPreview() {
  return (
    <figure
      className="border border-rule-strong bg-paper"
      aria-labelledby="theme-preview-caption"
    >
      <figcaption
        id="theme-preview-caption"
        className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 border-b border-rule px-4 py-3"
      >
        <span className="font-draft text-[1.0625rem] leading-[1.2] text-ink">
          One snippet — two drawings
        </span>
        <span className={CAPTION}>
          Illustrative specimens; not a repository's values
        </span>
      </figcaption>

      {/* The two specimens sit on one grid, so their labels, slugs and
          baselines line up across the pair whatever either one contains. */}
      <div className="grid gap-px bg-rule sm:grid-cols-2">
        <Specimen
          selector="<source>"
          condition="prefers-color-scheme: dark"
          dark
          delay={0}
        />
        <Specimen selector="<img>" condition="default" dark={false} delay={140} />
      </div>

      {SNIPPET && (
        <div className="border-t border-rule p-4">
          <CodeBlock
            code={SNIPPET}
            language="html"
            label={`README.md · ${SPECIMEN_SLUG}`}
            copyLabel="Copy"
            copyAriaLabel="Copy the theme-aware star-history snippet"
            maxHeightClass="max-h-44"
          />
        </div>
      )}
    </figure>
  );
}
