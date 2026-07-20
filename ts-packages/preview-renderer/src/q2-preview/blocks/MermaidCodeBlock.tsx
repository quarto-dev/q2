import { useContext, useEffect, useRef, useState } from 'react';
import { dataLocProps } from '../../framework';
import type { CodeBlock as CodeBlockType, NodeArgs } from '../../framework';
import { PreviewContext } from '../PreviewContext';
import { CodeBlock } from './CodeBlock';

/**
 * bd-5m4ga0s1: built-in mermaid diagram rendering.
 *
 * A ```mermaid fenced block parses to a Pandoc CodeBlock whose class
 * list contains `mermaid`. In `q2 render` the Rust `mermaid-render`
 * transform (crates/quarto-core/src/transforms/mermaid.rs) turns it
 * into `<pre class="mermaid">` + a CDN loader script; that transform
 * is excluded from the preview pipeline, so here the raw CodeBlock
 * reaches the React layer and this component owns rendering — for
 * both `q2-preview` documents and `q2-slides` decks (RevealDeck
 * renders slide content through the same registry).
 *
 * Registered as the `CodeBlock` entry in `registry.ts`; anything
 * without the `mermaid` class delegates to the plain built-in. User
 * `render-components` overrides of `CodeBlock` still win over this
 * component via the `mergedPreviewRegistry` layering.
 *
 * mermaid.js is loaded on demand from the CDN (dynamic import, cached
 * promise) rather than bundled — same first-cut CDN decision as the
 * Rust side, and diagram-free documents never pay for it. The brace
 * form `{mermaid}` is deliberately NOT matched (engine territory;
 * first-cut decision ratified 2026-07-20).
 */

/**
 * Exact-pinned mermaid version. Keep in sync with `MERMAID_VERSION`
 * in `crates/quarto-core/src/transforms/mermaid.rs` — the version
 * parity is locked by a test on each side.
 */
export const MERMAID_VERSION = '11.12.0';

const MERMAID_CDN_URL = `https://cdn.jsdelivr.net/npm/mermaid@${MERMAID_VERSION}/dist/mermaid.esm.min.mjs`;

/** The slice of mermaid's API this component consumes. */
interface MermaidApi {
    render(id: string, code: string): Promise<{ svg: string }>;
}

type MermaidLoader = () => Promise<MermaidApi>;

/**
 * Default loader: dynamic-import the ESM bundle from the CDN and
 * initialize it once. `@vite-ignore` keeps Vite from trying to
 * resolve/bundle the remote URL at build time — this import is meant
 * to happen in the browser at runtime.
 */
const defaultLoader: MermaidLoader = async () => {
    const mod = await import(/* @vite-ignore */ MERMAID_CDN_URL);
    const mermaid = mod.default;
    mermaid.initialize({ startOnLoad: false });
    return mermaid as MermaidApi;
};

let activeLoader: MermaidLoader = defaultLoader;
let mermaidPromise: Promise<MermaidApi> | null = null;

/** Load-once cache shared by every diagram on the page. */
function getMermaid(): Promise<MermaidApi> {
    if (!mermaidPromise) {
        mermaidPromise = activeLoader();
    }
    return mermaidPromise;
}

/**
 * Test seam: replace (or, with `null`, restore) the mermaid loader.
 * Resets the load-once cache either way so tests are independent.
 */
export function setMermaidLoaderForTests(loader: MermaidLoader | null): void {
    activeLoader = loader ?? defaultLoader;
    mermaidPromise = null;
}

/**
 * Monotonic id source — `mermaid.render()` requires a document-unique
 * element id for its transient render container.
 */
let renderSeq = 0;

const MermaidDiagram = ({ code }: { code: string }) => {
    const ref = useRef<HTMLDivElement | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        let cancelled = false;
        setError(null);
        const id = `quarto-mermaid-${++renderSeq}`;
        getMermaid()
            .then((mermaid) => mermaid.render(id, code))
            .then(({ svg }) => {
                if (cancelled || !ref.current) return;
                ref.current.innerHTML = svg;
            })
            .catch((err: unknown) => {
                if (cancelled) return;
                setError(err instanceof Error ? err.message : String(err));
            });
        return () => {
            cancelled = true;
        };
    }, [code]);

    if (error) {
        return (
            <div className="mermaid-diagram-error" data-mermaid-error="">
                <strong>Mermaid error:</strong> {error}
                <pre>{code}</pre>
            </div>
        );
    }

    return <div ref={ref} className="mermaid-diagram" data-mermaid-diagram="" />;
};

export const MermaidCodeBlock = (args: NodeArgs<CodeBlockType>) => {
    const { node } = args;
    const ctx = useContext(PreviewContext);
    const [[, classes], code] = node.c;

    if (!classes.includes('mermaid')) {
        return <CodeBlock {...args} />;
    }

    // Preserve the edit affordance + source-loc attributes the plain
    // CodeBlock carries, so click-to-edit and scroll-sync still work
    // on diagram blocks (the diagram is a *view* of the code cell).
    const poolId = (node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(node) : null;
    const isEditable =
        resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {};

    return (
        <div className="mermaid-diagram-container" {...affordanceAttr} {...dataLocProps(node)}>
            <MermaidDiagram code={code} />
        </div>
    );
};
