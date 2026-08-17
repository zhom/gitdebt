<!--
prompt-parity.md — the cross-language golden for the "Ask an agent" prompt.

What this is: the complete prompt gitdebt hands a coding agent, rendered in
every state that changes it — a repository with nothing measured, one with a
complete star history, one whose curve is historical star activity (with and
without a resolved total), and a profile with and without measured totals.

Two implementations render it: backend/src/agent_prompt.rs, asserted by
backend/tests/parity.rs, and frontend/src/lib/agent-prompt.ts, asserted by
frontend/scripts/embed-parity.test.mjs. Both compare byte for byte. The
frontend copy still backs the clipboard button while the Rust copy is what the
API serves, so a sentence reworded on one side and not the other would ship two
different prompts for the same repository.

A diff here means the two implementations have drifted. Fix the drift: work out
which side is wrong and change that code. Do not regenerate this file to make a
test pass.

Fixed inputs, no wall clock anywhere: site https://gitdebt.com,
api https://api.gitdebt.com, login LOGIN, and a synthetic star history of 600
daily points of +3 stars from 2013-03-09 followed by 90 daily points of +30.
OWNER/REPO is the placeholder slug, which is what selects the "resolve the
repository" opening; owner/repo is a resolved slug, the only way to reach the
other branch.

Format: one "===== BEGIN <label> =====" / "===== END <label> =====" pair per
rendered prompt with the prompt verbatim in between, blocks separated by one
blank line, single trailing newline.
-->

===== BEGIN repo OWNER/REPO — nothing measured =====
# Add gitdebt analytics to the project's README

