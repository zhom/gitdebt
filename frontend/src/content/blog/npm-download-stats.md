---
description: How do you read npm download stats? Query the public API for point or daily range counts, which measure tarball fetches, not users. Learn to read trends.
faq:
  - answer: You can see npm download stats from the downloads API at api.npmjs.org, which returns totals for last-day, last-week, and last-month plus daily ranges per package. Each package page also shows a Weekly Downloads figure. Both count tarball fetches, not unique users.
    question: Where can I see npm download stats for a package?
  - answer: "You can query npm download stats back to January 10, 2015, and one range query covers at most 18 months in total. For a longer history, request several ranges in sequence and join them yourself, according to npm's registry documentation."
    question: How far back can I query npm download stats?
  - answer: The API does work for scoped packages, and it takes scoped names written as @scope/name in the package position of the URL, such as api.npmjs.org/downloads/point/last-week/@babel/core. The count covers only that exact package name, not forks or related packages. This applies to any scoped package name.
    question: Does the API work for scoped packages?
  - answer: "High downloads do not mean a package is maintained. Downloads show that something is fetching a package, not that anyone is maintaining it. Existing projects and CI jobs can keep a count steady for an abandoned package. Check the repository's commits in the trailing 90 days against the 90 before, for example in a gitdebt report."
    question: Do high downloads mean a package is maintained?
publishDate: "2026-10-18"
tags:
  - npm
  - download stats
  - npm api
  - dependency health
  - github stars
  - package comparison
title: "npm download stats: how to query the API and read trends"
---

