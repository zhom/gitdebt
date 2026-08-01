<!--
embed-parity.md — the cross-language golden for gitdebt's README embed catalog.

What this is: every asset gitdebt can put in somebody else's README — for a
repository and for a profile — with the catalog metadata the /badges page and
the API's Markdown both read off it, and the exact snippet each one produces.

Two implementations render it: backend/src/agent_embeds.rs, asserted by
backend/tests/parity.rs, and frontend/src/lib/readme-embeds.ts, asserted by
frontend/scripts/embed-parity.test.mjs. Both compare byte for byte.

A diff here means the two implementations have drifted. Fix the drift: work out
which side is wrong and change that code. Do not regenerate this file to make a
test pass — that only lets the API and the /badges page disagree quietly.

Fixed inputs: slug OWNER/REPO, login LOGIN, site https://gitdebt.com,
api https://api.gitdebt.com.

Format, deliberately trivial to reproduce in any language:
  "# Repository assets — OWNER/REPO", then one section per asset in catalog
  order, then "# Profile assets — LOGIN" and its sections, then "# Rules" with
  EMBED_RULES as "- " bullets, one line each. An asset section is "## " + the
  asset id, a blank line, one "- key: value" line per catalog field, one
  "- url(FORMAT): " line per advertised format, a blank line, then the
  published snippet fenced in its own language. Sections are separated by one
  blank line and the file ends with a single newline.
-->

# Repository assets — OWNER/REPO

## badge-metrics

- name: Metrics badge
- purpose: Stars and forks in one compact chip, served from gitdebt's cache.
- placement: the badge row directly under the project title, alongside CI and license badges
- group: headline
- themed: true
- formats: svg, png, webp
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?metrics=stars,forks
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/badge.png?metrics=stars,forks
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/badge.webp?metrics=stars,forks

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?metrics=stars,forks&theme=dark" />
    <img alt="OWNER/REPO stars and forks" src="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?metrics=stars,forks&theme=light" />
  </picture>
</a>
```

## badge-signal

- name: Earned signal badge
- purpose: One evidence-backed claim — actively maintained, community powered, star momentum, or contributor readiness.
- placement: the badge row, but only for signals the repository has actually earned
- group: headline
- themed: true
- formats: svg, png, webp
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=active
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/badge.png?signal=active
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/badge.webp?signal=active

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=active&theme=dark" />
    <img alt="OWNER/REPO actively maintained" src="https://api.gitdebt.com/api/repos/OWNER/REPO/badge.svg?signal=active&theme=light" />
  </picture>
</a>
```

## chart

- name: Star history
- purpose: The full cumulative star curve, served from Postgres.
- placement: a `## Star history` section near the bottom of the README, above License
- group: headline
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/chart.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/chart.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/chart.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark" />
    <img alt="OWNER/REPO star history" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light" />
  </picture>
</a>
```

## card

- name: Repository card
- purpose: Stars, forks, contributors, languages, and a 90-day sparkline in one panel.
- placement: an About or Project status section, or a docs-site sidebar
- group: headline
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/card.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/card.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/card.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=dark" />
    <img alt="OWNER/REPO repository statistics" src="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=light" />
  </picture>
</a>
```

## usage

- name: Stars versus downloads
- purpose: Star growth against package-registry downloads, for projects that publish one.
- placement: next to the star-history chart, when the project ships a package
- group: headline
- themed: true
- formats: svg, png, webp
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/usage.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/usage.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/usage.webp

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/usage.svg?theme=dark" />
    <img alt="OWNER/REPO stars versus package downloads" src="https://api.gitdebt.com/api/repos/OWNER/REPO/usage.svg?theme=light" />
  </picture>
</a>
```

## heatmap

- name: Commit calendar
- purpose: Daily commit density across the last 52 weeks.
- placement: a Project health or Contributing section, where a prospective contributor is already reading
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg?theme=dark" />
    <img alt="OWNER/REPO commit activity calendar" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/heatmap.svg?theme=light" />
  </picture>
</a>
```

