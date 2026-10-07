import { describe, expect, it } from 'vitest';
import { splitTypstAssets } from './typstAssetSplit';

const b = (n: number) => new Uint8Array([n]);

describe('splitTypstAssets', () => {
  it('strips packages/ and keeps only font files from fonts/', () => {
    const { vendoredPackages, fonts } = splitTypstAssets([
      { path: 'packages/preview/showybox/2.0.4/typst.toml', bytes: b(1) },
      { path: 'fonts/Font Awesome 6 Free-Solid-900.otf', bytes: b(2) },
      { path: 'fonts/LICENSE.txt', bytes: b(3) },
      { path: 'fonts/Brand.TTF', bytes: b(4) },
      { path: 'other/x', bytes: b(5) },
    ]);
    expect(vendoredPackages).toEqual([{ path: 'preview/showybox/2.0.4/typst.toml', bytes: b(1) }]);
    expect(fonts.map((f) => f[0])).toEqual([2, 4]);
  });
});
