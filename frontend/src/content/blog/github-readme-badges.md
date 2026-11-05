---
description: "Which GitHub README badges belong in your repo? Add a short set you've earned, use GitHub's native CI badge, link each to proof, and audit them. Start here."
faq:
  - answer: GitHub README badges are small images, usually SVGs served by a service such as shields.io or by GitHub itself, embedded in a README with Markdown. Each one shows a live fact, such as build status, license, or latest release, and can link to the page that backs it up.
    question: What are GitHub README badges?
  - answer: Our editorial recommendation, not a GitHub rule, is to use about five at most for a library or CLI, one or two for an app or docs repo, and no status badges on a profile README. Each badge should tell a reader something they can verify.
    question: How many badges should a README have?
  - answer: For CI you do not need shields.io; GitHub has its own status badges. GitHub Docs says that a status badge shows whether a workflow is currently failing or passing. Use shields.io for license, release, stars, and static badges, knowing those depend on an outside image server.
    question: Do I need shields.io, or does GitHub have its own badges?
  - answer: "The image is most likely cached. GitHub Docs says that GitHub uses the open-source project Camo to host your images. Open the badge's own URL in a browser. If it shows the new status, wait and hard-reload, and confirm the image URL uses https."
    question: Why is my badge not updating after the status changed?
publishDate: "2026-11-05"
tags:
  - github
  - readme
  - badges
  - shields.io
  - documentation
  - open source
title: GitHub README badges worth adding and how to keep them honest
---

