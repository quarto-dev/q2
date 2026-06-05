import { describe, it, expect } from 'vitest';
import { sliceBytes } from './sliceSource';

describe('sliceBytes', () => {
  it('slices ASCII content by byte offset', () => {
    expect(sliceBytes('hello world', 0, 5)).toBe('hello');
  });

  it('slices from a non-zero start offset in ASCII', () => {
    expect(sliceBytes('hello world', 6, 11)).toBe('world');
  });

  it('returns empty string for zero-length slice', () => {
    expect(sliceBytes('hello', 2, 2)).toBe('');
  });

  it('returns full string when slicing entire byte range', () => {
    expect(sliceBytes('hello', 0, 5)).toBe('hello');
  });

  it('slices multibyte content by UTF-8 byte offsets (accented char)', () => {
    // 'café' in UTF-8:
    //   c = 0x63 (1 byte, offset 0)
    //   a = 0x61 (1 byte, offset 1)
    //   f = 0x66 (1 byte, offset 2)
    //   é = 0xC3 0xA9 (2 bytes, offsets 3–4)
    // Total: 5 bytes

    // JS String.slice(0, 4) would give 'café' (4 chars), but byte slice 0–3 is 'caf'
    expect(sliceBytes('café', 0, 3)).toBe('caf');

    // Byte offsets 0–5 give the full string
    expect(sliceBytes('café', 0, 5)).toBe('café');

    // Byte offsets 3–5 give just 'é'
    expect(sliceBytes('café', 3, 5)).toBe('é');
  });

  it('slices around a 4-byte emoji by UTF-8 byte offsets', () => {
    // 'hi 🎉 end' in UTF-8:
    //   h = offset 0 (1 byte)
    //   i = offset 1 (1 byte)
    //   ' ' = offset 2 (1 byte)
    //   🎉 = offsets 3–6 (4 bytes)
    //   ' ' = offset 7 (1 byte)
    //   e = offset 8 (1 byte)
    //   n = offset 9 (1 byte)
    //   d = offset 10 (1 byte)
    // Total: 11 bytes

    expect(sliceBytes('hi 🎉 end', 0, 3)).toBe('hi ');
    expect(sliceBytes('hi 🎉 end', 3, 7)).toBe('🎉');
    expect(sliceBytes('hi 🎉 end', 7, 11)).toBe(' end');
    expect(sliceBytes('hi 🎉 end', 0, 11)).toBe('hi 🎉 end');
  });

  it('returns empty string when start equals 0 and end equals 0', () => {
    expect(sliceBytes('hello', 0, 0)).toBe('');
  });
});