## commit-trend

- name: Maintenance pulse
- purpose: Commit volume over time, so a slowdown is visible rather than implied.
- placement: a Project health or Contributing section, where a prospective contributor is already reading
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.svg?theme=dark" />
    <img alt="OWNER/REPO commit trend" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/commit-trend.svg?theme=light" />
  </picture>
</a>
```

## contributors

- name: Contributors
- purpose: Who is actually landing commits, ranked, with avatars inlined.
- placement: a Project health or Contributing section, where a prospective contributor is already reading
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.svg?theme=dark" />
    <img alt="OWNER/REPO contributors" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/contributors.svg?theme=light" />
  </picture>
</a>
```

## bus-factor

- name: Ownership concentration
- purpose: How few people write half the commits.
- placement: a Project health or Contributing section, where a prospective contributor is already reading
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.svg?theme=dark" />
    <img alt="OWNER/REPO bus factor" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bus-factor.svg?theme=light" />
  </picture>
</a>
```

## lines

- name: Language activity
- purpose: Lines of code by language across the analyzed history.
- placement: a Project health or Contributing section, where a prospective contributor is already reading
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.svg?theme=dark" />
    <img alt="OWNER/REPO language activity" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/lines.svg?theme=light" />
  </picture>
</a>
```

## top-files

- name: File change frequency
- purpose: The files the most commits touch, dependency manifests excluded.
- placement: a Project health or Contributing section, where a prospective contributor is already reading
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.svg?theme=dark" />
    <img alt="OWNER/REPO file change frequency" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/top-files.svg?theme=light" />
  </picture>
</a>
```

## bug-magnets

- name: Fix-labelled changes
- purpose: Files most often touched by commits whose message reads like a fix.
- placement: a Project health or Contributing section, where a prospective contributor is already reading
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.svg?theme=dark" />
    <img alt="OWNER/REPO fix-labelled changes" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/bug-magnets.svg?theme=light" />
  </picture>
</a>
```

## todo-trend

- name: TODO/FIXME movement
- purpose: Whether known debt markers are being added or paid down.
- placement: a Project health or Contributing section, where a prospective contributor is already reading
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.svg
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.webp
- url(gif): https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.gif

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.svg?theme=dark" />
    <img alt="OWNER/REPO recent TODO and FIXME movement" src="https://api.gitdebt.com/api/repos/OWNER/REPO/stats/todo-trend.svg?theme=light" />
  </picture>
