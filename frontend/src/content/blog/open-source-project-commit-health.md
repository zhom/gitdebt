---
description: "What is open source project commit health? It is what a repo's git history says about maintenance, read as four signals. Learn to check it in five steps."
faq:
  - answer: "A repository with no recent commits is not necessarily abandoned. A finished, stable project can go quiet because nothing needs changing, while an abandoned one is quiet because no one is maintaining it. Commit cadence alone can't separate them. Check releases, how issues and pull requests are handled, and any archive or deprecation notice alongside the commit trend."
    question: Is a repository with no recent commits abandoned?
  - answer: "There is no fixed number of contributors that separates a healthy project from an unhealthy one. A small count shows where knowledge has pooled in the codebase, and it isn't a judgement about any person. Author identity, squash merges, bots and work done outside commits can all skew the count."
    question: How many contributors should write half the commits in a healthy project?
  - answer: More commits do not make a project healthier, since commit counts measure activity and say nothing about quality. One commit can be a typo fix and another a large rewrite, and a single change can be split into many. Read volume as cadence over time, then check what the commits changed.
    question: Do more commits mean a healthier project?
  - answer: You can check commit health for a repository without signing in. On the gitdebt report page, you enter an owner/repo and no account is needed. A repository nobody has analyzed yet is queued first, so its readings can arrive after a short wait.
    question: Can I check commit health for a repository without signing in?
  - answer: A fix-labelled commit does not mean there was a bug. It only has a message that looks like a fix, and that includes typo and lint changes. Some real repairs carry no such label. Read the share as repair load, a direction to investigate, not a defect count.
    question: Does a fix-labelled commit mean there was a bug?
publishDate: "2026-11-26"
tags:
  - commit health
  - open source
  - bus factor
  - git history
  - dependency evaluation
  - repository maintenance
title: "Open source project commit health: Read four signals in minutes"
---

**Open source project commit health is what a repository's git history says about how it is maintained, read as four directional signals: maintenance, ownership, repair load and debt markers. None of them is a score or a verdict. Commits record what maintainers changed; they cannot show code quality or how quickly anyone responds to issues and pull requests.**

Developers weighing a dependency often see a high star count and nothing that says whether anyone still maintains the code. Stars record what the crowd did. Commits record what the maintainers did, and commit health reads only the commits, kept apart from the stars.

Reading open source project commit health from a public repository's history takes a few minutes and gives a grounded, caveated view of its maintenance. Start by pasting an owner/repo into the [report page](https://gitdebt.com/report).

> **Key Takeaways**
> - Compare a recent window of commits with the full history: a recent window alone can't separate a slowdown from the project's normal rhythm.
> - Ownership concentration shows where knowledge has pooled in the codebase. Author identity, squash merges and bots can blur it.
> - A quiet repository can be finished or abandoned, and commit cadence alone can't settle which.
> - Commits can't show issue and PR response or releases, so read those separately.

## What is open source project commit health?

Open source project commit health is measured from four readings of the commit log: commit cadence over set windows, how few contributors write half the commits, the fix-labelled share of changes, and TODO/FIXME movement. Together they describe maintenance as directional signals.

