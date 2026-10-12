---
description: "GitHub's star history API returns daily star counts for any public repo, with no token. Learn what changed in 2026, how the endpoint works, and how to chart it."
faq:
  - answer: "You don't need one for public repositories. GitHub's docs say the endpoint can be used without authentication when only public resources are requested. A token still helps at volume: unauthenticated requests share a limit of 60 per hour, while a personal token gets a rate limit of 5,000 requests per hour."
    question: "Do I need a token to call GitHub's star history API?"
  - answer: "Only if you're an admin or collaborator on it, because the stargazer list still answers for those roles. Everyone else gets counts without names. GitHub built the history endpoint so tools can track star growth without exposing stargazer identities, which is the privacy line the 2026 restriction drew."
    question: Can I still see who starred my repository?
  - answer: It goes all the way to the week the repository was created. Pages walk backward from the newest week toward the creation week, and empty weeks come back as zeros, so a quiet stretch from years ago stays in the series. At 30 weeks per page and 100 pages, every GitHub repository fits.
    question: How far back does the star history go?
  - answer: "Two causes cover most cases. A chart that stopped moving in July 2026 has the frozen-curve problem: it was read from the stargazer list and never re-read. A chart that runs above the total was likely built from star actions, which count re-stars. Check the source line under the chart first."
    question: "Why doesn't my star chart match the count on GitHub?"
publishDate: "2026-10-12"
tags:
  - star history
  - github api
  - open source
  - readme
title: "GitHub star history API: what changed and how to use it"
---

# GitHub star history API: what changed and how to use it

**The GitHub star history API is `GET /repos/{owner}/{repo}/stargazers/history`, a REST endpoint GitHub announced on September 4, 2026. It returns a repository's stars grouped by calendar week, with a count for each day of the week. Public repositories need no token, and the response never names a stargazer. It replaced the stargazer list, which GitHub restricted to admins and collaborators.**

Take Maya, a made-up but typical maintainer of a small Rust CLI. In August 2026 she opens her README and sees the star-history chart flat since early July, even though new stars arrive every week. Her project is fine. The data under her chart, and under most star-history tools, had closed, and its replacement is the GitHub star history API.

For years, star history came from one place: the list of stargazers. A tool paged through it, read each star's timestamp, and counted the stars into a curve. When GitHub closed that list to outsiders in July 2026, every series read that way stopped on a fixed date. That's what happened to Maya's chart.

You probably know the old method. This guide gives you a working picture of the new one: what changed and when, how the endpoint's response is shaped, how to turn it into a cumulative curve, what the aggregate data can't tell you, and how to chart, compare and embed star history without code.

Every endpoint detail below comes from GitHub's changelog and REST documentation. gitdebt reads its own charts from this same source, day by day back to each repository's creation.

> **Key Takeaways**
> - On June 30, 2026, GitHub announced the stargazer list would be limited to admins and collaborators, because these lists were being misused to collect user data for spam.
> - The replacement endpoint, announced September 4, 2026, returns stars grouped by calendar week with a per-day count array.
> - Public repositories need no authentication, and no stargazer identity comes back.
> - A day is the finest unit the new endpoint reports.
> - Star counts alone shouldn't drive high-stakes decisions.

## What changed in GitHub's stargazer API in 2026?

GitHub limited its stargazer list to admins and collaborators in July 2026, and that created the frozen-curve problem: a star-history chart built from the list stops on the day access closed, while the repository keeps collecting stars the chart never sees. Nothing about the project changed. Only the data source did.

### What GitHub restricted, and why

GitHub announced the change on June 30, 2026. Access to two public endpoints became limited to admins and collaborators: List stargazers (`/repos/{owner}/{repo}/stargazers`) and List watchers (`/repos/{owner}/{repo}/subscribers`). Some users would now get empty responses or a 403 Forbidden status, on those endpoints and on the `/stargazers` and `/watchers` pages.

The stated reason was spam. GitHub said these lists had increasingly been misused to collect user data for spam activities. The REST docs now carry the same note: access to the stargazers listing endpoints is limited to admins and collaborators.

