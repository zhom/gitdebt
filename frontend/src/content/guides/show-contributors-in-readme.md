---
title: "How to show contributors in your GitHub README"
metaTitle: "Show contributors in a GitHub README"
description: "Add a grid of contributor avatars to your README, each linked to that person's GitHub profile, that updates itself as commit rankings change."
updatedDate: "2026-10-30"
order: 4
related:
  - contributor-ready-repository
  - commit-activity-graph-readme
  - readme-dark-mode-images
---

Add the people who write your project's code to its README, as a grid of avatars that each link to a GitHub profile. The grid updates itself, so you paste it once. You need a public GitHub repository and permission to edit its README.

## 1. Pick a grid or a chart

gitdebt gives you two shapes:

| Shape | What it is | Use it for |
| --- | --- | --- |
| Linked avatar grid | One small image per person, each in its own link | A Contributors or Thanks section where readers click through to people |
| Contributors chart | One image at `/stats/contributors.svg` that ranks people with avatars inside | A project health section where readers compare numbers |

An image in a README cannot hold working links inside it. That is why the grid is many small images, each wrapped in its own `<a>`.

## 2. Paste the grid

Each slot addresses a rank. Rank 0 is the person with the most commits, rank 1 the next, and so on. Replace `OWNER/REPO` with your lowercase slug:

```html
<a href="https://api.gitdebt.com/api/repos/OWNER/REPO/contributors/0"><img src="https://api.gitdebt.com/api/repos/OWNER/REPO/contributors/0/avatar.svg" width="64" height="64" alt="OWNER/REPO contributor 1" /></a>
<a href="https://api.gitdebt.com/api/repos/OWNER/REPO/contributors/1"><img src="https://api.gitdebt.com/api/repos/OWNER/REPO/contributors/1/avatar.svg" width="64" height="64" alt="OWNER/REPO contributor 2" /></a>
<a href="https://api.gitdebt.com/api/repos/OWNER/REPO/contributors/2"><img src="https://api.gitdebt.com/api/repos/OWNER/REPO/contributors/2/avatar.svg" width="64" height="64" alt="OWNER/REPO contributor 3" /></a>
```

Repeat the line for each rank you want. The [README badges and embeds page](/badges) has the full twelve-slot block ready to copy.

Each link answers with a redirect to whoever holds that rank today, for example `https://github.com/gaearon` for rank 0 of `facebook/react`. When the ranking changes, the same markup shows and links the new person. A rank past the end of your list shows a transparent image and links to GitHub's contributor graph. A small project can paste twelve slots: the empty slots sit at the end of the row.

## 3. Show more or fewer people

gitdebt resolves ranks 0 to 199. Generate as many lines as you want with a short script. Each prints 24 slots at 48 pixels.

### Bash

```bash
SLUG=owner/repo
for i in $(seq 0 23); do
  printf '<a href="https://api.gitdebt.com/api/repos/%s/contributors/%d"><img src="https://api.gitdebt.com/api/repos/%s/contributors/%d/avatar.svg" width="48" height="48" alt="%s contributor %d" /></a>\n' \
    "$SLUG" "$i" "$SLUG" "$i" "$SLUG" "$((i + 1))"
done
```

### JavaScript (Node.js)

```js
const slug = "owner/repo";
const base = `https://api.gitdebt.com/api/repos/${slug}/contributors`;
for (let i = 0; i < 24; i++) {
  console.log(
    `<a href="${base}/${i}"><img src="${base}/${i}/avatar.svg" width="48" height="48" alt="${slug} contributor ${i + 1}" /></a>`,
  );
}
```

### Python

```python
slug = "owner/repo"
base = f"https://api.gitdebt.com/api/repos/{slug}/contributors"
for i in range(24):
    print(f'<a href="{base}/{i}"><img src="{base}/{i}/avatar.svg" width="48" height="48" alt="{slug} contributor {i + 1}" /></a>')
```

Paste the output into the README. The avatar also exists as `avatar.png` and `avatar.webp` for sites that do not show SVG.

## 4. Give the grid a section

Put the grid under its own heading, with one sentence that says who is in it, so a screen reader user hears more than a list of numbered contributors:

```markdown
## Contributors

The people with the most commits to this repository, updated daily.

<a href="https://api.gitdebt.com/api/repos/OWNER/REPO/contributors/0">...</a>
```

Keep the alt text on every avatar. The snippet's `OWNER/REPO contributor 1` names the rank, because the person behind a rank can change.

## 5. Add a link to the full list

Close the section with one link for readers who want everyone:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">Contributor ranking on gitdebt</a>
```

GitHub's own list is at `https://github.com/OWNER/REPO/graphs/contributors`.

## 6. Wait for the first analysis

The ranking comes from the repository's git history, which gitdebt clones and reads. If nobody has analyzed the repository yet, the first request for rank 0 queues that work and the slots stay empty until it finishes. A large repository takes longer than a small one.

After that, gitdebt treats an analysis as current for 24 hours, so new commits count within about a day. Matching a new author's email to a GitHub login is a separate lookup, so their avatar and profile link can follow later.

To check the state:

```bash
curl -s https://api.gitdebt.com/api/repos/OWNER/REPO/progress.json
```

The grid is ready when the `analysis` object shows `"complete":true`.

## How the ranking works

| Rule | Detail |
| --- | --- |
| What counts | Commits in the git history, per author |
| Order | Most commits first. A tie is ordered by email address, so it never flips at random |
| One person, many emails | Merged when the emails resolve to one GitHub login |
| Bots | Left out. That covers names with `[bot]` and accounts such as `dependabot`, `renovate`, `github-actions` and `copilot` |
| Where a slot links | The GitHub profile of the person at that rank |

## Troubleshooting

| What you see | Cause | Fix |
| --- | --- | --- |
| A slot links to GitHub's contributor graph | gitdebt could not match that author's email to a GitHub login | The author can add the commit email to their GitHub account. gitdebt retries an unmatched author about every 30 days |
| The same person in two slots | They committed from two emails that do not resolve to one GitHub login | The author can add both emails to one GitHub account |
| Someone who reviews or triages is missing | The grid counts commits only | Thank them by name in the text of the section |
| All slots empty after an hour | The slug is wrong, or the repository is private | Check the slug. gitdebt reads public repositories only |

gitdebt does not count issues, reviews or discussions, and it cannot add a person who has no commits. Write those names into the README by hand.

[Open your repository on gitdebt](/report) to see the full contributor ranking before you paste the grid.
