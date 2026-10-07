// The default-font bundle format. Kept apart from fonts.ts so the main thread can import it
// without pulling in typst.ts (`@quarto/typst-host/fontBundle`).
/**
 * The fonts bundle served as `typst/fonts.bin.gz` (the default typst-assets fonts, built by
 * `scripts/fetch-typst-assets.mjs`): a little-endian u32 count, then for each font a u32 byte
 * length and the bytes. gzip is applied around the whole bundle.
 */
export function packFonts(fonts: Uint8Array[]): Uint8Array {
  const total = 4 + fonts.reduce((n, f) => n + 4 + f.length, 0);
  const out = new Uint8Array(total);
  const view = new DataView(out.buffer);
  view.setUint32(0, fonts.length, true);
  let at = 4;
  for (const f of fonts) {
    view.setUint32(at, f.length, true);
    out.set(f, at + 4);
    at += 4 + f.length;
  }
  return out;
}

export function unpackFonts(bundle: Uint8Array): Uint8Array[] {
  const view = new DataView(bundle.buffer, bundle.byteOffset, bundle.byteLength);
  if (bundle.length < 4) throw new Error('font bundle is truncated');
  const count = view.getUint32(0, true);
  const fonts: Uint8Array[] = [];
  let at = 4;
  for (let i = 0; i < count; i++) {
    if (at + 4 > bundle.length) throw new Error('font bundle is truncated');
    const len = view.getUint32(at, true);
    at += 4;
    if (at + len > bundle.length) throw new Error('font bundle is truncated');
    fonts.push(bundle.subarray(at, at + len));
    at += len;
  }
  return fonts;
}
