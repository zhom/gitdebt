import { CAPTION, EYEBROW, KPI, PANEL, TITLE } from "@/components/style-tokens";
import { cn } from "@/lib/utils";

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
      <header>
        <h1 className={TITLE}>
          {left.slug} <span className="text-muted-foreground">vs</span> {right.slug}
        </h1>
      </header>
      <div className="grid gap-3 sm:grid-cols-2">
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
    <div className={cn(PANEL, "p-3.5")}>
      <p className={cn(EYEBROW, "truncate")}>{summary.slug}</p>
      <p className={cn(KPI, "mt-3 text-[28px]")}>
        {summary.totalStars.toLocaleString()}
      </p>
      <p className={cn(CAPTION, "mt-2")}>{subtitle}</p>
    </div>
  );
}
