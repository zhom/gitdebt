---
description: Want a GitHub star chart animated gif for your README? Use a public chart.gif image URL wrapped in a link, drawn from real star history. Get the snippets.
faq:
  - answer: Paste a public chart.gif URL, wrapped in a link, into the README.md file as Markdown or an HTML img tag. Swap OWNER/REPO in api.gitdebt.com/api/repos/OWNER/REPO/chart.gif for your repository and keep the ?ref=readme link. This setup needs no sign-in, token or build step.
    question: How do I add an animated star history GIF to my README?
  - answer: "A star chart GIF can show real star data when it is drawn from the repository's own star history. gh-star-gif records a clip that highlights the Star button, and repogif animates a star count you type in. gitdebt's chart.gif plots the actual curve."
    question: Can a star chart GIF show real star data, or only a please-star clip?
  - answer: You should use chart.gif for most READMEs and chart.svg when you need crisp scaling. Both are plain public image URLs, so switching formats means swapping the .gif extension for .svg in the URL and leaving the rest of the snippet alone.
    question: Should I use a chart.gif or a chart.svg in my README?
  - answer: gitdebt cannot tell you which stars are fake, because it does not flag stars as fake, real or suspicious, and it does not score accounts. It stores star dates and day counts, never stargazer profiles, so the curve shows when stars arrived and nothing about who gave them.
    question: Can gitdebt tell me which stars are fake?
publishDate: "2026-11-04"
tags:
  - github readme
  - star history
  - animated gif
  - github stars
  - readme embeds
title: "GitHub star chart animated gif: Add a real star history"
---

