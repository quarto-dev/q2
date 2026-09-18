// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import type { Mock } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
    HepImage,
    HEPHAESTUS_SVG_WASM_VERSION,
    isHepTarget,
    setHephaestusLoaderForTests,
} from './HepImage';
import { previewRegistry } from '../registry';
import { AssetManifestContext } from '../AssetManifestContext';
import type { NodeArgs, ImageInline } from '../../framework';

/**
 * bd-sxiv2tio: built-in hephaestus plot-document rendering for
 * q2-preview documents and revealjs decks. An `![](plot.hep)` reaches
 * the React layer as a raw Image (the Rust `hephaestus-render`
 * transform is native-only and excluded from the preview pipeline);
 * the registry's `Image` entry is the hep-aware wrapper, which mounts
 * the npm `hephaestus-svg-wasm` `PlotView` over the bytes the asset
 * walker already exposes, and delegates everything else to the plain
 * built-in `<img>`.
 *
 * The wasm client is injected here (`setHephaestusLoaderForTests`) so
 * no wasm / font fetching happens under vitest; the real module is
 * exercised in the browser checks recorded in the plan.
 */

const HEP_BYTES = new Uint8Array([0x48, 0x45, 0x50, 0x48, 0x50, 0x4c, 0x4f, 0x54, 1, 2, 3]);

const image = (
    url: string,
    kvs: [string, string][] = [],
    attr: { id?: string; classes?: string[] } = {},
    alt: string = '',
    title: string = '',
): ImageInline =>
    ({
        t: 'Image',
        c: [
            [attr.id ?? '', attr.classes ?? [], kvs],
            alt ? [{ t: 'Str', c: alt }] : [],
            [url, title],
        ],
    }) as ImageInline;

function renderNode(
    node: ImageInline,
    manifest: Record<string, string> = {},
    Component = HepImage,
) {
    const args = { node, setLocalAst: () => {} } as NodeArgs<ImageInline>;
    return render(
        <AssetManifestContext.Provider value={manifest}>
            <Component {...args} />
        </AssetManifestContext.Provider>,
    );
}

interface FakeView {
    free: Mock<() => void>;
    hints: () => { width?: number; height?: number; dpi?: number };
    warnings: string[];
}

describe('isHepTarget', () => {
    it('matches local paths ending in .hep, ignoring query and fragment', () => {
        expect(isHepTarget('plot.hep')).toBe(true);
        expect(isHepTarget('figs/plot.HEP')).toBe(true);
        expect(isHepTarget('/figs/plot.hep?v=2')).toBe(true);
        expect(isHepTarget('plot.hep#frag')).toBe(true);
    });

    it('rejects other extensions and external / data URLs', () => {
        expect(isHepTarget('plot.png')).toBe(false);
        expect(isHepTarget('plot.hep.png')).toBe(false);
        expect(isHepTarget('plothep')).toBe(false);
        expect(isHepTarget('https://example.com/plot.hep')).toBe(false);
        expect(isHepTarget('http://example.com/plot.hep')).toBe(false);
        expect(isHepTarget('//cdn.example.com/plot.hep')).toBe(false);
        expect(isHepTarget('data:application/octet-stream;base64,SEVQ')).toBe(false);
    });
});