![Illustration for What is open source project commit health?](https://api.birchseek.com/m/iCymYQyPppZtPvVLYyt-h0HtnOhTJCtM.webp)

Cadence means commits per period, measured over defined windows such as the most recent months against the full history. A recent window alone can't separate a slowdown from a project's normal rhythm, so the full history sets the baseline.

Two charts carry cadence. The commit trend plots monthly commit volume, and you read it for direction: rising, flat or falling against the project's own past.

The commit heatmap shows when commits landed. It doesn't show who committed, how large the changes were, or whether they were good.

Four readings of open source project commit health come from the same history:

- **Maintenance:** recent commit activity.
- **Ownership:** how few contributors write half the commits.
- **Repair load:** the fix-labelled share of file changes. It describes fix-labelled history and doesn't prove a bug.
- **Debt markers:** whether TODO and FIXME comments are being added or removed over time.

The four readings stay separate instead of being folded into one grade, so you weigh them yourself. The [about and methodology page](https://gitdebt.com/about) documents how each reading is computed.

Suppose you open the commit heatmap for `facebook/react`. A steady weekly rhythm would suggest ongoing maintenance.

A long gap would raise a question: a release freeze, work moved to another repository, or a real slowdown. Neither pattern proves anything, because a heatmap can't show review, discussion or work outside commits. Check the commit trend and the full history before you conclude.

## What does bus factor show about open source project commit health?

Bus factor in a commit history shows how few contributors account for most of the changes, which marks where knowledge has pooled in a codebase. The name comes from a blunt measure: how many people could leave before nobody understands the code.

To read it from git, count commits per author and sort from most to least. Add them up until you pass half of all commits.

The number of authors you needed is the reading. A result of 2 means a small group wrote most of the history, and a result of 15 means the work is spread out.

Ownership concentration shows where knowledge has pooled in a codebase; it is not a judgement about any person. A founder who wrote most of a young project's early commits is an ordinary pattern.

You can open the ownership reading for a real repository such as [angular/angular](https://gitdebt.com/angular/angular) and ask questions of it. If a reading were concentrated, you'd ask who besides the top authors can review changes to core modules and who else can cut a release.

If it were spread out, you'd ask whether each area of the code still has more than a couple of people who know it.

The reading has limits, and each one can move the number in either direction:

- Author identity: one person can commit under several names or emails, which splits their commits and makes ownership look wider than it is.
- Squash merges: a pull request collapses into one commit credited to one author, so co-authors and review rounds disappear from the count.
- Bots: dependency updaters and release automation add commits, and they distort the ranking unless they're filtered out.
- Work outside commits: review, design, triage, support and mentoring leave no commits, so the history can't show who carries them.
- Commit size: a one-line typo fix counts the same as a rewrite, so commit counts say nothing about quality.

Every tool that reads commits applies its own rules about which branches to include and how to attribute authors. Check those rules before you compare two numbers from two tools.

Treat the result as a prompt for questions to ask the project. It can't tell you who understands the code.

## What do repair load and debt markers show, and what do they miss?

Repair load and debt markers show how much of a repository's history goes to fixes and to notes left in the code, and they miss whether that work was needed, finished or well done. Churn sits beside them, because it shows where those fixes land.

1. **Repair load (fix-labelled changes).**
 - Definition: the share of file changes that sit in commits whose message matches a fix heuristic.
 - Computed by: matching commit messages against fix wording, then counting the files those commits touched against all file changes.
 - Question it raises: do the same files keep appearing in fix-labelled commits?
 - Limit: the share depends on commit-message habits. A team that writes "fix" on every cleanup, or never writes it, will skew the number.

2. **File change frequency (churn).**
 - Definition: how many analyzed commits touched each file.
 - Computed by: a straight count per file across the analyzed history.
 - Question it raises: which files does the team revisit most, and is that where you'd expect the work to be?
 - Limit: it identifies frequently revisited files without claiming they were unstable or defective. A changelog or a lockfile can top the list for routine reasons.

3. **Debt markers (TODO/FIXME movement).**
 - Definition: whether TODO and FIXME comments are being added faster than they are removed.
 - Computed by: counting marker lines that appear and disappear in recent changes, set against an earlier stretch of history.
 - Question it raises: is the team clearing its own notes, or leaving more behind?
 - Limit: the movement depends on how a team writes comments. A project that tracks work in an issue tracker may show few markers, and another may leave old ones in place for years.

The exact fix heuristic and the way TODO/FIXME movement is counted are written up on the [methodology page](https://gitdebt.com/about). Read them before you compare two repositories, because a different heuristic gives a different share.

The open source project commit health readings work best in combination. If a dependency shows a high fix-labelled share and the same few files top the churn list, open the history of those files and read the commit messages.

## Is a quiet repository finished or abandoned?

A quiet repository is finished when its maintainers have deliberately stopped changing it, and abandoned when they've stopped without saying so, and commit cadence alone can't tell the two apart. Both leave the same flat line in a commit trend.

The shape of the history gives a lean. A busy history followed by a long quiet recent window suggests something changed: maybe the code reached its goal, maybe the people left.

A full history that was always sparse suggests a small project whose quiet is its normal rhythm. Compare the recent window with the full history before you read anything into the flat stretch, starting with the commit trend in a [gitdebt report](https://gitdebt.com/report).

The evidence that settles the question sits outside the commits:

- **Archived status.** [GitHub Docs on archiving](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) says you can archive a repository "to make it read-only for all users and indicate that it's no longer actively maintained." An archived repository is a stated stop.
- **A README note.** An explicit line saying the project is deprecated, or feature-complete, comes from the maintainers themselves.
- **Releases.** A recent tagged release with notes shows someone is still shipping. Notes that say "no further features planned" point to a finished project.
- **Issues and pull requests.** Look for replies, labels and closures. A quiet repository where maintainers still answer is being looked after; one where bug reports and PRs sit unanswered for months is likely abandoned.

A finished dependency still deserves a check on its security reports and its compatibility with your runtime, because stable code ages against the world around it.

Read the trend first, then use the archive flag, the README, the releases page and the issue tracker to decide.

## How do you check open source project commit health step by step?

You check open source project commit health in five steps, about five minutes in total, using one report and one visit to GitHub. The example repository is `vitejs/vite`, and each step asks one question of its history.

1. **Open the report and enter the repository.** Go to the [gitdebt report page](https://gitdebt.com/report) and type `vitejs/vite` as owner/repo. You're done when the commit trend, heatmap and four readings appear.
2. **Compare recent months with the full history, then scan the heatmap.** Check whether the latest months look like this project's usual months. Then check the heatmap for rhythm, such as steady weeks or bursts. A commit count measures activity, not quality.
3. **Read ownership.** Note how few contributors write half the commits. Author identity, squash merges, bots and work done outside commits all skew this reading.
4. **Read repair load and debt markers.** Note what share of changes is fix-labelled, and whether TODO/FIXME notes are growing or shrinking. Churn doesn't prove instability, and neither marker shows whether the work was needed or finished.
5. **Confirm with evidence outside the commits.** Open the repository on GitHub and look at recent releases, and at how issues and pull requests get answered. A quiet commit line beside recent releases and answered issues points toward finished. Silence in all three points toward abandoned.

Keep your read of open source project commit health as four separate directional signals, and keep star counts out of it.

To practice, [analyze a repository on the gitdebt report page](https://gitdebt.com/report), starting with `vitejs/vite` or a dependency you're weighing.

## Commit health vs stars, issues, PRs and releases: what does each show?

Each signal records a different actor: stars record the crowd, commits record maintainers and contributors, issues and PRs record reporters and maintainers, and releases and CI record what maintainers shipped.

| Signal | Stars | Commits | Issues and PRs | Releases and CI |
| --- | --- | --- | --- | --- |
| Who produced it | The crowd | Maintainers and contributors | Reporters and maintainers | Maintainers |
| Readable from git history | No | Yes | No | No |
| Where to read it | Star history | Commit trend, heatmap | Issue tracker, GitHub API | Releases page, Actions |
| What it can't show | Whether anyone maintains the code | Whether anyone answers users | Whether code changed | Whether the code kept moving between releases |

[OpenSource.net](https://opensource.net/measure-open-source-project-health/) puts the limit on stars plainly: "While popularity measurements might be interesting and exciting, they don’t often provide any real insight into project health".

Keeping the two series apart makes the gap readable. A star curve that keeps climbing while the commit trend flattens means attention outran maintenance. Flat stars over steady commits describe a quiet project that is still looked after.

The [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks) lets you set star history beside each repository's commit trend and look for that kind of divergence.

Response times cannot come from commits, because a commit records a change landing and no reply to anyone. The [GitHub org-metrics-dashboard project](https://github.com/github-community-projects/org-metrics-dashboard), for example, pulls its "Metrics around response times for issues and PRs" from the GitHub API.

CI status is separate too. [GitHub Docs on workflow badges](https://docs.github.com/en/actions/how-tos/monitor-workflows/add-a-status-badge) says a workflow status badge "shows whether a workflow is currently failing or passing". A commit history can't tell you whether tests pass.

Two frameworks cover more than commits do. [OpenSSF Scorecard](https://github.com/ossf/scorecard/blob/main/docs/checks.md) documents a Maintained check, which "determines whether the project is actively maintained", and it also has checks such as Contributors and CI-Tests. The maintenance reading from commits overlaps with the first of those only.

The [CHAOSS Starter Project Health metrics model](https://chaoss.community/kb/metrics-model-starter-project-health/) is the broader frame, with commits as one input.

README, LICENSE, CODE_OF_CONDUCT and SECURITY files are another signal that commit history doesn't record. The repository's community profile on GitHub shows which of them exist.

Each signal is best for one decision:

- Stars: gauging how much attention a repository has drawn.
- Commits: checking whether maintainers are still changing the code, and how concentrated that work is.
- Issues and PRs: judging how quickly a maintainer replies to you.
- Releases and CI: deciding whether you can pin a tested version.

## Frequently asked questions

### Is a repository with no recent commits abandoned?

Not necessarily. A finished, stable project can go quiet because nothing needs changing, while an abandoned one is quiet because no one is maintaining it. Check releases, issue and pull request handling, and any archive or deprecation notice alongside the commit trend.

### How many contributors should write half the commits in a healthy project?

No fixed number separates healthy from unhealthy. A small count shows where knowledge has pooled in the codebase, and it isn't a judgement about any person. Author identity, squash merges, bots and work done outside commits can all skew the count.

### Do more commits mean a healthier project?

Commit counts measure activity, not quality. One commit can be a typo fix and another a large rewrite, and a single change can be split into many. Read volume as cadence over time, then check what the commits changed.

### Can I check commit health for a repository without signing in?

You can. A repository nobody has analyzed yet is queued first, so its readings can arrive after a short wait.

### Does a fix-labelled commit mean there was a bug?

No. A fix-labelled commit only has a message that looks like a fix, and that includes typo and lint changes. Some real repairs carry no such label, so read the share as repair load, a direction to investigate.

## What should you do next to track open source project commit health?

The next step is to run the routine on one repository you depend on today, then widen it to the evidence commits can't show.

- Now, five minutes: pick one dependency you rely on and open its report. Read recent maintenance against the full history, then check how few contributors write half the commits.
- This week: open the issue tracker, the pull request queue and the release page for the same repository, and note how quickly people get answers and how recently a release shipped.
- This month: if you maintain a project, add a gitdebt embed to your README, so anyone evaluating it sees the same readings you would look for.

You can also look up any public repository from the [gitdebt home page](https://gitdebt.com/). When you're ready for your own README, the [README embeds at gitdebt.com/badges](https://gitdebt.com/badges) are copy-paste snippets for the health charts.

## Sources

[Archiving repositories - GitHub Docs](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) · [Measure open source project health](https://opensource.net/measure-open-source-project-health/) · [github-community-projects/org-metrics-dashboard](https://github.com/github-community-projects/org-metrics-dashboard) · [Adding a workflow status badge](https://docs.github.com/en/actions/how-tos/monitor-workflows/add-a-status-badge) · [OpenSSF Scorecard checks](https://github.com/ossf/scorecard/blob/main/docs/checks.md) · [CHAOSS Metrics Model: Starter Project Health](https://chaoss.community/kb/metrics-model-starter-project-health/) · [gitdebt badges](https://gitdebt.com/badges)
