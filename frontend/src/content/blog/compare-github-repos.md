---
description: Need to compare GitHub repos? Append /compare to the URL for branches and forks, or use git diff for separate repos. Compare star and commit history too.
faq:
  - answer: "Yes, you can use a local clone. Add the other repository as a remote with git remote add, run git fetch for that remote, then run git diff between your branch and the remote's default branch. Remove the remote afterward with git remote remove."
    question: Can I diff two different GitHub repositories?
  - answer: "Append /compare to the repository's path, then pick branches from the base and compare dropdown menus. Base is the starting point, and compare is the endpoint. This comparison view on GitHub lists the commits and changed files between the two branches."
    question: How do I compare two branches on GitHub?
  - answer: Not necessarily. Stars record what the crowd did, not what maintainers are doing now. A popular project can be quiet, and a small one can be actively maintained, so read commit history beside the star count instead of relying on stars alone.
    question: Is the repository with more stars the better choice?
  - answer: You can overlay up to eight repositories at gitdebt.com/compare, then switch the axis on the chart to days since first star. This lets you compare the first 90 days of each repository, so every project gets read from an equal start.
    question: How do I compare the star history of several repositories?
  - answer: No, gitdebt does not flag stars as fake, real, or suspicious, and it does not score accounts. Fake stars are a known industry topic, which is one more reason to read stars beside commit history instead of relying on stars alone.
    question: Does gitdebt detect fake stars?
publishDate: "2026-10-25"
tags:
  - github
  - git diff
  - star history
  - repository health
  - dependency review
title: "Compare GitHub repos: diff code or choose the right project"
---

