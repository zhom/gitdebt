---
description: How do you spot an inactive GitHub project? Check the archived flag, last human commit, latest release and issue replies. Learn the six checks in order.
faq:
  - answer: Check the last commit on the default branch, the latest release, how maintainers answer recent issues and pull requests, and whether the repository is archived. Then discount bot commits and compare the commit trend with the star history. Quiet alone does not mean abandoned.
    question: How do I check if a GitHub repository is still maintained?
  - answer: A long gap between commits does not prove abandonment, because a finished library can sit quiet and still work. Check whether maintainers reply to recent issues, whether releases still appear, and whether the code runs on current versions of its language and platform. Silence on all three points toward abandonment.
    question: Does a long gap between commits mean a project is abandoned?
  - answer: You can tell by reading the author on each recent commit in the commit list for the default branch. If every author is a bot account such as Dependabot, the project is getting automated dependency updates and no human edits. Find the last commit written by a person.
    question: How can I tell if the recent commits are only from Dependabot?
  - answer: "You can keep using an archived repository, and archiving also tells you the owner no longer maintains the project. GitHub's blog says archiving sends an important signal that influences a user's decision to rely on a project. Keeping a pinned copy is your call."
    question: Does an archived GitHub repository mean I must stop using it?
  - answer: "Stars can't tell you whether a project is alive, because they record what the crowd did on past days, while commit history records what maintainers did. Read them as two separate sources. Put the star curve next to the commit trend in a report for the repository, especially after a star surge."
    question: Can stars tell me whether a project is alive?
publishDate: "2026-11-06"
tags:
  - github
  - open source
  - dependencies
  - repository health
  - maintenance
title: "How to spot an inactive GitHub project: 6 checks in order"
---

**How to spot an inactive GitHub project comes down to six checks, in order: the archived flag, the last human commit on the default branch, the latest release, how maintainers answer recent issues and pull requests, any deprecation notice, and a published heuristic. Discount bot commits, which can make a dead project look busy. Before you call a quiet log abandoned, check whether the library is simply finished.**

The problem shows up when you're adding a library, or reviewing one already in your lockfile, and its repository page disagrees with itself. The latest commit looks recent, but the newest release is years old. GitHub's own tools for finding stale repositories are built for an administrator scanning an organization they run.

Working from the outside, you can run a short, ordered check on any public repository you don't own and reach a defensible decision: keep, watch or replace.

> **Key Takeaways**
> - Stars record what the crowd did and commits record what maintainers did, so read them as two separate sources.
> - A package can carry a deprecation warning on its registry while its GitHub repository stays unarchived.
> - No reading has a universal cutoff. Set your own quiet window by how hard the dependency would be to replace.

## What counts as an inactive GitHub project?

An inactive GitHub project is one where maintainers have stopped committing to the default branch, answering issues and shipping releases. A recent push to any branch, or a bot commit, does not prove activity. Judge by human commits on the default branch, plus releases and issue responses, over a window you choose.