gitdebt (https://gitdebt.com) turns public GitHub data into plain image URLs: star history, a metrics badge, and repository-health charts. No account, token, build step, or GitHub Action is involved — the URLs below are already live and already pointed at this project.

## Step 0 — resolve the repository

Run `git remote get-url origin` and take the `owner/repo` slug from it. Replace every `OWNER/REPO` below with that slug, lowercased. If the remote is not a public GitHub repository, stop and say so: gitdebt only serves public repositories.

## Numbers

Do not write statistics into the README by hand — they go stale. The images below are regenerated from live data. If you need a figure for prose, read it from https://api.gitdebt.com/api/repos/OWNER/REPO/health.json.

## What to add

Paste these snippets as-is. They are complete, and they already carry light and dark variants plus alt text.

### 1. Metrics badge — the badge row directly under the project title, alongside CI and license badges

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?metrics=stars,forks&theme=dark" />
    <img alt="OWNER/REPO stars and forks" src="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?metrics=stars,forks&theme=light" />
  </picture>
</a>
```

### 2. Star history — a `## Star history` section near the bottom of the README, above License

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark" />
    <img alt="OWNER/REPO star history" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light" />
  </picture>
</a>
```

Give it a `## Star history` heading of its own if the README does not already have one.

### 3. Repository card (optional) — an About or Project status section, or a docs-site sidebar

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=dark" />
    <img alt="OWNER/REPO repository statistics" src="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=light" />
  </picture>
</a>
```

### 4. Repository-health charts (optional)

Each of these is the same `<picture>` shape as above, with a different path. Add at most two, and only where a reader would want them — typically a Project health or Contributing section. More than that reads as clutter and slows the page down.

- `https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg` — Commit calendar: Daily commit density across the last 52 weeks.
- `https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.svg` — Maintenance pulse: Commit volume over time, so a slowdown is visible rather than implied.
- `https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.svg` — Contributors: Who is actually landing commits, ranked, with avatars inlined.
- `https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.svg` — Ownership concentration: How few people write half the commits.
- `https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.svg` — Language activity: Lines of code by language across the analyzed history.
- `https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.svg` — File change frequency: The files the most commits touch, dependency manifests excluded.
- `https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.svg` — Fix-labelled changes: Files most often touched by commits whose message reads like a fix.
- `https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.svg` — TODO/FIXME movement: Whether known debt markers are being added or paid down.

### 5. Earned signal badge (optional)

Fetch `https://api.gitdebt.com/api/repos/OWNER/REPO/earned-badges.json` first. It returns one entry per signal with an `earned` boolean. Publish only the signals where `earned` is `true` — an unearned signal renders greyed out and claims nothing.

Badge URL shape: `https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=SIGNAL&theme=dark`, where `SIGNAL` is `active`, `community`, `momentum`, or `contributor-ready`.

## Rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static: motion nobody asked for is bad manners in somebody else's README, and it keeps the SVG and raster forms of an asset identical. Motion is an explicit opt-in — add `animate=1` to an SVG URL and it plays in a GitHub README. The `.gif` variant is for the surfaces that take raster alone: rasterizers, CSS `background-image`, and README renderers outside GitHub such as npm, PyPI, and Docker Hub, which show an SVG as a single static frame.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

## If the project already shows a star-history chart

Replace it in place. Keep the surrounding heading and prose; swap only the image and the link it wraps. Do not stack a second chart underneath. Search the repository for these markers:

- `star-history.com`
- `api.star-history.com`
- `starchart.cc`
- `stars.medv.io`
- `seladb/starhistory`

## Where else to look

- README.md at the repository root
- docs/index.md, docs/README.md, or a docs-site landing page
- website/ or site/ landing content, if the project publishes one
- CONTRIBUTING.md, where repository-health charts tell a contributor what they are joining
- profile/README.md, when the checkout is the account's `.github` repository, which is where an organization profile README lives

Only touch a file where the addition genuinely belongs. An unrelated docs page does not need a commit calendar.

## Tuning

Query parameters, if the defaults do not fit:

- `theme=light|dark` (every SVG and raster asset) — Bakes that palette into the output. Default is light.
- `animate=1` (SVG charts, cards, and badges) — Opts into motion, which plays in a GitHub README. Off by default; use the `.gif` variant for surfaces that show an SVG as a static frame.
- `from=YYYY-MM-DD&to=YYYY-MM-DD` (star-history charts) — Inclusive date window. An invalid or inverted range is a 400.
- `rebase=1` (star-history charts) — Starts every series at zero, so projects of different ages compare fairly.
- `type=date|timeline` (star-history charts) — Calendar dates, or days-since-first-star.
- `log=1` (star-history charts) — Logarithmic y axis.
- `repos=owner/repo,owner/repo` (/api/chart.svg) — Overlays several repositories on one chart.
- `metrics=stars,forks,downloads` (/badge.svg) — Chooses the chips and their order. `downloads` needs a published package.
- `signal=active|community|momentum|contributor-ready` (/badge.svg) — Renders one evidence-backed claim instead of raw metrics.
- `hide_border=1, hide_title=1, card_width=N` (/card.svg) — Trims the card for tight layouts.

## Finish

1. Request each URL you added and confirm it answers 200 with an image content type.
2. Confirm every image is wrapped in the link with `?ref=readme` and carries alt text.
3. Confirm you changed nothing else: no reformatting, no reflowed prose, no reordered badges beyond the one you inserted.
4. Report what you added and where, and link the full report: https://gitdebt.com/OWNER/REPO
===== END repo OWNER/REPO — nothing measured =====

===== BEGIN repo owner/repo — complete star history =====
# Add gitdebt analytics to the owner/repo README

gitdebt (https://gitdebt.com) turns public GitHub data into plain image URLs: star history, a metrics badge, and repository-health charts. No account, token, build step, or GitHub Action is involved — the URLs below are already live and already pointed at this project.

## What gitdebt has measured

- 4,500 GitHub stars (+2,700 in 90 days, +900 in 30), running ahead of its lifetime pace.
- Star history begins Mar 2013.

Use these numbers if you write prose around the images. Do not invent others. Every figure is re-checkable at https://api.gitdebt.com/api/repos/owner/repo/health.json and https://api.gitdebt.com/api/repos/owner/repo/stars.json.

## What to add

Paste these snippets as-is. They are complete, and they already carry light and dark variants plus alt text.

### 1. Metrics badge — the badge row directly under the project title, alongside CI and license badges

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=dark" />
    <img alt="owner/repo stars and forks" src="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=light" />
  </picture>
</a>
```

### 2. Star history — a `## Star history` section near the bottom of the README, above License

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=dark" />
    <img alt="owner/repo star history" src="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=light" />
  </picture>
</a>
```

Give it a `## Star history` heading of its own if the README does not already have one.

### 3. Repository card (optional) — an About or Project status section, or a docs-site sidebar

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=dark" />
    <img alt="owner/repo repository statistics" src="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=light" />
  </picture>
</a>
```

### 4. Repository-health charts (optional)

Each of these is the same `<picture>` shape as above, with a different path. Add at most two, and only where a reader would want them — typically a Project health or Contributing section. More than that reads as clutter and slows the page down.

- `https://api.gitdebt.com/api/repos/owner/repo/stats/heatmap.svg` — Commit calendar: Daily commit density across the last 52 weeks.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/commit-trend.svg` — Maintenance pulse: Commit volume over time, so a slowdown is visible rather than implied.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/contributors.svg` — Contributors: Who is actually landing commits, ranked, with avatars inlined.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/bus-factor.svg` — Ownership concentration: How few people write half the commits.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/lines.svg` — Language activity: Lines of code by language across the analyzed history.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/top-files.svg` — File change frequency: The files the most commits touch, dependency manifests excluded.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/bug-magnets.svg` — Fix-labelled changes: Files most often touched by commits whose message reads like a fix.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/todo-trend.svg` — TODO/FIXME movement: Whether known debt markers are being added or paid down.

### 5. Earned signal badge (optional)

Fetch `https://api.gitdebt.com/api/repos/owner/repo/earned-badges.json` first. It returns one entry per signal with an `earned` boolean. Publish only the signals where `earned` is `true` — an unearned signal renders greyed out and claims nothing.

Badge URL shape: `https://api.gitdebt.com/api/repos/owner/repo/badge.svg?signal=SIGNAL&theme=dark`, where `SIGNAL` is `active`, `community`, `momentum`, or `contributor-ready`.

## Rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static: motion nobody asked for is bad manners in somebody else's README, and it keeps the SVG and raster forms of an asset identical. Motion is an explicit opt-in — add `animate=1` to an SVG URL and it plays in a GitHub README. The `.gif` variant is for the surfaces that take raster alone: rasterizers, CSS `background-image`, and README renderers outside GitHub such as npm, PyPI, and Docker Hub, which show an SVG as a single static frame.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

## If the project already shows a star-history chart

Replace it in place. Keep the surrounding heading and prose; swap only the image and the link it wraps. Do not stack a second chart underneath. Search the repository for these markers:

- `star-history.com`
- `api.star-history.com`
- `starchart.cc`
- `stars.medv.io`
- `seladb/starhistory`

## Where else to look

- README.md at the repository root
- docs/index.md, docs/README.md, or a docs-site landing page
- website/ or site/ landing content, if the project publishes one
- CONTRIBUTING.md, where repository-health charts tell a contributor what they are joining
- profile/README.md, when the checkout is the account's `.github` repository, which is where an organization profile README lives

Only touch a file where the addition genuinely belongs. An unrelated docs page does not need a commit calendar.

## Tuning

Query parameters, if the defaults do not fit:

- `theme=light|dark` (every SVG and raster asset) — Bakes that palette into the output. Default is light.
- `animate=1` (SVG charts, cards, and badges) — Opts into motion, which plays in a GitHub README. Off by default; use the `.gif` variant for surfaces that show an SVG as a static frame.
- `from=YYYY-MM-DD&to=YYYY-MM-DD` (star-history charts) — Inclusive date window. An invalid or inverted range is a 400.
- `rebase=1` (star-history charts) — Starts every series at zero, so projects of different ages compare fairly.
- `type=date|timeline` (star-history charts) — Calendar dates, or days-since-first-star.
- `log=1` (star-history charts) — Logarithmic y axis.
- `repos=owner/repo,owner/repo` (/api/chart.svg) — Overlays several repositories on one chart.
- `metrics=stars,forks,downloads` (/badge.svg) — Chooses the chips and their order. `downloads` needs a published package.
- `signal=active|community|momentum|contributor-ready` (/badge.svg) — Renders one evidence-backed claim instead of raw metrics.
- `hide_border=1, hide_title=1, card_width=N` (/card.svg) — Trims the card for tight layouts.

## Finish

1. Request each URL you added and confirm it answers 200 with an image content type.
2. Confirm every image is wrapped in the link with `?ref=readme` and carries alt text.
3. Confirm you changed nothing else: no reformatting, no reflowed prose, no reordered badges beyond the one you inserted.
4. Report what you added and where, and link the full report: https://gitdebt.com/owner/repo
===== END repo owner/repo — complete star history =====

===== BEGIN repo owner/repo — approximate star history =====
# Add gitdebt analytics to the owner/repo README

gitdebt (https://gitdebt.com) turns public GitHub data into plain image URLs: star history, a metrics badge, and repository-health charts. No account, token, build step, or GitHub Action is involved — the URLs below are already live and already pointed at this project.

## What gitdebt has measured

- 4,500 GitHub stars (+2,700 in 90 days, +900 in 30), running ahead of its lifetime pace.
- The star curve is historical star activity, not a net-star series: it records star actions and cannot see unstars. Describe it as star activity, never as net stars.
- Star history begins Mar 2013.

Use these numbers if you write prose around the images. Do not invent others. Every figure is re-checkable at https://api.gitdebt.com/api/repos/owner/repo/health.json and https://api.gitdebt.com/api/repos/owner/repo/stars.json.

## What to add

Paste these snippets as-is. They are complete, and they already carry light and dark variants plus alt text.

### 1. Metrics badge — the badge row directly under the project title, alongside CI and license badges

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=dark" />
    <img alt="owner/repo stars and forks" src="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=light" />
  </picture>
</a>
```

### 2. Star history — a `## Star history` section near the bottom of the README, above License

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=dark" />
    <img alt="owner/repo star history" src="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=light" />
  </picture>
</a>
```

Give it a `## Star history` heading of its own if the README does not already have one.

### 3. Repository card (optional) — an About or Project status section, or a docs-site sidebar

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=dark" />
    <img alt="owner/repo repository statistics" src="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=light" />
  </picture>
</a>
```

### 4. Repository-health charts (optional)

Each of these is the same `<picture>` shape as above, with a different path. Add at most two, and only where a reader would want them — typically a Project health or Contributing section. More than that reads as clutter and slows the page down.

- `https://api.gitdebt.com/api/repos/owner/repo/stats/heatmap.svg` — Commit calendar: Daily commit density across the last 52 weeks.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/commit-trend.svg` — Maintenance pulse: Commit volume over time, so a slowdown is visible rather than implied.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/contributors.svg` — Contributors: Who is actually landing commits, ranked, with avatars inlined.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/bus-factor.svg` — Ownership concentration: How few people write half the commits.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/lines.svg` — Language activity: Lines of code by language across the analyzed history.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/top-files.svg` — File change frequency: The files the most commits touch, dependency manifests excluded.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/bug-magnets.svg` — Fix-labelled changes: Files most often touched by commits whose message reads like a fix.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/todo-trend.svg` — TODO/FIXME movement: Whether known debt markers are being added or paid down.

### 5. Earned signal badge (optional)

Fetch `https://api.gitdebt.com/api/repos/owner/repo/earned-badges.json` first. It returns one entry per signal with an `earned` boolean. Publish only the signals where `earned` is `true` — an unearned signal renders greyed out and claims nothing.

Badge URL shape: `https://api.gitdebt.com/api/repos/owner/repo/badge.svg?signal=SIGNAL&theme=dark`, where `SIGNAL` is `active`, `community`, `momentum`, or `contributor-ready`.

## Rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static: motion nobody asked for is bad manners in somebody else's README, and it keeps the SVG and raster forms of an asset identical. Motion is an explicit opt-in — add `animate=1` to an SVG URL and it plays in a GitHub README. The `.gif` variant is for the surfaces that take raster alone: rasterizers, CSS `background-image`, and README renderers outside GitHub such as npm, PyPI, and Docker Hub, which show an SVG as a single static frame.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

## If the project already shows a star-history chart

Replace it in place. Keep the surrounding heading and prose; swap only the image and the link it wraps. Do not stack a second chart underneath. Search the repository for these markers:

- `star-history.com`
- `api.star-history.com`
- `starchart.cc`
- `stars.medv.io`
- `seladb/starhistory`

## Where else to look

- README.md at the repository root
- docs/index.md, docs/README.md, or a docs-site landing page
- website/ or site/ landing content, if the project publishes one
- CONTRIBUTING.md, where repository-health charts tell a contributor what they are joining
- profile/README.md, when the checkout is the account's `.github` repository, which is where an organization profile README lives

Only touch a file where the addition genuinely belongs. An unrelated docs page does not need a commit calendar.

## Tuning

Query parameters, if the defaults do not fit:

- `theme=light|dark` (every SVG and raster asset) — Bakes that palette into the output. Default is light.
- `animate=1` (SVG charts, cards, and badges) — Opts into motion, which plays in a GitHub README. Off by default; use the `.gif` variant for surfaces that show an SVG as a static frame.
- `from=YYYY-MM-DD&to=YYYY-MM-DD` (star-history charts) — Inclusive date window. An invalid or inverted range is a 400.
- `rebase=1` (star-history charts) — Starts every series at zero, so projects of different ages compare fairly.
- `type=date|timeline` (star-history charts) — Calendar dates, or days-since-first-star.
- `log=1` (star-history charts) — Logarithmic y axis.
- `repos=owner/repo,owner/repo` (/api/chart.svg) — Overlays several repositories on one chart.
- `metrics=stars,forks,downloads` (/badge.svg) — Chooses the chips and their order. `downloads` needs a published package.
- `signal=active|community|momentum|contributor-ready` (/badge.svg) — Renders one evidence-backed claim instead of raw metrics.
- `hide_border=1, hide_title=1, card_width=N` (/card.svg) — Trims the card for tight layouts.

## Finish

1. Request each URL you added and confirm it answers 200 with an image content type.
2. Confirm every image is wrapped in the link with `?ref=readme` and carries alt text.
3. Confirm you changed nothing else: no reformatting, no reflowed prose, no reordered badges beyond the one you inserted.
4. Report what you added and where, and link the full report: https://gitdebt.com/owner/repo
===== END repo owner/repo — approximate star history =====

===== BEGIN repo owner/repo — approximate star history, total not resolved =====
# Add gitdebt analytics to the owner/repo README

gitdebt (https://gitdebt.com) turns public GitHub data into plain image URLs: star history, a metrics badge, and repository-health charts. No account, token, build step, or GitHub Action is involved — the URLs below are already live and already pointed at this project.

## What gitdebt has measured

- The star curve is historical star activity, not a net-star series: it records star actions and cannot see unstars. Describe it as star activity, never as net stars.
- Star history begins Mar 2013.

Use these numbers if you write prose around the images. Do not invent others. Every figure is re-checkable at https://api.gitdebt.com/api/repos/owner/repo/health.json and https://api.gitdebt.com/api/repos/owner/repo/stars.json.

## What to add

Paste these snippets as-is. They are complete, and they already carry light and dark variants plus alt text.

### 1. Metrics badge — the badge row directly under the project title, alongside CI and license badges

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=dark" />
    <img alt="owner/repo stars and forks" src="https://api.gitdebt.com/api/repos/owner/repo/badge.svg?metrics=stars,forks&theme=light" />
  </picture>
</a>
```

### 2. Star history — a `## Star history` section near the bottom of the README, above License

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=dark" />
    <img alt="owner/repo star history" src="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=light" />
  </picture>
</a>
```

Give it a `## Star history` heading of its own if the README does not already have one.

### 3. Repository card (optional) — an About or Project status section, or a docs-site sidebar

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=dark" />
    <img alt="owner/repo repository statistics" src="https://api.gitdebt.com/api/repos/owner/repo/card.svg?theme=light" />
  </picture>
</a>
```

### 4. Repository-health charts (optional)

Each of these is the same `<picture>` shape as above, with a different path. Add at most two, and only where a reader would want them — typically a Project health or Contributing section. More than that reads as clutter and slows the page down.

- `https://api.gitdebt.com/api/repos/owner/repo/stats/heatmap.svg` — Commit calendar: Daily commit density across the last 52 weeks.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/commit-trend.svg` — Maintenance pulse: Commit volume over time, so a slowdown is visible rather than implied.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/contributors.svg` — Contributors: Who is actually landing commits, ranked, with avatars inlined.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/bus-factor.svg` — Ownership concentration: How few people write half the commits.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/lines.svg` — Language activity: Lines of code by language across the analyzed history.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/top-files.svg` — File change frequency: The files the most commits touch, dependency manifests excluded.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/bug-magnets.svg` — Fix-labelled changes: Files most often touched by commits whose message reads like a fix.
- `https://api.gitdebt.com/api/repos/owner/repo/stats/todo-trend.svg` — TODO/FIXME movement: Whether known debt markers are being added or paid down.

### 5. Earned signal badge (optional)

Fetch `https://api.gitdebt.com/api/repos/owner/repo/earned-badges.json` first. It returns one entry per signal with an `earned` boolean. Publish only the signals where `earned` is `true` — an unearned signal renders greyed out and claims nothing.

Badge URL shape: `https://api.gitdebt.com/api/repos/owner/repo/badge.svg?signal=SIGNAL&theme=dark`, where `SIGNAL` is `active`, `community`, `momentum`, or `contributor-ready`.

## Rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static: motion nobody asked for is bad manners in somebody else's README, and it keeps the SVG and raster forms of an asset identical. Motion is an explicit opt-in — add `animate=1` to an SVG URL and it plays in a GitHub README. The `.gif` variant is for the surfaces that take raster alone: rasterizers, CSS `background-image`, and README renderers outside GitHub such as npm, PyPI, and Docker Hub, which show an SVG as a single static frame.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

## If the project already shows a star-history chart

Replace it in place. Keep the surrounding heading and prose; swap only the image and the link it wraps. Do not stack a second chart underneath. Search the repository for these markers:

- `star-history.com`
- `api.star-history.com`
- `starchart.cc`
- `stars.medv.io`
- `seladb/starhistory`

## Where else to look

- README.md at the repository root
- docs/index.md, docs/README.md, or a docs-site landing page
- website/ or site/ landing content, if the project publishes one
- CONTRIBUTING.md, where repository-health charts tell a contributor what they are joining
- profile/README.md, when the checkout is the account's `.github` repository, which is where an organization profile README lives

Only touch a file where the addition genuinely belongs. An unrelated docs page does not need a commit calendar.

## Tuning

Query parameters, if the defaults do not fit:

- `theme=light|dark` (every SVG and raster asset) — Bakes that palette into the output. Default is light.
- `animate=1` (SVG charts, cards, and badges) — Opts into motion, which plays in a GitHub README. Off by default; use the `.gif` variant for surfaces that show an SVG as a static frame.
- `from=YYYY-MM-DD&to=YYYY-MM-DD` (star-history charts) — Inclusive date window. An invalid or inverted range is a 400.
- `rebase=1` (star-history charts) — Starts every series at zero, so projects of different ages compare fairly.
- `type=date|timeline` (star-history charts) — Calendar dates, or days-since-first-star.
- `log=1` (star-history charts) — Logarithmic y axis.
- `repos=owner/repo,owner/repo` (/api/chart.svg) — Overlays several repositories on one chart.
- `metrics=stars,forks,downloads` (/badge.svg) — Chooses the chips and their order. `downloads` needs a published package.
- `signal=active|community|momentum|contributor-ready` (/badge.svg) — Renders one evidence-backed claim instead of raw metrics.
- `hide_border=1, hide_title=1, card_width=N` (/card.svg) — Trims the card for tight layouts.

## Finish

1. Request each URL you added and confirm it answers 200 with an image content type.
2. Confirm every image is wrapped in the link with `?ref=readme` and carries alt text.
3. Confirm you changed nothing else: no reformatting, no reflowed prose, no reordered badges beyond the one you inserted.
4. Report what you added and where, and link the full report: https://gitdebt.com/owner/repo
===== END repo owner/repo — approximate star history, total not resolved =====

===== BEGIN profile LOGIN — measured =====
# Add gitdebt profile analytics to LOGIN's profile README

gitdebt (https://gitdebt.com) renders aggregate public-repository statistics for an account as plain image URLs. No account, token, or GitHub Action is involved. A profile README lives in a repository named after the account itself — `LOGIN/LOGIN` for a user; for an organization, a repository named `.github` with the file at `profile/README.md`. Create it if it does not exist.

## What gitdebt has measured

- 90,120 stars across LOGIN's public repositories (42 repositories counted).

Re-checkable at https://api.gitdebt.com/api/users/LOGIN/stats.json.

## What to add

Paste these as-is; both carry light and dark variants.

### 1. Maintainer card — the top of a profile README, under the introduction

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=dark" />
    <img alt="LOGIN maintainer statistics" src="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=light" />
  </picture>
</a>
```

### 2. Aggregate star history — a profile README, below the card

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/chart.svg?theme=dark" />
    <img alt="Aggregate star history across LOGIN's public repositories" src="https://api.gitdebt.com/api/users/LOGIN/chart.svg?theme=light" />
  </picture>
</a>
```

### 3. Optional footprint charts

- `https://api.gitdebt.com/api/users/LOGIN/stats/contributions.svg` — Contribution footprint: Authored work in owned projects versus other people's projects.
- `https://api.gitdebt.com/api/users/LOGIN/stats/languages.svg` — Language footprint: Lines of code by language across every analyzed owned repository.
- `https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.svg` — Commit activity: Every commit landed in the last 52 weeks, summed across owned repos.

## Rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static: motion nobody asked for is bad manners in somebody else's README, and it keeps the SVG and raster forms of an asset identical. Motion is an explicit opt-in — add `animate=1` to an SVG URL and it plays in a GitHub README. The `.gif` variant is for the surfaces that take raster alone: rasterizers, CSS `background-image`, and README renderers outside GitHub such as npm, PyPI, and Docker Hub, which show an SVG as a single static frame.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

## Finish

1. Request each URL and confirm it answers 200 with an image content type.
2. Confirm every image keeps its link wrapper and alt text.
3. Report what you added, and link the full report: https://gitdebt.com/LOGIN
===== END profile LOGIN — measured =====

===== BEGIN profile LOGIN — nothing measured =====
# Add gitdebt profile analytics to LOGIN's profile README

gitdebt (https://gitdebt.com) renders aggregate public-repository statistics for an account as plain image URLs. No account, token, or GitHub Action is involved. A profile README lives in a repository named after the account itself — `LOGIN/LOGIN` for a user; for an organization, a repository named `.github` with the file at `profile/README.md`. Create it if it does not exist.

## What to add

Paste these as-is; both carry light and dark variants.

### 1. Maintainer card — the top of a profile README, under the introduction

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=dark" />
    <img alt="LOGIN maintainer statistics" src="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=light" />
  </picture>
</a>
```

### 2. Aggregate star history — a profile README, below the card

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/chart.svg?theme=dark" />
    <img alt="Aggregate star history across LOGIN's public repositories" src="https://api.gitdebt.com/api/users/LOGIN/chart.svg?theme=light" />
  </picture>
</a>
```

### 3. Optional footprint charts

- `https://api.gitdebt.com/api/users/LOGIN/stats/contributions.svg` — Contribution footprint: Authored work in owned projects versus other people's projects.
- `https://api.gitdebt.com/api/users/LOGIN/stats/languages.svg` — Language footprint: Lines of code by language across every analyzed owned repository.
- `https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.svg` — Commit activity: Every commit landed in the last 52 weeks, summed across owned repos.

## Rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static: motion nobody asked for is bad manners in somebody else's README, and it keeps the SVG and raster forms of an asset identical. Motion is an explicit opt-in — add `animate=1` to an SVG URL and it plays in a GitHub README. The `.gif` variant is for the surfaces that take raster alone: rasterizers, CSS `background-image`, and README renderers outside GitHub such as npm, PyPI, and Docker Hub, which show an SVG as a single static frame.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.

## Finish

1. Request each URL and confirm it answers 200 with an image content type.
2. Confirm every image keeps its link wrapper and alt text.
3. Report what you added, and link the full report: https://gitdebt.com/LOGIN
===== END profile LOGIN — nothing measured =====
