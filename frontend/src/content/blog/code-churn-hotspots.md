---
description: Code churn hotspots are files that change often and are complex or widely shared. Learn to find them with git log --name-only or --numstat, filter noise.
faq:
  - answer: "Run git log --name-only --pretty=format: --since=\"6 months ago\", remove blank lines with grep -v '^$', then pipe the result through sort | uniq -c | sort -rn | head -20. The top entries are your most-changed files. Filter out lock files, generated code and bulk-format commits before trusting the list."
    question: How do I find the files that change most in git?
  - answer: "There's no validated universal threshold, so treat any published band as a vendor heuristic, not a standard. CodePulse notes that \"high churn is not automatically bad and low churn is not automatically good.\" Compare files against others in your own repository instead."
    question: What is a good code churn rate?
  - answer: "High churn isn't always bad. Rework, where recent code gets fixed again soon after it lands, is the warning sign. Deliberate refactoring and active feature work also raise counts, so read the diffs of the top files before judging them."
    question: Is high code churn always bad?
  - answer: Start with 3 months and compare it against 12 months. Both numbers are heuristics, not standards. Keep the files that rank high in both windows, since a short window shows current activity and a long one shows slow, steady churn.
    question: How long a time window should I use?
  - answer: "To exclude lock files and generated code, add pathspec excludes after a double dash. Use the command git log --name-only --pretty=format: --since=\"6 months ago\" -- . ':(exclude)package-lock.json' ':(exclude)*.min.js' ':(exclude)dist'. Swap in your own paths. The false-positive section above explains why each filter matters."
    question: How do I exclude lock files and generated code?
publishDate: "2026-10-26"
tags:
  - code-churn
  - git-log
  - hotspots
  - complexity
  - refactoring
title: "Code churn hotspots: find the files that change most in git"
---

**Code churn hotspots are files that change often and are also complex or widely shared. Find them by counting changes per file with `git log --name-only` or `--numstat` over a fixed time window, then pair each count with a complexity measure. Churn is a prompt to inspect, not proof of instability or defects.**

Code churn hotspots help when you face a large repository, have little time and can't read every file. They point to the files that keep getting revisited.

By the end, you can produce a ranked file list with plain git, filter out noise like lock files and bulk reformatting, combine the counts with complexity, and decide what to do with each file. Everything runs from a terminal inside the clone.

