import { describe, expect, it } from 'vitest';
import { packFonts } from '@quarto/typst-host/fontBundle';
import { looksLikeFontBundle } from './typstAssets';

describe('looksLikeFontBundle (the raw-body check for a server that adds Content-Encoding: gzip)', () => {
  it('accepts a packed bundle', () => {
    expect(looksLikeFontBundle(packFonts([new Uint8Array(100), new Uint8Array(5)]))).toBe(true);
  });
  it('rejects an HTML error page, a gzip stream and tiny bodies', () => {
    const enc = new TextEncoder();
    expect(looksLikeFontBundle(enc.encode('<!doctype html><html><body>not found</body></html>'))).toBe(false);
    expect(looksLikeFontBundle(new Uint8Array([0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 1, 2, 3]))).toBe(false);
    expect(looksLikeFontBundle(new Uint8Array(4))).toBe(false);
  });
});
