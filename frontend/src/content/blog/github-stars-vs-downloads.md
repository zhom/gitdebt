---
description: "GitHub stars vs downloads: stars record interest, downloads count fetches including CI and bots. Learn to read both against peers to measure real adoption."
faq:
  - answer: Neither metric alone is a better signal. Downloads are closer to usage but include CI, mirrors and bots; stars show interest and persist after a project stops. Read weekly downloads against peers in the same category, check the trend shape, and confirm with dependents, commits and release cadence before choosing.
    question: Are GitHub stars or npm downloads a better signal?
  - answer: Stars do not measure usage. A star is a bookmark that records attention, and you can star a project you never install. The top of the EvanLi/Github-Ranking list includes build-your-own-x and awesome, which are learning resources and lists, not packages.
    question: Do stars measure how many people use a package?
  - answer: A package can have many downloads but few stars because downloads count fetches, and no person has to be involved. CI runs, Docker rebuilds, mirrors and bots all add to the count. A library installed only as a dependency of other packages can also be fetched often while few people visit its repository.
    question: Why does a package have many downloads but few stars?
  - answer: "Downloads can be inflated because a count can run high without more people behind it. CI, Docker rebuilds, mirrors and bots all add fetches. Fake stars are a separate topic, covered in gitdebt's post on them. gitdebt does not flag stars as fake, real or suspicious."
    question: Can downloads be inflated?
  - answer: "Beyond stars and downloads, you should check dependents, issue activity, commit activity and release cadence, starting with the last-commit date. A gitdebt report lets you read a repository's star history beside health readings from its commit history, as separate sources."
    question: What should I check besides stars and downloads?
publishDate: "2026-11-02"
tags:
  - github stars
  - npm downloads
  - open source adoption
  - package metrics
  - star velocity
  - repository analytics
title: "GitHub stars vs downloads: measuring real open source adoption"
---

