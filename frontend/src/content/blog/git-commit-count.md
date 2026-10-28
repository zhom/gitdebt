---
description: How do you get a git commit count? Run git rev-list --count HEAD, then split it by author and 90 days. Learn what each figure shows and what it misses.
faq:
  - answer: "Run git rev-list --count HEAD inside the repository. It prints the number of commits reachable from the current branch's tip, merge commits included. Use git rev-list --count --all to count every ref instead. A shallow clone only reports the history it downloaded."
    question: How do I get the git commit count?
  - answer: "Run git rev-list --count --since=\"90 days ago\" HEAD to count commits in the last 90 days. For the 90 days before that, add --since=\"180 days ago\" --until=\"90 days ago\", then compare the two resulting numbers. Merge commits are included unless you add --no-merges to exclude them."
    question: How do I count commits in the last 90 days?
  - answer: "A clone made with git clone --depth downloads only the latest commits, and rev-list counts only what's on disk. Because the full history isn't present, the reported number is lower. Run git fetch --unshallow to download the full history, then count again."
    question: Why is the commit count lower in a shallow clone?
  - answer: "You can use GitHub's repository graphs or a report from gitdebt's analyzer without cloning the repository. The report URL is https://gitdebt.com/{owner}/{repo}, for example vitejs/vite. Both show activity over time, not the exact local commit count. This lets you inspect public activity without downloading the full history."
    question: "Can I see a public repository's commit activity without cloning it?"
  - answer: A higher commit count does not mean a healthier repository. A count says nothing about commit size, quality or risk, and squashing or splitting work changes it. Cadence, the trailing 90 days against the 90 before, says more about whether a project is alive than a lifetime total.
    question: Does a higher commit count mean a healthier repository?
publishDate: "2026-10-28"
tags:
  - git
  - commit count
  - git rev-list
  - git shortlog
  - repository health
  - open source
title: "Git commit count: totals, per-author and 90-day cadence"
---

**Run `git rev-list --count HEAD` for the git commit count of your current branch: it counts every commit reachable from HEAD, and `git rev-list --count --all` widens that to every ref. A local count only sees the history you cloned, so a shallow clone gives a truncated number.**

You're deciding whether to trust a repository: a dependency, a codebase you inherited, a project you might join. A search for the git commit count returns one number with nothing attached to say what it means.

