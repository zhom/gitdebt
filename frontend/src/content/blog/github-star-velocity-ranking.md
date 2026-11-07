---
description: What is a GitHub star velocity ranking? It lists repositories by stars gained over 1, 7 or 30 days. Learn to check a spike against commit history first.
faq:
  - answer: There is no official star velocity figure to look up, because you compute it yourself from star dates. Examples include the stars a repository gained over 1, 7 or 30 days. Any exact formula for GitHub Trending comes from community inference.
    question: Is there an official GitHub star velocity ranking?
  - answer: Use the window that fits your question, and always say which window a number covers. A 1-day count shows single events, 7 days shows weekly momentum, and 30 days smooths out one launch post. The gitdebt star velocity leaderboard lists all three windows.
    question: Which window should I use, 1, 7 or 30 days?
  - answer: "Two tools probably show different counts for the same day because they read different sources. A curve rebuilt from the current stargazer list leaves out accounts that have since unstarred. GitHub's own star history is net of unstars, so the two can disagree."
    question: Why do two tools show different star counts for the same day?
  - answer: Star velocity does not show that a project is maintained, because it records only what the crowd did. Maintainer activity shows up in the commit history, which you should check separately, for example through the maintenance reading on a gitdebt report.
    question: Does star velocity show a project is maintained?
  - answer: gitdebt does not detect fake stars, since it does not flag stars as fake, real or suspicious and does not score accounts. It stores star dates and day counts only. The methodology page on gitdebt lists what the tool deliberately leaves out.
    question: Does gitdebt detect fake stars?
publishDate: "2026-11-07"
tags:
  - github stars
  - star velocity
  - github trending
  - star history
  - commit history
  - repository health
title: "GitHub star velocity ranking: How to compare repos fairly"
---