**GitHub README badges are [small images, embedded with Markdown](https://naereen.github.io/badges/), that report one fact a reader can verify. Pick a short set you've earned for your repository type, use GitHub's native workflow badge for CI, and link each badge to the page that proves it. Most badges depend on an external image service, so audit them now and then.**

Pasting a long row of GitHub README badges from a catalogue takes a minute. A few months later, some images are broken, some link to dead services, and some still advertise tests or maintenance the project no longer has.

A badge is a claim about your project, and unlike prose it can turn false or vanish without anyone noticing. After reading, you'll be able to build a badge URL, pick a minimal set for your repository type, and audit the badges you already have.

> **Key Takeaways**
> - A profile README answers "who is this"; a project README answers "can I trust and use this". They need different badges.
> - A badge can lag behind the real status, because GitHub proxies and caches README images and badge servers set their own cache headers.
> - Alt text should name what the badge reports, like "CI build status", not the brand.
> - Skip unearned signals: star counts on a new repo, or "maintained" without recent commits.

## How do you build a shields.io badge URL for a GitHub README?

A [static shields.io badge](https://shields.io/badges/static-badge) URL puts a label, a message and a color after `/badge/`, joined by dashes. A static badge shows exactly the text you type. A dynamic badge reads a live value from a service instead.

![Image generated](https://api.birchseek.com/m/mNnuSdY4xBpYpj7fPcEWfF706OelE_mC.webp)

Either way, the image comes from `img.shields.io`, a third-party host, not from GitHub.

1. **Start with the base.** The shields.io static badge documentation gives the pattern `<LABEL>-<MESSAGE>-<COLOR>` after `https://img.shields.io/badge/`.
2. **Fill in the three parts.** `build-passing-brightgreen` gives a badge reading "build" and "passing" in bright green. Nothing checks that this is true, which is why static badges go stale.
3. **Escape special characters.** A literal dash is written as `--` and a literal underscore as `__`. A single `_` or `%20` becomes a space. An unescaped dash inside a label splits it into the wrong parts.
4. **Add query parameters after `?`.** The docs list `style`, `logo`, `logoColor` and `label`, joined with `&`. Supported styles are `flat`, `flat-square`, `plastic`, `for-the-badge` and `social`.
5. **Wrap the URL in Markdown.** Put the image inside a link so readers can click through, and write alt text that states the status.

The label "unit-tests" contains a dash, so it is written `unit--tests`. This URL also sets a style and a logo:

```text
https://img.shields.io/badge/unit--tests-passing-brightgreen?style=flat-square&logo=github&logoColor=white
```

The README Markdown that embeds it, linked to a page you control:

```html
[![Unit tests: passing](https://img.shields.io/badge/unit--tests-passing-brightgreen?style=flat-square&logo=github&logoColor=white)](https://github.com/OWNER/REPO/actions)
```

Success looks like a green flat-square badge reading "unit-tests | passing" with a white GitHub logo. If it reads "unit" and "tests", you left the dash single.

## How do you add live status badges for CI, license, release, stars and issues?

You add live status badges by pasting an image URL from GitHub or shields.io into your README and wrapping it in a link to the page that backs the claim. [GitHub's formatting guide](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) says, "You can display an image by adding! and wrapping the alt text in [ ]."

Wrapping that image in a second link gives `[![ALT](IMAGE_URL)](LINK_URL)`. Catalogues skip the outer link, so readers can't check what the badge says.

1. **Add CI with GitHub's native badge.** [GitHub's workflow badge documentation](https://docs.github.com/en/actions/how-tos/monitor-workflows/add-a-status-badge) says, "A status badge shows whether a workflow is currently failing or passing." It describes a `badge.svg` image under `/actions/workflows/`, so no third-party host is involved.

 Swap `ci.yml` for your file in `.github/workflows`. Add `?branch=main` to report one branch only, since the docs show the branch option as `?branch=feature-1`. Link the badge to the workflow's Actions page, with alt text that states the result, such as "CI workflow status".

```html
[![CI workflow status](https://github.com/OWNER/REPO/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/OWNER/REPO/actions/workflows/ci.yml)
```

2. **Add license and latest release.** A license badge shows the detected license name, not whether your dependencies are compatible. A release badge shows the newest tag, not how current the code on `main` is.

```html
[![License](https://img.shields.io/github/license/OWNER/REPO)](https://github.com/OWNER/REPO/blob/HEAD/LICENSE)
[![Latest release](https://img.shields.io/github/v/release/OWNER/REPO)](https://github.com/OWNER/REPO/releases/latest)
```

3. **Add stars and issues only if they help the reader.** A stars badge shows a count, not whether the project is alive or how it grew. An issues badge shows the open count, not how fast anyone responds. For growth over time, a [star history chart for your README](https://gitdebt.com/badges) answers a different question.

```html
[![Stars](https://img.shields.io/github/stars/OWNER/REPO)](https://github.com/OWNER/REPO/stargazers)
[![Open issues](https://img.shields.io/github/issues/OWNER/REPO)](https://github.com/OWNER/REPO/issues)
```

4. **Check the result.** Preview the README. Each badge should render with live text, and clicking it should land on the file, list or run behind the number. A badge that opens your repository's home page is not linked correctly.

## Which README badges suit a library, CLI, app, docs repo or profile?

Pick README badges by what the repository ships: libraries and CLIs prove they install and build, apps and docs repos need one or two, and profiles carry no status badges at all.

| | Library or CLI | App | Docs repo | Profile README |
| --- | --- | --- | --- | --- |
| Reader's question | Does it install and build? | Is it live and usable? | Is the site current? | Who is this? |
| Keep | CI, registry version or release, license | CI or release, license, site or download link | Docs build or deploy, content license | Stack, contact, one identity link |
| Skip | Download counters unless earned | Star and issue counters | Version and package badges | Build, version, package |
| Count (editorial) | 3 to 5 | 2 to 3 | 1 to 2 | 3 to 5 |

The counts in the table and the ordering advice that follows are editorial recommendations, not rules from GitHub.

### Project READMEs: can I trust and use this?

For a library, show the native workflow status badge, the package version from the registry it installs from (npm, PyPI or crates.io), and the license. Together they tell a reader whether it builds, which release to install, and whether they may use it.

A CLI swaps in the latest release and the version badge for its install channel. Skip download counters unless the number is one you're glad to defend.

An app gets fewer badges because end users rarely read them. Show CI or release status, the license, and a link to the live site or download.

A docs repo usually needs only the build or deploy status and the content license.

### Placement and order

Put one row directly under the title. Order it status first (CI), then version or release, then license, then optional links.

Keep a project to roughly 3 to 5. Each extra badge dilutes the ones that matter, and a reader who sees eight pills stops checking any of them.

### Profile READMEs: who is this?

A profile README describes a person, so nothing ships and build, version or package badges have nothing to report. Keep a short row of stack, identity and contact badges instead.

Use each platform's current name and handle. Check the platform's own site before you copy a social badge from a catalogue, because older ones can carry outdated names.

A stack row needs exactly one closing parenthesis per badge. Check the logo slug on Simple Icons before you use it.

```html
![LABEL](https://img.shields.io/badge/LABEL-HEXCOLOR?logo=SLUG&logoColor=white)
```

## How do you put a chart in a README and match light and dark themes?

A theme-aware chart in a README is an image wrapped in a `<picture>` element, with one drawing for dark mode and one for light. [GitHub Docs on theme-specific images](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#specifying-the-theme-an-image-is-shown-to) describe pairing the HTML `<picture>` element with the `prefers-color-scheme` media feature to pick the image for each theme.

A star-history chart shows the shape of growth, which a single star count cannot.

1. **Copy the snippet, don't guess the path.** Open the [gitdebt README badges page](https://gitdebt.com/badges) for the exact copy-paste snippets and health-chart URLs.

 Every asset is a plain image URL such as `https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg`. You can swap in `chart.gif`, `card.svg`, `badge.svg`, `usage.svg` or `og.png`. No account, token, API key, build step or GitHub Action is needed.
2. **Add both themes.** Use `theme=dark` in the `<source>` and `theme=light` in the fallback `<img>`. Charts are static by default. Add `animate=1` only if you want an SVG to move.
3. **Wrap the picture in a link.** Put `?ref=readme` on the link, never on the image URL.
4. **Commit and check both themes.** Switch GitHub's appearance setting and reload. If nobody has analyzed the repository yet, you'll see a placeholder while the work is queued. The real chart replaces it at the same URL.

Here is the worked example with `facebook/react`:

```html
<a href="https://gitdebt.com/facebook/react?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/facebook/react/chart.svg?theme=dark">
    <img alt="Star history chart for facebook/react" src="https://api.gitdebt.com/api/repos/facebook/react/chart.svg?theme=light">
  </picture>
</a>
```

The images are edge-cached for a few hours by design, so a new chart may take a while to appear.

A star chart shows popularity over time, not whether a project is alive. Pair it with a commit-based health reading such as maintenance or ownership, and keep the two sources separate.

gitdebt's stars come from GitHub's own star history. Its health readings come from the commit history, and they're directional signals, not verdicts. gitdebt does not flag stars as fake, real or suspicious.

The [about page](https://gitdebt.com/about) explains how the readings are measured, and the [angular/angular report](https://gitdebt.com/angular/angular) shows star history beside them.

To try it, copy your snippet from the badges page and swap in your own `OWNER/REPO`.

## What alt text should a README badge have?

Badge alt text should name what the badge reports, such as "CI build status" or "Latest release", rather than the brand name alone. The image changes while the alt text does not, so avoid hardcoding a value like "passing" on a live badge. Keep it short, and make a linked badge's destination clear.

[GitHub's formatting guide](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) puts the alt text inside the square brackets, `![ALT](URL)`, and says, "Alt text is a short text equivalent of the information in the image."

[The W3C's guidance on informative images](https://www.w3.org/WAI/tutorials/images/informative/) says the text alternative "should convey the meaning or content that is displayed visually."

| Badge | Weak alt text | Better alt text |
| --- | --- | --- |
| Workflow status | GitHub Actions | CI build status |
| Package version | npm | npm package version |
| Star chart | chart | Star history of OWNER/REPO |

For a linked badge, treat the alt text as the link's label. "CI build status (workflow runs)" tells a screen reader user where the link goes.

Alt text can't report the current value of a live badge. If a status matters, say it in the README text too, and put any chart numbers that matter in a sentence beside the image.

## Why do GitHub README badges break or show stale status?

A GitHub README badge breaks or shows stale status because the image comes from a third-party server, passes through GitHub's image proxy, and can be cached at both steps.

[GitHub's page on anonymized URLs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) says, "To host your images, GitHub uses the open-source project Camo." The same page has a section titled "Removing an image from Camo's cache", so a changed badge can lag behind the service that draws it.

The badge server adds its own cache headers too. The image can sit in the origin and edge caches before GitHub even fetches it.

Every gitdebt embed is a plain, cacheable image URL, so the same applies there: don't expect an exact refresh time.

When a badge looks wrong, check in this order:

1. **Open the image URL directly in a browser.** If it fails there, the service is the problem, not GitHub.
2. **Check the link target.** A badge that renders but links to a 404 still misleads.
3. **Compare the path with the service's current docs.** Paths get renamed.
4. **Confirm the badged service still exists.** An archived or shut-down service can't report anything true.
5. **Check for https.** Use `https://` image URLs. Snippets copied from older lists may use `http://`, so verify that each one still resolves before you reuse it.

Every third-party badge is a dependency. If its host is down, renamed or gone, readers see a broken image where your claim should be.

For CI, GitHub's native workflow badge avoids that risk. It's generated on the same platform that hosts the repository, so there's no extra host to disappear.

Re-check every badge on a schedule, such as each quarter or at each release.

## How do you audit README badges so they stay honest?

You audit README badges by running seven checks on each one.

1. **Click every badge.** Confirm the link lands on a page that backs the claim. A build badge should open the workflow runs, not a project homepage.
2. **Remove badges for retired services.** Before keeping a badge for Atom's package manager (APM), Bower, Gitter or godoc.org, read the service's own announcement and note where it points now. If the service is archived or deprecated, delete the badge.
3. **Fix image URLs.** Change any `http://` image URL to `https://`, then reload the README in a private window.
4. **Ask what it adds.** If a reader can't learn anything they couldn't already see on the repository page, delete the badge. Decorative ones are noise.
5. **Verify social badges.** Confirm the platform name and handle are current before you reuse them. A copied badge can point to a renamed account.
6. **Check the alt text.** Each badge's alt text should name what it reports, such as "CI build status".
7. **Apply the honesty rule.** Don't advertise signals the project hasn't earned. That means no download or star counter on a brand-new repository, and no "maintained" badge with no recent commits.

For check 2, take a godoc.org documentation badge in a Go README. Open Go's official documentation, find the documentation site it currently recommends, and point the badge there instead of relying on memory.

For check 7, test a "maintained" claim against the git history. The maintenance reading in a [gitdebt repository report](https://gitdebt.com/) is built from commits, so you can set it beside the badge. It's a directional signal, not a verdict.

## Frequently asked questions

### What are GitHub README badges?

They are small images, usually SVGs served by a service such as shields.io or by GitHub itself, placed in a README with Markdown. Each one shows a fact such as build status, license or latest release, and can link to the page that backs it up.

### How many badges should a README have?

Our editorial recommendation, not a GitHub rule: about five at most for a library or CLI, one or two for an app or docs repo, and no status badges on a profile README. Each badge should tell a reader something they can verify.

### Do I need shields.io, or does GitHub have its own badges?

For CI you don't need shields.io, because GitHub's own workflow status badge shows whether a workflow is currently failing or passing. Use shields.io for license, release, stars and static badges, knowing those depend on an outside image server.

### Why is my badge not updating after the status changed?

The image is most likely cached, since GitHub serves README images through its Camo proxy. Open the badge's own URL in a browser. If it shows the new status, wait and hard-reload, and confirm the image URL uses https.

## What should you do with your badges this week?

Start by cutting the badges that fail the audit, then make the rest link to proof.

**Today:** delete every badge that fails the audit. Remove archived services, dead links and images that tell a reader nothing.

**This week:**

- Switch CI to the native workflow badge.
- Rewrite alt text to name what each badge reports, and change any `http://` image URL to `https://`.
- Add a star history chart next to a commit-based health reading, kept as two separate sources.

**Each quarter:** re-run the audit.

To add the chart, open [gitdebt.com/badges](https://gitdebt.com/badges), copy the snippet, and swap `OWNER/REPO` for your repository. It needs no account and no token. gitdebt is free and open source, with [its source code on GitHub](https://github.com/zhom/gitdebt), so trying it costs nothing.

## Sources

[List of Badges, in Markdown | badges](https://naereen.github.io/badges/) · [Static Badge | Shields.io](https://shields.io/badges/static-badge) · [Basic writing and formatting syntax - GitHub Docs](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) · [Adding a workflow status badge - GitHub Docs](https://docs.github.com/en/actions/how-tos/monitor-workflows/add-a-status-badge) · [README badges, star-history charts and embeds | gitdebt](https://gitdebt.com/badges) · [About anonymized URLs - GitHub Docs](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-anonymized-urls) · [gitdebt.com](https://gitdebt.com/) · [GitHub - zhom/gitdebt](https://github.com/zhom/gitdebt)
