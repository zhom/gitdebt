---
description: How do you build a GitHub todo fixme tracker? Run git grep at fixed commit checkpoints and compare counts, or read a gitdebt report. Start with any repo.
faq:
  - answer: "GitHub code search and git grep both find TODO and FIXME comments without installing anything, but each returns only a snapshot. The trend comes from commit history. gitdebt's debt markers reading computes that movement from a shallow clone of the repository's git history."
    question: Can GitHub search for TODO and FIXME comments without installing anything?
  - answer: "Git blame shows when a line was last changed, which can be later than when the marker was written. The git documentation says blame \"annotates each line in the given file with information from the revision which last modified the line\". A reformatted line carries the reformat's date."
    question: Does git blame show when a TODO was written?
  - answer: "A high count on its own says nothing about code quality or the people who wrote the comments. The direction carries the signal: a count that rises across commits tells a different story from one that falls. Read it beside commit cadence and fix-labelled changes, as separate signals."
    question: Is a high TODO count a sign of bad code?
  - answer: You should pin a third-party action by commit SHA if a workflow runs one. GitHub Docs says pinning an action to a full-length commit SHA is currently the only way to use an action as an immutable release. A tag or branch name can later point at different code.
    question: Should I pin a third-party action by commit SHA?
publishDate: "2026-11-18"
tags:
  - todo comments
  - fixme
  - git grep
  - git blame
  - technical debt
  - code search
title: "GitHub todo fixme tracker: Measure marker trends over time"
---

**A github todo fixme tracker finds marker comments with `git grep` or GitHub code search, then reads how their count moves across commit history. The count is a directional signal, not a verdict on code quality or on the people who wrote the comments.**

This is for anyone adopting a library or inheriting a codebase where TODO and FIXME comments turn up in file after file. A public repository's history is already readable, so tracking those markers needs nothing added to the code.

After reading, you can find markers, measure their trend, date them within git blame's limits, choose whether to leave, convert, delete or enforce them, and read a dependency's trend without installing anything.

