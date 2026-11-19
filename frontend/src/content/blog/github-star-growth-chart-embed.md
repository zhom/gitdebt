---
description: How do you add a GitHub star growth chart embed to a README? Paste one img tag with a public SVG URL, no token, iframe or script needed. Get the snippet.
faq:
  - answer: "Add an img tag that points to https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg, with OWNER/REPO replaced by your repository. GitHub renders images in READMEs, so no iframe, token or build step is needed. The full snippet is on the badges page. Commit the README after pasting the tag."
    question: How do I embed a GitHub star growth chart in my README?
  - answer: "You don't need any of them, because the chart is a public image URL that renders without JavaScript. An iframe leaves a blank space in a README. Paste the img tag, commit the README, and nothing else in your repository changes."
    question: Can I use an iframe, a token or a GitHub Action instead?
  - answer: "Wrap two images in a picture element: one URL with theme=light and one with theme=dark. Each viewer's GitHub theme picks the matching drawing, so viewers on the light theme see the theme=light image and viewers on the dark theme see the theme=dark one. The element only switches between the variants you publish, so include both."
    question: How do I make the star chart match GitHub dark mode?
  - answer: "Your star chart shows a placeholder because your repository hasn't been analyzed yet. gitdebt queues the work, and the real chart replaces the placeholder at the same URL, so the img tag in your README stays unchanged. The wait varies, so reload the README page later."
    question: Why does my star chart show a placeholder?
publishDate: "2026-11-19"
tags:
  - github readme
  - star history
  - svg embed
  - dark mode
  - github stars
title: "GitHub star growth chart embed: One img tag, no token needed"
---

**A github star growth chart embed is a plain public image URL inside an `img` tag in your README, with no iframe or script. A light and dark pair goes in a `picture` element.**

Maintainers who want a GitHub star growth chart embed in a README often find setups that ask for a token, an iframe or a build step. A README displays images, so the chart only has to be one.

After reading, you can paste one snippet, match the chart to each viewer's light or dark GitHub theme, fix a chart that doesn't render, and read the curve without claiming more than it shows.

> **Key Takeaways**
> - Stars measure attention. Commit history is a separate source about maintenance, so read the two side by side without blending them.
> - Compare repositories on days since each one's first star, so the older repository doesn't win by default.
> - Switch to a log scale when one repository is far larger than the other.
> - A chart that looks stale may be GitHub's image cache, which you can't clear.

## How do you embed a GitHub star growth chart in a README?

A GitHub star growth chart embed goes into a README as an HTML `img` tag that points at a public SVG URL, wrapped in a link back to the repository's report. Because the chart is an image, it needs no JavaScript.