describe('HepImage', () => {
    let createCalls: { bytes: Uint8Array; opts: unknown }[];
    let views: FakeView[];
    let fetchMock: ReturnType<typeof vi.fn>;

    beforeEach(() => {
        createCalls = [];
        views = [];
        fetchMock = vi.fn(async (_url: string) => ({
            ok: true,
            status: 200,
            statusText: 'OK',
            arrayBuffer: async () => HEP_BYTES.buffer.slice(0),
        }));
        vi.stubGlobal('fetch', fetchMock);
    });

    afterEach(() => {
        setHephaestusLoaderForTests(null);
        vi.unstubAllGlobals();
    });

    /** Install a fake wasm client; returns the loader spy. */
    function installFakeClient(hints: FakeView['hints'] = () => ({})) {
        const loader = vi.fn(async () => ({
            PlotView: {
                create: async (container: HTMLElement, bytes: Uint8Array, opts: unknown) => {
                    createCalls.push({ bytes, opts });
                    container.innerHTML = '<svg data-fake-hep="1"></svg>';
                    const view: FakeView = { free: vi.fn<() => void>(), hints, warnings: [] };
                    views.push(view);
                    return view;
                },
            },
        }));
        setHephaestusLoaderForTests(loader);
        return loader;
    }

    it('mounts a plot over the fetched bytes instead of an <img>', async () => {
        installFakeClient();
        const { container } = renderNode(image('figs/plot.hep'), {
            'figs/plot.hep': 'blob:null/abc',
        });

        const plot = container.querySelector('[data-hephaestus-plot]');
        expect(plot).not.toBeNull();
        await waitFor(() => {
            expect(container.querySelector('svg[data-fake-hep]')).not.toBeNull();
        });
        expect(fetchMock).toHaveBeenCalledTimes(1);
        expect(fetchMock.mock.calls[0][0]).toBe('blob:null/abc');
        expect(createCalls).toHaveLength(1);
        expect(Array.from(createCalls[0].bytes)).toEqual(Array.from(HEP_BYTES));
        expect(createCalls[0].opts).toEqual({
            colorScheme: 'light',
            autoResize: true,
            picking: false,
        });
        expect(container.querySelector('img')).toBeNull();
    });

    it('delegates non-.hep images to the plain <img>', () => {
        const loader = installFakeClient();
        const { container } = renderNode(image('figs/plot.png', [['width', '320']]), {
            'figs/plot.png': 'blob:null/png',
        });

        const img = container.querySelector('img');
        expect(img).not.toBeNull();
        expect(img!.getAttribute('src')).toBe('blob:null/png');
        expect(img!.getAttribute('width')).toBe('320');
        expect(container.querySelector('[data-hephaestus-plot]')).toBeNull();
        expect(loader).not.toHaveBeenCalled();
        expect(fetchMock).not.toHaveBeenCalled();
    });

    it('leaves an external .hep URL to the plain <img>, like the Rust transform', () => {
        const loader = installFakeClient();
        const { container } = renderNode(image('https://example.com/plot.hep'));
        const img = container.querySelector('img');
        expect(img).not.toBeNull();
        expect(img!.getAttribute('src')).toBe('https://example.com/plot.hep');
        expect(loader).not.toHaveBeenCalled();
    });

    it('treats a .hep with a query string or fragment as a plot', async () => {
        installFakeClient();
        const { container } = renderNode(image('figs/plot.hep?v=2'), {
            'figs/plot.hep?v=2': 'blob:null/v2',
        });
        await waitFor(() => {
            expect(container.querySelector('svg[data-fake-hep]')).not.toBeNull();
        });
        expect(fetchMock.mock.calls[0][0]).toBe('blob:null/v2');
    });

    it('reports a missing file, without fetching, when the manifest has no entry', async () => {
        // A manifest miss means the asset walker found no such file in the
        // project VFS. The plain <img> falls back to the raw URL (a broken
        // image is its signal); here that would fetch the preview host's
        // HTML fallback and report "bad magic" for a file that simply is
        // not there, so say so instead — the preview's analogue of Q-18-1.
        const loader = installFakeClient();
        const { container } = renderNode(image('figs/does-not-exist.hep'), {});
        await waitFor(() => {
            expect(container.querySelector('[data-hephaestus-error]')).not.toBeNull();
        });
        const box = container.querySelector('[data-hephaestus-error]')!;
        expect(box.textContent).toContain('figs/does-not-exist.hep');
        expect(box.textContent).toContain('not found');
        expect(fetchMock).not.toHaveBeenCalled();
        expect(loader).not.toHaveBeenCalled();
    });

    it('loads the wasm client once across several plots', async () => {
        const loader = installFakeClient();
        const a = renderNode(image('a.hep'), { 'a.hep': 'blob:null/a' });
        const b = renderNode(image('b.hep'), { 'b.hep': 'blob:null/b' });
        await waitFor(() => {
            expect(a.container.querySelector('svg[data-fake-hep]')).not.toBeNull();
            expect(b.container.querySelector('svg[data-fake-hep]')).not.toBeNull();
        });
        expect(loader).toHaveBeenCalledTimes(1);
        expect(createCalls).toHaveLength(2);
    });

    it('shows an error box naming the file when the bytes cannot be fetched', async () => {
        const loader = installFakeClient();
        fetchMock.mockImplementation(async () => ({
            ok: false,
            status: 404,
            statusText: 'Not Found',
            arrayBuffer: async () => new ArrayBuffer(0),
        }));
        const { container } = renderNode(image('figs/missing.hep'), {
            'figs/missing.hep': 'blob:null/missing',
        });
        await waitFor(() => {
            expect(container.querySelector('[data-hephaestus-error]')).not.toBeNull();
        });
        const box = container.querySelector('[data-hephaestus-error]')!;
        expect(box.textContent).toContain('figs/missing.hep');
        expect(box.textContent).toContain('404');
        expect(loader).not.toHaveBeenCalled();
        expect(container.querySelector('[data-hephaestus-plot]')).toBeNull();
    });

    it('shows an error box with the message when the client rejects the document', async () => {
        setHephaestusLoaderForTests(async () => ({
            PlotView: {
                create: async () => {
                    throw new Error('unsupported document format version 9');
                },
            },
        }));
        const { container } = renderNode(image('figs/plot.hep'), {
            'figs/plot.hep': 'blob:null/abc',
        });
        await waitFor(() => {
            expect(container.querySelector('[data-hephaestus-error]')).not.toBeNull();
        });
        const box = container.querySelector('[data-hephaestus-error]')!;
        expect(box.textContent).toContain('unsupported document format version 9');
        expect(box.textContent).toContain('figs/plot.hep');
    });

    it('frees the wasm-side view on unmount', async () => {
        installFakeClient();
        const { container, unmount } = renderNode(image('figs/plot.hep'), {
            'figs/plot.hep': 'blob:null/abc',
        });
        await waitFor(() => {
            expect(container.querySelector('svg[data-fake-hep]')).not.toBeNull();
        });
        expect(views).toHaveLength(1);
        expect(views[0].free).not.toHaveBeenCalled();
        unmount();
        expect(views[0].free).toHaveBeenCalledTimes(1);
    });

    it('remounts the view when the bytes behind the image change', async () => {
        installFakeClient();
        const node = image('figs/plot.hep');
        const args = { node, setLocalAst: () => {} } as NodeArgs<ImageInline>;
        const { container, rerender } = render(
            <AssetManifestContext.Provider value={{ 'figs/plot.hep': 'blob:null/v1' }}>
                <HepImage {...args} />
            </AssetManifestContext.Provider>,
        );
        await waitFor(() => {
            expect(container.querySelector('svg[data-fake-hep]')).not.toBeNull();
        });
        rerender(
            <AssetManifestContext.Provider value={{ 'figs/plot.hep': 'blob:null/v2' }}>
                <HepImage {...args} />
            </AssetManifestContext.Provider>,
        );
        await waitFor(() => {
            expect(createCalls).toHaveLength(2);
        });
        expect(views[0].free).toHaveBeenCalledTimes(1);
        expect(fetchMock.mock.calls.map((c) => c[0])).toEqual(['blob:null/v1', 'blob:null/v2']);
    });

    describe('sizing (decision 1: reflow up to the hint width)', () => {
        it('uses width/height attributes (px) for the box and its aspect ratio', async () => {
            installFakeClient(() => ({ width: 900, height: 420 }));
            const { container } = renderNode(
                image('figs/plot.hep', [
                    ['width', '320'],
                    ['height', '240px'],
                ]),
                { 'figs/plot.hep': 'blob:null/abc' },
            );
            const plot = container.querySelector<HTMLElement>('[data-hephaestus-plot]')!;
            expect(plot.style.width).toBe('320px');
            expect(plot.style.maxWidth).toBe('100%');
            expect(plot.style.aspectRatio).toBe('320 / 240');
            await waitFor(() => {
                expect(container.querySelector('svg[data-fake-hep]')).not.toBeNull();
            });
            // Attributes win over the document hint per axis.
            expect(plot.style.width).toBe('320px');
            expect(plot.style.aspectRatio).toBe('320 / 240');
        });

        it("adopts the document's size hint once the plot has loaded", async () => {
            installFakeClient(() => ({ width: 900, height: 420 }));
            const { container } = renderNode(image('figs/plot.hep'), {
                'figs/plot.hep': 'blob:null/abc',
            });
            const plot = container.querySelector<HTMLElement>('[data-hephaestus-plot]')!;
            // Before the hint is known: the render-side default (672 × 480).
            expect(plot.style.width).toBe('672px');
            expect(plot.style.aspectRatio).toBe('672 / 480');
            await waitFor(() => {
                expect(plot.style.width).toBe('900px');
            });
            expect(plot.style.aspectRatio).toBe('900 / 420');
            expect(plot.style.maxWidth).toBe('100%');
        });

        it('mixes an explicit width with the hinted height', async () => {
            installFakeClient(() => ({ width: 900, height: 420 }));
            const { container } = renderNode(image('figs/plot.hep', [['width', '450']]), {
                'figs/plot.hep': 'blob:null/abc',
            });
            const plot = container.querySelector<HTMLElement>('[data-hephaestus-plot]')!;
            await waitFor(() => {
                expect(plot.style.aspectRatio).toBe('450 / 420');
            });
            expect(plot.style.width).toBe('450px');
        });

        it('falls back to 672 × 480 when neither attributes nor hints give a size', async () => {
            installFakeClient(() => ({}));
            const { container } = renderNode(image('figs/plot.hep', [['width', '50%']]), {
                'figs/plot.hep': 'blob:null/abc',
            });
            const plot = container.querySelector<HTMLElement>('[data-hephaestus-plot]')!;
            await waitFor(() => {
                expect(container.querySelector('svg[data-fake-hep]')).not.toBeNull();
            });
            // A percentage is not a size the scene can be solved at
            // (mirrors the Rust `parse_px`); it stays on the box as CSS.
            expect(plot.style.width).toBe('50%');
            expect(plot.style.aspectRatio).toBe('672 / 480');
        });
    });

    it('carries alt text, id, classes and title onto the plot box', async () => {
        installFakeClient();
        const { container } = renderNode(
            image('figs/plot.hep', [], { id: 'fig-a', classes: ['wide'] }, 'Sine wave', 'A title'),
            { 'figs/plot.hep': 'blob:null/abc' },
        );
        const plot = container.querySelector<HTMLElement>('[data-hephaestus-plot]')!;
        expect(plot.getAttribute('role')).toBe('img');
        expect(plot.getAttribute('aria-label')).toBe('Sine wave');
        expect(plot.id).toBe('fig-a');
        expect(plot.classList.contains('hephaestus-plot')).toBe(true);
        expect(plot.classList.contains('wide')).toBe(true);
        expect(plot.getAttribute('title')).toBe('A title');
    });

    it('is registered as the previewRegistry Image entry', () => {
        expect(previewRegistry.Image).toBe(HepImage);
    });

    it('is shadowed by a user render-components Image override', () => {
        const UserImage = () => null;
        const merged = { ...previewRegistry, ...{ Image: UserImage } };
        expect(merged.Image).toBe(UserImage);
        expect(previewRegistry.Image).toBe(HepImage);
    });

    it('pins the npm client to the version of the hephaestus crate', () => {
        const here = resolve(__dirname, '../../..');
        const pkg = JSON.parse(readFileSync(resolve(here, 'package.json'), 'utf-8'));
        expect(pkg.dependencies['hephaestus-svg-wasm']).toBe(HEPHAESTUS_SVG_WASM_VERSION);

        const lock = readFileSync(resolve(here, '../../Cargo.lock'), 'utf-8');
        const m = /name = "hephaestus"\nversion = "([^"]+)"/.exec(lock);
        expect(m, 'hephaestus in Cargo.lock').not.toBeNull();
        expect(m![1]).toBe(HEPHAESTUS_SVG_WASM_VERSION);
    });
});
