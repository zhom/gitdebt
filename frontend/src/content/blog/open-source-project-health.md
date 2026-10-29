---
description: "Open source project health: read dated signals like commits, releases, PR replies, contributor diversity, and security policy. Score any repo in 15 minutes."
faq:
  - answer: Check the date of the last commit and last release, then compare recent commit volume with earlier periods. Look at whether issues and pull requests get replies, whether more than one person merges changes, and whether a security policy exists. One signal alone is not proof; record the date you checked.
    question: How can I tell if an open source project is still maintained?
  - answer: "Stars are a poor measure of health because they record interest, not upkeep. GitHub's documentation says starring \"shows appreciation to the repository maintainer for their work.\" A popular repository can still have no recent commits, so read stars beside commit activity."
    question: Are GitHub stars a good measure of project health?
  - answer: "No universal number applies, so treat the metric as a warning sign, not a pass mark. Researchers define the truck factor as \"the minimal number of developers that have to be hit by a truck (or quit) before a project is incapacitated.\" It infers knowledge concentration from history and can't show who is still available."
    question: What is a good bus factor for an open source project?
  - answer: "A practical convention is to re-check before each major version upgrade and on a fixed schedule, such as quarterly. That's a habit, not a researched rule. Re-check sooner after a maintainer change, a license change or a published vulnerability, and write down each date."
    question: "How often should I re-check a dependency's health?"
  - answer: gitdebt does not detect fake stars. It also does not flag stars as fake, real or suspicious, and it does not score accounts. Instead, its report page shows star history beside commit-based readings so you can compare popularity with activity.
    question: Does gitdebt detect fake stars?
publishDate: "2026-10-29"
tags:
  - open-source
  - project-health
  - maintenance
  - security-policy
  - scorecard
title: "Open source project health: check before you depend on it"
---

**Open source project health is a set of dated, measurable signals: maintenance activity, release cadence, issue and PR responsiveness, contributor diversity and security practices. A project is healthy only when several independent signals agree, and popularity alone never counts. Record the check date with every reading, and treat each one as a directional signal, not a verdict.**

You're about to add a package or self-host a project, and the README says "actively maintained" with no evidence. That line can't tell you whether the project is alive. Neither can a star count, which records interest, not whether anyone fixed a bug last month.

After reading, you can run a dated scorecard on any public repository and decide whether to adopt it, contribute to it or fork it.

> **Key Takeaways**
> - Daily commits are still fragile when one contributor writes nearly all of them.
> - A "vetted" or "actively maintained" label without published criteria is a claim, not evidence.
> - A security policy or passing Scorecard check shows a practice exists, not that fixes ship quickly.
> - Re-check on triggers: a major release, a missed release, a new advisory or an archive notice.

## What is open source project health, and how is it measured?

Open source project health is how likely a project is to stay maintained, secure and usable over time, and it is measured through observable signals in the repository and its history.