**In github stars vs downloads, the [WarmStars guide on npm signals](https://warmstars.com/guides/github-stars-vs-npm-downloads) treats a star as a bookmark that records interest, and a download as a registry fetch that sits closer to usage but includes automated traffic such as CI, mirrors and bots. Neither number counts real users. Read both against peer packages, over time, and beside commit activity.**

The two numbers on a package page often disagree about whether a project is adopted or only admired. That matters when you pick a dependency, benchmark your own project, or report traction to someone else.

After reading, you can say what each number counts, run a per-category comparison with one npm API call per package, and pick the signals that fit your goal.

> **Key Takeaways**
> - [WarmStars](https://warmstars.com/guides/github-stars-vs-npm-downloads) advises comparing each package against rivals in its own category. A raw number alone tells you little.
> - A star is a bookmark, so it stays after commits stop. Read the last-commit date and release cadence beside it.
> - Downloads are padded by CI, Docker rebuilds, mirrors and bots, so weekly trend shape and dependents tell you more than one total.
> - Use downloads per star to pick outliers worth investigating, never to rank packages.

## What do GitHub stars and package downloads each count?

A GitHub star counts one signed-in user bookmarking a repository, while a registry download counts one fetch of a package archive, which needs no person at all. Stars measure interest and downloads measure fetches. Neither tells you how many people use the software.

![Image generated](https://api.birchseek.com/m/V2FxDBiAetyrLgN0TdKe-Lt1KmUjYk5m.webp)

### What a star counts

The [WarmStars guide](https://warmstars.com/guides/github-stars-vs-npm-downloads) defines it plainly: "A star is a bookmark. It measures attention, not use." Someone saw the project and wanted to find it again.

A bookmark doesn't expire when the work stops. A star total doesn't fall because commits did, so read the last-commit date and release cadence beside it.

### What a download counts

The same guide calls an npm download "an install event" and says it "counts CI, Docker rebuilds, mirrors, and bots alongside real humans." It counts a fetch, not a person.

One team's build pipeline can fetch the same package many times a day. A high count can come from a handful of users, and the number alone can't tell you which.

### Check it on the star ranking

Open the [Top-100-stars page of EvanLi/Github-Ranking](https://github.com/EvanLi/Github-Ranking/blob/master/Top100/Top-100-stars.md) and write down the date you read it, because the ranking moves. Then read the descriptions of the top entries.

The first three entries describe themselves like this:

- `build-your-own-x`: "Master programming by recreating your favorite technologies from scratch."
- `awesome`: "Awesome lists about all kinds of interesting topics"
- `public-apis`: "A collective list of free APIs"

A tutorial collection, a set of lists and a list of APIs aren't packages you install from a registry. They have no download count to compare.

People still starred them in huge numbers, because they wanted to save them. That is what a star records. Whether anyone runs the code is a separate question, and the download count gets closer to it, noise included.

## What does a download mean on npm, PyPI, crates.io and Docker Hub?

On npm, PyPI, crates.io and Docker Hub, a download or pull means the registry served a file or image, not that a person installed or ran it.

| Registry | Counter | One count is | Over-time data |
| --- | --- | --- | --- |
| npm | downloads | one package fetch | yes |
| PyPI | downloads | one release-file fetch | public dataset |
| crates.io | downloads | one crate fetch | yes |
| Docker Hub | pulls | one image pull | total only |

The counters also differ in where you read them. The [Python Packaging User Guide](https://packaging.python.org/en/latest/guides/analyzing-pypi-package-downloads/) describes PyPI's counts as coming from "the public PyPI download statistics dataset", so you query a dataset rather than read a number off the project page.

Each registry defines its counter in its own documentation. Read that definition before you compare a Rust crate with a Python package, because the two numbers aren't built the same way.

The automated traffic from CI, rebuilds, mirrors and bots affects every registry, and the table does not quantify it. Two checks catch most inflated download counts:

- **Compare weekly counts across several weeks.** One sharp spike that fades the next week points to automation, not adoption.
- **Compare against dependents.** If downloads climb but few other packages depend on the project, treat the number with caution.

gitdebt [plots package downloads from npm, crates.io, PyPI and Docker against stars](https://github.com/zhom/gitdebt). For Docker Hub that is a total pull count, because Docker Hub publishes no download history.

**Best for:**

- **npm:** JavaScript packages, read as weekly counts over several weeks.
- **PyPI:** Python packages, when you can query the public dataset.
- **crates.io:** Rust crates, compared against peers in the same category.
- **Docker Hub:** container images, as one total that can't show a trend.

## GitHub stars vs downloads: what do real packages show?

Real packages show stars and downloads disagreeing in three recurring ways: heavily starred repositories with nothing to install, small utilities with high downloads and few stars, and dormant repositories that keep their stars. Each case shows what one number leaves out.

| Disagreement | High stars, little to install | Low stars, high downloads | Stars after activity stops |
| --- | --- | --- | --- |
| Stars show | Interest in the content | Little attention | Past interest |
| Downloads show | None, or no package | Pulled in by other packages | Low or flat |
| First check | Is there installable code? | Who depends on it? | Last commit date |
| Misleading if read alone | Stars | Downloads | Stars |

The first case is the curated list. The [EvanLi/Github-Ranking page](https://github.com/EvanLi/Github-Ranking/blob/master/Top100/Top-100-stars.md) of top repositories by stars puts `build-your-own-x` first, `awesome` second and `public-apis` third. People bookmark them to read, so there is no package to download.

The second is the small utility. A package that larger packages install on your behalf collects downloads from every install of its dependents. Few people ever visit its repository to star it.

To test a suspect, run `npm explain <package>` in a project and read which dependents pull it in. Those downloads still carry the CI, mirror and bot traffic described above, so this case can overstate human use just as the curated list overstates it through stars.

The third is the dormant repository. A star isn't removed when commits stop, so an old project can keep its star count while its last commit ages. Check the date, not the total.

None of the three cases proves which number is right. They show that the two signals can point in opposite directions, so each case needs its own check.

### A check you can repeat

Open the EvanLi ranking page and read the Language column for `build-your-own-x` (Markdown) and `awesome` (None). Then open each repository on GitHub and look for a manifest such as `package.json`, or search its name on the npm registry.

If you find neither, the stars measure bookmarking of reading material, and a downloads-per-star ratio has no meaning. For a repository that does ship code, [open its gitdebt report](https://gitdebt.com/report) to read its star history beside its commit trend.

**Best for, by case:**

- **High stars, little to install:** judging what a topic's audience wants to learn, not choosing a dependency.
- **Low stars, high downloads:** spotting packages your stack already relies on, once you check dependents.
- **Stars after activity stops:** reading how much past interest a project drew, alongside its last commit and release dates.

## How do you read GitHub stars and downloads together?

You read stars and downloads together by applying three decision rules to rival packages, then using downloads per star to find outliers worth investigating. Treat every result as a directional reading, not a verdict.

**Three decision rules**

- **Weekly beats all-time.** [WarmStars](https://warmstars.com/guides/github-stars-vs-npm-downloads) advises: "Read downloads weekly, never all-time, and always against a rival." An all-time total keeps rising after people stop installing.
- **Trend shape beats total.** A high download total with a flat or falling weekly trend isn't growth. Stars with no trend and an old last commit signal interest that has stopped.
- **Peers beat absolute numbers.** A count means something only next to packages that solve the same problem.

**The method**

1. **Pick one category.** Use [frontend frameworks](https://gitdebt.com/compare/frontend-frameworks), for example. Mixing categories makes the ratios meaningless.
2. **List 6 to 10 packages.** Write the registry name and the repository for each. Check monorepos: the `vue` package ships from `vuejs/core`, so a wrong repository gives the wrong star count.
3. **Record stars and last-7-day downloads, with the date.** Collect all rows on the same day. The [npm registry's download-counts documentation](https://github.com/npm/registry/blob/main/docs/download-counts.md) describes a `downloads/point/{period}[/{package}]` endpoint, and `last-week` is a valid period:

```bash
curl -s https://api.npmjs.org/downloads/point/last-week/react
```

4. **Divide downloads by stars** for each package.
5. **Investigate the highest and lowest ratios.** A high ratio can mean heavy use as a transitive dependency, busy CI, or a small star count. A low ratio can mean stars from tutorials or curiosity, or distribution outside npm.

**Example.** Run the call above for `react`, `vue` and `svelte` on one day. Write the date beside each count, then pair it with the star count from each repository page.

The counts change weekly, so your table should carry its own date. Success is a table where one or two rows stand out, each with a reason you can check in the repository.

The ratio has limits. It divides a weekly flow by a cumulative count, so older projects tend to look lower. Downloads still include CI, mirrors and bots, and one registry never shows the full picture.

Use the ratio to choose what to look into, not to rank packages.

To repeat the comparison over time, gitdebt's Stars versus downloads chart plots npm, crates.io and PyPI downloads against stars. It works signed out, with no connection to a repository, and the snippets are on the [README badges and embeds page](https://gitdebt.com/badges).

## What does research say about stars, and how do investors read star velocity?

Keep what was measured apart from what people assume.

[Borges and Valente](https://arxiv.org/abs/1811.07643) ask the question directly in "What's in a GitHub Star? Understanding Repository Starring Practices in a Social Coding Platform", published in the Journal of Systems and Software in 2018. It studies starring as a developer practice.

Read its findings for the reasons people star, because a star's meaning comes from those reasons, not from install counts.

None of the sources cited in this section measures a correlation between stars and downloads or dependents. Any rule such as "X stars means Y installs" is an assumption.

Runa Capital's [ROSS Index page](https://runacap.com/ross-index/) ranks the "fastest-growing open-source startups" by GitHub star growth. You can run the same kind of check on [gitdebt's star velocity leaderboard](https://gitdebt.com/leaderboard), which ranks repositories by recent star gains.

A ranking by growth rewards a burst, such as a launch or a front-page post. A project used quietly for years can gain few stars in a window and rank low. That follows from how growth rankings work, not from a finding in either source.

gitdebt does not flag stars as fake, real or suspicious, and it does not score accounts. For the industry-wide problem of purchased stars, read gitdebt's post on [fake GitHub stars](https://gitdebt.com/blog/fake-github-stars).

Downloads need their own sanity check. Compare a package's weekly downloads with its dependents and release dates, and treat a jump with no matching release as unexplained.

## Which other signals show adoption, and which fit your goal?

Dependents, issue activity, commit activity and release cadence complement stars and downloads, and your goal decides which one to lean on.

**The complements, each with its limit**

1. **Dependents (reverse dependencies).** [GitHub's dependency graph](https://docs.github.com/en/code-security/supply-chain-security/understanding-your-software-supply-chain/about-the-dependency-graph) lists dependents, "the repositories and packages that depend on it." Limit: it shows who declares a dependency, not who runs it.
2. **Issue activity.** Check how many issues open and close, and how fast maintainers reply. Limit: a popular project can carry a long backlog, and a quiet one may simply be finished.
3. **Commit activity.** [gitdebt's repository health](https://gitdebt.com/) reports maintenance (commit cadence) and ownership (how few contributors write half the commits). Limit: both are directional signals, not verdicts, and ownership is not a judgment about any person. Stars and commit history are independent sources, so read them side by side and never blend them. The [apache/airflow report](https://gitdebt.com/apache/airflow) shows the two together.
4. **Release cadence.** Look at the dates of the last few tagged releases. Limit: a stable library may ship rarely, and automation can cut releases without new work.

**Which signal fits your goal**

- **Choosing a dependency:** compare weekly downloads against rivals, then check dependents, the last release date and the maintenance reading. No single number settles it.
- **Benchmarking your own project:** compare downloads per star and your 90-day commit trend against peers in your category. Peers must share a category, or the comparison misleads.
- **Evaluating a project for investment or hiring:** weigh dependents and the commit trend above star velocity, and treat ownership concentration as a question to ask, not a conclusion.
- **Reporting to stakeholders:** give one number per question, name what it counts, and date it. Each source's method is on the [gitdebt methodology page](https://gitdebt.com/about).

## Frequently asked questions

### Are GitHub stars or npm downloads a better signal?

Neither alone. [WarmStars](https://warmstars.com/guides/github-stars-vs-npm-downloads) notes that downloads are closer to usage but include CI, mirrors and bots, while stars show interest and persist after a project stops. Read weekly downloads against peers in the same category, check the trend shape, and confirm with dependents, commits and release cadence before choosing.

### Do stars measure how many people use a package?

Stars don't measure usage. A star is a bookmark that records attention, and you can star a project you never install. The top of the [EvanLi/Github-Ranking list](https://github.com/EvanLi/Github-Ranking/blob/master/Top100/Top-100-stars.md) includes build-your-own-x and awesome, which are learning resources and lists, not packages.

### Why does a package have many downloads but few stars?

Downloads count fetches, and no person has to be involved. CI runs, Docker rebuilds, mirrors and bots all add to the count. A library installed only as a dependency of other packages can also be fetched often while few people visit its repository.

### Can downloads be inflated?

Yes. A download count can run high without more people behind it, because every automated fetch counts as a download. Fake stars are a separate topic, covered in gitdebt's post on them, and gitdebt does not flag stars as fake, real or suspicious.

### What should I check besides stars and downloads?

Check dependents, issue activity, commit activity and release cadence, starting with the last-commit date. On [gitdebt](https://gitdebt.com/), a [repository report](https://gitdebt.com/report) lets you read a repository's star history beside health readings from its commit history, as separate sources.

## Your next steps for measuring adoption

Settling the github stars vs downloads question takes three passes, each one short.

**Today:** Open the package in front of you and check two things: the last-commit date and the weekly download trend. Then pull the same week's downloads for one rival.

**This week:** Run the per-category comparison for your shortlist. Collect stars and weekly downloads for each package, compute downloads per star, and explain the outliers in both directions. You can [overlay up to eight star histories](https://gitdebt.com/compare) to cover the star side.

**Next quarter:** For your own project, record star growth and weekly downloads on the same dates. Compare the shape of each trend, not the totals.

To read a repository's star history beside four health readings from its commit history, [look up any public repository at gitdebt.com](https://gitdebt.com/). Every public repository report works without an account, and [gitdebt is open source](https://gitdebt.com/about) and not affiliated with GitHub. Treat its readings as directional signals, not verdicts.

## Sources

[GitHub Stars vs npm Downloads: Which Signal to Trust ·…](https://warmstars.com/guides/github-stars-vs-npm-downloads) · [Github-Ranking/Top100/Top-100-stars.md at master ·…](https://github.com/EvanLi/Github-Ranking/blob/master/Top100/Top-100-stars.md) · [Analyzing PyPI package downloads, Python Packaging User…](https://packaging.python.org/en/latest/guides/analyzing-pypi-package-downloads/) · [npm registry download counts API documentation](https://github.com/npm/registry/blob/main/docs/download-counts.md) · [What's in a GitHub Star? Understanding Repository Starring…](https://arxiv.org/abs/1811.07643) · [ROSS Index by Runa Capital](https://runacap.com/ross-index/) · [About the dependency graph - GitHub Docs](https://docs.github.com/en/code-security/supply-chain-security/understanding-your-software-supply-chain/about-the-dependency-graph) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/). Every other source is linked where the article cites it.
