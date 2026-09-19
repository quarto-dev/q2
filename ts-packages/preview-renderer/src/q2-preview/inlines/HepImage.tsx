import { useContext, useEffect, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import type { PlotViewOptions } from 'hephaestus-svg-wasm';
import type { ImageInline, NodeArgs } from '../../framework';
import { inlinesToPlainText } from '../../framework';
import { AssetManifestContext } from '../AssetManifestContext';
import { Image } from './Image';

/**
 * bd-sxiv2tio: built-in hephaestus plot-document rendering.
 *
 * `![](plot.hep)` names a hephaestus plot document — the plot's data,
 * scales and theme rather than a picture of it. In `q2 render` the
 * native `hephaestus-render` transform
 * (crates/quarto-core/src/transforms/hephaestus.rs) lays the plot out
 * and writes an SVG beside the page. That transform is native-only and
 * excluded from the preview pipeline, so here the raw Image reaches the
 * React layer and this component owns rendering — for `q2-preview`
 * documents and `format: revealjs` decks alike (RevealDeck draws slide
 * content through the same registry).
 *
 * Registered as the `Image` entry in `registry.ts`; anything that is not
 * a local `.hep` target delegates to the plain built-in `<img>`. User
 * `render-components` overrides of `Image` still win via the
 * `mergedPreviewRegistry` layering.
 *
 * Rendering goes through the npm `hephaestus-svg-wasm` client — the
 * renderer-free sibling of the crate `q2 render` links, released in
 * lockstep with it (the version pin below is checked against
 * `Cargo.lock` from both sides). It is loaded on demand with a dynamic
 * import (cached promise), so plot-free documents never pay for the
 * 2.4 MB wasm; the client fetches its bundled Roboto faces on the first
 * plot and registers them with the shaper and the page. The bytes come
 * from the asset walker's manifest — the same blob URL any Image target
 * gets — via `fetch`.
 *
 * Sizing (decision 1 of the phase-2 plan): the box is as wide as the
 * `width` attribute, else the document's own size hint, else the
 * render-side default, capped at the column (`max-width: 100%`), and its
 * height follows the same aspect ratio. `PlotView` observes the box and
 * re-solves the layout at whatever size it gets, so a narrow column or a
 * slide gets a reflowed plot rather than a shrunk picture, while a wide
 * column matches `q2 render` exactly.
 */

/**
 * Exact-pinned npm client version. Must equal the `hephaestus` crate in
 * `Cargo.lock` — the plot-document format version is compared for
 * equality, so a `.hep` one side reads the other must too. Locked by a
 * test on each side (`HepImage.test.tsx`, `hephaestus.rs`).
 */
export const HEPHAESTUS_SVG_WASM_VERSION = '0.4.1';

/**
 * Render-side default (CSS px) when neither attributes nor the document
 * say otherwise: 7in × 5in at 96 dpi, knitr's `fig-width` / `fig-height`
 * defaults. Mirrors `DEFAULT_SIZE` in `hephaestus.rs`.
 */
const DEFAULT_SIZE = { width: 672, height: 480 };

/** The slice of the client's `PlotView` this component consumes. */
interface PlotViewLike {
    hints(): { width?: number; height?: number; dpi?: number };
    free(): void;
}

/** The slice of the npm client's API this component consumes. */
interface HephaestusClient {
    PlotView: {
        create(
            container: HTMLElement,
            doc: Uint8Array,
            opts?: PlotViewOptions,
        ): Promise<PlotViewLike>;
    };
}

type HephaestusLoader = () => Promise<HephaestusClient>;

/**
 * Default loader: dynamic-import the npm client and initialise its wasm
 * module once. The client's glue locates `hephaestus_svg_wasm_bg.wasm`
 * (and, later, its fonts) with `new URL(..., import.meta.url)`, which
 * Vite rewrites to hashed assets served beside the renderer bundle.
 */
const defaultLoader: HephaestusLoader = async () => {
    const mod = await import('hephaestus-svg-wasm');
    await mod.default();
    return { PlotView: mod.PlotView };
};

let activeLoader: HephaestusLoader = defaultLoader;
let clientPromise: Promise<HephaestusClient> | null = null;

/** Load-once cache shared by every plot on the page. */
function getClient(): Promise<HephaestusClient> {
    if (!clientPromise) {
        clientPromise = activeLoader();
    }
    return clientPromise;
}

/**
 * Test seam: replace (or, with `null`, restore) the client loader.
 * Resets the load-once cache either way so tests are independent.
 */
export function setHephaestusLoaderForTests(loader: HephaestusLoader | null): void {
    activeLoader = loader ?? defaultLoader;
    clientPromise = null;
}

function isExternal(url: string): boolean {
    return (
        url.startsWith('http://') ||
        url.startsWith('https://') ||
        url.startsWith('data:') ||
        url.startsWith('//')
    );
}

/**
 * True for a local (non-external, non-`data:`) URL whose path ends in
 * `.hep`, ignoring any query string or fragment. Mirrors `is_hep_target`
 * in `hephaestus.rs` (including `Path::extension` semantics: a bare
 * `.hep` file name has no extension).
 */
export function isHepTarget(url: string): boolean {
    if (isExternal(url)) return false;
    const path = url.split(/[?#]/, 1)[0];
    const base = path.slice(path.lastIndexOf('/') + 1);
    const dot = base.lastIndexOf('.');
    if (dot <= 0) return false;
    return base.slice(dot + 1).toLowerCase() === 'hep';
}

/**
 * A CSS-px size from an attribute (a bare number or `NNNpx`), or
 * `undefined` for anything the scene cannot be solved at (percentages,
 * physical units). Mirrors `parse_px` in `hephaestus.rs`.
 */
function parsePx(value: string | undefined): number | undefined {
    if (value === undefined) return undefined;
    const trimmed = value.trim();
    const number = (trimmed.endsWith('px') ? trimmed.slice(0, -2) : trimmed).trim();
    if (number === '') return undefined;
    const v = Number(number);
    return Number.isFinite(v) && v > 0 ? v : undefined;
}

interface HepPlotProps {
    /** The user-written target, for messages. */
    url: string;
    /**
     * Where the bytes are: the manifest's URL for `url`, or `null` when
     * the asset walker found no such file in the project. The plain
     * `<img>` falls back to the raw URL on a miss (its broken image is
     * the signal); here that would fetch the preview host's HTML
     * fallback and report "bad magic" for a file that is simply not
     * there, so a miss is reported as such — the preview's analogue of
     * the native transform's `Q-19-1`.
     */
    src: string | null;
    kvs: Record<string, string>;
    alt: string;
    id: string;
    classes: string[];
    title: string;
}

const HepPlot = ({ url, src, kvs, alt, id, classes, title }: HepPlotProps) => {
    const ref = useRef<HTMLSpanElement | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [hint, setHint] = useState<{ width?: number; height?: number } | null>(null);

    useEffect(() => {
        let cancelled = false;
        let view: PlotViewLike | null = null;
        setError(null);
        setHint(null);
        if (src === null) {
            setError('file not found in the project');
            return;
        }
        (async () => {
            const response = await fetch(src);
            if (!response.ok) {
                throw new Error(`HTTP ${response.status} ${response.statusText}`.trimEnd());
            }
            const bytes = new Uint8Array(await response.arrayBuffer());
            if (cancelled) return;
            const client = await getClient();
            if (cancelled || !ref.current) return;
            const created = await client.PlotView.create(ref.current, bytes, {
                colorScheme: 'light',
                autoResize: true,
                picking: false,
            });
            if (cancelled) {
                created.free();
                return;
            }
            view = created;
            const h = view.hints();
            setHint({ width: h.width, height: h.height });
        })().catch((err: unknown) => {
            if (cancelled) return;
            setError(err instanceof Error ? err.message : String(err));
        });
        return () => {
            cancelled = true;
            if (view) {
                view.free();
                view = null;
            }
        };
    }, [src]);

    if (error) {
        return (
            <span className="hephaestus-plot-error" data-hephaestus-error="">
                <strong>Plot error:</strong> {url}: {error}
            </span>
        );
    }

    // Attributes win per axis, then the document's hint, then the
    // render-side default. A non-px `width` (e.g. `50%`) is not a size
    // the scene can be solved at, but it is still what the author asked
    // the box to be, so it stays on the box as CSS.
    const attrWidth = parsePx(kvs.width);
    const attrHeight = parsePx(kvs.height);
    const width = attrWidth ?? hint?.width ?? DEFAULT_SIZE.width;
    const height = attrHeight ?? hint?.height ?? DEFAULT_SIZE.height;
    const rawWidth = kvs.width?.trim();
    const cssWidth = attrWidth !== undefined || !rawWidth ? `${width}px` : rawWidth;

    const style: CSSProperties = {
        display: 'inline-block',
        width: cssWidth,
        maxWidth: '100%',
        aspectRatio: `${width} / ${height}`,
        // The client replaces the box's content with an inline <svg>;
        // kill the strut/descender gap so the box is exactly the plot.
        lineHeight: 0,
        verticalAlign: 'top',
        overflow: 'hidden',
    };

    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (title) props.title = title;
    if (alt) props['aria-label'] = alt;

    return (
        <span
            ref={ref}
            className={['hephaestus-plot', ...classes].join(' ')}
            data-hephaestus-plot=""
            role="img"
            style={style}
            {...props}
        />
    );
};

export const HepImage = (args: NodeArgs<ImageInline>) => {
    const { node } = args;
    const [[id, classes, kvs], altInlines, [url, title]] = node.c;
    const manifest = useContext(AssetManifestContext);

    if (!isHepTarget(url)) {
        return <Image {...args} />;
    }

    const kvMap: Record<string, string> = {};
    for (const [k, v] of kvs) kvMap[k] = v;

    // Not `lookupAssetUrl`: its raw-URL fallback on a miss is the plain
    // <img>'s contract, not this one's (see `HepPlotProps.src`).
    const src = Object.prototype.hasOwnProperty.call(manifest, url) ? manifest[url] : null;

    return (
        <HepPlot
            url={url}
            src={src}
            kvs={kvMap}
            alt={inlinesToPlainText(altInlines)}
            id={id}
            classes={classes}
            title={title}
        />
    );
};
