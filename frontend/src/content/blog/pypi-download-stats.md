---
description: Where do PyPI download stats come from? BigQuery, pypistats.org, pepy.tech and ClickPy count fetch events, not users. Learn to read them and check upkeep.
faq:
  - answer: They report different windows. Pepy shows the latest available complete day plus 7-day and 30-day windows ending on that day, while pypistats.org shows last day, last week and last month. Compare the same window on both sites before treating a gap as an error.
    question: Why do pypistats.org and pepy.tech show different download numbers for the same package?
  - answer: PyPI still shows download statistics through its statistics page, which names BigQuery public datasets as its first-party source, plus a public dashboard on usage and performance. For a quick per-package count, use a third-party site such as pypistats.org or ClickPy, both listed on that page.
    question: Does PyPI still show download counts, or do I need another site?
  - answer: "The data on pypistats.org and ClickPy is updated daily, as both sites state. Pepy's checker reports the latest available complete day, so today's downloads won't show yet. Expect some lag, and check the date on each page before comparing numbers."
    question: How fresh is the data on these sites?
  - answer: "No, a high count shows how often files were fetched, not whether anyone maintains them. Check the commit history of the package's GitHub repository and read it as a separate signal from downloads. You can analyze any public repository for commit-based maintenance."
    question: Do high PyPI downloads mean a package is maintained?
  - answer: A brand-new package may not be listed yet, because pypistats.org and ClickPy update their data daily. Also confirm you typed the exact PyPI package name, as pepy.tech asks. If both of those are right, check again after the next daily update.
    question: Why does my new package show no download stats yet?
publishDate: "2026-10-27"
tags:
  - pypi
  - download stats
  - python packages
  - package maintenance
  - clickpy
  - bigquery
title: "PyPI download stats: how to read them and check upkeep"
---

