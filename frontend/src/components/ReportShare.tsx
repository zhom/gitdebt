import { ChevronDown, Share2 } from "lucide-react";

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
    <section id="share" className="scroll-mt-24" aria-labelledby="share-title">
      <details className="group rounded-xl border border-border bg-card p-2">
        <summary className="flex min-h-16 cursor-pointer list-none items-center justify-between gap-4 rounded-lg bg-primary px-4 py-3 text-primary-foreground transition-colors duration-150 hover:bg-primary/90 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring active:bg-primary/85 [&::-webkit-details-marker]:hidden sm:px-5">
          <div className="flex min-w-0 items-center gap-3">
            <span className="grid size-9 shrink-0 place-items-center rounded-full border border-white/20 bg-white/10">
              <Share2 className="size-4" strokeWidth={1.9} aria-hidden="true" />
            </span>
            <span className="min-w-0 text-left">
              <span
                id="share-title"
                className="block text-sm font-semibold sm:text-base"
              >
                Share &amp; embed
              </span>
              <span className="block truncate text-xs text-white/70 sm:text-sm">
                Copy the report, chart, or badge for your README.
              </span>
            </span>
          </div>
          <ChevronDown
            className="size-4 shrink-0 transition-transform duration-150 group-open:rotate-180 motion-reduce:transition-none"
            strokeWidth={2}
            aria-hidden="true"
          />
        </summary>
        <div className="grid gap-3 px-1 pt-3 pb-1 sm:grid-cols-3">
          <ShareOption
            title="Report link"
            description="Send the full interactive analysis."
            value={pageUrl}
            label="Copy report link"
            primary
          />
          <ShareOption
            title="Star-history chart"
            description="Responsive light and dark README embed."
            value={chartEmbed}
            label="Copy README embed"
          />
          <ShareOption
            title="Compact stats badge"
            description="Stars and forks without the full studio."
            value={badgeEmbed}
            label="Copy badge embed"
          />
        </div>
      </details>
    </section>
  );
}

function ShareOption({
  title,
  description,
  value,
  label,
  primary = false,
}: {
  title: string;
  description: string;
  value: string;
  label: string;
  primary?: boolean;
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
        className={`mt-auto inline-flex min-h-11 items-center rounded-md px-3 py-2 text-sm font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring ${
          primary
            ? "bg-primary text-primary-foreground hover:bg-primary/90"
            : "border border-border bg-background hover:bg-accent"
        }`}
      />
    </div>
  );
}
