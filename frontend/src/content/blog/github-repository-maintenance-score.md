---
description: A GitHub repository maintenance score is a reading of commit activity. Learn its signals and limits, then choose keep, pin, fork, replace or contribute.
faq:
  - answer: No universal good score exists, because each tool blends different signals into its number. Compare the commit-based maintenance reading with ownership, repair load and debt markers instead, and check which signals a tool combined. A directional reading is not a verdict on safety.
    question: What is a good GitHub repository maintenance score?
  - answer: A repository with no recent commits is not automatically abandoned, because a finished or feature-complete project can stay quiet for years. Check whether the repository is archived, whether issues still get replies, and whether the code still does its job before you label it abandoned.
    question: Does a repository with no recent commits count as abandoned?
  - answer: Stars are not a sign of maintenance, since they show popularity and say nothing about upkeep. They record what the crowd did, while commits record what the maintainers did, so gitdebt reads them from two separate sources. gitdebt does not flag stars as fake, real or suspicious.
    question: Are stars a sign that a repository is maintained?
  - answer: You can check whether a GitHub repository is still maintained by opening a report for it on gitdebt, which needs no account for any public repository. Read the maintenance reading first, then confirm the date of the last commit on the default branch.
    question: How can I check whether a GitHub repository is still maintained without installing anything?
  - answer: Bus factor describes how few contributors hold the knowledge needed to keep a project going. A low one shows where knowledge has pooled in the codebase; it is not a judgement about any person, and it matters most beside a quiet commit history.
    question: What is the bus factor of a GitHub repository, and is a low one bad?
publishDate: "2026-10-30"
tags:
  - github
  - repository maintenance
  - dependency management
  - bus factor
  - open source
  - commit activity
title: "GitHub repository maintenance score: What it can't show"
---

**A GitHub repository maintenance score is a directional reading of how actively a repository's maintainers commit, and it says nothing final about code quality. Stars record what the crowd did and commits record what the maintainers did, so read them as separate sources. Read the score beside ownership, repair load and debt markers, then pick one of five actions: keep, pin, fork, replace or contribute.**

Developers adding a dependency, or reviewing one someone else chose, often get a single grade with nothing behind it. A github repository maintenance score like that blends several signals into one number, so you can't tell which signal moved it.

After reading, you will be able to name the signals behind a score, know where each one falls short, and turn them into a decision about a dependency.

