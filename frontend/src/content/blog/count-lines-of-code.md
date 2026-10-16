---
description: How do you count lines of code? Split files into blank, comment and code lines, then total each language with cloc, scc or tokei. Learn what the total misses.
faq:
  - answer: Run a counter such as cloc, scc or tokei in the repository root; each reports files, blank, comment and code lines per language. For a public GitHub repository without installing anything, open gitdebt.com/owner/repo and read the language lines chart, which uses tokei over a shallow clone of HEAD.
    question: How do I count lines of code in a repo?
  - answer: "Blank lines and comments are counted as separate classes, so the code figure leaves them out. The cloc README says it counts blank lines, comment lines, and physical lines of source code. Totals differ when each tool's edge-case rules differ, so read each README's stated limits."
    question: Do blank lines and comments count, and why do cloc and scc totals differ?
  - answer: "GitHub's language data measures bytes, not lines, so it does not show line counts directly. The REST API documentation says each value is the number of bytes of code written in that language. For line counts, run a counter on a checkout or read a gitdebt report."
    question: Does GitHub show lines of code?
  - answer: A higher line count means a larger program, not a better one. Wikipedia defines source lines of code as a software metric used to measure the size of a computer program, so alone it says nothing about quality or complexity. Read it beside commit history, and treat both as directional.
    question: Does more lines of code mean better software?
publishDate: "2026-10-16"
tags:
  - lines of code
  - cloc
  - scc
  - tokei
  - github repositories
  - code metrics
title: "Count lines of code: tools, commands and limits"
---

