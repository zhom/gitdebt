import { ChevronDown, Share2 } from "lucide-react";

import { CopyButton } from "@/components/CopyButton";
import { CAPTION, HEADING, PANEL } from "@/components/style-tokens";
import { cn } from "@/lib/utils";

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
      <details className="group">
        <summary className="flex min-h-10 cursor-pointer list-none items-center justify-between gap-4 rounded-md px-2.5 outline-none transition-colors duration-150 hover:bg-card/60 focus-visible:ring-2 focus-visible:ring-accent/30 [&::-webkit-details-marker]:hidden">
          <span className="flex min-w-0 items-center gap-2.5">
            <Share2
              className="size-4 shrink-0 text-muted-foreground"
              strokeWidth={1.9}
              aria-hidden="true"
            />
            <span id="share-title" className={HEADING}>
              Share &amp; embed
            </span>
          </span>
          <ChevronDown
            className="size-4 shrink-0 text-muted-foreground transition-transform duration-200 group-open:rotate-180 motion-reduce:transition-none"
            strokeWidth={2}
            aria-hidden="true"
          />
        </summary>
        <div className="grid gap-3 pt-5 sm:grid-cols-3">
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
    <div className={cn(PANEL, "flex flex-col items-start gap-3 p-3.5")}>
      <div className="space-y-1">
        <p className="text-[13px]">{title}</p>
        <p className={CAPTION}>{description}</p>
      </div>
      <CopyButton
        value={value}
        idleLabel={label}
        ariaLabel={`${label}: ${title}`}
        variant={primary ? "primary" : "outline"}
        size="sm"
        className="mt-auto"
      />
    </div>
  );
}
