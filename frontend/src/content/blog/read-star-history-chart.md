---
description: Star history chart hard to read? Read slope (stars per day), not total. Spikes, steps, plateaus signal events; log scale compares size. Learn to vet repos.
faq:
  - answer: Read the slope rather than the total star count. Steep segments are bursts of attention, such as a launch or a front-page post. Flat segments mean few new stars arrived, not that the project is finished. Use log scale to compare sizes, and check commits for health.
    question: How do I read a star history chart?
  - answer: A sudden spike usually follows a dated event, such as a launch, a post, or a mention by a larger account. The chart shows when the stars arrived, not why. Match the date to your announcements and traffic referrers to find the cause.
    question: Why does my star history chart have a sudden spike?
  - answer: No, a flat star history chart shows that few new stars arrived, not that work stopped. A stable library can be quiet and still be maintained. Open the commit trend and release dates to tell a finished project from an abandoned one.
    question: Does a flat star history chart mean a project is dead?
  - answer: Use log scale to compare growth rates, or to put a mature and a young repository on one chart. Use linear scale for absolute counts. On log scale, equal vertical steps mean equal percentage growth. The choice depends on whether you want to compare rates or totals.
    question: Should I use log scale or linear scale?
  - answer: "A star history chart can only hint at fake stars, not prove them. A very steep jump on a young repository deserves a second look, but the curve can't show who starred or why. gitdebt doesn't flag stars as fake, real or suspicious, and it doesn't score accounts."
    question: Can a star history chart show fake stars?
publishDate: "2026-10-15"
tags:
  - star-history
  - github-stars
  - repo-maintenance
  - log-scale
  - spikes
  - plateaus
title: "How to read a star history chart: spikes, plateaus and log scale"
---

**Read a star history chart by its slope, stars gained per day, rather than its total. A spike is a burst of attention, a step is a dated event, a plateau means few new stars (not a verdict), and a straight line is a constant rate. Use log scale to compare repositories of different size, and check commits, because stars record popularity and commits record maintenance.**

A star history chart that climbs almost vertically for a week, or sits flat for two years, says less than it appears to. Maintainers read their own curve after a launch. Developers read one to vet a dependency, and DevRel teams and analysts read two side by side.

After reading, you can name the pattern on a curve, pick the scale and alignment that fit your question, and say what the curve can't show.

> **Key Takeaways**
> - A finished project and an abandoned one draw the same flat curve. Only commits and releases separate them.
> - A near-vertical jump on a young repository is a reason to check releases and commits, not a verdict.
> - On a log axis, parallel lines mean equal proportional growth. Confirm any claimed lead on the linear view.
> - Align by days since first star to compare launches, and by calendar date to compare the same market period.

## What does a star history chart actually plot?

A star history chart plots a repository's cumulative star count over time, so the slope of the line is stars gained per day. Steep stretches mean fast gains. Flat stretches mean few new stars. The line says nothing about commits.

