/**
 * Character-level change segments for a text run, derived from the edit
 * script (never by comparing old and new text on our own — text that
 * merely moved between paragraphs is not a text change).
 */

import type { Touched } from '@quarto/preview-renderer/astdiff';

export type TextKind = 'Str' | 'Space' | 'SoftBreak';

export function isTextLike(node: unknown): boolean {
    const t = (node as { t?: unknown } | null)?.t;
    return t === 'Str' || t === 'Space' || t === 'SoftBreak';
}

export function textOf(node: unknown): string {
    const n = node as { t?: unknown; c?: unknown };
    if (n.t === 'Str') return String(n.c);
    if (n.t === 'Space') return ' ';
    if (n.t === 'SoftBreak') return '\n';
    return '';
}

/**
 * `at` is the segment's offset in the new text (eq/ins) or the position it
 * was removed from (del).
 */
export type Seg = { kind: 'eq' | 'ins' | 'del'; text: string; at: number };

/** Text-level facts from the last applied edit script. */
export interface TextChangeLedger {
    inserted: Set<unknown>;
    changed: Map<unknown, { old: string; new: string }>;
    removed: { node: unknown; text: string; before: unknown | null; after: unknown | null }[];
}

export function emptyLedger(): TextChangeLedger {
    return { inserted: new Set(), changed: new Map(), removed: [] };
}

export function ledgerFrom(touched: Touched): TextChangeLedger {
    return {
        inserted: new Set(touched.insertedSubtrees.filter(isTextLike)),
        changed: new Map(touched.textChanges.map((c) => [c.node, { old: c.old, new: c.new }])),
        removed: touched.removedText,
    };
}

/** Char-level diff as a flat segment list (LCS; whole-replace past a size cap). */
export function charDiff(oldText: string, newText: string): Seg[] {
    const a = Array.from(oldText);
    const b = Array.from(newText);
    if (a.length * b.length > 400_000) {
        return [{ kind: 'del', text: oldText, at: 0 }, { kind: 'ins', text: newText, at: 0 }];
    }
    const n = a.length;
    const m = b.length;
    const dp = new Uint32Array((n + 1) * (m + 1));
    const at = (i: number, j: number) => i * (m + 1) + j;
    for (let i = n - 1; i >= 0; i--) {
        for (let j = m - 1; j >= 0; j--) {
            dp[at(i, j)] = a[i] === b[j] ? dp[at(i + 1, j + 1)] + 1 : Math.max(dp[at(i + 1, j)], dp[at(i, j + 1)]);
        }
    }
    const segs: Seg[] = [];
    const push = (kind: Seg['kind'], ch: string, pos: number) => {
        const last = segs[segs.length - 1];
        if (last && last.kind === kind) last.text += ch;
        else segs.push({ kind, text: ch, at: pos });
    };
    let i = 0;
    let j = 0;
    while (i < n || j < m) {
        if (i < n && j < m && a[i] === b[j]) { push('eq', a[i], j); i++; j++; }
        else if (j < m && (i >= n || dp[at(i, j + 1)] >= dp[at(i + 1, j)])) { push('ins', b[j], j); j++; }
        else { push('del', a[i], i); i++; }
    }
    return segs;
}

/** Segments for a run of text-like nodes according to the ledger; plain if it says nothing. */
export function segmentsFromLedger(nodes: unknown[], ledger: TextChangeLedger): Seg[] {
    const inRun = new Set<unknown>(nodes);
    const segs: Seg[] = [];
    let pos = 0;
    const push = (kind: Seg['kind'], text: string, at: number) => {
        if (text.length === 0) return;
        const last = segs[segs.length - 1];
        // Deleted text occupies no new-text offset, so consecutive deletions
        // share `at`; merge them. Other kinds merge when contiguous.
        const joins = last && last.kind === kind && (kind === 'del' ? last.at === at : last.at + last.text.length === at);
        if (joins) last.text += text;
        else segs.push({ kind, text, at });
    };
    // A deletion's neighbours were recorded at the moment it happened; a
    // neighbour may itself have been deleted since, so resolve through
    // other removals to nodes that still exist.
    const removedByNode = new Map(ledger.removed.map((r) => [r.node, r]));
    const resolve = (node: unknown | null, side: 'before' | 'after'): unknown | null => {
        let cur = node;
        for (let guard = 0; cur !== null && removedByNode.has(cur) && guard < 1000; guard++) cur = removedByNode.get(cur)![side];
        return cur;
    };
    const placed = ledger.removed.map((r) => ({ text: r.text, before: resolve(r.before, 'before'), after: resolve(r.after, 'after') }));
    const delBefore = (node: unknown) => {
        for (const r of placed) if (r.before === node) push('del', r.text, pos);
    };
    const delAfter = (node: unknown) => {
        for (const r of placed) if (r.after === node && !(r.before !== null && inRun.has(r.before))) push('del', r.text, pos);
    };
    for (const node of nodes) {
        delBefore(node);
        const text = textOf(node);
        const change = ledger.changed.get(node);
        if (ledger.inserted.has(node)) {
            push('ins', text, pos);
        } else if (change) {
            for (const seg of charDiff(change.old, change.new)) push(seg.kind, seg.text, pos + seg.at);
        } else {
            push('eq', text, pos);
        }
        pos += text.length;
        delAfter(node);
    }
    return segs;
}

/**
 * Map a caret offset in `oldText` to the corresponding offset in `newText`,
 * like an editor cursor: text inserted before it shifts it right, text
 * deleted before it shifts it left, a deletion spanning it collapses it to
 * the deletion's start, and an insertion exactly at it leaves it after the
 * inserted text (it sticks to the character on its right, consistent with
 * boundaries attaching to the following node).
 */
export function mapOffsetThroughChange(oldText: string, newText: string, offset: number): number {
    let oldPos = 0;
    let newPos = 0;
    for (const seg of charDiff(oldText, newText)) {
        const len = Array.from(seg.text).length;
        switch (seg.kind) {
            case 'eq':
                if (offset < oldPos + len) return newPos + (offset - oldPos);
                oldPos += len;
                newPos += len;
                break;
            case 'del':
                if (offset < oldPos + len) return newPos;
                oldPos += len;
                break;
            case 'ins':
                newPos += len;
                break;
        }
    }
    return newPos;
}
