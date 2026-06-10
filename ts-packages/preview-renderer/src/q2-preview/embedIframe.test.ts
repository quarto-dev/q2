/**
 * Tests for the embedded-deck iframe scan/rewrite helpers (bd-kjrpya2d).
 */
import { describe, it, expect } from 'vitest';
import { extractEmbedIframeSrcs, rewriteEmbedIframeSrcs } from './embedIframe';

const DECK_SRC = '/examples/presentations/03-fragments/slides.html';
// The exact shape `example_embed.rs::iframe_block` emits.
const EMBED_HTML = `<iframe class="embed-example-iframe" src="${DECK_SRC}" style="width: 100%; aspect-ratio: 16 / 9;" loading="lazy" allowfullscreen></iframe>`;

describe('extractEmbedIframeSrcs', () => {
  it('extracts the src of an embed-example-iframe', () => {
    expect(extractEmbedIframeSrcs(EMBED_HTML)).toEqual([DECK_SRC]);
  });

  it('returns nothing for html with no embed iframe', () => {
    expect(extractEmbedIframeSrcs('<p>hello</p>')).toEqual([]);
    // A plain iframe without the embed class is ignored.
    expect(extractEmbedIframeSrcs('<iframe src="/x.html"></iframe>')).toEqual([]);
  });

  it('skips external srcs', () => {
    const html = `<iframe class="embed-example-iframe" src="https://x.com/d.html"></iframe>`;
    expect(extractEmbedIframeSrcs(html)).toEqual([]);
  });

  it('handles multiple embeds and de-duplicates', () => {
    const html = `${EMBED_HTML}\n<iframe class="embed-example-iframe" src="/examples/p/02/slides.html"></iframe>\n${EMBED_HTML}`;
    expect(extractEmbedIframeSrcs(html)).toEqual([
      DECK_SRC,
      '/examples/p/02/slides.html',
    ]);
  });
});

describe('rewriteEmbedIframeSrcs', () => {
  it('rewrites the embed iframe src to its manifest blob URL', () => {
    const out = rewriteEmbedIframeSrcs(EMBED_HTML, { [DECK_SRC]: 'blob:abc' });
    expect(out).toContain('src="blob:abc"');
    expect(out).not.toContain(DECK_SRC);
    // Other attributes survive the rewrite.
    expect(out).toContain('class="embed-example-iframe"');
    expect(out).toContain('allowfullscreen');
    expect(out).toContain('aspect-ratio: 16 / 9');
  });

  it('leaves the iframe untouched on a manifest miss', () => {
    const out = rewriteEmbedIframeSrcs(EMBED_HTML, {});
    expect(out).toBe(EMBED_HTML);
  });

  it('does not touch non-embed iframes or other html', () => {
    const html = `<iframe src="/other.html"></iframe><p>x</p>`;
    expect(rewriteEmbedIframeSrcs(html, { '/other.html': 'blob:zzz' })).toBe(html);
  });

  it('rewrites only the matching embed among several iframes', () => {
    const html = `<iframe src="/plain.html"></iframe>${EMBED_HTML}`;
    const out = rewriteEmbedIframeSrcs(html, { [DECK_SRC]: 'blob:deck' });
    expect(out).toContain('<iframe src="/plain.html">'); // untouched
    expect(out).toContain('src="blob:deck"');
  });
});