</a>
```

## og

- name: Social preview
- purpose: A 1200x630 PNG for link unfurls on social platforms and chat apps.
- placement: a docs-site `og:image` meta tag — not the README, where it would be redundant
- group: social
- themed: false
- formats: png, webp
- url(png): https://api.gitdebt.com/api/repos/OWNER/REPO/og.png
- url(webp): https://api.gitdebt.com/api/repos/OWNER/REPO/og.webp

```markdown
[![OWNER/REPO on gitdebt](https://api.gitdebt.com/api/repos/OWNER/REPO/og.png)](https://gitdebt.com/OWNER/REPO?ref=readme)
```

# Profile assets — LOGIN

## card

- name: Maintainer card
- purpose: Aggregate public-repository totals for the account in one compact panel.
- placement: the top of a profile README, under the introduction
- group: headline
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/users/LOGIN/card.svg
- url(png): https://api.gitdebt.com/api/users/LOGIN/card.png
- url(webp): https://api.gitdebt.com/api/users/LOGIN/card.webp
- url(gif): https://api.gitdebt.com/api/users/LOGIN/card.gif

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=dark" />
    <img alt="LOGIN maintainer statistics" src="https://api.gitdebt.com/api/users/LOGIN/card.svg?theme=light" />
  </picture>
</a>
```

## chart

- name: Aggregate star history
- purpose: One curve summing star growth across every public repository owned.
- placement: a profile README, below the card
- group: headline
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/users/LOGIN/chart.svg
- url(png): https://api.gitdebt.com/api/users/LOGIN/chart.png
- url(webp): https://api.gitdebt.com/api/users/LOGIN/chart.webp
- url(gif): https://api.gitdebt.com/api/users/LOGIN/chart.gif

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/chart.svg?theme=dark" />
    <img alt="Aggregate star history across LOGIN's public repositories" src="https://api.gitdebt.com/api/users/LOGIN/chart.svg?theme=light" />
  </picture>
</a>
```

## contributions

- name: Contribution footprint
- purpose: Authored work in owned projects versus other people's projects.
- placement: a profile README, in place of a generic contribution-count widget
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/users/LOGIN/stats/contributions.svg
- url(png): https://api.gitdebt.com/api/users/LOGIN/stats/contributions.png
- url(webp): https://api.gitdebt.com/api/users/LOGIN/stats/contributions.webp
- url(gif): https://api.gitdebt.com/api/users/LOGIN/stats/contributions.gif

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/stats/contributions.svg?theme=dark" />
    <img alt="LOGIN contribution footprint" src="https://api.gitdebt.com/api/users/LOGIN/stats/contributions.svg?theme=light" />
  </picture>
</a>
```

## languages

- name: Language footprint
- purpose: Lines of code by language across every analyzed owned repository.
- placement: a profile README, next to the contribution footprint
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/users/LOGIN/stats/languages.svg
- url(png): https://api.gitdebt.com/api/users/LOGIN/stats/languages.png
- url(webp): https://api.gitdebt.com/api/users/LOGIN/stats/languages.webp
- url(gif): https://api.gitdebt.com/api/users/LOGIN/stats/languages.gif

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/stats/languages.svg?theme=dark" />
    <img alt="LOGIN language footprint" src="https://api.gitdebt.com/api/users/LOGIN/stats/languages.svg?theme=light" />
  </picture>
</a>
```

## commit-activity

- name: Commit activity
- purpose: Every commit landed in the last 52 weeks, summed across owned repos.
- placement: a profile README, as the activity strip
- group: health
- themed: true
- formats: svg, png, webp, gif
- url(svg): https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.svg
- url(png): https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.png
- url(webp): https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.webp
- url(gif): https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.gif

```html
<a href="https://gitdebt.com/LOGIN?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.svg?theme=dark" />
    <img alt="LOGIN commit activity" src="https://api.gitdebt.com/api/users/LOGIN/stats/commit-activity.svg?theme=light" />
  </picture>
</a>
```

## og

- name: Social preview
- purpose: A 1200x630 PNG for link unfurls.
- placement: a personal site's `og:image` meta tag
- group: social
- themed: false
- formats: png, webp
- url(png): https://api.gitdebt.com/api/users/LOGIN/og.png
- url(webp): https://api.gitdebt.com/api/users/LOGIN/og.webp

```markdown
[![LOGIN on gitdebt](https://api.gitdebt.com/api/users/LOGIN/og.png)](https://gitdebt.com/LOGIN?ref=readme)
```

# Rules

- No account, token, or API key is involved. Every URL is a plain public image.
- Themes are baked into each asset because GitHub renders README images against the reader's OS preference, not the page. Publish both variants with an HTML `<picture>` element, or pick one explicitly with `theme=light` / `theme=dark`. There is no `theme=auto`.
- Published snippets are static. Motion is opt-in: add `animate=1` to an SVG URL, or use the `.gif` variant where one exists, because GitHub strips SVG animation from README images in several contexts.
- Keep the surrounding link and its `?ref=readme` parameter. Attribution lives on the link; the image URL stays plain so CDNs can cache it.
- Do not add cache-busting query parameters. Media is edge-cached for a few hours by design and refreshes on its own.
- Alt text is not optional. Say what the image shows, not "chart".
- A repository nobody has analyzed yet renders a placeholder frame and queues the work instead of failing. Load the page once, or wait a few minutes, and the real chart replaces it at the same URL.
