import { useEffect, useMemo, useState } from "react";

import { ComparisonSheet } from "@/components/ComparisonSheet";
import {
  RepoComparisonMatrix,
  type ComparisonInitialRepo,
} from "@/components/RepoComparisonMatrix";
import {
  BODY,
  CAPTION,
  FIELD,
  HEADING,
  MEASURE,
  SECTION_ACTION,
} from "@/components/style-tokens";
import { VsHero } from "@/components/VsHero";
import { publishLiveSubject } from "@/lib/live-subject";
import { restoreServedTitle } from "@/lib/live-title";
import { cn } from "@/lib/utils";

type AnalyzeResponse = {
  repo: string;
  total_stars: number;
  created_at: string | null;
  pending?: boolean;
  backfilling?: boolean;
  not_found?: boolean;
  history: { date: string; stars: number }[];
};

type Props = {
  apiBase: string;
  canonical: string;
  initialLeft: AnalyzeResponse | null;
  initialRight: AnalyzeResponse | null;
  overlayPath: string;
  slug1: string;
  slug2: string;
};

function firstStarYear(data: AnalyzeResponse): string | null {
  const first = data.history[0]?.date;
  if (!first) return null;
  const date = new Date(first);
  return Number.isNaN(date.getTime()) ? null : String(date.getUTCFullYear());
}

function settled(data: AnalyzeResponse | null): boolean {
  return Boolean(data && !data.not_found && !data.pending && !data.backfilling);
}

/** The pathname the comparison would live at, taken from its own canonical. */
function canonicalPath(canonical: string): string | undefined {
  try {
    return new URL(canonical).pathname;
  } catch {
    return undefined;
  }
}

