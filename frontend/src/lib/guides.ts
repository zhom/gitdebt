import { getCollection, type CollectionEntry } from "astro:content";

export type Guide = CollectionEntry<"guides">;

/** Every guide, in the order the index lists them. */
export async function allGuides(): Promise<Guide[]> {
  const guides = await getCollection("guides");
  return guides.sort(
    (a, b) => a.data.order - b.data.order || a.id.localeCompare(b.id),
  );
}
