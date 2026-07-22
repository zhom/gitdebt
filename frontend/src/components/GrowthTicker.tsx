import { useEffect, useState } from "react";
import { Star } from "lucide-react";

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

  if (repos.length === 0) return <div className="h-[4.5rem] border-t border-border" aria-hidden="true" />;
  const repeated = [...repos, ...repos];

  return (
    <div className="group overflow-hidden border-t border-foreground bg-background" aria-label="Repository growth ticker">
      <div className="growth-ticker-track flex w-max">
        {repeated.map((repo, index) => (
          <a
            key={`${repo.repo}-${index}`}
            href={`/${repo.repo}`}
            className="flex min-h-[4.5rem] shrink-0 items-center gap-4 border-r border-border px-7 font-mono text-xs outline-none transition-colors hover:bg-muted focus-visible:bg-muted focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring motion-reduce:transition-none"
          >
            <span className="font-semibold text-foreground">{repo.repo}</span>
            <span className="inline-flex items-center gap-1 text-muted-foreground">
              {compact(repo.stars)} <Star className="size-3" aria-hidden="true" />
            </span>
            <span className="text-foreground">+{compact(repo.gained_7d)} / 7d</span>
            <span className="text-muted-foreground">+{compact(repo.gained_30d)} / 30d</span>
          </a>
        ))}
      </div>
    </div>
  );
}
