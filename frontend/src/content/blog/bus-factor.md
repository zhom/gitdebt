---
description: "Bus factor from git history: count fewest authors whose commits reach 50% of all commits using git shortlog -sn --no-merges, then check its limits. Start now."
faq:
  - answer: To calculate bus factor, count commits per author, sort descending, and add authors from the top until they reach half of the total. The number of authors added is the bus factor under the simple 50% method. It is a proxy for knowledge concentration, not a measurement of it, so check it against ownership of the files that matter.
    question: How do you calculate bus factor?
  - answer: "No verified source sets a universal threshold for a good bus factor. Judge the number against team size and how critical the system is: a 1 on a payments service is a bigger problem than a 1 on a side project."
    question: What is a good bus factor?
  - answer: Yes, bus factor and truck factor are the same measure. Wikipedia lists lottery factor, truck factor and circus factor as other names for the bus factor, so the two terms mean exactly the same thing and can be used interchangeably.
    question: Is bus factor the same as truck factor?
  - answer: No, a higher bus factor does not mean more risk. A higher bus factor means lower risk. The risk goes down as the number goes up. Knowledge is shared among more people, so the project survives more departures before it stalls.
    question: Does a higher bus factor mean more risk?
  - answer: Yes, you can measure it, but not from commits. List each task (support queue, deployments, client accounts), write down who can do it, and count the fewest people who cover every row. The idea applies to businesses too, not only repositories.
    question: Can you measure bus factor for a team that does not write code?
publishDate: "2026-10-22"
tags:
  - bus-factor
  - git-history
  - code-ownership
  - risk-assessment
  - team-health
title: "Bus factor: how to measure it from git history and its limits"
---

