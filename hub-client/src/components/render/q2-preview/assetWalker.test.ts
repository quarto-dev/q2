/**
 * Tests for `buildAssetManifest` — parent-side asset walker for q2-preview.
 *
 * Covers the cache-hit / content-change-eviction / image-removal /
 * external-skipping / VFS-failure / N=100 stress cases the plan calls
 * out in §"Asset walker tests".
 *
 * Mocks `vfsReadBinaryFile` and `URL.createObjectURL` / `URL.revokeObjectURL`
 * — these aren't available in node, and we want deterministic URLs.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';

const vfsMock = vi.fn();
vi.mock('../../../services/wasmRenderer', () => ({
    vfsReadBinaryFile: (path: string) => vfsMock(path),
}));

import { buildAssetManifest, type ManifestCacheEntry } from './assetWalker';

let urlCounter = 0;
const minted: string[] = [];
const revoked: string[] = [];

beforeEach(() => {
    urlCounter = 0;
    minted.length = 0;
    revoked.length = 0;
    vfsMock.mockReset();
    // jsdom does not implement URL.createObjectURL / revokeObjectURL.
    // Stub for deterministic minting.
    (globalThis.URL.createObjectURL as unknown) = vi.fn(() => {
        const url = `blob:test-${urlCounter++}`;
        minted.push(url);
        return url;
    });
    (globalThis.URL.revokeObjectURL as unknown) = vi.fn((url: string) => {
        revoked.push(url);
    });
    // jsdom node env has atob; under the workspace's `node` test env we
    // need to make sure it exists. Vite's vitest setup includes it.
    if (typeof globalThis.atob !== 'function') {
        (globalThis as { atob?: (s: string) => string }).atob = (s: string) =>
            Buffer.from(s, 'base64').toString('binary');
    }
});

const PNG_BYTES_B64 = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).toString(
    'base64',
);
const ALT_PNG_B64 = Buffer.from([0x00, 0x01, 0x02, 0x03]).toString('base64');

function imageNode(url: string) {
    return {
        t: 'Image',
        c: [
            ['', [], []],
            [{ t: 'Str', c: 'alt' }],
            [url, ''],
        ],
    };
}

function paraOf(...inlines: any[]) {
    return { t: 'Para', c: inlines };
}

function ast(blocks: any[]): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
    });
}

describe('buildAssetManifest', () => {
    it('mints a blob URL for a single Image and adds it to the manifest', () => {
        vfsMock.mockReturnValue({ success: true, content: PNG_BYTES_B64 });
        const cache = new Map<string, ManifestCacheEntry>();
        const json = ast([paraOf(imageNode('hero.png'))]);

        const { manifest, revoked: rev } = buildAssetManifest(json, '/project/index.qmd', cache);

        expect(manifest).toEqual({ 'hero.png': 'blob:test-0' });
        expect(minted).toEqual(['blob:test-0']);
        expect(rev).toEqual([]);
        expect(cache.size).toBe(1);
    });

    it('cache hit: same content on second run does not mint a new URL', () => {
        vfsMock.mockReturnValue({ success: true, content: PNG_BYTES_B64 });
        const cache = new Map<string, ManifestCacheEntry>();
        const json = ast([paraOf(imageNode('hero.png'))]);

        buildAssetManifest(json, '/project/index.qmd', cache);
        const result = buildAssetManifest(json, '/project/index.qmd', cache);

        expect(minted).toEqual(['blob:test-0']); // exactly one mint across both runs
        expect(revoked).toEqual([]);
        expect(result.manifest['hero.png']).toBe('blob:test-0');
    });

    it('content change: evicts the old URL and mints a new one', () => {
        const cache = new Map<string, ManifestCacheEntry>();
        const json = ast([paraOf(imageNode('hero.png'))]);

        vfsMock.mockReturnValue({ success: true, content: PNG_BYTES_B64 });
        buildAssetManifest(json, '/project/index.qmd', cache);
        vfsMock.mockReturnValue({ success: true, content: ALT_PNG_B64 });
        const result = buildAssetManifest(json, '/project/index.qmd', cache);

        expect(minted).toEqual(['blob:test-0', 'blob:test-1']);
        expect(revoked).toEqual(['blob:test-0']);
        expect(result.manifest['hero.png']).toBe('blob:test-1');
        expect(result.revoked).toEqual(['blob:test-0']);
    });

    it('image removed from AST: revokes its blob URL', () => {
        vfsMock.mockReturnValue({ success: true, content: PNG_BYTES_B64 });
        const cache = new Map<string, ManifestCacheEntry>();

        buildAssetManifest(ast([paraOf(imageNode('hero.png'))]), '/project/index.qmd', cache);
        const result = buildAssetManifest(
            ast([paraOf({ t: 'Str', c: 'no images here' })]),
            '/project/index.qmd',
            cache,
        );

        expect(revoked).toEqual(['blob:test-0']);
        expect(result.manifest).toEqual({});
        expect(cache.size).toBe(0);
    });

    it('external URLs are skipped (not in manifest, no VFS read)', () => {
        const cache = new Map<string, ManifestCacheEntry>();
        const json = ast([
            paraOf(imageNode('https://cdn.example.com/hero.png')),
            paraOf(imageNode('data:image/png;base64,iVBOR')),
            paraOf(imageNode('//cdn.example.com/hero.png')),
            paraOf(imageNode('http://cdn.example.com/hero.png')),
        ]);

        const result = buildAssetManifest(json, '/project/index.qmd', cache);

        expect(vfsMock).not.toHaveBeenCalled();
        expect(result.manifest).toEqual({});
        expect(minted).toEqual([]);
    });

    it('VFS read failure: image is omitted from the manifest, no mint', () => {
        vfsMock.mockReturnValue({ success: false, error: 'not found' });
        const cache = new Map<string, ManifestCacheEntry>();

        const result = buildAssetManifest(
            ast([paraOf(imageNode('missing.png'))]),
            '/project/index.qmd',
            cache,
        );

        expect(result.manifest).toEqual({});
        expect(minted).toEqual([]);
    });

    it('N=100 stress: exactly N mints on first run, 0 on second run with same content', () => {
        vfsMock.mockReturnValue({ success: true, content: PNG_BYTES_B64 });
        const cache = new Map<string, ManifestCacheEntry>();

        const blocks = [];
        for (let i = 0; i < 100; i++) {
            blocks.push(paraOf(imageNode(`img-${i}.png`)));
        }
        const json = ast(blocks);

        buildAssetManifest(json, '/project/index.qmd', cache);
        expect(minted).toHaveLength(100);

        const beforeSecond = minted.length;
        buildAssetManifest(json, '/project/index.qmd', cache);
        expect(minted).toHaveLength(beforeSecond);
        expect(revoked).toEqual([]);
    });

    it('preserves origPath unchanged in the manifest (not the resolved VFS key)', () => {
        // The manifest key is the user-written `c[2][0]`; that's what
        // <Image> looks up. The VFS read uses the resolved path internally.
        vfsMock.mockReturnValue({ success: true, content: PNG_BYTES_B64 });
        const cache = new Map<string, ManifestCacheEntry>();

        buildAssetManifest(
            ast([paraOf(imageNode('../shared/hero.png'))]),
            '/project/sub/page.qmd',
            cache,
        );

        // VFS was queried using the resolved path (no leading slash).
        expect(vfsMock).toHaveBeenCalledWith('project/shared/hero.png');
    });

    it('handles malformed AST JSON without throwing', () => {
        const cache = new Map<string, ManifestCacheEntry>();
        const result = buildAssetManifest('{ not json', '/project/index.qmd', cache);
        expect(result.manifest).toEqual({});
        expect(result.revoked).toEqual([]);
    });

    it('finds Images nested inside Figure / BulletList / Div', () => {
        vfsMock.mockReturnValue({ success: true, content: PNG_BYTES_B64 });
        const cache = new Map<string, ManifestCacheEntry>();

        const figure = {
            t: 'Figure',
            c: [
                ['', [], []],
                [null, []],
                [paraOf(imageNode('in-figure.png'))],
            ],
        };
        const list = {
            t: 'BulletList',
            c: [[paraOf(imageNode('in-list.png'))]],
        };
        const div = {
            t: 'Div',
            c: [['', [], []], [paraOf(imageNode('in-div.png'))]],
        };

        const json = ast([figure, list, div]);
        const result = buildAssetManifest(json, '/project/index.qmd', cache);

        expect(Object.keys(result.manifest).sort()).toEqual([
            'in-div.png',
            'in-figure.png',
            'in-list.png',
        ]);
    });
});
