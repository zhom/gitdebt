---
description: "How to evaluate open source library maintenance: read the last commit date, commit trend and ownership, then releases, issues and archived status. Start here."
faq:
  - answer: "Read the default branch's commit history: the last commit date, the monthly trend over the past year and who wrote the changes. Then check releases, issue handling and archived status. No single signal decides it, so treat the readings as directional and confirm with release notes and the maintainers' own statements."
    question: How do I know if an open source library is still maintained?
  - answer: "A library with no commits for a year is not necessarily abandoned, because a finished library can stay quiet. Check whether issues still get replies, whether releases or compatibility fixes still ship, and whether the repository is archived. No single signal decides it, so confirm with release notes and the maintainers' own statements."
    question: Is a library with no commits for a year abandoned?
  - answer: There is no fixed number of contributors that makes an open source project healthy. Look at how few contributors write half the commits, which shows how concentrated the work is. The OpenSSF guide notes that many widely used projects have only one maintainer.
    question: How many contributors should a healthy open source project have?
  - answer: GitHub stars do not show that a library is well maintained. Stars measure the crowd, and commits measure the maintainers. A high star count says people noticed the project, not that anyone still fixes it. Commit history shows the last commit date, the monthly trend and who wrote the changes.
    question: Do GitHub stars mean a library is well maintained?
  - answer: "gitdebt does not tell you whether a repository's stars are fake. It doesn't flag stars as fake, real or suspicious, and it doesn't score accounts. It charts star history beside readings measured from commits, so the crowd's attention can be compared with maintainer activity."
    question: "Does gitdebt tell me whether a repository's stars are fake?"
publishDate: "2026-11-01"
tags:
  - open source
  - library maintenance
  - commit history
  - bus factor
  - github
  - dependencies
title: "How to evaluate open source library maintenance: Check in order"
---

**To evaluate open source library maintenance, read the default branch's last commit date and monthly commit trend, check how few contributors write half the commits, then read releases, issues and archived status together. Stars and downloads measure the crowd, not the maintainers. Each reading is a directional signal, so when they disagree, check release notes, issues and the maintainers' own statements.**

You're about to add a dependency with a high star count and a tidy README, and one question is still open: is anyone looking after it? If nobody is, the fix or the upgrade becomes your team's job the day it breaks.

Four sources answer that question: commit history, ownership, releases and issue handling. When you evaluate open source library maintenance, a fixed order of checks settles the cases where they disagree.

> **Key Takeaways**
> - A quiet repository can be finished. Check issue replies, compatibility upkeep and releases before calling it abandoned.
> - A small core is ordinary: ownership concentration shows where knowledge has pooled.
> - Star history climbing while the commit trend falls means attention is outlasting upkeep.
> - Archived status is nearly binary: the repository is read-only, but its package can stay installable and popular.

## How to evaluate open source library maintenance from commit history

