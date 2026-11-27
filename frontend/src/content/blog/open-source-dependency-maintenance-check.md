---
description: How do you run an open source dependency maintenance check? Check commits, authors, releases, security response and archived flags, then decide. Start today.
faq:
  - answer: A dependency counts as maintained when its maintainers visibly act on it through recent commits, recent releases, responses to security reports, and no archived or deprecated flag. No single signal proves it. Read the signals together, and treat the result as directional.
    question: What makes an open source dependency maintained?
  - answer: A repository with no recent commits is not necessarily abandoned, because a small, finished library can stay quiet for years. Check next whether releases, security responses and issue replies have also stopped, and whether the repository carries an archived flag.
    question: Is a repository with no recent commits abandoned?
  - answer: Having one maintainer adds some risk, because knowledge sits with one person and the cost of that person leaving is higher. For a small, stable library that level of risk can be acceptable. Ownership concentration is not a judgement about any person.
    question: Does one maintainer mean a dependency is risky?
  - answer: OpenSSF Scorecard alone is insufficient for judging a dependency, and it works best as a first pass. Read its result beside the commit trend, ownership and release history for the same repository, so that each signal is checked against the others.
    question: Can I rely on OpenSSF Scorecard alone?
  - answer: gitdebt does not tell you whether stars are fake, because it does not flag stars as fake, real or suspicious, and it does not score accounts. The star history shows what the crowd did. The health readings come from commits and show what maintainers did.
    question: Does gitdebt tell me if stars are fake?
publishDate: "2026-11-27"
tags:
  - open source
  - dependency management
  - software maintenance
  - supply chain security
  - openssf scorecard
  - bus factor
title: "Open source dependency maintenance check: 5 signals to read"
---

**An open source dependency maintenance check reads several independent signals for each dependency (commit cadence, who writes the commits, release history, security response, and archived or deprecated flags) and ends in a decision: keep, watch, pin, fork or replace. A dependency counts as maintained when its maintainers act visibly across those signals, which stars and downloads can't show. Each signal points in a direction, and none settles the question alone.**

A package can look fine in review: a few thousand stars, a healthy weekly download count, a README that reads well. Those two numbers are often all a team has when it adds a dependency to `package.json` or keeps one already in the lockfile. Both measure how many people use the project. Neither says what its maintainers did last month.

An open source dependency maintenance check is five checks, each a few minutes long, and it gives you a written decision per package that you're able to revisit every quarter.