![Image generated](https://api.birchseek.com/m/ar0Pti4hPVjmSZrp-r9U4UJUGHvrHOSA.webp)

A star is a single GitHub action. [GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) says "Starring makes it easy to find a repository or topic again later" and that "Starring a repository also shows appreciation to the repository maintainer for their work."

A star is a bookmark and a thank-you. Neither measures use or quality.

gitdebt reads its series from GitHub's own star history. Its [methodology page](https://gitdebt.com/about) describes exact daily counts, net of unstars, back to the repository's creation, re-read on a schedule.

Older series built from historical data are re-read from GitHub's star history as gitdebt reaches them. Every chart names its source and coverage date, so check both before you read a stretch of the curve.

### Read one repository twice

Open the `facebook/react` report and read its curve twice: once as a total, once as a slope. The steps describe the method; no live numbers are quoted.

1. **As a total.** Look at the height of the line at the right edge. It shows that the repository has collected many stars over its life. It doesn't show when.
2. **As a slope.** Pick one steep stretch and one flat stretch. Divide each stretch's rise by its number of days.

With made-up figures, a stretch that climbs 600 stars over 30 days gains 20 stars a day. A flat stretch that adds 30 stars over the same 30 days gains 1 a day. The arithmetic is the same for any real stretch.

The total shows how big the pile is. The slope shows how fast it is growing now.

## What do spikes, steps and plateaus on a star history chart mean?

Spikes, steps and plateaus on a star history chart each point to a kind of event, but the curve alone can't confirm which one. Read the shape first, then check the dated record beside it.

**1. Launch spike and decay.** The line climbs sharply for a few days, then bends back toward a gentler slope. Possible causes include a Hacker News or Reddit front-page post, a newsletter mention, GitHub Trending or a Product Hunt launch.

The curve can't say whether any of those happened. Find the spike's date, search for posts from that week, and treat the slope after the decay as the project's baseline rate.

**2. Step change.** A near-vertical jump is followed by a return to roughly the old slope. A release, a viral post or a listing on a popular index can draw that shape.

Match the date of the jump to the repository's releases page and to dated posts that link to it.

**3. Long plateau.** The line runs nearly flat for months. A finished project and an abandoned one look identical in stars. Open the commit trend and the maintenance reading, described in the commit history section, before you decide anything.

**4. Steady linear versus accelerating growth.** A straight line on linear axes means a roughly constant number of new stars per day. An accelerating curve steepens because the daily rate is rising.

To put a number on the change, compare the average daily gain over the last 90 days with the 90 days before.

**5. Very steep jump on a young repository.** A young repository has little history to set a jump against, so a near-vertical start deserves a second look. Check whether releases, issues, contributors and commits match the attention. The curve itself shows nothing about who starred.

### Two cautions when comparing repositories

Star velocity is spike-sensitive. The [gitdebt star velocity leaderboard](https://gitdebt.com/leaderboard) ranks repositories by weekly pace, so a high position can come from a single spike. Open the repository's curve to see whether the gain is one step or a steady rate.

Curves can also dip, because the series is [net of unstars](https://gitdebt.com/about). A small dip is not a collapse. It means that period's net count fell.

## When should you switch a star history chart to log scale?

Switch a star history chart to log scale when your question is about proportional growth, or when a mature repository and a young one share the same axes. Stay on linear scale when the question is about absolute numbers.

The scale is a reading option: switching between linear and log changes the axis, not the underlying daily series.

The decision takes about two minutes:

1. **Write down the question first.** "How many more stars per day is it gaining?" needs linear, because vertical distance equals stars. "How fast is it growing in proportion to its size?" needs log. A lead measured in raw stars is a linear question.
2. **Look for a flattened floor.** In an overlay, check the smaller lines. If the largest repository (say `facebook/react` against a project one year old) presses every other line flat against the bottom, switch to log. Every line should then be readable, each with a visible slope.
3. **Re-read each pattern.** Spikes compress, so a launch spike that dominated the linear view shrinks to a modest bend. A plateau on a mature repository looks flatter still, because a few hundred stars barely move a large total. Early growth looks steeper: 100 to 1,000 stars is the same multiple, 10x, as 10,000 to 100,000.
4. **Read vertical distance as a multiple.** On a log axis, equal vertical distance means the same multiple, not the same number of stars. Parallel lines grow at the same proportional rate, even if one gains thousands more stars a day.
5. **Switch back before you state a gap.** Log scale makes small early gains look large and flattens big absolute gaps. A repository that looks close behind its rival on log scale can still trail by tens of thousands of stars, so confirm any claimed lead on the linear view.

## How do you compare several repositories on one curve fairly?

You compare several repositories fairly by overlaying them on one set of axes, choosing the alignment that matches your question, and rebasing the chart to a date window. It takes about five minutes on the [gitdebt compare page](https://gitdebt.com/compare).

1. **Add the repositories.** Enter the `owner/repo` slugs. The compare page overlays up to eight on one set of axes, so every line shares the same scale. You should see one chart with a legend entry per repository.
2. **Pick the alignment.** Calendar date answers "what did each project do in the same market period?" Days since first star answers "how did each grow from launch?" Calendar alignment puts a mature project's current curve beside a new project's launch, two different life stages, so use days since first star for that pairing.
3. **Set the scale.** If the repositories differ by orders of magnitude, switch to log scale. Stay on linear when you want absolute gaps.
4. **Set a date window.** Enter a from date and a to date. gitdebt's date windows are inclusive at both ends, according to its [source repository on GitHub](https://github.com/zhom/gitdebt). Rebasing to zero starts each line at zero on the from date, so you compare gains inside the window rather than lifetime totals.
5. **Read slopes, then check commits.** Compare steepness, not finish position. A steep line says attention arrived, not that the project is maintained, so open each report's commit trend before you decide.

A frequent misreading is comparing end heights without checking that the lines start together. The date window in step 4 fixes that.

**A worked example.** Open the [frontend frameworks field](https://gitdebt.com/compare/frontend-frameworks), which includes `facebook/react` and `angular/angular`. On calendar date, the older repositories look like long, gentle climbs.

Switch the alignment to days since first star and every line starts at the same origin. Then narrow the window to the first 90 days.

The chart now compares launches rather than eras. It shows which repository gained stars fastest right after release and which started slowly. It doesn't say why one took off, so read the dated events beside each curve before drawing a conclusion.

[Open the frontend frameworks comparison](https://gitdebt.com/compare/frontend-frameworks) and run these steps on it, or replace the field with your own eight repositories.

To put one of these charts in a README, use the [README badges and star history embeds](https://gitdebt.com/badges).

## What can stars not tell you without the commit history?

Stars can't tell you whether anyone is maintaining a repository, because a star records what the crowd did and a commit records what the maintainers did. gitdebt treats them as two independent sources and never blends them into one score.

Stars can also be inflated. A [2024 arXiv paper](https://arxiv.org/abs/2412.13459) by Hao He and co-authors, titled "4.5 Million (Suspected) Fake Stars in GitHub: A Growing Spiral of Popularity Contests, Scams, and Malware", studies suspected fake stars at scale.

The word "suspected" in that title matters. Detection of this kind is an estimate, not proof about any one repository.

[gitdebt's methodology](https://gitdebt.com/about) is explicit on this point: gitdebt does not flag stars as fake, real or suspicious, and it doesn't score accounts. It stores star dates and day counts only, never stargazer profiles.

A sudden step on a curve is a reason to check the dated record beside it, not a finding.

Three readings that stars cannot provide come from the commit history:

- **Maintenance:** whether recent commits are speeding up, holding steady, slowing down or have gone quiet.
- **Ownership:** how few contributors write half the commits. Ownership shows where knowledge has pooled in a codebase; it is not a judgement about any person.
- **Repair load:** the fix-labelled share of changes.

These are directional signals, not verdicts. A falling maintenance reading is a prompt to read the commit trend, not a reason to drop a dependency.

The two sources can disagree. A popular repository can have fading maintenance, and a flat one can be actively maintained.

The [apache/airflow report](https://gitdebt.com/apache/airflow) shows both on one page: the star curve next to the maintenance cadence, ownership concentration and fix-labelled change charts. Compare the shape of the curve with the commit trend. Numbers change daily, so read the live page.

Sometimes stars are the wrong metric altogether. Pair the curve with three other counts:

- **Forks:** copies of the repository that people took to build on or change.
- **Contributors:** the people whose commits actually landed.
- **Release cadence:** how often tagged versions ship.

A small, finished library can have a flat curve and few releases and still be fine to depend on. Stars alone can't settle that. Commits and releases can.

## Frequently asked questions

### How do I read a star history chart?

Measure the slope of a stretch, stars gained divided by days, instead of reading the height at the right edge. Steep segments are bursts of attention, such as a launch or a front-page post, and flat segments mean few new stars. Check the commit history for maintenance.

### Why does my star history chart have a sudden spike?

A spike usually follows a dated event, such as a launch, a post, or a mention by a larger account. The chart shows when the stars arrived, not why. Match the date to your announcements and traffic referrers.

### Does a flat star history chart mean a project is dead?

No. A flat line shows that few new stars arrived, not that work stopped, and a stable library can be quiet and still maintained. Open the commit trend and release dates to tell a finished project from an abandoned one.

### Should I use log scale or linear scale?

Use log scale to compare growth rates or to put a mature and a young repository on one chart, and linear scale for absolute counts. On log scale, equal vertical steps mean equal percentage growth.

### Can a star history chart show fake stars?

Only as a hint, never as proof, because the curve can't show who starred or why. A very steep jump on a young repository deserves a second look. gitdebt doesn't flag stars as fake, real or suspicious, and it doesn't score accounts.

## Next steps: read the curve, then check the commits

**Today:** [analyze a repository](https://gitdebt.com/report) and find the steepest and flattest stretch of its star history chart. Name the pattern: launch spike, step, plateau or steady line. Note the dates, then check what happened around them.

**This week:** overlay that repository with two or three comparable ones, aligned by days since first star so a mature project and a young one start from the same point. Switch to log scale if their sizes differ widely.

**Ongoing:** before you judge a dependency, read the commit-based health readings beside the curve: maintenance, ownership, repair load and debt markers. They come from a different source than stars.

Stars say a repository is popular. Commits say whether it is alive.

Look up any public repository at [gitdebt.com](https://gitdebt.com/). It's free and MIT-licensed, and no sign-in is needed. To put your own chart in a README, copy a light and dark pair from the [README star history snippets page](https://gitdebt.com/badges).

## Sources

[Saving repositories with stars - GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [Compare GitHub star history](https://gitdebt.com/compare) · [zhom/gitdebt](https://gitdebt.com/badges) · [4.5 Million (Suspected) Fake Stars in GitHub: A Growing…](https://arxiv.org/abs/2412.13459) · [apache/airflow report](https://gitdebt.com/apache/airflow)
