---
description: What are README best practices? Lead with what the project does, install steps, a license and a maintenance sign. Learn which other sections to keep or cut.
faq:
  - answer: "Answer a new reader's questions fast: what the project does, how to install it, how to use it, what license applies, and whether it's maintained. Keep required sections short, put install above decoration, never include credentials, and check the README against the repository regularly."
    question: What are the best practices for writing a README?
  - answer: Place it in the repository root, the docs directory, or the hidden .github directory. GitHub surfaces a README from any of those three locations on the repository page, so any one of these three places will make the README appear on GitHub.
    question: Where does the README file go in a GitHub repository?
  - answer: You do not need a table of contents for a short file. GitHub already generates an outline from your section headings, so a hand-written table of contents only earns its place when the README gets long enough to justify one.
    question: Do I need a table of contents in my README?
  - answer: Star and fork badges do not show a project is maintained; they measure popularity, not maintenance. Recent commits show whether anyone is still working on the project, so compare any status claim in the README with the actual commit history.
    question: Do star and fork badges show a project is maintained?
  - answer: It is never safe to put API keys or passwords in a README. Anyone who can read the README can read the secret, and it stays in git history after you delete it. Use placeholders, rotate any exposed key, and add a SECURITY.md file that says where to report vulnerabilities.
    question: Is it safe to put API keys or passwords in a README?
publishDate: "2026-10-23"
tags:
  - readme
  - github
  - documentation
  - open source
  - markdown
  - repository maintenance
title: "README best practices: what to include and what to cut"
---

**README best practices start with the first screen: what the project does, how to install it, its license, and a sign that someone maintains it. Further sections depend on project size and reader intent. Never put credentials in a README, and treat it as a claim the repository history can contradict, so an active label should match recent commits.**

You're about to publish a repository, or you've just landed on one from search and have a minute to decide whether to depend on it. README best practices cover both situations: one reader writes the file, the other judges it.

After reading, you'll be able to choose which sections to keep, write the core ones from sample Markdown, check how GitHub renders them, and keep the README truthful as the repository changes.

> **Key Takeaways**
> - Contribution rules and vulnerability reporting belong in separate files. The README links to them in one line.
> - Stars measure popularity; commits measure maintenance. A badge for one doesn't stand in for the other.
> - A roadmap section shows intent, not activity. Recent commits show activity.
> - Rerun your install steps in a clean directory on a fixed trigger, such as every release.

## Which README sections are required, and which are optional?

A README needs four sections, and every other section is optional until a specific reader needs it. The core is what someone requires to evaluate the project and start using it: a title with a one-line description, install steps, usage, and a license.