> **Key Takeaways**
> - Take dates from the default branch's commit history, not a repository-level "last updated" timestamp.
> - [Scorecard's Maintained check](https://github.com/ossf/scorecard/blob/main/docs/checks.md) looks at a 90-day activity window, so a high score is a starting point.
> - One maintainer can suit a small, stable library, and a quiet repository can be finished rather than abandoned.
> - Two weak signals pointing the same way are enough reason to start comparing replacements.

## How do you run an open source dependency maintenance check?

You check whether an open source dependency is maintained by running five short checks on it and reading the results together, because no single signal proves maintenance.

1. Count commits on the default branch. Open the branch's commit list and tally commits per quarter. The [deptriage maintainer health guide](https://deptriage.online/blog/open-source-maintainer-health-check) lists "Cadence: commits per quarter on the default branch over the last 24 months" and "Release cadence: published versions in the last 12 months" among its five signals. Skip the repository-level "updated" date and push timestamps such as `pushed_at`, because a push to any branch moves them, and keep in mind that cadence counts activity, not quality.

2. Check who writes the commits. Open the contributors list and see how few authors account for half of the recent commits. One maintainer is normal for a small, stable library, and concentration shows where knowledge has pooled rather than a flaw in any person.

3. Read the release history. For npm packages, run `npm view PACKAGE time` to list every publish date, then look for a release inside your upgrade window. A release can be a version bump with no fix, and a package with no recent release may simply need no change.

4. Look at security response. Open the Security tab and check for a `SECURITY.md` file and published advisories. Note the gap between a report and the fixed release. An empty advisory list means none were filed, which is different from none existing.

5. Check archived and deprecated status. [GitHub says archiving a repository](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) "makes it read-only for all users," and an archived repository shows a banner on its page. Then read the README and pinned issues for the maintainer's own status notice. A missing flag doesn't mean the maintainer is active.

An open source dependency maintenance check ends with five short notes per dependency. One weak signal is a prompt to look closer. Two or more pointing the same way, such as no recent release and no security response, are enough to start comparing replacements.

## Do GitHub stars and npm downloads show a dependency is maintained?

GitHub stars and npm downloads show what users did, and neither shows whether anyone maintains the code. Stars come from people who bookmarked a repository; downloads come from people who installed a package. A maintainer doesn't have to do anything for either number to rise.

Maintainer activity lives in separate records: the commit history, the release list and the advisory record. Keep those apart from the audience counts and never blend the two into one score. Then check where they disagree.

- Heavy downloads with a quiet repository: the library may be finished and stable, or nobody may be tending it. Check the date of the last release, open security reports and whether issues get replies, since downloads measure usage only.
- Rising stars with flat commits: attention arrived, for example from a launch post or a trending list, and the commit trend didn't respond.
- Flat stars with steady commits: a small project that is quietly looked after.
- Steady stars with commits that stop: the star curve keeps its shape for months after the work ends, so it is a late signal. The commit trend is the earlier one.

Plot downloads against stars and against monthly commits, and the mismatch shows up in one view instead of two separate tables. To judge a dependency against its alternatives, overlay their star curves on the [compare page](https://gitdebt.com/compare) and read each commit trend beside its own curve.

gitdebt doesn't flag stars as fake, real or suspicious, and it doesn't score accounts or profile stargazers. The commit and release record is a second source that doesn't depend on star counts at all.

A quiet repository can also be a finished one. Treat each disagreement as a reason to read the release notes and security policy, the next stops in an open source dependency maintenance check.

## How do you read commit trend instead of one last-commit date?

You read commit trend by checking how commits move month by month, week by week and author by author, instead of trusting the date of one commit. A last-commit date can't separate a project with steady weekly work from one that got a single commit last Tuesday.

1. Open the report at `gitdebt.com/owner/repo`. If nobody has analyzed the repository yet, enter it on the [report page](https://gitdebt.com/report) and wait for the queued work. The page puts star history beside [four readings taken from the commit history](https://gitdebt.com/), starting with maintenance. Read maintenance as a direction.
2. Read the commit trend. Look at monthly commit volume: steady, declining, or one spike. A quiet line can mean a finished library, so check the release history before you judge.
3. Read the 52-week commit heatmap. Look for gaps of several empty weeks and for clusters. The heatmap shows when commits landed, not what they changed, so a run of automated dependency bumps looks like activity.
4. Check ownership concentration: how few contributors write half the commits.
5. Compare with a last-commit date. Write down both answers. Where the one-line date says "recent" and the trend says "thinning", you've found what the date hid.

For a worked walk-through, open [`gitdebt.com/angular/angular`](https://gitdebt.com/angular/angular). Find the star history first, then scroll to the commit trend and the 52-week heatmap beneath it. Read the three charts in the order above and write down your own reading, since live charts change. Repeat on `facebook/react` and `vitejs/vite` to see how the same views lay out for different projects.

Run the report on your next dependency. Note where trend and date disagree, because that gap is what an open source dependency maintenance check is for.

## What does bus factor tell you about a dependency's maintainers?

Bus factor, or truck factor, tells you how many contributors a project could lose before it stalls. gitdebt's ownership reading approximates it by counting how few contributors write half the commits. One maintainer can suit a small, stable library, so read concentration as context for the project.

The count is simple to do by hand. List contributors by commit total, highest first, and add them up until you pass 50% of all commits. If that takes one person out of 200 contributors, knowledge has pooled with that person. If it takes six, the work is spread out.

[gitdebt's methodology page](https://gitdebt.com/about) describes it this way: "Ownership concentration shows how few contributors it takes to write half the commits. It shows where knowledge has pooled in a codebase; it is not a judgement about any person." You can read it for any public repository in its [gitdebt report](https://gitdebt.com/report).

The reading has limits:

- Commit authorship doesn't capture review, issue triage or release work, so a quiet name in the commit log may still carry a lot of the load.
- A small, stable library, such as a string-padding or color-conversion utility, may need little more than one careful maintainer.
- The count says nothing about whether that maintainer is still active.

So read concentration next to commit trend. One person writing most commits every month is a small team doing its job. One person whose commits stopped a year ago is a dependency to watch more closely.

## What does OpenSSF Scorecard measure, and where does it stop?

OpenSSF Scorecard stops at practices. It doesn't read ownership concentration, trend over years, or whether a project is finished on purpose.

The [Scorecard checks documentation](https://github.com/ossf/scorecard/blob/main/docs/checks.md) lists the checks, including Maintained, Security-Policy, Code-Review, Branch-Protection, Pinned-Dependencies and Vulnerabilities. Most of them test how a project is run (review rules, workflow permissions, a published security policy) rather than whether anyone is writing code.

The maintenance check is named Maintained. The documentation says it "determines whether the project is actively maintained" and that "If the project is archived, it receives the lowest score." For other repositories it looks at activity "in the last 90 days". The check reads that one window, with no multi-year trend.

The 0-10 number is built in two steps. Each check returns its own score from 0 to 10, and the [Scorecard README](https://github.com/ossf/scorecard) describes combining them into one overall score by risk weight. Because the aggregate blends many unrelated checks, a repository can score well overall while its Maintained check is low, or the reverse. Read the per-check scores as well as the total.

Before trusting any number, open the Maintained entry in the checks documentation and read its definition and window. Then run the CLI against one dependency, for example `scorecard --repo=github.com/owner/repo --checks=Maintained`, and compare the result with that definition.

Scorecard describes practices at one point in time. A small library that is complete and quiet can score low on a 90-day window.

Other services answer different questions. [OSV](https://osv.dev/) describes itself as "A distributed vulnerability database for Open Source", so it covers advisories rather than how a project is run. Commit trend and ownership concentration cover what Scorecard leaves out, so read them next to its scores.

## How much should security response count in an open source dependency maintenance check?

Security policy and vulnerability response should count as one of five signals: enough to move a decision, not enough to make it alone. They show how maintainers behave when someone hands them a problem, which the other four signals can't show.

Dependencies carry other people's flaws into your build. The [OWASP Dependency-Check project](http://dependency-check.github.io/DependencyCheck/) describes itself as a solution to the OWASP Top 10 2021 entry A06:2021, "Vulnerable and Outdated Components". [Phenomic defines maintenance risk](https://www.phenomic.io/assessing-the-maintenance-risk-of-an-open-source-dependency/) as the chance that a project "responds slowly to security issues", among other failures.

For a single dependency, run three checks:

1. Find the policy. Open the repository and look for a `SECURITY.md` file. It should say where to send a private report and what the maintainers promise to do with it.
2. Find the advisories. Check the repository's published security advisories, then look the package up in an advisory database such as OSV.
3. Measure the gap. For each advisory, write down the date it was published and the date of the first release that contains the fix. A gap of days reads differently from a gap of months.

Then compare the fix against the commit history. Timing matters. A fix that lands inside steady weekly activity says more than one that appears after a year of silence.

An empty advisory list can mean no flaws were found or none were reported, and a database lists only what has been filed with it. A missing `SECURITY.md` isn't proof of neglect, since a small, stable library may simply have no process written down.

## Should you keep, watch, pin, fork or replace a dependency?

You decide whether to keep, watch, pin, fork or replace a dependency by matching its signal pattern to the option whose cost you can accept. That's the last step of an open source dependency maintenance check.

| Option | Signal pattern | Cost it carries |
|---|---|---|
| Keep | Steady commits and releases, security reports get answers, no archived or deprecated flag. A quiet repository for a small, stable library can fit here too. | A recheck on your normal review cycle. |
| Watch | Commit trend slowing, releases spacing out, or half the commits coming from one or two authors, with no archived flag. | A recheck date and the attention to keep it. |
| Pin | Maintainers respond, but releases bring breaking changes, or you need a known version while you plan a move. | Fixes no longer arrive on their own; you read advisories yourself. |
| Fork | Archived or abandoned, security reports unanswered, and the code is small enough for you to own. | You become the maintainer: fixes, releases and advisories. |
| Replace | Deprecated status, an archived repository with a named successor, or unanswered security reports in code you can't own. | Migration effort: code changes, tests and review. |

Each option has a short test:

1. Keep when the library does one small job and its issues are answered or closed. Quiet may then mean finished.
2. Watch when you can say whether the next check, on the date you set, should show the same trend or a different one.
3. Pin when you can name the version you're holding and the date you'll review the pin.
4. Fork when your team could read and fix the whole codebase.
5. Replace when an alternative passes the same five-signal check. You can run it on any repository from [the gitdebt report page](https://gitdebt.com/report).

A quiet repository, one dominant author or a slow release year each has innocent explanations. The final call depends on migration effort: the more code touches the dependency, the more evidence you should want before replacing it.

## Frequently asked questions

### What makes an open source dependency maintained?

A dependency is maintained when its maintainers visibly act on it: recent commits, recent releases, responses to security reports, and no archived or deprecated flag. No single signal proves it, so read them together and treat the result as directional.

### Is a repository with no recent commits abandoned?

Not always, because a small, finished library can stay quiet for years. Check whether releases, security responses and issue replies have also stopped, and whether the repository carries an archived flag.

### Does one maintainer mean a dependency is risky?

One maintainer means knowledge sits with one person, so the cost of that person leaving is higher. For a small, stable library that can be acceptable. Ownership concentration is not a judgement about any person.

### Can I rely on OpenSSF Scorecard alone?

Scorecard works as a first pass. Read its result beside commit trend, ownership and release history for the same repository.

### Does gitdebt tell me if stars are fake?

gitdebt does not flag stars as fake, real or suspicious, and it doesn't score accounts. The star history shows what the crowd did, and the health readings come from commits and show what maintainers did.

## Run an open source dependency maintenance check this week

- Today, pick the one dependency you'd hate to replace and run the five-signal check on it: commit cadence, who writes the commits, release history, security response, and archived or deprecated status.
- This week, open its report and read the commit trend, ownership concentration, and downloads against stars. Where the star curve and the commit history disagree, write down why.
- Each quarter, repeat the open source dependency maintenance check for every dependency you watch and record one decision for each: keep, watch, pin, fork or replace. Write your reasoning next to the label so the next review can test it.

Look up the dependency on [gitdebt](https://gitdebt.com/) by its owner/repo slug.

## Sources

[OpenSSF Scorecard checks documentation](https://github.com/ossf/scorecard/blob/main/docs/checks.md) · [Free Tools to Check Open Source Maintainer Health Before](https://deptriage.online/blog/open-source-maintainer-health-check) · [Archiving repositories - GitHub Docs](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [OpenSSF Scorecard README](https://github.com/ossf/scorecard) · [OSV](https://osv.dev/) · [About - OWASP Dependency-Check](http://dependency-check.github.io/DependencyCheck/). Every other source is linked where the article cites it.
