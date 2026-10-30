---
title: "How to fix a star history chart that stopped updating"
metaTitle: "Fix a star history chart that stopped updating"
description: "Star history chart frozen since July 2026? Check if it reads GitHub's restricted stargazer list, swap the image in place and clear GitHub's image cache."
updatedDate: "2026-10-30"
order: 2
related:
  - add-star-history-chart-to-readme
  - readme-dark-mode-images
  - export-star-history
---

Find out why the star history chart in your README stopped moving, and replace it with one that updates. It takes about ten minutes. You need write access to the README and, for the optional checks, `curl` and the GitHub CLI (`gh`).

## 1. Confirm that the chart is stale

Open `README.md` and copy the URL of the chart image. Open that URL directly in a browser and read the last date on the x axis. If the last date is weeks old while the repository still gets stars, the chart is stale.

On the GitHub page itself, the image comes from GitHub's proxy at `camo.githubusercontent.com`. The proxy keeps a copy, so also open the source URL from the file.

## 2. Find out why it stopped

On 30 June 2026 GitHub [announced](https://github.blog/changelog/2026-06-30-upcoming-access-restrictions-to-public-api-endpoints-and-ui-views/) that the list stargazers endpoint, `/repos/{owner}/{repo}/stargazers`, and the `/stargazers` page would be limited to a repository's admins and collaborators. The restriction is in force now. A chart that pages through that list gets an error or an empty list, so its curve ends on the last day it could read.

You can see the restriction yourself. These are the answers on 30 October 2026:

| Request | Answer |
| --- | --- |
| `GET /repos/OWNER/REPO/stargazers` with no token | 401 |
| The same request with a token of an account that is not a collaborator | 404 |
| `GET /repos/OWNER/REPO/stargazers/history` with no token | 200 |

```bash
curl -s -o /dev/null -w '%{http_code}\n' https://api.github.com/repos/OWNER/REPO/stargazers
curl -s -o /dev/null -w '%{http_code}\n' https://api.github.com/repos/OWNER/REPO/stargazers/history
```

On 4 September 2026 GitHub [added the star history endpoint](https://github.blog/changelog/2026-09-04-new-api-endpoint-provides-privacy-safe-star-history-data/), `GET /repos/{owner}/{repo}/stargazers/history`. It returns star counts over time and no stargazer names. A chart that reads it keeps working.

## 3. Check your own workflows

Some repositories draw the chart in a GitHub Actions workflow and commit the image. Search the workflows for the old endpoint:

```bash
grep -rn "stargazers" .github/workflows
```

A match on `/stargazers` without `/history` is the cause. If the workflow does nothing else, you can delete it after step 4. If you want to keep your own script, [GitHub star history API: what changed and how to use it](/blog/github-star-history-api) shows how to move it to the history endpoint.

## 4. Replace the image in place

Swap only the image and the link around it. Keep your heading and the text near it. Replace `OWNER/REPO` with your lowercase slug:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark" />
    <img alt="OWNER/REPO star history" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light" />
  </picture>
</a>
```

gitdebt reads GitHub's star history endpoint, so it needs no token from you and no access to your repository. Do not add a second chart under the old one. Two charts that disagree confuse readers.

If the old chart was a file in the repository, delete the file and the workflow that wrote it in the same commit.

## 5. Commit and check the result

Commit and push, then reload the README on GitHub. The first request for a repository that gitdebt has not read yet returns a placeholder and queues the work. The chart fills in at the same URL, usually within a few minutes.

To check from a terminal:

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/progress.json
curl -s "https://api.gitdebt.com/api/repos/OWNER/REPO/stars.csv?from=2026-10-01" | tail -3
```

The first command shows `"complete":true` under `stars` when the series is ready. The second prints the last days with new stars.

## 6. Clear GitHub's cached copy if the URL did not change

GitHub builds the proxy URL from the source URL. When you replace the image with a new URL, the page gets a new proxy URL and shows the new chart at once.

If you kept the same image URL and only the server behind it changed, GitHub can show its old copy for a while. GitHub's [Camo troubleshooting page](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) gives the fix:

1. On the README page, right-click the chart and copy the image address. It starts with `https://camo.githubusercontent.com/`.
2. Send a purge request for that address:

```bash
curl -X PURGE "https://camo.githubusercontent.com/PASTE-THE-REST-HERE"
```

## Common problems

| What you see | Cause | Fix |
| --- | --- | --- |
| The new chart is a placeholder after an hour | The slug is wrong, or the repository is private | Check the slug on github.com. gitdebt reads public repositories only |
| A broken image icon | A query parameter is wrong and the server returned 400 | Run `curl -i` on the image URL and read the `error` field |

gitdebt cannot draw a private repository. GitHub's changelog keeps the stargazer list open to a repository's admins and collaborators, so for a private repository a script that runs with your own token is the way to chart it.

[Look up your repository on gitdebt](/report) to see the replacement chart before you commit it.
