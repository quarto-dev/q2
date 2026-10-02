import { describe, expect, it } from 'vitest';
import { resolvePandocFlag } from './buildFlag';

describe('resolvePandocFlag', () => {
  it('is on when the asset exists', () => {
    expect(resolvePandocFlag({ env: {}, assetExists: true })).toEqual({ enabled: true });
  });

  it('is off with a warning naming the fetch script when the asset is missing', () => {
    const f = resolvePandocFlag({ env: {}, assetExists: false });
    expect(f.enabled).toBe(false);
    expect(f.warning).toContain('fetch-pandoc-wasm.mjs');
  });

  it('VITE_PANDOC_WASM=0 turns it off silently, asset or not', () => {
    expect(resolvePandocFlag({ env: { VITE_PANDOC_WASM: '0' }, assetExists: true })).toEqual({ enabled: false });
    expect(resolvePandocFlag({ env: { VITE_PANDOC_WASM: '0' }, assetExists: false })).toEqual({ enabled: false });
  });

  it('any other VITE_PANDOC_WASM value does not enable a missing asset', () => {
    expect(resolvePandocFlag({ env: { VITE_PANDOC_WASM: '1' }, assetExists: false }).enabled).toBe(false);
  });
});