![Image generated](https://api.birchseek.com/m/Fp8IME48gwb88u-t_BAl8ndIqm_GdDwe.webp)

| Section | Weekend script | Library others install | Team or organization project | Reader who needs it |
| --- | --- | --- | --- | --- |
| Title, install, usage, license | Required | Required | Required | Evaluate, install |
| Table of contents | Skip | Add when it scrolls | Add | Evaluate |
| Features, tech stack, architecture | Skip | Features only | Add | Evaluate, contribute |
| Tests, roadmap, credits | Skip | Tests if you take pull requests | Add | Contribute |
| Contributing and security | Skip | Link to files | Link to files | Contribute |
| Badges | Skip | Build status | Build status | Evaluate |

Prune with one test per section: name the reader who would miss it if it were gone. If you can't name one, cut it.

In "Categorizing the Content of GitHub README Files," [Prana et al.](https://arxiv.org/abs/1802.06997) open from the point that README files shape a developer's first impression of a repository and document the project. They then sort README content into categories. That gives you a vocabulary for what READMEs contain, not a ruling on what each one must contain.

GitHub's own file handling supports a short README. GitHub treats files such as CONTRIBUTING, CODE_OF_CONDUCT and SECURITY as [community health files](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/creating-a-default-community-health-file). Contribution rules and vulnerability reporting can live in those files, and the README only links to them.

For a 40-line backup script, write the core four, which takes a few minutes, and stop. If you later publish it as a library, add a features list, then add a link to `CONTRIBUTING.md` once a stranger asks how to send a patch.

## What do README best practices look like in Markdown?

README best practices in Markdown come down to four short blocks near the top: a title with description, install steps, a usage example, and a license. Each sample below uses placeholders, so swap in your own names.

1. Name the thing in the title, then say what it does and for whom in one line. Keep the logo and badge row below these two lines. A stranger should see at a glance whether the project is relevant.

```html
# PACKAGE_NAME

PACKAGE_NAME does WHAT for WHO, in one sentence.
```

2. Make install copy-pasteable and state prerequisites on the line above the command. If you fetch a raw file, check the repository's default branch first (the branch dropdown on the main page shows it) instead of assuming `main` or `master`.

```bash
# Requires RUNTIME_NAME VERSION_NUMBER
PACKAGE_MANAGER install PACKAGE_NAME

# Fetching a raw file? Use the default branch, not an assumed name:
curl -fsSL https://raw.githubusercontent.com/OWNER/REPO/DEFAULT_BRANCH/FILE_PATH -o FILE_PATH
```

3. Show the smallest working example: one command or call, plus what the reader should see. Leave the full tutorial out and link to longer docs instead.

```bash
PACKAGE_NAME INPUT_FILE
# Expected output: DESCRIBE_THE_RESULT
```

4. Name the license and link the `LICENSE` file in the repository. [GitHub's licensing docs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository) explain why the file matters: "For your repository to truly be open source, you'll need to license it so that others are free to use, change, and distribute the software."

```html
## License

PACKAGE_NAME is released under the [LICENSE_NAME](LICENSE).
```

Choose the license by what you want others to be allowed to do, not by what other projects picked.

Test the result by following your own install and usage blocks in a clean directory. If either step needs knowledge the README doesn't give, add it to that block.

## How does GitHub render a README file?

GitHub displays the README it finds in the `.github` directory, the repository root, or `docs`, and renders it as Markdown on the repository page. Relative links and image paths resolve against the repository, and GitHub generates an outline from the headings.

[GitHub Docs on READMEs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes) says a README placed in any of those three locations is recognized, and GitHub will "automatically surface your README to repository visitors." If a repository holds more than one, the docs say the file shown is chosen "from locations in the following order": `.github`, then the root, then `docs`.

A `docs/README.md` never appears if the root already has one.

Links and images can point at files in the repository. Write `docs/CONTRIBUTING.md` instead of a full URL, and GitHub transforms the path "based on whatever branch you're currently on," so it works on any branch.

GitHub also builds a navigation outline. The docs say it will "automatically generate a table of contents based on section headings" for rendered Markdown files, README files included. A hand-written table of contents is therefore optional, and a short README can skip it.

Images need one more check. [GitHub's basic formatting guide](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) supports the `<picture>` element, so you can show a different image per theme with a `prefers-color-scheme` media query. This example shows a dark variant, with a light image as the fallback:

```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/logo-dark.svg">
  <img alt="Project logo" src="docs/logo-light.svg">
</picture>
```

A single dark-on-transparent image has no background of its own. On GitHub's dark theme, the dark ink can disappear into the page. Open your README in both themes before you publish.

## What can README badges and screenshots tell a reader, and what can't they?

A README badge or screenshot reports one narrow fact, and each one has a limit you should name beside it.

1. **Build status badge:** shows the last pipeline result. It doesn't measure code quality, and it can point to a workflow that no longer runs.
2. **Version or package badge:** shows that a release was published. It doesn't show that anyone still maintains the project.
3. **Star and fork badges:** show popularity. They don't show maintenance, so don't treat them as credibility signals, whatever a tutorial says.
4. **Screenshot:** shows what the output looked like when you captured it. It goes stale the day the interface changes.
5. **Star-history chart (optional):** shows how stars accumulated over time. Stars measure popularity, not health, so place the chart next to evidence from commits. gitdebt does not flag stars as real, fake or suspicious.

The chart is a plain image URL, so it renders without JavaScript. The `<picture>` element swaps in the `theme=light` and `theme=dark` variants, and the surrounding link keeps `?ref=readme`.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark">
    <img alt="Star history chart for OWNER/REPO" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light">
  </picture>
</a>
```

The [gitdebt README embeds](https://gitdebt.com/badges) are plain public image URLs that need no account and no token. Copy the snippet from that page and swap `OWNER/REPO` for your repository.

Decoration hurts when it pushes the install command below the first screen. A badge wall, a heavy GIF or a run of emoji headings can do it.

A reader deciding whether to depend on you wants the install line and the license first. Cut any asset that moves them down.

## How do you handle security and credentials in a README?

Handle security in a README by keeping every secret out of it and pointing readers to a security policy instead. That rules out real passwords, tokens and keys, and also "demo" logins that might be real. If a demo needs an account, explain how a reader creates their own.

[GitHub Docs on secret scanning](https://docs.github.com/en/code-security/secret-scanning/introduction/about-secret-scanning) explains why: a secret committed to a repository can be used by anyone who can read the repository. On a public repository, that means everyone.

Show placeholders instead, and commit only the example file:

```text
# .env.example (committed)
API_KEY=your-api-key-here
DATABASE_URL=postgres://user:password@localhost:5432/dbname
```

Tell readers to copy it to `.env`, fill in their own values, and keep `.env` in `.gitignore` so the real file never enters version control.

If a secret has already been committed, [GitHub's guidance on removing sensitive data](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository) says to treat any committed secret as compromised and change it.

Rotate the secret first. Deleting the line in a later commit doesn't erase it from history.

A heading called Security with nothing under it tells a reader nothing. Put one thing there: a link to a policy.

[GitHub Docs on security policies](https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository) says a `SECURITY.md` file gives people instructions for reporting vulnerabilities in your project.

Write yours so it asks for a private report, not a public issue. Then link it from the README in one line:

```html
## Security
Report vulnerabilities as described in [SECURITY.md](SECURITY.md). Please don't open a public issue.
```

## What does a README need when someone is deciding to depend on your project?

A README for someone deciding to depend on your project needs four things on the first screen: what it does, how to install it, the license, and a sign that someone maintains it.

That reader isn't admiring your formatting. They're deciding whether you're a risk in their lockfile.

If your README implies an active project, the commit history should agree. gitdebt keeps two sources apart: stars show what the crowd did, and commits show what the maintainers did. Neither stands in for the other.

A roadmap section is sometimes read as proof that a project is active. A roadmap shows intent, and only commits show work.

| Topic | README can state | Only the repository shows |
| --- | --- | --- |
| Install steps | The commands | Whether they work today |
| "Actively maintained" | The phrase | Recent commits |
| Supported versions | A list | Release history |

Read the README for what the authors intend. Read the repository for evidence.

### Annotated example: angular/angular

Open the README for `angular/angular` and check four criteria in order:

1. The first screen says what the project does.
2. There is an install command, or a link to one.
3. The license is named.
4. Any status wording, such as a maintenance claim, is present.

Then open the [gitdebt report for angular/angular](https://gitdebt.com/angular/angular) and compare any status wording from step 4 with the maintenance reading. gitdebt takes that reading from the direction of the commit trend, as described on the [gitdebt methodology page](https://gitdebt.com/about), and not from stars.

If the wording and the trend agree, the claim holds. If they disagree, trust the history and look closer.

Health readings are directional signals, not verdicts. A flat commit trend can describe a finished, stable library as easily as an abandoned one. Read the numbers on the day you decide, because they change.

## How do you keep a README accurate after launch?

You keep a README accurate by running a short checklist on a fixed trigger, such as every release or the first day of each quarter, and fixing whatever fails. Each check below names the failure it catches.

1. **Install steps.** Clone into an empty directory, check out the repository's actual default branch (don't assume `main`), and run the commands as written. This catches renamed commands, missing dependencies and version pins that no longer exist.
2. **Links.** Click every link, including relative paths to files and images. This catches 404s and docs that moved.
3. **Badges.** Open the target of each badge. This catches badges that point to a deleted workflow, an unpublished package or a service that's gone.
4. **Screenshots.** Put each image beside the current interface. This catches removed buttons, renamed menus and old branding.
5. **Status wording.** Compare a line like "actively maintained" with the recent commit history. This catches a claim the repository itself contradicts.
6. **Roadmap.** Check each item against what shipped. This catches finished work still listed as planned and abandoned work still listed as upcoming.
7. **License section.** Compare it with the `LICENSE` file. This catches a README that names one license while the file says another.

For check 5, open the project's report and read [the commit trend](https://gitdebt.com/about) for its recent direction. The [apache/airflow report](https://gitdebt.com/apache/airflow) shows what that looks like. Reword the status line if the two disagree.

Put the trigger in your release checklist, not in a promise to "keep it updated". A seven-item pass takes minutes, and a calendar reminder makes it happen.

## Frequently asked questions

### What are the best practices for writing a README?

Answer a new reader's questions fast: what the project does, how to install it, how to use it, what license applies, and whether it's maintained. Keep required sections short, put install above decoration, never include credentials, and check the README against the repository regularly.

### Where does the README file go in a GitHub repository?

Put it in the repository root, the `docs` directory, or the hidden `.github` directory. [GitHub surfaces a README](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes) from any of those three locations on the repository page.

### Do I need a table of contents in my README?

Not for a short file. GitHub generates an outline from your section headings, so a hand-written table of contents only earns its place when the README gets long.

### Do star and fork badges show a project is maintained?

Stars measure popularity, not maintenance. Recent commits show whether anyone is still working on the project, so compare any status claim in the README with the commit history.

### Is it safe to put API keys or passwords in a README?

Never. Anyone who can read the README can read the secret, and it stays in git history after you delete it. Use placeholders, rotate any exposed key, and add a [`SECURITY.md` file](https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository) that says where to report vulnerabilities.

## What to do with your README this week

These README best practices fit into three time boxes.

- **Today:** Trim the README to the core sections: title with description, install, usage and license. Then run the install command against the default branch and fix what breaks.
- **This week:** Add a `LICENSE` file and link a `SECURITY.md`. If you want one, add a single star-history embed.
- **Each release:** Run the maintenance checklist: install steps, links, badges and status statements.

Open the [report page](https://gitdebt.com/report) on [gitdebt](https://gitdebt.com/), enter your `owner/repo`, and read the maintenance reading beside what your README says. [Every public repository report works without an account](https://gitdebt.com/about). A repository nobody has analyzed yet is queued, and the real result appears at the same page.

Stars say popular, commits say alive, and your README should agree with the commits.

## Sources

[Categorizing the Content of GitHub README Files](https://arxiv.org/abs/1802.06997) · [Creating a default community health file - GitHub Docs](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/creating-a-default-community-health-file) · [Licensing a repository - GitHub Docs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository) · [About READMEs - GitHub Docs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes) · [Adding a security policy to your repository - GitHub Docs](https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository) · [Analyze a GitHub repository](https://gitdebt.com/about)
