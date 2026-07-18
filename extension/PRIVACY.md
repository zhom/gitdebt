# gitdebt extension privacy disclosure

Last updated: 2026-07-18

The browser extension adds gitdebt analytics to public GitHub repository pages.
It does not run in private browsing and does not automatically contact gitdebt
for repositories that GitHub marks private.

## Data sent to gitdebt

When the extension enters a public GitHub repository, it sends:

- the public repository owner and name; and
- the visible star count, if the count is available in GitHub's page.

This data is sent over HTTPS to `https://api.gitdebt.com`. It is used to request
the repository's public charts, keep cached data fresh, enqueue missing public
repositories for analysis, and maintain aggregate repository popularity
counters.

The observed star count is an untrusted freshness hint and is not stored.
Aggregate repository views and public repository analytics may be retained, but
they are not tied to an extension account or installation identifier. The
extension does not create an account, set a cookie, or send a stable identifier.
As with any web request, the hosting provider receives standard request metadata
such as an IP address and user agent; the service may process it transiently for
rate limiting, security, and operational logs.

gitdebt does not sell extension data, use it for advertising, or collect
stargazer profiles, private repository contents, browsing outside GitHub
repositories, keystrokes, or page interactions.

## Data stored in the browser

The extension stores one boolean preference: whether the GitHub panel is
enabled. It uses the browser's `storage.sync` service, so the browser vendor may
sync that preference according to the user's browser-account settings and the
vendor's privacy policy.

## User controls

The toolbar popup can disable automatic panels and freshness pings immediately.
Users can also uninstall the extension. Opening a full report from the popup or
panel is an explicit navigation to `gitdebt.com`.

The service privacy policy is available at
[https://gitdebt.com/privacy](https://gitdebt.com/privacy).
