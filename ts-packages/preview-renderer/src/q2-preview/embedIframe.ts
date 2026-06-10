/**
 * Embedded-example deck iframe helpers for q2-preview (bd-kjrpya2d).
 *
 * The Rust `.embed-example-iframe` transform emits the deck as a
 * `RawBlock(html)` `<iframe class="embed-example-iframe" src="…">`
 * (see `crates/quarto-core/src/transforms/example_embed.rs`). In
 * `q2 render` the `src` resolves against `_site/` on disk; in
 * `q2 preview` the page renders in-browser with no server, so the
 * deck must be inlined from the VFS instead — mirroring how images
 * flow through the parent-side asset manifest (`assetWalker.ts`).
 *
 * This module owns the (regex-based) tag scanning shared by the two
 * sides of that flow:
 *   - `assetWalker.buildAssetManifest` calls {@link extractEmbedIframeSrcs}
 *     to discover which deck srcs to read from the VFS + mint blob URLs for.
 *   - `blocks/RawBlock` calls {@link rewriteEmbedIframeSrcs} to swap each
 *     deck iframe's `src` to its manifest blob URL before injecting the HTML.
 *
 * Both sides scan the SAME Rust-emitted HTML, so the `src` strings line
 * up as manifest keys with no separate normalization.
 */

/** Class the Rust render puts on the embedded-deck `<iframe>`. */
export const EMBED_IFRAME_CLASS = 'embed-example-iframe';

/** Match each `<iframe …>` opening tag. */
const IFRAME_TAG = /<iframe\b[^>]*>/gi;
/** Does an iframe opening tag carry the embed class? */
const HAS_EMBED_CLASS = new RegExp(
  `\\bclass\\s*=\\s*["'][^"']*\\b${EMBED_IFRAME_CLASS}\\b`,
  'i',
);
/** Capture an iframe's `src` attribute value. */
const SRC_ATTR = /\bsrc\s*=\s*["']([^"']*)["']/i;

/** External (network/data) URLs are never read from the VFS. */
function isExternal(url: string): boolean {
  return (
    url.startsWith('http://') ||
    url.startsWith('https://') ||
    url.startsWith('data:') ||
    url.startsWith('blob:') ||
    url.startsWith('//')
  );
}

/**
 * Collect the (non-external) `src` of every embedded-deck iframe in
 * `html`. Order-preserving, de-duplicated within the call.
 */
export function extractEmbedIframeSrcs(html: string): string[] {
  if (!html.includes(EMBED_IFRAME_CLASS)) return [];
  const out: string[] = [];
  const seen = new Set<string>();
  for (const m of html.matchAll(IFRAME_TAG)) {
    const tag = m[0];
    if (!HAS_EMBED_CLASS.test(tag)) continue;
    const srcM = tag.match(SRC_ATTR);
    const src = srcM?.[1];
    if (!src || isExternal(src) || seen.has(src)) continue;
    seen.add(src);
    out.push(src);
  }
  return out;
}

/**
 * Rewrite each embedded-deck iframe's `src` in `html` to its manifest
 * entry (a blob URL minted by the asset walker). Iframes whose `src`
 * is absent from the manifest — external URLs, or a VFS miss — are left
 * untouched so they keep their original behaviour.
 */
export function rewriteEmbedIframeSrcs(
  html: string,
  manifest: Record<string, string>,
): string {
  if (!html.includes(EMBED_IFRAME_CLASS)) return html;
  return html.replace(IFRAME_TAG, (tag) => {
    if (!HAS_EMBED_CLASS.test(tag)) return tag;
    const srcM = tag.match(SRC_ATTR);
    const src = srcM?.[1];
    if (!src) return tag;
    const mapped = manifest[src];
    if (!mapped) return tag;
    return tag.replace(SRC_ATTR, (_m, p1prefix?: string) => {
      // Rebuild `src="<mapped>"`, preserving the original quote style.
      void p1prefix;
      const quote = tag.match(/\bsrc\s*=\s*(["'])/i)?.[1] ?? '"';
      return `src=${quote}${mapped}${quote}`;
    });
  });
}
