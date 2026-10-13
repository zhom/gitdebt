---
description: "Is there a star history API? Yes: GitHub's new endpoint returns star counts with timestamps and no stargazer names. Learn how to call it and migrate old code."
faq:
  - answer: Yes, GitHub announced an official star history API in its Sep 4, 2026 changelog. It is a REST endpoint that returns historical star counts with timestamps and no stargazer identities. Check the GitHub REST docs for current parameters and limits before using it.
    question: Is there an official GitHub API for star history?
  - answer: "GitHub restricted stargazer listing endpoints to admins and collaborators earlier in 2026 to protect user privacy. The old list exposed each stargazer's username, avatar, and profile URL, which spammers had used, and that exposure is why non-collaborators lost access to it."
    question: Why did the old stargazers endpoint stop working for non-collaborators?
  - answer: "GitHub's changelog doesn't say whether authentication is required or which scope applies, so you cannot assume either way. Read the REST docs for the current rule before you ship, and test one request against a public repository to confirm the behavior."
    question: Does the star history endpoint need a token?
  - answer: The Sep 4, 2026 changelog only promises historical star counts with timestamps, so it does not explicitly include a cumulative star count. If the docs show per-period entries, add them from oldest to newest yourself to get a running total.
    question: Does the response include a cumulative star count?
  - answer: "You should not give a third-party site write access to your repository for data alone; read it from GitHub's endpoint instead. starhistory.io calls itself the only tool that works after the June 2026 restriction, a self-reported claim, while star-history.com says it adopted GitHub's new API on Sep 5, 2026."
    question: Should I give a third-party site write access to my repository?
publishDate: "2026-10-12"
tags:
  - star history
  - github api
  - open source
  - readme
title: "GitHub star history API: what changed and how to use it"
updatedDate: "2026-10-13"
---

**Yes, there's an official star history API: [GitHub's Sep 4, 2026 changelog](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/) introduced `GET /repos/{owner}/{repo}/stargazers/history`, which returns privacy-safe weekly star data with no stargazer identities. It replaces the stargazers list for this job. A token-light script can page back to the repository's creation week and rebuild the star curve.**

A script, README chart or dashboard that read the stargazers list for timestamps may have stopped returning usable data. GitHub restricted those listing endpoints to admins and collaborators, and the star history API is the privacy-safe replacement.

By the end, you'll know what changed and when, how to call the new endpoint, how to build a cumulative curve, how to choose between tools, and how to migrate old code. Each step is something you can run against your own repository.

