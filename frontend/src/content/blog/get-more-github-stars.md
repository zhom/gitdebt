---
description: "How do you get more GitHub stars? Write a README that converts on the first screen, launch by each channel's rules, and track which one worked. Start now."
faq:
  - answer: "Make the README state what the project does and how to run it on the first screen. Then share it where each community's rules allow, such as Show HN with something people can try. Check Insights, Traffic to see what worked, and skip bought stars."
    question: What is the fastest way to get more GitHub stars?
  - answer: "Sending the link to people who might use the project is ordinary outreach, as long as they are free to say no. Paying for stars is a different act entirely. Read each channel's posting rules before you share anywhere public."
    question: Is it okay to ask friends to star my repository?
  - answer: "Don't count on it. The policy and the StarScout research are covered in the buying section above, and this page makes no claim about how many fake stars get removed or how fast. Skip bought stars instead of waiting to find out."
    question: Does GitHub remove fake stars?
  - answer: gitdebt does not judge them. It does not flag stars as fake, real or suspicious, and it does not score accounts. Open your report to read the star-history curve beside the commit history, then match spikes to your launch events.
    question: Does gitdebt tell me whether my stars are real?
  - answer: "There's no number to give. GitHub does not document the Trending ranking method in its public docs, so any threshold you read elsewhere is a guess. Watch your own referrers and star-history curve to see whether a launch moved the needle."
    question: How many stars do I need to reach GitHub Trending?
publishDate: "2026-10-24"
tags:
  - github stars
  - readme
  - show hn
  - github trending
  - open source
  - repository maintenance
title: "Get more GitHub stars: README, launch rules and tracking"
---

**To get more GitHub stars that last, write a README that converts a visitor on the first screen, launch on each channel by its own rules, check which channel produced the stars, and keep the repository visibly maintained with fast issue replies and regular releases. Skip buying stars: they add a number to the page, not users to the project.**

This is for maintainers who shipped a project, shared the link and watched the star count barely move. The fix starts with finding out why.

By the end you'll have a README checklist, a launch order that follows each channel's rules, a way to match stars to the channel that earned them, and a check to run after launch.

