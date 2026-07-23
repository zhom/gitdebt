import { useCallback, useEffect, useState } from "react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { BODY, EYEBROW, PANEL } from "@/components/style-tokens";
import { DitherSurface } from "@/components/ui/dither-surface";
import { INK } from "@/lib/dither";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";
import { cn } from "@/lib/utils";

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

  if (!badges && !failed) {
    return (
      <div className="grid gap-4 sm:grid-cols-3" aria-label="Checking earned badges">
        {[0, 1, 2].map((key) => (
          <div
            key={key}
            className={cn(PANEL, "h-24 motion-safe:animate-pulse")}
            aria-hidden="true"
          />
        ))}
      </div>
    );
  }

  if (failed) {
    return (
      <p className={BODY}>Badges are temporarily unavailable.</p>
    );
  }

  if (earned.length === 0) {
    const pending = badges?.some((badge) => badge.pending);
    return (
      <p className={BODY}>
        {pending
          ? "Badges are computed when analysis finishes. This section updates automatically."
          : "No badge earned yet. Badges require measured maintenance, distributed ownership, or recent star momentum."}
      </p>
    );
  }

  return (
    <div className="grid gap-3 sm:grid-cols-3">
      {earned.map((badge) => {
        const chartPath = `/api/repos/${owner}/${repo}/badge.svg?signal=${badge.id}`;
        const alt = `${slug}: ${badge.label}, ${badge.detail}`;
        return (
          <figure key={badge.id} className={cn(PANEL, "relative overflow-hidden")}>
            <figcaption className="flex items-center justify-between gap-2 p-3.5">
              <div className="min-w-0">
                <p className={EYEBROW}>{badge.label}</p>
                <p className="mt-1 text-[11px] text-muted-foreground">{badge.detail}</p>
              </div>
              <EmbedSnippet
                apiBase={apiBase}
                chartPath={chartPath}
                linkHref={embedLink}
                label={slug}
                altText={alt}
                variant="menu"
              />
            </figcaption>
            <div className="dither-fallback relative isolate flex min-h-16 items-center justify-center overflow-hidden border-t border-border/40 px-3 py-4">
              <DitherSurface fill={INK} variant="gradient" edge={0.5} alpha={0.16} />
              <img
                src={`${apiBase}${chartPath}&theme=${theme}&animate=1&render=${MEDIA_RENDER_REVISION}`}
                alt={alt}
                loading="lazy"
                decoding="async"
                className="relative block h-auto max-w-full"
              />
            </div>
          </figure>
        );
      })}
    </div>
  );
}