The phrase usually means one of three figures: a lifetime total, a per-author split or a windowed cadence. After reading, you can produce each one from a clone, read commit activity for a public repository through a [gitdebt report](https://gitdebt.com/about) without cloning, and say what each figure doesn't show.

> **Key Takeaways**
> - `--no-merges` drops merge commits. It doesn't count "unique" commits.
> - Cadence (trailing 90 days against the 90 before) says more about whether a repository is alive than a lifetime total.
> - Summing sorted `git shortlog -sn` counts until half the commits are covered shows where knowledge has pooled, not who to judge.
> - No count measures quality, effort or bug risk.

## How do you get the git commit count of a repository?

The git commit count of a repository is the number `git rev-list --count` prints, and it changes with the starting points you give it.

![Image generated](https://api.birchseek.com/m/0HHzu5ApRtgxdY8zlEN-37vZjEG6klVe.webp)

```bash
git rev-list --count HEAD
git rev-list --count --all
```

1. **Count the current branch.** Run `git rev-list --count HEAD`. The [git-rev-list documentation](https://git-scm.com/docs/git-rev-list) says `--count` prints "a number stating how many commits would have been listed, and suppress all other output." With `HEAD`, that means every commit reachable from the commit you have checked out, and nothing else.

2. **Count every ref.** Run `git rev-list --count --all`. The same documentation says `--all` makes git "Pretend as if all the refs in refs/, along with HEAD, are listed on the command line as <commit>." Other branches, tags and fetched remote-tracking refs all feed into this figure.

3. **Compare the two.** A feature branch with commits that aren't merged into your current branch makes the `--all` figure higher. Neither number is "the" count: one measures how long this line of history is, the other how much history is stored here.

4. **Check for a shallow clone.** The [git-clone documentation](https://git-scm.com/docs/git-clone) says `--depth` will "Create a shallow clone with a history truncated to the specified number of commits." Both commands count only what's present, so a shallow clone under-reports without any warning.

5. **Unshallow if needed.** Run `git rev-parse --is-shallow-repository`; the [git-rev-parse documentation](https://git-scm.com/docs/git-rev-parse) says it prints "true" for a shallow repository and "false" otherwise. If it prints `true`, run `git fetch --unshallow` and count again.

Success is a single integer on one line. If you see a list of hashes, you left out `--count`.

`git log --oneline | wc -l` is an alternative. It prints one line per commit, so for the same range it returns the same number.

The Pro Git chapter on [viewing the commit history](https://git-scm.com/book/en/v2/Git-Basics-Viewing-the-Commit-History) is the reference for `git log` and has a "Limiting Log Output" section for narrowing that range.

## Do merge commits count, and how do you exclude them?

Merge commits count by default: `git rev-list --count --no-merges HEAD` leaves them out, and `--merges` counts only them. A merge commit is a commit with more than one parent.

```bash
git rev-list --count --no-merges HEAD
git rev-list --count --merges HEAD
```

The [git-rev-list documentation](https://git-scm.com/docs/git-rev-list) says `--merges` will "Print only merge commits. This is exactly the same as --min-parents=2." It says `--no-merges` will "Do not print commits with more than one parent. This is exactly the same as --max-parents=1."

The two results add up to the plain `git rev-list --count HEAD` figure. If they don't, the commands ran against different starting points.

### What the flag does not tell you

Excluding merges changes the number without making it "unique". It says nothing about duplicate, rebased or cherry-picked changes.

A cherry-picked fix that lands on two branches still counts twice. So does a rebased change that was committed again.

The merge count also reflects workflow. A team that merges every pull request with a merge commit adds one merge per PR. A team that squashes adds none, so the same amount of work gives a different merge count.

Read the figure as activity plus habit.

### Per-author counts

`git shortlog -sn --no-merges HEAD` applies the same filter per author. Check that the author counts add up to your `--no-merges` total.

## How do you get a git commit count for the last 90 days?

A git commit count for the last 90 days is the number of commits dated inside a window that ends now, and `git rev-list --count` with `--since` prints it.

1. **Count the trailing 90 days.** Run this from the branch you care about:

```bash
git rev-list --count --since="90 days ago" HEAD
```

2. **Count the 90 days before that.** Add `--until` to close the window:

```bash
git rev-list --count --since="180 days ago" --until="90 days ago" HEAD
```

The [git-rev-list documentation](https://git-scm.com/docs/git-rev-list) describes `--since` as showing commits more recent than a date and `--until` as showing commits older than a date. The [Pro Git book](https://git-scm.com/book/en/v2/Git-Basics-Viewing-the-Commit-History) says relative dates such as "2 years 1 day 3 minutes ago" are accepted.

If your Git version rejects a relative form, use `--since="<start-date>" --until="<end-date>"` with real dates. Fixed dates go stale, so recompute them each time.

3. **Read the two numbers together.** A recent count above the earlier one means rising. Roughly equal means steady. Below means falling. Run both commands on the same ref, and confirm the clone isn't shallow, because a truncated history can make the older window look empty.

4. **Treat the window as approximate.** A rebase creates new commits, and the [git-rebase documentation](https://git-scm.com/docs/git-rebase) says that by default it uses the current time as the committer date. A window can therefore show when work was recorded, not when it was written.

A lifetime total only grows, so it can't show which way a repository is heading. The two windows can.

Each gitdebt report draws a [commit trend](https://gitdebt.com/about), monthly commit volume, so you can read direction without cloning anything.

That reading is a directional signal, not a verdict. A quiet stretch can mean a finished project as well as an abandoned one.

## How do you count git commits per author, and what does the split show?

`git shortlog -sn HEAD` counts git commits per author: it prints one row per author with a commit count, largest first.

```bash
git shortlog -sn HEAD
git shortlog -sn --all
```

The [git-shortlog documentation](https://git-scm.com/docs/git-shortlog) says `-s` will "Suppress commit description and provide a commit count summary only", and `-n` will "Sort output according to the number of commits per author instead of author alphabetic order." The second command widens the count to every ref.

Name `HEAD` in scripts. With no revision and no terminal attached, the same documentation says shortlog reads its log from standard input instead.

Three things make the split misleading:

1. **One person, several rows.** A changed email or a second laptop creates a second identity. The [gitmailmap documentation](https://git-scm.com/docs/gitmailmap) describes a `.mailmap` that maps several names and emails to one canonical identity. Add a `.mailmap` file at the repository root, then rerun the command.
2. **Bots as authors.** Automation commits under its own name, so remove bot rows before you read the list.
3. **Branches left behind.** `--all` reads every ref, including branches from closed pull requests that never merged. They can add authors who never landed work on the main line, so use `HEAD` for ownership.

To turn the sorted list into an ownership reading:

1. **Run** `git shortlog -sn HEAD` and apply your `.mailmap`.
2. **Add** the counts from the top row down.
3. **Stop** when the running sum reaches half the total commits.
4. **Read** the number of rows you used. That is how many contributors write half the commits.

If the top three rows reach half the total, three contributors wrote half the history. A small number shows where knowledge has pooled in a codebase. It is not a judgement about any person.

gitdebt's [ownership concentration chart](https://gitdebt.com/about) shows the same idea for a repository you haven't cloned; the [apache/airflow report](https://gitdebt.com/apache/airflow) is an example.

## What can a git commit count not tell you?

A git commit count can't tell you how large, how good or how risky a project is; it tells you how many commits the history holds.

A count can't support a health score or a bug forecast.

Commit size depends on habits. One developer commits 20 times a day, and another squash-merges a week of work into a single commit. Generated and bot commits add rows that carry no human effort.

So the number measures none of size, quality, effort or bug risk. It can also change while the project stays the same:

- **Shallow clone:** it holds only recent history, so the count comes out lower.
- **Rewritten history:** a rebase or force-push replaces commits with different ones.
- **Mailmap gap:** one person's commits stay split across several names in a per-author count.

What a count can support is a direction, through the 90-day comparison above.

Keep stars and commits as two independent sources, and don't blend them. Stars are what the crowd did, and commits are what maintainers did. Folding them into one score hides the case that matters most: a steep star curve with a flat commit line, or the reverse.

Read stars on their own in the [star-history comparison tool](https://gitdebt.com/compare), then set them beside the commit reading and look at where they disagree. gitdebt's [methodology page](https://gitdebt.com/about) says it "does not flag stars as fake, real, or suspicious", so the star curve is a record of attention and nothing more.

## How do you check a repository's commit count without cloning it?

**Option 1: GitHub's graphs.** Open the repository on github.com, choose Insights, then read the Contributors and Commits graphs. GitHub draws them under its own counting rules, so a bar there may not match what `git rev-list --count HEAD` prints on a clone.

Read those graphs for shape and direction, not as a number to reconcile.

**Option 2: a gitdebt report.** The report for any public repository lives at `https://gitdebt.com/{owner}/{repo}`, and [every public repository report works without an account](https://gitdebt.com/about). It shows a commit trend (monthly commit volume) and a commit heatmap (the last 52 weeks).

1. **Open the report.** Type the slug into the [analyze any repository form](https://gitdebt.com/report) or edit the URL directly. If nobody has analyzed the repository yet, [the first request starts the analysis](https://gitdebt.com/about) and the report fills in as sections finish.
2. **Read the commit trend for direction.** Look at whether monthly volume is rising, flat or falling. Don't add the bars up and call the result a lifetime total.
3. **Check the heatmap for gaps.** Long blank runs in the last 52 weeks show where the repository went quiet. A single quiet week means little.
4. **Compare the commit trend with the star history.** Keep them as two separate readings, not one score.

**Example.** Open `https://gitdebt.com/vitejs/vite` to see its commit trend and commit heatmap without cloning anything. `facebook/react` works the same way.

If the star line climbs while recent monthly bars shrink, popularity and upkeep are disagreeing. That's a prompt to read the commit history, not a verdict.

**Limits.** [gitdebt's about page](https://gitdebt.com/about) says the health signals come from a shallow git clone, which means the report covers recent commit patterns rather than a lifetime total.

Private repositories are never analyzed. For an exact total, clone the repository and run the commands from the first section.

To try it now, [open a gitdebt report for a repository you're evaluating](https://gitdebt.com/report). It takes a minute once the analysis exists.

## Frequently asked questions

### How do I get the git commit count?

Run `git rev-list --count HEAD` inside the repository; it prints the number of commits reachable from the current branch's tip, merge commits included. Use `git rev-list --count --all` to count every ref instead.

### How do I count commits in the last 90 days?

Run `git rev-list --count --since="90 days ago" HEAD`. For the 90 days before that, add `--since="180 days ago" --until="90 days ago"`, then compare the two numbers. Merge commits are included unless you add `--no-merges`.

### Why is the commit count lower in a shallow clone?

A clone made with `git clone --depth` downloads only the latest commits, and `rev-list` counts only what's on disk. Run `git fetch --unshallow` to download the full history, then count again.

### Can I see a public repository's commit activity without cloning it?

You can, through GitHub's repository graphs or a report from [gitdebt's analyzer](https://gitdebt.com/report) at `https://gitdebt.com/{owner}/{repo}`, such as `vitejs/vite`. Both show activity over time, not the exact local count.

### Does a higher commit count mean a healthier repository?

No. A count says nothing about commit size, quality or risk, and squashing or splitting work changes it. Cadence, the trailing 90 days against the 90 before, says more about whether a project is alive than a lifetime total.

## What should you do with your commit count next?

The next step with a git commit count is to run each of the three counts on one repository you maintain or depend on, on a schedule.

- **Today:** run `git rev-list --count HEAD` for the lifetime total. Then run `git rev-list --count --since="90 days ago" HEAD` for the trailing window.
- **This week:** run `git shortlog -sn`, add counts from the top until you reach half the commits, and note a mailmap if one person appears under several identities.
- **This month:** repeat the 90-day comparison and read the direction. Set it beside the star history as a separate source.

For a repository you don't have cloned, look it up at [gitdebt](https://gitdebt.com/) to read its commit trend. [No account is needed](https://gitdebt.com/about); try it on `facebook/react` or `vitejs/vite` first.

Stars say a repository is popular. Commits say whether it is alive.

## Sources

[gitdebt](https://gitdebt.com/about) · [git-rev-list Documentation](https://git-scm.com/docs/git-rev-list) · [git-clone Documentation](https://git-scm.com/docs/git-clone) · [git-rev-parse Documentation](https://git-scm.com/docs/git-rev-parse) · [Git - Viewing the Commit History](https://git-scm.com/book/en/v2/Git-Basics-Viewing-the-Commit-History) · [git-rebase Documentation](https://git-scm.com/docs/git-rebase) · [git-shortlog documentation](https://git-scm.com/docs/git-shortlog) · [gitdebt](https://gitdebt.com/report)