> **Key Takeaways**
> - A launch spike is attention, not health: read stars and commits as two independent sources, never blended.
> - Compare your first 90 days with peers in the same [compare category](https://gitdebt.com/compare), aligned by days since first star.
> - Treat any Trending threshold as unsourced unless GitHub documents its method.
> - Asking people to look at your project isn't buying stars.

## How do you write a README that gets more GitHub stars?

A README that earns stars tells a visitor what the project is, who it's for and why it differs before they scroll, then proves it runs. [GitHub surfaces the README](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes) to every visitor, so its first lines carry the most weight.

![Image generated](https://api.birchseek.com/m/mnJG4lVahpYJH67laUaxCMGtiEezx6BE.webp)

1. **Write the first screen.** In the opening lines, state what the project is (one sentence), who it's for (a named role or problem) and why it differs from the closest alternative. The usual mistake is a logo and a row of badges that push all three below the fold.
2. **Run the quick start as written.** Paste your install and first-run commands into a clean container or fresh machine. Success is a visible result with no step you had to guess. Commands that only work because of tools on your laptop are the usual failure.
3. **Add topics.** [GitHub's topics documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/classifying-your-repository-with-topics) says: "To help other people find and contribute to your project, you can add topics to your repository related to your project's purpose, subject area, affinity groups, or other important qualities." Use the gear icon beside About on the repository page.
4. **Upload a social preview image.** [GitHub's social preview documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/customizing-your-repositorys-social-media-preview) says: "Until you add an image, repository links expand to show basic information about the repository and the owner's avatar." Set the image under Settings → Social preview so shared links look deliberate.
5. **Check which README GitHub shows.** GitHub looks in three folders, in a fixed order.
6. **Embed a star-history chart.** Copy the snippet below and swap `OWNER/REPO` for your repository.

### Example: confirm which README GitHub shows

GitHub Docs says that if you put your README in your repository's hidden `.github`, root, or `docs` directory, GitHub "will recognize and automatically surface your README to visitors." If more than one exists, the docs say the order is the `.github` directory, then the root, then `docs`.

Open your repository and search for `README.md` in each location. A forgotten copy in `.github` will hide the one you just rewrote.

### The chart snippet

gitdebt serves each chart as a plain public image URL, so it renders in a README without JavaScript, an account, a token or a build step. The [README badges and embeds page](https://gitdebt.com/badges) lists every snippet.

The `theme=light` and `theme=dark` variants go in one `<picture>` element, and the `?ref=readme` link stays around the image.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
    <img alt="Star history chart" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg">
  </picture>
</a>
```

A chart in the README displays the count. It doesn't cause stars.

## Which launch channels get GitHub stars, and what are their rules?

A GitHub launch sequence runs through four channels, and each one publishes rules you should read before you post. The order below is a suggestion, not a schedule. Another project's reported timeline reflects its own conditions, so don't copy it.

1. **Awesome lists.** Start here, because maintainers review submissions by hand. Open the list's `CONTRIBUTING` file, check the inclusion criteria against your project, and match the entry format exactly. Submit only to lists where your project fits the stated scope.
2. **Show HN.** The [Show HN guidelines](https://news.ycombinator.com/showhn.html) open with a definition: "Show HN is for something you've made that other people can play with." Use that sentence as your gate: if a visitor can't run it or try it, it doesn't belong in a Show HN. Plan to stay in the thread and answer questions.
3. **Your target subreddit.** Rules differ by community, so read the subreddit's own rules page before you write a word. Then read Reddit's Reddiquette page for its self-promotion guidance. If a rule is unclear, message the moderators first.
4. **Product Hunt.** Read Product Hunt's launch guide and prepare what it asks makers to provide before you schedule anything. Treat it as a checklist.

### A concrete check before posting to Show HN

Open the Show HN guidelines in one tab and your README in another, then confirm three things:

- A stranger can run the tool from your quick start in a few minutes, without signing up for anything.
- The link you submit is the repository or a working demo, not a blog post about it.
- You'll be free for the next few hours to reply to comments.

If any of these fails, fix it first. The guidelines page is short enough to reread each time you submit.

### After each channel

Space the launches out so you can tell which one produced stars. Then open your repository's [gitdebt report](https://gitdebt.com/report) and match each jump in the curve to the day you posted.

## How does GitHub Trending work?

GitHub Trending is the page at `github.com/trending` that surfaces repositories attracting attention, and GitHub's public documentation does not publish its ranking method. Any exact star threshold or time window you read elsewhere is a guess, so measure your own results instead.

GitHub does not document the ranking formula, a minimum star count, or the time window the page measures. An article that states those numbers without linking a GitHub page is guessing.

Skip the target number. Aim for real attention from the channels above, then check the outcome in your own data. Our [guide to GitHub Trending](https://gitdebt.com/blog/github-trending) covers how to read the list.

The [gitdebt leaderboard of star velocity](https://gitdebt.com/leaderboard) shows stars gained over 1, 7 and 30 days for the repositories gitdebt tracks. It covers only repositories gitdebt has analyzed, and it is not a copy of GitHub's list, but it shows how your gain compares.

## How do you tell which channel gave you GitHub stars?

You can tell which channel gave you GitHub stars by reading two sources together: the referrer list on GitHub's traffic page and the shape of your star-history curve. Neither one names who starred, so treat the result as evidence, not proof.

1. **Open Insights, then Traffic, and read the referring sites.** [GitHub's traffic documentation](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) says that anyone with push access can view traffic, including "visitors from the last 14 days, referring sites, and popular content in the Traffic graph."

 Because the window is 14 days, copy the referrer list into a note soon after each launch. A launch from last month has already scrolled out of view. Success looks like a ranked list of sites you can match to the channels you used.

2. **Open your star history in the gitdebt report and read the shape.** gitdebt charts GitHub's own daily counts, net of unstars, and needs no sign-in or repository connection, as [how gitdebt works](https://gitdebt.com/about) describes.

 A sharp spike points to a dated event. A steady slope points to search or word of mouth. A plateau means a launch that did not travel. Match each spike date to your launch dates, then to the referrers from step 1.

3. **Switch the axis to days since first star and compare your early weeks with peers.** [The compare page](https://gitdebt.com/compare) can plot by days since each repository's first star, so a 2013 project and a 2024 project start from the same day. Use the repositories in your [compare category](https://gitdebt.com/compare), not a random set.

For example, pick peers such as `facebook/react` and `vuejs/core` from the [frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks) and plot them with your repository on days since first star. Read the shape of each curve, not a live number. One may climb in a single jump while another rises as a slope.

Two limits apply. Referrers show visits, not who starred, so a visit from a site doesn't prove a star came from it.

A spike that lands near two events, such as a Show HN post and a Reddit post a few hours apart, can't be split cleanly.

To run the check on your own repository, [analyze it in a gitdebt report](https://gitdebt.com/report) and open the star history.

## Should you buy GitHub stars?

Buying GitHub stars breaks GitHub's rules, while asking people who may find your project useful to look at it does not. Sending a link to a colleague or posting in a relevant forum is outreach, and the reader decides whether to star.

Paying for stars produces a count with no such decision behind it. [GitHub's Acceptable Use Policies](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies) list "rank abuse, such as automated starring or following" under their rules on inauthentic activity.

Researchers have measured the practice too. [The StarScout paper](https://arxiv.org/abs/2412.13459) reports "suspected fake stars", a label produced by its own detection heuristics. It is an estimate with a method's limits, not a ruling from GitHub.

As its [methodology page](https://gitdebt.com/about) states, gitdebt does not flag stars as fake, real or suspicious, and it doesn't score accounts. That can't be done correctly without privileged access to GitHub's full dataset. Our [article on fake GitHub stars](https://gitdebt.com/blog/fake-github-stars) covers the topic in depth.

## How do you keep a repository looking alive after the star spike?

You keep a repository looking alive after a star spike by showing steady maintainer activity, because stars record what the crowd did and commits record what the maintainers did.

Read the two as separate sources and never blend them. The [apache/airflow report](https://gitdebt.com/apache/airflow) puts the star curve beside commit-based charts such as ownership concentration and fix-labelled changes.

1. **Read the commit trend.** In your report, check whether monthly commit volume is rising, flat or falling. It comes from the git history, not from stars, and it's a directional signal, not a verdict: it can't say why commit volume changed.
2. **Answer every new issue.** A reply like "reproduced, looking into it" tells a visitor someone is there. Don't close issues silently, because an unanswered list is the first thing a skeptical visitor sees.
3. **Ship tagged releases.** [GitHub's releases documentation](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases) says "Releases are deployable software iterations you can package and make available for a wider audience to download and use." Tag each version and write short release notes.
4. **Publish `CONTRIBUTING.md`.** [GitHub's contributor guidelines documentation](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/setting-guidelines-for-repository-contributors) says you can add contribution guidelines to your repository's root, docs, or.github folder. Cover how to run the tests and how to open a pull request.
5. **Apply `good first issue` labels** to small, well-scoped tasks, following GitHub's documentation on encouraging helpful contributions with labels. A vague task with that label sends newcomers away, so state the file and the expected outcome.

Skip a public roadmap unless you'll keep it current. If you add one, read GitHub's current Projects documentation first and confirm how visibility works.

Success after about a month looks like this: new issues have replies, the latest release tag is recent, and the commit trend is flat or rising.

## Frequently asked questions

### What is the fastest way to get more GitHub stars?

Make the README state what the project does and how to run it on the first screen. Then share it where each community's rules allow, such as Show HN with something people can try. Check Insights → Traffic to see what worked, and skip bought stars.

### Is it okay to ask friends to star my repository?

Sending the link to people who might use the project is ordinary outreach, as long as they're free to say no. Paying for stars is a different act. Read each channel's posting rules before you share anywhere public.

### Does GitHub remove fake stars?

Don't count on it. GitHub's Acceptable Use Policies list automated starring as rank abuse, but how many fake stars get removed, and how fast, is not published. Skip bought stars instead of waiting to find out.

### Does gitdebt tell me whether my stars are real?

gitdebt doesn't judge them. It does not flag stars as fake, real or suspicious, and it doesn't score accounts. Open your [report](https://gitdebt.com/report) to read the star-history curve beside the commit history, then match spikes to your launch events.

### How many stars do I need to reach GitHub Trending?

There's no number to give. GitHub does not document the Trending ranking method in its public docs, so any threshold you read elsewhere is a guess. Watch your own referrers and star-history curve to see whether a launch moved the needle.

## Next steps: what to do today, this week and this month

- **Today:** Run the README checklist. The first screen should say what the project does and who it's for, and the quick start should run. Then add topics and a social preview image.
- **This week:** Read the rules for one launch channel and post once. Note the date so you can match it to the star curve later.
- **This month:** Compare Insights → Traffic referrers with your star-history curve, check your first 90 days against peers, and read the maintenance signal. It's directional, not a verdict.

To get more GitHub stars that last, measure before you repeat. Open your repository's report at [gitdebt.com/report](https://gitdebt.com/report), then copy the star-history embed into your README.

As the [README badges page](https://gitdebt.com/badges) explains, a repository nobody has analyzed yet shows a placeholder and queues the work. The real chart replaces it at the same URL. gitdebt is [MIT-licensed](https://github.com/zhom/gitdebt), and [every public repository report works without an account](https://gitdebt.com/about).

## Sources

[Classifying your repository with topics - GitHub Docs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/classifying-your-repository-with-topics) · [Customizing your repository's social media preview - GitHub…](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/customizing-your-repositorys-social-media-preview) · [About READMEs - GitHub Docs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes) · [README badges, star-history charts and embeds](https://gitdebt.com/badges) · [Show HN Guidelines](https://news.ycombinator.com/showhn.html) · [Viewing traffic to a repository - GitHub Docs](https://docs.github.com/en/repositories/viewing-activity-and-data-for-your-repository/viewing-traffic-to-a-repository) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [GitHub Acceptable Use Policies](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies). Every other source is linked where the article cites it.