**PyPI download stats come from [PyPI's own BigQuery public datasets](https://pypi.org/stats/) and public dashboard, plus third-party sites such as pypistats.org, [pepy.tech](https://www.pepy.tech/) and ClickPy. Each figure counts download events, not users, so a CI run or a repeat install adds to it. Downloads show how often a package is fetched, but they can't tell you whether anyone still maintains it.**

The question usually comes up at a `requirements.txt` line. A stats site shows millions of downloads for a candidate dependency, and you need to know whether the project behind it is alive.

After reading, you can pick the right source, look up one package by its distribution name, read the version, Python and system breakdowns, and set downloads beside commit-based maintenance from its GitHub repository.

> **Key Takeaways**
> - Search the distribution name you pass to `pip install`, not the import name.
> - Freshness differs by source. Check each page's date, and expect no data yet for a brand-new package.
> - Flat baselines and release spikes mostly reflect automated installs, so treat a decline as a question.
> - Downloads and commits are independent signals; read them side by side, never blended into one score.

## What do PyPI download stats actually count?

PyPI download stats count recorded requests for a distribution file, such as a wheel or sdist, from the package index. Each request is an event, not a person, so the count measures fetch volume, not unique users or adoption.

![An isometric illustration shows stylized people placing blue and green data blocks into sorting pipes.](https://api.birchseek.com/m/Da7tg6lx5ubIwE2Cr7Oh4OjDk0IET-fZ.webp)

Four things add events without adding users:

- **CI runs.** A pipeline that runs 40 builds a day, each starting with an empty cache and running `pip install`, adds 40 events a day from one team.
- **Dependency installs.** Installing package A also fetches package B if A requires it, even though you never chose B.
- **Repeat downloads.** Fetching the same version again counts again.
- **Mirrors.** A mirror may fetch a file once and serve many machines behind it, so one event can stand for several installs.

The sites that report these counts describe windows of fetches, not people. [Pepy](https://www.pepy.tech/) says its checker "shows downloads for the latest available complete day and the 7-day and 30-day windows ending on that day."

[PyPI's statistics page](https://pypi.org/stats/) says it provides "public datasets, including download statistics and metadata via BigQuery." The [pypistats.org page for `__all__`](https://pypistats.org/packages/__all__) says its figures "indicate downloads across all packages on PyPI."

A fetch count cannot show unique users, whether a package runs in production, or whether anyone still maintains it. Those questions need other evidence, such as commit history.

You can check the event idea yourself. The [ClickPy dashboard for pip](https://clickpy.clickhouse.com/dashboard/pip) at `clickpy.clickhouse.com/dashboard/pip` has a "File types by installer" panel.

That panel splits downloads by what fetched the file, not by who wanted it. The split shows a download is a machine request, and the dashboard doesn't try to turn it into a head count.

## Which PyPI download stats source should you use?

The right PyPI download stats source depends on your question: BigQuery for custom queries, the PyPI dashboard for PyPI's own usage, and pypistats.org, pepy.tech or ClickPy for per-package lookups.

| Source | Best for | Access needed |
| --- | --- | --- |
| BigQuery public datasets | Custom questions over raw download data | A SQL query |
| PyPI dashboard | Usage and performance of PyPI itself | None, public page |
| pypistats.org | Quick per-package lookups | None, search by name |
| pepy.tech | Latest day plus 7-day and 30-day totals | No signup or API key |
| ClickPy | Charts by version, Python version and system | None, browse dashboards |

The [pypi.org statistics page](https://pypi.org/stats/) lists two first-party sources: public datasets on BigQuery that include download statistics, and a public dashboard with statistics on usage and performance.

It also names libraries.io, pypistats.org, deps.dev and ClickPy as third-party services unaffiliated with PyPI.

Freshness is documented unevenly. The [pypistats.org search box](https://pypistats.org/) says its package data is updated daily, and [ClickPy's front page](https://clickpy.clickhouse.com/) says the same.

[Pepy's checker](https://www.pepy.tech/) reports the latest available complete day and the 7-day and 30-day windows ending on that day.

Expect the sources to disagree. Each one sets its own windows and day boundaries, so a figure from one site won't match another's. Compare trends within a single source, not totals across sources.

Before you write a BigQuery query, read the dataset's own docs for table names, what a row represents and stated limits. A refresh interval for BigQuery or the dashboard is not published on the pypi.org page.

- **If you want a custom question** (one package by country, or many packages at once), use BigQuery.
- **If you want PyPI's own usage and performance figures**, use the PyPI dashboard.
- **If you want one number fast**, use pypistats.org.
- **If you want a latest-day check with no account**, use pepy.tech.
- **If you want to read the breakdowns visually**, use ClickPy.

## How do you look up PyPI download stats for one package?

You look up PyPI download stats for one package by finding its distribution name, normalizing it and entering it on a stats site. Most wrong results come from the first two steps.

1. **Find the distribution name on the package's PyPI page.** Use the name in the page title and the `pip install` line, not the name you type after `import`. A stats site only knows the distribution name, so an import name often returns nothing or a different project.
2. **Normalize the name.** [PEP 503](https://peps.python.org/pep-0503/) says: "The name should be lowercased with all runs of the characters ., -, or _ replaced with a single - character." The rule is one line of Python, shown here with an example name:

```text
re.sub(r"[-_.]+", "-", name).lower()

Django_REST.framework  ->  django-rest-framework
```

3. **Enter the normalized name in a stats site.** Open pypistats.org, pepy.tech or ClickPy. [Pepy's own instructions](https://www.pepy.tech/) say to "Enter the exact PyPI package name", then it shows the latest complete day plus 7-day and 30-day windows. Success is a chart with daily counts.
4. **Allow for lag.** [pypistats.org](https://pypistats.org/) and ClickPy both describe their data as updated daily, and Pepy reports the "latest available complete day". Today's installs won't appear yet, so compare figures from the same date.
5. **Treat an empty chart on a new package as "not processed yet".** A package published hours ago usually has no data because the daily update hasn't reached it. Check again after the next refresh before you read zero as zero.

Skip the JSON API at `pypi.org/pypi/<name>/json`: it returns package metadata, not download counts.

[PyPI's statistics page](https://pypi.org/stats/) lists BigQuery public datasets as the first-party source for "download statistics and metadata". For counts, use BigQuery or one of the three sites above.

The count from step 3 is an event total. To judge whether the project is maintained, take its GitHub slug (for `apache-airflow`, `apache/airflow`) and open [its gitdebt report](https://gitdebt.com/apache/airflow) to read commit-based health beside it.

## How do you read download breakdowns and release spikes?

You read a PyPI download breakdown by treating each chart as one question about who fetched the files, then noting what that chart can't answer.

[ClickPy's pip dashboard](https://clickpy.clickhouse.com/dashboard/pip) at `clickpy.clickhouse.com/dashboard/pip` labels its panels "Downloads over time", "Top versions", "Downloads by Python version over time", "Downloads by system over time", "Downloads by country" and "File types by installer". Open it for `pip` and follow along.

- **Top versions:** shows whether the latest release is taking over or whether old pinned versions are still fetched. A long tail of old versions usually points to lockfiles and pins, not a failed release.
- **Downloads by Python version over time:** shows which interpreters the fetches come from. It describes the machines running the installer, often CI images, not the Python your future users run.
- **Downloads by system over time:** shows the OS mix. A heavy Linux share fits servers and CI runners, but the chart can't separate them from people.
- **File types by installer:** shows which tool fetched wheels and which fetched source distributions. A shift here can change counts without any change in real usage.

Most mature packages show a flat baseline in "Downloads over time". It comes from automated installs that run every day whether or not anyone is paying attention.

A release adds a spike. CI jobs and dependent packages pull the new version soon after it ships, so the peak is a burst of fetches, not a burst of users.

A decline is harder to read. It may mean fewer people use the package, or that others changed how they install it: a cache, a mirror, a pin, or a dependent that dropped it. Treat it as a question to check, not a finding.

Daily counts are noisy, so smoothing helps. The [all-packages page on pypistats.org](https://pypistats.org/packages/__all__) offers a "7-day smoothing" option, and [pepy.tech's checker](https://www.pepy.tech/) reports the latest complete day plus 7-day and 30-day windows.

## Why can a package with huge downloads be obscure or dormant?

A package with huge downloads can be obscure or dormant because downloads count automated fetching, not attention or upkeep.

[ClickPy's home page](https://clickpy.clickhouse.com/) shows the gap. Its "Emerging repos" list ranks packages by downloads in the last 3 months.

A count that size says the files are being fetched. It doesn't say anyone picked the package by name.

Two causes explain most of it:

- **Dependencies.** When package A requires package B, every install of A fetches B, so the person running `pip install` never types B's name.
- **CI pipelines.** A pipeline that installs without a cache fetches the same files on every run, so one team can add thousands of downloads.

The reverse also happens. A repository people star as a reference, or install from somewhere other than PyPI, can show many stars and modest downloads.

Each signal has a limit:

- **Downloads** say how often a package is fetched.
- **Stars** say what the crowd did, at the moment each person clicked.
- **Commits** say whether anyone is maintaining the code.

Stars and commits are two independent sources, and gitdebt never blends them. The [gitdebt methodology page](https://gitdebt.com/about) lists where each series comes from. gitdebt does not flag stars as fake, real or suspicious, and it doesn't score accounts.

Treat "last updated" labels with care. Each site computes its own, so a date on one stats page isn't proof of a package's maintenance state. Check what the label refers to, then look at the repository's commit history.

These signals describe a package; they don't hand you a verdict. A heavily fetched package with a quiet commit history is a reason to read the repository, not a reason to reject it.

## How do you check whether a PyPI package is still maintained?

You check whether a PyPI package is still maintained by following its page to the GitHub repository and reading that repository's commit history, because a download count can't show upkeep.

1. **Open the package's PyPI page and find the repository under Project links.** Some packages list only a homepage or a docs site. If no GitHub link appears, there's no commit history to read.
2. **Copy the owner/repo slug.** For the `apache-airflow` package it's `apache/airflow`. Use the upstream repository, not a fork.
3. **Open gitdebt.com/owner/repo,** such as the [apache/airflow report](https://gitdebt.com/apache/airflow). [Every public repository report works without an account](https://gitdebt.com/about).
4. **Read the commit-based charts.** The [about page](https://gitdebt.com/about) defines them: the commit trend (monthly commit volume), ownership concentration (how few contributors it takes to write half the commits), fix-labelled changes, and TODO and FIXME movement.
5. **Compare stars with downloads.** The [stars-versus-downloads chart](https://gitdebt.com/badges) plots package downloads against stars. The two come from independent sources and are never blended.

These readings are directional signals, not verdicts. Commit readings don't prove quality, and downloads remain fetch events.

Ownership concentration shows where knowledge has pooled in a codebase; it isn't a judgement about any person. gitdebt reads public repositories only, and it doesn't flag stars as fake, real or suspicious.

For your own README, the stars-versus-downloads chart is a plain image URL. It needs no account, token, build step or GitHub Action.

Add `theme=light` or `theme=dark` for a variant, and keep the `?ref=readme` link around the image:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <img src="https://api.gitdebt.com/api/repos/OWNER/REPO/usage.svg" alt="Stars versus downloads for OWNER/REPO">
</a>
```

A repository nobody has analyzed yet shows a placeholder and queues the work. To start that, enter the slug on the [repository report page](https://gitdebt.com/report). Ready-made snippets are on the [README embeds page](https://gitdebt.com/badges).

## Frequently asked questions

### Why do pypistats.org and pepy.tech show different download numbers for the same package?

They report different windows. [Pepy](https://www.pepy.tech/) shows the latest available complete day plus 7-day and 30-day windows ending on that day, while [pypistats.org](https://pypistats.org/packages/__all__) shows last day, last week and last month. Compare the same window on both sites before treating a gap as an error.

### Does PyPI still show download counts, or do I need another site?

[PyPI's statistics page](https://pypi.org/stats/) names BigQuery public datasets as its first-party source of download statistics, plus a public dashboard on usage and performance. For a quick per-package count, use a third-party site such as pypistats.org or ClickPy, both listed on that page.

### How fresh is the data on these sites?

[pypistats.org](https://pypistats.org/) and [ClickPy](https://clickpy.clickhouse.com/) both say their data is updated daily. Pepy's checker reports the latest available complete day, so today's downloads won't show yet. Check the date on each page before comparing numbers.

### Do high PyPI downloads mean a package is maintained?

No, a high count shows how often files were fetched, not whether anyone maintains them. Check the commit history of the package's GitHub repository and read it as a separate signal. [Analyze any public repository](https://gitdebt.com/report) to read its commit history.

### Why does my new package show no download stats yet?

A brand-new package may not be listed yet, because pypistats.org and ClickPy update daily. Also confirm you typed the exact PyPI package name, as pepy.tech asks. If both are right, check again after the next daily update.

## What to check next on any PyPI package

On any PyPI package, read one download source first, then the repository's commit history.

- **In the next 5 minutes:** find the distribution name on the package's PyPI page, normalize it, and read the download curve and version breakdown in one source.
- **Today:** open the package's repository report, such as the [apache/airflow report](https://gitdebt.com/apache/airflow), and read the commit-based charts next to the stars-versus-downloads chart.
- **This week:** if you maintain a package, add the stars-versus-downloads embed from the [README embeds page](https://gitdebt.com/badges) to your own README.

Stars say a repository is popular. Its commits say whether it is alive.

Look up the repository for any PyPI dependency at [gitdebt.com](https://gitdebt.com/). It's [MIT-licensed and open source](https://github.com/zhom/gitdebt), and [every public repository report works without an account](https://gitdebt.com/about).

## Sources

[Statistics · PyPI](https://pypi.org/stats/) · [PyPI Stats: Python Package Downloads | pepy.tech](https://www.pepy.tech/) · [PyPI Download Stats](https://pypistats.org/packages/__all__) · [PyPI downloads stats for pip - clickpy.clickhouse.com](https://clickpy.clickhouse.com/dashboard/pip) · [PyPI Download Stats](https://pypistats.org/) · [PEP 503 – Simple Repository API](https://peps.python.org/pep-0503/) · [PyPI downloads stats - ClickPy](https://clickpy.clickhouse.com/)
