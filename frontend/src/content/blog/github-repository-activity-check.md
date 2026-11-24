---
description: How do you do a GitHub repository activity check? Read archived status, last commit, cadence, releases and replies together. Start with the five-step check.
faq:
  - answer: To check GitHub repository activity, open the commits page for the last commit date, then Insights for Pulse and contributors, and the Releases page for release dates. Read commit cadence, release cadence, and issue and pull request replies together. Each signal is directional, so also check whether the repository is archived.
    question: How do I check if a GitHub repository is still active?
  - answer: No universal cutoff exists. A stable, finished library can stay quiet for months and still work fine. Read the gap beside release dates and whether maintainers still reply to issues and pull requests, because a single threshold on days since the last commit misleads.
    question: How long without a commit means a repository is abandoned?
  - answer: "You can't see traffic for a repository you do not own, because GitHub limits traffic data to people with push access. For a third-party dependency, read the public signals instead: commit dates, release dates, and how quickly maintainers answer issues and pull requests."
    question: Can I see traffic for a repository I do not own?
  - answer: "An archived repository is the maintainer's declaration that it is read-only and no longer actively maintained. Treat the archived banner as a prompt to start planning a replacement, and check whether the README names a successor before you make that decision."
    question: Does an archived repository mean it is dead?
publishDate: "2026-11-24"
tags:
  - github
  - repository health
  - dependencies
  - commit history
  - open source maintenance
title: "GitHub repository activity check: 5 signals to read in order"
---

**A GitHub repository activity check starts with the archived status, then the last commit date, commit cadence, latest release and maintainer replies on issues and pull requests. A repository is alive when recent commits, releases and replies agree. Read stars separately, because they measure popularity and say nothing about maintenance. Each signal is directional and none is a verdict.**

Developers adding a third-party package often open its GitHub page, see a large star count, and still can't tell whether anyone answers issues or ships fixes.

A GitHub repository activity check reads a few signals in a fixed order. With it you can judge one repository, line up several candidates in one grid, and know what each signal can't tell you.

