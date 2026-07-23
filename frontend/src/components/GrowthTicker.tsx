import { useEffect, useState } from "react";
import { Star } from "lucide-react";

import { ROW } from "@/components/style-tokens";
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

export function GrowthTicker({ apiBase }: { apiBase: string }) {
  const [repos, setRepos] = useState<Repo[]>([]);

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

  if (repos.length === 0)
    return <div className="h-16 border-t border-border/60" aria-hidden="true" />;
  const repeated = [...repos, ...repos];

  return (
    <div
      className="group relative isolate overflow-hidden border-t border-border/60"
      aria-label="Repository growth ticker"
    >
      <div className="growth-ticker-track flex w-max py-2">
        {repeated.map((repo, index) => (
          <a
            key={`${repo.repo}-${index}`}
            href={`/${repo.repo}`}
            className={cn(ROW, "mx-1 shrink-0 gap-4 px-3.5")}
          >
            <span className="text-foreground">{repo.repo}</span>
            <span className="inline-flex items-center gap-1 tabular-nums">
              {compact(repo.stars)} <Star className="size-3" aria-hidden="true" />
            </span>
            <span className="text-foreground tabular-nums">+{compact(repo.gained_7d)} / 7d</span>
            <span className="tabular-nums">+{compact(repo.gained_30d)} / 30d</span>
          </a>
        ))}
      </div>
    </div>
  );
}
