---
description: "What does GitHub Insights show on a repo you don't own? Most tabs, but not Traffic. Learn what each view tells you and run a six-step dependency check."
faq:
  - answer: "You can open most of it, but not all of it. Pulse, Contributors and Commits work on a public repository you don't own. Traffic doesn't, because anyone with push access to a repository can view its traffic, so judge the project from the other views."
    question: "Can I see the Insights tab on someone else's public repository?"
  - answer: GitHub Insights is not the same as insights for Projects; they are separate features within GitHub. Insights for Projects builds charts from the items added to a project, and it has nothing whatsoever to do with the repository Insights tab.
    question: Is GitHub Insights the same as insights for Projects?
  - answer: "The Insights tab does not show how many stars a repository gained over time, and it doesn't give a complete star history. Read stars from a dedicated source such as gitdebt, and keep them apart from commits, which show what maintainers did."
    question: Does GitHub Insights show how many stars a repository gained over time?
  - answer: git-insights/git-insights is not made by GitHub. It is a separate third-party analytics tool, and it has its own hosted version online at gitinsights.io. According to its README, it collects commits, pull requests, and issues for your software projects and teams.
    question: Is git-insights/git-insights made by GitHub?
  - answer: "You cannot see who visited a repository you do not own on GitHub. Traffic is limited to people with push access, and it covers visitors from the last 14 days. On someone else's project, read Pulse and the commit history instead."
    question: Can I see who visited a repository I do not own?
publishDate: "2026-10-21"
tags:
  - github insights
  - repository analytics
  - dependency maintenance
  - commit history
  - star history
  - open source
title: "GitHub Insights: what each view shows on repos you don't own"
---

**GitHub Insights has no single meaning: it can be a repository's Insights tab, insights for Projects charts, or a third-party tool such as git-insights/git-insights. To start with the tab, open the repository and click Insights; [Projects charts are built from project data](https://docs.github.com/en/issues/planning-and-tracking-with-projects/viewing-insights-from-your-project/about-insights-for-projects), and [git-insights is its own analytics tool](https://github.com/git-insights/git-insights). The Insights tab describes recent activity, so judging a repository also needs its commit history.**

This is for developers about to add a library from a public repository they don't own. From outside, a repository's Insights view is not the maintainer's view. A star count won't say whether the project is maintained well enough to build on.

After reading, you can interpret each Insights view as an outsider, know which views GitHub withholds from visitors, and run the same short check on any dependency before you commit to it.

> **Key Takeaways**
> - **[Traffic needs push access](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository).** On a public repository you don't own, you can't read its clones or visitors.
> - **Get star history elsewhere.** The Insights tab has no complete star history, so read stars from a dedicated source.
> - **Keep stars and commits apart.** Stars show what the crowd did; commits show what maintainers did.
> - **Every reading is directional.** Each one has a named limit, and none is a verdict.

## What is GitHub Insights? The tab, Projects charts and third-party tools

GitHub Insights usually means the Insights tab on a repository, which [GitHub documents as a set of graphs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/about-repository-graphs) such as Pulse, Contributors, Traffic and Commits. The name also covers insights for Projects charts and third-party tools such as git-insights/git-insights. The three are separate things.