**A GitHub star velocity ranking is a computed list: [stars gained over a window you pick](https://ghtrends.dev/daily/), such as 1, 7 or 30 days, sorted from highest to lowest. It is not an official GitHub ranking, and a spike says nothing about maintenance. Read it next to the repository's commit history, the second, independent source that shows whether the project is alive.**

Maintainers, developer-relations marketers, engineering managers, analysts and dependency reviewers reach the same point. A repository sits near the top of a GitHub star velocity ranking, and someone has to decide what the rank means.

After reading, you can define velocity for a window you choose and separate what GitHub documents about Trending from inference. You can also chart repositories of different ages fairly and check a spike against commit cadence.

> **Key Takeaways**
> - A daily count from GitHub's star history is net of unstars; a curve rebuilt from today's stargazer list holds only current stargazers.
> - Align repositories by days since first star, so a young repository's first 90 days sits beside an established one's launch period.
> - Stars show what the crowd did; commits show what maintainers did. Read them as two separate sources, never blended.
> - Name the window and the star source behind every velocity figure you quote.

## What is a GitHub star velocity ranking?

GitHub star velocity is the number of stars a repository gains over a fixed window, usually 1, 7 or 30 days. **Absolute velocity** is the raw count. **Relative velocity** divides that count by the repository's own star total at the start of the window, so a small project and a large one can be compared.

A star is a bookmark that other people can see. [GitHub Docs on starring](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) says you can star repositories to "keep track of projects you find interesting," and that starring "shows appreciation to the repository maintainer for their work."

The paper ["What's in a GitHub Star?"](https://arxiv.org/abs/1811.07643) (arXiv 1811.07643), by Hudson Borges and Marco Tulio Valente, describes starring as something users do "presumably to manifest interest or satisfaction with an open source project." Velocity therefore counts expressions of interest and appreciation, nothing more.

Two repositories that gain the same number of stars in the same 7 days tie on absolute velocity. On relative velocity, the smaller one ranks well above the larger, because the same gain is a bigger share of the stars it already had.

### Windows and limits

The [gitdebt leaderboard](https://gitdebt.com/leaderboard) ranks repositories by stars gained over 1, 7 and 30-day windows. A 1-day window shows a single day's burst. A 30-day window smooths it, so one launch post carries less weight. [gitdebt's methodology page](https://gitdebt.com/about) explains where each star series comes from.

Relative velocity has a weak point. A repository with a very small starting total can post a large ratio from a handful of new stars.

Neither measure in a GitHub star velocity ranking says whether anyone still maintains the code. The commit history answers that separate question.

## How does GitHub Trending ranking work, and what is documented?

GitHub's documentation confirms that stars feed its repository rankings, and it publishes no formula for the Trending page. Everything past that point is community inference.

[GitHub Docs on saving repositories with stars](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) states: "Many of GitHub's repository rankings depend on the number of stars a repository has." That sentence names stars as an input to rankings. It gives no time window, no weighting and no baseline for comparing one repository with another.

Third-party write-ups describe the method with more confidence. A [RepoBoost blog post](https://buygithub.com/blog/how-github-stars-work/) asserts that Trending ranks by star velocity, defined as "the rate at which new stars arrive relative to a repository's normal pace." Others reverse-engineer the page by watching which repositories appear and when.

Treat these descriptions as hypotheses. They suggest what to test, but no GitHub page backs them, so they can't be cited as how Trending works.

A GitHub star velocity ranking you compute from star dates is a different object from the Trending page. You choose the window (1, 7 or 30 days), you choose raw gains or gains relative to a repository's own baseline, and you can publish the arithmetic. Trending offers none of that to check.

A repository can therefore lead your 7-day list and be absent from Trending, or the reverse, without either list being wrong. Each answers a different question.

When you cite a ranking, name its source:

- Trending: GitHub's page, method unpublished.
- Computed velocity: stars gained over a stated window, from dated star counts. The [star velocity leaderboard](https://gitdebt.com/leaderboard) is one example, and [how gitdebt works](https://gitdebt.com/about) lists where its star series come from.

## How do you read a GitHub star velocity ranking without being misled?

A GitHub star velocity ranking misleads in five predictable ways, and each one has a check you can run before you trust a position on the list.

1. **Small-base inflation.** A handful of stars on a tiny repository produces a large relative figure. Read the relative figure beside the absolute count.
2. **Single-event spikes.** A launch post packs weeks of attention into a day or two. The arXiv study [2511.04453](https://arxiv.org/abs/2511.04453) looks at Hacker News launches. Before you quote any figure from it, check its sample size, whether it reports means or medians, and the limits its authors state. Then open the commit history to see what the maintainers did after the spike.
3. **Window choice.** A 1-day window shows events, a 7-day window shows a week's momentum, and a 30-day window smooths a single launch. State which window a figure uses, as the [gitdebt leaderboard](https://gitdebt.com/leaderboard) does with its 1, 7 and 30-day columns.
4. **Inorganic stars.** Suspected fake stars are a studied industry topic. The arXiv paper [2412.13459](https://arxiv.org/abs/2412.13459), "4.5 Million (Suspected) Fake Stars in GitHub", is one place to read the method and its limits. gitdebt does not flag stars as fake, real or suspicious, does not score accounts, and stores star dates and day counts only, never stargazer profiles, as [how gitdebt works](https://gitdebt.com/about) explains.
5. **Unstars.** gitdebt reads GitHub's own star history, which is net of unstars, so a day can show a smaller gain than the number of people who starred that day.

The arithmetic is easiest to see with assumed figures. Assume repository A has 10 stars and gains 5 in a week, while repository B has 50,000 stars and gains 500. A shows +50% and B shows +1%, yet B gained 100 times as many stars.

Assume also that 400 of B's 500 stars landed on one day after a launch post. The 1-day window flags B on that day. The 30-day window spreads the same stars across the month, and a flat commit trend beside it shows the interest has not turned into maintenance.

When you build a GitHub star velocity ranking, sort by absolute gain first, then by relative gain, and keep the window the same across every row you compare.

## How do you chart star velocity by date and by days since first star?

You chart star velocity by calendar date to see who grew over the same period, and by days since first star to compare repositories that launched years apart. Either chart is only as exact as the star source behind it.

GitHub's REST API lists stargazers at `GET /repos/{owner}/{repo}/stargazers`. [GitHub's REST documentation for starring](https://docs.github.com/en/rest/activity/starring) says a request with the `application/vnd.github.star+json` media type returns a `starred_at` timestamp for each star. The endpoint lists current stargazers, so a curve rebuilt from it leaves out every star that was later removed.

Results are paginated, with a maximum of 100 items per page, according to [GitHub's pagination guide](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api).

```bash
curl -H "Accept: application/vnd.github.star+json" \
  "https://api.github.com/repos/facebook/react/stargazers?per_page=100&page=1"
```

GitHub's own star history gives exact daily counts, net of unstars, back to the repository's creation, and gitdebt re-reads it on a schedule, as [gitdebt's methodology page](https://gitdebt.com/about) describes.

Each chart names its source, coverage date and state. Check which source a curve uses before you trust its precision.

1. Open the report or `/compare`. Go to `gitdebt.com/owner/repo` for one repository, or [gitdebt.com/compare](https://gitdebt.com/compare) for up to eight on one set of axes. No sign-in is needed.
2. Choose calendar date. This view shows every repository over the same period. It suits a question like "who grew this quarter?" and misleads when the repositories launched years apart.
3. Switch the axis to days since first star. Each curve now starts at day zero, so a young repository and an old one line up at their own beginnings.
4. Compare the first 90 days. Read the curves from day zero to day 90, and ignore everything to the right.
5. Use log scale when sizes differ widely. On a linear axis, a repository with a small total flattens against a much larger one, while log scale keeps both shapes readable.
6. Export CSV or JSON if you need the numbers. Use the file for your own velocity windows of 1, 7 or 30 days.
7. Write down the source. Name where each velocity figure came from: GitHub's star history, the stargazer list or historical data. Figures from different sources aren't equally exact.

For an equal-start example, open the [frontend frameworks star history comparison](https://gitdebt.com/compare/frontend-frameworks), pick a recently launched repository, and add an established one such as `facebook/react`. On calendar date, the established repository's launch years sit far to the left and the two curves barely overlap.

On days since first star, both launches start at the same point. The young repository's first 90 days sit directly beside the established repository's launch period.

## How do you check that a fast-rising repository is still maintained?

You check that a fast-rising repository is still maintained by reading its commit history beside its star curve, because stars record what the crowd did and commits record what the maintainers did. The two never blend into one score.

The liveness signals come from the git history alone. [gitdebt's methodology page](https://gitdebt.com/about) describes how health signals are computed from a shallow clone of the repository, so no star count feeds any of them. The quickest to read are these:

- Maintenance: recent commit activity.
- Commit trend: commit volume per month.

Another reading, ownership, shows how few contributors write half the commits. It shows where knowledge has pooled in a codebase. It is not a judgement about any person.

Once you have the velocity figure and the commit cadence, three combinations cover most spikes. Each one points to something to investigate.

1. Rising stars, steady commits. Attention and work are moving together. Open the commit trend and check whether recent releases or merged changes line up with the climb.
2. Rising stars, quiet commits. Attention has arrived while maintenance has slowed. Look at the heatmap for when activity stopped, and ask whether a single launch post or list inclusion drove the stars.
3. Steady stars, active commits. Maintainers are shipping without a crowd. The project may be young, niche or settled, so compare it with peers before reading anything into the flat curve.

None of these combinations settles whether a project is safe to depend on. A commit shows that someone changed the code. It doesn't show that the change was good.

A repository report places the star history and the health readings side by side. The [angular/angular report](https://gitdebt.com/angular/angular) is one example, pairing star history with commit-based health readings. Read the spike first, then the cadence under it, and treat the pairing as a directional signal.

## How do you track star velocity and put the chart in a README?

You track star velocity on the gitdebt leaderboard, compare repositories on the compare page, and embed the chart as a plain image URL in your README. Everything works signed out. gitdebt is not affiliated with GitHub.

1. **Open the leaderboard.** The [star velocity leaderboard](https://gitdebt.com/leaderboard) ranks repositories by stars gained over recent 1, 7 and 30-day windows. Find your repository and read all three windows. A repository that leads on 1 day but not on 30 usually had a single-event spike.
2. **Compare up to eight repositories.** On [the compare page](https://gitdebt.com/compare), add your repository and its peers, then switch the axis from calendar date to days since first star. Skipping that switch is the usual mistake, because a young repository and an established one only line up fairly on the second axis.
3. **Copy the snippet.** The [badges page](https://gitdebt.com/badges) lists the embeds, including light and dark star-history charts as copy-paste README snippets. The chart is a plain public image URL, `https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg`, and `theme=light` and `theme=dark` select the two drawings. Replace `OWNER/REPO` and keep the `?ref=readme` link around the image.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
    <img alt="Star history of OWNER/REPO" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

4. **Paste it into your README and preview.** Nothing needs a token, an API key or a build step. To queue a repository yourself, [analyze it on the report page](https://gitdebt.com/report).

The snippet is working when the chart renders in both GitHub themes.

## Frequently asked questions

### Is there an official GitHub star velocity ranking?

GitHub's documentation names stars as an input to its rankings but gives no formula for Trending, and exact formulas found elsewhere are community inference. Star velocity is a figure you compute yourself from star dates, such as the stars a repository gained over 1, 7 or 30 days.

### Which window should I use, 1, 7 or 30 days?

Use the window that fits your question, and always name it. A 1-day count shows single events, 7 days shows weekly momentum, and 30 days smooths out one launch post. The [star velocity leaderboard](https://gitdebt.com/leaderboard) lists all three.

### Why do two tools show different star counts for the same day?

They probably read different sources. A curve rebuilt from the current stargazer list leaves out accounts that have since unstarred, while GitHub's own star history is net of unstars.

### Does star velocity show a project is maintained?

No. Star velocity records what the crowd did, so check the commit history separately, for example the maintenance reading on a gitdebt report.

### Does gitdebt detect fake stars?

gitdebt does not flag stars as fake, real or suspicious, and it does not score accounts. It stores star dates and day counts only. The [methodology page](https://gitdebt.com/about) lists what it deliberately leaves out.

## What should you do next with a GitHub star velocity ranking?

Start with one repository and widen the view over a month:

- Today: look up one repository and read its star history beside its commit history.
- This week: compare it with up to eight repositories on [days since first star](https://gitdebt.com/compare), using the first 90 days of each.
- This month: watch 1, 7 and 30-day velocity, check each spike against commit cadence, and treat every reading as a directional signal.

On a GitHub star velocity ranking, a spike matched by a burst of commits needs less scrutiny. A spike over a quiet commit history calls for a closer look at what drove the stars.

[Analyze any public repository on the gitdebt report page](https://gitdebt.com/report).

## Sources

[GitHub Trending Today: Repositories Gaining the Most Stars](https://ghtrends.dev/daily/) · [Saving repositories with stars - GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) · [What's in a GitHub Star? Understanding Repository Starring…](https://arxiv.org/abs/1811.07643) · [GitHub star velocity leaderboard](https://gitdebt.com/leaderboard) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring) · [angular/angular — star history & stats · gitdebt](https://gitdebt.com/angular/angular) · [Compare GitHub star history](https://gitdebt.com/compare). Every other source is linked where the article cites it.
