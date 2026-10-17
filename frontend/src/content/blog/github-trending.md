---
description: "What does GitHub Trending tell you? Only attention in a window, not upkeep. Learn to check a repo's star history, then its commits, before you depend on it."
faq:
  - answer: GitHub Trending is the page at github.com/trending that lists repositories drawing attention today. A separate developers page lists people. It shows attention in a window, not quality or upkeep. In other words, the page highlights what is currently attracting interest, not whether a repository is well maintained or high quality.
    question: What is GitHub Trending?
  - answer: "It's a figure GitHub prints beside each repository, labeled 'stars today'. GitHub's page doesn't state the basis for it, so read it as attention in a window, not as quality. You should not treat this number as proof that a repository is good, because the method behind it is not explained."
    question: "What does \"stars today\" mean on GitHub Trending?"
  - answer: "GitHub does not publish how trending repositories are chosen on the trending page itself. It shows a list and a 'stars today' figure without stating a ranking method, so treat the order as unexplained and date any figure you cite."
    question: Does GitHub publish how trending repositories are chosen?
  - answer: "A repository can reach GitHub Trending with fake stars because that can't be ruled out. A 2024 arXiv study examined fake stars on GitHub, and GitHub's Acceptable Use Policies prohibit rank abuse such as automated starring. Neither source says which trending entry is affected, so you cannot identify a specific case from this."
    question: Can a repository reach GitHub Trending with fake stars?
  - answer: "Gitdebt does not tell you whether a trending repository's stars are fake. It does not flag stars as fake, real or suspicious, and it stores only star dates and daily counts, never stargazer profiles. It shows the shape of a star history beside commit-based health readings, which helps you ask better questions but does not settle star authenticity."
    question: "Does gitdebt tell me whether a trending repository's stars are fake?"
publishDate: "2026-10-17"
tags:
  - github trending
  - star history
  - commit history
  - repository health
  - open source
title: "GitHub Trending: how to check a repo before you depend on it"
---

