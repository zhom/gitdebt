<div align="center">
  <img src="assets/gitdebt-logo.svg" alt="gitdebt logo" width="112">
  <h1>gitdebt</h1>
  <p>Star-history charts and repository-health analytics for GitHub repositories.</p>
  <a href="https://gitdebt.com">gitdebt.com</a>
</div>

<p align="center">
  <a href="https://github.com/zhom/gitdebt/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/zhom/gitdebt/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/zhom/gitdebt/blob/main/LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-111111"></a>
  <a href="https://gitdebt.com/zhom/gitdebt?ref=readme"><img alt="gitdebt stars" src="https://api.gitdebt.com/api/repos/zhom/gitdebt/badge.svg"></a>
</p>

<p align="center">
  <a href="https://gitdebt.com/zhom/gitdebt?ref=readme">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/zhom/gitdebt/chart.svg?theme=dark">
      <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/zhom/gitdebt/chart.svg?theme=light">
      <img alt="Star history of gitdebt, charted by gitdebt" src="https://api.gitdebt.com/api/repos/zhom/gitdebt/chart.svg?theme=dark">
    </picture>
  </a>
</p>

<p align="center">
  <a href="https://gitdebt.com/zhom/gitdebt?ref=readme">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/zhom/gitdebt/card.svg?theme=dark">
      <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/zhom/gitdebt/card.svg?theme=light">
      <img alt="gitdebt repository card" src="https://api.gitdebt.com/api/repos/zhom/gitdebt/card.svg?theme=dark">
    </picture>
  </a>
</p>

<div align="center"><sub>Every image above is gitdebt charting itself, served live.</sub></div>

## Features

- Star history for one repo or up to eight overlaid, with date windows, log
  scale, and CSV/JSON export.
- Repository-health charts from git history: bug magnets, churn, contributors,
  commit heatmaps, TODO/FIXME trend, bus factor, language lines.
- Package downloads (npm, crates.io, PyPI, Docker) plotted against stars.
- Embeddable README assets: SVG and animated GIF charts, cards, and badges.
- Stores star timestamps, not stargazer profiles. No account scoring or
  labelling.

> [!NOTE]
> New star history is rebuilt from historical data, which records public star
> events but not unstars, so those curves are approximate public star activity.
> Repositories cached from GitHub earlier keep their exact snapshots.

## Usage

Open [gitdebt.com](https://gitdebt.com) and enter `owner/repo`.

To embed a theme-aware chart in your own README:

```html
<a href="https://gitdebt.com/owner/repo?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=light">
    <img alt="Star history of owner/repo" src="https://api.gitdebt.com/api/repos/owner/repo/chart.svg?theme=dark">
  </picture>
</a>
```

Swap `chart.svg` for `chart.gif` (animated), `card.svg`, `badge.svg`, or
`og.png`.

## License

[MIT](LICENSE) — see [CONTRIBUTING.md](CONTRIBUTING.md) to contribute and
[SECURITY.md](SECURITY.md) to report a vulnerability.
