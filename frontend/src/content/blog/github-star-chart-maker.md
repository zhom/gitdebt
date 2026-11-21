---
description: Need a free GitHub star chart maker? gitdebt draws your star history from one image URL in your README, with no token or app. Copy the snippet and start.
faq:
  - answer: "gitdebt is a free, MIT-licensed star chart maker that needs no token, sign-in, GitHub App or workflow. Paste one picture snippet with your owner/repo into the README. It uses a plain image URL, and the chart is drawn from GitHub's own star history."
    question: Is there a free GitHub star chart maker that needs no token?
  - answer: "Use a picture element with two sources: one image URL ending in theme=dark for dark viewers and one ending in theme=light for light viewers. The README snippets page has the full block to copy, and each viewer sees the drawing that matches their theme."
    question: How do I show a dark mode star chart in my README?
  - answer: "Nobody has analyzed that repository yet, so the image URL returns a placeholder while the work is queued. The real chart replaces it at the same URL, so you don't edit the README or paste a new link. Reload the page after a short wait."
    question: Why is my new chart a placeholder?
  - answer: "The star chart is net of unstars, so each point on the series is the running total after removals. gitdebt keeps star dates and day counts only, which means it lists no accounts and can't tell you which person removed a star."
    question: Does the star chart show unstars?
publishDate: "2026-11-21"
tags:
  - github stars
  - star history
  - readme
  - repository comparison
  - open source
  - gitdebt
title: "Free GitHub star chart maker: Add one image URL to your README"
---

