---
title: "How to export GitHub star history to CSV or JSON"
metaTitle: "Export GitHub star history to CSV or JSON"
description: "Download any public repository's daily star history as CSV or JSON, cut it to a date range, and load it in Python or JavaScript. No account or key."
updatedDate: "2026-10-30"
order: 10
related:
  - repository-stats-api
  - add-star-history-chart-to-readme
  - count-github-stars
---

Download the star history of a public GitHub repository as a file you can open in a spreadsheet or load in code. You get one row per day with the running total and the stars gained that day. You need a terminal or a script. No account or API key is involved.

## 1. Make sure the history exists

gitdebt serves exports from its own database and does not call GitHub while it answers. If nobody has opened the repository on gitdebt yet, the export is empty. Open `https://gitdebt.com/OWNER/REPO` once to queue the read, then check it:

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/progress.json
```

The export is ready when the `stars` object shows `"complete":true`.

## 2. Download the CSV

```bash
curl -s -o stars.csv https://api.gitdebt.com/api/repos/OWNER/REPO/stars.csv
```

The file starts like this, for `facebook/react` in October 2026:

```text
date,total,delta
2026-10-01,250228,32
2026-10-02,250247,19
2026-10-03,250270,23
```

| Column | Meaning |
| --- | --- |
| `date` | The calendar day, `YYYY-MM-DD`, as GitHub's star history reports it |
| `total` | The running total of stars at the end of that day |
| `delta` | Stars gained that day |

A day with no new stars has no row. Step 5 fills those days in when you need a continuous series.

Each date is the day GitHub's star history reports. Those days do not always line up with UTC midnight, so keep the dates as they are and do not shift them by a time zone.

For a quick sum per month without a script, `awk` reads the file directly:

```bash
awk -F, 'NR > 1 { month = substr($1, 1, 7); gained[month] += $3 } END { for (m in gained) print m, gained[m] }' stars.csv | sort
```

Each output line is a month and the stars gained in it.

The series counts the people who star the repository today, so a removed star leaves the history. The last `total` can differ a little from the count on the repository page.

## 3. Cut it to a date range

Add `from`, `to` or both. Both dates are inclusive:

```bash
curl -s "https://api.gitdebt.com/api/repos/OWNER/REPO/stars.csv?from=2026-10-01&to=2026-10-31"
```

By default each `total` stays the true running total. Add `rebase=1` to count from zero at the start of the window, which shows the stars gained in that window:

```text
date,total,delta
2026-10-01,32,32
2026-10-02,51,19
2026-10-03,74,23
```

| Parameter | Values | Effect |
| --- | --- | --- |
| `from` | `YYYY-MM-DD` | First day to include |
| `to` | `YYYY-MM-DD` | Last day to include |
| `rebase` | `1` | `total` starts at zero on the first day of the window |

## 4. Use the JSON version for scripts

`stars.json` takes the same parameters and adds facts about the series:

```bash
curl -s "https://api.gitdebt.com/api/repos/OWNER/REPO/stars.json?from=2026-10-01&to=2026-10-03"
```

```json
{"repo":"facebook/react","total_stars":251733,"complete":true,"history_kind":"daily_stargazers","approximate":false,"series":[{"date":"2026-10-01","total":250228,"delta":32},{"date":"2026-10-02","total":250247,"delta":19},{"date":"2026-10-03","total":250270,"delta":23}]}
```

| Field | Meaning |
| --- | --- |
| `total_stars` | The repository's current star count. The date window does not change it |
| `complete` | `false` while gitdebt has no full history. `series` is then empty |
| `history_kind` | Where the series comes from. `daily_stargazers` is GitHub's own star history |
| `approximate` | `true` when part of the series comes from older historical data |
| `series` | The same rows as the CSV |

## 5. Load it in Python or JavaScript

### Python with pandas

```python
import pandas as pd

url = "https://api.gitdebt.com/api/repos/OWNER/REPO/stars.csv?from=2026-01-01"
df = pd.read_csv(
    url, parse_dates=["date"], storage_options={"User-Agent": "star-export/1.0"}
).set_index("date")

days = pd.date_range(df.index.min(), df.index.max(), freq="D")
df = df.reindex(days)
df["delta"] = df["delta"].fillna(0).astype(int)
df["total"] = df["total"].ffill().astype(int)

weekly = df["delta"].resample("W").sum()
print(weekly.tail())
```

The `User-Agent` matters. The API's edge refuses Python's default `urllib` user agent with HTTP 403, and pandas uses `urllib` for URLs. `requests` and `curl` send their own user agents and are not affected.

### JavaScript (Node.js 18 or later, or a browser)

```js
const url = "https://api.gitdebt.com/api/repos/OWNER/REPO/stars.json?from=2026-01-01";
const body = await (await fetch(url)).json();
if (!body.complete) throw new Error("No complete star history yet");

const rows = [];
let previous = null;
for (const point of body.series) {
  const day = new Date(`${point.date}T00:00:00Z`);
  while (previous && previous.getTime() + 86_400_000 < day.getTime()) {
    previous = new Date(previous.getTime() + 86_400_000);
    rows.push({ date: previous.toISOString().slice(0, 10), total: rows.at(-1).total, delta: 0 });
  }
  rows.push(point);
  previous = day;
}
console.log(rows.slice(-7));
```

The API allows requests from any origin, so this also runs in a web page.

## 6. Export several repositories

Loop over the slugs and save one file each. Keep to a few requests a second: the export endpoints allow about 40 requests per 10 seconds from one IP address.

```bash
for slug in vitejs/vite evanw/esbuild; do
  curl -s -o "${slug//\//_}.csv" "https://api.gitdebt.com/api/repos/$slug/stars.csv"
  sleep 1
done
```

To compare them on one chart instead of in files, use the [compare page](/compare).

## Common errors

| Status | Body | Cause |
| --- | --- | --- |
| 400 | `{"error":"invalid date: expected YYYY-MM-DD"}` | A date is not a real `YYYY-MM-DD` day |
| 400 | `{"error":"invalid date range: from is after to"}` | `from` is later than `to` |
| 400 | `{"error":"invalid owner/repo"}` | The slug has characters GitHub does not allow |
| 404 | `{"error":"repo not found"}` | GitHub reports the repository as deleted |
| 429 | `Too Many Requests! Wait for 5s` | Too many requests from one IP. Wait for the seconds in `Retry-After` |
| 200 with only the header row | The history is not complete yet | Do step 1 and try again |

gitdebt exports counts per day and never the names of the people who starred. GitHub limits its stargazer list to a repository's admins and collaborators (announced on 30 June 2026), and gitdebt does not collect names. If you maintain the repository, GitHub's [list stargazers endpoint](https://docs.github.com/en/rest/activity/starring) still answers for you with your own token.

To check the numbers on a chart first, [look up the repository on gitdebt](/report).
