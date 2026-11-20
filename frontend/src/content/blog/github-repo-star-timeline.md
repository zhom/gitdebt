---
description: A GitHub repo star timeline shows cumulative stars, net of unstars. Learn to read spikes, add a README chart and compare repos by age. Start signed out.
faq:
  - answer: Open gitdebt.com/owner/repo, or enter owner/repo at the gitdebt report page, and the star timeline loads without any sign-in. The chart plots cumulative stars by date, can switch to an axis of days since first star, and names its source and coverage date.
    question: How do I see the star timeline of a GitHub repository?
  - answer: "You do not need a GitHub token to chart stars on gitdebt, because its charts work when you are signed out. A token matters only if you call GitHub's API yourself, because authenticated and unauthenticated requests to that API have different rate limits."
    question: Do I need a GitHub token to chart stars?
  - answer: "A repository's star count goes down when people unstar it. The gitdebt timeline is net of unstars, so the line plotted on the chart can dip as well as climb, and any dip shows stars that people removed after adding them."
    question: "Why did a repository's star count go down?"
  - answer: Switch the chart axis to days since first star to compare repositories of different ages. Each repository then starts at day zero, so an older and a newer project can be read over the same first 90 days on the compare page.
    question: How do I compare repositories of different ages?
  - answer: gitdebt does not detect fake stars. It does not flag stars as fake, real or suspicious, and it does not score individual accounts. The timeline shows the dates on which stars arrived on the repository and does not identify who added them.
    question: Does gitdebt detect fake stars?
publishDate: "2026-11-20"
tags:
  - github stars
  - star history
  - readme chart
  - repo comparison
  - open source
title: "GitHub repo star timeline: How to read, embed and compare it"
---