![Illustration for How do you embed a GitHub star growth chart in a README?](https://api.birchseek.com/m/_FJbzvMK4GxBytnW1bDcHc0JDAJp2_JT.webp)

1. Paste the snippet into your `README.md`, then replace `vercel/next.js` (the OWNER/REPO part) with your own slug.

```html
<a href="https://gitdebt.com/vercel/next.js?ref=readme">
  <img src="https://api.gitdebt.com/api/repos/vercel/next.js/chart.svg" alt="Star history of vercel/next.js" width="720">
</a>
```

Keep the `?ref=readme` link around the image. Then open the README preview to check that the star history appears.

2. Set the parameters. Add `theme=light` or `theme=dark` to the image URL to pick a drawing. The SVG is static by default, and `animate=1` opts it into motion. For an animated GIF, swap `chart.svg` for `chart.gif`.

3. Serve both themes. In the snippet, the `source` element handles dark mode, and the `img` is the light fallback.

```html
<a href="https://gitdebt.com/vercel/next.js?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/vercel/next.js/chart.svg?theme=dark">
    <img src="https://api.gitdebt.com/api/repos/vercel/next.js/chart.svg?theme=light" alt="Star history of vercel/next.js" width="720">
  </picture>
</a>
```

The markup contains no star count. The README never carries a number you have to update by hand.

Grab the ready-made markup from the [gitdebt badges page](https://gitdebt.com/badges). If your repository has no chart yet, [queue a report](https://gitdebt.com/report) for it.

## Why does a GitHub star growth chart embed need an image, not an iframe?

A README shows a chart image but not an iframe because GitHub renders Markdown image syntax and does not render iframes. An `img` tag that points at a public image URL displays, while an iframe embed leaves a blank space.

GitHub's [basic writing and formatting syntax page](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) describes the image route: "You can display an image by adding! and wrapping the alt text in [ ]." A README treats a chart the same way it treats a screenshot or a logo.

A chart tool therefore has to hand back an image URL. Some tools take a different route. The [dev.to walkthrough of a self-hosted star history app](https://dev.to/xuanhun/how-to-add-a-live-star-history-chart-to-your-github-readme-15hk) says its data query and image download features need you to "supply your GitHub personal access token." That setup ties the chart to a credential you have to create and keep.

gitdebt returns the image directly. The URL is public and cacheable, so any viewer can load it without running a script.

The minimal form of the embed is a single tag:

```html
<img src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg" alt="Star history for OWNER/REPO">
```

An embed method that depends on an iframe or a script tag will not display in a README.

## Where does star history data come from, and what does the curve count?

Older series built from GitHub's stargazer list or historical data are re-read from GitHub's star history as gitdebt reaches them.

GitHub's REST API exposes the same raw material. Its [starring endpoint](https://docs.github.com/en/rest/activity/starring) lists the people who starred a repository, and the `application/vnd.github.star+json` media type adds a timestamp for each star, the `starred_at` field. Counting those timestamps per day produces a history.

That list comes back in pages. GitHub's [pagination docs](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api) say that when a response would include many results, GitHub paginates them and returns a subset. Reading a large repository in full therefore takes many requests.

Rate limits explain why some chart tools ask for a token. GitHub's [rate limit docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) give 60 requests per hour for unauthenticated requests and 5,000 requests per hour for authenticated ones.

Each point on the curve is the repository's cumulative star count for one calendar day. A star that was later removed doesn't count. The line records attention; maintenance is read from the commit history.

Every chart names its source, its coverage date and its state. That label tells you how current the drawing is before you read anything into it.

The same data works outside the README. The JSON export and the Markdown route for AI agents use these URLs:

```bash
# star history as JSON
curl https://api.gitdebt.com/api/repos/facebook/react/stars.json

# any report as Markdown, for AI agents
curl https://api.gitdebt.com/api/md/facebook/react
```

## How do you read a star growth chart, and what can it not show?

You read a star growth chart by choosing a time axis and a scale, then treating the curve as a record of attention rather than upkeep. Those two controls change what the same data answers.

Two time axes answer two questions:

- **Calendar date** puts every repository on real dates, so it shows what was happening in the same month.
- **Days since first star** lines each repository up at its own first star, so it shows which one grew faster in its first 90 days.

Linear and log axes answer different questions.

| Scale | What it does | Use it when |
|---|---|---|
| Linear | Spaces values by star count, so a large total flattens a young repository into a line near zero | The two repositories are close in size |
| Log | Spaces values by ratio, so a young repository's early growth stays readable | One repository is far larger than the other |

Read the axis labels first. On a log axis, equal vertical steps mean equal multiples, not equal star counts.

A spike shows attention at a moment. It says nothing about why people starred, and nothing about whether anyone still maintains the code.

Stars are what the crowd did. Commits are what the maintainers did. Maintenance is read from the commit history, a separate and independent source, and gitdebt never blends the two into one score.

The report for [angular/angular](https://gitdebt.com/angular/angular) carries both: the star curve and commit-based health readings. The two can disagree. A steep star curve beside thin recent commits, or a flat curve beside steady commits, is the finding worth following up.

Health readings are directional signals. A quiet commit month can mean a stable library or a stalled one, and only the repository itself can tell you which.

## How do you compare star growth between two repositories fairly?

You compare star growth between two repositories fairly by aligning both curves to days since each one's first star, so the older repository's head start no longer decides the result.

1. Open [gitdebt's compare page](https://gitdebt.com/compare) and enter `facebook/react` and `vercel/next.js` as owner/repo slugs. A misspelled slug returns no curve.
2. Switch the axis to days since first star. Both lines now start at day zero. If you skip this, the calendar view gives the older repository years of extra history.
3. Set one date window for both, such as the first 90 days after each repository's first star. Reading a different span for each repository brings the age bias back.
4. Choose the scale. Pick linear when the two are close in size. Pick log when one is far larger, because on a linear axis the smaller curve flattens along the bottom.
5. Read the shapes: slope, early bursts and plateaus. Leave the counts alone.

With `facebook/react` and `vercel/next.js` on a 90-day window, look for three shapes:

- A steep climb: one line rises from day zero while the other builds slowly.
- A jump: either line rises in a single day, the way a launch announcement might produce.
- A plateau: either line flattens before day 90.

Each shape describes how attention arrived. None of them needs a number.

The compare page overlays up to eight repositories on one set of axes, so you can add more slugs to the same drawing. For your own analysis, download the `stars.json` export for each repository.

Equal-start growth compares attention curves only. Project quality and maintainer activity sit in the commit history.

## Why is my GitHub star growth chart embed not rendering or out of date?

A GitHub star growth chart embed fails to render or looks out of date for one of three reasons: the repository hasn't been analyzed yet, the markup isn't a plain image, or GitHub's image proxy is serving an older copy. Work through these checks in order.

1. Check for a placeholder. Per [gitdebt's embed documentation](https://gitdebt.com/badges), a repository nobody has analyzed yet renders a placeholder and queues the work, and the real chart replaces it at the same URL. Leave the snippet as it is and reload the README later; a second embed isn't needed.
2. Confirm the markup is an `img` tag. An iframe or a script leaves a blank space in a README. Use an `img` tag, optionally inside a `<picture>` element for light and dark variants.
3. Confirm the repository is public.
4. Read the URL character by character. Compare the owner and repo with the address in your browser, because one wrong letter points at a different repository. Check that the path ends in `chart.svg`, that `theme=` is spelled exactly `light` or `dark`, and that the `?ref=readme` link still wraps the image.
5. Open the report page and find the chart's named source, its coverage date and its state. If the report shows a newer coverage date than the picture in your README, the README is showing an older copy.
6. Reload later if the image looks stale. [GitHub's documentation](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) says "To host your images, GitHub uses the open-source project Camo." README images pass through that proxy, so the copy you see may be one it already stored.

You can fix the markup, the repository's visibility and the URL spelling yourself. You can also reload the README after waiting.

You can't clear or configure GitHub's proxy cache, and how long it keeps an image is not published. A current report page beside an older README image points to the cache. The image updates once the proxy fetches the URL again.

## Do star charts show fake stars?

A star chart draws the daily totals GitHub reports, so it can't tell you whether any single star came from a genuine user. Researchers do study the topic: Hao He, Haoqin Yang, Philipp Burckhardt, Alexandros Kapravelos, Bogdan Vasilescu and Christian Kästner published an [arXiv paper on suspected fake stars on GitHub](https://arxiv.org/abs/2412.13459).

gitdebt's [methodology page](https://gitdebt.com/about) is explicit: gitdebt does not flag stars as fake, real or suspicious, and it doesn't score accounts. It stores only star dates and day counts, never stargazer profiles.

A steep curve over a flat commit trend is a reason to look closer at a repository. To check, open the report for a repository such as `facebook/react` and read the commit trend under the star curve.

## Frequently asked questions

### How do I embed a GitHub star growth chart in my README?

Add an `img` tag that points to `https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg`, with OWNER/REPO replaced by your repository. GitHub renders images in READMEs, so no iframe, token or build step is needed. The full snippet is on the [badges page](https://gitdebt.com/badges).

### Can I use an iframe, a token or a GitHub Action instead?

You don't need any of them. Paste the `img` tag, commit the README, and nothing else in your repository changes.

### How do I make the star chart match GitHub dark mode?

Wrap two images in a `picture` element: one URL with `theme=light` and one with `theme=dark`. Each viewer's GitHub theme picks the matching drawing, so include both variants.

### Why does my star chart show a placeholder?

Your repository hasn't been analyzed yet. gitdebt queues the work, and the real chart replaces the placeholder at the same URL. The wait varies, so reload the README page later.

## What should you do next with your GitHub star growth chart embed?

Start today: copy the snippet from [gitdebt.com/badges](https://gitdebt.com/badges), swap `OWNER/REPO` for your repository, and commit the README. Your github star growth chart embed then lives in the page as a plain image URL.

Next, reload the README and confirm the real chart replaced any placeholder. Then add the light and dark pair so the chart matches each viewer's theme. If the image still looks old, compare it with the coverage date on the report before changing anything.

After that, compare repositories on equal-start growth, and read stars beside commits in [gitdebt's repository reports](https://gitdebt.com/). Stars say a repository is popular; its commits say whether it is alive.

Paste the snippet into your README now.

## Sources

[README badges, star-history charts and embeds](https://gitdebt.com/badges) · [Basic writing and formatting syntax - GitHub Docs](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) · [How to add a live star history chart to your github readme](https://dev.to/xuanhun/how-to-add-a-live-star-history-chart-to-your-github-readme-15hk) · [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring) · [Using pagination in the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api) · [Rate limits for the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [angular/angular — star history & stats · gitdebt](https://gitdebt.com/angular/angular). Every other source is linked where the article cites it.
