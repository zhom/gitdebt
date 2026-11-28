---
description: "Is a dependency still maintained? A GitHub repo health dashboard reads four commit signals: cadence, ownership, fix share and TODOs. Check any public repo."
faq:
  - answer: "A GitHub repo health dashboard turns a repository's commit history into four readings: how steadily it's maintained, how many contributors write half the commits, how much of the work is fix-labelled, and whether TODO/FIXME markers are rising. Read them as directional signals."
    question: What is a GitHub repo health dashboard?
  - answer: "A high star count doesn't make a repository healthy, because stars and commits measure different things. Stars show popularity, while commits show maintenance, so check both separately. A popular repository can have a stalled commit trend, and a small one can have commits in every recent month."
    question: Is a repository with many stars healthy?
  - answer: "A high bus-factor reading doesn't make a project safe or unsafe. Ownership concentration shows where knowledge has pooled in a codebase, and it is a directional reading and not a judgement about any person. Pair it with the repository's commit trend before deciding."
    question: Does a high bus factor mean a project is safe?
  - answer: "You don't need a GitHub token to read a gitdebt report, and it needs no sign-in. Tokens raise GitHub API limits for tools that call the API directly: one analyzer lists 60 requests an hour unauthenticated and 5,000 an hour with a token."
    question: "Do I need a GitHub token to read a repository's health?"
  - answer: "gitdebt doesn't detect fake stars. It does not flag stars as fake, real or suspicious, and it does not profile stargazers. Use it to read what maintainers did in the commit history, and treat stars as separate context when judging a repository."
    question: Does gitdebt detect fake stars?
publishDate: "2026-11-28"
tags:
  - github
  - repo health
  - dependency review
  - commit history
  - bus factor
  - readme badges
title: "GitHub repo health dashboard: Check if a dependency is alive"
---

