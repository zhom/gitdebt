---
title: "How to add a commit activity graph to a repository README"
metaTitle: "Add a commit activity graph to a README"
description: "Embed a 52-week commit calendar or a monthly commit trend for any public repository in its README, in light and dark versions, from one image URL."
updatedDate: "2026-10-30"
order: 5
related:
  - actively-maintained-badge
  - show-contributors-in-readme
  - repository-stats-api
---

GitHub draws a contribution calendar for user profiles only. You can put a commit graph for one repository in its README, so a visitor can see how often code lands. You need a public GitHub repository and permission to edit its README.

## 1. Pick the graph

gitdebt draws two commit graphs for every repository it has analyzed:

| Graph | Path | What it shows |
| --- | --- | --- |
| Commit calendar | `/stats/heatmap.svg` | One cell per day for the last 52 weeks, darker on days with more commits |
| Maintenance pulse | `/stats/commit-trend.svg` | Commits per month across the whole history, with the total and the busiest month |

The calendar answers "is work happening now". The trend answers "how has the pace changed over the years".

Both count non-merge commits on the default branch, on the day each commit was authored. Commits from automation, such as dependency updates, are included.

## 2. Paste the snippet

Replace `OWNER/REPO` with your lowercase slug. This is the calendar:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg?theme=dark" />
    <img alt="OWNER/REPO commit activity calendar" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg?theme=light" />
  </picture>
</a>
```

For the monthly trend, change `stats/heatmap.svg` to `stats/commit-trend.svg` in both URLs and change the alt text to `OWNER/REPO commit trend`.

The `<picture>` element shows the dark image to readers in dark mode. [How to show light and dark mode images in a GitHub README](/guides/readme-dark-mode-images) explains how the choice is made.

In plain Markdown, with the light image only:

```markdown
[![OWNER/REPO commit activity calendar](https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg)](https://gitdebt.com/OWNER/REPO?ref=readme)
```

## 3. Show one calendar year

By default the calendar covers the 52 weeks up to today and is titled "Commits in the last 52 weeks". Add `year` to draw one calendar year instead, for example in a yearly report or a release note:

```text
https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg?year=2025
```

The title then reads "Commits in 2025" with the total for that year.

| Parameter | Applies to | Effect |
| --- | --- | --- |
| `theme=light` or `theme=dark` | Both graphs | Colors baked into the image. The default is light |
| `year=YYYY` | Calendar only | 1 January to 31 December of that year |
| `animate=1` | SVG only | The graph draws itself when it loads. The default is a static image |

## 4. Use PNG or GIF outside GitHub

Both graphs exist at the same path with four extensions: `.svg`, `.png`, `.webp` and `.gif`. GitHub shows the SVG. Use the PNG on a site that does not show SVG images, or in a slide or a document:

```text
https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.png?theme=light
```

An unknown graph name or extension returns HTTP 400 with `{"error":"unknown chart or format"}`.

## 5. Wait for the first analysis

The graphs come from the repository's git history, which gitdebt clones and reads. If nobody has analyzed the repository yet, the first request queues that work and returns a placeholder. The graph replaces the placeholder at the same URL when the analysis ends. A large repository takes longer than a small one.

To start the work and watch it, open `https://gitdebt.com/OWNER/REPO`, or check the queue from a terminal:

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/progress.json
```

The graph is ready when the `analysis` object shows `"complete":true`.

After the first run, gitdebt treats an analysis as current for 24 hours, and images are cached for a few hours. Today's commits can take up to a day to appear.

## 6. Read the numbers as text (optional)

The monthly counts behind the trend are also in the repository's health summary as JSON. Use it when you want to write a number in prose without a second image:

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/health.json | jq '.commit_months[-3:]'
```

`commit_months` holds the last 24 months, with months that had no commits set to zero. [How to get GitHub repository stats as JSON](/guides/repository-stats-api) lists the other fields.

## Troubleshooting

| What you see | Cause | Fix |
| --- | --- | --- |
| Fewer commits than you expect | Merge commits and commits on other branches do not count | Expected. gitdebt reads the default branch only |
| The trend reads "no commit data yet" | The analysis has not finished, or it found no commits | Check `progress.json` |
| The placeholder stays for hours | The slug is wrong, or the repository is private | Check the slug. gitdebt reads public repositories only |
| A broken image | A wrong path or extension, answered with HTTP 400 | Check the graph name and extension against the table in step 1 |

gitdebt does not draw a graph for a single branch other than the default branch, or for one folder of a monorepo. For those, run `git log --since="1 year ago" --format=%ad --date=short -- path/` locally and chart the dates yourself.

[Open your repository on gitdebt](/report) to see both graphs before you paste one.
