import { CopyButton } from "@/components/CopyButton";

type Props = {
  apiBase: string;
  owner: string;
  repo: string;
  pageUrl?: string;
};

export function ReportShare({
  apiBase,
  owner,
  repo,
  pageUrl = `https://gitdebt.com/${owner}/${repo}`,
}: Props) {
  const slug = `${owner}/${repo}`;
  const attributedPage = `${pageUrl}?ref=readme`;
  const chartBase = `${apiBase}/api/repos/${slug}/chart.svg?animate=0`;
  const chartEmbed = `<a href="${attributedPage}">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="${chartBase}&theme=dark" />
    <img alt="${slug} star history" src="${chartBase}&theme=light" />
  </picture>
</a>`;
  const badgeBase = `${apiBase}/api/repos/${slug}/badge.svg?metrics=stars,forks&style=modern&animate=0`;
  const badgeEmbed = `<a href="${attributedPage}">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="${badgeBase}&theme=dark" />
    <img alt="${slug} repository stats" src="${badgeBase}&theme=light" />
  </picture>
</a>`;

  return (
    <details className="group border-y border-border">
      <summary className="flex min-h-14 cursor-pointer list-none items-center justify-between gap-4 py-3 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring [&::-webkit-details-marker]:hidden">
        <div className="text-sm">
          <p className="font-medium">Share this report</p>
          <p className="text-muted-foreground">
            Link to the analysis or add a theme-aware chart to a README.
          </p>
        </div>
        <span
          className="font-mono text-lg text-muted-foreground transition-transform duration-150 group-open:rotate-45 motion-reduce:transition-none"
          aria-hidden="true"
        >
          +
        </span>
      </summary>
      <div className="grid gap-3 border-t border-border py-5 sm:grid-cols-3">
        <ShareOption
          title="Report link"
          description="Send the full interactive analysis."
          value={pageUrl}
          label="Copy link"
        />
        <ShareOption
          title="Star-history chart"
          description="Responsive light and dark README embed."
          value={chartEmbed}
          label="Copy HTML"
        />
        <ShareOption
          title="Compact stats badge"
          description="Stars and forks without the full studio."
          value={badgeEmbed}
          label="Copy HTML"
        />
      </div>
    </details>
  );
}

function ShareOption({
  title,
  description,
  value,
  label,
}: {
  title: string;
  description: string;
  value: string;
  label: string;
}) {
  return (
    <div className="flex flex-col items-start gap-3 rounded-lg border border-border bg-background p-4">
      <div className="space-y-1">
        <p className="text-sm font-medium">{title}</p>
        <p className="text-sm leading-relaxed text-muted-foreground">
          {description}
        </p>
      </div>
      <CopyButton
        value={value}
        idleLabel={label}
        ariaLabel={`${label}: ${title}`}
        className="mt-auto inline-flex min-h-11 items-center rounded-md border border-border px-3 py-2 text-sm font-medium hover:bg-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      />
    </div>
  );
}