> **Key Takeaways**
> - A snapshot count can't say whether debt is growing. The trend across commits can.
> - [`git blame`](https://git-scm.com/docs/git-blame) reports the last revision to modify a line, so a marker's apparent date can be later than when someone wrote it.
> - Triage or batch existing markers before converting them to issues, or the first scan can flood the tracker.
> - Markers concentrated in one file or author show where knowledge has pooled, not who is at fault.

## What do TODO, FIXME, HACK and BUG comments mean?

`TODO` conventionally marks work the author plans to do, while `FIXME`, `BUG` and `HACK` conventionally mark something the author already knows is wrong, broken or awkward. No standard defines these words, and teams use them differently.

- `TODO` points at something the author means to add or change later. The code around it usually works as written.
- `FIXME` points at code the author believes is wrong.
- `BUG` records a defect the author has seen.
- `HACK` marks a deliberate shortcut or workaround.

The groups overlap in practice. A `TODO` can describe a real defect, and a `FIXME` can describe a tidy-up. The sorting follows the word the author chose, whatever the severity of the problem.

Tools draw the lines differently too. [ClineTools' TODO Tracker](https://clinetools.com/tool-todo-tracker) looks for markers including `TODO`, `FIXME`, `HACK`, `BUG`, `WORKAROUND`, `OPTIMIZE`, `REFACTOR`, `DEPRECATED` and `NOTE`, and applies "Priority classification" with "high, medium, and low severity levels." The tool assigns that priority; the comment's author did not.

A marker records what its author believed on the day they wrote it. It can't show whether the code is defective today, whether the problem was fixed since, or who should handle it. Read markers as self-reported notes.

## How does a github todo fixme tracker find markers in a repository?

You find TODO and FIXME comments in a GitHub repository with code search, `git grep` or a language-aware parser, each stricter than the last, and each with its own false positives.

1. Search in the browser with GitHub code search. Open github.com and type a query such as `repo:facebook/react FIXME`, then repeat it for `TODO`.

 Success is a list of matching files, but the search matches the word anywhere, so string literals, documentation, test fixtures, and vendored or generated files all count. It returns today's list, with no way to see earlier counts.

2. Run `git grep` in a local clone. Clone the repository, then exclude vendored and generated paths with pathspecs:

   ```bash
   git grep -nE 'TODO|FIXME' -- . ':(exclude)vendor' ':(exclude)node_modules' ':(exclude)*.min.js'
   ```

 Success is one `file:line:text` row per marker, so you can count rows with `| wc -l`. Strings, documentation, fixtures and committed generated files still count unless you add their paths to an exclusion list you'll maintain for each repository, and the search covers tracked files at the checked-out commit only.

3. Read comment tokens with a language-aware parser. A parser reads only comments, so a `"TODO"` inside a string literal no longer counts. The [Track TODO Action's marketplace page](https://github.com/marketplace/actions/track-todo-action) says its source code "is parsed using tdg which supports comments for almost all existing languages."

 The parser still counts markers in vendored and generated files unless you exclude them, plus comments in documentation and test fixtures, and it misses files in languages it doesn't cover while taking more setup than a one-line command.

Each step removes some false positives left by the one before, and none removes all of them. Documentation prose that says "TODO" passes every step, because it's a real comment or a real file.

Pick one method and keep its settings fixed: the marker list, the excluded paths and the parser version. A `git grep` count and a parser count differ for reasons unrelated to the code, so numbers over time are comparable only when they come from the same method.

## How does a github todo fixme tracker measure counts over time?

You track a GitHub TODO and FIXME count over time by running the same search at several points in commit history, either with git yourself or by reading a report that already did it. A single count is a snapshot; the trend shows whether a project is accumulating or clearing marker comments.

### Measure it yourself with git

1. Fix one search and keep it. Choose a pattern such as `TODO|FIXME` and a list of excluded paths (vendored and generated folders). Changing either between checkpoints makes the counts incomparable.
2. Pick checkpoints, such as the last commit before the start of each quarter.
3. Count at each checkpoint. Run the same command against each commit, with no checkout needed:

```bash
git grep -I -n -E 'TODO|FIXME' \
  $(git rev-list -n 1 --before=2024-01-01 HEAD) | wc -l
```

4. List the commits that moved the count. The [git log documentation](https://git-scm.com/docs/git-log) says `-S<string>` looks for differences that change the number of occurrences of the specified string, and `-G<regex>` looks for differences whose patch text contains added or removed lines matching the regex. Run `git log --date=short --format='%h %ad %s' -S'FIXME'` to see them.

That's the cost of a hand-built github todo fixme tracker: you run one search per checkpoint, and every run needs the same method, or the line you draw is noise.

### Read the history in a report

1. Open a public report at `gitdebt.com/owner/repo` in a browser. You add no workflow, token or tool to your own code.
2. Read the debt markers reading, which sits with the other health readings, then the recent TODO/FIXME movement chart, which shows direction across commits.
3. Check the method. gitdebt's [methodology page](https://gitdebt.com/about) describes health signals as computed from a shallow clone, so the chart reflects the history that clone contains. The page also sets out what the debt markers reading counts and excludes; read it before comparing two repositories.

For example, open the [angular/angular report](https://gitdebt.com/angular/angular). Find the debt markers reading beside maintenance, ownership and repair load, then scroll to the recent TODO/FIXME movement chart. A line trending up while maintenance stays steady means the project is adding flagged work faster than it clears it.

To see this for a dependency you're weighing, [analyze any public repository at gitdebt.com/report](https://gitdebt.com/report) and read its debt markers next to the other readings.

## How old is a TODO comment, and who wrote it?

Git blame shows who last changed a TODO line and when, which is not always who wrote the marker or when. The [git-scm blame documentation](https://git-scm.com/docs/git-blame) says blame "annotates each line in the given file with information from the revision which last modified the line."

A `// TODO: handle timeouts` line written years ago and later re-indented by a formatter shows the formatting commit in blame: a recent date and the person who ran the formatter. Only the whitespace is new.

Blame has documented options that reduce this distortion:

- `-w` tells blame to "ignore whitespace when comparing the parent's version and the child's to find where the lines came from."
- `-M` will "detect moved or copied lines within a file."
- `-C` goes further: "in addition to -M, detect lines moved or copied from other files that were modified in the same commit."
- `--ignore-rev <rev>` will "ignore changes made by the revision when assigning blame, as if the change never happened." Name a bulk-formatting commit there and blame skips it.

These options narrow the gap without closing it. A comment that someone rewrote still belongs to the rewrite.

Read authorship as a map of where knowledge has pooled. If one file, or one person's commits, holds most of the markers, that is where the context sits. It's a fact about the codebase, not a judgment about any person.

Ownership is a separate reading: how few contributors it takes to write half the commits. A project can show concentrated ownership with few markers, or many markers spread across many authors.

To see both beside the commit history, open a [report for any public repository](https://gitdebt.com/report), such as `vitejs/vite`, and read the debt markers and ownership readings one after the other.

## Should you leave, convert, delete or enforce TODO comments?

Leave a TODO comment when it is recent, small and still accurate, convert it to an issue when it needs an owner and a priority, delete it when nobody plans to do the work, and enforce a limit when the count must stop growing.

| Criterion | Leave | Convert to issues | Delete | Enforce |
| --- | --- | --- | --- | --- |
| Marker stays beside the code | Yes | Yes | No | Yes |
| Creates a tracker entry | No | Yes | No | No |
| Blocks new markers | No | No | No | Yes |
| Needs token permissions | No | Yes | No | No |
| Main risk | Drift | Issue flood | Lost context | Bypass |

Leaving a marker costs nothing and keeps the note next to the code. Markers drift out of date, and nothing ranks them. Check whether the count is rising in a [gitdebt report](https://gitdebt.com/report) before deciding how much effort the pile deserves.

The [Track TODO action on GitHub Marketplace](https://github.com/marketplace/actions/track-todo-action) is the documented example of converting. Its page says it "will manage issues based on TODO / BUG / FIXME / HACK comments in the source code".

It also says: "When a new todo comment is added, a new issue is created. When this comment is removed on the branch it was added, the corresponding issue is closed."

On a repository with many existing markers, the first scan can open an issue for each one, so triage or batch the markers first. The workflow token needs permission to create issues, and the action's Marketplace page lists its own token requirement.

[GitHub Docs on token permissions](https://docs.github.com/en/actions/security-for-github-actions/security-guides/automatic-token-authentication) says the `permissions` key "allows you to configure the minimum permissions required for a workflow or job", so grant only that.

Pin third-party actions too. [GitHub's security hardening guide](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions) has a section titled "Pin actions to a full-length commit SHA". A released version tag is the other option; a branch such as `@master` is not, because it changes whenever its owner pushes.

```yaml
- uses: actions/checkout@main   # branch: moves
- uses: actions/checkout@v4     # released tag: fixed
```

Deleting is honest when the idea is dead, though the reasoning behind it leaves the file with the comment.

Enforcement relies on git hooks. The [githooks documentation](https://git-scm.com/docs/githooks) says the `pre-commit` hook "takes no parameters, and is invoked before obtaining the proposed commit log message and making a commit", so it must inspect the staged changes itself. A non-zero exit aborts the commit, and the hook "can be bypassed with the --no-verify option".

The `update` hook runs on the receiving repository and "takes three parameters: the name of the ref being updated, the old object name stored in the ref, and the new object name to be stored in the ref". A non-zero exit "prevents git-receive-pack from updating that ref".

## Can a github todo fixme tracker help you judge a dependency?

TODO and FIXME trends give you one directional signal about a dependency: whether its maintainers are accumulating or clearing the debt they flagged themselves.

A marker count that climbs release after release means flagged work is arriving faster than it is closed. A count that flattens or falls means someone is finishing items or deleting stale ones.

Commenting habits differ between projects. One team marks every loose end in the source, while another keeps them in the issue tracker and leaves the code clean. Compare a project with its own past, not with a neighbor's total.

Read the other health readings separately, because each answers a different question:

- Maintenance is commit cadence: how recently and how steadily the project changes.
- Ownership is how few contributors write half the commits. It shows where knowledge has pooled in a codebase and isn't a judgment about any person.
- Repair load is the share of file changes whose commits are labeled as fixes.

A rising marker count with steady commits reads as an active project with a growing list of planned work. The same rise with falling cadence reads as a backlog that fewer people are touching. Keep the readings apart instead of blending them into one score.

Open the reports for `vitejs/vite` or `facebook/react` and read the debt markers movement beside the other three. You can [look up any public repository](https://gitdebt.com/) the same way, and the [frontend frameworks comparison page](https://gitdebt.com/compare/frontend-frameworks) sets similar projects side by side.

## Frequently asked questions

### Can GitHub search for TODO and FIXME comments without installing anything?

GitHub code search and `git grep` both find TODO and FIXME comments without installing anything, but each returns only a snapshot.

### Does git blame show when a TODO was written?

Git blame shows when a line was last changed, which can be later than when the marker was written. A reformatted line carries the reformat's date unless you pass `-w` or name the formatting commit with `--ignore-rev`.

### Is a high TODO count a sign of bad code?

No. The direction carries the signal: a count that rises across commits tells a different story from one that falls. Read it beside commit cadence and fix-labelled changes, as separate signals, and compare a project with its own past.

### Should I pin a third-party action by commit SHA?

Pin it if a workflow runs a third-party TODO action. A tag or branch name can later point at different code.

## What should you do next with your TODO and FIXME markers?

Run your github todo fixme tracker as a routine: a fixed search today, a trend check this week and a policy decision this month.

**Today:** Run one fixed search, either `git grep -nE "TODO|FIXME"` or the same query in GitHub code search. Write down the method and the exact pattern so later counts compare with this one.

**This week:** Read the trend for your own repository and for any dependency you're weighing, such as `vitejs/vite`. Triage the markers before converting any to issues.

**This month:** Decide between leave, convert, delete or enforce. If you use a workflow, pin the action to a full commit SHA and give its token only the permissions it needs.

To read a repository's marker history without installing anything, [analyze any public repository](https://gitdebt.com/report) with gitdebt.

## Sources

[git-blame Documentation](https://git-scm.com/docs/git-blame) · [TODO Tracker - ClineTools | Find TODO, FIXME, HACK & BUG…](https://clinetools.com/tool-todo-tracker) · [Track TODO Action · Actions · GitHub Marketplace · GitHub](https://github.com/marketplace/actions/track-todo-action) · [git-log Documentation](https://git-scm.com/docs/git-log) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [Automatic token authentication - GitHub Docs](https://docs.github.com/en/actions/security-for-github-actions/security-guides/automatic-token-authentication) · [Security hardening for GitHub Actions - GitHub Docs](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions) · [githooks Documentation - Git](https://git-scm.com/docs/githooks)
