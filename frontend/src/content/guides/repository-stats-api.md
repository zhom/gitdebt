---
title: "How to get GitHub repository stats as JSON"
metaTitle: "Get GitHub repository stats as JSON"
description: "Read commit cadence, bus factor, contributors, hotspot files and TODO trends for any public GitHub repository as JSON, with curl, JavaScript or Python."
updatedDate: "2026-10-30"
order: 11
related:
  - export-star-history
  - actively-maintained-badge
  - count-github-stars
---

GitHub's API gives you a repository's star count and its commit list, and leaves the arithmetic to you. gitdebt's JSON API gives you finished health figures for any public repository: recent commit volume, bus factor, contributor count, the most-changed file and TODO movement. You need a terminal or a script. No account or API key is involved.

## 1. Pick the endpoint

All endpoints sit under `https://api.gitdebt.com/api/repos/OWNER/REPO/` and answer `GET`:

| Endpoint | What it returns | Size |
| --- | --- | --- |
| `health.json` | A fixed summary of the repository's health | Small, the same for any repository |
| `stats.json` | Every row behind the site's health charts: commit days, authors, files, languages, TODO days | Large for an old repository |
| `earned-badges.json` | The maintained, community and momentum signals with their verdicts | Small |
| `progress.json` | Whether the star history and the code analysis are done | Small |
| `stars.json`, `stars.csv` | Daily star history. See [How to export GitHub star history to CSV or JSON](/guides/export-star-history) | One row per day with stars |
| `report.md` | All of the above as one Markdown document, for people and coding agents | Medium |

Start with `health.json`. Use `stats.json` only when you draw your own charts.

## 2. Read health.json

### curl

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/health.json \
  | jq '{last_commit_day, commits_window, commits_previous_window, bus_factor, contributors}'
```

### JavaScript

```js
const response = await fetch(
  "https://api.gitdebt.com/api/repos/OWNER/REPO/health.json",
);
if (response.status === 202) throw new Error("Not analyzed yet");
const health = await response.json();
console.log(health.last_commit_day, health.bus_factor);
```

### Python

```python
import requests

response = requests.get(
    "https://api.gitdebt.com/api/repos/OWNER/REPO/health.json", timeout=30
)
if response.status_code == 202:
    raise SystemExit("Not analyzed yet")
health = response.json()
print(health["last_commit_day"], health["bus_factor"])
```

Use `requests`, `curl` or `fetch`. The API's edge refuses Python's default `urllib` user agent with HTTP 403, so set a `User-Agent` header if you use `urllib` directly.

## 3. Know what each field means

These fields were present for `facebook/react` on 30 October 2026:

| Field | Meaning |
| --- | --- |
| `ready` | `true` when a finished analysis backs the figures |
| `analyzed_at` | When gitdebt last read the git history |
| `archived` | Whether the repository is archived on GitHub |
| `stars` | The current star count |
| `window_days` | The length of the recent window, 90 days |
| `commits_window` | Commits in the last 90 days |
| `commits_previous_window` | Commits in the 90 days before that |
| `last_commit_day` | The day of the most recent commit |
| `commit_months` | Commits per month for the last 24 months, with empty months set to 0 |
| `total_commits` | All commits on the default branch, merge commits included |
| `contributors` | People with commits, bots left out |
| `bus_factor` | The fewest people whose commits add up to more than half of all commits |
| `top_author_commits` | Commits by the person with the most |
| `tracked_files` | Files in the history, dependency manifests and lock files left out |
| `fix_changes` | File changes made by commits whose message reads like a fix |
| `fresh_files` | Files changed in the last year |
| `hotspot` | The file most commits touched, with its commit and fix counts |
| `todo_delta_window` | TODO and FIXME markers added minus removed in the last 90 days |
| `todo_outstanding` | TODO and FIXME markers added minus removed over the whole history |
| `analysis_truncated` | Always `false` today, because gitdebt reads the whole history |

Apart from `total_commits`, commit figures count non-merge commits on the default branch.

A falling `commits_window` against `commits_previous_window` and an old `last_commit_day` point to a slowdown. A `bus_factor` of 1 means one person wrote more than half of the commits. [Bus factor: how to measure it from git history and its limits](/blog/bus-factor) explains how far to trust that number.

## 4. Start the analysis for a new repository

`health.json` and `stats.json` read gitdebt's database and never start work. For a repository nobody has analyzed yet, they answer:

```text
HTTP/2 202
{"ready":false,"repo":"owner/repo"}
```

To queue the analysis, request the Markdown report once. It answers 202 with a `Retry-After` header while the work runs:

```bash
curl -s -o /dev/null -w '%{http_code}\n' https://api.gitdebt.com/api/repos/OWNER/REPO/report.md
```

Then poll the progress until the code analysis is done:

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/progress.json | jq '.analysis'
```

```json
{"phase":"complete","complete":true,"processed_units":2,"total_units":2,"percent":100}
```

The analysis clones the repository, so a large repository takes longer than a small one. Add `?enqueue=0` to `report.md` to read without queueing anything.

## 5. Check many repositories

A script that checks your dependencies can loop over slugs. Stay under the rate limits per IP address: `health.json` and `stats.json` allow about 160 requests per 10 seconds, and `report.md`, `stars.json` and `earned-badges.json` about 40.

```bash
for slug in vitejs/vite evanw/esbuild facebook/react; do
  curl -s "https://api.gitdebt.com/api/repos/$slug/health.json" \
    | jq -r '[.repo, .last_commit_day, .commits_window, .bus_factor] | @tsv'
done
```

Each line prints the slug, the last commit day, the commits in 90 days and the bus factor.

## Common errors

| Status | Body | Cause |
| --- | --- | --- |
| 202 | `{"ready":false,...}` | No finished analysis yet. Do step 4 |
| 400 | `{"error":"invalid owner/repo"}` | The slug has characters GitHub does not allow |
| 429 | `Too Many Requests! Wait for 5s` | Too many requests from one IP. Wait for the seconds in `Retry-After` |
| 403 | `error code: 1010` | The edge refused the client's user agent. Send a `User-Agent` header |

gitdebt reads public repositories only and cannot report on a private one. It also does not report issues, pull requests or releases. For those, use GitHub's REST API with your own token.

To see the same figures drawn as charts, [open the repository on gitdebt](/report).
