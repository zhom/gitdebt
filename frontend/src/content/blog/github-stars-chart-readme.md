---
description: Want a github stars chart readme embed? Paste a public image URL in a picture element with a dark mode source. Compare three embed methods and read the curve.
faq:
  - answer: "To add a GitHub stars chart to your README, paste an html snippet that wraps a plain image URL in a link. Use a picture element with a prefers-color-scheme source for dark mode and an img fallback for light. Copy it from gitdebt's README embeds page and use your own owner/repo slug."
    question: How do I add a GitHub stars chart to my README?
  - answer: "You don't need a token, workflow or build step for a star history chart when it's a plain image URL. Approaches that commit a generated file do use a workflow, which trades setup effort for a chart stored in your repository."
    question: Do I need a token or GitHub Action for a star history chart?
  - answer: "Showing a different chart in dark mode takes one picture element. Put the theme=dark URL in a source tag with media=\"(prefers-color-scheme: dark)\", and put the theme=light URL in the img tag inside the picture element. The viewer's theme setting decides which one loads."
    question: How do I show a different chart in dark mode?
  - answer: Your README chart is most likely not updating because you are seeing a cached copy, since the image proxy can delay updates. A repository nobody has analyzed yet shows a placeholder first, and the real chart replaces it at the same URL.
    question: Why is my README chart not updating?
  - answer: "A star chart can't tell real stars from fake ones, because it plots only star dates and counts. gitdebt does not flag stars as fake, real or suspicious, and it stores star dates and day counts without any stargazer profiles."
    question: Can a star chart show fake stars?
publishDate: "2026-11-03"
tags:
  - github readme
  - star history
  - dark mode
  - image embeds
  - repository health
title: "GitHub stars chart readme: Add a light and dark mode embed"
---

**A GitHub stars chart README embed is a plain public image URL pasted into the README, wrapped in a `<picture>` element with a `prefers-color-scheme` source so it switches between light and dark. It needs no token, workflow or iframe. The curve shows cumulative stars, so it reads as popularity, and commit history is the place to check repository health.**

Visitors to a README with no growth picture can't tell whether a project is gaining ground or standing still. [Some older github stars chart readme recipes](https://dev.to/xuanhun/how-to-add-a-live-star-history-chart-to-your-github-readme-15hk) ask for a personal access token, and others, such as [leereilly/star-chart](https://github.com/leereilly/star-chart), commit a generated chart on a schedule. A plain image URL needs neither.

If you maintain a public repository, you can embed a chart in a few lines, weigh three embed methods by what each costs you, and read the curve without over-reading a spike or a plateau.

> **Key Takeaways**
> - Replace the `owner/repo` slug in every URL of the snippet, or light and dark modes show different repositories.
> - A chart that updates late is usually GitHub's cached copy, and GitHub asks you to purge that cache sparingly.
> - The curve is net of unstars, so a flat stretch can hide stars and unstars cancelling out.
> - Align repositories to days since first star to compare early growth instead of age.

## How do you add a GitHub stars chart to a README?

You add a GitHub stars chart to a README by pasting a plain image URL wrapped in a link and a `<picture>` element. Here is the pattern filled in for `vitejs/vite`, with a `prefers-color-scheme` source for dark mode:

![Illustration for How do you add a GitHub stars chart to a README?](https://api.birchseek.com/m/GCPefcmkDXFeQxn8k56ZL8jUjKuioBNf.webp)

```html
<a href="https://gitdebt.com/vitejs/vite?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/vitejs/vite/chart.svg?theme=dark">
    <img alt="Star history of vitejs/vite" src="https://api.gitdebt.com/api/repos/vitejs/vite/chart.svg?theme=light">
  </picture>
</a>
```

1. Open `README.md` on github.com or in your local clone, and pick the spot for the chart, usually under the project description.
2. Paste the snippet and replace `vitejs/vite` with your slug. It appears in three URLs (the link and both image sources) and in the alt text. If you change only one image URL, light and dark modes show different repositories.
3. Keep the link and its `?ref=readme` parameter, which sends readers who click the chart to the repository's report. Leave the `<img>` inside the `<picture>` element: it is the fallback for any viewer that ignores the `<source>` tag.
4. Choose static or animated. Static SVG is the default. Use `chart.gif` in place of `chart.svg`, or add `animate=1` to an SVG URL to opt into motion.
5. Commit, then load the repository page. Switch GitHub between light and dark appearance to check that the chart follows.

A repository nobody has analyzed yet shows a placeholder on its first visit. gitdebt's [README badges and embeds page](https://gitdebt.com/badges) says such a repository "shows a placeholder and queues the work; the real chart replaces it at the same URL." You don't edit the GitHub stars chart README again. Reload after a short wait and the chart appears.

The same page has a ready-made GitHub stars chart README snippet to copy, so you only swap in your repository.

## Why can't a GitHub stars chart README embed use iframes or scripts?

Iframes and scripts don't work in a GitHub README because GitHub handles every README picture as a file to fetch, proxy and cache, so no code from the chart's host runs on the page. [GitHub's page on anonymized URLs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) says GitHub uses the open-source project Camo, which "generates an anonymous URL proxy for each file."

A GitHub stars chart README embed that arrives as an image fits within that rule. The [`leereilly/star-chart` project](https://github.com/leereilly/star-chart) describes its output as a standalone SVG and says it "does not need a dashboard, third-party image host, or runtime JavaScript."

An image URL also carries no credential. Your browser requests it like any other picture, so nothing secret goes into the README. Anyone who opens the repository page gets the same file.

### Why the chart may update late

The Camo proxy sits between the reader and the chart's host. GitHub Docs describes Camo as hiding "your browser details and related information from other users," and the copy it serves is cached.

After a repository gains stars, the README can therefore show an older curve for a while. GitHub Docs warns that purging the cache "forces every GitHub user to re-request the image, so you should use it very sparingly." For a cumulative star history, a short delay changes the picture very little.

## Workflow file, hosted image URL or iframe: which GitHub stars chart README embed fits?

The README chart embed that fits depends on where the chart goes and who carries the upkeep: a committed file or a hosted image URL works in a README, and an iframe works only on a site you control.

| Criterion | Committed file (workflow) | Hosted image URL | Iframe |
| --- | --- | --- | --- |
| Setup effort | Workflow file to write and maintain | Paste one URL | Page you control |
| Freshness | As of the last run | As of the proxy's last fetch | Live on the page |
| Token need | Write access for the workflow | None for a public URL | None |
| README support | Yes | Yes | No |
| Failure mode | Stale file, or a blocked push | Delayed update | Blank frame under the site's policy |

A committed file gives you full control over the image, and the cost is upkeep. The [star-chart project](https://github.com/leereilly/star-chart) describes this route as rendering a standalone SVG that it "can commit the result on a schedule." Between runs the file can lag the real star count.

If the default branch is protected, the commit has to arrive as a pull request, because [GitHub's branch protection](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches) offers "Require a pull request before merging."

A hosted image URL has nothing to maintain. Because GitHub's Camo proxy serves its own copy of the image, a chart can show an older version for a while after the source changes. A gitdebt embed is this kind of URL, and the [README snippets](https://gitdebt.com/badges) need no account, token, workflow or build step.

An iframe suits a page you own, such as a project site or a docs page. It renders only if that site's content security policy allows frames. A README on GitHub won't display it, so the chart there would be empty.

In short, for a GitHub stars chart README:

- The committed file fits teams that want the chart stored in the repository and don't mind keeping a workflow running.
- The hosted image URL fits a README that should stay current without extra files or steps.
- An iframe fits your own website, when its policy allows frames.

| Where the chart goes | Use |
| --- | --- |
| A GitHub README | Image URL, or committed file if you want it stored in the repo |
| Your own website | Iframe only if the site allows frames; otherwise the image URL |

## Where does GitHub star history data come from now?

GitHub's star history endpoint is where current star data comes from: it returns a public repository's stars grouped by calendar week, newest first, according to [a dev.to write-up on the new endpoint](https://dev.to/yusufcengiz/building-readme-star-charts-on-githubs-new-star-history-endpoint-1k0o). The request is `GET /repos/{owner}/{repo}/stargazers/history`.

Each weekly entry holds a `total` and a `days` list with seven daily counts, so a chart can draw exact daily steps without reading individual stargazers. The endpoint works for any public repository without authentication. Pages hold up to 30 weeks, and you can request up to 100 pages.

Older tools worked from the stargazers listing instead. [GitHub's pagination documentation](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api) says that "when a response from the REST API would include many results, GitHub will paginate the results and return a subset of the results."

A repository with a long history therefore meant many requests per chart, one page at a time. The history endpoint returns the whole series already bucketed by date.

gitdebt reads its star series from this history. Every chart names its source, coverage date and state, so you can see which kind of data you're looking at.

The [methodology page](https://gitdebt.com/about) describes where each series comes from. Because the counts come from GitHub's own star history, the curve stays net of unstars. gitdebt stores star dates and day counts only, never stargazer profiles.

## How do you read a GitHub star history chart, and when can you not trust it?

You read a GitHub star history chart by its slope, because it is a running total: a steep stretch means many people starred in a short time, and a flat stretch means the total held steady.

A flat line is where trust runs out. A star count also falls when someone unstars, so a flat or dipping line can hide stars and unstars cancelling each other out.

Resolution depends on how the source groups its data. A chart that reads only the endpoint's weekly totals can't place a spike within its week, while one that reads each week's `days` list can.

To read any chart:

1. Set a date window around the period you care about.
2. Compare slopes, not heights. A line that's still climbing says more than a high total.
3. Note each vertical step, then check the commit history around that date.
4. Check the repository name. If a series is keyed by `owner/repo`, a rename or transfer can split it into two series or cut it short.

Overlaying repositories on calendar dates shows who was ahead in any given month.

If the totals differ by orders of magnitude, switch to log scale so the smaller lines don't flatten against the axis.

The curated [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks) offers the same views for a ready-made set of repositories.

Fake stars are a documented industry topic. A research paper on arXiv is titled ["4.5 Million (Suspected) Fake Stars in GitHub: A Growing Spiral of Popularity Contests, Scams, and Malware"](https://arxiv.org/abs/2412.13459).

A star chart can't tell real stars from fake ones, because the curve holds counts and dates, not who starred. As [gitdebt's methodology page](https://gitdebt.com/about) puts it, a star chart plots star dates and counts only, and gitdebt does not flag stars as fake, real or suspicious.

## Does a rising star curve mean the repository is maintained?

A rising star curve shows that people are starring a repository, and on its own it says nothing about whether anyone still maintains it. Stars are what the crowd did; commits are what the maintainers did. The two come from independent sources, so a report should show them side by side and keep them out of a single blended score.

The two can disagree in both directions:

- A repository can climb steadily on the strength of an old blog post or a trending list while its last commit is months old.
- Another can sit on a flat curve and still receive commits every week, because its audience stopped growing long ago.

Two commit-based readings address the maintenance question. The maintenance reading looks at recent commit cadence: how recently and how often the code changed. The commit trend shows monthly commit volume, so you can see whether activity is rising, steady or tapering.

Both are directional signals. A commit count says how often the code changed, not how good the changes were. A mature, finished library can also be quiet because there's nothing left to add.

gitdebt report pages put the star history beside [four health readings from the commit history](https://gitdebt.com/): maintenance, ownership, repair load and debt markers. None of them comes from stars.

The [angular/angular report](https://gitdebt.com/angular/angular) shows this layout for a large framework repository. Open it and compare the star curve with the commit trend yourself, since live values change.

## Frequently asked questions

### How do I add a GitHub stars chart to my README?

Paste an HTML snippet that wraps a plain image URL in a link, using a `<picture>` element with a `prefers-color-scheme` source for dark mode and an `<img>` fallback for light. Copy it from [gitdebt's README embeds page](https://gitdebt.com/badges) and use your own owner/repo slug.

### Do I need a token or GitHub Action for a star history chart?

A star history chart needs no token, workflow or build step when it's a plain image URL. Approaches that commit a generated file do use a workflow, which trades setup effort for a chart stored in your repository.

### How do I show a different chart in dark mode?

Use one `<picture>` element. Put the `theme=dark` URL in a `<source media="(prefers-color-scheme: dark)">` tag and the `theme=light` URL in the `<img>` inside it, and the viewer's theme setting decides which one loads.

### Why is my README chart not updating?

A README chart that looks stale is usually a cached copy, since GitHub's image proxy can delay updates. A repository nobody has analyzed yet shows a placeholder first, and the real chart replaces it at the same URL.

### Can a star chart show fake stars?

A star chart can't tell real stars from fake ones, because it plots star dates and counts only. gitdebt does not flag stars as fake, real or suspicious, and it stores star dates and day counts, never stargazer profiles.

## What to do next with your GitHub stars chart README embed

Once the GitHub stars chart README embed is in place, work through three steps over the next few months.

- Today, paste the snippet with your own `owner/repo` in each URL, then open the repository page and check both light and dark appearance.
- This week, read the curve by calendar date, then by days since the first star, and put the commit-based readings from your repository's report beside it.
- This quarter, compare your curve with two or three similar repositories, such as `facebook/react` and `vercel/next.js`, aligned to their first star.

[Analyze your repository for free](https://gitdebt.com/report). gitdebt is MIT-licensed, and the embed needs no account, token or build step, so you can remove the snippet at any time and lose nothing.

## Sources

[How to add a live star history chart to your github readme](https://dev.to/xuanhun/how-to-add-a-live-star-history-chart-to-your-github-readme-15hk) · [README badges, star-history charts and embeds · gitdebt](https://gitdebt.com/badges) · [About anonymized URLs - GitHub Docs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) · [GitHub - leereilly/star-chart](https://github.com/leereilly/star-chart) · [About protected branches - GitHub Docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches) · [Building README star charts on GitHub's new star history…](https://dev.to/yusufcengiz/building-readme-star-charts-on-githubs-new-star-history-endpoint-1k0o) · [Using pagination in the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about). Every other source is linked where the article cites it.