### Why star-history charts broke

The stargazer list was the raw material for star-history charts. With the `application/vnd.github.star+json` media type, each entry included a timestamp of when the star was created, so a tool could sort the stars and count them into a curve. The list had a ceiling even before 2026: it returned up to 100 stargazers per page and stopped at page 400, so nothing beyond 40,000 stars came back.

After July, a tool could read the list only for repositories it administered or collaborated on. Everyone else's chart froze. gitdebt hit the frozen-curve problem too: a series it read from the stargazer list stops on a fixed date until it's re-read from GitHub's star history. Every gitdebt chart names its source and its coverage date, so you can tell a frozen curve from a quiet repository. If one of your charts looks stuck, check [how gitdebt sources each star series](https://gitdebt.com/about).

A guide written before July 2026 that pages through `/stargazers` with the star+json media type now works only where you're an admin or collaborator.

## How does the GitHub star history API endpoint work?

The GitHub star history API is the REST endpoint `GET /repos/{owner}/{repo}/stargazers/history`. It returns a repository's stars grouped by calendar week, most recent first, and each week carries a seven-day array of per-day counts that starts on Sunday. Public repositories can be read without a token, and no stargazer is ever named.

### The request

```bash
curl -L \
 -H "Accept: application/vnd.github+json" \
 -H "X-GitHub-Api-Version: 2026-03-10" \
 "https://api.github.com/repos/OWNER/REPO/stargazers/history?per_page=30&page=1"
```

GitHub's own example adds an `Authorization: Bearer` header. You can skip it: the endpoint can be used without authentication when only public resources are requested. If you do send a fine-grained token, it needs read access to repository Metadata.

### The response

Each element of the JSON array is one week. This is GitHub's documented example:

```json
[
 { "week": 1754784000, "total": 19, "days": [0, 12, 7, 0, 0, 0, 0] }
]
```

- `week`: a Unix timestamp for the start of the week.
- `total`: that week's star count.
- `days`: the number of stars created on each day of the week, starting on Sunday.

Weeks without stars come back with zero counts instead of disappearing, so the series has no holes. If you've parsed `GET /repos/{owner}/{repo}/stats/commit_activity`, the shape will look familiar.

### Paging and limits

Pages move backward toward the repository's creation week, and weeks inside a page run newest to oldest. Concatenate the pages in order and you get one continuous series. `per_page` tops out at 30 weeks and `page` at 100. The page count depends on a repository's age, not its popularity: a year of history fits in two pages. A request that fails validation, or one GitHub treats as spam, gets a `422`.

One detail trips up careful code: week and day boundaries are not guaranteed to align with UTC. Treat each day as the calendar day GitHub reports, not as midnight UTC.

### The companion count endpoint

`GET /repos/{owner}/{repo}/stargazers/count` returns a single `count`: the current number of users who have starred the repository. People who removed their star aren't included. It's the fastest way to check a total without walking the whole history.

## How do you build a cumulative curve from the star history API?

The endpoint gives you weekly deltas, newest first. A chart needs a running total, oldest first. Getting from one to the other takes four steps:

1. **Fetch every page.** Start at page 1 and keep going until a page comes back with fewer than 30 weeks.
2. **Concatenate and reverse.** Pages already join into one continuous series, newest to oldest. Reverse it once so the creation week comes first.
3. **Expand each week into days.** The `days` array starts on Sunday, so day `i` is the week's start date plus `i` days.
4. **Add as you go.** Each day's running total is one point on the curve.

Here's a minimal version that needs only `curl` and `jq`:

```bash
OWNER=vitejs REPO=vite
for page in $(seq 1 100); do
 batch=$(curl -s -H "Accept: application/vnd.github+json" \
 "https://api.github.com/repos/$OWNER/$REPO/stargazers/history?per_page=30&page=$page")
 echo "$batch"
 [ "$(echo "$batch" | jq length)" -lt 30 ] && break
done | jq -s -r '
 add | reverse
 | [.[] | .week as $w | .days | to_entries[]
 | {date: ($w + .key * 86400 | todate[0:10]), stars: .value}]
 | foreach .[] as $d (0; . + $d.stars; "\($d.date),\(.)")'
```

