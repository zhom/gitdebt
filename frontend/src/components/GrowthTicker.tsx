import { useEffect, useMemo, useRef, useState } from "react";
import { Star } from "lucide-react";

import { ROW } from "@/components/style-tokens";
import { metricsFor, naturalWidth } from "@/lib/pretext";
import { cn } from "@/lib/utils";

type Repo = {
  repo: string;
  stars: number;
  gained_7d: number;
  gained_30d: number;
};

function compact(value: number): string {
  return new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

/** Marquee speed. Constant px/s regardless of how wide the content is. */
const SCROLL_PX_PER_SEC = 42;

/**
 * Fixed chrome per ticker item, in rem, summed from the item's own utilities:
 * `px-3.5`·2 + `gap-4`·3 (four spans) + `mx-1`·2 + the `size-3` star + its
 * `gap-1`. pretext measures the *variable* text; this covers the rest so we
 * never have to touch the DOM to size an item.
 */
const ITEM_CHROME_REM = 0.875 * 2 + 1 * 3 + 0.25 * 2 + 0.75 + 0.25; // 6.25rem

export function GrowthTicker({ apiBase }: { apiBase: string }) {
  const [repos, setRepos] = useState<Repo[]>([]);
  const containerRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLSpanElement>(null);
  const [track, setTrack] = useState<{ copies: number; duration: number } | null>(
    null,
  );

  useEffect(() => {
    let active = true;
    fetch(`${apiBase}/api/activity.json`, {
      headers: { accept: "application/json" },
    })
      .then((response) => (response.ok ? response.json() : null))
      .then((body: { repos?: Repo[] } | null) => {
        if (active && Array.isArray(body?.repos)) setRepos(body.repos);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [apiBase]);

  // The visible strings of every item, computed once per data set.
  const itemTexts = useMemo(
    () =>
      repos.map((repo) => [
        repo.repo,
        compact(repo.stars),
        `+${compact(repo.gained_7d)} / 7d`,
        `+${compact(repo.gained_30d)} / 30d`,
      ]),
    [repos],
  );

  // Measure one copy of the strip with pretext (no per-item layout reflow),
  // then repeat it enough times that half the track always fills the viewport
  // — so the `translateX(-50%)` loop is seamless and gap-free — and set a
  // width-proportional duration for constant scroll speed.
  useEffect(() => {
    if (repos.length === 0) return;
    const container = containerRef.current;
    const measure = measureRef.current;
    if (!container || !measure) return;

    const compute = () => {
      const { font, letterSpacing } = metricsFor(measure);
      const rem =
        parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
      const chrome = ITEM_CHROME_REM * rem;
      let copyWidth = 0;
      for (const parts of itemTexts) {
        let itemWidth = chrome;
        for (const part of parts) itemWidth += naturalWidth(part, font, letterSpacing);
        copyWidth += itemWidth;
      }
      if (copyWidth <= 0) return;
      const viewport = container.clientWidth || copyWidth;
      // `copies/2` copies must be at least a viewport wide; keep `copies` even
      // so translating exactly half the track lands on an identical frame.
      let copies = Math.ceil((2 * viewport) / copyWidth);
      copies = Math.max(2, copies % 2 === 0 ? copies : copies + 1);
      const shiftPx = (copyWidth * copies) / 2;
      setTrack({ copies, duration: Math.max(12, shiftPx / SCROLL_PX_PER_SEC) });
    };

    compute();
    const observer = new ResizeObserver(compute);
    observer.observe(container);
    return () => observer.disconnect();
  }, [repos, itemTexts]);

  if (repos.length === 0)
    return <div className="h-16 border-t border-border/60" aria-hidden="true" />;

  const copies = track?.copies ?? 2;

  return (
    <div
      ref={containerRef}
      className="group relative isolate overflow-hidden border-t border-border/60"
      aria-label="Repository growth ticker"
    >
      {/* Off-screen probe: lets pretext read the exact item font off computed
          style rather than a hard-coded stack that could drift from the CSS. */}
      <span
        ref={measureRef}
        aria-hidden="true"
        className="pointer-events-none absolute -z-10 font-mono text-[12px] opacity-0"
      />
      <div
        className="growth-ticker-track flex w-max py-2"
        style={
          track ? { animationDuration: `${track.duration}s` } : undefined
        }
      >
        {Array.from({ length: copies }, (_, copy) =>
          repos.map((repo, index) => (
            <a
              key={`${copy}-${repo.repo}-${index}`}
              href={`/${repo.repo}`}
              aria-hidden={copy > 0 ? "true" : undefined}
              tabIndex={copy > 0 ? -1 : undefined}
              className={cn(ROW, "mx-1 shrink-0 gap-4 px-3.5")}
            >
              <span className="text-foreground">{repo.repo}</span>
              <span className="inline-flex items-center gap-1 tabular-nums">
                {compact(repo.stars)} <Star className="size-3" aria-hidden="true" />
              </span>
              <span className="text-foreground tabular-nums">+{compact(repo.gained_7d)} / 7d</span>
              <span className="tabular-nums">+{compact(repo.gained_30d)} / 30d</span>
            </a>
          )),
        )}
      </div>
    </div>
  );
}
