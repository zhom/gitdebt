---
description: "Which technical debt metrics need no cost data? Four git signals: maintenance, ownership, repair load and debt markers. Learn to read them as trends over time."
faq:
  - answer: "No universal threshold for a good technical debt ratio is one this article can cite. The ratio depends on how remediation and development cost are estimated, so compare your own ratio across releases, not against another team's figure. Check the rating bands in the scanner you use."
    question: What is a good technical debt ratio?
  - answer: "You can measure technical debt without paid tools by reading a repository's git history. Compare commits in the trailing 90 days with the 90 before, check how few contributors write half the commits, track the fix-labelled share of file changes, and watch to-do and fix-me comment movement. Treat each as a directional signal, not a verdict."
    question: How do I measure technical debt without paid tools?
  - answer: "High code churn isn't always bad, because it counts how often files were touched, not why. Feature work and refactoring raise it too. Treat it as a prompt to open those files, then check whether fix-labelled changes cluster there instead."
    question: Is high code churn always bad?
  - answer: Read the commit trend and the stars separately. Stars show what the crowd did, and commits show what the maintainers did. A rising star curve can sit beside a thinning commit trend, so check recent commits before you decide for yourself.
    question: How can I tell whether a GitHub dependency is still maintained?
  - answer: "They count as self-admitted debt, because developers wrote them to flag shortcuts or unfinished work. A raw count has limits: it shows neither size, age nor cost, and a repository with none may simply not use the convention. Watch the movement over time instead."
    question: Do to-do and fix-me comments count as technical debt?
publishDate: "2026-10-20"
tags:
  - technical debt
  - git history
  - code ownership
  - code churn
  - dora metrics
  - open source maintenance
title: "Technical debt metrics: what git history can and can't show"
---

**The technical debt metrics that matter most are technical debt ratio, lead time and change failure rate, but they need internal effort or cost data. From a repository's git history alone, you can read four others: maintenance, ownership, repair load and debt markers, the movement of comments that flag unfinished work. Read those four as directional signals, not verdicts.**

You're deciding whether a dependency is still alive, or whether your own codebase is carrying debt. You have no effort estimates and no paid scanner output. That's the usual position for an engineering manager, a maintainer or an analyst reading a public repository.

With a small set of signals you can compute the classic formulas when you hold the data, read commit-based signals when you don't, and track both as a trend over months instead of one number.

> **Key Takeaways**
> - Stars show what the crowd did; commit history shows what the maintainers did, and the two can disagree.
> - Commit history doesn't record authorship method, so it can't separate AI-written code from hand-written code.
> - One reading is weak. A trend across a few readings is the unit of evidence.
> - Every signal has a limit: frequent file changes don't prove instability, fix-labelled changes don't prove bugs, and ownership concentration judges no person.

## What are technical debt metrics, and which ones need cost data?

Technical debt metrics are measurements of how much future rework a codebase's shortcuts are likely to cost, and only some of them need cost data. Technical debt ratio, lead time and change failure rate depend on internal records. Commit cadence, author concentration and the movement of unfinished-work comment markers can be read from a public repository's git history.