A library is still maintained when its default branch shows recent, steady human commits, and three measurements from the commit history show that. Fixed windows such as "three months" or "12 months" are rules of thumb. The [OpenSSF Concise Guide for Evaluating Open Source Software](https://best.openssf.org/Concise-Guide-for-Evaluating-Open-Source-Software.html) (revision dated 2025-03-28) is a published checklist, and its windows should be read the same way.

1. Read the last commit on the default branch. In a clone, run `git log -1 --format=%cI`. [Git prints the newest commit's committer date](https://git-scm.com/docs/git-log) in strict ISO 8601 form, for example `2025-03-28T09:14:02+00:00`.

 Open that commit and check its author. A commit date is not a push date, rebases and squash merges change dates, and one bot commit can make a dead repository look recent.

2. Count commits per month over the trailing year. Open the commit trend on the repository's report at `gitdebt.com/owner/repo`, which works for any public repository without sign-in.

 Squash-merge projects produce fewer, larger commits than projects that keep many small ones, and automated dependency-bump commits inflate counts. Compare a project only with its own past.

3. Read the direction of the trend: rising, flat, declining, or a long gap followed by a restart. The direction tells you more than any single threshold, because a flat, low line and a steep slide down to the same level call for different decisions.

4. Separate finished from abandoned.

 - A finished library has a small, stable scope and a flat or low-but-nonzero trend. Its issues still get triaged and it keeps up with compatibility changes.
 - An abandoned one shows a declining trend and unanswered issues and pull requests. It may also carry a deprecation notice or archived status.

If the readings disagree, open the release notes and recent issues, and look for a statement from the maintainers.

Commit history says nothing about issue handling or releases, so step 4 stays manual.

## How many people actually maintain the library?

The number of people who actually maintain a library shows in its ownership concentration: how few contributors write half of the repository's commits. A long contributor list can hide a small core. The reading shows where knowledge has pooled in a codebase; it is not a judgment about any person.

The truck factor literature formalizes the idea as the number of developers whose loss would stall a project. Avelino and colleagues defined it in their 2016 paper, ["A novel approach for estimating truck factors."](https://arxiv.org/abs/1604.06766) Their abstract describes the truck factor as the minimal number of developers that have to be hit by a truck (or quit) before a project is incapacitated.

The paper also says there was no consensus on how to calculate it, so any figure is an estimate.

You can measure a simpler version yourself:

1. Count commits per author. In a local clone, `git shortlog -sn` does this: the [git-shortlog documentation](https://git-scm.com/docs/git-shortlog) says `-s` gives a commit count summary only and `-n` sorts by number of commits.
2. Add the counts from the top author down until you pass 50% of all commits.
3. The number of authors you needed is the concentration reading.

A contributor list counts everyone who ever landed a change, including people who sent one fix and left. The concentration reading counts only the authors who wrote half the work, which a long list does not reveal.

Commit authorship has limits. It misses review, triage, design and release work. Depending on how a project merges, the person who merges can receive credit for someone else's change.

One person can also appear under several names or emails, which splits their count.

A small core is an ordinary finding. [The OpenSSF guide](https://best.openssf.org/Concise-Guide-for-Evaluating-Open-Source-Software.html) notes that many widely used projects have one maintainer, so a single-maintainer project is common and sometimes healthy. Ask what happens to the library if that person stops.

## What do stars and downloads say about library maintenance?

Stars and downloads record what the crowd did, and neither one records whether a maintainer is still working.

| | Stars | Downloads | Commit history |
| --- | --- | --- | --- |
| What it counts | Stargazer actions | Registry fetches | Changes to the default branch |
| Who acts | The crowd | Installers | Maintainers |
| Where to read it | Star history chart | Registry page | Commit trend and heatmap |

[TrustOSS](https://trustoss.org/blog/how-to-evaluate-open-source-library) calls both "popularity signals" and says "both are laggy, and one of them is actively being manufactured at industrial scale." A download count is a count of fetches, so an abandoned package can keep climbing as long as projects keep installing it.

gitdebt does not flag stars as fake, real or suspicious. It doesn't label accounts or score stargazers, and it stores star dates and day counts only. The [methodology page](https://gitdebt.com/about) lists what it deliberately leaves out.

Keep the two sources separate. Open the report at `gitdebt.com/owner/repo` and read the star history beside the commit trend, without blending them into one number.

When the two disagree, the disagreement is the finding. Stars still climbing while monthly commits fall means attention is outlasting upkeep. Commits holding steady while stars stay flat describes a quiet project that is still being worked on.

Neither pattern settles the question, so check release notes and issue replies next.

To see how a library's growth compares with its peers, use the [star history comparison](https://gitdebt.com/compare). It overlays up to eight repositories on one set of axes, by calendar date or by days since first star. Days since first star lines up a young repository and an old one at the same age.

Use stars to gauge how much attention a library has drawn and downloads to confirm that a package is actually being installed. Use the commit history to judge whether maintainers are still changing the code.

## How do issues, releases and archived status fit with commits?

Issues, releases and archived status show whether maintainers respond and ship, which commits alone can't show. Read each one beside the commit trend, and treat a mismatch as a lead to follow.

| Pattern | Plausible readings | What to check next |
| --- | --- | --- |
| Commits without releases | Work lands on the default branch; registry users get older code | Latest registry publish date against the last default-branch commit |
| Releases without commits | Release process runs from another branch, repository or automation | The release workflow and the branch or repository it builds from |
| Both quiet | Finished or abandoned | The earlier test: issue replies and compatibility upkeep |

On the tracker, check whether new issues get a first response, whether pull requests are reviewed or left open, and whether stale items are closed with a reason. A low response rate can reflect volume, and a clean tracker can mean a bot closes issues automatically.

For release cadence, compare GitHub releases or tags with the publish dates on the registry. The OpenSSF Best Practices Working Group's [Concise Guide for Evaluating Open Source Software](https://best.openssf.org/Concise-Guide-for-Evaluating-Open-Source-Software.html), dated 2025-03-28, frames its checks as questions for judging a dependency's security and sustainability, so keep release dates among them.

Archived status is the closest thing to a binary signal. [GitHub Docs on archiving](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) says archiving "makes it read-only for all users and indicates that it's no longer actively maintained."

The flag sits on the repository, so the package can still be installed from its registry. Check the registry page for a deprecation notice.

## How do you evaluate open source library maintenance with a repository report?

To evaluate open source library maintenance with a repository report, read the charts in a fixed order and note each chart's limit before you move on. Open `gitdebt.com/owner/repo` for any public repository.

1. Read the star history. It shows what the crowd did and says nothing about what maintainers did, so keep its slope out of your reading of the later charts.
2. Read the commit trend. Check monthly commit volume for a steady line, a decline or a stop. It counts commits, not their size, so a quiet stretch can describe a finished library.
3. Read the commit heatmap. Check whether work is spread across weeks or bunched into one burst.
4. Read contributors and ownership. Ownership is how few contributors write half the commits.
5. Read repair load and debt markers. Repair load is the fix-labelled share of changes, a message heuristic that doesn't prove bugs. File change frequency counts files touched, which doesn't mean those changes were defective. Debt markers show the direction that marker comments are moving, which says little about code quality.
6. Open stars versus downloads if the library is published to npm, crates.io, PyPI or Docker. A wide gap between the two is a lead to follow.

The analysis runs on a shallow clone, so it reads recent history rather than the project's full past.

For a walk-through, open the [angular/angular report](https://gitdebt.com/angular/angular) and follow the same order. Start with the star history. Then ask whether monthly commit volume holds steady, and check the heatmap for gaps.

Finish on contributors and ownership to see how widely the commits are spread. Where the crowd's curve and the maintainers' commits diverge, that divergence is what to investigate. To run the same pass on your dependency, [analyze a repository](https://gitdebt.com/report).

## How to evaluate open source library maintenance when signals disagree

When maintenance signals disagree, weigh them in a fixed order and treat the disagreement as the cue to read what the maintainers wrote. Apart from archived or deprecated status, no signal outranks the others.

1. Check archived or deprecated status first. Open the repository page and read the top of the README. If the repository is archived or the maintainers say it's deprecated, stop: every other reading is now background.
2. Read the last commit and the commit trend. Look at the default branch and the monthly commit volume. A quiet line can mean a finished library, so don't call it abandoned yet.
3. Compare releases with commits. Commits without releases and releases without commits point to different situations, so note which one you see.
4. Read issue and PR handling. Open a few recent issues and pull requests and check whether a maintainer replied. Judge replies, not the open count.
5. Set ownership concentration against how replaceable the library is for you. A small core matters more when swapping the library out would be expensive.
6. If readings still disagree, read the primary text. Go through the release notes, open issues and the maintainers' own statements before deciding.

Worked example, `request/request`: the [repository's README](https://github.com/request/request) opens with a deprecation notice that says "request is fully deprecated".

[GitHub Docs](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) says archiving makes a repository "read-only for all users".

Step 1 ends the evaluation. The star count says the project was popular, and the status says nobody can add to it. Start no new work on it and plan a migration for existing use. The decision rests on the maintainers' own statement.

A quiet library that isn't archived gets a different outcome: check its issues and release notes first.

## Where does maintenance fit beside security, license and dependency checks?

Maintenance answers one question about a dependency, whether anyone still looks after it, and four sources own the other questions.

1. OpenSSF Scorecard answers what an automated check says about the project's activity and practices. The [Scorecard checks documentation](https://github.com/ossf/scorecard/blob/main/docs/checks.md) defines the Maintained check this way: "This check determines whether the project is actively maintained." It is one check among the many Scorecard runs, so read it beside the others.

2. OSV answers how vulnerabilities in this package are published and fixed. Its advisory records are a security-response signal.

3. The [deps.dev documentation](https://docs.deps.dev/) says the service helps developers understand the structure, construction and security of open source packages.

4. The OpenSSF Concise Guide for Evaluating Open Source Software answers what else to check before adopting a dependency. The [guide](https://best.openssf.org/Concise-Guide-for-Evaluating-Open-Source-Software.html), revision dated 2025-03-28, is written to evaluate a dependency "for security and sustainability". It covers security practices, adoption and licensing checks beyond maintenance.

None of the four replaces reading the commit history, which records what the maintainers did.

## Frequently asked questions

### How do I know if an open source library is still maintained?

Read the default branch's commit history: the last commit date, the monthly trend over the past year and who wrote the changes. Then check releases, issue handling and archived status, and confirm with release notes and the maintainers' own statements.

### Is a library with no commits for a year abandoned?

Not necessarily, because a finished library can stay quiet. Check whether issues get replies, whether releases or compatibility fixes still ship, and whether the repository is archived.

### How many contributors should a healthy open source project have?

There's no fixed number. Look at how few contributors write half the commits. [The OpenSSF guide](https://best.openssf.org/Concise-Guide-for-Evaluating-Open-Source-Software.html) notes that many widely used projects have only one maintainer.

### Do GitHub stars mean a library is well maintained?

Stars measure the crowd, and commits measure the maintainers. A high star count says people noticed the project, not that anyone still fixes it.

### Does gitdebt tell me whether a repository's stars are fake?

gitdebt doesn't flag stars as fake, real or suspicious, and it doesn't score accounts. It charts star history beside readings measured from commits.

## What to do next with your shortlist of libraries

- Today: open the report for the library you're about to add. Read the star history, the commit trend and the contributors, then repair load and debt markers.
- This week: check releases, issues and any maintainer statements wherever a reading is ambiguous. Use OpenSSF Scorecard for security practices, OSV for advisories and deps.dev for dependents.
- Each quarter: re-read the commit trend for every dependency you keep. A flat stretch after steady months is a cue to check releases and issues.

To evaluate open source library maintenance across your shortlist, analyze any public repository at [gitdebt.com/report](https://gitdebt.com/report). It's free and needs no account. Lookups by repository name also start from the [gitdebt home page](https://gitdebt.com/).

## Sources

[Concise Guide for Evaluating Open Source Software](https://best.openssf.org/Concise-Guide-for-Evaluating-Open-Source-Software.html) · [git-log Documentation](https://git-scm.com/docs/git-log) · [A Novel Approach for Estimating Truck Factors](https://arxiv.org/abs/1604.06766) · [git-shortlog Documentation](https://git-scm.com/docs/git-shortlog) · [How to Evaluate an Open-Source Library Before You Depend on…](https://trustoss.org/blog/how-to-evaluate-open-source-library) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [Compare GitHub star history](https://gitdebt.com/compare) · [Archiving repositories - GitHub Docs](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories). Every other source is linked where the article cites it.
