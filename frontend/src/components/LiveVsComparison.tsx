import { useEffect, useMemo, useState } from "react";

import { ChartViewer } from "@/components/ChartViewer";
import {
  BODY,
  EYEBROW,
  HEADING,
  PANEL_PADDED,
  SECTION_ACTION,
  SECTION_HEADER,
} from "@/components/style-tokens";
import { VsHero } from "@/components/VsHero";

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
  return Boolean(
    data &&
      !data.not_found &&
      !data.pending &&
      !data.backfilling,
  );
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
        return { repo: slug, total_stars: 0, created_at: null, not_found: true, history: [] };
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
  const ready = Boolean(
    left?.history.length && right?.history.length,
  );
  const initializing = Boolean(
    left?.pending || left?.backfilling || right?.pending || right?.backfilling,
  );
  const thClass =
    "py-3 pr-4 font-mono text-[10px] font-medium tracking-[0.14em] text-muted-foreground/70 uppercase whitespace-nowrap";

  if (unavailable) {
    return (
      <section className={`${PANEL_PADDED} mt-8`} role="alert">
        <p className="font-mono text-[10px] tracking-[0.25em] text-[var(--swatch-red)] uppercase">
          Comparison unavailable
        </p>
        <h1 className={`mt-2 ${HEADING}`}>A repository is not public</h1>
        <p className={`mt-2 ${BODY}`}>
          Check both repository names and confirm that they are public.
        </p>
      </section>
    );
  }

  if (!heroLeft || !heroRight) {
    return (
      <section className={`${PANEL_PADDED} mt-8`} aria-live="polite">
        <p className={EYEBROW}>Initializing comparison</p>
        <h1 className={`mt-2 ${HEADING}`}>Loading both repositories</h1>
        <p className={`mt-2 ${BODY}`}>
          This deployment is fetching the public metadata and star history now.
          The comparison updates here automatically.
        </p>
      </section>
    );
  }

  return (
    <>
      <div className="mt-8">
        <VsHero left={heroLeft} right={heroRight} />
      </div>

      {ready ? (
        <div className="mt-12">
          <ChartViewer
            apiBase={apiBase}
            path={overlayPath}
            alt={`Star history overlay of ${slug1} and ${slug2}`}
            caption="Star history overlay"
            embedLink={canonical}
            label={`${slug1} vs ${slug2}`}
          />
        </div>
      ) : (
        <section className={`${PANEL_PADDED} mt-12`} aria-live="polite">
          <p className={EYEBROW}>Current totals ready</p>
          <p className={`mt-2 max-w-[70ch] ${BODY}`}>
            {initializing
              ? "Historical timestamps are still being initialized. This chart appears automatically when both durable jobs complete."
              : "A historical overlay needs recorded star events for both repositories. Current totals remain available above."}
          </p>
        </section>
      )}

      <section className="mt-16 scroll-mt-24 border-t border-border/60 pt-12">
        <div className={SECTION_HEADER}>
          <h2 className={HEADING}>Summary</h2>
          <a
            href={`/compare?repos=${encodeURIComponent(`${slug1},${slug2}`)}`}
            className={SECTION_ACTION}
          >
            add a third repo <span aria-hidden="true">→</span>
          </a>
        </div>
        <div className="-mx-6 mt-6 overflow-x-auto">
          <div className="inline-block min-w-full px-6 py-2 align-middle">
            <table className="w-full text-left text-base sm:text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className={thClass}>Metric</th>
                  <th className={`${thClass} text-right`}>{slug1}</th>
                  <th className={`${thClass} text-right`}>{slug2}</th>
                </tr>
              </thead>
              <tbody className="tabular-nums">
                <tr className="border-b border-border/40">
                  <td className="py-3 pr-4 text-muted-foreground">Total stars</td>
                  <td className="py-3 pr-4 text-right">
                    {heroLeft.totalStars.toLocaleString()}
                  </td>
                  <td className="py-3 text-right">
                    {heroRight.totalStars.toLocaleString()}
                  </td>
                </tr>
                <tr>
                  <td className="py-3 pr-4 text-muted-foreground">First star</td>
                  <td className="py-3 pr-4 text-right text-muted-foreground">
                    {heroLeft.firstStarYear ?? "—"}
                  </td>
                  <td className="py-3 text-right text-muted-foreground">
                    {heroRight.firstStarYear ?? "—"}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>
    </>
  );
}
