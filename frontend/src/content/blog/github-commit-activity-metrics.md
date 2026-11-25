---
description: What do GitHub commit activity metrics show? Four views of pace, rhythm, ownership and timing. Learn to read each one and check whether a project is alive.
faq:
  - answer: A repository with few recent commits does not automatically count as abandoned, since it may be feature-complete or stable. Read a longer commit window, because a short one can miss slow but steady work, and treat the result as a directional signal, not a verdict.
    question: Does a repository with few recent commits count as abandoned?
  - answer: "The counts differ because GitHub's statistics exclude merge commits, and the contributor view also excludes empty commits, while git log lists both by default. Repositories that merge often show the widest gap between the GitHub count and the count from your local git log."
    question: "Why doesn't the GitHub commit statistics API match my local git log?"
  - answer: "The API returns 202 because GitHub computes statistics in a background job. When the data isn't cached, the API answers 202 and starts compiling it. Wait a few seconds and send the same request again, and you'll get the cached result."
    question: Why does the GitHub statistics API return a 202 or an empty response?
  - answer: "More commits do not mean a healthier project by themselves, because activity isn't quality. Merge and squash workflows change how many commits a change produces, and bot accounts can add commits of their own. Read commit volume alongside author concentration."
    question: Do more commits mean a healthier project?
  - answer: The API counts up to 10,000 commits for code frequency and for additions and deletions in contributor statistics, as GitHub documents. Repositories with more commits than that need another source for those figures, so check the commit total before relying on them.
    question: "How many commits does GitHub's statistics API count before it stops?"
publishDate: "2026-11-25"
tags:
  - github
  - commit activity
  - repository health
  - github api
  - dependency review
  - open source
title: "GitHub commit activity metrics: Read each view and its limits"
---