The output is one `date,total` line per day, oldest first, ready for a spreadsheet or a plotting library. The current week's later days come back as zeros, so the last few lines repeat today's total.

### Three things to get right

- **Keep GitHub's calendar days.** Week and day boundaries aren't guaranteed to line up with UTC, so don't spread a day's stars across invented timestamps. The script adds whole days to each week's start, which keeps every star on the day GitHub put it.
- **Budget your requests.** Unauthenticated calls share a limit of 60 requests per hour. A repository with ten years of history needs 18 pages, so add a token when you chart more than a few repositories at once.
- **Check the total.** Compare your last running total with `GET /repos/{owner}/{repo}/stargazers/count`. If the gap is bigger than the stars a repository could gain between your two requests, fetch again.

Want the curve without maintaining a script? The next sections cover what this data can't show, then the no-code route.

## What does the new star history data lose compared with the stargazer list?

The new endpoint trades detail for privacy. You get exact daily counts for any public repository, and you lose everything that tied a star to a person or a moment.

| | Stargazer list | Star history endpoint |
|---|---|---|
| Who can read it | Admins and collaborators only, since July 2026 | Anyone, for public repositories |
| Identities | One entry per stargazer | None returned |
| Time resolution | A timestamp per star | One count per day |
| Size limit | Nothing beyond 40,000 stars | Up to 100 pages of 30 weeks |

### What you can no longer chart

Per-star timestamps made hour-by-hour launch charts possible. With one count per day, a launch spike shows up as one tall day instead of an hourly climb. And because no identities come back, any analysis of who starred (account age, overlap with other repositories) is off the table for anyone outside the repository. For most questions people ask of a star curve, daily resolution is enough.

### Why two charts of one repository can disagree

Take Priya, a hypothetical developer-relations lead building a quarterly report. She pulls two star charts for the same repository and finds one running well above the other. Neither tool is broken. One reads GitHub's star history, where unstars are already taken out. The other was rebuilt from historical star data, where star actions are recorded and unstars are not, so it reads as an attention signal rather than a net star count.

The gap is real and it can be large. A series built from star actions counts re-stars, and it can exceed the repository's own star total. Priya's fix took five minutes: she used the net series for the report and noted the source under the chart.

gitdebt labels the source on every chart and moves older series onto GitHub's star history as it re-reads them. When two charts disagree, check the source line before you check the math.

## How do you chart, compare and embed GitHub star history without code?

You don't need a script to read, compare or publish a star curve. gitdebt does the fetching, the cumulative math and the rendering, and every public repository report works without an account.

### Read one repository

