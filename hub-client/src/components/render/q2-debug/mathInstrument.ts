/**
 * Source-position instrumentation for KaTeX output.
 *
 * KaTeX's HTML carries no source positions, and its DOM order does not
 * follow source order (a fraction's vlist is built bottom-up). But its
 * parse tree does: every node has `loc.start` / `loc.end` into the
 * LaTeX string. So we parse first, then wrap each math-mode *leaf*
 * (one glyph: `mathord`, `textord`, `atom`, symbol `op`, `spacing`) in
 * the trusted `\htmlData{s=START,e=END}{…}` extension and render that.
 * Each glyph span in the output then sits inside
 * `span[data-s][data-e]` carrying its character range in the LaTeX.
 *
 * `entry.tsx` turns those into absolute `data-off` byte stamps after
 * each render (`stampMathLeaves`), at which point the caret, selection
 * hull, and click resolver treat them as ordinary leaves.
 *
 * Skipped on purpose (they are fields of structural nodes, not leaves,
 * so the generic walk never sees them as leaves): `\left(` / `\right)`
 * delimiters, `\sqrt[3]` optional args, `\big)` sizing delimiters —
 * wrapping any of those would break parsing. Also skipped: text-mode
 * leaves (`\text{…}` letters; wrapping each letter perturbs spacing)
 * and ranges from macro expansion (outside the input, or overlapping).
 */

import katex from 'katex';

interface Loc { start: number; end: number }
interface Leaf extends Loc {
    /**
     * Spacing-class command that must re-wrap an `atom` so KaTeX keeps
     * its binary/relation/punctuation spacing: `\htmlData` alone yields
     * an `enclosing` span that the spacing pass treats as an ord.
     */
    classCmd: string | null;
}
interface ParseNodeLike {
    type: string;
    mode?: 'math' | 'text';
    loc?: Loc | null;
    family?: string;
    [key: string]: unknown;
}

const ATOM_CLASS_CMD: Record<string, string> = {
    bin: '\\mathbin',
    rel: '\\mathrel',
    open: '\\mathopen',
    close: '\\mathclose',
    punct: '\\mathpunct',
    inner: '\\mathinner',
};

// Glyph leaves. Deliberately *not* here: `op` symbols like `\sum` (their
// `_`/`^` limits attach to the op node; wrapping turns them into side
// scripts) and `spacing` (no glyph to point at).
const LEAF_TYPES = new Set(['mathord', 'textord', 'atom']);

function collectLeaves(node: unknown, out: Leaf[]): void {
    if (Array.isArray(node)) {
        for (const n of node) collectLeaves(n, out);
        return;
    }
    if (!node || typeof node !== 'object') return;
    const n = node as ParseNodeLike;
    if (typeof n.type === 'string' && LEAF_TYPES.has(n.type)) {
        if (n.mode === 'math' && n.loc && n.loc.end > n.loc.start) {
            out.push({
                start: n.loc.start,
                end: n.loc.end,
                classCmd: n.type === 'atom' ? ATOM_CLASS_CMD[n.family ?? ''] ?? null : null,
            });
        }
        return;
    }
    for (const [key, value] of Object.entries(n)) {
        if (key === 'loc') continue;
        collectLeaves(value, out);
    }
}

/**
 * Wrap each leaf of `latex` in `\htmlData{s=…,e=…}{…}`. Returns `null`
 * when the parse fails or nothing could be instrumented.
 */
export function instrumentLatex(latex: string): string | null {
    let tree: unknown;
    try {
        tree = (katex as unknown as {
            __parse: (expr: string, opts: Record<string, unknown>) => unknown;
        }).__parse(latex, { strict: false });
    } catch {
        return null;
    }
    const leaves: Leaf[] = [];
    collectLeaves(tree, leaves);
    // Right-to-left so earlier offsets stay valid; drop anything out of
    // range or overlapping its already-accepted neighbour (macro expansion).
    leaves.sort((a, b) => b.start - a.start || a.end - b.end);
    let out = latex;
    let lastStart = Infinity;
    let wrapped = 0;
    for (const { start, end, classCmd } of leaves) {
        if (start < 0 || end > latex.length || end > lastStart) continue;
        // Outer braces: KaTeX rejects a bare command as a `^`/`_`/`\frac`
        // argument ("Got function ... with no arguments as argument").
        const inner = `\\htmlData{s=${start},e=${end}}{${out.slice(start, end)}}`;
        const wrappedLeaf = classCmd ? `${classCmd}{${inner}}` : `{${inner}}`;
        out = `${out.slice(0, start)}${wrappedLeaf}${out.slice(end)}`;
        lastStart = start;
        wrapped++;
    }
    return wrapped > 0 ? out : null;
}

export interface MathRender {
    html: string;
    /** True when the output carries `data-s`/`data-e` leaf spans. */
    instrumented: boolean;
}

/**
 * Render `latex` with KaTeX, instrumented when possible. Falls back to a
 * plain render if the instrumented string fails to parse (KaTeX emits a
 * `katex-error` span with `throwOnError: false`).
 */
export function renderMath(latex: string, displayMode: boolean): MathRender {
    const base = { displayMode, throwOnError: false, output: 'html' as const };
    const instrumented = instrumentLatex(latex);
    if (instrumented) {
        try {
            const html = katex.renderToString(instrumented, {
                ...base,
                strict: false,
                trust: (ctx) => ctx.command === '\\htmlData',
            });
            if (!html.includes('katex-error')) return { html, instrumented: true };
        } catch {
            // fall through to the plain render
        }
    }
    try {
        return { html: katex.renderToString(latex, base), instrumented: false };
    } catch {
        return { html: '', instrumented: false };
    }
}
