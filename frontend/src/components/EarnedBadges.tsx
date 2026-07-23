import { useCallback, useEffect, useState } from "react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";

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
            className="dither-panel h-24 rounded-xl motion-safe:animate-pulse"
            aria-hidden="true"
          />
        ))}
      </div>
    );
  }

  if (failed) {
    return (
      <p className="border-y border-border py-4 text-sm text-muted-foreground">
        Badges are temporarily unavailable.
      </p>
    );
  }

  if (earned.length === 0) {
    const pending = badges?.some((badge) => badge.pending);
    return (
      <p className="border-y border-border py-4 text-sm leading-relaxed text-muted-foreground">
        {pending
          ? "Badges are computed when analysis finishes. This section updates automatically."
          : "No badge earned yet. Badges require measured maintenance, distributed ownership, or recent star momentum."}
      </p>
    );
  }

  return (
    <div className="grid border-y border-border sm:grid-cols-3">
      {earned.map((badge) => {
        const chartPath = `/api/repos/${owner}/${repo}/badge.svg?signal=${badge.id}`;
        const alt = `${slug}: ${badge.label}, ${badge.detail}`;
        return (
          <figure key={badge.id} className="relative border-b border-border last:border-b-0 sm:border-r sm:border-b-0 sm:last:border-r-0">
            <figcaption className="flex min-h-16 items-center justify-between gap-2 px-3 py-2">
              <div>
                <p className="font-mono text-xs tracking-wide text-foreground uppercase">
                  {badge.label}
                </p>
                <p className="mt-0.5 text-xs text-muted-foreground">{badge.detail}</p>
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
            <div className="dither-badge-bed flex min-h-16 items-center justify-center border-t border-border px-3 py-3">
              <img
                src={`${apiBase}${chartPath}&theme=${theme}&animate=1&render=${MEDIA_RENDER_REVISION}`}
                alt={alt}
                loading="lazy"
                decoding="async"
                className="block h-auto max-w-full"
              />
            </div>
          </figure>
        );
      })}
    </div>
  );
}
