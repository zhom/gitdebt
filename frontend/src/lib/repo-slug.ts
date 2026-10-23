/**
 * What a person types or pastes when they mean a repository, reduced to the
 * `owner/repo` every route and asset accepts.
 *
 * People paste the address bar more often than they type a slug: a GitHub URL,
 * often a deep one (`/tree/main/src`), a clone URL, or the slug itself. All of
 * them name the same repository, so all of them are accepted, and only a value
 * that names no repository is refused.
 */

/** `owner/repo`, the only shape every asset route accepts. */
export const SLUG_RE = /^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)$/;

export function parseRepoSlug(value: string): string | null {
  const path = value
    .trim()
    .replace(/^git@github\.com:/i, "")
    .replace(/^(?:https?:\/\/)?(?:www\.)?github\.com\//i, "")
    .replace(/[?#].*$/, "")
    .replace(/^\/+/, "");
  const [owner = "", repo = ""] = path.split("/");
  const slug = `${owner}/${repo.replace(/\.git$/i, "")}`;
  return SLUG_RE.test(slug) ? slug : null;
}
