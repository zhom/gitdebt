---
title: "How to add a star history chart to your GitHub README"
metaTitle: "Add a star history chart to a GitHub README"
description: "Paste one snippet to show a repository's full star history in its README, with light and dark versions, date ranges and log scale. No token needed."
updatedDate: "2026-10-30"
order: 1
related:
  - readme-dark-mode-images
  - fix-star-history-chart
  - export-star-history
  - actively-maintained-badge
---

You can put a live star history chart in your repository's README with one snippet. The chart is an image URL that gitdebt redraws from GitHub's own star history, so you paste it once and do not edit it again. You need a public GitHub repository and permission to change its README.

## 1. Find your repository slug

The slug is the `owner/repo` part of the repository URL. For `https://github.com/zhom/gitdebt` the slug is `zhom/gitdebt`. If you have a clone, this command prints the URL:

```bash
git remote get-url origin
```

Write the slug in lowercase. gitdebt lowercases every slug, so `Zhom/GitDebt` and `zhom/gitdebt` give the same chart.

## 2. Copy the snippet

Replace `OWNER/REPO` with your slug in all four places:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark" />
    <img alt="OWNER/REPO star history" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light" />
  </picture>
</a>
```

The `<picture>` element sends the dark chart to readers whose system is in dark mode and the light chart to everyone else. Each image has its colors baked in, so there is no automatic theme. [How to show light and dark mode images in a GitHub README](/guides/readme-dark-mode-images) explains the pattern.

The link opens the repository's report on gitdebt. Keep `?ref=readme` on the link and off the image URL. Each new image URL makes a new cache entry, so a changed image URL loads slower.

The [README badges and embeds page](/badges) has the same snippet with a copy button, already filled in for any repository you enter. If you want one theme and plain Markdown, use this line. Without `theme`, the chart is light.

```markdown
[![OWNER/REPO star history](https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg)](https://gitdebt.com/OWNER/REPO?ref=readme)
```

## 3. Paste it into README.md

Put the snippet under its own heading near the end of the README, above the license section:

```markdown
## Star history

<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>...</picture>
</a>
```

Paste the full snippet from step 2 in place of the shortened one above. Leave an empty line between the heading and the `<a>` tag, or GitHub does not render the HTML.

Commit and push, then open the repository on GitHub. GitHub sends README images through its image proxy at `camo.githubusercontent.com`. The URL in the rendered page changes to a proxy URL. The URL in your file stays the same.

If the README already has a star history chart from another service, replace that image in place. Keep your heading and text, and do not stack a second chart under the first.

## 4. Wait for the first read

If nobody has opened your repository on gitdebt before, the first request returns a placeholder frame and queues the work. The real chart replaces it at the same URL, usually within a few minutes. A browser can keep its copy of the placeholder for up to four hours, so open the image URL in a private window to see the current state.

To start the work at once, open `https://gitdebt.com/OWNER/REPO` in a browser. To check its state from a terminal:

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/progress.json
```

The chart is ready when the `stars` object in the answer shows `"complete":true`.

The curve comes from GitHub's star history endpoint. It counts the people who star the repository today, day by day, back to the week it was created. A star that someone removes leaves the curve too.

## 5. Change the chart with query parameters

All parameters are optional. Use the same query on the `<source>` and the `<img>` URL, with only `theme` different.

| Parameter | Values | Effect |
| --- | --- | --- |
| `theme` | `light`, `dark` | Colors baked into the image. The default is light, and any other value also gives light. |
| `from`, `to` | `YYYY-MM-DD` | An inclusive date window. You can set one side only. |
| `rebase` | `1` | Starts the window at zero instead of at the running total. |
| `type` | `date`, `timeline` | Calendar dates, or days since the first star. The default is `date`. |
| `log` | `1` | A logarithmic y axis. |
| `animate` | `1` | The SVG draws itself when it loads. The default is a static image. |

This URL shows stars gained since 1 January 2026 on a log scale:

```text
https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark&from=2026-01-01&rebase=1&log=1
```

A bad date makes the image request fail with HTTP 400, and GitHub then shows a broken image. The response body tells you why:

| Request | Response body |
| --- | --- |
| `from=2026-13-05` | `{"error":"invalid date: expected YYYY-MM-DD"}` |
| `from=2026-10-05&to=2026-10-01` | `{"error":"invalid date range: from is after to"}` |

## 6. Use another file format outside GitHub

Each chart has four file types at the same path: `chart.svg`, `chart.png`, `chart.webp` and `chart.gif`. GitHub shows the SVG, and `animate=1` plays there. npm, PyPI and Docker Hub show an SVG as one static frame, so use the GIF when you want motion on those pages.

The GIF has two motions. `motion=wave` is the default and loops. `motion=draw` draws the curve once. Any other value returns HTTP 400 with `chart.gif supports motion=wave (default) or motion=draw`.

```markdown
![OWNER/REPO star history](https://api.gitdebt.com/api/repos/OWNER/REPO/chart.gif?motion=draw)
```

## Troubleshooting

| What you see | Cause | Fix |
| --- | --- | --- |
| A broken image icon | A parameter is wrong and the server returned 400 | Run `curl -i` on the image URL and read the error |
| The placeholder stays for hours | The slug is wrong, or the repository is private or deleted | Check the slug on github.com. gitdebt reads public repositories only |
| The chart is a day behind | An embedded chart rereads its history when it is more than 24 hours old, and images are cached for a few hours | Wait. A cache-busting parameter makes a new cache entry and does not make the data newer |
| GitHub shows an old image after gitdebt updated | GitHub's image proxy kept its copy | See [How to fix a star history chart that stopped updating](/guides/fix-star-history-chart) |

gitdebt cannot chart a private repository. For a private repository, call GitHub's star history endpoint with a token that can read it. [GitHub star history API: what changed and how to use it](/blog/github-star-history-api) shows the requests.

To see your chart before you paste it, [look up your repository on gitdebt](/report).