export function LiveVsComparison({
  apiBase,
  canonical,
  initialLeft,
  initialRight,
  overlayPath,
  slug1,
  slug2,
}: Props) {
  const [left, setLeft] = useState(initialLeft);
  const [right, setRight] = useState(initialRight);
  const [unavailable, setUnavailable] = useState(false);

  useEffect(() => {
    if (settled(initialLeft) && settled(initialRight)) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;

    async function read(slug: string): Promise<AnalyzeResponse | null> {
      const response = await fetch(`${apiBase}/api/repos/${slug}/analyze`, {
        cache: "no-store",
        credentials: "omit",
        headers: { accept: "application/json" },
        signal: AbortSignal.timeout(8_000),
      });
      if (response.status === 404) {
        return {
          repo: slug,
          total_stars: 0,
          created_at: null,
          not_found: true,
          history: [],
        };
      }
      if (!response.ok) return null;
      return (await response.json()) as AnalyzeResponse;
    }

    async function poll() {
      const [nextLeft, nextRight] = await Promise.allSettled([
        read(slug1),
        read(slug2),
      ]);
      if (cancelled) return;
      const leftValue = nextLeft.status === "fulfilled" ? nextLeft.value : null;
      const rightValue = nextRight.status === "fulfilled" ? nextRight.value : null;
      if (leftValue) setLeft(leftValue);
      if (rightValue) setRight(rightValue);
      if (leftValue?.not_found || rightValue?.not_found) {
        setUnavailable(true);
        return;
      }
      if (settled(leftValue ?? left) && settled(rightValue ?? right)) return;
      attempt += 1;
      const delay = Math.min(15_000, 1_500 * 2 ** Math.min(attempt, 3));
      timer = setTimeout(() => void poll(), delay);
    }

    void poll();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [apiBase, initialLeft, initialRight, slug1, slug2]);

  const heroLeft = useMemo(
    () =>
      left && !left.not_found
        ? {
            slug: slug1,
            totalStars: left.total_stars,
            firstStarYear: firstStarYear(left),
          }
        : null,
    [left, slug1],
  );
  const heroRight = useMemo(
    () =>
      right && !right.not_found
        ? {
            slug: slug2,
            totalStars: right.total_stars,
            firstStarYear: firstStarYear(right),
          }
        : null,
    [right, slug2],
  );

  /*
   * The tab, once both sides are real.
   *
   * `publishLiveSubject` defers to the served canonical, so on the prerendered
   * `/vs/a/b/c/d` page — the only route that mounts this island today, and one
   * whose title the build already wrote for these exact two repositories — this
   * is deliberately a no-op. It fires when the island is mounted anywhere the
   * server could not name the pair, and it un-does itself the moment a side
   * turns out not to be public: a tab that still promises a comparison of a
   * repository nobody can read is the same defect pointed the other way.
   */
  useEffect(() => {
    if (unavailable) {
      restoreServedTitle();
      return;
    }
    if (!heroLeft || !heroRight) return;
    publishLiveSubject({
      subject: `${slug1} vs ${slug2}`,
      description: `${slug1} and ${slug2} compared: star growth on shared axes, plus commit cadence, ownership concentration and codebase size for each.`,
      path: canonicalPath(canonical),
      image: `${apiBase}/api/og.png?repos=${encodeURIComponent(`${slug1},${slug2}`)}`,
    });
  }, [apiBase, canonical, heroLeft, heroRight, slug1, slug2, unavailable]);

  const ready = Boolean(left?.history.length && right?.history.length);
  const initializing = Boolean(
    left?.pending || left?.backfilling || right?.pending || right?.backfilling,
  );
  const comparisonInitial = useMemo<ComparisonInitialRepo[]>(
    () => [
      ...(left
        ? [
            {
              slug: slug1,
              total_stars: left.total_stars,
              created_at: left.created_at,
              history: left.history,
              pending: left.pending,
              backfilling: left.backfilling,
            },
          ]
        : []),
      ...(right
        ? [
            {
              slug: slug2,
              total_stars: right.total_stars,
              created_at: right.created_at,
              history: right.history,
              pending: right.pending,
              backfilling: right.backfilling,
            },
          ]
        : []),
    ],
    [left, right, slug1, slug2],
  );

  if (unavailable) {
    return (
      <section className="mt-10 border border-rule-strong bg-paper p-6" role="alert">
        <p className={FIELD}>Comparison unavailable</p>
        <h1 className={cn(HEADING, "mt-3")}>
          One of these repositories is not public
        </h1>
        <p className={cn(BODY, MEASURE, "mt-3")}>
          GitHub did not expose {slug1} or {slug2} as a public repository. Check
          both names, or open them on GitHub if you have private access. gitdebt
          never ingests private repository data.
        </p>
      </section>
    );
  }

  if (!heroLeft || !heroRight) {
    return (
      <section
        className="mt-10 border border-rule-strong bg-paper p-6"
        aria-live="polite"
      >
        <p className={FIELD}>Reading both repositories</p>
        <h1 className={cn(HEADING, "mt-3")}>The comparison is being drawn</h1>
        <p className={cn(BODY, MEASURE, "mt-3")}>
          Public metadata and the two star series are being read now. This sheet
          fills in as each one lands; nothing here waits on you.
        </p>
      </section>
    );
  }

  return (
    <>
      <div className="mt-10">
        <VsHero left={heroLeft} right={heroRight} />
      </div>

      {ready ? (
        <div className="mt-14">
          <ComparisonSheet
            apiBase={apiBase}
            path={overlayPath}
            caption="Star history overlay"
            embedLink={canonical}
            label={`${slug1} vs ${slug2}`}
            series={[
              { slug: slug1, points: left?.history ?? [] },
              { slug: slug2, points: right?.history ?? [] },
            ]}
          />
        </div>
      ) : (
        <section
          className="mt-14 border border-rule-strong bg-paper p-6"
          aria-live="polite"
        >
          <p className={FIELD}>Overlay pending</p>
          <p className={cn(BODY, MEASURE, "mt-3")}>
            {initializing
              ? "Both current totals are above. The two curves are drawn as soon as the durable jobs recording their star events finish."
              : "An overlay needs recorded star events on both sides. The current totals above stand on their own until then."}
          </p>
        </section>
      )}

      <div className="mt-10 flex justify-end">
        <a
          href={`/compare?repos=${encodeURIComponent(`${slug1},${slug2}`)}`}
          className={SECTION_ACTION}
        >
          add a third repository
        </a>
      </div>

      <div className="mt-14">
        <RepoComparisonMatrix
          apiBase={apiBase}
          repos={[slug1, slug2]}
          initial={comparisonInitial}
        />
      </div>

      <p className={cn(CAPTION, MEASURE, "mt-4")}>
        Both curves are drawn on shared axes. They are not drawn from a shared
        source: each series names its own below.
      </p>
    </>
  );
}