**To count lines of code, [split every file into blank lines, comment lines and code lines](https://github.com/AlDanial/cloc), then total each class per language. Open-source counters such as cloc and [scc report these classes in many languages](https://github.com/boyter/scc), and gitdebt shows them for any public GitHub repository without an install, [using tokei on a shallow clone of HEAD](https://gitdebt.com/about).**

Two counters often give the same repository different totals, and both can be correct under their own counting rules. That matters to developers and engineering managers who must put one size figure in a report and stand behind it.

After reading, you can run the right command, read each column of the result correctly, and say plainly what the total does not mean.

> **Key Takeaways**
> - A blank line inside a block comment may count as blank or as comment, so two counters can disagree on one file.
> - [GitHub's language percentages are byte shares](https://docs.github.com/en/rest/repos/repos#list-repository-languages), not line counts.
> - A line count measures how much text exists, not quality or productivity.
> - Keep the HEAD checkout and the commit history as two separate readings, never one score.

## What does it mean to count lines of code?

Counting lines of code (LOC) means tallying the lines in source files, usually split into three classes: blank lines, comment lines and code lines. [Counters such as cloc](https://github.com/AlDanial/cloc) and scc report each class per language, so the code-line figure leaves out blanks and comments.

![Image generated](https://api.birchseek.com/m/Z4CYKQ3xtKGlrpfh3AkP8cp5VvLNyB0k.webp)

A blank line has nothing on it. A comment line holds only a comment. A code line holds source code.

The cloc README describes its job this way: "cloc counts blank lines, comment lines, and physical lines of source code in many programming languages." [The scc README](https://github.com/boyter/scc) uses nearly the same wording.

When a report says "lines of code," it usually means the code column, not the file total. Two totals for one repository can both be correct if one counts every line and the other counts code lines only.

In this three-line file, line 1 is a comment, line 2 is blank, and line 3 is code:

```text
# add two numbers

total = 1 + 2
```

The file has 3 physical lines, but its code count is 1. The example shows the definitions; it is not output from any tool.

Physical lines also differ from logical statements. [Wikipedia's "Source lines of code" entry](https://en.wikipedia.org/wiki/Source_lines_of_code) says "There are two major types of SLOC measures: physical SLOC (LOC) and logical SLOC (LLOC)."

Logical SLOC "attempts to measure the number of executable" statements, so two statements on one physical line count as two logical lines but one physical line. The counters named here report physical lines.

## How do counters treat comments, generated files and duplicates?

Counters treat comments, generated files and duplicates by their own documented rules, so two tools can give one repository different totals without either being wrong.

The smallest case is a five-line block comment whose third line is empty. A counter that files the empty line under comments reports 5 comment lines and 0 blank lines.

A counter that files it under blank lines reports 4 comment lines and 1 blank line. The file is identical; only the split changes.

Four more cases move totals the same way:

- **Minified files:** a whole program can sit on one very long line, so line counts say little about its size.
- **Generated files:** output from a code generator can dwarf the hand-written code around it.
- **Duplicate files:** the same file committed twice can be counted twice or once, depending on the tool.
- **Vendored code:** third-party source copied into the repository counts as yours unless you exclude its directory.

Each tool documents whether it skips, counts or flags these. Read that before you compare two totals.

For cloc, [the documentation page](https://cloc.sourceforge.net/) has a Differences section and a Limitations section, listed in its contents. Check both for the case in front of you.

[The scc README](https://github.com/boyter/scc) also lists unique lines of code among its goals. It says scc aims to "produce unique lines of code or DRYness metrics."

A unique-line count counts each repeated line once. Copied files add to total lines but not to unique lines.

Before you compare two totals, record the tool name, its version and any exclude rules you used. Without those, the numbers are not comparable.

## cloc vs scc vs tokei: which line counter fits your repository?

The right counter among cloc, scc and tokei depends on whether you need plain blank, comment and code totals, extra estimates, or a tool you can run with the least setup.

| | cloc | scc | tokei |
| --- | --- | --- | --- |
| Current home | github.com/AlDanial/cloc | github.com/boyter/scc | github.com/XAMPPRocky/tokei |
| Stated scope | Blank, comment and code lines | Same line classes, plus estimates | Files, lines, code, comments and blanks |
| Documented extras | Run comparison, combined reports | COCOMO, LOCOMO, complexity, unique lines | Over 150 languages, nested comments |

[cloc's Quick Start](https://github.com/AlDanial/cloc) names three install routes: GitHub releases, a package manager, or the Docker image. The Windows executable has no requirements, the source version needs a Perl interpreter, and the Docker version needs Docker.

[cloc's documentation](https://cloc.sourceforge.net/) also lists Differences and Combine Reports. Those help when you compare two checkouts or merge counts from several directories.

[The scc README](https://github.com/boyter/scc) describes a tool "similar to cloc, sloccount and tokei". It adds COCOMO calculation, LOCOMO estimation for LLM-based development costs, complexity estimates "similar to cyclomatic complexity calculators", and unique lines of code, which it calls DRYness metrics.

Treat those as estimates. A cost figure from a formula is not a measurement of effort.

tokei is the third common counter, and the one gitdebt uses. [Its README](https://github.com/XAMPPRocky/tokei) says it shows files, total lines, code, comments and blanks grouped by language, supports over 150 languages, and installs with `cargo install tokei`.

Check the LICENSE file of any counter before you bundle it into a product.

**Best for:**

- **cloc:** comparing two versions of a codebase, merging reports, or running on Windows with no runtime.
- **scc:** a single run that also gives complexity, unique-line and COCOMO figures.
- **tokei:** a counter whose README lists the languages you use and an install route that fits your machine.

If two counters give you different totals, pick one, write down its version, and compare only against runs of that same tool.

## How do you count lines of code across a whole repository?

You count lines of code across a whole repository by running one counter at the repository root and reading its per-language table. These steps use `vitejs/vite` as the example.

1. **Clone a shallow copy.** Run `git clone --depth 1 https://github.com/vitejs/vite.git`, then `cd vite`. [The git documentation](https://git-scm.com/docs/git-clone) describes `--depth` as creating "a shallow clone with a history truncated to the specified number of commits." That is enough for a language count, but not for history or older commits.

2. **Run a counter at the root.** Use any one of these:

```bash
cloc .
scc
tokei
```

3. **Read the table.** Each row is a language, with its file count and its blank, comment and code lines. Compare the code column across languages, not the grand total.

4. **Count a specific commit.** Shallow clones hold only the latest commit, so run `git clone https://github.com/vitejs/vite.git` without `--depth`. Then run `cloc --git <commit-hash>`, replacing the placeholder with a real hash from `git log`. The output is the same table, for that commit.

5. **Exclude vendored and generated directories.** Skip this and a committed `node_modules` or `dist` folder can swamp your own code. Each tool has its own option:

```bash
cloc --exclude-dir=node_modules,dist .
scc --exclude-dir node_modules,dist
tokei . --exclude node_modules --exclude dist
```

[The scc README](https://github.com/boyter/scc) also says its `.ignore` files are "100% the same as.gitignore files with the same syntax," so you can list paths in one file instead of repeating flags.

6. **Check the result.** Re-run after excluding. A correct run shows your main languages at the top and no rows for vendored or build output languages you did not write.

If you only need the language breakdown of a public repository, skip the install. Enter its `owner/repo` at [gitdebt's report page](https://gitdebt.com/report) with no account.

The language lines chart reads the current checkout only, so it says nothing about commit history.

## What does a lines of code count not tell you?

A lines of code count tells you how large the source text is, and on its own it says nothing about complexity, quality, productivity or maintainability.

[The Wikipedia entry on source lines of code](https://en.wikipedia.org/wiki/Source_lines_of_code) defines the metric as "a software metric used to measure the size of a software program by counting the number of lines in the text of the program's source code." Size is the whole claim.

Each item below is a separate question that a line count cannot answer:

- **Complexity:** A 40-line function with deeply nested branches can be harder to change than a 400-line table of constants.
- **Quality:** Two files of equal length can differ in tests, naming and defects.
- **Productivity:** Lines written per day rewards verbose code and penalizes a developer who deletes 200 lines to fix a bug.
- **Maintainability:** A large file that nobody touches can cost less than a small one that changes every week.

Some counters add a separate reading. [The scc README](https://github.com/boyter/scc) says it can "estimate code complexity similar to cyclomatic complexity calculators." That is a labeled estimate sitting beside the line count, not part of it.

Comparing languages adds another limit. [The cloc documentation](https://cloc.sourceforge.net/) lists a section called "Third Generation Language Scale Factors" next to one called "Limitations."

Read both before you compare totals across languages, because a scaled count is still a size measure.

To learn what maintainers have done with the code, read a second source: the commit history. The [gitdebt methodology page](https://gitdebt.com/about) shows how the checkout and the history are kept apart. Treat both as directional readings, not verdicts.

## How do you get lines of code by language for a public GitHub repo?

You get lines of code by language for a public GitHub repo from a gitdebt report or a local counter, because GitHub's own language data measures bytes, not lines.

1. **Check what GitHub's language bar measures.** The REST API endpoint "List repository languages" returns one value per language: `curl https://api.github.com/repos/facebook/react/languages`. [GitHub's documentation](https://docs.github.com/en/rest/repos/repos#list-repository-languages) says each value is "the number of bytes of code written in that language." Bytes are not lines, so do not read these shares as line counts.
2. **Open the gitdebt report.** Go to `gitdebt.com/facebook/react`. There is nothing to install, no sign-in and no repository to connect. If nobody has analyzed a repository yet, you see a placeholder while the work is queued, so reload the page later.
3. **Read the language lines chart.** [gitdebt measures it with tokei](https://gitdebt.com/about) over a checkout of the current HEAD, so it shows the code as it stands now.
4. **Repeat for a second repository.** Open `gitdebt.com/vitejs/vite` and compare the two language lines charts. Then compare each commit trend.

Expect size and activity to come apart. A repository can hold far more code than another and still show a quieter commit trend, or the reverse.

The checkout tells you what the code is. The commit history tells you what maintainers did.

A shallow clone leaves out older history. That limit matters for commit-based readings, which the [methodology page](https://gitdebt.com/about) explains. It does not affect a language count of HEAD, which needs only the latest files.

To skip the clone, [open a report for any public repository](https://gitdebt.com/report) and read its language lines chart.

## How do line counts fit into a repository health check?

A repository health check treats a line count as one input and reads it beside the commit history, with the two sources kept apart.

The HEAD checkout is the set of files on the default branch right now. Language lines come from there, so they show what the code is today.

They say nothing about how the code got that way. They also carry the counting edge cases covered earlier, such as generated and vendored files.

The commit history shows what maintainers did. A gitdebt report reads it through charts its [about page](https://gitdebt.com/about) defines: the commit trend (monthly commit volume), ownership concentration (how few contributors it takes to write half the commits), fix-labelled changes, and TODO and FIXME movement.

The about page describes how [health signals are computed from a shallow clone](https://gitdebt.com/about). Each reading has a limit.

Ownership concentration shows where knowledge has pooled in a codebase; it is not a judgement about any person. Fix-labelled changes describe labeled history and do not prove a bug.

Do not merge these into one score. Put them side by side and let a mismatch raise a question.

A large codebase with a quiet commit trend could be finished, stable or neglected, and the numbers cannot say which. A small codebase with heavy commit activity could be young, or it could be rewriting itself.

For a set of repositories to practice on, open the [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks). Then read a single report such as [angular/angular](https://gitdebt.com/angular/angular) and check its language lines against its commit trend.

## Frequently asked questions

### How do I count lines of code in a repo?

Run a counter such as cloc, scc or tokei in the repository root; each reports files, blank, comment and code lines per language. For a public GitHub repository without installing anything, open `gitdebt.com/owner/repo` and read the language lines chart, which [uses tokei over a shallow clone](https://gitdebt.com/about) of HEAD.

### Do blank lines and comments count, and why do cloc and scc totals differ?

Counters report blank, comment and code lines as separate classes, so the code figure leaves out the other two. [The cloc README](https://github.com/AlDanial/cloc) says it "counts blank lines, comment lines, and physical lines of source code". Totals differ when each tool's edge-case rules differ, so read each README's stated limits.

### Does GitHub show lines of code?

GitHub's language data measures bytes, not lines. [The REST API documentation](https://docs.github.com/en/rest/repos/repos#list-repository-languages) says each value is "the number of bytes of code written in that language". For line counts, run a counter on a checkout or read a gitdebt report.

### Does more lines of code mean better software?

A higher count means a larger program, not a better one. [Wikipedia defines source lines of code](https://en.wikipedia.org/wiki/Source_lines_of_code) as "a software metric used to measure the size of a computer program", so alone it says nothing about quality or complexity. Read it beside commit history, and treat both as directional.

## Next steps: count the code, then read the history

Count the code locally first, then pin down the edge cases, then read the commit history as a separate signal.

- **In five minutes:** run `cloc .`, `scc .` or `tokei` in the root of a local checkout. Read the code column, not the grand total.
- **This week:** look for vendored, generated and minified directories and exclude them. Write the tool, its version and your settings beside the number, because a count nobody can reproduce is hard to compare later.
- **Before you rely on it:** read the repository's commit history. A line count says how big the code is, not whether anyone still maintains it.

To skip the setup, look up any public repository at [gitdebt.com](https://gitdebt.com/) and read its language lines beside the four health readings: maintenance, ownership, repair load and debt markers. gitdebt is [MIT-licensed](https://github.com/zhom/gitdebt), and [every public repository report works without an account](https://gitdebt.com/about), so there is nothing to install.

## Sources

[GitHub - AlDanial/cloc: cloc counts blank lines, comment…](https://github.com/AlDanial/cloc) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [Sloc Cloc and Code (scc) - GitHub](https://github.com/boyter/scc) · [REST API endpoints for repositories: List repository…](https://docs.github.com/en/rest/repos/repos#list-repository-languages) · [Source lines of code - Wikipedia](https://en.wikipedia.org/wiki/Source_lines_of_code) · [CLOC -- Count Lines of Code](https://cloc.sourceforge.net/) · [git-clone Documentation](https://git-scm.com/docs/git-clone)
