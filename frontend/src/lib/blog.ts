import { getCollection, type CollectionEntry } from "astro:content";

export type BlogPost = CollectionEntry<"blog">;

/**
 * Whether any post file exists, decided without asking the content store.
 *
 * `getCollection()` warns on every call for a collection with no entries, and
 * an empty blog is this site's normal starting state, not a mistake. The glob
 * is only counted; its loaders are never called, so no post is imported here.
 */
const POST_FILES = Object.keys(
  import.meta.glob("../content/blog/*.md", { query: "?raw" }),
);

/** Milliseconds since the epoch; a date-only value is read as UTC midnight. */
export function postTime(value: string): number {
  return Date.parse(value);
}

/** The calendar day as the publisher wrote it, `YYYY-MM-DD`. */
export function postDay(value: string): string {
  return value.slice(0, 10);
}

/** Every post that is not a draft, newest first, then by title. */
export async function publishedPosts(): Promise<BlogPost[]> {
  if (POST_FILES.length === 0) return [];
  const posts = await getCollection("blog", ({ data }) => data.draft !== true);
  return posts.sort(
    (a, b) =>
      postTime(b.data.publishDate) - postTime(a.data.publishDate) ||
      a.data.title.localeCompare(b.data.title),
  );
}
