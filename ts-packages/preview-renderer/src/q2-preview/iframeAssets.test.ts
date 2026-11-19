/**
 * Tests for `resolveAssetManifest` — the iframe mints one blob URL per
 * asset key, reuses it while the key stays, revokes it once unused, and
 * passes string entries (URLs) through.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';

import { resetMintedAssetsForTest, resolveAssetManifest } from './iframeAssets';

let urlCounter = 0;
const revoked: string[] = [];

beforeEach(() => {
    urlCounter = 0;
    revoked.length = 0;
    resetMintedAssetsForTest();
    (globalThis.URL.createObjectURL as unknown) = vi.fn(() => `blob:iframe-${urlCounter++}`);
    (globalThis.URL.revokeObjectURL as unknown) = vi.fn((url: string) => {
        revoked.push(url);
    });
});

const png = (byte: number) => new Blob([new Uint8Array([byte])], { type: 'image/png' });

describe('resolveAssetManifest', () => {
    it('mints a URL in the iframe for each asset', () => {
        expect(resolveAssetManifest({ 'hero.png': { key: 'asset-1', blob: png(1) } })).toEqual({
            'hero.png': 'blob:iframe-0',
        });
    });

    it('keeps the URL while the key stays, even for a re-cloned Blob', () => {
        resolveAssetManifest({ 'hero.png': { key: 'asset-1', blob: png(1) } });
        const second = resolveAssetManifest({ 'hero.png': { key: 'asset-1', blob: png(1) } });

        expect(second).toEqual({ 'hero.png': 'blob:iframe-0' });
        expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
        expect(revoked).toEqual([]);
    });

    it('revokes a URL once no manifest uses its key', () => {
        resolveAssetManifest({ 'hero.png': { key: 'asset-1', blob: png(1) } });
        const changed = resolveAssetManifest({ 'hero.png': { key: 'asset-2', blob: png(2) } });

        expect(changed).toEqual({ 'hero.png': 'blob:iframe-1' });
        expect(revoked).toEqual(['blob:iframe-0']);
    });

    it('passes a URL through, and keeps a minted URL a host re-posts alive', () => {
        const first = resolveAssetManifest({ 'hero.png': { key: 'asset-1', blob: png(1) } });
        // A host re-posting the payload the iframe resolved sends the URL back as a string.
        const reposted = resolveAssetManifest({ 'hero.png': first['hero.png']! });

        expect(reposted).toEqual({ 'hero.png': 'blob:iframe-0' });
        expect(revoked).toEqual([]);
        expect(resolveAssetManifest({ 'logo.png': 'https://example.com/logo.png' })).toEqual({
            'logo.png': 'https://example.com/logo.png',
        });
    });
});