> **Key Takeaways**
> - The new endpoint returns timestamped star counts, not stargazer identities, so per-user `starred_at` data is gone ([GitHub changelog](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/)).
> - If a page holds 30 weeks, a 200-week-old repository needs 7 requests (200 ÷ 30 = 6.67, rounded up).
> - Check the weekday the `days` array starts on before converting week timestamps to calendar dates.
> - Claims that star-history.com is permanently broken predate [GitHub's Sep 4 changelog](https://www.star-history.com/blog/new-github-star-history-api/).

## What changed with GitHub's stargazers endpoint, and when?

GitHub changed its star data in two steps: it first limited the stargazer list to admins and collaborators, then on September 4, 2026 it added a star history endpoint that returns counts without identities.

![Image generated](https://api.birchseek.com/m/fPortwt_YhkEB1rnLzbzLU5PGOHAr2us.webp)

- **Earlier in 2026:** GitHub's changelog says stargazer listing endpoints "were restricted to admins and collaborators to protect user privacy." The quoted text gives no day.
- **Sep 4, 2026:** GitHub announced a new star history REST API endpoint that returns historical star counts with timestamps "without exposing individual stargazer data."
- **Sep 5, 2026:** Tianzhou of star-history.com wrote that "GitHub just introduced a privacy-safe API for star history data" and that Star History "adopted it immediately."

The old approach explains why charts broke. According to the star-history.com post, it used the list stargazers endpoint, which returns a `starred_at` timestamp for each stargazer. "Sort the timestamps and you have the curve."

The same response carried each stargazer's username, avatar and profile URL. The post says starring a popular repo "effectively adds you to a public mailing list." A tool that rebuilt its curve from that list was reading personal data to get a count.

### Fact-check: "the only tool that works"

The starhistory.io homepage says it is "The only tool that works after June 2026 API restriction." Set that beside the dated sources. GitHub's Sep 4 changelog announces a replacement endpoint, and the Sep 5 star-history.com post says "If a chart broke in the past two months, it should now work again."

The starhistory.io claim appears to predate the new endpoint, and its status is self-reported. Don't pick a tool from it. Open the two dated pages, then check whether a chart renders for a repository you know.

## How does the GitHub star history API endpoint work?

The GitHub star history API is a REST endpoint, `GET /repos/{owner}/{repo}/stargazers/history`, that returns star counts over time without naming who starred. GitHub's changelog says it lets you track repository star growth "without exposing stargazer identities." Think of it as a ledger of totals, not a guest book.

The difference matters for your code. The older list stargazers endpoint, according to the star-history.com post, "returns a `starred_at` timestamp for each stargazer" and also each stargazer's "username, avatar, and profile URL." That's a guest book: every name, with a time. A history endpoint gives you how many people signed and when. You can still draw a growth curve, but you can't ask who signed.

GitHub restricted the older route first. Its changelog says, "Earlier this year, stargazer listing endpoints were restricted to admins and collaborators to protect user privacy." The star history endpoint is the replacement for tools that only need counts.

The announcement is short. It doesn't publish response fields, page size, page limit or rate limits, and it sends readers to the GitHub REST API docs for details. Use that docs page as your reference for field names before you write a parser.

### Check the details before you build on them

Two requests tell you more than any summary. Run one with a token and one without, and keep the full headers.

```bash
curl -i -H "Accept: application/vnd.github+json" \
  https://api.github.com/repos/OWNER/REPO/stargazers/history

curl -i -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer $GITHUB_TOKEN" \
  https://api.github.com/repos/OWNER/REPO/stargazers/history
```

Record these from the real responses:

- The status code with and without a token, which shows whether authentication is required.
- The field names and types in each entry.
- Whether entries run oldest first or newest first.
- How many entries one page returns, and whether `page` and `per_page` change that.
- The `x-ratelimit-remaining` header after each call.

Write the results into a comment in your script, with the date. Docs change, and a dated note tells the next reader what you checked.

If you only want the curve, enter `owner/repo` on [gitdebt](https://gitdebt.com/) instead of writing code.

## How do you pull a full star history with curl, Python and JavaScript?

You pull a full star history by requesting the history endpoint page by page until it returns nothing, sorting the weeks oldest-first, and adding each week's count to a running total. GitHub's changelog says the endpoint lets you "Retrieve historical star counts with timestamps without exposing individual stargazer data."

The code below assumes the response shape from the endpoint reference earlier in this article: a list of entries with `week` (a Unix timestamp), `total` (stars gained that week) and `days` (seven daily values). Check it against the GitHub REST docs before you rely on it.

1. **Request page 1.** Send one request and confirm you get a JSON list. Add a token header if you hit rate limits, and add the API version header only if the endpoint docs ask for it.

```bash
curl -s -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer $GITHUB_TOKEN" \
  "https://api.github.com/repos/vitejs/vite/stargazers/history?page=1"
```

2. **Keep requesting pages.** Each page goes further back in time. Stop on an empty list, which is the signal that you've passed the creation week. Don't stop after a short page without checking the next one.

3. **Sort ascending by `week`.** Pages arrive newest-first. If you skip this sort, the running total counts backward and the curve is wrong.

4. **Accumulate and convert.** Add each `total` to a running sum. Convert `week` to a date in UTC, because a local time zone can shift the label by a day.

```python
import requests, datetime as dt

url = "https://api.github.com/repos/vitejs/vite/stargazers/history"
hdr = {"Accept": "application/vnd.github+json"}
rows, page = [], 1
while True:
    r = requests.get(url, params={"page": page}, headers=hdr, timeout=30)
    r.raise_for_status()
    batch = r.json()
    if not batch:
        break
    rows += batch
    page += 1

rows.sort(key=lambda w: w["week"])
running = 0
for w in rows:
    running += w["total"]
    day = dt.datetime.fromtimestamp(w["week"], dt.timezone.utc).date()
    print(day, running)
```

The Node version (save it as `.mjs` for top-level `await`) does the same:

```javascript
const url = "https://api.github.com/repos/vitejs/vite/stargazers/history";
const rows = [];
for (let page = 1; ; page++) {
  const res = await fetch(`${url}?page=${page}`, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const batch = await res.json();
  if (batch.length === 0) break;
  rows.push(...batch);
}
rows.sort((a, b) => a.week - b.week);
let running = 0;
for (const w of rows) {
  running += w.total;
  console.log(new Date(w.week * 1000).toISOString().slice(0, 10), running);
}
```

If you run either script against `vitejs/vite`, success looks like a date column that starts in the repository's creation week and a second column that never decreases. This is a hypothetical walk-through, not a captured run, so no star counts are quoted here. Print the first and last rows with today's date beside them if you want a record.

The curve is built from weekly buckets, so it is exact only at week granularity. For finer detail, use the `days` array, which splits each week into daily counts. Weeks start on Sunday, so don't label them Monday.

If you'd rather not page the API, gitdebt publishes the same kind of series as JSON at `https://api.gitdebt.com/api/repos/{owner}/{repo}/stars.json`.

## Do you need a token, and how many requests does a repository cost?

A token isn't clearly required for the history endpoint, but one raises your hourly request budget a lot. GitHub's changelog announcing the endpoint doesn't say whether public repositories need authentication or which scope applies, so check the endpoint's reference page before you ship. That detail is not published in the changelog.

The budget is documented elsewhere. GitHub's REST rate limit docs say the primary limit for unauthenticated requests is [60 requests per hour](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api), and that authenticated requests get [5,000 requests per hour](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api).

### What one repository costs

Each request returns one page. These figures are calculations, not measurements. They assume 30 weeks per page and a 100-page ceiling, as in the endpoint reference above.

`pages = ceil(weeks since creation / 30)`

- **1-year-old repository:** 52 weeks, so ceil(52 / 30) = 2 requests.
- **10-year-old repository:** 520 weeks, so ceil(520 / 30) = 18 requests.
- **Ceiling:** 100 pages × 30 weeks = 3,000 weeks, about 57 years of history.

Eighteen requests fits inside an unauthenticated hour. Ten such repositories (180 requests) does not.

### Cache it

The endpoint returns weekly counts, not a running total, so you have to build the cumulative curve yourself. Store the sums with each page.

1. Fetch every page once and save each completed older page. Past weeks don't change.
2. On a schedule, re-fetch only the first page, since pages run newest first. That's 1 request per repository per refresh.
3. Recompute the running total from your stored pages.

### Sample when a trend is enough

For a sparkline, fetch every 3rd page. You then see 30-week windows with 60-week gaps between them. You keep weekly growth rates inside each window, but you lose absolute totals, because skipped pages hide the stars gained in between.

If you'd rather skip the plumbing, you can read the same history on [gitdebt](https://gitdebt.com/).

## Star history API options compared: which one should you use?

| | Official endpoint | star-history.com | starhistory.io | gitdebt |
| --- | --- | --- | --- | --- |
| Access needed | Not published | Hosted chart | GitHub App install | No account |
| User data exposed | None, counts only | Not published | Not published | Day counts only |
| Maintenance you carry | Your script and paging | None, hosted | None, hosted | None, hosted image |
| Trust check | GitHub itself | Adopted the new endpoint | Self-reported claims | Open source |

For a data series, use the official endpoint; for a ready-made chart, pick a hosted tool only after checking what it asks for.

The old stargazers endpoint is no longer an option for most people. GitHub's changelog says [stargazer listing endpoints were restricted to admins and collaborators](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/), and the new endpoint works "without exposing stargazer identities." The old one returned each stargazer's username, avatar and profile URL, [according to the star-history.com author](https://www.star-history.com/blog/new-github-star-history-api/).

Stale marketing is the main hazard. starhistory.io's page says it is "The only tool that works after June 2026 API restriction," yet star-history.com says it [adopted the new endpoint immediately](https://www.star-history.com/blog/new-github-star-history-api/) and that broken charts "should now work again."

### Case study: starhistory.io and write access

starhistory.io's connect flow tells you to [install the starhistory-io GitHub App on your organization or account](https://starhistory.io/) and select the repositories to track. The same page has a section titled "Why does the App request write access?" Its answers are self-reported. Its open-source and permission claims aren't independently verified here.

Before you install any star history app, check:

- Which permissions it requests, and which of them are write.
- Why a read-only chart needs each one.
- Whether you can verify the repository behind the service.
- Whether the code is public.
- Whether the official endpoint gives you the same series with no install.

gitdebt is a neutral row only in the sense that we make it. Its curves come from [GitHub's own star history](https://gitdebt.com/about), no account is needed, and the [compare page](https://gitdebt.com/compare) overlays up to eight repositories. It is not affiliated with GitHub, and [gitdebt does not flag stars as fake, real or suspicious](https://gitdebt.com/about).

**Best for:**

- **Official endpoint:** scripts and dashboards that only need numbers.
- **star-history.com:** a hosted chart you embed without writing code.
- **starhistory.io:** only if the checklist above passes for you.
- **[gitdebt](https://gitdebt.com/):** README charts and multi-repository comparisons without sign-in.

## How do you migrate a script built on the old stargazers endpoint?

Migrating means swapping one endpoint, accepting weekly data instead of per-user data, and deleting whatever stored identities. Work through these seven steps.

1. **Swap the endpoint and header.** Replace the stargazers list call, and the custom Accept header the old code sent, with `GET /repos/{owner}/{repo}/stargazers/history`.
2. **Parse weeks, not users.** Drop the per-user `starred_at` parsing and read the weekly entries instead.
3. **Drop stargazer storage.** GitHub says the endpoint returns counts "without exposing individual stargazer data", so there's nothing identifying to keep.
4. **Rebuild cumulative sums.** Sort the weeks oldest-first and keep a running total.
5. **Move date logic to weeks.** Check the REST docs for the week-start day and time zone before you compare against old daily numbers.
6. **Cache completed pages.** Store finished pages and refetch only the newest one.
7. **Add a fallback path.** Handle the 404 and 403 responses the docs list.

**What you lose:** who starred and when. A list of notable stargazers has no replacement here.

Don't treat "write access is the only route" as settled. GitHub's changelog says only that the listing endpoints "were restricted to admins and collaborators".

## How do you put a star history chart in your README?

A star history chart goes in your README as a plain image URL inside a link, and gitdebt's embed needs no account, token, API key, build step or GitHub Action.

1. Open the [gitdebt README embeds page](https://gitdebt.com/badges) and copy the star-history snippet.
2. Swap `OWNER/REPO` for your repository in all four places. Missing one is the usual mistake.
3. Paste it into `README.md` and commit. Keep `?ref=readme` on the link only, never on the image URL.
4. Preview the README. A repository nobody has analyzed yet shows a placeholder and queues the work. The real chart replaces it at the same URL, so you don't edit anything.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
    <img alt="Star history chart" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

Every embed is a public image URL that renders without JavaScript and can be cached. Add `animate=1` to opt an SVG into motion, or swap in `chart.gif`.

Hosted chart services from other providers, such as starhistory.io, can also be embedded in a README as images.

To overlay up to eight repositories, use [gitdebt's compare page](https://gitdebt.com/compare). The project is [open source under the MIT licence](https://github.com/zhom/gitdebt) and has no affiliation with GitHub.

**Try it:** copy the star-history snippet from [gitdebt.com/badges](https://gitdebt.com/badges).

## Frequently asked questions

### Is there an official GitHub API for star history?

GitHub announced one in its Sep 4, 2026 changelog: a REST endpoint that returns historical star counts with timestamps and no stargazer identities. Check the GitHub REST docs for current parameters and limits.

### Why did the old stargazers endpoint stop working for non-collaborators?

GitHub restricted stargazer listing endpoints to admins and collaborators earlier in 2026 to protect user privacy. The old list exposed each stargazer's username, avatar and profile URL, which spammers had used.

### Does the star history endpoint need a token?

GitHub's changelog doesn't say whether authentication is required or which scope applies. Read the REST docs for the current rule before you ship, and test one request against a public repository.

### Does the response include a cumulative star count?

The Sep 4, 2026 changelog only promises historical star counts with timestamps. If the docs show per-period entries, add them from oldest to newest yourself to get a running total.

### Should I give a third-party site write access to my repository?

Not for data alone: read it from GitHub's endpoint instead. starhistory.io calls itself the only tool that works after the June 2026 restriction, a self-reported claim, while star-history.com says it adopted GitHub's new API on Sep 5, 2026.

## What should you do next?

**Today:** Run the curl call from the walkthrough against one of your own repositories. Then check each response field against the GitHub REST docs.

**This week:** Add paging, a running cumulative sum and a cache of completed pages. Then delete any code that still reads the old stargazers list, since GitHub restricted those listing endpoints to admins and collaborators.

**This month:** If all you need is a chart, skip the code and use a plain image embed. To see which repositories are gaining fastest, check the [star velocity leaderboard](https://gitdebt.com/leaderboard).

The star history API suits scripts that need raw weekly counts. For everything else, [look up any public repository at gitdebt](https://gitdebt.com/) to read its star history beside four repository health readings from commits. Or copy a README snippet from the [embeds page](https://gitdebt.com/badges).

Every public repository report [works without an account](https://gitdebt.com/about).

## Sources

[New API endpoint provides privacy-safe star history data](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/) · [The New GitHub Star History API](https://www.star-history.com/blog/new-github-star-history-api/) · [starhistory.io — Professional GitHub Star Charts](https://starhistory.io/) · [Rate limits for the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) · [Analyze a GitHub repository](https://gitdebt.com/about) · [gitdebt: GitHub star history and repository health](https://gitdebt.com/) · [README badges, star-history charts and embeds](https://gitdebt.com/badges)
