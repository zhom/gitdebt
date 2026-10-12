import { existsSync, readdirSync } from "node:fs";
import { defineCollection } from "astro:content";
import { glob, type Loader } from "astro/loaders";
import { z } from "astro/zod";

/**
 * The blog: one Markdown file per article, committed by an external publisher.
 *
 * The contract a publisher writes against is the file path and the front
 * matter below, nothing else. `src/content/blog/{slug}.md` is published at
 * `/blog/{slug}`; the slug comes from the file name alone, so a `slug` key in
 * the front matter cannot move a post to a URL its file name does not state.
 * Keys the schema does not name are kept rather than refused, because the
 * publisher may add metadata this site has no use for yet.
 */

const BLOG_BASE = "./src/content/blog";

const DATE_ONLY = /^(\d{4})-(\d{2})-(\d{2})$/;
const RFC3339 =
  /^\d{4}-\d{2}-\d{2}[Tt ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:[Zz]|[+-]\d{2}:\d{2})$/;

function isCalendarDate(value: string): boolean {
  const match = DATE_ONLY.exec(value);
  if (!match) return false;
  const [year, month, day] = match.slice(1).map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  return (
    date.getUTCFullYear() === year &&
    date.getUTCMonth() === month - 1 &&
    date.getUTCDate() === day
  );
}

/**
 * A publish or update date, normalised to a string.
 *
 * YAML turns an unquoted `2026-10-12` (and an unquoted timestamp) into a Date
 * before the schema sees it, while a quoted one stays a string, so both shapes
 * are accepted. A date-only value stays `YYYY-MM-DD`; a timestamp keeps the
 * publisher's own offset when it arrived as a string, so the calendar day a
 * reader sees is the day the publisher meant.
 */
const postDate = z
  .union([z.string(), z.date()])
  .transform((value, context) => {
    if (value instanceof Date) {
      if (Number.isNaN(value.getTime())) {
        context.addIssue({ code: "custom", message: "Invalid date" });
        return z.NEVER;
      }
      const iso = value.toISOString();
      return iso.endsWith("T00:00:00.000Z") ? iso.slice(0, 10) : iso;
    }
    const trimmed = value.trim();
    if (isCalendarDate(trimmed)) return trimmed;
    if (
      RFC3339.test(trimmed) &&
      isCalendarDate(trimmed.slice(0, 10)) &&
      !Number.isNaN(Date.parse(trimmed))
    ) {
      return trimmed;
    }
    context.addIssue({
      code: "custom",
      message: "Expected a YYYY-MM-DD date or an RFC 3339 timestamp",
    });
    return z.NEVER;
  });

/** `My Post.md` and `my-post.md` both publish at `/blog/my-post`. */
function slugFromFileName(entry: string): string {
  const slug = entry
    .replace(/\.md$/i, "")
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  if (!slug) {
    throw new Error(`Blog post file name has no usable slug: ${entry}`);
  }
  return slug;
}

/**
 * The stock glob loader, quiet when the folder holds no post yet.
 *
 * An empty blog is a normal state for this site, not a misconfiguration, and
 * the stock loader warns about it on every build. Builds here must finish
 * without warnings, so the empty case is answered before the loader runs.
 */
function blogLoader(): Loader {
  const markdown = glob({
    base: BLOG_BASE,
    pattern: "*.md",
    generateId: ({ entry }) => slugFromFileName(entry),
  });
  return {
    name: "gitdebt:blog",
    load: async (context) => {
      const directory = new URL(`${BLOG_BASE}/`, context.config.root);
      const hasPosts =
        existsSync(directory) &&
        readdirSync(directory).some((file) => file.endsWith(".md"));
      if (!hasPosts) {
        context.store.clear();
        return;
      }
      await markdown.load(context);
    },
  };
}

const blog = defineCollection({
  loader: blogLoader(),
  schema: z.looseObject({
    title: z.string(),
    description: z.string(),
    publishDate: postDate,
    updatedDate: postDate.optional(),
    tags: z.array(z.string()).default([]),
    faq: z
      .array(z.object({ question: z.string(), answer: z.string() }))
      .optional(),
    draft: z.boolean().optional(),
  }),
});

export const collections = { blog };