**A github star chart animated gif can mean a [please-star call-to-action clip](https://github.com/shinshin86/gh-star-gif), a [mock counter with typed-in numbers](https://pypi.org/project/repogif/), or a real star history drawn from the repository's own data. For a README you want the third: a public image URL, wrapped in a link, that renders without JavaScript.**

Searching for a github star chart animated gif to put at the top of a README turns up [tools that record a click](https://github.com/shinshin86/gh-star-gif) on the Star button. Those tools need Node, Python or a headless browser installed. A plain image URL needs nothing installed.

Maintainers who want a truthful GitHub star chart animated GIF need the right kind of image, a snippet that renders, a way to read the curve, and fixes for a chart that won't show.

> **Key Takeaways**
> - GitHub serves README images through its [Camo proxy](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls), so a redrawn chart can lag its source URL.
> - [gitdebt does not flag stars](https://gitdebt.com/about) as fake, real or suspicious, and does not score accounts.

## What does a GitHub star chart animated GIF actually mean?

Only one kind of GitHub star chart animated GIF, a chart drawn from the repository's own star history, shows what happened. The other two are staged.

| Criterion | gh-star-gif | repogif | Real star-history chart |
| --- | --- | --- | --- |
| What it shows | Star button click | Mock star and fork counter | Cumulative star curve |
| Where the numbers come from | None | Typed in by you | The repository's own history |
| What it needs to run | npx, Playwright, ffmpeg | pip, Puppeteer | An image URL |

[The gh-star-gif README](https://github.com/shinshin86/gh-star-gif) says it "opens a GitHub repository page, highlights the Star button, moves a fake cursor toward it, and shows an emoji-heavy callout message prompting viewers to star the repo."

The tool drives headless Chromium through Playwright and encodes the video with ffmpeg, so it records a click and carries no star data.

[The repogif package on PyPI](https://pypi.org/project/repogif/) describes GIFs that "mimic GitHub repo stars/forks", with a customizable repository name, star count, and fork count. repogif captures with Puppeteer and installs with pip. Because you supply the count, the animation can show any number and proves nothing.

[GitHub Docs on saving repositories](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) says "You can star repositories and topics to keep track of projects you find interesting and discover related content in your news feed." A star is a save, so a real curve counts how many people saved the repository over time. It is a crowd signal, not a quality verdict.

The gh-star-gif page lists this command, which produces a clip of a cursor moving toward the Star button and nothing about the star curve:

```bash
npx gh-star-gif https://github.com/shinshin86/gh-star-gif
```

A README that claims traction should show the repository's own data, and [gitdebt's README badges page](https://gitdebt.com/badges) lists one source of such a chart.

- gh-star-gif suits a please-star ask, if you accept a Node, Playwright and ffmpeg setup.
- repogif suits a mock-up or demo visual, with the count labeled as an example.
- A star-history chart drawn from the repository's own data suits showing traction in a README.

## How do you add a GitHub star chart animated GIF to a README?

You add an animated star history GIF to a README by pasting your repository's `chart.gif` URL, wrapped in a link to its report.

1. Build the image URL: take `https://api.gitdebt.com/api/repos/OWNER/REPO/chart.gif` and swap `OWNER/REPO` for your real slug, such as `vitejs/vite`. A wrong slug leaves a broken image.
2. Point the link at the report, `https://gitdebt.com/OWNER/REPO?ref=readme`, and keep `?ref=readme` on it.
3. Paste snippet (a), (b) or (c) into your README and commit.

Snippet (a) is Markdown, with the image inside a link.

 [GitHub Docs describes Markdown images](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) this way: "You can display an image by adding! and wrapping the alt text in [ ]. Then wrap the link for the image in parentheses ()."

```html
[![Star history of OWNER/REPO](https://api.gitdebt.com/api/repos/OWNER/REPO/chart.gif)](https://gitdebt.com/OWNER/REPO?ref=readme)
```

Markdown image syntax takes only alt text and a URL, so it has no size option. Snippet (b) puts an HTML `img` tag inside an anchor instead. Use it when the chart must be resized, and set `width` to whatever fits your README.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <img src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.gif" alt="Star history of OWNER/REPO" width="600">
</a>
```

Snippet (c) serves a dark and a light chart. The `img` is the fallback.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.gif?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.gif?theme=light">
    <img src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.gif" alt="Star history of OWNER/REPO">
  </picture>
</a>
```

Once the README renders, your GitHub star chart animated GIF shows the repository's curve, and clicking it opens the report. The badges page also has each snippet ready to copy with the URLs filled in.

## Is a GitHub star chart animated GIF or an animated SVG better for a README?

A `chart.gif` suits most READMEs and a `chart.svg` suits READMEs that need crisp scaling; both are plain image URLs, while an MP4 is a file you upload.

| Criterion | `chart.gif` | `chart.svg` | MP4 video |
| --- | --- | --- | --- |
| Plain image URL | Yes | Yes | No, a file to upload |
| Motion | Always on | Static by default, `animate=1` opts in | Yes |
| Redrawn at the same URL | Yes | Yes | No, regenerate and re-upload |
| Scales without blur | No, raster | Yes, vector | Fixed frame |

[GitHub Docs on anonymized URLs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) states: "To host your images, GitHub uses the open-source project Camo." Your README image is requested through GitHub's proxy, so it has to be a public URL that returns an image.

A page that draws a chart with JavaScript returns HTML, which Markdown image syntax can't display.

Animated SVG has the same limit. [One open-source SVG chart project](https://github.com/YusufCeng1z/star-history/) notes that its "animation is plain CSS inside the SVG, so it plays in a GitHub README, where scripts don't run." Motion in a README SVG has to come from CSS.

Video follows a different path. [GenMotion's star count video generator](https://genmotion.dev/tools/github-star-count) outputs an MP4, which you upload as a file. The clip shows the count from the day you made it.

[gitdebt's README embeds](https://gitdebt.com/badges) serve `chart.gif` and `chart.svg` from the same URL pattern, and `animate=1` switches an SVG from static to motion.

Pick GIF when motion is the goal, SVG when sharp edges at any width matter, and MP4 only for a post or a launch page:

- `chart.gif` for a README that should move with no extra setup.
- `chart.svg` for a static, sharp chart, or for motion through `animate=1`.
- MP4 for a one-off social clip that you regenerate when the number changes.

## Where does star history data come from, and what are its limits?

Star history data comes from two places, GitHub's REST stargazers listing and GitHub's own star history, and each has limits to check before you trust a curve.

[GitHub's REST reference](https://docs.github.com/en/rest/activity/starring) says the stargazers endpoint "Lists the people that have starred the repository." The `application/vnd.github.star+json` media type "Includes a timestamp of when the star was created," which is the `starred_at` field.

Without that media type you get users but no dates.

The list covers current stargazers. A user who unstarred no longer appears, so that star never shows in a curve built from this listing.

Results are paginated. [GitHub's pagination guide](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api) says that GitHub "will paginate the results and return a subset of the results," so a client requests page after page, in order, then counts the `starred_at` dates per day to rebuild a long history.

The second source is GitHub's own star history. [gitdebt's methodology page](https://gitdebt.com/about) says it reads exact daily counts from it, net of unstars, back to the repository's creation.

gitdebt stores star dates and day counts only, never stargazer profiles.

Granularity changes the shape of a curve as much as the source does. A daily series shows a launch-day spike as a spike. A weekly or coarser series spreads the same stars across the whole period, so a short burst looks like steady growth.

Before comparing two repositories, check that both curves use the same granularity.

## How do you read a star history curve once you have it?

You read a star history curve by choosing a scale, choosing an x-axis, dating any step in the line, and checking that step against a source outside the curve. The [compare page](https://gitdebt.com/compare) overlays up to eight repositories on one set of axes for this.

1. Choose the scale. Linear shows absolute gain: a steep stretch means many stars arrived. It hides small repositories, because beside a large one their line flattens near zero.

 Log shows rate of growth and lets a small and a large repository share one axis. Read it with care: a steep log segment can be a modest number of stars.

2. Choose the x-axis. Calendar dates show what happened in the same market moment, such as two repositories rising after the same release cycle. They hide head starts. Days since first star (equal-start growth) lines every repository up at day zero, so you can compare the first 90 days of up to eight repositories fairly.

3. Date each step. A sudden step can follow a launch post, a release or a link from a large site. The curve shows when it happened, not who starred or why. Note the date, then look for a matching release, post or link before you name a cause.

4. Cross-check outside the curve. Stars record what the crowd did. The commit history records what the maintainers did, and the two stay separate sources. A step with no matching release or commit activity is still only a step.

### Worked comparison

Start on calendar dates with a linear scale and note which curve sits highest.

Switch the axis to days since first star and compare only the first 90 days of each repository. The order of the curves can change, because an older repository's head start disappears once every line begins at its own first star.

Switch to log and check whether the repositories that looked flat on linear were growing at a similar rate. Read the ordering of the curves, since the shape matters more here than any single count.

## Can a star chart tell you whether stars are real?

A GitHub star chart animated GIF can't tell you whether stars are real, because a curve shows when stars arrived and says nothing about who gave them. Neither a smooth line nor a sudden step settles the question of fake stars.

[gitdebt's methodology page](https://gitdebt.com/about) states that gitdebt does not flag stars as fake, real or suspicious, and it does not score accounts. That can't be done correctly without privileged access to GitHub's full dataset, so gitdebt doesn't attempt it.

A second, independent source helps instead. Stars show what the crowd did. The commit history shows what the maintainers did, through four readings: maintenance, ownership, repair load and debt markers.

gitdebt keeps the two sources apart and never blends them into one score. Open a [repository report such as angular/angular](https://gitdebt.com/angular/angular), read the star curve first, then the four health readings from commits.

If the curve climbs while maintenance stays flat, that gap is a reason to look closer. Each reading is a directional signal, not a verdict.

## Why is my GitHub star chart animated GIF missing or out of date?

A GitHub star chart animated GIF that is missing or out of date in a README usually traces to one of six causes: an unanalyzed repository, GitHub's image proxy, a typo in the URL, a private repository, a young repository, or a missing `<img>` fallback.

1. A placeholder shows instead of a chart. The repository hasn't been analyzed yet. Open [the report page](https://gitdebt.com/report), enter `owner/repo` to queue the analysis, then reload the README.

2. An old chart stays after the data has changed. [GitHub's documentation on anonymized URLs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) says "Camo generates an anonymous URL proxy for each file," so README images load through GitHub's anonymized image proxy and a redrawn chart can lag behind its source. Open the chart URL directly in a browser to see the current drawing, then reload the README later.

3. The image is broken or shows the wrong repository. The `OWNER/REPO` slug or a parameter such as `theme=dark` has a typo. Paste the full image URL into a browser tab and confirm it renders the right repository.

4. A private repository gets no chart. Only a public repository gets a chart.

5. The curve is very short. A young repository has few days of star history, so the line is brief. Nothing is broken, and the curve lengthens as stars arrive.

6. The light or dark variant shows nothing. The `<picture>` element is missing its `<img>` fallback. Check that the `<img>` tag with the default chart URL sits inside `<picture>`, after the `<source>` tags.

## Frequently asked questions

### How do I add an animated star history GIF to my README?

Paste a public chart.gif URL, wrapped in a link, into README.md as Markdown or an HTML img tag. Swap OWNER/REPO in api.gitdebt.com/api/repos/OWNER/REPO/chart.gif for your repository and keep the?ref=readme link.

### Can a star chart GIF show real star data, or only a please-star clip?

A GIF can show real star data when it's drawn from the repository's own star history. [gh-star-gif records a clip](https://github.com/shinshin86/gh-star-gif) that highlights the Star button, and [repogif animates a star count](https://pypi.org/project/repogif/) you type in. gitdebt's chart.gif plots the actual curve.

### Should I use a chart.gif or a chart.svg in my README?

Use chart.gif for most READMEs and chart.svg when you need crisp scaling. Both are plain public image URLs, so you swap one extension for the other and leave the rest of the snippet alone.

### Can gitdebt tell me which stars are fake?

No. It stores star dates and day counts, never stargazer profiles, so the curve shows when stars arrived and nothing about who gave them.

## What should you do next with your star history chart?

Three steps take a github star chart animated gif from idea to a working README.

Today, look up your repository, copy the `chart.gif` snippet into `README.md`, and use the `<picture>` element so light and dark themes each get their own drawing.

This week, open the README on github.com and confirm the chart renders in both themes. Then add your repository to the [compare page](https://gitdebt.com/compare), switch the axis to days since first star, and set its first 90 days beside similar repositories.

After that, read the curve beside the health readings in your repository's report, keeping stars and commits as two separate sources and treating both as directional signals.

[Look up your repository on gitdebt](https://gitdebt.com/) and copy the embed from its [badges page](https://gitdebt.com/badges). It is not affiliated with GitHub, and you can delete the snippet whenever you like.

## Sources

[GitHub - shinshin86/gh-star-gif](https://github.com/shinshin86/gh-star-gif) · [repogif · PyPI](https://pypi.org/project/repogif/) · [About anonymized URLs - GitHub Docs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [Saving repositories with stars - GitHub Docs](https://docs.github.com/en/get-started/exploring-projects-on-github/saving-repositories-with-stars) · [Basic writing and formatting syntax - GitHub Docs](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) · [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring) · [gitdebt README](https://github.com/zhom/gitdebt). Every other source is linked where the article cites it.
