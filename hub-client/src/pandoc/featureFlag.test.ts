import { describe, expect, it } from 'vitest';
import { pandocWarmEnabled } from './featureFlag';

describe('pandocWarmEnabled', () => {
  it('is on unless the build sets VITE_PANDOC_WARM=0', () => {
    expect(pandocWarmEnabled(undefined)).toBe(true);
    expect(pandocWarmEnabled('')).toBe(true);
    expect(pandocWarmEnabled('1')).toBe(true);
    expect(pandocWarmEnabled('0')).toBe(false);
  });
});