**The [bus factor](https://contributoriq.com/blog/what-is-bus-factor-how-to-calculate-measure) is the minimum number of people who would have to leave before a project stalls. A higher number means [lower risk](https://lawsofsoftwareengineering.com/laws/bus-factor/). From git history, count the fewest authors who together wrote 50% of commits; Degree of Authorship refines this per file. Treat it as a proxy: commits show who wrote code, not who still understands it.**

You want a single **bus factor** number before you depend on a public repository or sign off on your own team's setup. Run `git shortlog -sn --no-merges` in a clone and count how few names reach half the total.

With that command, you can compute the number from git history, read it against team size and recognize when to distrust it.

> **Key Takeaways**
> - Commit share overstates concentration after bulk edits and understates it when knowledge lives in people's heads.
> - Report a number per module, not one per repository. It shows where to act.
> - Treat published risk thresholds as heuristics, not findings, unless cited research says otherwise.
> - Count availability, not only departure: leave or illness lowers the number as surely as quitting.

## What is the bus factor, and what does a bus factor of N mean?

The bus factor is the minimum number of people who would have to become unavailable before a project stalls because the remaining team lacks the knowledge to continue, in [ContributorIQ's definition](https://contributoriq.com/blog/what-is-bus-factor-how-to-calculate-measure). A bus factor of 3 means losing any 2 people leaves the project running, and losing the right 3 stops it.

![Image generated](https://api.birchseek.com/m/Zh1fIXLSSpV_7AfOFIO3TjpfGeV0Q4f6.webp)

[Wikipedia's bus factor entry](https://en.wikipedia.org/wiki/Bus_factor) says the name is derived from the phrase "in case they get hit by a bus," and lists truck factor and lottery factor as other names.

Read the number as the size of the smallest group whose loss stalls the project:

- **1:** losing one specific person stalls it.
- **2:** losing any one person is survivable, but losing the right two is not.
- **3:** losing any two people is survivable, but losing the right three is not.

Some pages phrase this loosely. [Laws of Software Engineering](https://lawsofsoftwareengineering.com/laws/bus-factor/) says a bus factor of 5 "means the project could lose any one of five specific people before stopping work." Read precisely, any 4 people can go, and the right 5 stop it.

The number is not a headcount of key people. A team can name five key people and still have a bus factor of 1 if one of them alone knows how to deploy.

Departure risk is permanent loss. Availability risk is temporary: vacation, parental leave, illness or burnout. Both lower the number while someone is out.

A bus factor of 0 means the sole expert has already left.

## How do you measure bus factor from git history?

To measure bus factor from git history, count the fewest authors whose commits add up to half of all commits. Use a full clone, because a shallow clone made with `--depth 1` [keeps only the latest commits](https://git-scm.com/docs/git-clone).

1. **Export commits per author.** Run `git shortlog -sn --no-merges`. According to the [git-shortlog documentation](https://git-scm.com/docs/git-shortlog), the `-n` flag sorts authors by commit count. The `-s` flag prints counts only, and `--no-merges` drops merge commits. Add a `.mailmap` file first, or one person with two emails counts as two authors and the number comes out too high.
2. **Total the commits and halve it.** Add every count, then divide by 2.
3. **Add authors from the top.** Keep a running total, starting with the largest author, and stop when it reaches half. The number of authors you added is the bus factor.
4. **Check what you counted.** Bots and bulk reformatting commits inflate one author's share. Exclude them and rerun.

The same steps as one command:

```bash
git shortlog -sn --no-merges | awk '{c[NR]=$1; t+=$1} END{for(i=1;i<=NR;i++){s+=c[i]; if(s>=t/2){print i; exit}}}'
```

**Worked example with made-up numbers (not a real repository).** Ten authors made 1,000 commits. Half is 500.

| Author | Commits | Running share |
|---|---|---|
| A | 420 | 42% |
| B | 180 | 60% |
| C | 110 | 71% |
| D | 90 | 80% |

Author A alone is short of 500. A plus B reaches 600, so the bus factor is 2. The other six authors (E to J, 200 commits together) never enter the count.

Commit counts say who typed, not who understands. For a finer reading, score authorship per file instead of per commit.

Truck-factor research such as [Avelino and colleagues' algorithm](https://arxiv.org/abs/1604.06766) first works out who authored each file from the commit history. It then counts the people who can't be removed before too many files lose all their authors. Take the exact formula and cutoff from the original paper, not from a blog summary.

You can run the count four ways:

- **Spreadsheet:** lowest effort, commit level only. Aliases are merged by hand, and anyone can check the work.
- **Shell or Python script:** low effort once written, and it can reach file level. Aliases are handled through `.mailmap`, and the script is easy to rerun.
- **Open-source tools:** moderate setup, usually file level. Settings differ per tool, so record the version you ran.
- **Commercial tools:** least effort, but the method may be hidden. Ask what counts as an author before comparing numbers.

For a quick reading of any public repository, the [gitdebt report](https://gitdebt.com/report) shows how many contributors write half the commits, with no sign-in. It is the 50% method on commits, not a knowledge audit, and the [gitdebt methodology page](https://gitdebt.com/about) lists its limits. gitdebt never analyzes private repositories.

## When does a git-history count mislead you?

A git-history count misleads you when commit authorship stops matching who understands the code. Run the check beside each distortion before you trust the number.

1. **Bulk or automated commits.** Bots, generated code and dependency bumps credit one account with hundreds of commits nobody reasoned about. *Check:* drop bot accounts from the `git shortlog -sn --no-merges` output and recount.

2. **Formatting changes and mass renames.** Whoever ran the formatter appears to own every line it touched. *Check:* run `git blame -w -M` on a few central files, which ignores whitespace changes and follows moved lines.

3. **Copied, vendored or moved files.** The original author vanishes behind the commit that pasted the code in. *Check:* exclude vendored directories, then spot-check who owns five files you know well.

4. **Split identities and squash merges.** One person with three emails looks like three people, and a squash merge can credit whoever clicked the button. *Check:* add a `.mailmap` file that maps each person's addresses to one identity, then rerun the count.

5. **Knowledge that never reaches version control.** Tickets, chat threads, ops runbooks and people's heads leave no commits. *Check:* ask your top three authors what only they know, and list the answers next to the number.

6. **Authors who already left.** A departed engineer still counts toward the 50% unless you filter by recent activity. *Check:* rerun with `--since="12 months ago"`. If the number drops, the lower one is closer to your real exposure.

If a check moves the result by a full person, report the adjusted number and name the filters you applied. If item 5 turns up undocumented knowledge, treat the git count as an upper bound.

## What does the research say about risky and healthy values?

Treat any fixed healthy value with suspicion. What you can defend is the definition and a way of comparing.

The bus factor is a count, and a count only means something against what it could have been. Tables that map team size to a verdict (1 is critical, 5 or more is safe) are heuristics unless they cite a study.

Three adjustments make the number meaningful:

1. **Compare the number to team size.** The bus factor can't exceed the number of people who commit. A 3 on a team of 3 means every person holds part of the knowledge. A 3 on a team of 40 means knowledge has pooled in a few hands.
2. **Weigh it by how critical the system is.** A bus factor of 1 on an internal script is an annoyance. The same value on a payments service is a business risk.
3. **Weigh it by how long a replacement would take.** [Wikipedia](https://en.wikipedia.org/wiki/Bus_factor) contrasts bus factor with key person risk, noting that financial or managerial executives are "theoretically replaceable at an insurable cost." Deep technical knowledge usually isn't, so a low value on code that takes months to learn is worse than the same value on code a new hire reads in a week.

If you cite a paper's typical values, take them from the paper itself. Check how it defines authorship, which repositories it sampled and what year it ran, because those choices change the numbers more than team size does.

To get your own reading per repository, [run a gitdebt report](https://gitdebt.com/report) and compare the ownership reading with the contributor count. The ownership reading shows where knowledge has pooled in a codebase; it is not a judgement about any person.

## What do real key-person losses look like, from left-pad onward?

A real key-person loss happens when work that many people depend on stops because one person's knowledge, access or decisions are gone, and the cases differ in how that person is lost. Wikipedia notes that the bus factor is [similar to the much older idea of key person risk](https://en.wikipedia.org/wiki/Bus_factor), but applied to technical experts.

In the left-pad incident of March 2016, nobody left a team: a maintainer withdrew his own packages from the npm registry, which broke the builds of projects that depended on them.

The dates, the package list, the dependency chain and the policy changes npm made afterward are in npm's own post-mortem. Read those details there rather than in a summary.

Run `git shortlog -sn --no-merges` on a small package where one author holds nearly every commit, and the 50% method gives a bus factor of 1.

Before an incident like left-pad, that count would have shown concentration: one person could change or remove the code alone. It would not have shown intent, or the registry rules that decide whether a maintainer can remove a package.

Three situations get lumped together, and the number treats them the same:

- **Departure:** the expert leaves and takes undocumented knowledge along.
- **Withdrawal:** the expert is still around but pulls code or access.
- **Single-maintainer dependency:** nobody has left. One person has always done the work, and your risk comes from relying on them.

Only the first matches the textbook definition. The other two still show up as a low count, but the fix differs: documentation helps a departure, while a withdrawal calls for a pinned or mirrored copy.

To see where commits pool in a project you rely on, enter its `owner/repo` in [the gitdebt report](https://gitdebt.com/report) and read the ownership reading.

## How do you raise the bus factor, and in what order?

You raise the bus factor by getting a second person to review, change and understand the code that only one person touches, starting with the cheapest, most informative steps. The effort and time ratings below are practitioner judgment, not research findings, so adjust them to your team.

1. **Build the ownership map for the riskiest modules.** Effort: low, 1 to 2 days. Run `git shortlog -sn --no-merges -- path/to/module` for each critical directory and note the top author's share. *Verify:* every critical module has a named top author and a recorded share.

2. **Name a backup per critical area.** Effort: low, about 1 week. Write one person's name next to each module and get their agreement. *Verify:* no critical module on the map has a blank backup.

3. **Rotate code review.** Effort: low to medium, 2 to 4 weeks to become routine. Make a second person, starting with the backup, read every change in sole-owner areas. *Verify:* review records show a second name on those changes.

 Reviews don't alter commit authorship, so the commit count won't move yet.

4. **Pair on the sole-owner code.** Effort: medium to high, 1 to 3 months. The backup types while the owner talks through the code, then they swap. *Verify:* re-run the measure on the trailing 90 days (`git log --since="90 days ago"`) and check that a second author appears on the targeted files.

5. **Write runbooks and decision records.** Effort: medium, 1 to 2 months. Capture what the repository can't show: deploy steps, access, and why past choices were made. *Verify:* run a hands-on handover in which the backup completes a real task using only the documents.

 Documentation alone doesn't change commit-based numbers, so the handover is the only real test of this step.

6. **Fold the topics into onboarding.** Effort: medium, one hiring cycle. Add the mapped modules and runbooks to the new-hire checklist. *Verify:* the next hire finishes a starter task in a mapped area without the original owner.

Re-measure per module after each step, not once for the whole repository. The [gitdebt report page](https://gitdebt.com/report) shows an ownership reading for any public repository if you want a second view beside your own script.

## How do you map bus factor per module and outside software?

You map bus factor per module by replacing one repository-wide number with a grid of who can change and who can operate each part, then counting the fewest people who cover every row. Plan on about an hour for a first pass.

1. **Build the grid.** List modules, systems or skills as rows and people as columns. Mark each cell **C** (can change it unaided) or **O** (can run it in production). Ask people directly; a single past review doesn't make someone a C.
2. **Count each row.** The row's number is the minimum people whose absence leaves nobody marked. Flag every row at 1 or 0.
3. **Seed the git rows per directory.** Run the 50% count on each path instead of the whole repository:

```bash
git shortlog -sn --no-merges -- src/billing/
```

 Count the fewest authors whose commits reach half of that directory's total. Bulk edits and moved files distort small directories most, and git can't see who can operate anything. To read ownership concentration on a large project, open the [apache/airflow report](https://gitdebt.com/apache/airflow).
4. **Reuse the grid outside software.** Swap commits for who handles which accounts, runbooks, queues or on-call rotations. The definition holds: the minimum people whose absence stalls the work.

 [ActiveCollab](https://activecollab.com/blog/project-management/bus-factor) describes star performers as "People who, if they were to leave, would cause your entire business or project to fall apart." The grid goes further by naming every row where that is true.
5. **Recompute on a schedule.** Rerun the git counts monthly over a trailing window, such as 12 months, and update the grid by hand. Alert on any row that drops to 1. Mark planned leave as unavailable in that month's grid, because leave is availability risk, not departure risk.

Success looks like a short list of flagged rows, each with a named person assigned to train a second owner.

## Frequently asked questions

### How do you calculate bus factor?

Count commits per author, sort descending, and add authors from the top until they reach half of the total. The number of authors added is the bus factor under the simple 50% method. Check it against ownership of the files that matter, because it measures commit concentration, not knowledge.

### What is a good bus factor?

No universal threshold has been established. Judge the number against team size and how critical the system is: a 1 on a payments service is a bigger problem than a 1 on a side project.

### Is bus factor the same as truck factor?

They're the same measure. [Wikipedia](https://en.wikipedia.org/wiki/Bus_factor) lists lottery factor, truck factor and circus factor as other names for the bus factor.

### Does a higher bus factor mean more risk?

No. [Laws of Software Engineering](https://lawsofsoftwareengineering.com/laws/bus-factor/) says a higher bus factor means lower risk. Knowledge is shared among more people, so the project survives more departures before it stalls.

### Can you measure bus factor for a team that does not write code?

You can, but not from commits. List each task (support queue, deployments, client accounts), write down who can do it, and count the fewest people who cover every row. As [ActiveCollab notes](https://activecollab.com/blog/project-management/bus-factor), the idea applies to businesses too, not only repositories.

## What to do this week, this month and this quarter

- **Today:** run `git shortlog -sn --no-merges` on one critical repository and note how few authors reach half the commits.
- **This month:** build the per-module ownership map and name a backup for every module with a bus factor of 1.
- **This quarter:** recompute the **bus factor** on a trailing window, then confirm a second author is committing to the files you targeted.

Start with the repository you depend on. Look up a dependency or your own public repository on [gitdebt](https://gitdebt.com/) to read its ownership reading beside its commit trend. The [repository report](https://gitdebt.com/report) is free, and no account is needed.

Treat the result as a directional signal, not a verdict. Once your own numbers improve, [add the health charts to your README](https://gitdebt.com/badges).

## Sources

[What Is Bus Factor? Definition, Risks & Calculation](https://contributoriq.com/blog/what-is-bus-factor-how-to-calculate-measure) · [Bus Factor - Laws of Software Engineering](https://lawsofsoftwareengineering.com/laws/bus-factor/) · [Bus factor - Wikipedia](https://en.wikipedia.org/wiki/Bus_factor) · [git-shortlog Documentation](https://git-scm.com/docs/git-shortlog) · [Analyze a GitHub repository](https://gitdebt.com/about) · [Bus Factor: What Is It, How To Calculate It & Why Use It](https://activecollab.com/blog/project-management/bus-factor)
