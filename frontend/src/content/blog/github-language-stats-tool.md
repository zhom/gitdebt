---
description: "Which GitHub language stats tool should you use? It depends on the unit: the API and language bar report bytes, tokei counts lines. Learn to pull a repo's mix."
faq:
  - answer: "Open the repository page and read the language bar, or call the REST API languages endpoint for owner/repo, which returns bytes of code per language. For a line count instead of GitHub's measure, run tokei on a checkout. Bytes and lines can rank the same languages differently."
    question: How do I see the language breakdown of a GitHub repository?
  - answer: "The GitHub language bar does not count lines of code, because GitHub's language figures come from bytes. The API documents each language's value as a number of bytes of code, so a language written with long lines weighs more than its line count suggests. Compare percentages only within one unit."
    question: Does the GitHub language bar count lines of code?
  - answer: "The two tools measure different things and filter different files. GitHub's language figures come from bytes, and its Linguist excludes documentation from language stats. Tokei reports files, code, comment and blank lines grouped by language. Expect different percentages from the same checkout."
    question: Why does GitHub show a different language percentage than tokei?
  - answer: "A tool that reads only public data can't include private repositories, so their languages never appear in its numbers. Some profile cards offer a count_private=true option for the owner's own account. To check a public repository, queue a report with its owner/repo slug."
    question: Can a language stats tool include private repositories?
publishDate: "2026-11-08"
tags:
  - github
  - language stats
  - linguist
  - tokei
  - readme cards
  - rest api
title: "GitHub language stats tool: How to pick bytes or lines"
---