![Image generated](https://api.birchseek.com/m/I5Mwqs653rJzXa_g8ZTmKJpSyUlFp4S_.webp)

**1. The repository Insights tab.** GitHub's repository documentation says that "Repository graphs help you view and analyze data for your repository." This is the tab most readers mean when they search the term.

**2. Insights for Projects.** This feature is attached to a GitHub Project, not to a repository. [GitHub Docs on insights for Projects](https://docs.github.com/en/issues/planning-and-tracking-with-projects/viewing-insights-from-your-project/about-insights-for-projects) says you can use it "to view, create, and customize charts that use the items added to your project as their source data."

It offers two chart types, current charts and historical charts. The docs add that "Insights does not track items you have archived or deleted."

**3. Third-party tools.** These are separate projects, not GitHub features. The [git-insights/git-insights README](https://github.com/git-insights/git-insights) calls it "an open analytics tool to give you insights on your software projects and teams."

It is not run by GitHub.

GitHub documents the repository tab and insights for Projects on separate pages. That is why "GitHub Insights" is not one product.

## How do Pulse, Contributors, Commits and Code frequency work in GitHub Insights?

Pulse, Contributors, Commits and Code frequency are four graphs in a repository's Insights tab, and each counts something narrower than its name suggests.

1. **Pulse: a window, not a history.** GitHub Docs describes [Pulse as a summary of repository activity](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-a-summary-of-repository-activity) for a time period you select. On a repository you don't own, choose the longest period offered and check whether pull requests were merged and issues closed. Pulse can't show what happened before the window, so one quiet period says little.

2. **Contributors: commits, under stated rules.** The Docs page on [viewing a project's contributors](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-a-projects-contributors) says merge commits and empty commits aren't counted. Read it for how many different names appear and whether one name dominates. It can't show who reviewed, triaged, wrote docs or answered questions, because none of that leaves a counted commit.

3. **Commits: weekly volume.** The Docs on [analyzing changes to a repository's content](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/analyzing-changes-to-a-repositorys-content) describe a commit activity graph covering the last year. Look for steady bars versus a flat line that started months ago. It can't tell you whether a commit was a feature, a typo fix or a bot update, or whether anyone reviewed it.

4. **Code frequency: line churn.** The same Docs page says the graph shows content additions and deletions for each week in the repository's history. Read it for the shape: steady small changes, or one huge spike followed by silence. Generated files, vendored code and renames inflate the totals as easily as real work does.

None of the four judges quality. For a commit-based reading of the same history, including how few contributors write half the commits, run the repository through [a gitdebt report](https://gitdebt.com/report). Treat the result as a directional signal, not a verdict.

## Who can see Traffic, Dependency graph, Network and Forks in a repository's Insights?

Any visitor to a public repository can read its Dependency graph, Network and Forks views, but Traffic is limited to people with push access. [GitHub Docs on repository traffic](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) says "Anyone with push access to a repository can view its traffic, including full clones (not fetches), visitors from the last 14 days, referring sites, and popular content in the traffic graph."

1. **Traffic: push access only, 14 days deep.** The same Docs sentence sets the window at the last 14 days, so even owners see only two weeks. Without push access you can't read page views or clones for someone else's project, so don't try to judge it by them.

2. **Dependency graph: what a repository depends on.** [GitHub's dependency graph docs](https://docs.github.com/en/code-security/supply-chain-security/understanding-your-software-supply-chain/about-the-dependency-graph) describe it as "a summary of the manifest and lock files stored in a repository". It lists declared packages, not whether those packages are maintained.

3. **Dependabot alerts: built on the dependency graph.** For [Dependabot alerts](https://docs.github.com/en/code-security/dependabot/dependabot-alerts/about-dependabot-alerts), the Docs say they notify you about vulnerable dependencies. Check that your language's package ecosystem is supported before relying on them.

4. **Network: branch history across copies.** The Docs say the [network graph](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/understanding-connections-between-repositories) "displays the branch history of the entire repository network, including fork branches." It shows where work diverged. It doesn't show which fork is better.

5. **Forks: copies, not users.** The Forks view lists repositories copied from this one. A fork can be made to send a one-line fix or to keep a backup, so a high fork count doesn't measure how many people use the code. Open a few forks to see whether any has its own commits.

### What you can and cannot check on a repo you don't own

You can check:

- Commit cadence in Commits and Code frequency.
- How many people write the code in Contributors.
- The declared dependencies in the dependency graph.
- Whether forks carry unique work in Network.

You cannot check:

- Page views, clones and referring sites (Traffic).
- A complete star history, which the tab doesn't show.

For the commit-based readings beside a star curve, enter `owner/repo` on the [gitdebt report page](https://gitdebt.com/report).

## Does GitHub Insights show star history, and how do you chart it?

GitHub's Insights tab does not show a complete star history, so you chart stars from a separate source. Keep the two sources apart: stars are what the crowd did, and commits are what the maintainers did. gitdebt never blends them.

GitHub Docs, in ["Saving repositories with stars"](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars), says "You can star repositories and topics to keep track of projects you find interesting and discover related content in your news feed."

The [REST documentation for starring](https://docs.github.com/en/rest/activity/starring) describes stars as showing "an approximate level of interest." Its stargazers endpoint "Lists the people that have starred the repository."

Pulse, Commits and the other Insights graphs measure repository activity, not stars over time.

gitdebt is not affiliated with GitHub. It does not flag stars as fake, real or suspicious, and it does not score accounts. It stores star dates and day counts only.

A star-history chart in a README is a plain image URL, so it renders without JavaScript. The snippet below draws facebook/react. Paste it into your README and swap in your own `owner/repo`:

```html
<a href="https://gitdebt.com/facebook/react?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/facebook/react/chart.svg?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/facebook/react/chart.svg?theme=light">
    <img alt="Star history for facebook/react" src="https://api.gitdebt.com/api/repos/facebook/react/chart.svg?theme=light">
  </picture>
</a>
```

The `theme=light` and `theme=dark` sources sit in one `<picture>` element, and the `?ref=readme` link stays around the image. The URL is cacheable. It needs no account, token, API key, build step or GitHub Action.

If nobody has analyzed a repository yet, the URL renders a placeholder and queues the work. The real chart replaces it at the same URL.

Copy the ready-made snippet from the [gitdebt README embeds page](https://gitdebt.com/badges). To see how several repositories grow side by side, open the [frontend frameworks star history comparison](https://gitdebt.com/compare/frontend-frameworks).

## What does the commit history show that GitHub Insights does not?

The commit history shows how a codebase has been maintained, owned and repaired over time, which the Insights tab only partly covers. [gitdebt](https://gitdebt.com/) reads seven signals from it to sit beside that tab. Each has a limit, so read them as directional signals, not verdicts.

1. **Commit trend.** This is monthly commit volume, which shows maintenance as cadence. Cadence is not quality: a busy month can be churn, and a quiet one can be a finished library.
2. **Commit heatmap.** [It covers the last 52 weeks](https://gitdebt.com/about), so gaps and bursts are easy to spot. It counts commits, not how large they were.
3. **File change frequency (churn).** This counts how many analyzed commits touched each file. It identifies frequently revisited files without claiming those changes were unstable or defective.
4. **Fix-labelled changes.** Also called "bug magnets", this lists files that appear in commits whose message matches a fix heuristic. It describes fix-labelled history and does not prove a bug.
5. **Contributors.** This shows who wrote the commits. Authorship misses review, design and support work, which leave no commit.
6. **Ownership concentration.** This shows how few contributors it takes to write half the commits. It shows where knowledge has pooled in a codebase; it is not a judgement about any person.
7. **Debt marker movement.** This tracks whether comments that flag unfinished work are growing or shrinking in recent changes. A marker is a comment someone chose to write, so it reflects habit as much as debt.

[gitdebt's methodology page](https://gitdebt.com/about) states that the readings come from a shallow git clone analyzed server-side, and each signal is a pure, unit-tested function.

## How do you check whether a dependency is still maintained?

Checking whether a dependency is still maintained takes six readings of a public repository you don't own, all from views a visitor can open. Traffic is not one of them.

1. **Open Pulse and pick the longest period it offers.** Look for merged pull requests and new activity. Limit: a quiet window can mean a finished library, not an abandoned one.
2. **Open Contributors and Commits.** Check whether activity is spread across months or stops at a date. Limit: one burst from a single author is a thin sign of life.
3. **Open Dependency graph.** Read what the project itself depends on, and note any Dependabot activity if it's visible. Limit: it lists declared dependencies, not whether anyone reviews them.
4. **Read the direction of the commit trend.** Compare recent months against the months before them: rising, flat or falling. Limit: cadence is not quality, and a small commit isn't a fix.
5. **Check ownership and repair load.** Note how few contributors write half the commits and how much change is fix-labelled. Limit: these are directional context, and ownership shows where knowledge has pooled, not a judgement about any person.
6. **Set stars aside.** They record popularity, what the crowd did, not what maintainers are doing.

For a worked check on `apache/airflow`, read Pulse and Contributors on github.com/apache/airflow, then open its [gitdebt report for apache/airflow](https://gitdebt.com/apache/airflow). Compare the maintenance reading with the star curve.

A rising curve beside a falling commit trend is the disagreement to look for. No numbers are quoted for this example, because they change daily.

If Pulse, commits and the maintenance reading all point down, look for an active fork or an alternative. Read the open issues for maintainer replies. That tells you where to look next; it isn't a verdict on the project.

## Frequently asked questions

### Can I see the Insights tab on someone else's public repository?

You can open most of it, but not all of it. Pulse, Contributors and Commits work on a public repository you don't own. Traffic doesn't, because [GitHub Docs on traffic](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) says "Anyone with push access to a repository can view its traffic", so judge the project from the other views.

### Is GitHub Insights the same as insights for Projects?

No, they are separate features. [Insights for Projects](https://docs.github.com/en/issues/planning-and-tracking-with-projects/viewing-insights-from-your-project/about-insights-for-projects) builds charts from the items added to a project, and it has nothing to do with the repository Insights tab.

### Does GitHub Insights show how many stars a repository gained over time?

No, the Insights tab doesn't give a complete star history. Read stars from a dedicated source such as [gitdebt](https://gitdebt.com/), and keep them apart from commits, which show what maintainers did.

### Is git-insights/git-insights made by GitHub?

No, it's [a separate third-party analytics tool](https://github.com/git-insights/git-insights) with its own hosted version at gitinsights.io. Its README says it collects commits, pull requests and issues for your software projects and teams.

### Can I see who visited a repository I do not own?

No. Traffic is limited to people with push access, and it covers visitors from the last 14 days. On someone else's project, read Pulse and the commit history instead.

## Next steps: check one dependency today

- **Today:** pick one dependency you rely on and run the six-step maintenance check on it. Read its maintenance and ownership readings, then check the dates of its latest commits.
- **This week:** read the commit-history readings for the dependencies you rely on most. Note which have slowed and which depend on one or two authors.
- **Ongoing:** if you maintain a project, add a star-history embed to your README from the [badges page](https://gitdebt.com/badges).

Stars say a repository is popular. Its commits say whether it is alive. A useful reading of GitHub Insights covers both.

Start with one repository: [analyze any public repository at the gitdebt report page](https://gitdebt.com/report). [Every public repository report works without an account](https://gitdebt.com/about), and gitdebt is [MIT-licensed](https://github.com/zhom/gitdebt).

## Sources

[About insights for Projects - GitHub Docs](https://docs.github.com/en/issues/planning-and-tracking-with-projects/viewing-insights-from-your-project/about-insights-for-projects) · [GitHub - git-insights/git-insights: Git Insights is an…](https://github.com/git-insights/git-insights) · [Viewing traffic to a repository - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) · [About repository graphs - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/about-repository-graphs) · [Viewing a summary of repository activity - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-a-summary-of-repository-activity) · [Analyzing changes to a repository's content - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/analyzing-changes-to-a-repositorys-content) · [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about). Every other source is linked where the article cites it.
