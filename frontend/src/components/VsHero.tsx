export type RepoSummary = {
  slug: string;
  totalStars: number;
  firstStarYear: string | null;
};

type Props = {
  left: RepoSummary;
  right: RepoSummary;
};

export function VsHero({ left, right }: Props) {
  return (
    <section className="space-y-6">
      <header className="space-y-2">
        <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
          Star history · head to head
        </p>
        <h1 className="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
          {left.slug} <span className="text-muted-foreground">vs</span> {right.slug}
        </h1>
      </header>
      <div className="grid gap-6 sm:grid-cols-2">
        <RepoCard summary={left} />
        <RepoCard summary={right} />
      </div>
    </section>
  );
}

function RepoCard({ summary }: { summary: RepoSummary }) {
  const subtitle = summary.firstStarYear
    ? `${summary.totalStars.toLocaleString()} stars · since ${summary.firstStarYear}`
    : `${summary.totalStars.toLocaleString()} stars`;
  return (
    <div className="card-panel relative overflow-hidden p-6">
      <div
        className="absolute inset-x-0 top-0 h-px bg-linear-to-r from-transparent via-signal to-transparent"
        aria-hidden="true"
      />
      <p className="truncate font-mono text-xs tracking-wide text-muted-foreground uppercase">
        {summary.slug}
      </p>
      <div className="mt-3">
        <span className="inline-block text-4xl font-semibold tracking-tight tabular-nums sm:text-5xl">
          {summary.totalStars.toLocaleString()}
        </span>
      </div>
      <p className="mt-1 text-base text-muted-foreground sm:text-sm">{subtitle}</p>
    </div>
  );
}