**A github repo star timeline is the cumulative count of a repository's stars over time, net of unstars. It records what the crowd did and says nothing about whether maintainers are active. You can read it, embed it in a README or compare it with other repositories, and [gitdebt's report page](https://gitdebt.com/report) shows it signed out, with no account needed.**

Maintainers adding a chart to a README and developers deciding whether to depend on a library usually start from a bare star count rather than a GitHub repo star timeline. Stars say a repository is popular. Commits say whether it is alive.

With the steps here, you can date the shape of a GitHub repo star timeline, put an accurate chart in your README and compare repositories of different ages on equal terms.

> **Key Takeaways**
> - A star curve shows when stars arrived; releases and commits from the same week suggest why.
> - Align repositories to days since first star, or the oldest one looks strongest by default.
> - Reading dated stars from GitHub's API yourself is slow without a token, because unauthenticated requests are tightly rate limited.
> - gitdebt does not flag stars as fake, real or suspicious; check commits and downloads instead.

## What is a GitHub repo star timeline and where does the data come from?

A GitHub repo star timeline plots a repository's cumulative stars against time, and its dated data comes from GitHub's stargazers REST endpoint. That endpoint returns star timestamps through a dedicated media type, in pages, so large repositories take many requests.

![Illustration for What is a GitHub repo star timeline and where does the data come from?](https://api.birchseek.com/m/pRX9ieCA_ZMgnv1bJXnIpX9F9aOUJgAn.webp)

The endpoint is `GET /repos/{owner}/{repo}/stargazers`. [GitHub's REST documentation](https://docs.github.com/en/rest/activity/starring) says it lists the people who have starred a repository, and that the `application/vnd.github.star+json` media type "includes a timestamp of when the star was created." Without that header you get the stargazers but no dates.

Dated star lists are not limited to people with access to the repository. The same documentation says the endpoint "can be used without authentication if only public resources are requested."

The request for `vercel/next.js` looks like this:

```bash
curl -H "Accept: application/vnd.github.star+json" \
  "https://api.github.com/repos/vercel/next.js/stargazers?per_page=100&page=1"
```

Each entry in the response carries a `starred_at` field next to the user. The documentation caps a page at 100 results ("The number of results per page (max 100)"). If a repository had 100,000 stars, a full read would need 1,000 requests.

Rate limits set how long those requests take. [GitHub's rate limit documentation](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) says "The primary rate limit for unauthenticated requests is 60 requests per hour," while authenticated requests get 5,000 requests per hour. At 60 per hour, the 1,000-request read would take more than 16 hours.

gitdebt reads GitHub's own star history as exact daily counts, net of unstars, back to repository creation. Older series use historical data that is re-read from GitHub's star history as gitdebt reaches them.

Every chart names its source, coverage date and state, so a reader can see how current the timeline is. The [methodology page](https://gitdebt.com/about) describes each source.

## How do you read a GitHub repo star timeline without over-reading it?

You read a GitHub star timeline by sorting its shape into a spike, a plateau or a slow slope, then checking that shape against the project's own history. The curve shows when stars arrived. It cannot show why they came.

- **Spike:** a steep rise over a few days. It can correspond to a launch, a release, a mention on a popular site or a day on GitHub's Trending page. The same jump can follow any of these, so the shape alone names none of them.
- **Plateau:** a flat stretch. It can correspond to a quiet period or to a mature project that most interested people already know. Flat means few new stars and says nothing about whether the code is maintained.
- **Slow slope:** a steady climb, which can correspond to organic discovery over months or years. A star does not measure use, so a slope does not show that more people run the code.

Each shape on a GitHub repo star timeline points to something worth checking:

1. Mark the date. Note the first day of the rise on a calendar axis rather than the day of the peak. Starting at the peak is the usual mistake, because it points you at the wrong week of history.
2. Check the project's own records. Open the releases page and that week's commits on GitHub, and look for a tagged release, a README rewrite or a burst of commits. The commit trend is a separate source from the star curve, so a match between them is a lead to follow up.
3. Change the view. Days since first star lines repositories up at the same age, so `facebook/react` and `vitejs/vite` can be compared over their first 90 days. A log scale makes early growth readable, and a linear scale shows which late gains were large. The [compare page](https://gitdebt.com/compare) overlays repositories this way.
4. Write the reading down. A good note says "the rise starts the day of the 2.0 release" and stops there. If nothing in releases or commits lines up with the date, leave the cause open.

The result is one dated note per shape on the GitHub repo star timeline, each tied to a record you checked, with a cause claimed only where a record supports it.

## How do you add a GitHub repo star timeline to your README?

You add a star history chart to a README by pasting one HTML snippet that points at a plain image URL, then swapping in your own `OWNER/REPO`. The current snippet for every asset is on [gitdebt's README badges and embeds page](https://gitdebt.com/badges); the star history version looks like this:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <img alt="Star history chart for OWNER/REPO" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

1. Open your README.md on GitHub (pencil icon) or in your editor, and pick the spot for the chart, usually under the project description.
2. Paste the snippet. The `<source>` carries the `theme=dark` drawing, and the `<img>` is the `theme=light` fallback.
3. Replace `OWNER/REPO` in all three places: the link, the `srcset` and the `src`. Replacing only one leaves a chart for the wrong repository. Keep `?ref=readme` on the link around the image.
4. Commit and reload the repository page. The chart should appear under your description; switch GitHub's appearance setting between light and dark to confirm the matching version loads. If the repository hasn't been analyzed yet, you'll see a placeholder, and the real chart replaces it at the same URL once the work finishes, with no README edit.
5. Add options only if you need them. Append `animate=1` to an SVG URL to opt into motion. Swap `chart.svg` for `chart.gif` where a GIF is required.

The snippet works in a README because each GitHub repo star timeline chart is a plain image URL.

## How do you compare repos on a GitHub star timeline fairly?

You compare repositories fairly on a GitHub star timeline by aligning each one to days since its first star instead of the calendar date. A repository that is 10 years old and one that is 2 years old can't share a calendar axis, because the older line carries years of history the younger one hasn't lived yet.

The comparison follows these steps:

1. Overlay the repositories. Open [gitdebt's compare page](https://gitdebt.com/compare) and add your repositories as `owner/repo`. The page overlays up to eight on one set of axes. Add only the repositories your question needs, because eight crowded lines are harder to read than three.
2. Switch the axis to days since first star. Every line now starts at day 0. Staying on the calendar view rewards whichever repository is oldest.
3. Compare the first 90 days. Look at slope and shape: a steady climb, an early spike or a slow start. Where each line ends matters less than how it got there.
4. Switch to log scale if one repository dwarfs the others. On a linear axis the largest line flattens the rest into the floor. On a log axis, equal percentage growth shows as equal slope, so read slopes there as rates of growth.
5. Set a date window and export. Choose an inclusive from and to date to isolate a launch period or a quarter.

Done well, every GitHub repo star timeline begins at the same left edge. You can then describe the differences in plain words.

For a worked example, add `facebook/react`, `vercel/next.js` and `vitejs/vite`. On the calendar axis the three lines begin in different years. After you switch to days since first star, they share day 0, and you can read which repository had the steepest first 90 days.

If `facebook/react` towers over the others, switch to log scale to see the early curves.

On a GitHub repo star timeline, equal-start alignment compares growth shape only. Code quality, adoption and maintainer activity are questions for the commit history.

## How does a star timeline compare with a repository's commit history?

A star timeline records what the crowd did, and a repository's commit history records what the maintainers did, so each one answers a question the other can't.

The [angular/angular report](https://gitdebt.com/angular/angular) shows the pairing: star history with commit-based health readings beside it. The two come from separate sources and are never blended into one score.

To read them together, work down the page:

1. Find a rise, plateau or dip on the star curve and note the months it covers.
2. Check the commit trend (monthly commit volume) for those same months.

[gitdebt's four headline readings](https://gitdebt.com/) come from the git history, not from stars:

- Maintenance: how recently and how regularly commits land.
- Ownership: how few contributors wrote half the commits.
- Repair load: the share of changes carrying a fix label.
- Debt markers: movement in TODO and FIXME comments.

Each reading is a directional signal. Ownership shows where knowledge has pooled in a codebase; it is not a judgement about any person.

Fix-labelled changes describe fix-labelled history. A file that appears often in them isn't proven to contain a bug.

The useful cases are the ones where the two sources disagree. A steep star curve beside a flat commit trend means attention arrived without matching commit activity. A launch or a popular mention could explain it, and so could a finished project that needs few changes.

Steady commits under a flat GitHub repo star timeline mean work is continuing without new attention.

Either disagreement tells you where to look next: release notes, open issues or recent pull requests.

## Can a GitHub repo star timeline contain bought or inflated stars?

Yes, a GitHub repo star timeline can contain bought or inflated stars, because the curve records when stars arrived and nothing in its shape shows who added them or why.

A preprint on arXiv is titled ["4.5 Million (Suspected) Fake Stars in GitHub: A Growing Spiral of Popularity Contests, Scams, and Malware"](https://arxiv.org/abs/2412.13459). The title calls the stars "suspected" and ties the practice to popularity contests, scams and malware. The paper itself sets out the method and findings.

As [gitdebt's methodology page](https://gitdebt.com/about) states, gitdebt does not flag stars as fake, real or suspicious, and it does not score accounts.

The practical response is to check stars against independent signals:

- Commit-based readings: the commit trend and commit heatmap show whether maintainers are working, whatever the star curve says.
- Package downloads: counts from npm, crates.io, PyPI or Docker come from a different system, so a steep star climb with flat downloads deserves a closer look.
- Your own history check: a spike that matches a release or launch is easier to explain than one with no visible cause.

None of these proves anything alone, but together they make a GitHub repo star timeline harder to misread.

Velocity rankings carry the same limit. The [star velocity leaderboard](https://gitdebt.com/leaderboard) ranks repositories by stars gained over recent 1, 7 and 30-day windows, so a repository near the top has gained stars quickly. The ranking does not say why, and a short window magnifies any sudden burst.

## Frequently asked questions

### How do I see the star timeline of a GitHub repository?

Open `gitdebt.com/owner/repo`, or enter owner/repo at the [report page](https://gitdebt.com/report). The chart plots cumulative stars by date, switches to days since first star, and names its source and coverage date.

### Do I need a GitHub token to chart stars?

Not for gitdebt charts, which work signed out. A token matters only if you call GitHub's API yourself, where authenticated and unauthenticated requests have [different rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api).

### Why did a repository's star count go down?

The timeline counts stars net of unstars. When people remove stars, the line can dip as well as climb.

### How do I compare repositories of different ages?

Switch the axis to days since first star. Each repository then starts at day zero, so an older and a newer project can be read over the same first 90 days on the [compare page](https://gitdebt.com/compare).

### Does gitdebt detect fake stars?

gitdebt does not flag stars as fake, real or suspicious, and it doesn't score individual accounts. The timeline shows when stars arrived, not who added them.

## What should you do next with a GitHub repo star timeline?

Next, open one repository's report today, add its chart to your README this week, and check its commits against its stars each month.

- Today: open the report for a repository you depend on or maintain, and date each spike, plateau and slow slope against its releases.
- This week: paste the light and dark snippet into your README. Then compare the repository against two peers by days since first star.
- Each month: check the commit trend beside the star curve. If stars climb while commits thin out, take that question to the maintainers.

[Analyze your repository on the gitdebt report page](https://gitdebt.com/report).

## Sources

[REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring) · [Rate limits for the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [README badges, star-history charts and embeds](https://gitdebt.com/badges) · [Compare GitHub star history](https://gitdebt.com/compare) · [angular/angular — star history & stats · gitdebt](https://gitdebt.com/angular/angular) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/) · [4.5 Million (Suspected) Fake Stars in GitHub: A Growing…](https://arxiv.org/abs/2412.13459). Every other source is linked where the article cites it.
