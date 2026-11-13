import { useContext, useEffect, useRef, useState } from 'react';
import { dataLocProps } from '../../framework';
import type { CodeBlock as CodeBlockType, NodeArgs } from '../../framework';
import { PreviewContext } from '../PreviewContext';
import { useCommentAnchorRef } from '../commentAnchor';
import { CodeBlock } from './CodeBlock';

/**
 * bd-ijlapt70 (prototype): built-in Graphviz diagram rendering.
 *
 * A ```dot fenced block parses to a Pandoc CodeBlock whose class list
 * contains `dot`. There is no Rust render transform for Graphviz yet,
 * so the raw CodeBlock reaches the React layer in every pipeline and
 * this component owns rendering in `q2 preview` and QuartoHub.
 *
 * Graphviz needs no DOM: the wasm module takes the source text and
 * returns an SVG string. The runtime is `@viz-js/viz`, which embeds
 * the wasm binary in its JavaScript, so it is bundled (as a lazy
 * chunk) rather than fetched from a CDN, and diagram-free documents
 * never pay for it.
 *
 * The brace form `{dot}` is deliberately NOT matched, following the
 * mermaid precedent. See
 * `claude-notes/plans/2026-11-13-graphviz-support.md`.
 */

/**
 * Exact-pinned Viz.js version. Keep in sync with the `@viz-js/viz`
 * entry in this package's `package.json`.
 */
export const VIZ_VERSION = '3.31.0';

/** Layout engines in the pinned build (`engines` in `@viz-js/viz`). */
const ENGINES = new Set([
    'circo',
    'dot',
    'fdp',
    'neato',
    'nop',
    'nop1',
    'nop2',
    'osage',
    'patchwork',
    'sfdp',
    'twopi',
]);

interface GraphvizRenderResult {
    status: 'success' | 'failure';
    output?: string;
    errors: { level?: 'error' | 'warning'; message: string }[];
}

/** The slice of the Viz instance API this component consumes. */
interface GraphvizApi {
    render(code: string, options: { format: string; engine: string }): GraphvizRenderResult;
}

type GraphvizLoader = () => Promise<GraphvizApi>;

/**
 * Default loader: dynamic-import the package so the bundler splits
 * the 1.2 MB runtime into its own chunk, loaded on first use.
 */
const defaultLoader: GraphvizLoader = async () => {
    const viz = await import('@viz-js/viz');
    return (await viz.instance()) as GraphvizApi;
};

let activeLoader: GraphvizLoader = defaultLoader;
let graphvizPromise: Promise<GraphvizApi> | null = null;

/** Load-once cache shared by every diagram on the page. */
function getGraphviz(): Promise<GraphvizApi> {
    if (!graphvizPromise) {
        graphvizPromise = activeLoader();
    }
    return graphvizPromise;
}

/**
 * Test seam: replace (or, with `null`, restore) the Graphviz loader.
 * Resets the load-once cache either way so tests are independent.
 */
export function setGraphvizLoaderForTests(loader: GraphvizLoader | null): void {
    activeLoader = loader ?? defaultLoader;
    graphvizPromise = null;
}

const LINK_ATTRS = ['href', 'xlink:href'];

/**
 * Parse Graphviz SVG output and strip anything that can run script.
 *
 * Graphviz copies `URL`/`href` attributes and HTML-like labels from
 * the diagram source into its SVG, and the source is author-supplied
 * text that collaborators share. So the SVG is parsed (never assigned
 * to `innerHTML`), `<script>` and `<foreignObject>` are dropped,
 * `on*` handlers are removed, and links are kept only when they are
 * `http(s):`, `mailto:` or a same-document `#fragment`.
 *
 * Returns `null` when the text is not an SVG document.
 */