> **Key Takeaways**
> - Check the last commit on the default branch, not only `pushed_at` or `updated_at`, which the GitHub REST API lists as [separate repository fields](https://docs.github.com/en/rest/repos/repos).
> - Quiet isn't the same as abandoned: an owner sets a repository to read-only by [archiving it](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories).
> - Time to close an issue says more about responsiveness than a raw count of open issues.
> - Ownership concentration shows where knowledge has pooled in a codebase, and it is not a judgement about any person.

## What is a GitHub repository maintenance score?

A GitHub repository maintenance score summarizes how recently and how steadily a repository's maintainers have been committing. Commit cadence, issue handling, release cadence and file-hygiene checks are separate signals, and many tools blend them into one number without saying so.

Each signal in a GitHub repository maintenance score asks its own question:

- Commit cadence: is anyone writing code?
- Issue handling: does anyone reply to users?
- Release cadence: do changes ship?
- File hygiene: do a README, a license and a CI config exist?

Folded into one 0-100 or A-F figure, a drop in one signal hides behind the steady rest. Each blended input carries a limit you can read off the input itself:

- Flat issue-count penalties count open issues without separating bug reports from questions and feature requests, so a busy project can look worse than a quiet one.
- Hygiene-heavy checklists reward files that exist, and those files stay in place long after the last commit.
- Issue-responsiveness grading measures replies, so a small library with few reports gives it little to grade.

A maintenance reading does not measure code quality, security or popularity. It also can't show whether a project is finished, because a feature-complete library can go quiet without being abandoned.

gitdebt splits the picture into four named readings:

| Reading | What it tracks |
|---|---|
| Maintenance | Commit cadence |
| Ownership | How few contributors write half the commits |
| Repair load | The fix-labelled share of changes |
| Debt markers | Movement of to-do and fix-me comments in the code |

## Which signals go into a GitHub repository maintenance score, and where does each fall short?

A GitHub repository maintenance score draws on up to five signals. Each one answers a narrower question than its label suggests.

Recent commit activity is the first input to check in any GitHub repository maintenance score. It counts commits inside a window, and the window length changes the answer. A short window marks a repository between releases as quiet, while a long one hides a recent stop.

The documentation for [OpenSSF Scorecard's Maintained check](https://github.com/ossf/scorecard/blob/main/docs/checks.md) looks at activity "during the previous 90 days". Bot commits such as dependency bumps and trivial edits count the same as a meaningful change.

[gitdebt's maintenance reading](https://gitdebt.com/about) is commit cadence from the git history, and its commit trend and commit heatmap (last 52 weeks) show the pattern behind the reading. Those charts show the gaps and bursts that one figure hides.

Last push and last commit are different dates. [GitHub's REST API repository object](https://docs.github.com/en/rest/repos/repos) lists `pushed_at` and `updated_at` as separate fields. A push can land on any branch, so read the commit date on the default branch directly.

Issue and pull request handling reflects inbound demand as much as maintainer effort, because a popular project receives more reports than a small one with the same team. [CHAOSS publishes a Time to Close metric](https://chaoss.community/kb/metric-time-to-close/), which gives responsiveness a named definition.

A flat penalty per open issue distorts the reading. Duplicates, feature requests and stale reports count the same as defects, and the penalty grows with popularity. Closing time says more than a raw count.

Release cadence suits projects that ship versioned packages on a rhythm. These repositories may rarely tag a release and still be well kept:

- a finished library
- a repository that deploys from `main`
- a repository that holds documentation

A missing tag describes the release process. It says nothing about upkeep.

File-hygiene checks test for the presence of a license, a README and a CI config file. A repository can pass every one of them and have no commit in years, so these checks say nothing about activity.

## Stars vs commits: how is popularity different from upkeep?

A star count measures popularity. A commit history measures upkeep, which is what a GitHub repository maintenance score should track, so gitdebt reads them from two separate sources and never blends them.

A star records that someone liked the repository on the day they clicked. It stays in the total whether or not anyone still edits the code, so a star count can't tell you if the code is being maintained.

The two sources can disagree, and the disagreement is the useful part. A large star total beside thin recent commits and a modest total beside steady commits describe different situations, and a single blended number hides both.

Page views and clones would be closer adoption signals. GitHub's documentation on [viewing repository traffic](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) says that anyone with push access to a repository can view its traffic. For a dependency you don't own, that data isn't open to you, so traffic can't stand in for adoption.

gitdebt's star history comes from GitHub's own star history. Its four health readings come from commits, so [none of them depends on stars](https://gitdebt.com/about).

Star velocity, the stars gained over 1, 7 and 30 days on the [star velocity leaderboard](https://gitdebt.com/leaderboard), is a popularity measure and says nothing about maintenance.

gitdebt does not flag stars as fake, real or suspicious, and it does not score accounts. It stores star dates and day counts only.

To compare the two sources on one repository, open the [angular/angular report](https://gitdebt.com/angular/angular). Read the star history first, then the commit trend. The first shows how much attention the repository has drawn, and the second shows how recently its maintainers have committed.

## Does a low GitHub repository maintenance score mean abandoned, finished or archived?

A quiet repository can be finished, archived or abandoned, and commit counts alone can't tell you which. Each state calls for a different response, so separate them before you act on a low reading.

A finished project, such as a small library that pads a string or parses a date format, may need few changes once it works. Low commit activity there can mean little is left to do. A GitHub repository maintenance score that penalizes low activity cannot tell a finished project from a neglected one.

An archived repository is the easiest state to confirm, because the owner has declared it. [GitHub Docs says](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) you can archive a repository "to make it read-only for all users and indicate that it's no longer actively maintained."

In an abandoned repository, nobody answers and problems pile up. Commits can't show an unanswered bug report or a pull request waiting for review.

Four checks separate the states:

1. Look at the repository page for an archived status.
2. Open the issues and pull requests tabs and see whether anyone replies, even to decline a request.
3. Read the README and recent issues for a statement that the project is complete or feature-frozen.
4. See whether dependency bumps and security fixes from contributors still get merged.

Repair load and debt markers add a commit-based view. A quiet repository with few fix-labelled changes and flat debt-marker movement fits a finished project. One where fix-labelled changes and markers keep growing fits a backlog.

Both readings are directional. Treat a low GitHub repository maintenance score as a prompt to run the four checks.

## What do ownership concentration and bus factor say about upkeep?

Ownership concentration shows how few people have written most of the code. It belongs beside a GitHub repository maintenance score, because upkeep depends on whoever remains when those people step away. It is a reading about the codebase, not a judgement about any person.

gitdebt's contributors chart and ownership concentration chart are both built from the git history of a shallow clone.

A low contributor count means commits have pooled with a few authors, and a high one means they are spread wider.

The peer-reviewed truck factor work comes from Avelino and colleagues. [Their truck factor paper](https://arxiv.org/abs/1604.06766) describes the truck factor as the minimal number of developers that have to be hit by a truck (or quit) before a project is incapacitated.

They note that there is no consensus about how to calculate it. Their own scope is a corpus of 133 popular project in GitHub.

Read the ownership chart with these limits in mind before you weigh it against a GitHub repository maintenance score:

- A commit credits an author. It doesn't record who reviewed the change or who understands the module, so authorship is not knowledge.
- A squash merge folds a pull request into one commit, so the tally follows the merge habit of the project.
- Automated dependency updates add commits and can inflate one name's share.
- Commits older than the shallow clone's depth don't appear in either chart, so earlier authors may be missing.

To see where commits have pooled in a repository you're weighing, open its report from [gitdebt's report page](https://gitdebt.com/report) and read the ownership reading beside the maintenance reading. [The methodology page](https://gitdebt.com/about) describes how each signal is computed.

## How do you turn a GitHub repository maintenance score into a decision?

You turn a GitHub repository maintenance score into a decision by reading its signals in a fixed order and ending on a single action for the dependency.

1. Open the report at `gitdebt.com/owner/repo`, for example `gitdebt.com/facebook/react`. Read maintenance, ownership, repair load and debt markers one at a time. Averaging them hides the disagreement that tells you the most.
2. Check the GitHub repository page for the archived banner; if it is there, skip to fork or replace, because a flat commit trend then has a known cause.
3. Open the commit trend chart, which shows monthly volume, and read it over a longer window than the headline reading. A steady, low line fits a finished project, while a drop after a busy stretch suggests a handover or a fade.

### Weigh ownership and repair load, then act

4. Read ownership concentration to see how few contributors write half the commits.
5. Check repair load and debt markers for the direction each one is moving. The fix label is a message heuristic, so a high repair load describes history and doesn't prove bugs.
6. Map the result to an action.

| Action | Signals pointing toward it |
|---|---|
| Keep | Steady or finished commit trend, owners spread across several people, repair load and debt markers flat or falling |
| Pin | Quiet trend with no archive flag, or repair load rising; lock a version and re-read later |
| Fork | Archived or stalled, but the code still fits your needs and you can carry it |
| Replace | Archived or fading, ownership pooled in one or two people, repair load and debt markers rising |
| Contribute | Commits active but ownership thin; your patches land where owners are scarce |

Keep, pin, fork and replace are choices about your own dependency. Contribute fits a project that is alive but short on owners. Treat the mapping as a starting rule and confirm it against the project's own issues.

To keep the evidence in front of your team, health charts and cards [embed in a README](https://gitdebt.com/badges) as plain public image URLs, with light and dark variants in one `<picture>` element.

```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
  <img alt="Repository health chart" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
</picture>
```

No JavaScript is needed to render it. A repository nobody has analyzed yet shows a placeholder and queues the work, and the real chart replaces it at the same URL.

To run the procedure now, open the [repository report page](https://gitdebt.com/report) and enter the dependency you're weighing.

## Frequently asked questions

### What is a good GitHub repository maintenance score?

No universal good score exists, because each tool blends different signals into its number. Check which signals a tool combined, then compare the commit-based maintenance reading with ownership, repair load and debt markers.

### Does a repository with no recent commits count as abandoned?

Not automatically, because a finished or feature-complete project can stay quiet for years. Check whether the repository is archived, whether issues still get replies, and whether the code still does its job before you label it abandoned.

### Are stars a sign that a repository is maintained?

Stars show popularity, which a GitHub repository maintenance score should not track. They record what the crowd did, while commits record what the maintainers did, so gitdebt reads them from two separate sources.

### How can I check whether a GitHub repository is still maintained without installing anything?

Read the maintenance reading first, then confirm the date of the last commit on the default branch.

### What is the bus factor of a GitHub repository, and is a low one bad?

Bus factor describes how few contributors hold the knowledge needed to keep a project going. A low one shows where knowledge has pooled in the codebase and is not a judgement about any person; it matters most beside a quiet commit history.

## What should you do next with a GitHub repository maintenance score?

Put a GitHub repository maintenance score to work on the dependency you trust least first. Then work outward over the quarter.

- **Today:** open the report for the dependency you are least sure about. Read its maintenance, ownership, repair load and debt markers separately.
- **This week:** check archive status and ownership for your top dependencies. Decide whether to keep, pin, fork, replace or contribute, and write the decision next to each one.
- **This quarter:** re-read the commit trend behind each decision. Then add a health chart to your own README so visitors can run the same check on your project.

Enter any owner/repo on [gitdebt's report page](https://gitdebt.com/report). [gitdebt is free, MIT-licensed](https://github.com/zhom/gitdebt) and open source.

## Sources

[REST API endpoints for repositories - GitHub Docs](https://docs.github.com/en/rest/repos/repos) · [Archiving repositories - GitHub Docs](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) · [OpenSSF Scorecard checks documentation: Maintained](https://github.com/ossf/scorecard/blob/main/docs/checks.md) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [Time to Close - CHAOSS](https://chaoss.community/kb/metric-time-to-close/) · [Viewing traffic to a repository - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) · [A Novel Approach for Estimating Truck Factors](https://arxiv.org/abs/1604.06766) · [README badges, star-history charts and embeds](https://gitdebt.com/badges). Every other source is linked where the article cites it.