![Illustration for What counts as an inactive GitHub project?](https://api.birchseek.com/m/l_7dvfzHE7x2NRQgUXyn1Yf6zIajhhed.webp)

The default branch is the one the repository page opens on, and it usually holds the code people install. A push to any branch is a weaker signal.

A feature branch, a stalled pull request or an automated job can all register a push while the code you'd depend on stays untouched.

GitHub's own tooling draws the same line. The [stale-repos action](https://github.com/marketplace/actions/stale-repos) "identifies and reports repositories with no activity for configurable amount of time."

Its README exposes an `ACTIVITY_METHOD` setting to decide what counts as activity. The default, `pushed`, uses the last push to any branch. The `default_branch_updated` option uses the latest commit on the default branch.

The two settings can disagree about the same repository. If maintainers last committed to `main` long ago and a bot pushed a branch yesterday, `pushed` makes the repository look current. Under `default_branch_updated` it looks idle, which matches what the maintainers did.

The second measure still counts bot commits that land on the default branch. If a bot merges version bumps automatically, the commit log keeps moving after the people have left. Read the author names as well as the dates.

A project counts as inactive when all three of these hold over your window:

- No human commits on the default branch.
- No releases.
- No maintainer responses to issues or pull requests.

Pick the window by how hard the dependency is to replace. A tool you can swap out in an afternoon justifies a shorter window than a database driver you'd have to rewrite around.

A finished, stable library can also go quiet without being abandoned. Treat a missing signal as a reason to look closer before you decide.

## How do you spot an inactive GitHub project in six checks?

You can spot an inactive GitHub project by running six checks in order, from the quickest and most decisive to the slowest, and by noting what each one can't prove.

1. Look for an archived notice at the top of the repository page. The owner sets that flag, so a missing notice doesn't prove the project is alive. Plenty of abandoned repositories are never archived.

2. Open the commit list on the default branch and read the dates and authors. Switch to the default branch first, because a push to a side branch says nothing about the code people install. Skip commits from accounts such as `dependabot[bot]` and find the last human author, since a log full of recent commits can be automation only.

3. Open the Releases page and note the date of the latest release. A healthy sign is a release date that fits the project's own history of shipping. A finished library may ship rarely and still work, so an old release alone proves little.

4. Read recent issues and pull requests, open and closed, and look for maintainer replies. A reply, a label or a merge from a maintainer is a sign of life. An issue left without a response says more than an issue that is still open. Popular, healthy projects carry high open-issue counts, so read the replies instead of the total.

5. Read the README and the package page for deprecation or successor notices. Phrases like "no longer maintained" or "use X instead" settle the question. Check the registry's deprecation flag too. Maintainers often never add a notice, so silence here is not evidence.

6. Cross-check with a published heuristic. Read how the heuristic defines a maintained project before you lean on the result. A complete, stable library can score low on one.

Owners run similar tools on their own repositories. The stale-repos action reports idle repositories so owners can consider archiving them. The owner chooses that window, so on a project you don't control, your own checks have to do the work.

Read the result as a pattern. A repository with no human commits on the default branch, no recent release and unanswered issues is inactive. A repository that is quiet on only one of those signals may simply be finished.

## What does an archived GitHub repository mean?

An archived GitHub repository is one its owner has made read-only to indicate the project is no longer actively maintained. [GitHub Docs on archiving](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) says: "You can archive a repository to make it read-only for all users and indicate that it's no longer actively maintained."

Nobody, including the owner, can push new commits to an archived repository until the owner unarchives it.

[The GitHub Blog](https://github.blog/open-source/maintainers/announcing-the-stale-repos-action/) describes the same act from the owner's side: archiving projects that are no longer in active development "sends an important signal to project communities, influencing a user's decision to rely on your project." Archiving is the documented way for an owner to say a project has ended.

The package registry is the second place to look. An owner can mark a package on its registry without archiving the repository on GitHub.

A registry notice is separate from the GitHub archive flag. A repository can show one without the other.

Registries differ in what they offer. Check each registry's own listing for the package, such as its PyPI project page, instead of assuming it works like npm.

Both markers are statements by the owner, and neither is an audit. A repository can be abandoned without being archived or deprecated.

An archived repository can also be the right pin for a frozen need, such as a small library that does one job and is finished.

## Do stars help you spot an inactive GitHub project?

A GitHub star records what the crowd did on one day, so stars can't tell you whether maintainers are still working on a project. Commits record what the maintainers did.

[gitdebt's methodology page](https://gitdebt.com/about) says star history comes from GitHub's own star history, as exact daily counts net of unstars. The health readings come from the commit history of a shallow clone.

To read the two side by side:

1. Open the [gitdebt report page](https://gitdebt.com/report) and enter a public `owner/repo`. No sign-in or setup is needed.
2. Read the star history first and look for a surge.
3. Scroll to the commit trend, then read the maintenance reading beneath it.

A star surge followed by a flat or falling commit trend is worth a closer look: the crowd arrived and the maintainers stopped committing. The reverse, a flat star curve with steady commits, can mean a quiet but healthy project.

Three directional readings come from the commit history:

- Maintenance is commit cadence: how regularly commits land.
- Ownership is how few contributors write half the commits. It shows where knowledge has pooled in a codebase and is not a judgment about any person.
- Repair load is the fix-labelled share of file changes. It's a heuristic on commit messages and does not prove a bug.

Each reading points in a direction without settling the question. A finished library can show low cadence and still be complete. gitdebt also does not flag stars as fake, real or suspicious; it only charts the daily counts.

For a reference point, open the [angular/angular report](https://gitdebt.com/angular/angular). Read its star history, then its commit trend and the three readings beneath. That gives you a large, well-known public repository to compare against.

Repeat the same steps with the dependency you're judging, and compare how its commit trend moves after its own star growth.

To try it on a project in your lockfile, [run a report for that owner/repo](https://gitdebt.com/report).

## Is a quiet dependency complete or abandoned?

A quiet dependency is complete when it still works and maintainers still answer, and abandoned when nobody replies and nothing is looked after, so commit silence alone can't separate the two. Weigh the answers to these six questions together.

1. Is it archived or deprecated? Yes points to abandoned. A no tells you little, because many maintainers leave without archiving.

2. Do maintainers still reply to issues and security reports, even without commits? Yes points to quiet but complete. Read the replies, since a reply that only says "won't fix" is a weaker sign than a diagnosis.

3. Does the latest release work with the current runtime and the dependencies you use? Yes supports complete. It can still break at the next runtime release, so run it in your own CI before you rely on it.

4. Are open bugs reproducible and unanswered, or closed with explanations? Reproducible and unanswered points to abandoned, while closed with explanations points to care. Automated stale bots can close issues with no explanation, so open a few closures and read them.

5. Are the remaining commits mostly Dependabot version updates? Yes means little human work is happening. A small, stable library may need only version bumps, so check whether a person merges them and cuts releases.

6. Does the README describe the project as complete or in maintenance mode? Yes, paired with a small, stable scope, supports complete. A README can be out of date, and a maintainer's stated departure outweighs it.

Two more readings from the commit history add context to the six answers: ownership concentration and a thin commit trend. A focused library written by one author can be healthy even when its ownership is concentrated.

Repair load reads the same way. It describes fix-labelled history and doesn't prove a bug.

These readings come with no universal cutoff. Set your own window for how long a dependency may stay quiet, based on how much you depend on it and how fast its runtime moves.

The [gitdebt methodology page](https://gitdebt.com/about) explains how gitdebt computes these readings and what it deliberately does not do.

## What should you do after you spot an inactive GitHub project?

After you spot an inactive GitHub project, sort it into keep and pin, watch, or replace, then look for a successor before you plan the swap.

Keep and pin applies when the library is stable, complete and has no known security issue. Watch applies when the repository is quiet but maintainers still reply. Replace applies when it's archived, deprecated, or has security or compatibility problems nobody answers.

1. Re-read the README, the archive banner and the registry listing, and look for a successor or migration note in each. Read an archive banner as the maintainers' own statement. Check pinned issues too, since a note can sit there instead.

2. Open the repository's forks and run the same activity checks on any candidate. A fork is only useful if it has its own recent human commits and releases. A fork that only carries the original history, or whose recent commits are automated, isn't a maintained successor.

3. Shortlist replacements and compare them side by side. Use the [gitdebt compare tool](https://gitdebt.com/compare) to overlay each candidate's star history; the [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks) shows what a comparison field looks like. Open each candidate's commit trend as well and keep the two readings separate.

4. Plan the swap. Pin the version you run today, open a tracking issue that names one owner, and set a review date. The result is a written decision per dependency: keep, watch or replace, with a named person and a date attached.

## Frequently asked questions

### How do I check if a GitHub repository is still maintained?

Check the last commit on the default branch, the latest release, how maintainers answer recent issues and pull requests, and whether the repository is archived. Then discount bot commits and compare the commit trend with the star history. Quiet alone does not mean abandoned.

### Does a long gap between commits mean a project is abandoned?

Not by itself, because a finished library can sit quiet and still work. Check whether maintainers reply to recent issues, whether releases still appear, and whether the code runs on current versions of its language and platform. Silence on all three points toward abandonment.

### How can I tell if the recent commits are only from Dependabot?

Open the commit list for the default branch and read the author on each recent commit. If every author is a bot account such as Dependabot, the project is getting automated dependency updates and no human edits. Find the last commit written by a person.

### Does an archived GitHub repository mean I must stop using it?

Archiving doesn't force you to stop, but it tells you the owner no longer maintains the project. The repository is read-only, so no new commits can land until the owner unarchives it. Keeping a pinned copy is your call.

### Can stars tell me whether a project is alive?

Stars can't, because they record what the crowd did on past days, while commit history records what maintainers did. Read them as two separate sources. Put the star curve next to the commit trend in a [report for the repository](https://gitdebt.com/report), especially after a star surge.

## Next steps: audit your dependencies this week

These time-boxed steps turn the procedure into a routine.

- **Today:** Pick the one dependency you are least sure about and run the six checks, from the archived flag through to how maintainers answer recent issues.
- **This month:** List every dependency marked watch or replace, set a review date for each, and shortlist alternatives, including maintained forks.

To start with the dependency you picked today, [analyze a repository on gitdebt](https://gitdebt.com/) and read its report without creating an account.

## Sources

[stale-repos (GitHub Marketplace)](https://github.com/marketplace/actions/stale-repos) · [Archiving repositories - GitHub Docs](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) · [Announcing the Stale Repos Action](https://github.blog/open-source/maintainers/announcing-the-stale-repos-action/) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about)