**There is no single github language stats tool; the right one depends on the unit and the scope. The [REST API languages endpoint](https://docs.github.com/en/rest/repos/repos#list-repository-languages) reports bytes of code, [tokei](https://github.com/XAMPPRocky/tokei) counts lines, and README cards display one of those numbers. A language mix describes the code and says nothing about whether a project is healthy.**

This problem belongs to developers judging a dependency such as `vitejs/vite` and to maintainers showing their own project's makeup. Both see the language bar first. It invites two wrong readings: that the percentages measure how much code was written in each language, and that they say something about project quality.

After reading, you can pull one repository's breakdown, show it in a README and name what it leaves out.

> **Key Takeaways**
> - Bytes and lines can rank the same repository's languages differently, so label the unit on every chart.
> - The language bar covers only countable files on the default branch; documentation, vendored and generated files drop out.
> - Tools built on public data never see private repositories.

## What does a GitHub language stats tool actually measure?

GitHub language stats break a codebase down by programming language, and the answer changes with the unit: bytes of code, number of files, or lines of code. The answer also changes with scope, since GitHub's language bar reports shares of one repository while a profile-wide aggregate combines many.

Bytes measure how much storage the code takes. A file count gives each file one vote, however large it is. Lines count rows of code, so formatting matters: a file with very long lines has many bytes but few lines.

Any github language stats tool picks one of these units, and the units can disagree on the same repository. A few very large files in one language and hundreds of small files in another put the first language ahead by bytes and the second ahead by file count. By lines, the order depends on how each language is formatted.

A per-repository breakdown describes one codebase. That is what you need when you judge a single dependency such as `vitejs/vite`.

A profile-wide aggregate adds many repositories into one set of shares. One large repository, or a pile of small experiments, can dominate the total, so the result describes a person's portfolio rather than any project in it.

Language popularity rankings measure something else again. The [`emmanuel-keller/github-language-statistics` README](https://github.com/emmanuel-keller/github-language-statistics/blob/master/README.md), for example, counts the "Number of active repositories in GitHub Per language and per month". That count across many repositories shows how widely a language is used on the platform, and nothing about what one codebase contains.

A repository's makeup answers "what is this project written in?" A popularity ranking answers "which languages do people use?" Keep the two questions apart when you read any github language stats tool.

## How does GitHub decide which languages a repository uses?

GitHub decides a repository's languages by running its open source Linguist library over the repository's files and turning the detected languages into shares of bytes. [GitHub Docs says](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-repository-languages), "GitHub uses the open source Linguist library to determine file languages for syntax highlighting and repository statistics."

Linguist looks at each file, decides what it is, and either files it under a language or sets it aside. The [Linguist README](https://github.com/github-linguist/linguist) says the library is used "to detect blob languages, ignore binary or vendored files, suppress generated files in diffs, and generate language breakdown graphs."

The unit behind the bar is bytes. The [REST API documentation](https://docs.github.com/en/rest/repos/repos#list-repository-languages) says "The value shown for each language is the number of bytes of code written in that language." The bar shows each language's share of that total, so one language with a few large files can outrank another with many small ones.

The bar follows one branch. GitHub Docs says, "Language statistics will update after you push changes to your default branch."

```text
special-vendored-path/* linguist-vendored
```

The bar is therefore a share of bytes in the detected, countable files of the default branch. A byte count measures size only, so it can't say how much effort went into each language or which part of the code matters most.

## How do you get the language breakdown of one public repository?

You get one public repository's language breakdown by reading the language bar on its page, then calling the REST API languages endpoint for exact byte counts.

1. Open `github.com/facebook/react` and look at the colored bar in the sidebar. It shows shares only, with no counts, so treat it as a quick look rather than a measurement.
2. Call the [REST API languages endpoint](https://docs.github.com/en/rest/repos/repos#list-repository-languages) from a terminal:

   ```bash
   curl -s https://api.github.com/repos/facebook/react/languages
   ```

 The response is one JSON object with one key per detected language and that language's byte count as the value.

   ```json
   {
     "JavaScript": js_bytes,
     "HTML": html_bytes,
     "CSS": css_bytes
   }
   ```

 Public repositories need no token for this read. For a private repository, send a token that has access to it.
3. Convert bytes to percentages. Add all the values, which are bytes rather than lines or files, then divide each language's bytes by that total and multiply by 100.
4. For lines instead, open the report at `gitdebt.com/facebook/react` and find the language lines chart. [gitdebt builds it](https://gitdebt.com/about) with tokei over the HEAD checkout, so it counts lines and can rank languages differently from the byte-based bar.
5. To embed it in your README, use the [gitdebt repository card](https://gitdebt.com/badges). The card is a plain image URL, so it renders without JavaScript:

   ```html
   <img src="https://api.gitdebt.com/api/repos/facebook/react/card.svg?theme=dark" alt="Repository card for facebook/react">
   ```

 Swap in your own `owner/repo`, and use `theme=light` for a light variant.

If the endpoint returns 600 bytes for one language and 400 for another, the total is 1,000 and the shares are 60% and 40%. Run the same division on every key in the real response for `facebook/react`. If the shares match the sidebar bar, you read the endpoint correctly.

A mismatch with gitdebt's language lines chart is expected, because bytes and lines are different units.

## How do you show a repository's language stats in a README?

You show a repository's language stats in a README by embedding a public image URL, or by calling the API and rendering the JSON yourself.

1. Copy the card URL. Swap OWNER/REPO in `https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg` for your slug, such as `vitejs/vite`. The card shows stars, forks, contributors, languages and a 90-day sparkline, and the [README embeds page](https://gitdebt.com/badges) lists the other assets.
2. Paste a `<picture>` snippet into README.md. One snippet carries both variants, `theme=light` and `theme=dark`.

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=dark">
    <source media="(prefers-color-scheme: light)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=light">
    <img alt="OWNER/REPO repository card" src="https://api.gitdebt.com/api/repos/OWNER/REPO/card.svg?theme=light">
  </picture>
</a>
```

3. Commit and open the README on GitHub. You should see the card in the page's current theme. A repository nobody has analyzed yet shows a placeholder and queues the work, so reload later; the real card replaces it at the same URL.

As a github language stats tool, the card summarizes languages only, so it can't replace a line count or a byte breakdown.

The API route returns JSON, which Markdown can't render. You would run your own script or workflow that fetches the numbers and writes a table or an image into the repository.

Choose the API route when you need exact figures in your own format. Choose the image when you want a card with no script to maintain.

[Copy the repository card snippet from the badges page](https://gitdebt.com/badges) and swap in your slug.

## Which GitHub language stats tool fits: bar, API, tokei or card?

The right github language stats tool depends on the unit you need: the bar and the API report bytes from Linguist, tokei and gitdebt report lines, and README cards display a figure computed elsewhere.

| Route | What it measures | Unit | Scope | Where it stops |
| --- | --- | --- | --- | --- |
| GitHub language bar | Languages detected by Linguist | Bytes, shown as percent | One repository, default branch | Shows shares only, no per-file or line detail |
| REST API languages endpoint | Languages detected per repository | Bytes per language | One repository | Returns raw bytes, you draw the chart yourself |
| tokei | Code, comments and blank lines | Lines | Any directory you run it on | Counts what is on disk now, nothing about history |
| README stats cards | A figure computed elsewhere | Depends on the card | Depends on the card | Displays a number, doesn't define it |
| gitdebt language lines | Lines per language from tokei | Lines | HEAD checkout of a shallow clone | Current checkout only, not effort or quality |

The bar and the API both come from Linguist's detection, so they agree with each other. The [tokei README](https://github.com/XAMPPRocky/tokei) describes a different unit: "Tokei will show the number of files, total lines within those files and code, comments, and blanks grouped by language."

If you run `tokei` in a local checkout, you get a line table that need not match GitHub's byte-based bar. A language with long lines can rank higher by bytes than by lines.

Stats cards only draw a result computed somewhere else. A user of one popular card [asked in an issue](https://github.com/anuraghazra/anuraghazra/issues/66) whether private repositories could be included, after setting `count_private=true`. The answer depends on how the card computes its figure, so check that before trusting the number.

gitdebt's language lines chart is a line count from tokei over the HEAD checkout of a shallow clone.

The chart reflects the current checkout only and counts lines, so it measures neither history, effort nor quality. Read it beside GitHub's bar and label the unit on each.

Each github language stats tool route has a best use:

- GitHub language bar: a quick look at one repository's makeup before you open the code.
- REST API endpoint: scripts that need exact byte counts or a custom chart.
- tokei: a line-based breakdown of code you have checked out, including comments and blanks.
- README stats cards: showing a number in a profile or README once you know where it comes from.
- gitdebt language lines: a line-based view of any public repository without cloning it yourself.

## What can't a GitHub language stats tool tell you about a repository?

Language stats can't tell you what is in a private repository, and they can't tell you whether public code is maintained, owned or healthy, because a language percentage only describes the files.

A public image URL or a public API call sees public repositories only. In an [issue on the anuraghazra stats card project](https://github.com/anuraghazra/anuraghazra/issues/66), one user wrote, "Most of my contributions are from private repositories, is it possible to include them in the count?" Counting private code takes the owner's own access, which a public embed doesn't have.

A repository's language bar looks the same whether its last commit landed yesterday or three years ago. The percentage also doesn't show who owns the code or how often files get repaired.

Those answers come from a separate source, the git history. [gitdebt](https://gitdebt.com/) pairs a repository's star history with four health readings from its commit history:

- Maintenance: recent commit activity.
- Ownership: how many contributors write half the commits.
- Repair load: the share of file changes labeled as fixes.
- Debt markers: how the comments that flag unfinished work move over time.

Each reading is a directional signal to follow up on. Ownership concentration shows where knowledge has pooled; it is not a judgment about any person.

Keep these readings next to the language mix on the page, and never fold one into the other. They come from different data and answer different questions.

Stars record what the crowd did, and the [frontend frameworks star history comparison](https://gitdebt.com/compare/frontend-frameworks) shows that curve for the major projects.

For the dependency decision, the language mix answers "what is it written in," and the commit history answers "is it alive."

## Frequently asked questions

### How do I see the language breakdown of a GitHub repository?

Open the repository page and read the language bar, or call the [REST API languages endpoint](https://docs.github.com/en/rest/repos/repos#list-repository-languages) for `owner/repo`, which returns bytes of code per language. For a line count instead, run tokei on a checkout. Bytes and lines can rank the same languages differently.

### Does the GitHub language bar count lines of code?

No, GitHub's language figures come from bytes. The API documents each language's value as a number of bytes of code, so a language written with long lines weighs more than its line count suggests. Compare percentages only within one unit.

### Why does GitHub show a different language percentage than tokei?

The two tools measure different things and filter different files. [GitHub's Linguist](https://github.com/github-linguist/linguist/blob/main/docs/overrides.md) excludes documentation from language stats, while [tokei](https://github.com/XAMPPRocky/tokei) reports files, code, comment and blank lines grouped by language. Expect different percentages from the same checkout.

### Can a language stats tool include private repositories?

A tool that reads only public data can't include private repositories, so their languages never appear in its numbers. Some profile cards offer a `count_private=true` option for the owner's own account. 

## Next steps for reading a repository's language mix

The next steps are to label the unit on every language number, put a card in your README, and read commit history before you adopt a dependency.

Today, read the language bar on the repository you're evaluating, then pull the API breakdown for the same `owner/repo`. Write the unit beside each number, bytes or lines, so nobody compares them by mistake.

This week, add the repository card embed to your README. The [badges page](https://gitdebt.com/badges) offers repository cards in light and dark variants.

Before adopting a dependency, read its commit-history health readings next to the language mix, and keep them as two sources.

To run a report now, [open gitdebt.com/report](https://gitdebt.com/report), enter `owner/repo`, and read the language breakdown beside the health readings.

## Sources

[REST API endpoints for repositories: List repository…](https://docs.github.com/en/rest/repos/repos#list-repository-languages) · [XAMPPRocky/tokei](https://github.com/XAMPPRocky/tokei) · [README.md - emmanuel-keller/github-language-statistics](https://github.com/emmanuel-keller/github-language-statistics/blob/master/README.md) · [About repository languages - GitHub Docs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-repository-languages) · [github-linguist/linguist README](https://github.com/github-linguist/linguist) · [How gitdebt works: methodology and data sources](https://gitdebt.com/about) · [README badges, star-history charts and embeds](https://gitdebt.com/badges) · [Most used languages count from private repositories? #66](https://github.com/anuraghazra/anuraghazra/issues/66). Every other source is linked where the article cites it.
