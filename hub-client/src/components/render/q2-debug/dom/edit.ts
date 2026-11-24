/**
 * Text edits through the AST caret (dom/caret.ts).
 *
 * An edit is "replace the text between two host positions with `text`".
 * It is carried out on a copy of the document's AST: for every text run
 * the range touches, the run's text is spliced and re-tokenised into
 * Str / Space / SoftBreak inlines that replace the run's nodes in their
 * parent list. Runs strictly inside the range lose all their text; the
 * replacement text goes where the range starts. Every node (inline or
 * block) that the range contains completely is removed. Nothing is joined
 * or unwrapped: a node the range only reaches into stays, emptied as far
 * as the range goes. Hosts that are not text runs (code leaves) are left
 * alone.
 *
 * The copy keeps the AST's document-level fields (`astContext`, `meta`),
 * so it can be reconciled into the document and sent to the host as is.
 * The host's JSON reader requires a source-info reference (`s`) on every
 * inline, so the new nodes share one Generated pool entry (`by: unknown`,
 * no anchors) appended to the copy's pool.
 */

import type { PandocAST } from '@quarto/preview-renderer/framework';
import type { DebugDocument } from './document';
import { isTextLike, textOf } from './textDiff';

/** A text host and an offset into its AST text. */
export interface HostPosition {
    host: HTMLElement;
    offset: number;
}

/**
 * The document's AST with `[start, end)` replaced by `text`, or `null`
 * when the positions can't be resolved. The document itself is untouched.
 */
export function replaceText(doc: DebugDocument, start: HostPosition, end: HostPosition, text: string): PandocAST | null {
    const ast = doc.ast;
    if (!ast) return null;
    const hosts = doc.textHosts();
    const first = hosts.indexOf(start.host);
    const last = hosts.indexOf(end.host);
    if (first === -1 || last === -1 || last < first) return null;

    const copies = new Map<unknown, unknown>();
    const copy = clone(ast, copies) as PandocAST;
    const generated = addGeneratedSourceInfo(copy);
    for (let k = first; k <= last; k++) {
        const nodes = doc.nodesOf(hosts[k]);
        if (!nodes.every(isTextLike)) continue;
        const old = nodes.map(textOf).join('');
        const a = k === first ? start.offset : 0;
        const b = k === last ? end.offset : old.length;
        const next = old.slice(0, a) + (k === first ? text : '') + old.slice(b);
        // An empty run has no node to locate; its list is known directly.
        const spot = nodes.length === 0
            ? emptyListSpot(copies.get(doc.listOf(hosts[k])))
            : containerOf(copy, copies.get(nodes[0]));
        if (!spot) continue;
        spot.list.splice(spot.index, nodes.length, ...tokenize(next, generated));
    }
    for (const node of containedNodes(doc, start, end)) {
        // Not found when an ancestor was removed just before: already gone.
        const spot = containerOf(copy, copies.get(node));
        if (spot) spot.list.splice(spot.index, 1);
    }
    return copy;
}

/**
 * Nodes whose boxes lie entirely between `start` and `end`: after the
 * start host and before the end host in document order, containing neither.
 */
function containedNodes(doc: DebugDocument, start: HostPosition, end: HostPosition): unknown[] {
    if (start.host === end.host) return [];
    const out: unknown[] = [];
    for (const box of doc.contentElement.querySelectorAll<HTMLElement>('.node')) {
        const node = doc.nodeOf(box);
        if (node === undefined) continue;
        if (box.contains(start.host) || box.contains(end.host)) continue;
        const afterStart = (start.host.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
        const beforeEnd = (end.host.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_PRECEDING) !== 0;
        if (afterStart && beforeEnd) out.push(node);
    }
    return out;
}

/**
 * Append a `Generated { by: unknown, from: [] }` entry to the AST's
 * source-info pool (creating the pool if needed) and return its index.
 */
function addGeneratedSourceInfo(ast: PandocAST): number {
    const doc = ast as unknown as { astContext?: { p?: unknown[] } };
    doc.astContext ??= {};
    doc.astContext.p ??= [];
    doc.astContext.p.push({ r: [0, 0], t: 4, d: { by: { kind: 'unknown' }, from: [] } });
    return doc.astContext.p.length - 1;
}

/**
 * Str / Space / SoftBreak inlines for `text`, each referencing pool entry `s`.
 * A non-breaking space counts as a space: the writer spells a `Space` the
 * parser would otherwise strip (trailing, leading, doubled) as `\ `, which
 * reads back as U+00A0 in the Str; typing on from there should give a
 * plain space again, not carry the non-breaking one along.
 */
export function tokenize(text: string, s: number): unknown[] {
    const out: unknown[] = [];
    let word = '';
    const flush = () => {
        if (word !== '') out.push({ t: 'Str', c: word, s });
        word = '';
    };
    for (const ch of text) {
        if (ch === ' ' || ch === '\u00a0') { flush(); out.push({ t: 'Space', s }); }
        else if (ch === '\n') { flush(); out.push({ t: 'SoftBreak', s }); }
        else word += ch;
    }
    flush();
    return out;
}

function emptyListSpot(list: unknown): { list: unknown[]; index: number } | null {
    return Array.isArray(list) ? { list, index: 0 } : null;
}

/** Deep copy, recording each object's and array's copy in `copies` (keyed by the original). */
function clone(value: unknown, copies: Map<unknown, unknown>): unknown {
    if (Array.isArray(value)) {
        const out = value.map((v) => clone(v, copies));
        copies.set(value, out);
        return out;
    }
    if (value !== null && typeof value === 'object') {
        const out: Record<string, unknown> = {};
        for (const [k, v] of Object.entries(value)) out[k] = clone(v, copies);
        copies.set(value, out);
        return out;
    }
    return value;
}

/** The list under `root` that holds `target` (by identity), and its index there. */
function containerOf(root: unknown, target: unknown): { list: unknown[]; index: number } | null {
    if (target === undefined) return null;
    if (Array.isArray(root)) {
        const index = root.indexOf(target);
        if (index !== -1) return { list: root, index };
        for (const item of root) {
            const found = containerOf(item, target);
            if (found) return found;
        }
    } else if (root !== null && typeof root === 'object') {
        for (const v of Object.values(root)) {
            const found = containerOf(v, target);
            if (found) return found;
        }
    }
    return null;
}