![Image generated](https://api.birchseek.com/m/Tt49dYbNiXTD1nc2165zyYH6-ZrbVp7q.webp)

The metaphor comes from [Ward Cunningham's 1992 OOPSLA experience report](http://c2.com/doc/oopsla92.html) on the WyCash portfolio management system. He wrote that "Shipping first time code is like going into debt."

[Martin Fowler](https://martinfowler.com/bliki/TechnicalDebt.html) later described the interest as the extra effort it takes to add new features, so it shows up where code gets changed.

### Metrics that need internal effort or cost data

- **Technical debt ratio (TDR):** [SonarSource's metric definitions](https://docs.sonarsource.com/sonarqube-server/user-guide/code-metrics/metrics-definition/) define it from the cost to develop the software and the cost to fix it. Both inputs are estimates only your team holds.
- **Lead time for changes:** [DORA's four keys guide](https://dora.dev/guides/dora-metrics-four-keys/) describes it as the time it takes for a code commit or change to be successfully deployed to production. It needs deployment records.
- **Change failure rate:** [DORA's guide](https://dora.dev/guides/dora-metrics-four-keys/) describes it as the ratio of deployments that require immediate intervention following a deployment. It needs incident data.

### Metrics a public repository's history shows

Four readings need only commits: maintenance, ownership, repair load and debt markers. File change frequency adds a view of where change concentrates, which is where Fowler's interest gets paid.

These readings show activity, not cost.

## How are the four classic technical debt metrics calculated?

The four classic technical debt metrics are technical debt ratio, code churn, defect ratio and cycle time, and each one reduces to a single formula. The worked numbers in each example are arithmetic illustrations, not statistics.

1. **Technical debt ratio (TDR).** [SonarSource's documentation](https://docs.sonarsource.com/sonarqube-server/user-guide/code-metrics/metrics-definition/) gives the formula as "Remediation cost / Development cost". If fixing every flagged issue would take 40 hours and writing the code took 800, TDR is 0.05. SonarQube's scale rates that an A ("A=0-0.05").

 *Limit:* both costs are estimates, so TDR needs internal data and shifts when the estimating model changes.

2. **Code churn.** Churn here follows [Nagappan and Ball's relative definition](https://www.microsoft.com/en-us/research/publication/use-of-relative-code-churn-measures-to-predict-system-defect-density/) from ICSE 2005, which will "relate the amount of churn to other variables such as component size and the temporal extent of churn". In formula form: relative churn = churned lines ÷ total lines.

 If 300 lines were added or changed in a 6,000-line component during a release, relative churn is 0.05. Other definitions count raw lines or commits, so churn figures don't compare across sources.

 *Limit:* the same paper found that "absolute measures of code churn are poor predictors of defect density". Churn also can't separate planned feature work from rework, so a high reading marks where to look, not what is wrong.

3. **Defect ratio.** Defect ratio = defects ÷ thousands of lines of code. If 12 defects were logged against 40,000 lines, the ratio is 12 ÷ 40 = 0.3 per 1,000 lines.

 Flip the fraction and the number rises as quality improves, so check the direction before comparing sources.

 *Limit:* it counts only defects someone reported. In a public repository, fix-labelled commits describe fix-labelled history, not proven bugs.

4. **Cycle time.** Cycle time is the time from the first commit on a change to its merge. DORA's lead time for changes starts at the same commit but ends later: [the DORA guide](https://dora.dev/guides/dora-metrics-four-keys/) describes it as the span from "code committed to code successfully running in production".

 If a change is first committed Monday, merged Wednesday and deployed Friday, cycle time is 2 days and lead time is 4.

 *Limit:* it shows how fast changes move, not how much debt sits behind them. It also needs pull request or deployment timestamps, which a bare commit history doesn't hold.

## Which technical debt metrics can you read from git history?

A repository's git history shows four technical debt readings with no effort estimates and no paid scanner: maintenance, ownership, repair load and debt markers. Remediation costs and deployment records stay out of reach, because a public repository doesn't publish them.

Each reading comes with a limit:

- **Maintenance** is read from the [commit trend](https://gitdebt.com/about), which shows whether monthly commit volume is rising or falling. It counts activity, not quality, and a finished library can be quiet on purpose.
- **Ownership** measures concentration of authorship. gitdebt's own wording, from its [methodology page](https://gitdebt.com/about), is that "Ownership concentration shows how few contributors it takes to write half the commits." It shows where knowledge has pooled, not a judgement about any person.
- **Repair load** is the fix-labelled share of file changes. It depends on how people word commit messages, so it describes fix-labelled history and does not prove bugs.
- **Debt markers** are the to-do and fix-me comments developers leave in code, and whether they're growing or shrinking. A count says nothing about what one note would cost to resolve.

Stars are a separate source. They record what the crowd did, while commits record what the maintainers did, and gitdebt never blends the two.

A rising star curve can sit beside a falling maintenance reading, so open `facebook/react` and `vitejs/vite` and read both. The [frontend frameworks star history comparison](https://gitdebt.com/compare/frontend-frameworks) puts the star curves side by side.

[Martin Fowler's technical debt essay](https://martinfowler.com/bliki/TechnicalDebt.html) says that "the extra effort that it takes to add new features is the interest paid on the debt." File change frequency, a count of analyzed commits touching each file, shows where that change concentrates.

File change frequency identifies frequently revisited files without claiming those changes were unstable or defective. According to the same methodology page, signals are computed from a shallow clone.

## How do you read code ownership and bus factor from commits?

Commit authorship shows where knowledge has pooled in a codebase when you count how few authors it takes to write half the commits. A low count means a small group wrote most of the history. A high count means the work is spread out.

Two papers frame the idea. Avelino et al. (ICPC 2016) [describe truck factor](https://arxiv.org/abs/1604.06766) as "the minimal number of developers that have to be hit by a truck (or quit) before a project is incapacitated."

Bird et al. (ESEC/FSE 2011) [studied Windows Vista and Windows 7](https://www.microsoft.com/en-us/research/publication/dont-touch-my-code-examining-the-effects-of-ownership-on-software-quality/) and report that ownership measures, such as the proportion of ownership for the top owner, have a relationship with pre-release faults and post-release failures.

Treat the count as a map of the history, not a verdict on any person. It can mislead in four cases:

- Squashed merges credit the person who merged.
- Bots add commits.
- Vendored or generated files inflate a few names.
- Company-paid teams often commit under a few shared accounts.

Read the trend, not a snapshot. On the [apache/airflow report](https://gitdebt.com/apache/airflow), open the ownership concentration and contributors charts. Check whether the author count for half the commits is widening or narrowing.

## When do churn, fix and TODO/FIXME signals mislead?

Churn, fix-labelled changes and debt markers mislead when you read them as proof of bad code, because each one counts a trace in history, not a defect. Treat each as a question to ask.

**Churn.** Lockfiles, configuration, changelogs and generated files often top the file change frequency chart. So does any file where a feature is being built right now. A file near the top deserves a look at its diffs before any conclusion.

**Fix-labelled changes.** From outside a project you usually have only commit messages, so a keyword match on "fix" is a rough stand-in for a link between commits and a bug tracker.

The keyword match misses fixes worded as "patch" or "resolve" and catches typo and lint fixes. If the fix-labelled share rises, read a sample of those messages before drawing a conclusion.

**Debt markers.** A count of to-do and fix-me comments shows the markers developers left, not the debt itself.

Debt nobody commented stays invisible. A falling count may mean comments were deleted, not that work was done, so check whether removed markers sit in commits that also changed code.

**AI-generated code.** Git history shows the volume and timing of changes: a burst of large commits, a jump in file change frequency. It can't show that a tool wrote them. Don't infer authorship from a spike. Open the diffs.

Read all three signals beside the commit trend. When they agree over several months, you have a direction.

## How do you read technical debt metrics as a set over time?

You read technical debt metrics as a set by checking four git-history signals in a fixed order, writing down a direction for each, and then asking whether the directions agree.

1. **Open the repository's report.** Enter the `owner/repo` slug in the [gitdebt report lookup](https://gitdebt.com/report) and wait for the sheet to load. Keep the star curve out of this pass.
2. **Read the commit trend for its recent direction.** Compare the latest months with the months before them: higher, flat or lower. One quiet month, such as a holiday, isn't a decline.
3. **Read the file change frequency chart and note which files recur.** A file near the top is revisited often, and that is all the chart claims.
4. **Compare against debt marker movement.** Note whether the count of self-admitted markers is rising, flat or falling. A team that never writes them leaves nothing to read.
5. **Check ownership concentration and repair load.** Count how few contributors write half the commits, then note the fix-labelled share of file changes.
6. **Write down a direction for each signal, not a score.** Four words (rising, flat, falling, unclear) are enough.

When the directions agree, the reading is firmer. Falling maintenance, narrowing ownership and rising marker counts together suggest fewer people are doing less work while open notes pile up.

When they conflict, check something else before concluding anything. Falling commits with flat markers and wide ownership might describe a stable project, so look at release cadence and issue activity.

A rising star curve beside a falling maintenance reading is the disagreement worth noticing.

To practice, open `facebook/react` and `vitejs/vite` in two tabs. Compare the commit trend and file change frequency charts side by side, and record direction only, with no figures.

Then repeat the steps on the [angular/angular report](https://gitdebt.com/angular/angular) and see whether your four directions match or split.

Re-read on a schedule, such as monthly, and compare windows. One reading isn't a trend.

Teams with cost and deployment data can add technical debt ratio, lead time and change failure rate beside these four readings.

## Frequently asked questions

### What is a good technical debt ratio?

A good technical debt ratio is one that falls across your own releases, because the ratio depends on how remediation and development cost are estimated. Compare your ratio against your own history, not another team's figure, and check the rating bands in the scanner you use.

### How do I measure technical debt without paid tools?

You can measure technical debt without paid tools by reading a repository's git history. Read the commit trend, check how few contributors write half the commits, track the fix-labelled share of file changes, and watch to-do and fix-me comment movement.

### Is high code churn always bad?

High code churn isn't always bad, because it counts how often files were touched, not why. Feature work and refactoring raise it too. Treat it as a prompt to open those files, then check whether fix-labelled changes cluster there.

### How can I tell whether a GitHub dependency is still maintained?

Read the commit trend and the stars separately, since stars show what the crowd did and commits show what the maintainers did. A rising star curve can sit beside a thinning commit trend, so check recent commits before you decide.

### Do to-do and fix-me comments count as technical debt?

They count as self-admitted debt, because developers wrote them to flag shortcuts or unfinished work. A raw count shows neither size, age nor cost, and a repository with none may simply not use the convention. Watch the movement over time instead.

## What should you do in the next 90 days?

In the next 90 days, take one dependency through the four commit-based readings, record their directions, and compare two windows.

**Today:** Open the report for one dependency you rely on, such as `vitejs/vite`, and read its commit trend first. Then note maintenance, ownership, repair load and debt markers, with the limit of each written beside it.

**This month:** Record the direction of each reading (up, flat or down) with the date you read it.

**At 90 days:** Open the same report and compare the two windows. Note which directions changed and which held.

Add TDR, lead time and change failure rate only if your team already records the cost and deployment data they need.

[Look up any public repository on gitdebt](https://gitdebt.com/) and read its maintenance.

gitdebt is [MIT-licensed](https://github.com/zhom/gitdebt), and [every public repository report works without an account](https://gitdebt.com/about). If you maintain a project, you can also copy an embed from the [README badges page](https://gitdebt.com/badges).

Stars say a repository is popular. Its commits say whether it is alive.

## Sources

[The WyCash Portfolio Management System (Ward Cunningham…](http://c2.com/doc/oopsla92.html) · [Metric definitions (SonarQube documentation)](https://docs.sonarsource.com/sonarqube-server/user-guide/code-metrics/metrics-definition/) · [DORA's software delivery metrics: the four keys](https://dora.dev/guides/dora-metrics-four-keys/) · [Use of relative code churn measures to predict system…](https://www.microsoft.com/en-us/research/publication/use-of-relative-code-churn-measures-to-predict-system-defect-density/) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [Technical Debt, Martin Fowler](https://martinfowler.com/bliki/TechnicalDebt.html) · [A Novel Approach for Estimating Truck Factors](https://arxiv.org/abs/1604.06766) · [Don't Touch My Code! Examining the Effects of Ownership on…](https://www.microsoft.com/en-us/research/publication/dont-touch-my-code-examining-the-effects-of-ownership-on-software-quality/)