export function sanitizeGraphvizSvg(svgText: string): SVGSVGElement | null {
    const doc = new DOMParser().parseFromString(svgText, 'image/svg+xml');
    const root = doc.documentElement;
    if (!root || root.localName !== 'svg' || doc.querySelector('parsererror')) {
        return null;
    }
    for (const el of [...root.querySelectorAll('script, foreignObject')]) {
        el.remove();
    }
    for (const el of [root, ...root.querySelectorAll('*')]) {
        for (const attr of [...el.attributes]) {
            const name = attr.name.toLowerCase();
            if (name.startsWith('on')) {
                el.removeAttribute(attr.name);
            } else if (LINK_ATTRS.includes(name) && !/^(https?:|mailto:|#)/i.test(attr.value.trim())) {
                el.removeAttribute(attr.name);
            }
        }
    }
    return root as unknown as SVGSVGElement;
}

/**
 * A `//| fig-align` cell option (or the attribute form) arrives as a
 * `fig-align` attribute on the CodeBlock. Same handling as mermaid.
 */
function figAlign(attrs: [string, string][]): 'left' | 'center' | 'right' | undefined {
    const value = attrs.find(([key]) => key === 'fig-align')?.[1];
    return value === 'left' || value === 'center' || value === 'right' ? value : undefined;
}

/** `graph-layout` selects the layout engine; anything unknown means `dot`. */
function graphLayout(attrs: [string, string][]): string {
    const value = attrs.find(([key]) => key === 'graph-layout')?.[1];
    return value !== undefined && ENGINES.has(value) ? value : 'dot';
}

interface DiagramProps {
    code: string;
    engine: string;
    align?: 'left' | 'center' | 'right';
}

const GraphvizDiagram = ({ code, engine, align }: DiagramProps) => {
    const ref = useRef<HTMLDivElement | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        let cancelled = false;
        setError(null);
        getGraphviz()
            .then((graphviz) => {
                if (cancelled || !ref.current) return;
                const result = graphviz.render(code, { format: 'svg', engine });
                if (result.status !== 'success' || result.output === undefined) {
                    const messages = result.errors.map((e) => e.message.trim()).filter(Boolean);
                    setError(messages.join('; ') || 'Graphviz could not lay out this diagram.');
                    return;
                }
                const svg = sanitizeGraphvizSvg(result.output);
                if (!svg) {
                    setError('Graphviz returned output that is not SVG.');
                    return;
                }
                // Scale down to the column width, keeping the natural
                // size as the maximum (Graphviz sizes are in points).
                svg.style.maxWidth = '100%';
                svg.style.height = 'auto';
                ref.current.replaceChildren(document.importNode(svg, true));
            })
            .catch((err: unknown) => {
                if (cancelled) return;
                setError(err instanceof Error ? err.message : String(err));
            });
        return () => {
            cancelled = true;
        };
    }, [code, engine]);

    if (error) {
        return (
            <div className="graphviz-diagram-error" data-graphviz-error="">
                <strong>Graphviz error:</strong> {error}
                <pre>{code}</pre>
            </div>
        );
    }

    return (
        <div
            ref={ref}
            className="graphviz-diagram"
            data-graphviz-diagram=""
            style={align ? { textAlign: align } : undefined}
        />
    );
};

export const GraphvizCodeBlock = (args: NodeArgs<CodeBlockType>) => {
    const { node } = args;
    const ctx = useContext(PreviewContext);
    const anchorRef = useCommentAnchorRef(node);
    const [[, classes, attrs], code] = node.c;

    if (!classes.includes('dot')) {
        return <CodeBlock {...args} />;
    }

    // Same edit affordance + source-loc attributes as the mermaid
    // wrapper, so click-to-edit and scroll-sync work on diagram blocks.
    const poolId = (node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(node) : null;
    const isEditable =
        resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {};

    return (
        <div ref={anchorRef} className="graphviz-diagram-container" {...affordanceAttr} {...dataLocProps(node)}>
            <GraphvizDiagram code={code} engine={graphLayout(attrs)} align={figAlign(attrs)} />
        </div>
    );
};