**[GitHub commit activity metrics](https://docs.github.com/en/rest/metrics) come as four views: commits per week, the last 52 weeks of daily counts, commit totals per contributor, and the hourly commit distribution. Each is a directional signal about pace, rhythm, ownership or timing, and it reads best over a 26-week or 52-week window. Commit counts don't measure effort or code quality.**

You're about to add a dependency or review a repository for your team, and its commit history shows either constant activity or a long gap. Neither tells you much until you know how GitHub counts commits and where GitHub commit activity metrics stop being reliable.

> **Key Takeaways**
> - An empty first response from a [statistics endpoint](https://docs.github.com/en/rest/metrics/statistics) is a 202 while GitHub compiles the data, so retry.
> - Raw commit volume can disagree with author concentration: a busy repository can still have half its commits written by a few contributors.
> - A quiet repository can be finished rather than abandoned, so weigh the gap against its history.
> - Stars and commits are independent sources; read them side by side, never blended.

## What do GitHub commit activity metrics actually show?

GitHub commit activity metrics come from five REST endpoints, and each one answers a single question about pace, rhythm, ownership, timing or trend.

![Illustration for What do GitHub commit activity metrics actually show?](https://api.birchseek.com/m/94P1kMYrd515kXbCQgWlVyZAYasygXpR.webp)

[GitHub's REST documentation](https://docs.github.com/en/rest/metrics/statistics) says the repository statistics endpoints fetch "the data that GitHub uses for visualizing different types of repository activity."

Its [metrics page](https://docs.github.com/en/rest/metrics) lists five of them under the docs' own titles: Get the weekly commit activity, Get the last year of commit activity, Get all contributor commit activity, Get the weekly commit count, Get the hourly commit count for each day.

| Docs title | What it counts | Question it answers |
|---|---|---|
| Get the weekly commit count | Commits per week | Pace: how many commits land in a typical week? |
| Get the last year of commit activity | Commit activity over the past year | Rhythm: which days and weeks were busy, and which were silent? |
| Get all contributor commit activity | Commits per contributor | Ownership: who wrote the commits? |
| Get the hourly commit count for each day | Commits by hour for each day | Timing: when in the week does work happen? |
| Get the weekly commit activity | Weekly activity totals | Trend: how did volume move from week to week? |

The weekly count gives the overall shape, and the last-year view shows the days inside each week. The contributor and hourly views slice the same commits by person and by clock time.

One request is enough for a first look at GitHub commit activity metrics. This one fetches `facebook/react`, and any public `owner/repo` works:

```bash
curl -L https://api.github.com/repos/facebook/react/stats/commit_activity
```

The docs warn that if the data "hasn't been cached when you query a repository's statistics, you'll receive a 202 response" while a background job compiles it. Repeat the call a moment later, then check the weeks you remember as busy against what comes back.

Each of the GitHub commit activity metrics answers only its own question. The weekly count says nothing about who wrote the commits, and the contributor totals say nothing about when.

## What do GitHub commit statistics leave out?

GitHub's commit statistics leave out merge commits everywhere, leave out empty commits in the contributor view, and set a 10,000-commit limit on two of the endpoints.

[GitHub Docs](https://docs.github.com/en/rest/metrics/statistics) states: "Statistics exclude merge commits. Contributor statistics also exclude empty commits." A repository that lands pull requests through merge commits will therefore show fewer commits than a local `git log` count.

The documentation says the code frequency endpoint, which returns weekly additions and deletions, can only be used for repositories with fewer than 10,000 commits. It gives the same 10,000-commit limit for additions and deletions in the contributor statistics.

For an old, busy repository, check those two views before you quote them.

GitHub says computing repository statistics is expensive, so it returns cached data whenever possible. That is why a first request can come back empty with a 202.

A 202 isn't an error. The documentation says to give the job a few moments to complete and then submit the request again.

The docs recommend setting the Accept header to `application/vnd.github+json`. This one asks for the last year of commit activity for a repository:

```bash
curl --request GET \
  --url "https://api.github.com/repos/OWNER/REPO/stats/commit_activity" \
  --header "Accept: application/vnd.github+json"
```

Swap `OWNER/REPO` for the repository you're checking, such as `vitejs/vite`.

## How do you read GitHub commit activity metrics as a maintenance signal?

Reading GitHub commit activity metrics as a maintenance signal means choosing a time window, counting consecutive active weeks, and comparing the overall trend against any single spike.

1. Pick a window. Start with 26 weeks, then look at 52 weeks as a second pass. A 4-week window swings with holidays and release freezes, so one slow month can look like abandonment.
2. Count consecutive active weeks. Call a week active if it has at least one commit, and find the longest unbroken run ending at the most recent complete week. A long run points to steady work, while isolated active weeks separated by gaps point to occasional work.
3. Compare the trend against any single spike. Open the commit trend and commit heatmap in a [gitdebt report](https://gitdebt.com/report) and check whether one week holds most of the window's commits. If it does, the repository had a burst rather than a cadence.

[findarepo's active ranking](https://findarepo.com/active/) uses the same 26-week window. Its ranking page says each repository is scored on "how consistently it received commits over the last 26 complete weeks, so steady long-term work outranks a single burst of activity."

Suppose one repository has 40 commits in one week and none in the other 25 weeks of the window. A second has 1 to 3 commits in each of 20 of those weeks.

The second repository has the longer run of consecutive active weeks. It is the steadier of the two despite the lower total.

A flat line fits a finished library as well as a neglected one. A small library can be complete, and its maintainers may have closed it on purpose.

GitHub commit activity metrics also can't show intent. Nothing in them says whether maintainers plan to continue, and a busy history can sit beside unanswered issue reports.

## Why does commit count need author concentration beside it?

Commit count needs author concentration beside it because volume shows how much happened, while concentration shows how many people that work depended on.

Ownership concentration is the number of contributors it takes to write half the commits. It shows where knowledge has pooled in a codebase; it is not a judgement about any person.

The raw input is GitHub's "Get all contributor commit activity" statistic from its [metrics endpoints](https://docs.github.com/en/rest/metrics). [GitHub Docs](https://docs.github.com/en/rest/metrics/statistics) says it "Returns the total number of commits authored by the contributor."

Sort contributors by that total, add from the top until you pass half of the combined count, and note how many names you used. That is a share-of-commits reading.

Because merge commits and empty commits are excluded from contributor statistics, the shares describe non-merge, non-empty commits only.

If a repository has 1,000 commits in a year and two contributors wrote 500 of them, half the activity rests on two people. If 40 contributors wrote the same 1,000, it takes many names to reach half. Both repositories look equally busy, yet the first depends on two people.

A low count can describe a small, focused project, and it can't say who reviewed, designed or triaged the work. To see the ownership reading for a large public repository, open the [angular/angular report](https://gitdebt.com/angular/angular) and compare it with the commit trend.

## What can't GitHub commit activity metrics tell you?

GitHub commit activity metrics count commits that landed in a history, so they can't tell you how much effort went in, how good the code is, or what the maintainers plan next.

1. Commits per week: a commit count doesn't measure effort or size of change. A one-line typo fix counts the same as a rewrite, so don't treat totals as productivity.
2. Weekly and daily totals: a project's merge workflow changes how many commits land. Squashing pull requests turns many commits into one, so compare a repository against its own past, not against a project with a different workflow.
3. All totals: bot accounts can add commits, so a busy week may be dependency bumps rather than feature work.
4. Excluded commits: merge and empty commits fall outside the counts, so treat a total as a floor for what happened.
5. Every commit view: activity says nothing about code quality. A repository can commit daily and still ship broken releases.

Each of gitdebt's four health readings has a limit of its own:

- Maintenance shows recent commit cadence. It can't say whether issues get answered or releases ship.
- Ownership shows how few contributors write half the commits. It can't say who reviews the work.
- Repair load shows the fix-labelled share of file changes. It depends on how maintainers word commit messages and can't say a defect exists.
- Debt markers show TODO/FIXME movement. They can't say how much technical debt a codebase carries or whether a marker is still relevant.

gitdebt computes these signals from a shallow git clone, so its readings come from the analyzed history, not from GitHub's statistics endpoints. The [methodology on the about page](https://gitdebt.com/about) lists the data sources and what gitdebt deliberately does not do.

## How do you check whether a dependency is still alive?

You check whether a dependency is still alive by reading its commit history and its star history as two separate sources, in a public report that needs no API token and no sign-in.

1. Open the report. Enter `owner/repo`, for example `facebook/react`, on the [gitdebt report page](https://gitdebt.com/report). A repository nobody has analyzed yet is queued, so check back shortly.
2. Read the commit trend, which shows monthly commit volume. Look for a steady line or a slow fade, and remember that a quiet stretch alone doesn't mean abandonment.
3. Count how many weeks in a row show activity to separate a single burst from steady work.
4. Check ownership concentration: how few contributors wrote half the commits.
5. Glance at the secondary readings. Repair load (the fix-labelled share of changes) and TODO/FIXME movement are directional. File change frequency counts how many analyzed commits touched each file; it points to files that get revisited without saying they were unstable. Fix-labelled changes describe fix-labelled history and don't prove a bug.
6. Compare against stars. Open the [star history comparison](https://gitdebt.com/compare), which can "overlay up to eight repositories' star histories on one set of axes." Read it after the commit steps, not during them.

Stars show what the crowd did, and commits show what the maintainers did. The star comparison records only the crowd's actions, not who the people were, and gitdebt doesn't flag stars as fake, real or suspicious.

### Worked example: facebook/react and vitejs/vite

Run steps 1 to 5 on `facebook/react`, then repeat them on `vitejs/vite`. For each repository, write down three plain observations before moving on: whether the commit trend is rising, flat or fading, whether the heatmap shows consecutive active weeks, and how concentrated ownership is.

Then enter both slugs on the compare page and read the two star curves. If the commit notes and the star curves point in different directions, record that disagreement rather than averaging the two.

To run the same checks on your own dependency, enter its slug on the report page and follow the six steps.

## How do you put commit activity in a README?

A commit activity view goes into a README as a plain image URL, so GitHub renders it without JavaScript.

The embeds on [gitdebt's README badges and embeds page](https://gitdebt.com/badges) follow one pattern: `https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg`.

1. Pick the asset. Replace `OWNER/REPO` with your slug, for example `vitejs/vite`, and choose the filename.
2. Add both themes. Request `theme=light` and `theme=dark`, then publish them together in a `<picture>` element pasted straight into your README. Keep the `?ref=readme` link around the image.
3. Get the commit calendar from the badges page. The calendar is one of the assets listed there, so copy its snippet and swap in your slug.
4. Check the render by previewing the README on GitHub. A repository nobody has analyzed yet shows a placeholder, and the real image replaces it at the same URL, so you don't edit the snippet again.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=dark">
    <img alt="OWNER/REPO repository card" src="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=light">
  </picture>
</a>
```

Publishing only one theme leaves the image hard to read for people who use the other.

## Frequently asked questions

### Does a repository with few recent commits count as abandoned?

Not necessarily. A repository with few recent commits may be feature-complete or stable. Read a longer commit window, because a short one can miss slow but steady work.

### Why doesn't the GitHub commit statistics API match my local git log?

[GitHub's statistics](https://docs.github.com/en/rest/metrics/statistics) exclude merge commits, and the contributor view also excludes empty commits, while git log lists both by default. Repositories that merge often show the widest gap between the two counts.

### Why does the GitHub statistics API return a 202 or an empty response?

GitHub computes statistics in a background job. When the data isn't cached, the API answers 202 and starts compiling it. Wait a few moments and send the same request again to get the cached result.

### Do more commits mean a healthier project?

Not by themselves, because activity isn't quality. Merge and squash workflows change how many commits a change produces, and bot accounts can add commits of their own. Read volume beside author concentration.

### How many commits does GitHub's statistics API count before it stops?

GitHub documents a 10,000-commit limit for code frequency and for additions and deletions in contributor statistics. Larger repositories need another source for those figures.

## What should you check first?

Check the commit trend and commit heatmap of the dependency you would least want to replace, then widen the check from there.

- Today: open the report for that dependency using the [gitdebt lookup](https://gitdebt.com/), and read the commit trend and the commit heatmap.
- This week: check ownership concentration, then compare it against the star-history curve.
- This month: put a repository card or commit calendar in your own README, using the snippet from [gitdebt.com/badges](https://gitdebt.com/badges).

If the commit trend and the star curve disagree on a repository, read that one first.

Enter any owner/repo at [gitdebt.com/report](https://gitdebt.com/report).

## Sources

[REST API endpoints for metrics - GitHub Docs](https://docs.github.com/en/rest/metrics) · [REST API endpoints for repository statistics - GitHub Docs](https://docs.github.com/en/rest/metrics/statistics) · [Most Actively Developed GitHub Repos — Measured Commit…](https://findarepo.com/active/) · [gitdebt methodology (about page)](https://gitdebt.com/about) · [gitdebt star history comparison](https://gitdebt.com/compare) · [gitdebt README badges and embeds](https://gitdebt.com/badges)