Open [the repository lookup](https://gitdebt.com/report) and enter `owner/repo`. The report draws the full star history and names the source and coverage date under the chart. A repository nobody has read yet renders a placeholder frame and queues the work, then fills in at the same URL.

### Compare up to eight

[The comparison builder](https://gitdebt.com/compare) puts up to eight star histories on one drawing. Plot by calendar date or by days since each repository's first star, so a project launched last year can be read against one launched in 2015. Switch to log scale when one curve dwarfs the rest, narrow the date window, and export the series as CSV or JSON for your own analysis.

### Embed it in a README

Every asset is a plain image URL served from gitdebt's own database, with no account and no token. One snippet carries a light and a dark drawing, so the chart follows the reader's theme:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
 <picture>
 <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark" />
 <img alt="OWNER/REPO star history" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light" />
 </picture>
</a>
```

Replace `OWNER/REPO` with your repository and paste it under a `## Star history` heading. There's no GitHub App to install and no token to renew. [The badge and embed catalog](https://gitdebt.com/badges) has the same snippet ready to copy, plus cards, badges and health charts.

If your README has the frozen-curve problem, swapping in this snippet takes two minutes.

## What can't a star history chart tell you?

A star curve records attention. It doesn't measure code quality, real usage, or whether anyone still maintains the project, and it can be gamed. Researchers who scanned GitHub event data from July 2019 to December 2024 identified 6.0 million suspected fake stars and 18,617 repositories with fake star campaigns. Their advice: star counts should not be used for high-stakes decisions by themselves.

gitdebt does not flag stars as fake, real, or suspicious, and it doesn't score individual accounts. That can't be done correctly without privileged access to GitHub's full dataset. The useful move is to read a second, independent source next to the stars.

### Pair the curve with the commit history

Stars are what the crowd did. The commit history is what the maintainers did. [gitdebt's repository lookup](https://gitdebt.com/) puts four readings from the commit history next to the star curve:

- **Maintenance:** commit cadence, or how regularly work actually lands.
- **Ownership:** whether the work is spread across a team or resting on one person.
- **Repair load:** how much of recent work is fixing rather than building.
- **Debt markers:** whether TODO and FIXME markers are being cleared or piling up.

None of these come from stars, which is why they can disagree with the curve. A steep star curve beside a falling commit cadence tells a different story than the same curve beside steady upkeep. The [apache/airflow report](https://gitdebt.com/apache/airflow) shows both on one page, so it's a good place to practice before you judge a repository you depend on.

Treat each reading as a directional signal, not a verdict.

## What to do next with your repository's star history

The stargazer list isn't coming back for outsiders, and you don't need it. GitHub's star history endpoint gives daily counts for any public repository, and moving over takes an afternoon.

- **Today:** check every chart and script that touches star data. Anything that pages `/stargazers` for a repository you don't administer has the frozen-curve problem.
- **This week:** point those scripts at `/stargazers/history`, and keep each star on the calendar day GitHub reports.
- **Before your next release:** put a live chart in your README, and read it next to the commit history before you quote it in a pitch or a review.

The quickest way to do all three is to [open your repository's report on gitdebt](https://gitdebt.com/report). Enter `owner/repo`, check the source line under the curve, compare it with up to seven other repositories, then copy the README snippet. gitdebt is MIT-licensed and open source, and the report needs no account.

## Sources

1. It replaced the stargazer list, which GitHub restricted to admins and collaborators. — [New API endpoint provides privacy-safe star history data - GitHub Changelog](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/)
2. the response never names a stargazer — [New API endpoint provides privacy-safe star history data - GitHub Changelog](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/)
3. Public repositories need no token — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
4. a REST endpoint GitHub announced on September 4, 2026 — [New API endpoint provides privacy-safe star history data - GitHub Changelog](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/)
5. It returns a repository's stars grouped by calendar week, with a count for each day of the week. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
6. When GitHub closed that list to outsiders in July 2026, every series read that way stopped on a fixed date. — [About & methodology · gitdebt](https://gitdebt.com/about)
7. A tool paged through it, read each star's timestamp, and counted the stars into a curve. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
8. gitdebt reads its own charts from this same source, day by day back to each repository's creation. — [About & methodology · gitdebt](https://gitdebt.com/about)
9. The replacement endpoint, announced September 4, 2026, returns stars grouped by calendar week with a per-day count array. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
10. Public repositories need no authentication, and no stargazer identity comes back. — [New API endpoint provides privacy-safe star history data - GitHub Changelog](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/)
11. A day is the finest unit the new endpoint reports. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
12. Star counts alone shouldn't drive high-stakes decisions — [Six Million (Suspected) Fake Stars on GitHub: A Growing Spiral of Popularity Contests, Spam, and Malware](https://arxiv.org/html/2412.13459v2)
13. because these lists were being misused to collect user data for spam — [Upcoming access restrictions to public API endpoints and UI views - GitHub Changelog](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/)
14. On June 30, 2026, GitHub announced the stargazer list would be limited to admins and collaborators — [Upcoming access restrictions to public API endpoints and UI views - GitHub Changelog](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/)
15. Access to two public endpoints became limited to admins and collaborators: List stargazers (`/repos/{owner}/{repo}/stargazers`) and List watchers (`/repos/{owner}/{repo}/subscribers`). — [Upcoming access restrictions to public API endpoints and UI views - GitHub Changelog](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/)
16. Some users would now get empty responses or a 403 Forbidden status, on those endpoints and on the `/stargazers` and `/watchers` pages. — [Upcoming access restrictions to public API endpoints and UI views - GitHub Changelog](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/)
17. GitHub said these lists had increasingly been misused to collect user data for spam activities. — [Upcoming access restrictions to public API endpoints and UI views - GitHub Changelog](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/)
18. The REST docs now carry the same note: access to the stargazers listing endpoints is limited to admins and collaborators. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
19. With the `application/vnd.github.star+json` media type, each entry included a timestamp of when the star was created — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
20. it returned up to 100 stargazers per page and stopped at page 400, so nothing beyond 40,000 stars came back. — [The New GitHub Star History API - Star History](https://www.star-history.com/blog/new-github-star-history-api/)
21. a series it read from the stargazer list stops on a fixed date until it's re-read from GitHub's star history. — [About & methodology · gitdebt](https://gitdebt.com/about)
22. Every gitdebt chart names its source and its coverage date — [About & methodology · gitdebt](https://gitdebt.com/about)
23. GitHub limited its stargazer list to admins and collaborators in July 2026 — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
24. GitHub announced the change on June 30, 2026. — [Upcoming access restrictions to public API endpoints and UI views - GitHub Changelog](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/)
25. If you do send a fine-grained token, it needs read access to repository Metadata. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
26. `week`: a Unix timestamp for the start of the week. — [The New GitHub Star History API - Star History](https://www.star-history.com/blog/new-github-star-history-api/)
27. `days`: the number of stars created on each day of the week, starting on Sunday. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
28. Weeks without stars come back with zero counts instead of disappearing — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
29. Pages move backward toward the repository's creation week, and weeks inside a page run newest to oldest. Concatenate the pages in order and you get one continuous series. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
30. `per_page` tops out at 30 weeks and `page` at 100. — [The New GitHub Star History API - Star History](https://www.star-history.com/blog/new-github-star-history-api/)
31. If you've parsed `GET /repos/{owner}/{repo}/stats/commit_activity`, the shape will look familiar. — [The New GitHub Star History API - Star History](https://www.star-history.com/blog/new-github-star-history-api/)
32. The page count depends on a repository's age, not its popularity — [The New GitHub Star History API - Star History](https://www.star-history.com/blog/new-github-star-history-api/)
33. A request that fails validation, or one GitHub treats as spam, gets a `422`. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
34. week and day boundaries are not guaranteed to align with UTC — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
35. returns a single `count`: the current number of users who have starred the repository. People who removed their star aren't included. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
36. It returns a repository's stars grouped by calendar week, most recent first, and each week carries a seven-day array of per-day counts that starts on Sunday. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
37. You can skip it: the endpoint can be used without authentication when only public resources are requested. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
38. Unauthenticated calls share a limit of 60 requests per hour. — [Rate limits for the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
39. Pages already join into one continuous series, newest to oldest. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
40. The `days` array starts on Sunday — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
41. Week and day boundaries aren't guaranteed to line up with UTC — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
42. Nothing beyond 40,000 stars — [The New GitHub Star History API - Star History](https://www.star-history.com/blog/new-github-star-history-api/)
43. Up to 100 pages of 30 weeks — [The New GitHub Star History API - Star History](https://www.star-history.com/blog/new-github-star-history-api/)
44. One reads GitHub's star history, where unstars are already taken out. — [About & methodology · gitdebt](https://gitdebt.com/about)
45. The other was rebuilt from historical star data, where star actions are recorded and unstars are not, so it reads as an attention signal rather than a net star count. — [About & methodology · gitdebt](https://gitdebt.com/about)
46. A series built from star actions counts re-stars, and it can exceed the repository's own star total. — [About & methodology · gitdebt](https://gitdebt.com/about)
47. A timestamp per star — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
48. Anyone, for public repositories — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
49. Admins and collaborators only, since July 2026 — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
50. None returned — [New API endpoint provides privacy-safe star history data - GitHub Changelog](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/)
51. Every asset is a plain image URL served from gitdebt's own database, with no account and no token. — [README badges · gitdebt](https://gitdebt.com/badges)
52. One snippet carries a light and a dark drawing — [GitHub star history & repository health charts | gitdebt](https://gitdebt.com/)
53. The report draws the full star history and names the source and coverage date under the chart. — [About & methodology · gitdebt](https://gitdebt.com/about)
54. Switch to log scale when one curve dwarfs the rest, narrow the date window, and export the series as CSV or JSON for your own analysis. — [zhom/gitdebt on GitHub](https://github.com/zhom/gitdebt)
55. every public repository report works without an account — [About & methodology · gitdebt](https://gitdebt.com/about)
56. A repository nobody has read yet renders a placeholder frame and queues the work, then fills in at the same URL. — [README badges · gitdebt](https://gitdebt.com/badges)
57. puts up to eight star histories on one drawing — [GitHub star history & repository health charts | gitdebt](https://gitdebt.com/)
58. Plot by calendar date or by days since each repository's first star — [Compare GitHub star history · gitdebt](https://gitdebt.com/compare)
59. **Repair load:** how much of recent work is fixing rather than building. — [GitHub star history & repository health charts | gitdebt](https://gitdebt.com/)
60. **Ownership:** whether the work is spread across a team or resting on one person. — [GitHub star history & repository health charts | gitdebt](https://gitdebt.com/)
61. **Debt markers:** whether TODO and FIXME markers are being cleared or piling up. — [GitHub star history & repository health charts | gitdebt](https://gitdebt.com/)
62. None of these come from stars, which is why they can disagree with the curve. — [GitHub star history & repository health charts | gitdebt](https://gitdebt.com/)
63. Researchers who scanned GitHub event data from July 2019 to December 2024 identified 6.0 million suspected fake stars and 18,617 repositories with fake star campaigns. — [Six Million (Suspected) Fake Stars on GitHub: A Growing Spiral of Popularity Contests, Spam, and Malware](https://arxiv.org/html/2412.13459v2)
64. Their advice: star counts should not be used for high-stakes decisions by themselves. — [Six Million (Suspected) Fake Stars on GitHub: A Growing Spiral of Popularity Contests, Spam, and Malware](https://arxiv.org/html/2412.13459v2)
65. puts four readings from the commit history next to the star curve — [GitHub star history & repository health charts | gitdebt](https://gitdebt.com/)
66. **Maintenance:** commit cadence, or how regularly work actually lands. — [GitHub star history & repository health charts | gitdebt](https://gitdebt.com/)
67. gitdebt does not flag stars as fake, real, or suspicious, and it doesn't score individual accounts. That can't be done correctly without privileged access to GitHub's full dataset. — [About & methodology · gitdebt](https://gitdebt.com/about)
68. unauthenticated requests share a limit of 60 per hour — [Rate limits for the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
69. a personal token gets a rate limit of 5,000 requests per hour — [Rate limits for the REST API - GitHub Docs](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
70. At 30 weeks per page and 100 pages, every GitHub repository fits. — [The New GitHub Star History API - Star History](https://www.star-history.com/blog/new-github-star-history-api/)
71. A chart that runs above the total was likely built from star actions, which count re-stars. — [About & methodology · gitdebt](https://gitdebt.com/about)
72. GitHub built the history endpoint so tools can track star growth without exposing stargazer identities — [New API endpoint provides privacy-safe star history data - GitHub Changelog](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/)
73. Pages walk backward from the newest week toward the creation week, and empty weeks come back as zeros — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
74. Only if you're an admin or collaborator on it, because the stargazer list still answers for those roles. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
75. GitHub's docs say the endpoint can be used without authentication when only public resources are requested. — [REST API endpoints for starring - GitHub Docs](https://docs.github.com/en/rest/activity/starring)
76. gitdebt is MIT-licensed and open source, and the report needs no account. — [About & methodology · gitdebt](https://gitdebt.com/about)
