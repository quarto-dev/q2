import { describe, expect, it } from 'vitest';
import { MAX_PNG_SIDE, metafileSize } from './metafileToPng';
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
  it('sizes P1\'s EMF from its frame (0.01 mm) at 96 dpi, doubled, with the bounds as the extent', () => {
    const emf = loadImportRecording('emf-docx').media.find((m) => m.rel.endsWith('.emf'))!;
    expect(metafileSize(emf.bytes, 'emf')).toEqual({ width: 200, height: 100, extX: 100, extY: 50 });
  });

  it('sizes a WMF from its placeable header', () => {
    expect(metafileSize(validWmf(), 'wmf')).toEqual({ width: 13, height: 7, extX: 100, extY: 50 });
  });

  it('caps the longest side at 4096, keeping the aspect ratio', () => {
    // 1000 mm x 500 mm is far over the cap.
    const size = metafileSize(emfHeader([0, 0, 9999, 4999], [0, 0, 100000, 50000]), 'emf');
    expect([size.width, size.height]).toEqual([MAX_PNG_SIDE, MAX_PNG_SIDE / 2]);
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
