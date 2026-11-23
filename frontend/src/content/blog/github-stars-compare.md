---
description: How do you run a GitHub stars over time compare fairly? Fix one date window, choose alignment and scale, then check commits. Compare up to eight repos now.
faq:
  - answer: "Put the repositories on one chart with the same date window, switch the axis to days since each repository's first star if their ages differ, and use a log scale when their sizes differ widely. Then check commit history, because stars alone don't show maintenance. The compare view has all three controls."
    question: How do you compare GitHub stars over time?
  - answer: "A repository's GitHub star count can go down, because a star series is net of unstars. When someone removes a star, the running count of stars drops, so a curve can dip or flatten without any change in the project itself."
    question: "Can a repository's GitHub stars go down?"
  - answer: "You don't need a token or a sign-in to see star history on gitdebt. The chart embeds on gitdebt also need no account, token or API key at all, so you can open a report or copy a README snippet straight away."
    question: Do you need a token to see GitHub star history?
  - answer: A high star count does not show that a project is maintained, because a star curve records crowd attention and cannot show maintainer work. Read recent commit cadence, ownership and repair load before you depend on a repository. gitdebt does not flag stars as fake, real or suspicious.
    question: Does a high star count mean a project is maintained?
publishDate: "2026-11-23"
tags:
  - github stars
  - star history
  - repository comparison
  - commit history
  - open source metrics
title: "GitHub stars over time compare: Set window, alignment, scale"
---

