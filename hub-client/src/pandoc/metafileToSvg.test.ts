import { describe, expect, it } from 'vitest';
import { metafileSize, normalizeSvgIds } from './metafileToSvg';
import { loadImportRecording } from '../test-utils/pandocRecordings';
import { validWmf } from '../test-utils/metafiles';

/** An EMF header with the given `rclBounds` and `rclFrame` (the rest of the file is not read for the size). */
function emfHeader(bounds: [number, number, number, number], frame: [number, number, number, number]): Uint8Array {
  const b = new Uint8Array(88);
  const v = new DataView(b.buffer);
  v.setUint32(0, 1, true);
  v.setUint32(4, 88, true);
  [...bounds, ...frame].forEach((n, i) => v.setInt32(8 + i * 4, n, true));
  return b;
}

describe('metafileSize', () => {
  it('sizes P1\'s EMF from its frame (0.01 mm) at 96 dpi (1x), with the bounds as the extent', () => {
    const emf = loadImportRecording('emf-docx').media.find((m) => m.rel.endsWith('.emf'))!;
    expect(metafileSize(emf.bytes, 'emf')).toEqual({ width: 100, height: 50, extX: 100, extY: 50 });
  });

  it('sizes a WMF from its placeable header', () => {
    expect(metafileSize(validWmf(), 'wmf')).toEqual({ width: 7, height: 3, extX: 100, extY: 50 });
  });

  it('does not cap a large metafile: the stored SVG is vector, and the rasterizer caps at export', () => {
    // 1000 mm x 500 mm is about 3780 x 1890 px at 96 dpi.
    const size = metafileSize(emfHeader([0, 0, 9999, 4999], [0, 0, 100000, 50000]), 'emf');
    expect([size.width, size.height]).toEqual([3780, 1890]);
  });

  it('falls back to the frame\'s units when rclBounds is empty', () => {
    const size = metafileSize(emfHeader([0, 0, -1, -1], [0, 0, 2646, 1323]), 'emf');
    expect(size).toMatchObject({ extX: 2646, extY: 1323 });
  });

  it('refuses what has no size: not an EMF, an empty frame, a WMF without a placeable header, and P1\'s malformed WMF', () => {
    expect(() => metafileSize(new Uint8Array(100), 'emf')).toThrow(/not an EMF/);
    expect(() => metafileSize(emfHeader([0, 0, 9, 9], [0, 0, 0, 0]), 'emf')).toThrow(/frame is empty/);
    expect(() => metafileSize(new Uint8Array([1, 0, 9, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), 'wmf')).toThrow(/placeable/);
    const bad = loadImportRecording('emf-docx').media.find((m) => m.rel.endsWith('.wmf'))!;
    expect(() => metafileSize(bad.bytes, 'wmf')).toThrow(/WMF/);
  });
});

describe('normalizeSvgIds', () => {
  it('renumbers EMF and WMF ids by order of first appearance, in definitions and references alike', () => {
    const svg = '<clipPath id="EMFJS_c7"/><pattern id="EMFJS_p3"/><g clip-path="url(#EMFJS_c7)" fill="url(#EMFJS_p3)"/>';
    expect(normalizeSvgIds(svg)).toBe('<clipPath id="EMFJS_c0"/><pattern id="EMFJS_p1"/><g clip-path="url(#EMFJS_c0)" fill="url(#EMFJS_p1)"/>');
    expect(normalizeSvgIds('<clipPath id="wmfjs_c4"/><use href="#wmfjs_c4"/><clipPath id="wmfjs_c9"/>')).toBe('<clipPath id="wmfjs_c0"/><use href="#wmfjs_c0"/><clipPath id="wmfjs_c1"/>');
  });

  it('is idempotent', () => {
    const once = normalizeSvgIds('<a id="wmfjs_c5"/><b id="wmfjs_p2"/>');
    expect(normalizeSvgIds(once)).toBe(once);
  });

  it('gives equal output for the same drawing under different counter values', () => {
    const draw = (n: number) => `<clipPath id="wmfjs_c${n}"/><rect clip-path="url(#wmfjs_c${n})"/>`;
    expect(normalizeSvgIds(draw(0))).toBe(normalizeSvgIds(draw(41)));
  });

  it('leaves other text, including look-alike names, alone', () => {
    const svg = '<text>EMFJS_c and wmfjs_x</text><g id="my_wmfjs_c1"/><g id="other_c3"/>';
    expect(normalizeSvgIds(svg)).toBe(svg);
  });
});
