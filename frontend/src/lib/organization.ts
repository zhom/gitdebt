/**
 * The site's Organization entity, emitted on every page by the root layout.
 *
 * `sameAs` lists only profiles that exist and belong to gitdebt. Add one here
 * when it is live, never in advance.
 */

export const ORGANIZATION_SAME_AS = ["https://github.com/zhom/gitdebt"] as const;

export const ORGANIZATION_DESCRIPTION =
  "Star history and repository health for any public GitHub repository.";

export function organizationId(site: string): string {
  return `${site}/#organization`;
}

export function organizationJsonLd(site: string): Record<string, unknown> {
  return {
    "@context": "https://schema.org",
    "@type": "Organization",
    "@id": organizationId(site),
    name: "gitdebt",
    url: `${site}/`,
    logo: {
      "@type": "ImageObject",
      url: `${site}/icon-512.png`,
      contentUrl: `${site}/icon-512.png`,
      width: 512,
      height: 512,
    },
    image: `${site}/icon-512.png`,
    description: ORGANIZATION_DESCRIPTION,
    sameAs: [...ORGANIZATION_SAME_AS],
  };
}
