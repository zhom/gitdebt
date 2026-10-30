---
title: "How to add stats to your GitHub profile README"
metaTitle: "Add stats to your GitHub profile README"
description: "Create a GitHub profile README and add a stats card, top languages, commit activity and pinned repository cards that update themselves. No token needed."
updatedDate: "2026-10-30"
order: 8
related:
  - count-github-stars
  - readme-dark-mode-images
  - show-contributors-in-readme
---

Build a profile README with live statistics: total stars, languages, a year of commit activity and cards for the repositories you want to show. Every item is an image URL, so you need no token or workflow. You need a GitHub account with public repositories.

## 1. Create the profile README

GitHub shows a README on your profile page when all of these are true: a repository has the same name as your username, it is public, and it has a `README.md` with content in its root.

1. In the upper-right corner of any GitHub page, open the + menu and click New repository.
2. Under "Repository name", type your username exactly.
3. Select Public and turn on Add README.
4. Click Create repository, then click Edit README.

For an organization, the profile README lives in a public repository named `.github`, at `profile/README.md`.

## 2. Let gitdebt read your repositories

Open `https://gitdebt.com/LOGIN`, with your username in place of `LOGIN`. The page asks gitdebt to list your public repositories, read their star history and queue their code analysis. Profile images only read what gitdebt already has, so until this first visit the card says "no gitdebt data yet".

Star history fills in first. Commit and language figures follow as each repository's analysis finishes, and a large account takes longer.

## 3. Add the stats card

Paste this into `README.md` and replace `LOGIN`:

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=dark" />
    <img alt="LOGIN maintainer statistics" src="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=light" />
  </picture>
</a>
```

The card shows total stars earned, commits found, the number of repositories you contributed to, repositories tracked and total forks. The figures cover the repositories gitdebt tracks for the account. That is why the card prints "repos tracked", and why some figures end in a plus sign.

Change the card with query parameters:

| Parameter | Values | Effect |
| --- | --- | --- |
| `hide` | `stars`, `commits`, `contribs`, `repos`, `forks` | Removes those fields. Separate several with commas |
| `show` | `since`, `langs` | Adds the first year with commits, or your top languages |
| `card_width` | 420 to 1000 | Width in pixels. The default is 560 |
| `custom_title` | Any text | Replaces the `@login` title |
| `number_format` | `short`, `long` | `64.3k` or `64,308`. The default is `short` |
| `hide_border`, `hide_title` | `1` | Trims the card for tight layouts |
| `theme` | `light`, `dark` | The two color sets. The default is light |

For example, `card.svg?show=langs&hide=forks&number_format=long&theme=dark`. Color parameters such as `bg_color` or `title_color` are accepted and ignored, because the card comes in the two themes only.

## 4. Add languages and commit activity

Four more images use the same `<picture>` shape with a different path:

| Image | Path after `/api/users/LOGIN/` | What it shows |
| --- | --- | --- |
| Language footprint | `stats/languages.svg` | Lines of code by language across your analyzed repositories |
| Commit activity | `stats/commit-activity.svg` | Every commit in the last 52 weeks, summed across the repositories you own |
| Contribution footprint | `stats/contributions.svg` | Work in your own projects against work in other people's projects |
| Aggregate star history | `chart.svg` | One curve that adds up the stars of your 50 most-starred repositories |

## 5. Add pinned repository cards

A repository card shows stars, forks, contributors, commits, the repository's age, its languages and a 90-day star sparkline. Use it to feature the projects you want visitors to open:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=dark" />
    <img alt="OWNER/REPO repository statistics" src="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=light" />
  </picture>
</a>
```

The repository card takes `hide` and `show` with the fields `stars`, `forks`, `contributors`, `commits`, `age` and `lines`, plus `card_width` from 320 to 800 (default 400), `custom_title` and `hide_border=1`. To show two cards side by side, put both `<a>` blocks one after the other with no blank line between them. They line up when the page is wide enough.

## 6. Commit and check

Commit `README.md` and open your profile page. If a card still says "no gitdebt data yet" or "code analysis is still running", reload later. Images in progress are served with a short cache at the edge so they fill in on their own, but your browser can keep its copy for a few hours. A private window shows the current state.

Every image also exists as `.png` and `.webp`, and the card and chart as `.gif`, for sites that do not show SVG.

## What the card does not show

gitdebt reads git history and star history. It does not count pull requests, issues, reviews, followers or streaks, and its color is limited to two themes. GitHub's own contribution graph on your profile page covers pull requests, issues and reviews. Private repositories are never read or counted.

## Troubleshooting

| What you see | Cause | Fix |
| --- | --- | --- |
| "no gitdebt data yet" | gitdebt has not listed the account's repositories | Open `https://gitdebt.com/LOGIN` once |
| Commits lower than you expect | Only tracked public repositories count, and merge commits do not | Expected. The plus sign marks a lower bound |
| A repository is missing from the star curve | The curve sums the 50 most-starred repositories | Expected for large accounts |
| HTTP 400 `{"error":"invalid login"}` | The username in the URL is not a valid GitHub login | Check the spelling. Logins have no spaces or dots |
| The profile README does not show | The repository name does not match the username, or the repository is private | Rename it to the exact username and make it public |

Your profile sheet on [gitdebt](/) is at `gitdebt.com/LOGIN` and shows every figure before you add the images.
