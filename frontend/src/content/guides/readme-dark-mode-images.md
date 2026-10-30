---
title: "How to show light and dark mode images in a GitHub README"
metaTitle: "Light and dark mode images in a GitHub README"
description: "Show a different README image in light and dark mode with the picture element or GitHub's mode-only fragments, and test both themes before you push."
updatedDate: "2026-10-30"
order: 3
related:
  - add-star-history-chart-to-readme
  - github-profile-readme-stats
  - commit-activity-graph-readme
---

A logo or chart that looks right on a white README can disappear on a dark one. You can show each reader the version that fits their theme. You need two versions of the image and permission to edit the README.

## 1. Make a light and a dark version

Make one image for a light background and one for a dark background. Keep the same width and height, so the page does not move when the theme changes.

For files in your repository, commit both, for example `docs/logo-light.png` and `docs/logo-dark.png`.

For gitdebt charts, badges and cards, you do not make files. Every image URL takes `theme=light` or `theme=dark`, and the colors are baked into the image. There is no `theme=auto`, because an image cannot read the page it sits on.

```text
https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light
https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark
```

## 2. Write a picture element

GitHub's formatting docs state: "The `<picture>` HTML element is supported." Put the dark image in a `<source>` with a media query and the light image in the `<img>`:

```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/OWNER/REPO/main/docs/logo-dark.png" />
  <img alt="Project logo" src="https://raw.githubusercontent.com/OWNER/REPO/main/docs/logo-light.png" />
</picture>
```

The browser uses the first `<source>` whose media query matches. If none matches, it shows the `<img>`. So the `<img>` is the light version and also the image for any site that ignores `<picture>`. Put the alt text on the `<img>`. One alt text covers both versions.

`prefers-color-scheme` reports the color setting of the reader's browser or operating system. GitHub also has its own theme setting, under Settings, Appearance, "Theme mode". A reader who picks a dark GitHub theme on a light system can still get the light image. Step 4 gives the method that follows GitHub's setting instead.

## 3. Wrap it in a link

GitHub makes every README image a link to the image file. To send readers somewhere useful, put your own link around the `<picture>`:

```html
<a href="https://gitdebt.com/OWNER/REPO?ref=readme">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=dark" />
    <img alt="OWNER/REPO star history" src="https://api.gitdebt.com/api/repos/OWNER/REPO/chart.svg?theme=light" />
  </picture>
</a>
```

Leave a blank line before and after an HTML block in Markdown. Without it, the Markdown lines after the block are not converted.

## 4. Use the mode-only fragments to follow GitHub's theme

GitHub's stylesheet has rules for two URL fragments, `#gh-light-mode-only` and `#gh-dark-mode-only`. When a README image link ends in one of them, GitHub hides it in the other theme. The rules follow the "Theme mode" setting on GitHub. When that setting follows the system, they follow `prefers-color-scheme`.

This works for images stored in your repository, written as Markdown with relative paths:

```markdown
![Project logo](docs/logo-light.png#gh-light-mode-only)
![Project logo](docs/logo-dark.png#gh-dark-mode-only)
```

GitHub turns each image into a link to its file in the repository, and the link keeps the fragment, so the stylesheet can hide it. Sites without GitHub's stylesheet show both images, one after the other. GitHub's current formatting docs describe only the `<picture>` element.

## 5. Pick the method

| | `<picture>` | Mode-only fragments |
| --- | --- | --- |
| Follows | The browser or system color setting | The GitHub theme setting |
| Images from other hosts | Yes | Not reliable, because GitHub's link then points at its image proxy |
| Sites without GitHub's stylesheet | Show one image | Show both images |

Use `<picture>` for gitdebt images and for anything you link out of the README. Use the fragments for a logo or a screenshot stored in the repository when matching GitHub's own theme matters more to you.

## 6. Test both themes before you push

You do not have to change your system theme to test the `<picture>` version. In Chrome or Edge, open DevTools, open the command menu with Ctrl+Shift+P (Cmd+Shift+P on a Mac), run "Show Rendering", and set "Emulate CSS media feature prefers-color-scheme" to dark or light. Reload the README and check the image.

To test the fragments, change "Theme mode" in your GitHub appearance settings and reload.

## Troubleshooting

| What you see | Cause | Fix |
| --- | --- | --- |
| The light image on a dark GitHub page | `<picture>` follows the system setting, and the system is light | Expected. Use the fragments if you need GitHub's setting |
| Both images, one above the other | Fragments on a site without GitHub's stylesheet, or a fragment on an image from another host | Use `<picture>` |
| Dark text invisible on a dark page | A transparent image with dark lettering | Give the image its own background, or make a true dark version |
| A gitdebt image stays light | `theme` is missing or misspelled. Any value except `dark` gives light | Use `theme=dark` exactly |
| Markdown after the image is not converted | No blank line after the HTML block | Add the blank line |

The [README badges and embeds page](/badges) gives every gitdebt image as a ready `<picture>` snippet with both themes filled in.
