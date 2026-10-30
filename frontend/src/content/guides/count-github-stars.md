---
title: "How to count the total stars of a GitHub user or organization"
metaTitle: "Count total stars of a GitHub user or org"
description: "Add up the stars across every public repository of a GitHub user or organization with gh, curl, Python or JavaScript, then chart the total over time."
updatedDate: "2026-10-30"
order: 9
related:
  - github-profile-readme-stats
  - export-star-history
  - repository-stats-api
---

GitHub shows stars per repository and has no total for an account. You can add them up for any user or organization: the exact current number comes from GitHub's API, and the total over time from gitdebt. You need a terminal. A GitHub token is optional and raises the rate limit.

## 1. Decide what you count

| Question | Choice in this guide |
| --- | --- |
| Stars the account received, or stars it gave? | Received. Step 6 covers stars given |
| Include forks? | No. A fork's stars belong to the fork, and most forks have none. Remove the filter to include them |
| Private repositories? | No. Step 7 covers your own private repositories |
| User or organization? | Both. The same endpoint lists the public repositories of either |

The endpoint is `GET https://api.github.com/users/LOGIN/repos`. It returns 30 repositories per page by default and at most 100 with `per_page=100`, so a large account needs several pages.

For a user, the endpoint returns the repositories the account owns, because its `type` parameter defaults to `owner`. Repositories that the user works on inside an organization belong to that organization and are not in the sum. Archived repositories are public and are included. Count each organization on its own and add the results if you want a person's total across their organizations.

## 2. Count with the GitHub CLI

If you use `gh`, one line does it. `--paginate` fetches every page:

```bash
gh api --paginate "users/LOGIN/repos?per_page=100" \
  --jq '.[] | select(.fork | not) | .stargazers_count' \
  | awk '{ total += $1 } END { print total }'
```

For `rust-lang`, an organization, this printed 402845 on 30 October 2026, summed over 227 repositories that are not forks.

`--jq` runs once per page, which is why the sum happens in `awk` and not in `jq`. To include forks, delete `select(.fork | not) |` from the filter. To list the repositories behind the number, print `.full_name` next to `.stargazers_count`.

## 3. Count without the CLI

These scripts do the same with plain HTTP. Each reads `GITHUB_TOKEN` when it is set. Without a token, GitHub allows 60 requests an hour from one IP address. With one, it allows 5,000.

### Bash and curl

```bash
LOGIN=octocat
auth=()
[ -n "$GITHUB_TOKEN" ] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
total=0
page=1
while :; do
  body=$(curl -sf "${auth[@]}" "https://api.github.com/users/$LOGIN/repos?per_page=100&page=$page") || exit 1
  [ "$(echo "$body" | jq 'length')" -eq 0 ] && break
  total=$((total + $(echo "$body" | jq '[.[] | select(.fork | not) | .stargazers_count] | add // 0')))
  page=$((page + 1))
done
echo "$total"
```

### Python

```python
import json
import os
import urllib.request

login = "octocat"
headers = {}
if os.environ.get("GITHUB_TOKEN"):
    headers["Authorization"] = f"Bearer {os.environ['GITHUB_TOKEN']}"

total = 0
page = 1
while True:
    url = f"https://api.github.com/users/{login}/repos?per_page=100&page={page}"
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers)) as response:
        repos = json.load(response)
    if not repos:
        break
    total += sum(repo["stargazers_count"] for repo in repos if not repo["fork"])
    page += 1
print(total)
```

### JavaScript (Node.js 18 or later)

```js
const login = "octocat";
const headers = process.env.GITHUB_TOKEN
  ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` }
  : {};
let total = 0;
for (let page = 1; ; page++) {
  const response = await fetch(
    `https://api.github.com/users/${login}/repos?per_page=100&page=${page}`,
    { headers },
  );
  if (!response.ok) throw new Error(`GitHub answered ${response.status}`);
  const repos = await response.json();
  if (repos.length === 0) break;
  for (const repo of repos) if (!repo.fork) total += repo.stargazers_count;
}
console.log(total);
```

Save the JavaScript as `count.mjs` and run `node count.mjs`, so that top-level `await` works.

## 4. See the total over time

The API gives today's number. To see how the total grew, open `https://gitdebt.com/LOGIN`. The page sums the star history of the account's 50 most-starred public repositories into one curve, and states the gains in the last 30 and 90 days. It works for users and organizations.

The same curve is an image you can embed:

```text
https://api.gitdebt.com/api/users/LOGIN/chart.svg
```

The first visit queues the work for an account gitdebt has not seen. Large repositories fill in first, and the curve grows as the rest finish.

## 5. Read gitdebt's totals as JSON

`stats.json` returns what gitdebt has stored for the account. It reads gitdebt's database and does not call GitHub:

```bash
curl -s https://api.gitdebt.com/api/users/LOGIN/stats.json \
  | jq '{repos_tracked, total_stars, total_forks}'
```

`total_stars` covers the repositories gitdebt tracks for the account, and `repos_tracked` says how many that is. The number can trail GitHub's own sum by a few stars or by repositories gitdebt has not listed yet. For one account on 30 October 2026, gitdebt reported 64,308 stars over 60 tracked repositories, and the GitHub API summed 64,467 over 97 repositories that are not forks. Use step 2 when you need the exact current number.

## 6. Count the stars an account gave

Stars given are the repositories the account starred. Ask for one per page and read the last page number from the `Link` header:

```bash
curl -sI "https://api.github.com/users/LOGIN/starred?per_page=1" | grep -i '^link:'
```

The number after `page=` in the `rel="last"` link is the count.

## 7. Include your own private repositories

gitdebt never reads private repositories. The GitHub API lists your own private repositories with your token:

```bash
gh api --paginate "user/repos?visibility=private&affiliation=owner&per_page=100" \
  --jq '.[] | .stargazers_count' \
  | awk '{ total += $1 } END { print total }'
```

Add the result to the public total from step 2.

## Troubleshooting

| What you see | Cause | Fix |
| --- | --- | --- |
| HTTP 403 with "API rate limit exceeded" | More than 60 requests in an hour without a token | Set `GITHUB_TOKEN`, or wait for the hour to pass |
| HTTP 404 | The login does not exist | Check the spelling on github.com |
| gitdebt's total differs from the API | gitdebt counts the repositories it tracks | Use the API for the exact number and gitdebt for the trend |

To put the total in a profile README, see [How to add stats to your GitHub profile README](/guides/github-profile-readme-stats). To see the curve for any account now, [open gitdebt](/) and enter a login after the domain, as in `gitdebt.com/LOGIN`.