**A github repo health dashboard reads [four signals from commit history](https://gitdebt.com/): maintenance (commit cadence), ownership (how few contributors write half the commits), repair load (the fix-labelled share of changes) and debt markers (TODO/FIXME movement). The readings point in a direction and keep stars out of the calculation. gitdebt draws them as [plain image snippets](https://gitdebt.com/badges) you can paste into a README.**

A repository page shows a star count, a green CI badge and a license file. None of those says whether anyone still maintains the code, and the gap matters when you add a dependency, review a vendor's repository or report on a project's traction.

After reading, you can interpret the four signals, run a short check on any public repository, compare candidates and publish the result as an image in your README.

> **Key Takeaways**
> - Judge a dependency on its commit trend, authorship concentration and the gap since its last commits, with repair load as context.
> - A README, license and CI checklist shows that files exist, not that anyone is committing.
> - Decide on commit readings and treat the star curve as separate context.
> - A README chart served from a public image URL needs no token and uses no API quota.

## What does a GitHub repo health dashboard measure?

A GitHub repo health dashboard measures how a repository's code is being worked on, using only its git history, and each of its four readings has a limit that belongs beside it.

![Illustration for What does a GitHub repo health dashboard measure?](https://api.birchseek.com/m/Vev0WUCDXhtOlel3x6-bQPWjwYcXv4CY.webp)

- **Maintenance** is commit cadence over recent months. A quiet repository may be finished rather than abandoned, and a latest-commit timestamp alone hides the trend. Read the monthly shape, not one date.
- **Ownership** counts how few contributors it takes to write half the commits. [CHAOSS](https://chaoss.community/kb/metric-contributor-absence-factor/) publishes related metrics under the names Contributor Absence Factor and [Elephant Factor](https://chaoss.community/kb/metric-elephant-factor/). The reading shows where knowledge has pooled in a codebase; it is not a judgement about any person.
- **Repair load** is the fix-labelled share of changes. It depends on commit-message conventions, so a team that never writes "fix" in a message will read as zero.
- **Debt markers** track TODO/FIXME movement. They count comments and say nothing about how important any one comment is.

### History charts that describe, not diagnose

Three charts sit beside the four readings of a GitHub repo health dashboard, and each one describes history only.

- File change frequency (churn) counts how many analyzed commits touched each file. It identifies frequently revisited files without claiming those changes were unstable or defective.
- Fix-labelled changes ("bug magnets") list files that appear in commits whose message matches a fix heuristic. They describe fix-labelled history and don't prove a bug.
- TODO/FIXME movement shows whether markers were added or removed recently. It does not show whether the code behind them is risky.

### Why a checklist score can't show a project is alive

[GitHub Docs](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/about-community-profiles-for-public-repositories) says the community profile checklist "checks to see if a project includes recommended community health files, such as README, CODE_OF_CONDUCT, LICENSE, or CONTRIBUTING, in a supported location."

None of those files records a commit. A repository can hold every file on the list and have no commits in years, so a checklist percentage measures presence and says nothing about maintenance.

For a dependency check, open the [report for any public repository](https://gitdebt.com/report) and read the four signals with their limits in mind.

## How do you use a GitHub repo health dashboard to check a dependency?

You check whether a dependency is alive by reading its commit history on a GitHub repo health dashboard in four passes: trend, ownership, the gap since the last commits, and repair context.

[Robert Gove's project page](https://rpgove.com/projects/github-dashboard) for his GitHub dashboard frames the problem as which libraries "represent risks because of their activity levels or underlying technologies". Its dashboard shows commits over time and a bus factor chart.

1. **Read the commit trend and heatmap.** Open the repository's report and look at monthly commit volume, then at the commit heatmap for the last 52 weeks. Look for commits spread across many months rather than one burst. Low volume is not death: a finished, stable library can be quiet and still be sound.
2. **Read ownership concentration.** Note how few contributors write half the commits. A count of one or two means knowledge has pooled in a small group. The count describes the codebase, so don't read it as a verdict on someone's work.
3. **Compare the recent gap with the earlier cadence.** Measure the time since the last commits against how often the project committed before. A repository that shipped monthly for years and has been silent for half a year has stopped, while one that always committed twice a year hasn't. A single "latest commit" timestamp can't tell these apart.
4. **Cross-check repair load and TODO/FIXME movement.** A high fix-labelled share or a growing TODO/FIXME count describes history and proves no bugs. Use both as context for steps 1 to 3, then write one word in your notes: alive, slowing, stopped or concentrated.

The result is a short note you can defend, such as "steady months, several authors, no unusual gap: alive."

To run the same four steps on a dependency you're weighing, open `gitdebt.com/owner/repo` with your own slug in place of `owner/repo`.

## Stars vs commits: when do popularity and maintenance disagree?

Popularity and maintenance disagree when the star curve and the commit history move in different directions, because stars record what the crowd did and commits record what maintainers did.

| Criterion | Star curve | Commit history | Presence checklist |
| --- | --- | --- | --- |
| Who acted | Anyone with an account | Maintainers and contributors | The repository owner |
| What it records | Interest over time | Work done on the code | Whether files exist |
| Shows the project is alive | No | Directional, with limits | No |
| Feeds the four health readings | No | Yes | No |

A steep star curve beside a flat or fading commit trend means many people bookmarked the repository while few changes landed. A flat star curve beside steady monthly commits means a small audience and ongoing upkeep. Both patterns describe history and neither rates quality.

A star is a single click and says nothing about the work that follows it.

gitdebt does not label stars as fake, real or suspicious. It does not profile stargazers: it stores star dates and day counts only. It does not analyze private repositories.

## How do you compare repository health across several repos?

You compare repository health across several repos by recording the same four readings for each candidate, drawing their star histories on a separate overlay, and writing one decision note per candidate from the commit evidence.

1. **List the candidates as `owner/repo` slugs.** Keep the shortlist to eight or fewer, the most the overlay in step 3 takes. Copy each slug from its GitHub URL so you don't analyze a fork by mistake.
2. **Open each report and record the same four readings.** Use one table row per candidate and four columns: maintenance, ownership, repair load and debt markers. Aim for a grid with no blank cells.
3. **Overlay the star histories.** Pick days since first star when the candidates differ in age, so each curve starts at its own day zero. Pick the log scale when sizes differ widely, or the smaller curves flatten against the bottom axis.
4. **Keep the two sources apart.** Decide on the commit readings and use the overlay only as context.
5. **Write a decision note per candidate.** Two sentences are enough, for example: "Commits steady for the last year, half of them from two authors, safe to adopt with a pinned version." Write the note from your table row, not from the star curve.

The curated [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks) works as a template for a shortlist. Open it to see a finished overlay, then run steps 2, 4 and 5 on each repository it contains.

Candidates that differ in age, scope or commit conventions aren't strictly comparable on a GitHub repo health dashboard. The fix-labelled share depends on how each team words its commit messages, so one project's higher share may reflect its writing style. Read differences between candidates as direction rather than a ranking.

## Where does a GitHub repo health dashboard get its data?

A GitHub repo health dashboard gets its data from one of two places: GitHub's REST API, which returns repository metadata, or the repository's own git history, which records every commit.

The API route has a ceiling. The [Hexabench repo analyzer](https://www.hexabench.dev/github-repo-analyzer) reports a limit of 60 requests per hour without a token and 5,000 per hour with one. [GitHub's rate-limit docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) say that exceeding the primary limit returns a 403 or 429 response, with the `x-ratelimit-remaining` header at 0.

A full-history analysis needs many requests per repository, so anonymous visitors run out quickly. Many tools therefore hold deeper analysis behind a token field.

Pasting a token into a third-party page grants that page access on your behalf. [GitHub's documentation on access tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens) says fine-grained personal access tokens "have several security advantages over personal access tokens (classic)", so a fine-grained token is the safer one to hand over.

GitHub's own [Insights tab](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/about-repository-graphs) already draws graphs from this data. A commit-history report starts from the same raw material and adds derived readings: how few contributors write half the commits, the fix-labelled share of changes and TODO/FIXME movement.

The git route avoids the API quota. gitdebt's [methodology page](https://gitdebt.com/about) describes computing health signals from a shallow clone, analyzed server-side.

A shallow clone holds a limited slice of history, not necessarily every commit ever made. Check the commit trend again before you act on a single number.

## How do you add a GitHub repo health dashboard to a README?

You add a GitHub repo health dashboard to a README by pasting a `<picture>` snippet that points at public image URLs, so it renders on GitHub without JavaScript, a hosted app or a token.

1. **Open the embed catalog.** Pick the health charts there for commit-based readings. The snippet below uses `chart.svg`, the star-history chart, which shows what the crowd did, so keep it as separate context.

2. **Copy the snippet and swap `OWNER/REPO`.** Replace every occurrence, including the link and the alt text.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
    <img alt="Star history chart for OWNER/REPO" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

3. **Keep the `?ref=readme` link around the image.** Dropping the wrapping `<a>` removes the click-through to the full report.

4. **Commit the change to `README.md` and reload the repository page.** The chart should follow the reader's light or dark theme.

A repository nobody has analyzed yet renders a placeholder and queues the work. The real chart replaces it at the same URL, so you don't edit the README again.

Images are static by default, and adding `animate=1` opts an SVG into motion.

## Frequently asked questions

### What is a GitHub repo health dashboard?

A GitHub repo health dashboard turns a repository's commit history into four readings: how steadily it's maintained, how many contributors write half the commits, how much of the work is fix-labelled, and whether TODO/FIXME markers are rising. Read them as directional signals rather than a score.

### Is a repository with many stars healthy?

A high star count doesn't make a repository healthy, because stars show popularity while commits show maintenance. A popular repository can have a stalled commit trend, and a small one can have commits in every recent month, so check both separately.

### Does a high bus factor mean a project is safe?

A high bus-factor reading doesn't make a project safe or unsafe. Ownership concentration shows where knowledge has pooled in a codebase and is not a judgement about any person, so pair it with the commit trend before deciding.

### Do I need a GitHub token to read a repository's health?

Tokens raise GitHub API limits for tools that call the API directly: one analyzer lists 60 requests an hour unauthenticated and 5,000 an hour with a token.

### Does gitdebt detect fake stars?

gitdebt doesn't detect fake stars. It does not flag stars as fake, real or suspicious, and it does not profile stargazers. Use it to read what maintainers did in the commit history, and treat stars as separate context.

## What should you do next with a GitHub repo health dashboard?

- Today, pick the one dependency you're least sure about and run the four-step check on it: commit trend, ownership, the gap since its last commits, then repair context.
- This week, shortlist the candidates for any upcoming choice, fill in one row of readings per repository and overlay their star histories separately.
- This month, add the dashboard image to your own README with the `<picture>` snippet from the [README badges and embeds page](https://gitdebt.com/badges).

Start with today's dependency: [analyze any public repository](https://gitdebt.com/report) at gitdebt.com/report.

## Sources

[gitdebt: GitHub star history and repository health](https://gitdebt.com/) · [README badges, star-history charts and embeds](https://gitdebt.com/badges) · [CHAOSS Metric: Contributor Absence Factor](https://chaoss.community/kb/metric-contributor-absence-factor/) · [About community profiles for public repositories - GitHub…](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/about-community-profiles-for-public-repositories) · [Robert Gove - GitHub Dashboard for Analyzing Repo Health](https://rpgove.com/projects/github-dashboard) · [GitHub Repo Analyzer — Health & Dependencies | Hexabench](https://www.hexabench.dev/github-repo-analyzer) · [Rate limits for the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) · [Managing your personal access tokens - GitHub Docs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens). Every other source is linked where the article cites it.