![Image generated](https://api.birchseek.com/m/LmEbVAHWwj0WFWFPTWZdRnGVNbscp8t-.webp)

Those signals need published criteria. The [kakoni/awesome-healthcare list](https://github.com/kakoni/awesome-healthcare) says each link "has been vetted to ensure the project is active" but doesn't publish the criteria. The CHAOSS project and OpenSSF Scorecard do publish theirs, so each signal below names its data source and its limit.

- **Maintenance activity.** Count commits and merged PRs from the last 90 days on the repository's commit list. [Scorecard's Maintained check](https://github.com/ossf/scorecard/blob/main/docs/checks.md) "determines whether the project is actively maintained." Limit: commits count activity, not quality, and one person can write all of them.
- **Release cadence.** Read the Releases page and tags. [CHAOSS lists a Release Frequency](https://chaoss.community/kbtopic/all-metrics/) metric. Limit: a stable library may release rarely by design.
- **Issue and PR responsiveness.** Open five recent issues and note the wait for a first human reply. CHAOSS lists Time to First Response. Limit: a bot reply or an auto-close can hide silence.
- **Contributor diversity.** Use the Contributors graph. CHAOSS lists Elephant Factor, and Scorecard's Contributors check "tries to determine if the project has recent contributors from multiple organizations." Limit: GitHub profiles don't reliably show an employer.
- **Security practices.** Look for a `SECURITY.md` file and the Security tab. Limit: a policy file shows intent, not how fast fixes ship.

[Scorecard's README](https://github.com/ossf/scorecard) documents a command line run. It reads a GitHub token from the `GITHUB_AUTH_TOKEN` environment variable.

```bash
export GITHUB_AUTH_TOKEN=<your_token>
scorecard --repo=github.com/owner/repo
```

Run it on a repository you're considering. Note which checks it reports, such as Maintained and Security-Policy, beside the date of the run.

## Do stars show open source project health, or only popularity?

Stars show popularity, not open source project health, because a star records one reader's reaction while health depends on what maintainers do afterward. [GitHub's documentation on stars](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) says that starring "shows appreciation to the repository maintainer for their work". That is a reaction, not a repair.

| Signal | What it measures | Limit |
| --- | --- | --- |
| Stars | Interest at the time of starring | Says nothing about upkeep |
| Forks | Copies taken | Many are never changed |
| Last commit date | When anyone last committed | One commit can be a typo fix |
| Commit trend | Trailing 90 days against the 90 before | Counts volume, not quality |
| Release dates | When maintainers shipped | Some projects ship from main |
| Open vs. closed issues | Backlog and closure rate | Labels and triage habits differ |

"Actively maintained" needs criteria and a date. A usable version reads: commits in the trailing 90 days match or exceed the 90 before, as of a stated day.

Third-party scores need their method read first. [LibHunt's healthcare topic page](https://www.libhunt.com/topic/healthcare) prints figures such as "39,987" and "10.0" beside ERPNext without saying, on that page, what each measures. Treat a score with no stated method as unexplained.

gitdebt keeps the two sources apart: stars are what the crowd did, and the commit history is what the maintainers did.

To read them side by side, open the [apache/airflow report](https://gitdebt.com/apache/airflow): its star curve sits on the same page as the maintenance, ownership and fix-labelled change charts.

Note the source and coverage date each chart names, and write that date next to any conclusion you draw.

Use stars to gauge interest and the commit history to gauge upkeep. To see several repositories on one set of axes, try [the compare page](https://gitdebt.com/compare). No sign-in is needed.

## Which governance and funding risks can sink a dependency?

Governance and funding risks sink a dependency when too few people can maintain it, nobody has written down who decides, or the license doesn't clearly let you use the code. Activity charts miss all three, so work through this checklist and date what you find.

The first risk has a name in research. A [2016 paper by Avelino and colleagues](https://arxiv.org/abs/1604.06766) describes truck factor as "the minimal number of developers that have to be hit by a truck (or quit) before a project is incapacitated."

1. **License clarity.** Open `LICENSE` and look for an SPDX identifier such as `MIT` or `Apache-2.0`. A missing or vague license is a risk on its own.

 [GitHub Docs on licensing](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository) says that "without a license, the default copyright laws apply, meaning that you retain all rights to your source code and no one may reproduce, distribute, or create derivative works from your work."

2. **Ownership concentration.** Count how few contributors write half the commits. It shows where knowledge has pooled in a codebase; it is not a judgement about any person.

3. **Written governance.** Look for `GOVERNANCE.md`, `MAINTAINERS` or `CODEOWNERS`. If none exists, decisions probably rest on habit, and you can't tell who may merge or release.

4. **Foundation or fiscal host.** Check the README and website for a foundation or fiscal host page. No named steward means no organization is on record to continue the project if its maintainers step back.

5. **Funding links.** [GitHub Docs on sponsor buttons](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/displaying-a-sponsor-button-in-your-repository) says, "You can configure your sponsor button by editing a FUNDING.yml file in your repository's.github folder." Absence doesn't prove a project is unfunded, but it means no funding path is published.

6. **Succession path.** Look in `CONTRIBUTING.md` or the governance file for how a contributor becomes a maintainer. A documented path suggests the project can outlast one person's availability. A gap suggests a stall is hard to reverse.

Score each item as present, partial or not published, and keep the notes next to your activity data. gitdebt's [methodology page](https://gitdebt.com/about) lists what gitdebt measures and what it leaves out.

## How do you judge a project's security posture?

A project's security posture is judged by three checks you can finish in minutes: whether it publishes a way to report vulnerabilities, whether past reports led to fixed releases, and what an automated scorer says.

**Start with the disclosure path.** [GitHub Docs on security policies](https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository) says that "You can give instructions for how to report a security vulnerability in your project by adding a security policy to your repository." Open the repository's Security tab and look for a `SECURITY.md` file.

GitHub also documents [privately reporting a security vulnerability](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability), another route a maintainer can switch on. If neither exists, a researcher has nowhere safe to send a report.

**Then read the advisory history.** Open the project's published security advisories and pick two or three. For each, write down the date it was reported, the date of the fixed release, and whether older supported versions got the fix too (a backport).

No time-to-fix figure applies to every project, so measure it for the one you're evaluating. An empty advisory list doesn't mean the code has no vulnerabilities. It may mean nobody has looked, or that fixes ship without announcements.

**Finally, run an automated score.** OpenSSF Scorecard publishes its checks in the [Scorecard checks documentation](https://github.com/ossf/scorecard/blob/main/docs/checks.md). Four apply to security:

- **Security-Policy**: "This check tries to determine if the project has published a security policy."
- **Vulnerabilities**: looks for "open, known unfixed vulnerabilities."
- **Dependency-Update-Tool**: checks whether the project uses a dependency update tool, "specifically dependabot or renovatebot."
- **Branch-Protection**: checks whether the "default and release branches are protected."

The documentation changes between releases, so note which Scorecard version produced your number.

Treat the score as a prompt for questions. It detects the presence of practices, not whether people follow them, and it can't tell a quick fix from a slow one.

Pair it with the advisory timeline and with the maintenance and ownership readings in a [gitdebt report](https://gitdebt.com/report). Those readings come from commit history and don't measure security.

## How do you check open source project health step by step?

You check open source project health by dating the check, reading seven signals from public sources, and scoring each green, amber or red against thresholds you set in advance. Allow about 15 minutes per repository.

1. **Record the check date.** Write today's date (YYYY-MM-DD) at the top of your notes. Every reading below goes stale, and an undated reading can't be compared later.
2. **Read the last commit and last release dates.** Use the repository's Commits page and Releases page on GitHub. A recent commit with no release in over a year means users aren't receiving fixes.
3. **Compare commit volume.** Count commits in the trailing 90 days against the 90 before: `git log --since="90 days ago" --oneline | wc -l`, then add `--until="90 days ago"` and `--since="180 days ago"` for the earlier window. A shallow clone may be too short to cover both windows.
4. **Sample issue and PR response.** Open the 20 most recently created issues and PRs and note how long each waited for a first human reply. A sample of 20 is small, so treat it as a rough reading.
5. **Count contributors.** Run `git shortlog -sn` and count how few names add up to half the commits. Bots and squash-merges can distort the list.
6. **Confirm the files.** Look for `LICENSE`, a governance document and `SECURITY.md`. A file's presence doesn't show anyone follows it.
7. **Run OpenSSF Scorecard** against the repository and record the overall score and any failed checks.

The scorecard below grades each signal. Its thresholds are adjustable conventions, not industry standards.

| Signal | Where to read it | Green | Amber | Red |
|---|---|---|---|---|
| Last commit | Commits page | 30 days or less | 31 to 180 days | Over 180 days |
| Last release | Releases page | 180 days or less | 181 to 365 days | Over 365 days |
| 90-day commits vs prior 90 | `git log` | 80% or more of prior | 40% to 79% | Under 40% |
| First response, sample of 20 | Issues and PRs | Median 7 days or less | 8 to 30 days | Over 30 days or none |
| Contributors writing half the commits | `git shortlog -sn` | 3 or more | 2 | 1 |
| License, governance, security policy | Repo files | All three | Two | One or none |
| OpenSSF Scorecard | Scorecard result | 7 or above | 4 to 6.9 | Below 4 |

A stable library can be finished rather than abandoned, and a quiet 90 days may be a release freeze.

Four open source healthcare projects show how to fill the table. It is a blank template: every cell reads "not checked", and slugs marked (verify) need confirming before you rely on them. Fill it by running steps 1 to 7.

Data capture date: not checked

| Project | Slug | Last commit | Last release | Issues open/closed |
|---|---|---|---|---|
| OpenMRS | `openmrs/openmrs-core` (verify) | not checked | not checked | not checked |
| OpenEMR | `openemr/openemr` (verify) | not checked | not checked | not checked |
| Orthanc | not checked | not checked | not checked | not checked |
| HAPI FHIR | `hapifhir/hapi-fhir` (verify) | not checked | not checked | not checked |

A curated list's "active" label can't stand in for this table, because it comes without criteria. If stars and commits disagree for one of these four, that gap is your finding.

For steps 3 and 5, gitdebt's [documented method](https://gitdebt.com/about) computes maintenance, ownership, repair load and debt markers from a shallow clone of the git history, which saves the manual counting.

## Adopt, contribute or fork: which fits your risk tolerance?

The choice to adopt, contribute or fork depends on what a failure would cost you and how much maintenance you can absorb, not on the score alone. A linter that fails loudly in CI is easy to swap. A library in your login path is not, so it needs a stricter reading.

| Scorecard result | Low tolerance | Medium tolerance | High tolerance | Re-check trigger |
| --- | --- | --- | --- | --- |
| Mostly green | Adopt, pin version | Adopt as is | Adopt as is | Major release |
| Mixed amber | Fork or replace | Adopt and contribute | Adopt as is | Missed release |
| Mostly red | Replace | Fork or replace | Pin and monitor | New advisory |
| Stale, critical | Migrate | Fork | Pin and monitor | Archive notice |

A stale project leaves four options, and each has a cost. Pinning is cheap now, but you own any security fix you miss. Contributing upstream costs review time and may never merge.

Forking gives you control and makes you the maintainer. [GitHub Docs on forks](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/working-with-forks/about-forks) describes a fork as "a separate repository with its own settings and collaboration space." Migrating costs rewrite and testing effort, but it ends the dependency.

Treat an archive notice as a hard trigger. [GitHub Docs on archiving](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) says archiving lets you "make it read-only for all users and indicate that it's no longer actively maintained."

Set a re-check interval and store the check date beside each decision, for example in a dependency log. Pull the maintenance and ownership readings from a [gitdebt report](https://gitdebt.com/report).

Each action suits a different case:

- **Adopt as is:** low-criticality tools with mostly green signals.
- **Adopt and contribute:** amber projects you depend on, where maintainers still answer.
- **Fork or replace:** red results on a critical path.

## Frequently asked questions

### How can I tell if an open source project is still maintained?

Check the date of the last commit and last release, then compare recent commit volume with earlier periods. Look at whether issues and pull requests get replies, whether more than one person merges changes, and whether a security policy exists. One signal alone is not proof; record the date you checked.

### Are GitHub stars a good measure of project health?

Stars are a poor measure of health because they record interest, not upkeep. A popular repository can still have no recent commits, so read stars beside commit activity.

### What is a good bus factor for an open source project?

No universal number applies, so treat the metric as a warning sign, not a pass mark. It infers knowledge concentration from commit history and can't show who is still available.

### How often should I re-check a dependency's health?

A practical convention is to re-check before each major version upgrade and on a fixed schedule, such as quarterly. That's a habit, not a researched rule. Re-check sooner after a maintainer change, a license change or a published vulnerability, and write down each date.

### Does gitdebt detect fake stars?

gitdebt does not detect fake stars. It does not flag stars as fake, real or suspicious, and it doesn't score accounts, as its [methodology page](https://gitdebt.com/about) states.

## What should you check this week, this month and this quarter?

Check your three most critical dependencies this week, record their governance this month, and compare dated results each quarter.

- **This week:** List your direct dependencies. Run the scorecard on the three most critical ones and stamp each result with today's date.
- **This month:** Record license, governance and security policy status for those projects. For any amber or red result, decide whether to adopt, contribute or fork.
- **This quarter:** Re-run the checks and compare them with the previous stamped result. A reading that slid from green to amber tells you more than one snapshot.

Start with your most critical dependency today. Look the repository up on [gitdebt](https://gitdebt.com/) and read the star history beside the four readings taken from its commit history: maintenance, ownership, repair load and debt markers. Every public repository report [works without an account](https://gitdebt.com/about), so you risk nothing by running all three.

## Sources

[GitHub - kakoni/awesome-healthcare](https://github.com/kakoni/awesome-healthcare) · [OpenSSF Scorecard checks documentation](https://github.com/ossf/scorecard/blob/main/docs/checks.md) · [CHAOSS metrics list](https://chaoss.community/kbtopic/all-metrics/) · [ossf/scorecard README](https://github.com/ossf/scorecard) · [Saving repositories with stars - GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) · [Top 23 Healthcare Open-Source Projects - LibHunt](https://www.libhunt.com/topic/healthcare) · [A Novel Approach for Estimating Truck Factors](https://arxiv.org/abs/1604.06766) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about). Every other source is linked where the article cites it.
