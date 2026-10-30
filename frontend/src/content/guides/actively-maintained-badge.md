---
title: "How to add an actively maintained badge to your README"
metaTitle: "Add an actively maintained badge to a README"
description: "Add a README badge that says actively maintained only while recent commits prove it, check whether your repository earns it, and see what greys it out."
updatedDate: "2026-10-30"
order: 6
related:
  - contributor-ready-repository
  - commit-activity-graph-readme
  - repository-stats-api
---

A hand-written "maintained: yes" badge stays green after the last maintainer leaves. You can add a badge that gitdebt checks against the commit history every day, so it turns grey when the work stops. You need a public GitHub repository and permission to edit its README.

## 1. Check whether the repository earns it today

gitdebt marks a repository as actively maintained when both of these are true in the last 30 days:

| Condition | Threshold |
| --- | --- |
| Most recent commit | On one of the last 30 days |
| Commits in the last 30 days | 5 or more |

Commits here are non-merge commits on the default branch, counted by the day they were authored. Commits from automation count too, so a repository whose only recent commits are dependency updates can still earn the badge. Read `detail` and the commit history together before you show it.

Ask the API for the current verdict before you add the badge:

### curl

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/earned-badges.json \
  | jq '.[] | select(.id == "active")'
```

### JavaScript

```js
const response = await fetch(
  "https://api.gitdebt.com/api/repos/OWNER/REPO/earned-badges.json",
);
const signals = await response.json();
console.log(signals.find((signal) => signal.id === "active"));
```

### Python

```python
import requests

signals = requests.get(
    "https://api.gitdebt.com/api/repos/OWNER/REPO/earned-badges.json", timeout=30
).json()
print(next(s for s in signals if s["id"] == "active"))
```

The answer looks like this:

```json
{"id":"active","label":"actively maintained","detail":"38 commits / 30d","earned":true,"pending":false}
```

| Field | Meaning |
| --- | --- |
| `earned` | `true` when both conditions hold today |
| `pending` | `true` while gitdebt has not finished its first analysis of the repository |
| `detail` | The number the badge prints, such as `38 commits / 30d` |

## 2. Paste the badge

Replace `OWNER/REPO` with your lowercase slug:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=active&theme=dark" />
    <img alt="OWNER/REPO actively maintained" src="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=active&theme=light" />
  </picture>
</a>
```

Put it in the badge row under the project title, next to your CI and license badges. The badge prints the label and the commit count from `detail`. The `.png` and `.webp` extensions give the same badge as a raster image.

## 3. Know what happens when the work slows down

When a repository stops meeting a condition, the same URL draws the badge in grey ink and its accessible label ends in "not earned".

The verdict uses a moving 30-day window and changes on its own in either direction. The README does not need an edit.

The badge has nothing to say about a project that is finished on purpose. A stable library with three commits a month greys out even when it is in good hands. For such a project, write the status in the README text instead, for example "Stable. Security fixes only.", and leave this badge out.

## 4. Add the other signal badges

The same endpoint has three more signals. Change `signal=active` to one of these:

| `signal` | Label | Earned when |
| --- | --- | --- |
| `community` | community powered | 5 or more contributors, and at least 2 people are needed to reach half of all commits |
| `momentum` | star momentum | 25 or more new stars in 30 days, and either 100 or more or at least 1% of the total |
| `contributor-ready` | contributor ready | The project files a new contributor needs are present. See [How to make a GitHub repository ready for contributors](/guides/contributor-ready-repository) |

`earned-badges.json` reports `active`, `community` and `momentum`. The `contributor-ready` badge prints its own count, such as `5/13 project guides`.

Add only the badges the repository earns. An unearned badge is grey and makes the badge row longer without telling the reader anything.

## 5. Wait for the first analysis

If nobody has analyzed the repository on gitdebt yet, `pending` is `true` and the badge reads "analysis pending". Open `https://gitdebt.com/OWNER/REPO` once to queue the work. To watch it:

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/progress.json
```

The verdict is ready when the `analysis` object shows `"complete":true`. After that, gitdebt treats an analysis as current for 24 hours, so a burst of commits today can take up to a day to count.

## Troubleshooting

| What you see | Cause | Fix |
| --- | --- | --- |
| Grey badge with `0 commits / 30d` | No commits on the default branch in 30 days | Expected. Work on other branches counts once it is merged |
| Grey badge with 3 or 4 commits | Fewer than 5 commits in the window | Expected. The badge turns back on its own |
| "analysis pending" for hours | The slug is wrong, or the repository is private | Check the slug. gitdebt reads public repositories only |
| HTTP 400 `{"error":"unknown badge signal"}` | A misspelled `signal` value | Use one of `active`, `community`, `momentum`, `contributor-ready` |

gitdebt cannot set a custom threshold, such as one commit a month. If your project needs another rule, read `last_commit_day` and `commit_months` from [the repository's health JSON](/guides/repository-stats-api) and publish your own status.

[Look up your repository on gitdebt](/report) to see every signal it earns today.