**To compare GitHub repos, first decide which question you have. To diff code, [append `/compare` to a repository's URL](https://docs.github.com/en/pull-requests/how-tos/commit-changes/comparing-commits) for branches, tags, commits and forks, or add a git remote and run `git diff` for two separate repositories. To choose between projects, read star history beside commit history, for example at [gitdebt.com/compare](https://gitdebt.com/compare).**

A search to compare GitHub repos usually means one of two jobs. You have two pieces of code and need a diff, or you have two projects and need to pick one. Each job needs different tools, and most guides cover only the first.

You will be able to produce a working diff between branches, tags, commits or separate repositories. You will also be able to run a side-by-side check on candidate projects, with the limits of each signal in view.

> **Key Takeaways**
> - For forks, write each side of the compare URL as `owner:branch`. Swapping base and compare reverses the diff.
> - The local `git diff` method needs no shared history and works with any default branch name.
> - Aligning star curves to days since first star puts repositories of different ages on equal footing.
> - When stars and commits disagree, read the recent commits instead of averaging the two signals.

## What does it mean to compare GitHub repos?

Comparing GitHub repos means one of two jobs: diffing code, or weighing whole projects against each other to choose one. The two jobs need different tools and different evidence.

![Image generated](https://api.birchseek.com/m/GRkuI8iRd5eyr53IoHveY-XjoSq_6N0E.webp)

A diff answers "what changed between these two states of code?" Its evidence is files, lines and commits. GitHub's compare page handles this job.

A project comparison answers "which of these should I rely on?" A code diff between two unrelated projects shows that they differ. It doesn't show which one is alive, popular or safe to depend on.

Here is the same phrase applied to each job:

- **Diff:** open `https://github.com/OWNER/REPOSITORY/compare` for `facebook/react`, as [GitHub's guide to comparing releases](https://docs.github.com/en/repositories/releasing-projects-on-github/comparing-releases) describes. Pick two tags and you see the commits and changed files between them.
- **Project comparison:** overlay `facebook/react` and `angular/angular` on one chart at [gitdebt.com/compare](https://gitdebt.com/compare). The page puts star histories on one set of axes. No code is diffed.

The compare URL and the local diff serve the first job. Star history and the health readings serve the second.

## How do you compare branches, tags and forks with a GitHub compare URL?

GitHub's compare page diffs two refs of one repository, and you reach it by editing the URL. The [GitHub Docs page on comparing commits](https://docs.github.com/en/pull-requests/how-tos/commit-changes/comparing-commits) says, "To compare different versions of your repository, append /compare to your repository's path." It also states that "the compare page uses base as the starting point and compare as the endpoint."

1. **Open the compare page.** Go to the URL below, then pick refs from the base and compare dropdowns.

   ```text
   https://github.com/OWNER/REPOSITORY/compare
   ```

 Success is a list of commits and changed files. The Docs steps end with "Review the commits and changed files in the comparison."

2. **Type two branches into the path.** Use this when you already know both names.

   ```text
   https://github.com/OWNER/REPOSITORY/compare/BASE...COMPARE
   ```

 Put the older or reference branch first. Swapping the two reverses the direction of the diff. Use the repository's real default branch for BASE, which isn't always `main`.

3. **Add the `tags/` prefix for tags.** Use this to see what changed between two releases.

   ```text
   https://github.com/OWNER/REPOSITORY/compare/tags/v1.0.0...tags/v2.0.0
   ```

4. **Write `owner:branch` for forks.** Use this to see what a fork has that the original lacks.

   ```text
   https://github.com/OWNER/REPOSITORY/compare/main...FORK-OWNER:feature-x
   ```

 Prefix both sides if neither ref lives in the repository in the URL.

5. **Paste two commit SHAs for an exact range.** Use this when you have hashes from a log or a CI run.

   ```text
   https://github.com/OWNER/REPOSITORY/compare/OLDER_SHA...NEWER_SHA
   ```

6. **Use `~N` to step back N commits.** Use this for a quick view of what landed recently.

   ```text
   https://github.com/OWNER/REPOSITORY/compare/main~5...main
   ```

 This shows the last five commits on `main`.

## How do you compare two separate GitHub repos locally with git?

You can compare two separate GitHub repos locally by adding one as a remote inside a clone of the other, fetching it, and running `git diff` against its remote branch.

1. **Open a clone of the first repository.** Run `cd` into it and check out its default branch, so `HEAD` points at the code you want on one side of the diff.
2. **Add the other repository as a remote.** Use its HTTPS clone URL and any short name, such as `other`.
3. **Fetch the remote.** This downloads its branches without touching your working files. Expect a few seconds for a small repository and longer for a large one.
4. **Find the remote's default branch.** Run `git remote show other` and read the `HEAD branch` line. Don't assume `master`: newer repositories often use `main`, and some use another name. Substitute each repository's actual default branch wherever you see `DEFAULT_BRANCH`.
5. **Diff against the remote branch.** Start with `--stat` for a file-level summary, then drop it for the full patch.
6. **Remove the remote.** This deletes the remote's name and its tracking branches from your clone.

```bash
cd path/to/your/clone
git remote add other https://github.com/OWNER/REPO.git
git fetch other
git remote show other
git diff --stat HEAD other/DEFAULT_BRANCH
git diff HEAD other/DEFAULT_BRANCH
git remote remove other
```

Success looks like a list of changed files with insertion and deletion counts from the `--stat` run. Because `git diff` compares two commits, the two repositories don't need a shared history.

If the fetch fails, check the URL and confirm that the repository is public.

This method compares the tips of two branches. Which project is healthier or more popular is a separate question, and it needs different evidence.

## How do you compare GitHub repos to choose one?

To compare GitHub repos and choose one, read the crowd signal and the maintainer signals separately, then set them side by side. Stars record what people did. Commit history records what the maintainers did.

1. **Read stars as attention.** [GitHub's documentation on stars](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) says, "You can star repositories and topics to keep track of projects you find interesting". A star shows interest at one moment. It doesn't show whether anyone still maintains the code, so a project can look popular and be quiet.
2. **Check star velocity for recency.** The [star velocity leaderboard](https://gitdebt.com/leaderboard) shows stars gained over 1, 7 and 30 days. That shows recent attention only and says nothing about code quality.
3. **Read maintenance.** Per [gitdebt's methodology page](https://gitdebt.com/about), the commit trend shows monthly commit volume across the analyzed history. The reading shows the direction of cadence, not quality. It doesn't show that a quiet, finished library is neglected.
4. **Read ownership.** Ownership concentration shows how few contributors it takes to write half the commits. It shows where knowledge has pooled; it is not a judgement about any person, and a small team can run a project well.
5. **Read repair load.** Fix-labelled changes count how often each file appears in commits whose message matches a fix heuristic. It rests on commit messages and describes history without proving a bug. Mature projects often fix more because more people use them.
6. **Read debt markers.** The TODO and FIXME movement chart tracks how the comment notes that flag unfinished work move over time. Rising notes can mean work is piling up, and falling notes can mean it's being cleared. The reading tracks comments, not total debt, and some teams keep that list in an issue tracker instead.

GitHub's Insights tab (contributors, commit activity, pulse) is useful inside one repository. You would open it once per project and compare the results by hand.

When the signals disagree, don't average them. A steep star curve with falling maintenance means you should read the recent commits and decide whether the project is finished or drifting.

A flat curve with steady commits and several contributors often describes a small project that's actively maintained. Treat every reading as directional, never as a verdict.

## How do you compare star history and repository health side by side?

You compare star history and repository health side by side by overlaying the star curves on [gitdebt's compare page](https://gitdebt.com/compare), then reading each repository's commit-based readings beside its curve. Star history needs no sign-in.

1. **Overlay the repositories.** Enter up to eight `owner/repo` slugs, or start from the [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks) or the [AI coding tools comparison](https://gitdebt.com/compare/ai-coding-tools). Every chart names its star history source and coverage date, so check both before you trust a curve.
2. **Switch the axis to days since first star.** A calendar axis favors old repositories. Equal-start growth lines every project up at its first star. Its limit: early curves can reflect a launch, a post or a release, and say nothing about long-term use.
3. **Compare the first 90 days.** Use the log scale if one curve flattens the rest. Note which repositories climb steadily and which jump once.
4. **Open each report and read the four health readings.** [All four come from commits](https://gitdebt.com/about), not stars, so they can disagree with the curve: maintenance, ownership, repair load and debt markers.
5. **Pair each curve with its readings.** A steep curve with falling maintenance and a flat curve with steady maintenance describe different projects.
6. **Share the result.** Every chart is a plain image URL, such as `https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg`, with light and dark variants. It renders in a README without JavaScript, and the [badges page](https://gitdebt.com/badges) has the snippet format.

For example, overlay `facebook/react` and `angular/angular`, switch to days since first star, and compare each repository's first 90 days. Then open the [`angular/angular` report](https://gitdebt.com/angular/angular) and read its four readings beside the curve. Repeat for `facebook/react`.

Success looks like a short note per repository: how the first 90 days differed, and whether the four readings agree with the curve or contradict it. Overlay your own shortlist on the compare page and run these six steps.

## Do fake stars change how you compare GitHub projects?

Fake or purchased stars are a known industry topic, and they are a reason to read stars next to commit history instead of alone. Researchers have published an [arXiv paper](https://arxiv.org/abs/2412.13459) titled "4.5 Million (Suspected) Fake Stars in GitHub: A Growing Spiral of Popularity Contests, Scams, and Malware".

Each signal has a limit. A star count shows how many accounts clicked, not who uses the code. Commit history shows what maintainers did, not whether the code is good.

Read together, they cover each other's gaps. A project that looks popular but has gone quiet shows up when the curve and the commits disagree.

As [gitdebt's methodology page](https://gitdebt.com/about) states, gitdebt does not flag stars as fake, real or suspicious, and it does not score accounts or profile stargazers. It stores only star dates and day counts, and it never states how many stars are "missing" from a history. Our [article on fake GitHub stars](https://gitdebt.com/blog/fake-github-stars) covers the topic in depth.

## Frequently asked questions

### Can I diff two different GitHub repositories?

You can, using a local clone. Add the other repository as a remote with git remote add, run git fetch for that remote, then run git diff between your branch and the remote's default branch. Remove the remote afterward with git remote remove.

### How do I compare two branches on GitHub?

Following [GitHub's comparing commits guide](https://docs.github.com/en/pull-requests/how-tos/commit-changes/comparing-commits), append /compare to the repository's path, then pick branches from the base and compare dropdown menus. Base is the starting point and compare is the endpoint. GitHub lists the commits and changed files between them.

### Is the repository with more stars the better choice?

Not necessarily. Stars record what the crowd did, not what maintainers are doing now. A popular project can be quiet, and a small one can be actively maintained, so read commit history beside the star count.

### How do I compare the star history of several repositories?

Overlay up to eight repositories at [gitdebt.com/compare](https://gitdebt.com/compare), then switch the axis to days since first star. Compare the first 90 days of each, so every project is read from an equal start.

### Does gitdebt detect fake stars?

No. As its [methodology page](https://gitdebt.com/about) states, gitdebt does not flag stars as fake, real or suspicious, and it does not score accounts. Fake stars are a known industry topic, which is one more reason to read stars beside commit history.

## Your next steps: from a shortlist to a decision

**Today:** Decide which question you have. For a code question, append `/compare` to the repository path, which [GitHub's documentation](https://docs.github.com/en/pull-requests/how-tos/commit-changes/comparing-commits) describes as the way to compare versions. For two separate repositories, add a remote, fetch it, and run `git diff` against its default branch.

**This week:** For a project decision, overlay your shortlist (up to eight) on [gitdebt's compare page](https://gitdebt.com/compare). Switch the axis to days since first star and compare each repository's first 90 days. Then read maintenance, ownership, repair load and debt markers in each report.

**At your next dependency review:** Run the same check again and note what moved. Use any reading that shifted to decide which commits or files to read next.

To start, [analyze a repository at gitdebt.com/report](https://gitdebt.com/report). [Every public repository report works without an account](https://gitdebt.com/about), so you can try it on one dependency first.

## Sources

[Comparing commits - GitHub Docs](https://docs.github.com/en/pull-requests/how-tos/commit-changes/comparing-commits) · [Compare GitHub star history](https://gitdebt.com/compare) · [Saving repositories with stars - GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/) · [README badges, star-history charts and embeds](https://gitdebt.com/badges) · [4.5 Million (Suspected) Fake Stars in GitHub: A Growing…](https://arxiv.org/abs/2412.13459) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about)
