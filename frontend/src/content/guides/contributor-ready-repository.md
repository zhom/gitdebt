---
title: "How to make a GitHub repository ready for contributors"
metaTitle: "Make a GitHub repository ready for contributors"
description: "Add the files a first-time contributor looks for, from CONTRIBUTING.md to a security policy, CI and tests, then check the result with GitHub and gitdebt."
updatedDate: "2026-10-30"
order: 7
related:
  - actively-maintained-badge
  - show-contributors-in-readme
  - add-star-history-chart-to-readme
---

A first-time contributor needs to know what the project is, whether they may use it, how to send a change and how the change gets tested. You add a file for each of those questions, then check the result in two places. You need write access to the repository. Most steps take a few minutes each.

## 1. See what the repository already has

GitHub has a checklist for public repositories. On the repository's main page, click Insights, then click Community Standards in the left sidebar. Each missing file has an Add button.

gitdebt keeps a stricter checklist with 13 items, read from the files in the repository's default branch. Ask for its verdict:

```bash
curl -s "https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=contributor-ready" \
  | grep -o 'aria-label="contributor ready[^"]*"'
```

The answer reads like `aria-label="contributor ready: 5/13 project guides, not earned"`. The count is how many of the 13 items gitdebt found. Steps 2 to 6 add the eight items the badge needs.

## 2. Add the four documents

gitdebt looks for these files in the repository root, in `.github/` or in `docs/`. Case does not matter.

| Item | File name starts with | What it tells a contributor |
| --- | --- | --- |
| README | `README` | What the project is and how to run it |
| License | `LICENSE`, `LICENCE` or `COPYING` | Whether they may use and change the code |
| Contributing guide | `CONTRIBUTING.` | How to set up, test and send a change |
| Code of conduct | `CODE_OF_CONDUCT.` | How people are expected to behave, and who enforces it |

GitHub can write two of these for you. To add a code of conduct, choose Add file, then Create new file, and type `CODE_OF_CONDUCT.md` as the file name. GitHub then offers "Choose a code of conduct template". The license works the same way with the file name `LICENSE`.

Keep `CONTRIBUTING.md` short and concrete. Give the commands to install, test and lint, the branch to target and what a good pull request includes. [GitHub's guide to contributor guidelines](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/setting-guidelines-for-repository-contributors) lists more.

## 3. Add a security policy

Create `SECURITY.md` in the root, `.github/` or `docs/`. Say where to report a vulnerability privately and how fast you answer. Do not ask people to open a public issue for a vulnerability. GitHub's page on [adding a security policy](https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository) covers private vulnerability reporting.

## 4. Add an issue or pull request template

gitdebt needs one of these:

| Item | Where gitdebt looks |
| --- | --- |
| Issue templates | Any file in `.github/ISSUE_TEMPLATE/` |
| Pull request template | A file named `pull_request_template.*` in the root or in `.github/` |

A minimal pull request template, saved as `.github/pull_request_template.md`:

```markdown
## What this changes

## How you tested it

- [ ] Tests pass locally
```

GitHub also reads a pull request template in `docs/`, but gitdebt does not count one there. Put it in `.github/` to satisfy both.

## 5. Add CI

gitdebt counts CI when it finds any of these paths:

| CI system | Path |
| --- | --- |
| GitHub Actions | Any file in `.github/workflows/` |
| GitLab CI | `.gitlab-ci.yml` |
| Azure Pipelines | `azure-pipelines.yml` |
| CircleCI | Any file in `.circleci/` |

A workflow that runs your tests on every pull request also gives a contributor a result before a maintainer looks at the change.

## 6. Make the tests findable

gitdebt counts tests when one of these exists:

| Pattern | Example |
| --- | --- |
| A `test/` or `tests/` folder at the root | `tests/test_parser.py` |
| A `tests/` folder anywhere | `crates/core/tests/smoke.rs` |
| A file ending in `_test.rs`, `_test.py`, `.test.ts`, `.test.tsx`, `.spec.ts` or `.spec.tsx` | `src/parse.test.ts` |

gitdebt does not count other naming styles today. Go files ending in `_test.go`, JavaScript files ending in `.test.js` and folders named `__tests__` are not detected. If your tests use one of those styles, the badge stays grey on this item even though the tests exist. Say in `CONTRIBUTING.md` how to run them, and do not rename tests only for a badge.

## 7. Add the optional items

Five more items count toward the 13 but do not decide the badge:

| Item | Where gitdebt looks |
| --- | --- |
| Contributor license agreement | `CLA.md`, `CLA.txt`, `CONTRIBUTOR_LICENSE_AGREEMENT.md` or `CONTRIBUTOR-LICENSE-AGREEMENT.md` |
| Code owners | `CODEOWNERS` in the root, `.github/` or `docs/` |
| Changelog | A file starting with `CHANGELOG` or `CHANGES.` in the root, `.github/` or `docs/` |
| Dependency updates | `.github/dependabot.yml`, `.github/dependabot.yaml`, `renovate.json`, `renovate.json5` or `.renovaterc` |
| Pull request template or issue templates | Whichever of the two you did not add in step 4 |

A CLA counts toward the 13 only. Add one if your project needs signed agreements.

## 8. Check the result

Commit the files to the default branch. gitdebt reads the repository again when its last analysis is more than 24 hours old, and a visit to `https://gitdebt.com/OWNER/REPO` asks for that. Then run the command from step 1 again. When all eight required items are present, the label ends in "earned".

To show it, paste the badge into the README:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=contributor-ready&theme=dark" />
    <img alt="OWNER/REPO contributor ready" src="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=contributor-ready&theme=light" />
  </picture>
</a>
```

## Troubleshooting

| What you see | Cause | Fix |
| --- | --- | --- |
| GitHub's checklist is complete, gitdebt's count is lower | The files live in your account's `.github` repository as defaults | gitdebt reads only the repository's own files. Commit a copy into the repository |
| A file you added is not counted | It is in another folder, or on a branch other than the default | Move it to the root, `.github/` or `docs/` on the default branch |
| The count did not change after a commit | The last analysis is less than 24 hours old | Wait a day and check again |
| "analysis pending" | gitdebt has not analyzed the repository yet | Open `https://gitdebt.com/OWNER/REPO` once |

[Look up your repository on gitdebt](/report) to see its health signals next to its star history.
