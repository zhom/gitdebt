/**
 * Bump when renderer markup changes. Cloudflare intentionally caches public
 * media for a day; a stable revision keeps on-site previews and newly copied
 * README embeds off stale edge objects without changing renderer bytes.
 */
export const MEDIA_RENDER_REVISION = "14";