**npm download stats come from the [official npm registry downloads API](https://github.com/npm/registry/blob/main/docs/download-counts.md), which returns counts for the last day, last week, last month, or a daily date range. Read each figure as package fetches, not unique users. Downloads describe what consumers did; commits describe what maintainers did, so check both before judging a package.**

A package page shows one number, Weekly Downloads, and you have to decide whether it justifies a new dependency. That figure alone can't tell you where it comes from or what sits beside it.

You will be able to query the API, read a trend instead of a single week, and cross-check downloads against repository stars and commits.

> **Key Takeaways**
> - Compare packages over full-week windows so one release day or build burst can't decide the result.
> - Compare change across a window, not the raw gap. A larger package can be flat while a smaller one climbs.
> - Downloads and stars are independent signals. A package can lead on one and trail on the other.
> - A low commit count can mean a finished package, so treat health readings as directional, not verdicts.

## How do you get npm download stats from the official API?

The official way to get npm download stats is to call the public downloads API at `api.npmjs.org` with a period or a date range, plus a package name.

![Image generated](https://api.birchseek.com/m/RKgd9d6u6OYZu2e1Vs-yzq83xapZWdqS.webp)

The [npm registry documentation](https://github.com/npm/registry/blob/main/docs/download-counts.md) lists point values at `https://api.npmjs.org/downloads/point/{period}[/{package}]`. It lists ranges at `https://api.npmjs.org/downloads/range/{period}[/{package}]`.

1. **Pick the endpoint.** Use a point endpoint for one total and the range endpoint for one total per day.
 - Last day: `https://api.npmjs.org/downloads/point/last-day/react`
 - Last week: `https://api.npmjs.org/downloads/point/last-week/react`
 - Last month: `https://api.npmjs.org/downloads/point/last-month/react`
 - Daily range: `https://api.npmjs.org/downloads/range/2024-01-01:2024-01-31/react`

 The range endpoint takes a `start:end` pair joined by a colon. Swap `point` for `range` with a `last-*` period and you get daily rows for that period.

2. **Write scoped names whole.** Keep the `@` and the slash, as in `/downloads/point/last-week/@babel/core`. Dropping the scope queries a different package or returns an error.

3. **Call it with `fetch`:**

```javascript
const url = "https://api.npmjs.org/downloads/point/last-week/react";
const res = await fetch(url);
if (!res.ok) throw new Error(`npm API returned ${res.status}`);
const { downloads, start, end, package: name } = await res.json();
console.log(name, start, end, downloads);
```

4. **Read the response.** A point response carries `downloads`, `start`, `end` and `package`. A range response puts an array of `{ downloads, day }` objects under `downloads`. The numbers in this example are placeholders, not live data.

```json
{
  "downloads": 1000,
  "start": "2024-01-01",
  "end": "2024-01-07",
  "package": "react"
}
```

5. **Check your range against the limits.** Success is a 200 response with a total that matches the window you asked for. A date range that is too long fails, so check it before you loop.

### Range and bulk limits

The [npm registry download counts documentation](https://github.com/npm/registry/blob/main/docs/download-counts.md) states that "Queries are limited to at most 18 months of data." It also gives the earliest date for which data is returned: January 10, 2015.

For bulk queries, the docs say they "are limited to at most 128 packages at a time and at most 365 days of data."

### Update cadence and rate limits

npm's documentation does not publish a rate-limit figure or a precise update cadence. Don't assume one: cache responses, retry with backoff on errors, and treat a very recent day as possibly incomplete.

## What does an npm download count actually measure?

An npm download count is the number of times a package's tarball (the compressed file npm fetches when you install) was requested from the registry in a period. It is not a count of unique users or projects.

Installs in CI pipelines, mirrors and caches can raise or lower the figure, so read it as a relative signal. A counter clicks on every fetch and can't tell one person installing five times from five people installing once.

- **Not unique users.** One developer who installs a package, deletes `node_modules`, and installs again adds two fetches. The counter has no concept of a person.
- **CI installs.** A project that runs `npm install` on every pull request fetches its dependencies on every run unless it restores a cache. A busy repository can add far more fetches than it has developers.
- **Caches.** A local or CI cache can serve a tarball without contacting the registry, so that install may not be counted at all.
- **Mirrors.** A company mirror can fetch a tarball once and serve it to many machines, so many installs can show up as one.

These four are general cautions about how counters behave, not statements from npm's documentation.

### What the Weekly Downloads figure represents

The Weekly Downloads number on a package page is a total for a recent seven-day period, not a lifetime count. A week holds every weekday and both weekend days, so a single build burst or release day can't dominate it the way it can in a daily row.

A download says nothing about whether the maintainers still commit, review or release. Read it beside the commit history in a [gitdebt report](https://gitdebt.com/report), and keep the two separate instead of blending them into one score.

## How do you compare packages and read download trends?

To compare two packages and read their download trends, use the same window for both, work in weekly totals, smooth them, mark releases, and compare change instead of size. The worked pair here is `react` and `vue`, with no live numbers quoted.

1. **Fetch the same daily range for each package.** Use identical start and end dates, such as 26 full weeks plus 3 extra weeks at the front. The extra weeks feed the moving average in step 3. Fetching `react` for one span and `vue` for another makes every later comparison meaningless.

   ```bash
   curl https://api.npmjs.org/downloads/range/2024-01-01:2024-09-01/react
   curl https://api.npmjs.org/downloads/range/2024-01-01:2024-09-01/vue
   ```

 The official docs also describe a bulk query that fetches several packages in one call, within the bulk limits listed under Range and bulk limits.

2. **Sum the days into 7-day buckets.** Every bucket holds five weekdays and two weekend days, so the weekly swing cancels out. Daily totals stay noisy because they rise and fall with the day of the week. Start each bucket on the same weekday for both packages.

3. **Apply a 4-week moving average.** For each week, average that bucket with the three before it. Success looks like a line that moves gently instead of jumping between points.

4. **Mark release dates on the chart.** Take them from each package's release list and draw a vertical line for each. A release, or a day when many CI jobs reinstall dependencies, can produce a spike without any change in real adoption. Treat that as a caution, not a measured result. If a bump sits on a release line, discount it.

5. **Compare change over the window, not the gap.** Divide the last smoothed value by the first and note the percentage change for each package.

If `react` shows a higher total than `vue` but its smoothed line is flat, while `vue` rises across the window, the slopes tell you more than the raw gap does. Apply the same check to any pair you're weighing.

Read the slope as a direction, not a headcount. For a second signal that doesn't depend on installs, overlay star history on [gitdebt's compare page](https://gitdebt.com/compare).

## Which tool should you use to read npm download stats?

The right tool for npm download stats depends on the view you need: the package page for a glance, the API for scripted history, and a third-party site for a side-by-side view.

| Method | Best for | Limitation | Source |
| --- | --- | --- | --- |
| npm package page | Quick glance at one package | Weekly Downloads headline only | [react on npm](https://www.npmjs.com/package/react) |
| Official downloads API | Point and range queries in scripts | You write the request and any chart | [npm registry download counts](https://github.com/npm/registry/blob/main/docs/download-counts.md) |
| npmtraffic | Daily rows, release markers, comparing 2 to 5 packages | Its ranges end at yesterday UTC | [npmtraffic](https://npmtraffic.com/) |
| DevToolLab | Daily, weekly or monthly counts in a browser | One package view at a time | [DevToolLab](https://devtoollab.com/tools/npm-download-stats) |

Third-party tools read the same official API, so check the API documentation for windows and limits before you quote a date range from any of them.

## How do npm downloads relate to GitHub stars?

npm downloads and GitHub stars measure different crowds, so they can disagree. [GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) says "Starring a repository also shows appreciation to the repository maintainer for their work," so a star is a bookmark or thank-you from a person.

A download is a tarball fetch by a person, a build server or another package.

High downloads with low stars could mean a transitive or build-time dependency fetched mostly by tooling. High stars with low downloads could mean a repository people follow but don't install from npm. Both are possible explanations, not findings.

To check `facebook/react` and `vercel/next.js`:

1. Find the npm package name in the repository's package metadata.
2. Read the downloads trend from the API.
3. Read the repository's star history by calendar date, such as on the [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks).
4. Compare the shapes, not the numbers.

gitdebt's [stars-versus-downloads chart](https://gitdebt.com/badges) is a plain image URL that renders in a README without JavaScript. It supports npm, crates.io, PyPI and Docker downloads.

```text
https://api.gitdebt.com/api/repos/facebook/react/usage.svg
```

The [about and methodology page](https://gitdebt.com/about) lists the source and state of each series. The chart keeps the two series apart: stars show what the crowd did, downloads show what consumers did.

## Is an npm dependency still alive? A four-signal checklist

An npm dependency looks alive when its downloads and its repository activity both point to continued use and upkeep, and four checks help you read that. Each one is a directional signal, not a verdict.

1. **Downloads trend.** Chart weekly buckets across the whole window and read the slope, not the latest week. *Limit:* a flat line can reflect CI traffic, so it doesn't prove steady adoption.
2. **Maintenance.** Check whether the commit trend is rising, steady or fading. *Limit:* a low commit count can mean a finished package that needs little change.
3. **Ownership concentration.** Check how few contributors write half the commits. *Limit:* [gitdebt's about page](https://gitdebt.com/about) says it "shows where knowledge has pooled in a codebase; it is not a judgement about any person."
4. **Repair load.** Read the fix-labelled share of file changes. *Limit:* it describes commit-message history and does not prove bugs.

### When the signals disagree

Rising downloads with falling maintenance is a disagreement. So is the reverse: steady commits while downloads slide.

Neither pattern tells you the package is dying or healthy. Open the changelog and the issue tracker before you decide.

### Try it on a real repository

The [angular/angular report](https://gitdebt.com/angular/angular) shows these readings drawn from commit history. gitdebt computes its health readings from the git history, not from stars.

To check your own dependency, enter its `owner/repo` on the [gitdebt report page](https://gitdebt.com/report). Then set what you see beside the package's weekly download line.

## Frequently asked questions

### Where can I see npm download stats for a package?

npm download stats come from the [downloads API](https://github.com/npm/registry/blob/main/docs/download-counts.md) at `api.npmjs.org`, which returns totals for last-day, last-week and last-month plus daily ranges per package. Each package page also shows a Weekly Downloads figure. Both count tarball fetches, not unique users.

### How far back can I query npm download stats?

Data starts on January 10, 2015, and one range query covers at most 18 months, according to npm's registry documentation. For a longer history, request several ranges in sequence and join them yourself.

### Does the API work for scoped packages?

Yes, the API takes scoped names written as `@scope/name` in the package position of the URL, such as `api.npmjs.org/downloads/point/last-week/@babel/core`. The count covers only that exact name, not forks or related packages.

### Do high downloads mean a package is maintained?

Downloads show that something is fetching a package, not that anyone is maintaining it. Existing projects and CI jobs can keep a count steady for an abandoned package. Check the repository's commit trend, for example in a [gitdebt report](https://gitdebt.com/report).

## What should you do next with npm download stats?

Next, run one query, build one weekly comparison, and add a repository health reading before you choose a dependency.

- **In 5 minutes:** Run one point query against the downloads API for the package you're weighing, and note the last-week total.
- **In 30 minutes:** Pull daily ranges for two candidate packages, group them into weekly buckets, and compare the change in each, not the raw size.
- **This week:** Add the repository's health reading before you decide. Check its commit trend and how few contributors write half the commits.

To get that second source in one place, look up the repository on [gitdebt](https://gitdebt.com/). It shows the star history beside four health readings from the commit history.

[Every public repository report works without an account](https://gitdebt.com/about): enter any `owner/repo` on the [report page](https://gitdebt.com/report). Keep your npm download stats open in the next tab and compare the two trends.

## Sources

[NPM Download Stats Online | Package Analytics - DevToolLab](https://devtoollab.com/tools/npm-download-stats) · [download-stats - npm](https://github.com/npm/registry/blob/main/docs/download-counts.md) · [Saving repositories with stars - GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) · [README badges, star-history charts and embeds](https://gitdebt.com/badges) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/)