**gitdebt is a free github star chart maker for public repositories: put one image URL, `https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg`, in your README and it draws the star history. Swap OWNER/REPO for your repository, or copy the ready-made [README snippet](https://gitdebt.com/badges).**

Maintainers who want a truthful star-history chart in a README usually meet setup first: [some tools](https://starhistory.io/) have you install a GitHub App, and [others run a scheduled workflow](https://skillsclaude.org/skills/star-history-chart) on a repository token. A free GitHub star chart maker that works from a plain image URL skips both.

After reading, you can embed a light and dark chart, chart any public repository, overlay several on one set of axes, and read the curve without claiming more than it shows.

> **Key Takeaways**
> - One [README snippet](https://gitdebt.com/badges) carries a light and a dark drawing.
> - Stars show what the crowd did; commit history shows what maintainers did.

## What is a free GitHub star chart maker and what does it chart?

A free GitHub star chart maker turns a public repository's star history into a line chart of cumulative stars over time. gitdebt draws that chart from a plain image URL, with no sign-in, token or workflow.

![Illustration for What is a free GitHub star chart maker and what does it chart?](https://api.birchseek.com/m/96rhkGBh3BHw_Wv_faYxwP6tK1T4pB25.webp)

The stars are GitHub repository bookmarks. [GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) says you can "star repositories and topics to keep track of projects you find interesting and discover related content in your news feed." The number on a repository page is a single total for today, with no dates attached.

A free GitHub star chart maker adds the time axis. It plots the cumulative total for each day, so a launch week, a steady climb or a plateau becomes visible.

gitdebt reads star history for public repositories only.

## How do you add a free GitHub star chart to your README?

You add a free GitHub star chart to your README by pasting one HTML snippet that points at a plain image URL, with `OWNER/REPO` swapped for your repository.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <img alt="Star history chart" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

1. Copy the snippet, or take it from the [README snippets on the badges page](https://gitdebt.com/badges).
2. Replace `OWNER/REPO` in all three places: the link, the dark `srcset` and the light `src`. Skipping one gives a chart for the wrong repository or a broken link.
3. Keep the `?ref=readme` link around the image. It sends a reader who clicks the chart to the full report.
4. Paste the block into `README.md` and commit it. Open the rendered README and switch GitHub between light and dark appearance to confirm each drawing loads.

For `vitejs/vite`, the dark source is `https://api.gitdebt.com/api/repos/vitejs/vite/chart.svg?theme=dark` and the light fallback is `https://api.gitdebt.com/api/repos/vitejs/vite/chart.svg?theme=light`. The link target becomes `https://gitdebt.com/vitejs/vite?ref=readme`. Nothing else in the block changes.

The browser picks the dark source when the viewer's appearance is dark and falls back to the `<img>` otherwise.

### Why it works with no setup

The README holds only an image URL, so it needs no JavaScript and no build step. You don't install a GitHub App or grant write access to your repository.

GitHub's page on [anonymized URLs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) says it uses the open-source project Camo, which "generates an anonymous URL proxy for each file". Your chart reaches readers through that proxy.

The docs also describe purging Camo's cache, noting that it "forces every GitHub user to re-request the image". An updated chart shows up when the proxy refreshes its cached copy.

SVG is static by default, and `animate=1` opts an SVG into motion. The same URL pattern serves other assets: swap `chart.svg` for `chart.gif`, `card.svg` or `badge.svg`.

### What a repository nobody has analyzed shows

If nobody has analyzed the repository yet, the image URL renders a placeholder and queues the work. The real chart replaces the placeholder at the same URL, so the README needs no edit.

## Where does a free GitHub star chart maker get its data, and what are the limits?

The star data comes from GitHub's own star history for a public repository, drawn as cumulative stars per day, so the chart covers exactly what that history covers.

The GitHub Docs page for the REST API's [Starring endpoints](https://docs.github.com/en/rest/activity/starring) describes the list-stargazers endpoint as one that lists the people who have starred the repository. The same page documents a custom media type, `application/vnd.github.star+json`, which includes a timestamp of when the star was created.

Older series built from GitHub's stargazer list or historical data are re-read from GitHub's star history as gitdebt reaches them.

The series has three limits:

- Net of unstars: a dip means more stars were removed than added that day. The series can't say who unstarred, and gitdebt keeps day counts, not accounts.
- Coverage date: the line is current only to the date it was last read. A flat tail on a busy repository may mean a stale read rather than a quiet project.
- Scheduled reads: reads run on a schedule, so the newest days land after the day itself.

Every chart names its source, its coverage date and its state. When a curve looks wrong, check the coverage date first, since a stale date explains a flat ending better than any change in the project does.

## How do you use a free GitHub star chart maker for any repository or several at once?

You make a GitHub star chart for any public repository by entering its `owner/repo` at gitdebt.com/report, and you put several on one set of axes with the compare page.

1. Open the report form. Go to https://gitdebt.com/report and type the slug in `owner/repo` form, for example `vitejs/vite`.
2. Read the report. Your chart lives at `gitdebt.com/owner/repo`. If nobody has analyzed the repository yet, you'll see a placeholder first. The real chart replaces it at the same URL once the work finishes, so reload that address later instead of submitting the form again.
3. Open the comparison page. On [gitdebt compare](https://gitdebt.com/compare), add repositories one by one, up to eight.
4. Pick the axes. Calendar date lines repositories up by when growth happened. Days since first star lines them up by age, the fairer view when one project launched years after another.
5. Try a head-to-head page for two repositories. The URL pattern is `https://gitdebt.com/vs/owner1/repo1/owner2/repo2`.
6. Check the numbers yourself. Open `https://api.gitdebt.com/api/repos/owner/repo/stars.json` and compare the series with what the chart draws.

A common mistake is reading a log chart as a linear one. On a log axis, a straight rising line means steady percentage growth rather than steady star gains.

### Example: equal-start growth

Open the compare page and add several repositories to one set of axes.

Switch it to equal-start growth. Every line now begins at its own first star, so you can compare the first 90 days of each repository instead of the order in which they launched. Switch the scale to log if one line sits far above the rest and flattens the others.

## How do you read a free GitHub star chart maker's output without over-reading it?

You read a star history chart without over-reading it by making four choices in order: the axis, the scale, the window, and only then the shape of the line.

1. Pick the axis. Calendar date answers "what happened when", so use it to line a jump up with a release or a launch post. Days since first star aligns repositories of different ages, so the first 90 days of an old project and a new one can be compared fairly. On a calendar axis, an older repository shows years of history before a newer one has started.
2. Pick the scale. Linear scale shows absolute gaps and hides early growth. Log scale shows rate of growth and makes small and large repositories comparable. Use linear to ask who is ahead, and log to ask who is growing faster.
3. Pick the window. Set an inclusive from/to date range to isolate a launch or a release. If the window starts the day after a spike, it cuts out the event you meant to study.
4. Read the shape. A spike shows that many stars arrived in a short window.

 The chart can't say why (a launch post, a trending list, a link from elsewhere) or whether those users stayed engaged. A plateau shows stars arriving slowly, and it doesn't show that usage or maintenance has stopped.

To try it, open the compare page and add `vercel/next.js` and `vitejs/vite`. Switch the axis to days since first star and the scale to log, then set the window to the first 90 days.

Both lines now start at the same left edge, so the steeper slope is the repository that gathered stars faster in its opening weeks. Switch to calendar date and the same two lines separate by age, which answers a different question.

Stars measure attention, not usage. Treat each reading as a directional signal and pair it with package downloads or commit history before drawing a conclusion about a repository.

## Can a free GitHub star chart maker tell you whether stars are fake?

A star chart can't tell you whether stars are fake, because it counts stars by date and says nothing about who gave them.

Fake stars are a documented industry topic. A research paper on arXiv, ["4.5 Million (Suspected) Fake Stars in GitHub: A Growing Spiral of Popularity Contests, Scams, and Malware"](https://arxiv.org/abs/2412.13459), studies them directly. The paper calls its findings suspected fake stars, not confirmed ones.

That kind of analysis judges the accounts behind each star. gitdebt does not flag stars as fake, real or suspicious, and it does not score accounts.

A sharp spike can come from a launch post or a release, and the line alone can't separate those causes from any other. For a dependency decision, check the commit history next.

## Is a repository with many stars still maintained?

A repository with many stars is still maintained only if its commit history shows recent, steady work, and the star count can't tell you that. Stars record what the crowd did on a given day; commits record what the maintainers did. A popular repository can go quiet in its commits while its star line keeps climbing.

gitdebt reads those two sources separately and never blends them. Beside the star history, each report shows [four health readings from the commit history](https://gitdebt.com/), none of which use stars:

- Maintenance: how steadily commits land.
- Ownership: how few contributors write half the commits.
- Repair load: the share of changes labelled as fixes.
- Debt markers: how TODO and FIXME comments move over time.

The readings are computed from a [shallow git clone analyzed server-side](https://gitdebt.com/about). They are directional signals, not verdicts. Ownership concentration shows where knowledge has pooled in a codebase; it isn't a judgment about any person.

To check a repository, open the [angular/angular report](https://gitdebt.com/angular/angular) and follow this order:

1. Read the star history first, as a record of popularity.
2. Read the maintenance reading, then the commit trend.
3. Ask whether stars and commits move together or diverge.

A rising star line over a mostly empty heatmap marks a repository that is popular and quiet.

## Frequently asked questions

### Is there a free GitHub star chart maker that needs no token?

Yes. gitdebt draws the chart from a plain image URL with no sign-in, token or workflow, so you paste one picture snippet with your owner/repo into the README.

### How do I show a dark mode star chart in my README?

Use a `picture` element with two sources: one image URL ending in `theme=dark` for dark viewers and one ending in `theme=light` for light viewers. The [README snippets page](https://gitdebt.com/badges) has the full block to copy, and each viewer sees the drawing that matches their theme.

### Why is my new chart a placeholder?

Nobody has analyzed that repository yet, so the image URL returns a placeholder while the work is queued. The real chart replaces it at the same URL, so you don't edit the README or paste a new link. Reload the page after a short wait.

### Does the star chart show unstars?

Only as a net change: a dip means more stars were removed than added that day. The chart can't say who unstarred, since gitdebt keeps day counts, not accounts.

## What should you do next?

Put the free GitHub star chart maker to work at three points. Start quick, then go deeper.

- Today: paste the [README snippet](https://gitdebt.com/badges) into your README and swap `OWNER/REPO` for your repository. One edit adds both the light and the dark drawing.
- This week: put your repository next to two or three alternatives on the [compare page](https://gitdebt.com/compare) and switch the axis to days since first star. That lines up each repository's first months, whatever year it launched.
- At your next dependency review: read stars beside the maintenance reading, since the two can disagree.

To start, enter an `owner/repo` on the [report page](https://gitdebt.com/report) and analyze a repository.

## Sources

[starhistory.io — Professional GitHub Star Charts](https://starhistory.io/) · [Generate Self-Hosted Star History Charts | SkillsClaude](https://skillsclaude.org/skills/star-history-chart) · [README badges, star-history charts and embeds](https://gitdebt.com/badges) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/) · [Saving repositories with stars](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) · [About anonymized URLs - GitHub Docs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) · [REST API endpoints for starring](https://docs.github.com/en/rest/activity/starring) · [gitdebt methodology](https://gitdebt.com/about). Every other source is linked where the article cites it.