> **Key Takeaways**
> - Churn varies by tool: [code-hotspots counts change frequency](https://github.com/pdewouters/code-hotspots), [CodePulse counts code rewritten or deleted](https://codepulsehq.com/guides/code-churn-guide), so one file can rank differently.
> - Lock files, generated code and bulk-format commits distort a ranking more than your choice of tool does.
> - Churn becomes a priority only beside complexity, because high churn alone isn't automatically bad.
> - Time windows and churn bands are vendor heuristics, not validated standards.

## What is a code churn hotspot, and does it predict bugs?

A code churn hotspot is a file that changes often over a chosen time window and is also hard to change safely, usually because it is complex or widely shared. Churn alone only shows frequent revisits.

![Image generated](https://api.birchseek.com/m/fNxuDJ7tDpcU3cR852-MPFAACdp9Ig7X.webp)

Pairing churn with a complexity measure such as lines of code or cyclomatic complexity shows where changes are likely to cost the most.

Keep the measurement and the inference apart. What is measured is a count taken from git history, such as commits touching a file in the last 90 days. What is inferred is that the file is risky or unstable.

The first is a fact about the repository. The second is a judgment you add.

Tools define the count differently. The [`code-hotspots` README](https://github.com/pdewouters/code-hotspots) describes "git churn (change frequency)". [CodePulse's churn guide](https://codepulsehq.com/guides/code-churn-guide) defines churn as "the rate at which code is rewritten or deleted".

Those definitions rank files differently. A file edited in 50 small commits ranks high on frequency. A file with one commit that deletes half its lines can rank high on deletions and low on frequency.

### Does it predict bugs?

CodePulse's churn guide says a "2005 Microsoft Research study found files with high relative churn were up to 8x more likely to contain defects". The page gives no authors, title or link for that study, so treat the figure as a vendor claim, not a verified finding.

The same CodePulse page adds that "high churn is not automatically bad and low churn is not automatically good."

Even a well-sourced study describes a population of files. It is not a verdict on the file in front of you.

Treat a hotspot as a reason to read the file and its history, not as proof of a defect.

## How is code churn measured: commits, rewritten lines or deletions?

Code churn is measured in three main ways: commits touching a file, lines rewritten relative to file size, or code rewritten or deleted. Each method ranks the same file differently.

| | Commit count | Lines rewritten vs. file size | Rewritten or deleted code |
| --- | --- | --- | --- |
| Counts | Commits touching a file | Changed lines divided by file size | Code replaced or removed |
| Rewards | Frequent small edits | Heavy rewrites of small files | Replacement and removal |
| Hides | How big each change was | How many separate commits | Cleanup versus rework |
| Ranks high | Large file, many tiny edits | Small file rewritten once | Pruning-heavy history |
| Ranks low | Small file rewritten once | Large file, tiny edits | Additions-only growth |

The [`code-hotspots` README](https://github.com/pdewouters/code-hotspots) counts change frequency. gitdebt's [file change frequency](https://gitdebt.com/about) is the same flavor: a count of analyzed commits that touched each file.

It identifies frequently revisited files without claiming those changes were unstable or defective.

[Churnmap's Marketplace page](https://marketplace.visualstudio.com/items?itemName=Indrasol.churnmap) says it highlights "the files that change most and hurt most." That page doesn't state a formula, so check whether it ranks by commits or by lines before comparing its scores with another tool's.

CodePulse's rewritten-or-deleted measure is a vendor definition too. Any percentage band a vendor attaches to churn is a heuristic unless it cites data.

The same file can swing between tools. A 5,000-line router with one-line edits in dozens of commits tops a commit-count ranking.

A 40-line module rewritten in a single commit barely registers there, yet nearly all of it changed relative to its size.

**Best for:**

- **Commit count:** a fast first pass with plain git, paired with a complexity score.
- **Lines rewritten vs. file size:** spotting small files that get replaced wholesale.
- **Rewritten or deleted code:** a rework signal, once you know how the vendor computes it.

## How do you find code churn hotspots with plain git?

You find code churn hotspots with plain git by counting how often each file changes in a fixed window, then pairing that count with a size or complexity measure. The whole run takes a few minutes on a laptop.

1. **Pick a window.** Windows of 30, 90 and 365 days, and cutoffs like "top 5% of files", are heuristics from tool vendors, not validated standards. Choose one, write it down, and keep it fixed when you compare runs.

 For vitejs/vite, use 2025-01-01 to 2025-04-01 (90 days). Clone it first:

   ```bash
   git clone https://github.com/vitejs/vite.git && cd vite
   ```

2. **Count commits per file.** Add `--no-merges` so merge commits don't double-count. The `grep .` drops the blank lines that `--pretty=format:` leaves behind.

   ```bash
   git log --no-merges --since="2025-01-01" --until="2025-04-01" \
     --name-only --pretty=format: | grep . | sort | uniq -c | sort -rn | head -20
   ```

 Don't add `--follow` here. It works on one file only, so use it later to trace a single suspect through renames. Add `--use-mailmap` only if you count authors, since it merges duplicate identities.

3. **Add changed lines.** `--numstat` prints added and deleted lines per file. The awk script totals them and skips binary files, which show `-`.

   ```bash
   git log --no-merges --since="2025-01-01" --until="2025-04-01" \
     --numstat --pretty=format: | awk 'NF==3 && $1!="-" {c[$3]++; a[$3]+=$1; d[$3]+=$2} \
     END {for (f in c) print c[f], a[f], d[f], f}' | sort -rn | head -20
   ```

4. **Join with a complexity proxy.** Run `wc -l` on each top file as a rough size measure. A real complexity tool such as tokei or a cyclomatic complexity checker gives a better signal. This loop skips files deleted since the window.

   ```bash
   git log --no-merges --since="2025-01-01" --until="2025-04-01" --name-only --pretty=format: \
     | grep . | sort | uniq -c | sort -rn | head -20 \
     | while read -r n f; do [ -f "$f" ] && echo "$n $(wc -l < "$f") $f"; done
   ```

 Success looks like this. The values are placeholders, not live vite numbers:

   ```text
   <commits> <lines> <path>
   <commits> <lines> <path>
   ```

 Files with a high commit count and a high line count are your candidates. Check each for lock files, generated code and bulk-format commits before you trust the ranking.

The [git-log documentation](https://git-scm.com/docs/git-log) defines `--numstat` as showing the "number of added and deleted lines in decimal notation". It says `--no-merges` does "not print commits with more than one parent", and that `--follow` "works only for a single file".

For a quick commit-count view without cloning, open a finished report such as [the apache/airflow report](https://gitdebt.com/apache/airflow). Its file change frequency chart shows how many analyzed commits touched each file.

It's computed from a shallow clone, so it only covers the history that clone contains.

## Which changes create false code churn hotspots?

False code churn hotspots are files whose change counts rise for reasons that say nothing about how hard the code is to change. Five kinds of change cause most of them.

1. **Lock files.** Package managers rewrite `package-lock.json` or `yarn.lock` whenever dependencies move. A lock file can top a commit-count ranking without anyone having designed or reasoned about it.
2. **Generated and minified code.** Build output changes every time its source changes, and one edit can rewrite a whole minified line. Counting it measures your build, not your design.
3. **Bulk formatting and rename commits.** One commit that reformats 400 files adds one to 400 counts. [Git's pathspec documentation](https://git-scm.com/docs/gitglossary) notes that an exclude pathspec means "it will be run through all exclude pathspecs" and, if matched, the path is ignored.
4. **Bot commits.** Dependency updaters and release automation touch manifests on a schedule. Filter them by author, since they reflect a policy, not a decision about the code.
5. **Config and docs.** These change often by design. Keep them in a separate list instead of deleting them, because a hot config file can still matter.

To check whether a lock file leads your ranking, run a count with it excluded:

```bash
git log --since="12 months ago" --name-only --pretty=format: -- . ':(exclude)package-lock.json' \
  | sort | uniq -c | sort -rn | head -20
```

Compare this with the same command without the exclude. If `package-lock.json` drops out of first place, the old ranking was measuring dependency updates.

For formatting commits, list their hashes in a file and run `git blame --ignore-revs-file .git-blame-ignore-revs <file>`. [Git's blame documentation](https://git-scm.com/docs/git-blame) describes the option as "Ignore revisions listed in file, which must be in the same format as an fsck.skipList."

### Limits and misuse

Filtering doesn't make a count exact. Four limits remain:

- **Squash merging** [combines a branch's commits into one](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/about-merge-methods-on-github), so many edits to a file on that branch count once.
- **Commit style and team size** shift counts. Teams that commit in small steps, or have more people, produce higher counts for the same work.
- **Keyword-based fix detection** misses fixes with vague messages and catches messages that merely mention "fix". gitdebt's [methodology page](https://gitdebt.com/about) covers how its signals are computed. Fix-labelled changes describe history; they don't prove a bug.
- **Per-developer rankings** mislead because they reward commit habits. Read churn at team level.

## How do you read churn against complexity?

Read churn against complexity by placing each file in one of four quadrants, then checking whether its recent changes were rework or refactoring.

The [code-hotspots README](https://github.com/pdewouters/code-hotspots) describes this layout: files are plotted with churn on the X-axis and complexity on the Y-axis, creating four quadrants. It labels the top-right corner "PRIORITY HOTSPOTS".

| Quadrant | What it usually means | Decision |
| --- | --- | --- |
| High churn, high complexity | Revisited often and hard to change | Act first: read the history, add tests, plan a refactor |
| High churn, low complexity | Often config or glue code | Check for noise (lock files, bulk edits) before acting |
| Low churn, high complexity | Stable legacy | Leave alone unless it blocks current work |
| Low churn, low complexity | Quiet and simple | Ignore |

The code-hotspots README gives similar actions for two of these corners: "Deprioritize" for stable legacy and "Ignore" for the low-low corner.

A file in the top-right quadrant can still be healthy or unhealthy. To tell rework from refactoring, open the file's commits with `git log --numstat -- path/to/file` and check three things:

- **Message.** Repeated "fix" or "revert" subjects point to rework. Subjects like "extract" or "simplify" point to refactoring.
- **Added versus deleted.** Refactors often delete about as much as they add. Many small patches to the same area suggest rework.
- **Tests.** A refactor usually keeps tests passing or adds them. Repeated fixes with no test changes suggest the same problem is coming back.

Thresholds are judgment calls. Pick a cutoff for "high" that gives you a list short enough to read, and expect to move it.

The plot ranks files for reading. It does not deliver a verdict on any of them.

## What do you do with a hotspot once you find it?

A code churn hotspot gets one of four outcomes: refactor it, add tests, split the file or its ownership, or leave it alone. Pick the outcome with a rule, not a hunch, starting from the file's quadrant. Budget about 10 minutes per file.

1. **Refactor** when the file is high churn and high complexity, and the same logic keeps changing. Run `git log -L :functionName:path/to/file.js` to see whether one function is edited again and again. Skip files whose churn came from a bulk rename or format commit.
2. **Add tests** when churn is high and nothing guards the file. Run `git log --since="6 months ago" --name-only --format= -- test/ | sort | uniq -c | sort -rn` and check that the file has a matching test that changes with it. Write tests that pin current behavior before you refactor.
3. **Split the file or its ownership** when many authors touch it for unrelated reasons. Count them with `git log --since="6 months ago" --use-mailmap --format=%aN -- path/to/file.js | sort -u | wc -l`. Then read the commit subjects. If they mention billing, logging and parsing, those are three modules.
4. **Leave it alone** when the file is top-left: complex, but stable. Rewriting code that nobody touches adds risk and no benefit.
5. **Track the trend** after any change. Rerun the identical command, with the same window, the same exclusions and the same branch, before and after. If the file's count drops, it's cooling. If it rises, it's getting hotter.

A window under three months is noisy, so wait for a full window before you call the result. A refactor can raise churn for a few weeks, so compare the first window after the change with the one after it.

The author count is also an ownership reading. Run `git shortlog -sn --use-mailmap --since="6 months ago" HEAD -- path/to/file.js` to see how commits are spread.

Ownership shows where knowledge has pooled in a codebase; it is not a judgement about any person. [CodePulse's guide](https://codepulsehq.com/guides/code-churn-guide) describes the same team-level framing, previewing knowledge concentration "without turning activity into individual scores".

For a public repository, the [ownership and repair-load readings on gitdebt](https://gitdebt.com/) show the pattern without any commands.

## Frequently asked questions

### How do I find the files that change most in git?

Run git log --name-only --pretty=format: --since="6 months ago", remove blank lines with grep -v '^$', then pipe the result through sort | uniq -c | sort -rn | head -20. The top entries are your most-changed files. Filter out lock files, generated code and bulk-format commits before trusting the list.

### What is a good code churn rate?

There's no validated universal threshold, so treat any published band as a vendor heuristic, not a standard. Compare files against others in your own repository instead.

### Is high code churn always bad?

High churn isn't always bad. Rework, where recent code gets fixed again soon after it lands, is the warning sign. Deliberate refactoring and active feature work also raise counts, so read the diffs of the top files before judging them.

### How long a time window should I use?

Start with 3 months and compare it against 12 months. Both numbers are heuristics, not standards. Keep the files that rank high in both windows, since a short window shows current activity and a long one shows slow, steady churn.

### How do I exclude lock files and generated code?

Add pathspec excludes after a double dash: git log --name-only --pretty=format: --since="6 months ago" -- . ':(exclude)package-lock.json' ':(exclude)*.min.js' ':(exclude)dist'. Swap in your own paths, so dependency updates and build output stop inflating the counts.

## Next steps: find your first hotspot this week

Finding your first code churn hotspot takes three small steps spread over a month, not a project.

- **Today:** Run the commit-count command on one repository with a 90-day window. Write down the top 10 files.
- **This week:** Exclude lock files, generated code and formatting commits. Add a complexity column, then place each file in a quadrant.
- **This month:** Pick one hotspot and make one decision: refactor, add tests, split it, or leave it alone. Then rerun the same command with the same window and the same exclusions.

That before-and-after run is your success check. A falling count suggests the file is cooling, but it doesn't prove the code is healthier. Treat every reading as a directional signal, not a verdict.

Want a commit-based view of a public repository without cloning it? Open a [gitdebt report](https://gitdebt.com/report) to see file change frequency and fix-labelled changes. Every public repository report [works without an account](https://gitdebt.com/about), so you can try it on one repo first.

## Sources

[GitHub - pdewouters/code-hotspots](https://github.com/pdewouters/code-hotspots) · [High Code Churn Isn't Bad. Unless You See This Pattern](https://codepulsehq.com/guides/code-churn-guide) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [Churnmap: Code Hotspots City - Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=Indrasol.churnmap) · [git-log Documentation](https://git-scm.com/docs/git-log) · [gitdebt apache/airflow report](https://gitdebt.com/apache/airflow) · [gitglossary Documentation - Git](https://git-scm.com/docs/gitglossary) · [git-blame Documentation - Git](https://git-scm.com/docs/git-blame)