> **Key Takeaways**
> - [GitHub's traffic data](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) needs push access, so it can't be used to judge a repository you don't maintain.
> - The archived banner is the one official abandonment signal, so check it before reading anything else.
> - One cutoff on days since last commit misjudges finished, stable libraries and abandoned ones alike.
> - Record each signal on its own line, because disagreement between signals tells you what to check next.

## How do you run a GitHub repository activity check on a dependency?

1. **Check the archived state.** Look for the archived banner at the top of the repository page. The [GitHub Docs page on archiving repositories](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) says archiving lets owners "make it read-only for all users and indicate that it's no longer actively maintained". An archived repository has answered your question, so stop and shortlist a replacement.
2. **Read the last commit date on the default branch.** Open `https://github.com/OWNER/REPO/commits` in a browser, or call the REST API commits endpoint. Check the default branch, because a recent commit on a side branch says little about the code you install.
3. **Read the latest release date.** Open the Releases page, or request `/repos/OWNER/REPO/releases/latest` from the API. A repository with no releases may publish through tags, so a missing release is a prompt to look further.
4. **Read the cadence, the replies and the authors, in that order.** Count commits per month over the past year: one commit after 14 months of silence differs from steady monthly work. Then open the 10 newest issues and pull requests and note which got a reply or a merge. Last, see who writes the commits.
5. **Write one line per signal.** Record each reading separately, for example "archived: no; last commit: recent; last release: 14 months ago; replies: slow; authors: concentrated". A single pass or fail hides the disagreement between signals, and that disagreement is the useful part.

![Illustration for How do you run a GitHub repository activity check on a dependency?](https://api.birchseek.com/m/p5NXs-ODIjlHehvAAX0xI_fZcqjSct9W.webp)

From a terminal, step 2 is one documented call:

```bash
curl "https://api.github.com/repos/OWNER/REPO/commits?per_page=1"
```

Swap in the real owner and repository names, such as `facebook/react`. The `per_page=1` parameter limits the response to a single commit, and that commit's date is the last commit date on the default branch.

## What do GitHub's built-in activity views show, and what do they miss?

GitHub's built-in activity views each cover a narrow scope, and none of them alone shows whether a repository you don't own is maintained. [GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository) says that "Repository graphs help you view and analyze data for your repository." Read each one with its scope in mind.

1. Pulse: GitHub Docs says you can use Pulse to see "an overview of a repository's pull request, issue, and commit activity." It summarizes a chosen window, so it shows what happened lately and nothing about whether the pace is rising or falling over years.

2. Contributors graph: it shows who has committed to the repository. For a dependency, scan it for whether a few names account for most of the bars. It counts commits, so it can't show how much effort or review sat behind each one.

3. Commit and code frequency: these two charts plot commits and lines changed over time, so they show whether work arrives steadily or in bursts. A single large reformatting commit looks the same as weeks of feature work, so a tall bar isn't proof of progress.

4. Activity view: [GitHub Docs describes the activity view](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/using-the-activity-view-to-see-changes-to-a-repository) as "a detailed history of changes to a repository, such as pushes, merges, force pushes, and branch changes." You can filter by branch, user, activity type and time period. It lists events, so it can confirm that something was pushed last week but not whether the work was meaningful.

5. Traffic: [GitHub's traffic documentation](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) says "Anyone with push access to a repository can view its traffic." A consumer of a dependency usually has no push access, so this view can't help you judge a library you only install.

Three things a dependency check needs appear in none of these views:

- Release cadence: how often tagged versions reach users.
- Maintainer replies: whether issues and pull requests get an answer, and how fast.
- Authorship concentration: what share of commits comes from the top one or two authors. The contributors graph lists people, but you have to do the counting yourself.

That is why a GitHub repository activity check reads releases and replies alongside the commit history.

## Which signals does a GitHub repository activity check read?

Five signals show whether a GitHub repository is alive: last commit date, commit cadence, release cadence, issue and pull request responsiveness, and contributor concentration. Each one is directional, and each has a limit to read beside it.

1. Last commit date shows the latest change to the repository and nothing more. A dependency bot or a docs typo fix can be the most recent commit, so open that commit and check what it touched.
2. Commit cadence shows rhythm across months: steady, bursty or tapering. Commit size and kind vary, so a run of one-line edits and a run of feature work look the same on a count.
3. Release cadence shows whether work reaches users. Some projects ship from the default branch, or tag releases rarely by design, so a sparse releases page can describe a deliberate habit.
4. Issue and pull request responsiveness shows whether maintainers reply to people. A reply is not a resolution, so read a few threads to see whether replies lead to merged or closed work.
5. Contributor concentration shows where knowledge has pooled, meaning how few people write most of the commits. It describes how the history is distributed and is not a judgement about any person.

A rule such as "treat the repository as dead after N days without a commit" collapses these five readings into one number. That number misleads in both directions.

A finished, stable library with a narrow scope can stay quiet for long stretches and still be sound. An abandoned repository can show a recent automated commit and look active.

Read together, the signals point to the next check. If the last commit is old but maintainers still answer issues, the library may be finished rather than abandoned.

If commits look recent but issues sit unanswered and one account wrote nearly all of them, the activity may be a bot or a single person's last effort.

## Are stars a sign that a repository is maintained?

A popular repository is not necessarily a maintained one. Stars record what the crowd did, and commits record what maintainers did, so a star curve can keep rising while the commit history stays quiet, or the reverse.

The two come from independent sources. Read them separately and never merge them into one score. A star is one visitor's click; a commit is a change a maintainer recorded in the project's history.

gitdebt reads star history from GitHub's own star history, as exact daily counts net of unstars. Older series built from historical data are re-read from that source as gitdebt reaches them, as its [methodology page](https://gitdebt.com/about) describes.

Its health readings come from the commit history instead, so the star history and the health readings never feed each other.

To see the two sources side by side, open the [report for `angular/angular`](https://gitdebt.com/angular/angular). Whether the two agree or part ways is something to read off the charts on the day you look.

A rising star curve over flat commits suggests attention is outpacing upkeep. A flat star curve over steady commits suggests a quiet but tended project. Confirm either reading with release dates and maintainer replies on issues.

## Can you check GitHub repository activity without installing anything?

Yes: a gitdebt report opens signed out at `gitdebt.com/owner/repo`, so a GitHub repository activity check needs nothing installed. [GitHub's activity view](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/using-the-activity-view-to-see-changes-to-a-repository) lists "pushes, merges, force pushes, and branch changes", while the gitdebt readings come from the git history.

1. **Open the report.** Go to `gitdebt.com/owner/repo`, for example the [angular/angular report](https://gitdebt.com/angular/angular). A repository nobody has analyzed yet shows a waiting notice and queues the work. Reload later before judging it, because that notice does not mean an empty history.

2. **Read the four readings with their limits.** The [gitdebt methodology page](https://gitdebt.com/about) states each one:
 - Maintenance is recent commit activity. A finished, stable library can read quiet without being abandoned.
 - Ownership is how few contributors write half the commits.
 - Repair load is the fix-labelled share of file changes. It's a commit-message heuristic and does not prove a bug.
 - Debt markers track TODO/FIXME movement. A rise or fall says comments changed, not that quality did.

3. **Check file change frequency.** This chart counts how many analyzed commits touched each file. It identifies frequently revisited files without claiming those changes were unstable or defective.

4. **Pull the same data for scripts.** Fetch `https://api.gitdebt.com/api/repos/OWNER/REPO/health.json` to compare several dependencies in one loop, with no token.

5. **Treat the result as a direction.** The readings are computed from a shallow clone. Set them beside the last commit date, release cadence and maintainer replies from your earlier checks, and don't let any single reading decide.

6. **Share the reading in your README.** Paste this snippet and swap `OWNER/REPO`. The light and dark drawings sit in one `picture` element, and the `?ref=readme` link stays around the image. It needs no token, account or build step.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <img alt="Repository chart for OWNER/REPO" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

The snippet works when the chart renders on your repository page in both color themes. Other health charts are listed on the [README embeds page](https://gitdebt.com/badges).

Open a report for the dependency you're about to add and read its four readings before you commit to it.

## How do you compare repositories with a GitHub repository activity check?

Choose the candidates and fix one time window. Then run the same GitHub repository activity check on each and record the results in a single grid.

| Criterion | Star overlay | Commit-based readings |
| --- | --- | --- |
| Data source | Star history | Commit history |
| Repositories per view | Several | One report per repository |
| Question answered | Popularity | Maintenance |
| Time alignment | Calendar date or days since first star | One window you fix |

**Best for, commit-based readings:** deciding which shortlisted candidate maintainers are still working on.

Record six signals for every candidate: archived state, last commit date, commit cadence, latest release date, issue and pull request replies, and ownership concentration. Use the same window for all of them, such as the last 12 months.

Put stars in their own column and fill it last, so a large count can't offset a quiet commit history.

Aligning curves to days since first star starts every repository at the same point. An older and a newer project are then compared over the same early days.

Switch to days since first star and note which curves rise fastest in the first stretch. Then open each repository's report and fill the other six columns from its commit history. If two repositories look alike on stars, the commit columns are where they separate.

The overlay shows popularity only. [gitdebt reports](https://gitdebt.com/) pair the star history with four health readings taken from the commit history, so those readings have to be compared repository by repository, and each one is directional.

## Frequently asked questions

### How do I check if a GitHub repository is still active?

Check whether the repository is archived, then open the commits page for the last commit date, Insights for Pulse and contributors, and the Releases page for release dates. Read commit cadence, release cadence, and issue and pull request replies together, since each signal is directional.

### How long without a commit means a repository is abandoned?

No universal cutoff exists. A stable, finished library can stay quiet for months and still work fine. Read the gap beside release dates and whether maintainers still reply to issues and pull requests, because a single threshold on days since the last commit misleads.

### Can I see traffic for a repository I do not own?

You can't, because [GitHub limits traffic data](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) to people with push access. For a third-party dependency, read the public signals instead: commit dates, release dates, and how quickly maintainers answer issues and pull requests.

### Does an archived repository mean it is dead?

An [archived repository](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) is the maintainer's declaration that it is read-only and no longer actively maintained. Treat the banner as a prompt to plan a replacement, and check whether the README names a successor before you decide.

## What should you do next, today and this quarter?

- Today: run the five-step GitHub repository activity check on the dependency you're weighing. Write one line per signal: archived state, last commit date, commit cadence, latest release, and maintainer replies on issues and pull requests.
- This week: shortlist two or three alternatives and fill the same grid for each. Use the same time window for every candidate so the lines are comparable.
- This quarter: re-read the signals for the dependencies you already rely on. Treat a change in cadence as a prompt to look closer at releases and replies.

Start your GitHub repository activity check with the dependency on your mind.

## Sources

[Viewing traffic to a repository - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) · [Archiving repositories - GitHub Docs](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) · [Viewing activity and data for your repository - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository) · [Using the activity view to see changes to a repository](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/using-the-activity-view-to-see-changes-to-a-repository) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/)