**To run a fair github stars over time compare, make three choices: window (the same start and end dates for every repository), alignment (calendar dates or days since each repository's first star), and scale (linear or log). Then check each commit history, because stars measure what the crowd did and commits measure what maintainers did.**

Put [angular/angular's star history](https://gitdebt.com/angular/angular) beside `vitejs/vite` on a calendar axis and the older curve sits higher, because it has had years more time to collect stars. The chart shows which repository is older. It can't show which one grew faster.

Maintainers, dependency choosers and devtool marketers run into this problem whenever they compare traction. After reading, you can build a fair comparison, read the shape of a curve, and know when stars stop being useful evidence.

> **Key Takeaways**
> - A star series counts current stars, so it can fall when people remove stars.
> - A spike shows that attention arrived. It doesn't show why, or whether it stayed.
> - Equal-start alignment compares early growth; log scale compares growth rates. Pick the one that matches your question.
> - Velocity windows of 1, 7 or 30 days describe recent momentum, not long-term adoption.

## What does a GitHub stars-over-time chart actually measure?

A GitHub stars-over-time chart measures how many stars a repository holds on each date, net of the stars users have removed.

![Illustration for What does a GitHub stars-over-time chart actually measure?](https://api.birchseek.com/m/O7pkd1tstMFbxii9kkjN4BzpgHAmVPVP.webp)

A GitHub star is a bookmark and a public signal of interest in a repository, according to [GitHub Docs on saving repositories with stars](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars). The same page says you can star repositories "to keep track of projects you find interesting and discover related content in your news feed."

A star history plots the running count by date. When a user removes a star, the count falls, so the curve is not guaranteed to rise.

Because anyone can unstar, a day can end below the day before. A flat stretch or a small dip is a normal reading.

The series gitdebt draws follows this definition. Its [methodology page](https://gitdebt.com/about) describes the star series as exact daily counts, net of unstars.

A star records interest from one account. It says nothing about usage, adoption or quality, so a star from someone who never installs the code counts the same as a star from a daily user.

The star curve is one source of evidence about what the crowd did. What maintainers did shows up in the commit history, which is a separate record.

## How do you compare GitHub stars over time fairly?

You compare GitHub stars over time fairly by fixing the window, the alignment and the scale before you read any curve. The [gitdebt compare view](https://gitdebt.com/compare) sets all three on one page.

1. Set one date window. Enter the same inclusive from and to dates for every repository. If one repository is cut off earlier than another, its curve looks like slower growth when it is only a shorter window.
2. Choose the alignment. Use calendar dates to ask who was bigger when. Use days since each repository's first star to ask who grew faster from launch. Reading calendar dates as a growth race is a common mistake, because the older repository has had more time.
3. Choose the scale. Use linear to see absolute gaps. Use log when the repositories differ widely in size, so that the slopes show growth rates. A wide gap on a log axis means a large ratio, not a large count.
4. Overlay the repositories. Add up to eight on one set of axes.
5. Write down the one question the comparison should answer.

For a worked comparison, enter `vitejs/vite` and `vercel/next.js` and give both the same window. The calendar view answers which repository was larger at any date. It also shows that the two started collecting stars in different years.

Next, switch the axis to days since first star and compare the first 90 days. That view answers which project gained attention faster after launch, whatever year each one began.

Then switch to log scale. A gap that looks wide on the linear axis turns into a comparison of slopes, and roughly parallel lines mean similar proportional growth.

A finished comparison shows two curves that start from the same footing and answer one named question.

[Compare your own repositories on gitdebt](https://gitdebt.com/compare).

## Where does the data to compare GitHub stars over time come from?

GitHub star history data comes from GitHub's REST API, which lists a repository's stargazers one page at a time and returns the date of each star only when you ask for a specific media type.

The [GitHub REST docs for starring](https://docs.github.com/en/rest/activity/starring) describe the `application/vnd.github.star+json` media type, which "includes a timestamp of when the star was created." Send it in the `Accept` header, and each entry carries a `starred_at` date.

The same docs cap a page at 100 results ("max 100"). A request for the first page looks like this:

```bash
curl -H "Accept: application/vnd.github.star+json" \
  "https://api.github.com/repos/OWNER/REPO/stargazers?per_page=100&page=1"
```

Rate limits set the pace.

Those limits are why some tools ask for a token. The command-line tool [dtolnay/star-history](https://github.com/dtolnay/star-history) states: "We require a token for accessing GitHub's GraphQL API."

Paging makes large repositories slow, because each request returns at most 100 stars and every request counts against the rate limit.

Tools that rebuild a history in your browser can be slow, may truncate the series, or may ask for your token.

gitdebt reads from GitHub's own star history instead. Older series are built from historical data and re-read from GitHub's star history as gitdebt reaches them. Every chart names its source, coverage date and state.

## How do you read spikes, plateaus and steady slopes in star history?

Read a spike as attention arriving in a short window, a plateau as few new stars arriving, and a steady slope as consistent inflow; none of the three shapes explains why.

A spike is a near-vertical jump. It can't tell you why the attention arrived or whether it stayed, and the curve alone can't name a launch channel as the cause.

To investigate a spike, zoom the date window around the jump and check whether the slope afterward returns to the earlier rate. Then look at commits in the same window, which record what maintainers did while the crowd was starring.

A plateau is a flat stretch. It can't tell you the project is abandoned or unused, because a mature library can be installed daily and still gain few stars. Only the commit history speaks to maintenance.

A steady slope is a constant incline. It shows that people keep starring at a similar rate, whatever their reasons, and it can't tell you the project is good.

A curve can also bend downward. Each removed star lowers the count by one, so a dip means more stars were removed than added in that period.

Short-window views show recent momentum. Windows of 1, 7 and 30 days, such as those ranked on the [star velocity leaderboard](https://gitdebt.com/leaderboard), describe the latest stretch of the curve.

Those windows say little about long-term adoption, since a repository can rank high over 7 days and flatten the month after.

## What can a star curve not tell you about whether a project is maintained?

A star curve can't tell you whether a project is maintained, because it records what the crowd did and says nothing about what the maintainers did.

| Signal | Star history | Commit history | Repository traffic |
| --- | --- | --- | --- |
| Measures | Crowd attention, net, dated | Maintenance, ownership, repair load, debt markers | Visits and clones |
| Source | Star dates from GitHub | Git history | GitHub Insights |
| Who can see it | Anyone | Anyone, for a public repository | Push access |
| Cannot tell you | Why attention arrived, or whether it stayed | Code quality, or anyone's worth | Anything outside its window |

The commit history is an independent source. Read it beside the star history and never blend the two into one score.

[gitdebt](https://gitdebt.com/) pairs a star history with four health readings from the commit history: maintenance, ownership, repair load and debt markers.

Ownership concentration shows where knowledge has pooled in a codebase. It is not a judgement about any person.

Repository traffic is a third signal, and a narrow one. [GitHub Docs on repository traffic](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) says anyone with push access can view a repository's traffic, including visitors from the last 14 days.

Restricted visibility and a short window rule traffic out as a replacement for a public star history.

When a steep star curve sits beside a flat or falling commit trend, or the reverse, open the commit history and look closer. Health readings are directional signals, not verdicts.

Each signal fits a different job:

- Star history: judging how much attention a repository drew, and when.
- Commit history: judging whether anyone is still maintaining the code.
- Repository traffic: maintainers checking recent visits to their own repository.

## Can fake stars distort how you compare GitHub stars over time?

Yes, fake stars can distort a comparison, because a star-history chart records when each star was added and says nothing about who added it or why.

A curve can't separate purchased or automated stars from genuine interest. A chart can show that many stars arrived on one Tuesday, and it can't show whether those accounts came on their own.

According to its [methodology page](https://gitdebt.com/about), gitdebt does not flag stars as fake, real or suspicious, and it doesn't score accounts. Its position is that telling manipulated stars apart correctly needs privileged access to GitHub's full dataset, which a public star-history tool doesn't have.

An isolated spike is a reason to check other evidence. Match the spike date against a release, a launch post or a conference talk you can find yourself, then check whether the slope afterward stays flat or resumes.

Stars also say nothing about upkeep. A curve can keep rising while commits slow, and a flat curve can belong to a busy project.

Open each repository's [report](https://gitdebt.com/report) and read the maintenance reading from its commit history beside the stars.

## How do you add a star history comparison to a README?

A star history comparison goes into a README as one image URL wrapped in a link, and a `picture` element lets a single snippet carry both a light and a dark drawing. The [README embeds on the gitdebt badges page](https://gitdebt.com/badges) use this pattern.

1. Copy the snippet below into `README.md`.
2. Replace `OWNER/REPO` in all four places. A missed one points the image at the wrong repository.
3. Keep the `?ref=readme` link around the image so the chart opens its report.
4. Commit and open the README on GitHub. The chart should render in both themes.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
    <img alt="Star history chart" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

The SVG is static by default. Add `animate=1` to opt into motion, or swap `chart.svg` for `chart.gif`.

A repository nobody has analyzed yet renders a placeholder and queues the work, and the real chart replaces it at the same URL.

## Frequently asked questions

### How do you compare GitHub stars over time?

Put the repositories on one chart with the same date window, switch the axis to days since each repository's first star if their ages differ, and use a log scale when their sizes differ widely. Then check commit history, because stars alone don't show maintenance. The [compare view](https://gitdebt.com/compare) has all three controls.

### Can a repository's GitHub stars go down?

Stars can go down, because a star series is net of unstars. When someone removes a star, the running count drops, so a curve can dip or flatten without any change in the project itself.

### Do you need a token to see GitHub star history?

Tools that rebuild star history through GitHub's API often ask for a token, because unauthenticated requests are rate limited.

### Does a high star count mean a project is maintained?

A star curve records crowd attention, not maintainer work, so it can't show whether a project is maintained. Read recent commit cadence, ownership and repair load before you depend on a repository.

## Next steps: compare your repositories this week

A github stars over time compare takes three passes, and the first fits in one sitting.

Today, pick 2 to 8 repositories and put them in one window, with the same start and end dates for every repository.

This week, switch the axis to days since first star, then try log scale if the sizes differ widely. Open each repository's report and read the commit-based health readings next to the curve.

After that, add a README snippet from the [badges page](https://gitdebt.com/badges), so readers see the same comparison you checked.

Run the comparison on the [compare page](https://gitdebt.com/compare), which overlays up to eight on one set of axes, or [analyze a repository](https://gitdebt.com/report) on its own.

## Sources

[Saving repositories with stars - GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring) · [GitHub - dtolnay/star-history](https://github.com/dtolnay/star-history) · [GitHub star velocity leaderboard](https://gitdebt.com/leaderboard) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/) · [Viewing traffic to a repository - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) · [README badges, star-history charts and embeds](https://gitdebt.com/badges). Every other source is linked where the article cites it.
