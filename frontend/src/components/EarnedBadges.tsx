import { useCallback, useEffect, useState } from "react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { BODY, CAPTION, FIELD, MEASURE } from "@/components/style-tokens";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";
import { cn } from "@/lib/utils";

/**
 * The badges this repository has actually earned, each shown as the asset a
 * README would carry.
 *
 * The badge itself is a server-rendered image, so what is on this page is the
 * same bytes a reader would see embedded — never a re-drawn approximation of
 * it. The image sits on the paper with a drawn edge and nothing behind it: no
 * tinted tile, no field, no texture. A mark in a coloured box is the component
 * kit's idea of a badge, not a drawing's.
 */

type EarnedBadge = {
  id: "active" | "community" | "momentum";
  label: string;
  detail: string;
  earned: boolean;
  pending: boolean;
};

export function EarnedBadges({
  owner,
  repo,
  apiBase,
  embedLink,
}: {
  owner: string;
  repo: string;
  apiBase: string;
  embedLink: string;
}) {
  const slug = `${owner}/${repo}`;
  const [badges, setBadges] = useState<EarnedBadge[] | null>(null);
  const [failed, setFailed] = useState(false);
  const theme = useRenderedTheme();

  const load = useCallback(async () => {
    try {
      const response = await fetch(
        `${apiBase}/api/repos/${owner}/${repo}/earned-badges.json`,
        { cache: "no-store", credentials: "omit" },
      );
      if (!response.ok) throw new Error("badge evidence unavailable");
      setBadges((await response.json()) as EarnedBadge[]);
      setFailed(false);
    } catch {
      setFailed(true);
    }
  }, [apiBase, owner, repo]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    function refresh(event: Event) {
      const detail = (event as CustomEvent<{
        repo?: string;
        stars?: { phase?: string };
        analysis?: { phase?: string };
      }>).detail;
      if (detail?.repo?.toLowerCase() !== slug.toLowerCase()) return;
      if (
        detail.stars?.phase === "complete" ||
        detail.analysis?.phase === "complete"
      ) {
        void load();
      }
    }
    window.addEventListener("gitdebt:repo-progress", refresh);
    return () => window.removeEventListener("gitdebt:repo-progress", refresh);
  }, [load, slug]);

  const earned = badges?.filter((badge) => badge.earned) ?? [];

  // Every waiting state says what it is waiting for. A row of pulsing grey
  // rectangles says nothing, and it says it in a way that reads as content
  // that failed to arrive.
  if (!badges && !failed) {
    return (
      <p className={cn(BODY, MEASURE)}>
        Reading badge evidence for {slug}.
      </p>
    );
  }

  if (failed) {
    return (
      <p className={cn(BODY, MEASURE)}>Badges are temporarily unavailable.</p>
    );
  }

  if (earned.length === 0) {
    const pending = badges?.some((badge) => badge.pending);
    return (
      <p className={cn(BODY, MEASURE)}>
        {pending
          ? "Badges are computed when analysis finishes. This section updates on its own."
          : "No badge earned yet. Badges require measured maintenance, distributed ownership, or recent star momentum."}
      </p>
    );
  }

  return (
    <ul role="list" className="grid gap-4 sm:grid-cols-3">
      {earned.map((badge) => {
        const chartPath = `/api/repos/${owner}/${repo}/badge.svg?signal=${badge.id}`;
        const alt = `${slug}: ${badge.label}, ${badge.detail}`;
        return (
          <li key={badge.id} className="h-full">
            {/* Equal-height cells on one grid: the badge image is anchored to
                the bottom of every card, so a longer detail line in one column
                cannot push its neighbour's image out of step. */}
            <figure className="m-0 grid h-full cut-edge grid-rows-[auto_1fr_auto] p-4 [--pad-x:1rem] [--pad-y:1rem]">
              <figcaption className="flex items-start justify-between gap-3">
                <h3 className={FIELD}>{badge.label}</h3>
                <EmbedSnippet
                  apiBase={apiBase}
                  chartPath={chartPath}
                  linkHref={embedLink}
                  label={slug}
                  altText={alt}
                  variant="menu"
                />
              </figcaption>
              <p className={cn(CAPTION, "mt-2")}>{badge.detail}</p>
              <img
                src={`${apiBase}${chartPath}&theme=${theme}&animate=1&render=${MEDIA_RENDER_REVISION}`}
                alt={alt}
                loading="lazy"
                decoding="async"
                className="mt-4 block h-auto max-w-full self-end justify-self-start"
              />
            </figure>
          </li>
        );
      })}
    </ul>
  );
}