**[GitHub Trending](https://github.com/trending) is the page at github.com/trending that lists repositories drawing attention today, with a [separate page for developers](https://github.com/trending/developers). A "stars today" gain shows what the crowd noticed, not quality or upkeep. Stars are what the crowd did; commits are what maintainers did, so check the star history shape, then the commit history.**

A repository is climbing github trending today, and you have to decide whether to try it, depend on it or write about it. The list shows what's getting attention. It doesn't show whether anyone maintains the code.

After reading, you can compare trending sources by what each says about itself. You can also run a two-step check on any entry: the shape of its star history first, then its commit history.

> **Key Takeaways**
> - Every trending list is a timestamped snapshot, so date any star total or "stars today" figure you cite.
> - Switch the star-history chart to days since first star and compare the first weeks. A spike falls away; sustained growth keeps climbing.
> - gitdebt does not flag stars as fake, real or suspicious.
> - Commit-based health readings are directional signals, not verdicts.

## What is GitHub Trending and how do you use it?

You use GitHub Trending by narrowing its list with the language and date-range controls, then treating each entry as a lead: the page measures attention in a window, not code quality or maintenance.

![Image generated](https://api.birchseek.com/m/9Lh9MmHkk1fgLYzgb8-J2dkFhT6XgsH3.webp)

**The repositories page.** GitHub describes it as "See what the GitHub community is most excited about today." Each entry carries a "stars today" label. Read the date range and language options on [the live trending page](https://github.com/trending), because they can change and a screenshot goes stale.

**How the list is computed.** Neither the repositories page nor the developers page states how the list is built or what the "stars today" figure is based on. Treat it as a label, not a formula, and don't assume a start time, a threshold or a weighting.

**The developers page.** At [github.com/trending/developers](https://github.com/trending/developers), GitHub says "These are the developers building the hot tools today." It lists ranked people, each paired with one popular repository.

A developer's rank points to one repository. It says nothing about the health of that person's other repositories, and it isn't a judgment about the person.

gitdebt reads ownership the same way: it shows where knowledge has pooled in a codebase, not how anyone performs.

To check what a trending repository's commits say, paste its `owner/repo` into [a gitdebt report](https://gitdebt.com/report). No account is needed.

## What does a repository on GitHub Trending actually tell you?

A repository on GitHub Trending tells you one thing: it drew a burst of attention in a stated window. GitHub prints a figure such as "stars today" beside each entry on [its trending page](https://github.com/trending). That figure doesn't measure quality, fitness for your use, or whether anyone maintains the code.

A trending entry records what the crowd did. It says nothing about what the maintainers did.

Every list is also a timestamped snapshot. Each page captures its numbers at its own moment, so the same repository can carry different star totals and descriptions on different pages.

When two pages disagree, check their timestamps before you assume an error.

Three habits keep you from over-reading an entry:

- **Date every figure.** Write the day next to any star total or one-day gain you cite, or link to a live page instead of copying the number.
- **Read the README first.** It's the primary source. Treat any third-party summary as a pointer to it.
- **Check the maintainers' side.** Open the repository's [live report](https://gitdebt.com/report) and read the commit history and maintenance reading next to the star curve. The [star velocity leaderboard](https://gitdebt.com/leaderboard) shows gains over 1, 7 and 30 days, so you know which window a gain covers.

These readings are directional. They show where to look next, not whether to adopt the code.

## Where else can you see trending repositories?

GitHub's page is the source, and third-party trackers such as [Trendshift](https://trendshift.io/), [gittrend.io](https://gittrend.io/) and [githot.dev](https://githot.dev/en) publish their own daily lists with their own extras.

Before you lean on one, look for a page that explains how it ranks and how often it refreshes. If you can't find one, treat its order as unexplained, the same way you treat GitHub's.

[gitdebt's leaderboard](https://gitdebt.com/leaderboard) is one more option: it ranks tracked repositories by weekly pace and shows the stars gained over 1, 7 and 30 days. It covers only repositories gitdebt has analyzed, and a star gain still says nothing about upkeep.

Use any of these as a lead list, then check the star history and commit history in a [gitdebt report](https://gitdebt.com/report).

## Can you trust the stars on a trending repository?

A trending position rests on new stars, and stars can be bought or gamed, so a large ["stars today" figure](https://github.com/trending) doesn't prove genuine interest.

GitHub states its position in its [Acceptable Use Policies](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies). Under spam and inauthentic activity, the policies list "Rank abuse, such as automated starring or following" and "Inauthentic interactions, such as fake accounts and automated inauthentic activity".

Researchers have studied the problem too. A [2024 arXiv paper](https://arxiv.org/abs/2412.13459) is titled "4.5 Million (Suspected) Fake Stars in GitHub: A Growing Spiral of Popularity Contests, Scams, and Malware".

The word "suspected" is the authors' own, and the figure is their result, not a gitdebt result.

gitdebt does not flag stars as fake, real or suspicious. It stores star dates and day counts only, as [gitdebt's methodology page](https://gitdebt.com/about) explains. Read the shape of the star history and the commit history instead.

## How do you check a trending repository before you depend on it?

You check a trending repository by reading two independent records in order, the star history and then the commit history, and then reading the README and recent commits yourself. The whole pass works signed out, with no repository connection.

1. **Read the star history.** Enter `owner/repo` at the [gitdebt report page](https://gitdebt.com/report) and find the source, coverage date and state named on the chart. [gitdebt's methodology](https://gitdebt.com/about) states its source as GitHub's own star history: exact daily counts, net of unstars.

 Read the shape on a linear scale, then again on a log scale. A steady slope means attention arriving over time. Steps suggest discrete events, such as a launch post, and one vertical jump means a single burst.

 Reading only the linear view misleads, because a late spike flattens everything before it. You're done when you can describe the shape in one sentence.

2. **Read the commit history.** Find the four health readings, which the [about page](https://gitdebt.com/about) defines through its charts: maintenance (the commit trend), ownership (how few contributors it takes to write half the commits), repair load (fix-labelled changes) and debt markers (TODO and FIXME movement). Ownership shows where knowledge has pooled and is not a judgement about any person. Fix-labelled changes describe history and don't prove a bug.

 These readings come from the git history of a shallow clone, not from stars, so they can disagree with the star curve. That disagreement is the finding.

 Never blend the two series into one score: stars are what the crowd did, commits are what the maintainers did.

3. **Read the README and recent commits yourself.** Open the repository on GitHub, follow the install steps in the README, and scan the last 10 to 20 commits for who wrote them and what they changed. The readings tell you where to look; they don't replace looking.

For a worked example with no live numbers, open the [apache/airflow report](https://gitdebt.com/apache/airflow). Read the star history first and note its source and shape.

Then read the maintenance, ownership and fix-labelled change charts beside it. Keep two questions separate: what the crowd did, and what the maintainers did.

Run a report on the repository you just found, using the [gitdebt report page](https://gitdebt.com/report), and read its star history beside its commit history.

## How do you tell a star spike from sustained growth?

You tell a star spike from sustained growth by starting every repository at the same origin and reading the first weeks of each curve. The [compare tool](https://gitdebt.com/compare) plots up to eight star histories on one set of axes, by calendar date or by days since each repository's first star.

1. **Add the trending repository and up to seven peers.** Pick repositories from the same field. A new project beside a decade-old giant on a linear scale looks flat, so switch to log if one line hugs the floor.
2. **Switch the axis to days since first star.** This is equal-start growth. On the calendar axis, repositories of different ages sit in different places and can't be read against each other.
3. **Read the first weeks or months.** A sharp jump on a flat history reads as one burst of attention. A curve that climbed steadily before the list reads as sustained interest.
4. **Check star velocity on the [leaderboard](https://gitdebt.com/leaderboard).** It shows stars gained over 1, 7 and 30 days. Name the window whenever you quote a figure, and don't copy live numbers, since they change.
5. **Open the commit history in the repository's report.** The maintenance reading comes from commits only, so it answers what the maintainers did.

For a repeatable example, start from the curated [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks) to pick peers, then put them and your trending repository in the compare tool on days since first star.

The curated page is a template for comparing like with like: the same field, the same origin, the same window.

Shape is context, not proof of quality. It says nothing about whether the stars are authentic.

## How do you show a real star-history chart in your README?

You show a real star-history chart in your README by pasting a plain image URL from [gitdebt.com/badges](https://gitdebt.com/badges), so readers see the whole curve instead of a screenshot of one trending day.

1. Copy the star-history snippet from the badges page. It needs [no account and no token](https://gitdebt.com/badges).
2. Replace `OWNER/REPO` with your repository. Keep the light and dark pair in one `<picture>` element, and keep the `?ref=readme` link around the image.
3. Commit the README. If nobody has analyzed the repository yet, a placeholder shows and queues the work. Reload later: the real chart replaces it at the same URL.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <img alt="Star history" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

The image is static by default, and `animate=1` opts an SVG into motion. The URL is cacheable and renders without JavaScript.

## Frequently asked questions

### What is GitHub Trending?

GitHub Trending is the page at github.com/trending that lists repositories drawing attention today. A [separate developers page](https://github.com/trending/developers) lists people. It shows attention in a window, not quality or upkeep.

### What does "stars today" mean on GitHub Trending?

It's a figure GitHub prints beside each repository on [the trending page](https://github.com/trending), labeled "stars today". GitHub's page doesn't state the basis for it, so read it as attention in a window, not as quality.

### Does GitHub publish how trending repositories are chosen?

Not on the trending page itself. It shows a list and a "stars today" figure without stating a ranking method, so treat the order as unexplained and date any figure you cite.

### Can a repository reach GitHub Trending with fake stars?

That can't be ruled out. A [2024 arXiv study](https://arxiv.org/abs/2412.13459) examined fake stars on GitHub, and [GitHub's Acceptable Use Policies](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies) prohibit rank abuse such as automated starring. Neither says which trending entry is affected.

### Does gitdebt tell me whether a trending repository's stars are fake?

No. As its [methodology page](https://gitdebt.com/about) states, gitdebt does not flag stars as fake, real or suspicious, and it stores only star dates and daily counts, never stargazer profiles. It shows the shape of a star history beside commit-based health readings, which helps you ask better questions but doesn't settle whether any star is authentic.

## Next steps: from a trending list to a repository you trust

**Today:** pick one repository from GitHub Trending and read its README. Then open its [report](https://gitdebt.com/report) and check the shape of the star history and the four readings.

**This week:** put it beside similar repositories in the [compare tool](https://gitdebt.com/compare), switched to days since first star.

**Ongoing:** if your own repository trends, paste the star-history embed from [gitdebt.com/badges](https://gitdebt.com/badges) into your README.

[Look up any public repository at gitdebt.com](https://gitdebt.com/). It's [MIT-licensed and open source](https://github.com/zhom/gitdebt), [every public report works without an account](https://gitdebt.com/about), and gitdebt is [not affiliated with GitHub](https://gitdebt.com/terms).

Stars say a repository is popular. Its commits say whether it is alive.

## Sources

[Trending repositories on GitHub today · GitHub](https://github.com/trending) · [Trending developers on GitHub today · GitHub](https://github.com/trending/developers) · [GitHub Acceptable Use Policies](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies) · [4.5 Million (Suspected) Fake Stars in GitHub: A Growing…](https://arxiv.org/abs/2412.13459) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [README badges, star-history charts and embeds](https://gitdebt.com/badges) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/)
