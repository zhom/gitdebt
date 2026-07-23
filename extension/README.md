# gitdebt browser extension

A Manifest V3 extension for Chrome and Firefox that adds gitdebt star-history
and code-health charts to public GitHub repository pages.

## Behavior

- Injects into `https://github.com/{owner}/{repo}` and repository sub-pages.
- Confirms GitHub marks the repository public before contacting gitdebt.
- Sends one freshness ping when the user enters a public repository. The ping
  contains `{owner, repo}` and the visible star count when GitHub has rendered
  it.
- Fetches analysis asynchronously and polls while the backend prepares a cold
  repository.
- Shows star history first. Contributor, language, churn, bug-magnet, heatmap,
  TODO/FIXME, bus-factor, and commit-trend charts load only when expanded.
- Handles GitHub Turbo/PJAX navigation without duplicating panels or counting
  every file/issue navigation as another repository visit.
- Uses a Shadow DOM and GitHub's Primer color variables for visual isolation and
  light/dark/high-contrast support.

The extension does not request GitHub API access, inject remote code, run in
private browsing, or collect GitHub account details. See [PRIVACY.md](PRIVACY.md)
for the complete disclosure.

## Develop

Requirements: Node.js 20 or newer.

```bash
cd extension
npm ci
npm test
npm run lint
npm run start:firefox
# or: npm run start:chrome
```

The production extension always uses `https://api.gitdebt.com`. For local API
work, temporarily change `DEFAULT_API_BASE` in `content.js`; do not ship that
change.

## Package

```bash
npm run package
```

Packaging runs tests and `web-ext lint`, then writes:

```text
dist/gitdebt-chrome-<version>.zip
dist/gitdebt-firefox-<version>.zip
```

The archives contain the same cross-browser MV3 source. Chrome ignores
`browser_specific_settings`; Firefox uses it for the extension ID and built-in
data-collection consent.

When submitting to a store, upload the generated archive, not the working
`extension/` directory.

## Permissions

| Permission | Reason |
| --- | --- |
| GitHub content-script match | Read the current public repo slug/star count and add the panel |
| `storage` | Sync the on/off preference |
| `activeTab` | Enable “Open this repo” after the user opens the toolbar popup |

There are no API host permissions. Backend requests are normal credential-free
CORS requests to the fixed gitdebt API, and chart images load cross-origin.

## Files

```text
manifest.json
content.js
content.css
popup.html
popup.css
popup.js
icons/
test/
build.mjs
PRIVACY.md
```
