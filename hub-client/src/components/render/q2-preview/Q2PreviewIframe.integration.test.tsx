/**
 * Parent-side theme three-way + blob-URL lifecycle test (Plan 2A
 * §"Test plan" — "Theme three-way posting + blob-URL lifecycle test").
 *
 * Mounts `<Q2PreviewIframe>`, mocks `vfsReadFile` to return predictable
 * bytes per fingerprint, and spies on `URL.createObjectURL` /
 * `URL.revokeObjectURL`. Drives `IFRAME_READY` by dispatching synthetic
 * messages on `window`, then walks each fingerprint transition and
 * asserts (a) the post payload and (b) the URL lifecycle.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/react';
import { act } from 'react';

// Mock the VFS reads so the test doesn't need WASM. The theme mock is
// keyed off `mockBytes`; the asset-binary mock is keyed off a per-path
// map so the asset-manifest test can vary which paths return which bytes.
let mockBytes: string | null = null;
const mockBinaryByPath: Map<string, string | null> = new Map();
vi.mock('../../../services/wasmRenderer', () => ({
    vfsReadFile: vi.fn(() => {
        if (mockBytes === null) return { success: false };
        return { success: true, content: mockBytes };
    }),
    vfsReadBinaryFile: vi.fn((path: string) => {
        const bytes = mockBinaryByPath.get(path);
        if (bytes === null || bytes === undefined) return { success: false };
        return { success: true, content: bytes };
    }),
}));

import { Q2PreviewIframe } from './Q2PreviewIframe';

let postMessageSpy: ReturnType<typeof vi.spyOn>;
let createUrlSpy: ReturnType<typeof vi.spyOn>;
let revokeUrlSpy: ReturnType<typeof vi.spyOn>;
let urlCounter = 0;

beforeEach(() => {
    mockBytes = null;
    mockBinaryByPath.clear();
    urlCounter = 0;
    // Stub `URL.createObjectURL` / `revokeObjectURL` — jsdom does not
    // implement them. Each call mints a unique synthetic URL.
    createUrlSpy = vi
        .spyOn(URL, 'createObjectURL')
        .mockImplementation(() => `blob:mock:${++urlCounter}`);
    revokeUrlSpy = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    // Note: postMessage is spied per-test once we have iframeRef.
    postMessageSpy = vi.fn() as any;
});

afterEach(() => {
    createUrlSpy.mockRestore();
    revokeUrlSpy.mockRestore();
});

interface RenderHarness {
    rerender: (fp: string | null | undefined) => void;
    unmount: () => void;
    postMessage: ReturnType<typeof vi.fn>;
}

function renderWithFingerprint(
    initial: string | null | undefined,
): RenderHarness {
    // We want to spy on the iframe element's contentWindow.postMessage.
    // Defer the spy until after first render mounts the iframe DOM.
    const utils = render(
        <Q2PreviewIframe
            astJson={'{"pandoc-api-version":[1,23,0],"meta":{},"blocks":[]}'}
            currentFilePath="/test.qmd"
            setAst={() => {}}
            themeFingerprint={initial}
        />,
    );

    const iframe = utils.container.querySelector('iframe');
    if (!iframe) throw new Error('iframe not mounted');
    // jsdom auto-creates a contentWindow on the iframe; spy on its
    // postMessage method so we can assert on the payloads.
    const cw = iframe.contentWindow;
    if (!cw) throw new Error('iframe.contentWindow is null');
    const postMessage = vi.fn();
    Object.defineProperty(cw, 'postMessage', {
        value: postMessage,
        configurable: true,
    });

    // Drive IFRAME_READY so the effects gated on `iframeReady` fire.
    act(() => {
        window.dispatchEvent(
            new MessageEvent('message', {
                data: { type: 'IFRAME_READY' },
                source: cw,
            }),
        );
    });

    return {
        rerender: (fp) =>
            utils.rerender(
                <Q2PreviewIframe
                    astJson={
                        '{"pandoc-api-version":[1,23,0],"meta":{},"blocks":[]}'
                    }
                    currentFilePath="/test.qmd"
                    setAst={() => {}}
                    themeFingerprint={fp}
                />,
            ),
        unmount: utils.unmount,
        postMessage,
    };
}

function themePosts(postMessage: ReturnType<typeof vi.fn>) {
    return postMessage.mock.calls
        .map((c) => c[0])
        .filter((m) => m?.type === 'UPDATE_THEME');
}

describe('Q2PreviewIframe theme blob-URL lifecycle', () => {
    test('full three-way + revocation walk', () => {
        // Start undefined → no theme posts.
        const harness = renderWithFingerprint(undefined);
        expect(themePosts(harness.postMessage)).toHaveLength(0);

        // 1. fingerprint='abc' (mock returns 'A') → one mint, one post.
        mockBytes = 'A';
        act(() => harness.rerender('abc'));
        let posts = themePosts(harness.postMessage);
        expect(posts).toHaveLength(1);
        expect(posts[0]).toMatchObject({
            cssUrl: 'blob:mock:1',
            fingerprint: 'abc',
        });
        expect(createUrlSpy).toHaveBeenCalledTimes(1);
        expect(revokeUrlSpy).toHaveBeenCalledTimes(0);

        // 2. fingerprint='abc' again → no new post (dedup).
        act(() => harness.rerender('abc'));
        posts = themePosts(harness.postMessage);
        expect(posts).toHaveLength(1);
        expect(createUrlSpy).toHaveBeenCalledTimes(1);
        expect(revokeUrlSpy).toHaveBeenCalledTimes(0);

        // 3. fingerprint='def' (mock returns 'B') → revoke 'abc' URL,
        //    mint new, one post.
        mockBytes = 'B';
        act(() => harness.rerender('def'));
        posts = themePosts(harness.postMessage);
        expect(posts).toHaveLength(2);
        expect(posts[1]).toMatchObject({
            cssUrl: 'blob:mock:2',
            fingerprint: 'def',
        });
        expect(createUrlSpy).toHaveBeenCalledTimes(2);
        expect(revokeUrlSpy).toHaveBeenCalledWith('blob:mock:1');

        // 4. fingerprint=null → revoke 'def' URL, post explicit clear,
        //    no new mint.
        act(() => harness.rerender(null));
        posts = themePosts(harness.postMessage);
        expect(posts).toHaveLength(3);
        expect(posts[2]).toMatchObject({ cssUrl: null, fingerprint: null });
        expect(createUrlSpy).toHaveBeenCalledTimes(2);
        expect(revokeUrlSpy).toHaveBeenCalledWith('blob:mock:2');

        // 5. fingerprint=undefined → no post, no mint, no revoke.
        const postsBefore = posts.length;
        const createCallsBefore = createUrlSpy.mock.calls.length;
        const revokeCallsBefore = revokeUrlSpy.mock.calls.length;
        act(() => harness.rerender(undefined));
        posts = themePosts(harness.postMessage);
        expect(posts).toHaveLength(postsBefore);
        expect(createUrlSpy.mock.calls.length).toBe(createCallsBefore);
        expect(revokeUrlSpy.mock.calls.length).toBe(revokeCallsBefore);

        // 6. fingerprint='ghi' (mock returns 'C') → mint, post; no
        //    revoke (last URL was already revoked at step 4).
        mockBytes = 'C';
        act(() => harness.rerender('ghi'));
        posts = themePosts(harness.postMessage);
        expect(posts).toHaveLength(postsBefore + 1);
        expect(posts[posts.length - 1]).toMatchObject({
            cssUrl: 'blob:mock:3',
            fingerprint: 'ghi',
        });
        expect(createUrlSpy).toHaveBeenCalledTimes(3);
        expect(revokeUrlSpy.mock.calls.length).toBe(revokeCallsBefore);

        // 7. Unmount → revoke the outstanding 'ghi' URL.
        harness.unmount();
        expect(revokeUrlSpy).toHaveBeenCalledWith('blob:mock:3');
    });

    test('UPDATE_AST payload contains assetManifest derived from the AST', () => {
        // Image AST. The walker mints a blob URL for each unique image
        // and includes the user-written URL string in the manifest.
        const PNG_BYTES_B64 = Buffer.from([0x89, 0x50, 0x4e, 0x47]).toString('base64');
        mockBinaryByPath.set('test/hero.png', PNG_BYTES_B64);

        const astJson = JSON.stringify({
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                {
                    t: 'Para',
                    c: [
                        {
                            t: 'Image',
                            c: [
                                ['', [], []],
                                [{ t: 'Str', c: 'alt' }],
                                ['hero.png', ''],
                            ],
                        },
                    ],
                },
            ],
        });

        const utils = render(
            <Q2PreviewIframe
                astJson={astJson}
                currentFilePath="/test/index.qmd"
                setAst={() => {}}
            />,
        );

        const iframe = utils.container.querySelector('iframe');
        const cw = iframe!.contentWindow!;
        const postMessage = vi.fn();
        Object.defineProperty(cw, 'postMessage', {
            value: postMessage,
            configurable: true,
        });

        act(() => {
            window.dispatchEvent(
                new MessageEvent('message', {
                    data: { type: 'IFRAME_READY' },
                    source: cw,
                }),
            );
        });

        const astPosts = postMessage.mock.calls
            .map((c) => c[0])
            .filter((m) => m?.type === 'UPDATE_AST');
        expect(astPosts).toHaveLength(1);
        expect(astPosts[0].payload).toMatchObject({
            astJson,
            currentFilePath: '/test/index.qmd',
        });
        expect(astPosts[0].payload.assetManifest).toBeDefined();
        expect(astPosts[0].payload.assetManifest['hero.png']).toMatch(/^blob:/);

        // Unmount revokes the asset blob URL.
        const beforeRevokes = revokeUrlSpy.mock.calls.length;
        utils.unmount();
        expect(revokeUrlSpy.mock.calls.length).toBeGreaterThan(beforeRevokes);
    });

    test('IFRAME_READY revokes outstanding URL', () => {
        // Defensive contract from the plan: on `IFRAME_READY` the
        // wrapper revokes any outstanding blob URL it holds, since a
        // fresh iframe no longer references it. In the real flow this
        // path runs on the first `IFRAME_READY` of a fresh component
        // mount (refs init undefined/null, so the revoke is a no-op).
        // The test simulates a synthetic second `IFRAME_READY` to
        // exercise the revocation branch.
        mockBytes = 'A';
        const harness = renderWithFingerprint('abc');
        expect(themePosts(harness.postMessage)).toHaveLength(1);
        expect(createUrlSpy).toHaveBeenCalledTimes(1);

        const iframe = document.querySelector('iframe');
        const cw = iframe!.contentWindow!;
        act(() => {
            window.dispatchEvent(
                new MessageEvent('message', {
                    data: { type: 'IFRAME_READY' },
                    source: cw,
                }),
            );
        });
        expect(revokeUrlSpy).toHaveBeenCalledWith('blob:mock:1');
        // Unmount cleanup should now be a no-op on the URL we already
        // revoked (the ref was cleared on IFRAME_READY).
        const revokesBefore = revokeUrlSpy.mock.calls.length;
        harness.unmount();
        expect(revokeUrlSpy.mock.calls.length).toBe(revokesBefore);
    });
});
