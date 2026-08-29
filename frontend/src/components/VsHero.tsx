import { CAPTION, FIGURE, FIELD, TITLE } from "@/components/style-tokens";
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

/**
 * The head of a comparison sheet: two subjects, and the one figure they are
 * being compared on.
 *
 * The two columns sit on one drawn grid and every row of that grid is the same
 * height in both, whatever the slugs are: the slug line and the figure line are
 * each held to a single line, and the only variable-length string is the last
 * one, where a difference in length cannot push anything out of step. A
 * comparison whose two halves do not line up is unreadable as a comparison.
 */
export function VsHero({ left, right }: Props) {
  return (
    <section aria-labelledby="vs-title">
      <h1 id="vs-title" className={TITLE}>
        {left.slug} <span className="text-ink-3">vs</span> {right.slug}
      </h1>

      <ul
        role="list"
        className="mt-8 grid grid-cols-1 divide-y divide-rule border border-rule-strong bg-paper sm:grid-cols-2 sm:divide-x sm:divide-y-0"
      >
        <RepoReading summary={left} />
        <RepoReading summary={right} />
      </ul>
    </section>
  );
}

function RepoReading({ summary }: { summary: RepoSummary }) {
  const since = summary.firstStarYear
    ? `First recorded star in ${summary.firstStarYear}.`
    : "No first-star date recorded yet.";
  return (
    <li className="min-w-0 p-5">
      <p className="truncate font-mono text-[0.8125rem] text-ink">
        {summary.slug}
      </p>
      <p className={cn(FIELD, "mt-4")}>Stars</p>
      <p className={cn(FIGURE, "mt-1.5 text-ink")}>
        {summary.totalStars.toLocaleString()}
      </p>
      <p className={cn(CAPTION, "mt-2")}>{since}</p>
    </li>
  );
}
