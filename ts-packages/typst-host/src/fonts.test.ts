import { describe, expect, it } from 'vitest';
import { fontFamilies, packFonts, unpackFonts } from './fonts.ts';
import { defaultFonts, typstModule, vendoredFonts } from './fixtures.test-util.ts';

describe('font bundle', () => {
  it('round-trips, including empty fonts and sub-views', () => {
    const fonts = [new Uint8Array([1, 2, 3]), new Uint8Array(0), new Uint8Array([9])];
    const back = unpackFonts(packFonts(fonts));
    expect(back.map((f) => [...f])).toEqual(fonts.map((f) => [...f]));
  });

  it('rejects a truncated bundle', () => {
    const bundle = packFonts([new Uint8Array(10)]);
    expect(() => unpackFonts(bundle.subarray(0, bundle.length - 1))).toThrow(/truncated/);
    expect(() => unpackFonts(new Uint8Array(2))).toThrow(/truncated/);
  });

  it('the fetched default-font bundle holds the 17 typst-assets fonts', () => {
    expect(defaultFonts().length).toBe(17 + vendoredFonts().length);
  });
});

describe('fontFamilies', () => {
  it('lists each family once, in file order', async () => {
    const families = await fontFamilies(await typstModule(), defaultFonts());
    expect(new Set(families).size).toBe(families.length);
    expect(families).toEqual(expect.arrayContaining(['Libertinus Serif', 'New Computer Modern', 'New Computer Modern Math', 'DejaVu Sans Mono']));
    expect(families.filter((f) => f.startsWith('Font Awesome 6'))).toHaveLength(3);
  });
});
