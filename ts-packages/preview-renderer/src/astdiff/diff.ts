/**
 * The edit-script algorithm.
 *
 * Each child list is diffed in two layers:
 *  1. Identical subtrees (equal structural hash) are anchored with a longest
 *     common subsequence, so unchanged content is never touched.
 *  2. Each gap between anchors is walked greedily, left to right:
 *     - same kind: look for a split or merge, otherwise recurse into the pair;
 *     - different kinds: look for an unwrap, a wrap, or an unwrap+wrap "type
 *       change", otherwise delete or insert, whichever lets the remaining
 *       nodes pair up sooner.
 *
 * Unwrap, split and merge rewrite the "before" list in place and restart
 * from the current position so the LCS layer can re-anchor.
 */

import type { Attr } from '../framework/types';
import type { AttrChange, Op, Path } from './ops';
import {
    buildView,
    mainChildren,
    mainSlot,
    shellOf,
    withMainChildren,
    withText,
    wrapChain,
    type Elem,
    type View,
} from './view';

/** Diff two block lists, addressed as the root's `blocks` slot. */
export function diffBlocks(before: unknown[], after: unknown[]): Op[] {
    const a = before.map((b) => buildView(b, 'block'));
    const b = after.map((x) => buildView(x, 'block'));
    const ops: Op[] = [];
    diffLists(a, b, [], 'blocks', 'block', ops);
    return ops;
}

function child(path: Path, slot: string, index: number): Path {
    return [...path, { slot, index }];
}

// --- attr diff -------------------------------------------------------------

function lcsFlags<T>(a: T[], b: T[], eq: (x: T, y: T) => boolean): [boolean[], boolean[]] {
    const n = a.length;
    const m = b.length;
    const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
    for (let i = n - 1; i >= 0; i--) {
        for (let j = m - 1; j >= 0; j--) {
            dp[i][j] = eq(a[i], b[j]) ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
        }
    }
    const keptA = new Array<boolean>(n).fill(false);
    const keptB = new Array<boolean>(m).fill(false);
    let i = 0;
    let j = 0;
    while (i < n && j < m) {
        if (eq(a[i], b[j])) {
            keptA[i] = true;
            keptB[j] = true;
            i++;
            j++;
        } else if (dp[i + 1][j] >= dp[i][j + 1]) {
            i++;
        } else {
            j++;
        }
    }
    return [keptA, keptB];
}

export function attrChanges(a: Attr, b: Attr): AttrChange[] {
    const changes: AttrChange[] = [];
    if (a[0] !== b[0]) changes.push({ change: 'set_id', old: a[0], new: b[0] });
    const [keptA, keptB] = lcsFlags(a[1], b[1], (x, y) => x === y);
    a[1].forEach((cls, i) => { if (!keptA[i]) changes.push({ change: 'remove_class', class: cls }); });
    b[1].forEach((cls, i) => { if (!keptB[i]) changes.push({ change: 'add_class', class: cls }); });
    const oldMap = new Map(a[2]);
    const newKeys = new Set<string>();
    for (const [key, value] of b[2]) {
        newKeys.add(key);
        const old = oldMap.get(key);
        if (old !== value) changes.push({ change: 'set_attribute', key, old: old ?? null, new: value });
    }
    for (const [key, old] of a[2]) {
        if (!newKeys.has(key)) changes.push({ change: 'remove_attribute', key, old });
    }
    return changes;
}

// --- node diff -------------------------------------------------------------

function diffNode(a: View, b: View, path: Path, ops: Op[]): void {
    if (a.hash === b.hash) return;
    if (a.attr !== null && b.attr !== null && a.attrJson !== b.attrJson) {
        ops.push({ op: 'change_attr', path, changes: attrChanges(a.attr, b.attr), attr: b.attr });
    }
    for (const name of Object.keys(b.propsJson)) {
        if (a.propsJson[name] !== b.propsJson[name]) {
            ops.push({ op: 'change_property', path, name, old: a.props[name], new: b.props[name] });
        }
    }
    if (a.text !== null && b.text !== null && a.text !== b.text) {
        ops.push({ op: 'change_text', path, old: a.text, new: b.text });
    }
    for (const bslot of b.slots) {
        const aslot = a.slots.find((s) => s.name === bslot.name);
        if (aslot) {
            diffLists(aslot.children, bslot.children, path, bslot.name, bslot.elem, ops);
        } else {
            bslot.children.forEach((c, k) => ops.push({ op: 'insert', path: child(path, bslot.name, k), node: c.value }));
        }
    }
    for (const aslot of a.slots) {
        if (!b.slots.some((s) => s.name === aslot.name)) {
            for (let k = 0; k < aslot.children.length; k++) {
                ops.push({ op: 'delete', path: child(path, aslot.name, 0) });
            }
        }
    }
}

// --- list diff -------------------------------------------------------------

interface Cursor {
    i: number;
    j: number;
    pos: number;
}

/** LCS of identical subtrees as absolute `[ai, bj]` index pairs. */
function anchors(a: View[], b: View[], aOff: number, bOff: number): [number, number][] {
    let lo = 0;
    while (lo < a.length && lo < b.length && a[lo].hash === b[lo].hash) lo++;
    let hiA = a.length;
    let hiB = b.length;
    while (hiA > lo && hiB > lo && a[hiA - 1].hash === b[hiB - 1].hash) { hiA--; hiB--; }
    const out: [number, number][] = [];
    for (let k = 0; k < lo; k++) out.push([aOff + k, bOff + k]);
    const midA = a.slice(lo, hiA);
    const midB = b.slice(lo, hiB);
    if (midA.length > 0 && midB.length > 0) {
        // Recover the pairing by walking both kept sequences in order.
        const [keptA, keptB] = lcsFlags(midA, midB, (x, y) => x.hash === y.hash);
        let j = 0;
        for (let i = 0; i < midA.length; i++) {
            if (!keptA[i]) continue;
            while (!keptB[j]) j++;
            out.push([aOff + lo + i, bOff + lo + j]);
            j++;
        }
    }
    for (let k = 0; k < a.length - hiA; k++) out.push([aOff + hiA + k, bOff + hiB + k]);
    return out;
}

function diffLists(aIn: View[], b: View[], parent: Path, slot: string, elem: Elem, ops: Op[]): void {
    const a = aIn.slice();
    const c: Cursor = { i: 0, j: 0, pos: 0 };
    for (;;) {
        const pairs = anchors(a.slice(c.i), b.slice(c.j), c.i, c.j);
        pairs.push([a.length, b.length]);
        let restart = false;
        for (const [ai, bj] of pairs) {
            if (diffGap(a, ai, b, bj, c, parent, slot, elem, ops) === 'spliced') {
                restart = true;
                break;
            }
            if (ai < a.length) {
                c.i = ai + 1;
                c.j = bj + 1;
                c.pos += 1;
            }
        }
        if (!restart) return;
    }
}

function hashSet(views: View[]): Set<number> {
    return new Set(views.map((v) => v.hash));
}

function diffGap(a: View[], aEnd: number, b: View[], bEnd: number, c: Cursor, parent: Path, slot: string, elem: Elem, ops: Op[]): 'done' | 'spliced' {
    for (;;) {
        const hasA = c.i < aEnd;
        const hasB = c.j < bEnd;
        const path = child(parent, slot, c.pos);
        if (!hasA && !hasB) return 'done';
        if (!hasB) {
            ops.push({ op: 'delete', path });
            c.i++;
            continue;
        }
        if (!hasA) {
            ops.push({ op: 'insert', path, node: b[c.j].value });
            c.j++;
            c.pos++;
            continue;
        }

        const x = a[c.i];
        const y = b[c.j];

        if (x.hash === y.hash) {
            c.i++; c.j++; c.pos++;
            continue;
        }

        if (x.kind === y.kind) {
            const at = splitPoint(x, y, b.slice(c.j + 1, bEnd));
            if (at !== null) {
                ops.push({ op: 'split', path, at });
                const [first, second] = splitView(x, at);
                a.splice(c.i, 1, first, second);
                return 'spliced';
            }
            const merge = mergeTarget(x, y, a.slice(c.i + 1, aEnd));
            if (merge !== null && merge.kind === 'sibling') {
                const l = c.i + 1 + merge.index;
                for (let k = c.i + 1; k < l; k++) {
                    ops.push({ op: 'delete', path: child(parent, slot, c.pos + 1) });
                }
                ops.push({ op: 'merge', path });
                const merged = mergeViews(x, a[l]);
                a.splice(c.i, l - c.i + 1, merged);
                return 'spliced';
            }
            if (merge !== null && merge.kind === 'absorb') {
                // The following siblings are what `y` gained: wrap them in an
                // empty shell of the same kind and merge it into `x`. The
                // shell is temporary — merge drops it — so the siblings keep
                // their identity and `x` keeps its own.
                const xm = mainSlot(x)!;
                ops.push({
                    op: 'wrap',
                    path: child(parent, slot, c.pos + 1),
                    count: merge.count,
                    wrapper: shellOf(y),
                    via: [],
                    slot: xm.name,
                });
                ops.push({ op: 'merge', path });
                const absorbed = a.slice(c.i + 1, c.i + 1 + merge.count);
                const merged = withMainChildren(x, [...mainChildren(x), ...absorbed]);
                a.splice(c.i, 1 + merge.count, merged);
                return 'spliced';
            }
            diffNode(x, y, path, ops);
            c.i++; c.j++; c.pos++;
            continue;
        }

        // Different kinds. Unwrap `x` if the nodes it (possibly indirectly)
        // wraps show up in the gap, or if it wraps a single node that `y`
        // is a re-typed/edited version of.
        const xChain = wrapChain(x, elem);
        if (xChain) {
            const gap = hashSet(flattenWrappers(b.slice(c.j, bEnd), elem));
            const xc = xChain.children;
            // Look through wrappers on both sides: the nodes `x` wraps may
            // themselves be wrapped once more, and may end up (un)wrapped
            // differently in the gap.
            const exact = flattenWrappers(xc, elem).some((ch) => gap.has(ch.hash));
            const onlyChildLikeY = xc.length === 1 && looksLike(xc[0], y);
            if (exact || onlyChildLikeY) {
                ops.push({ op: 'unwrap', path, via: xChain.via, slot: xChain.slot });
                a.splice(c.i, 1, ...xc);
                return 'spliced';
            }
        }

        // Wrap a run of `a` nodes in `y` if some of them are the nodes `y`
        // (possibly indirectly) wraps, or `y` wraps a single node that is a
        // re-typed/edited version of `x`.
        const yChain = wrapChain(y, elem);
        if (yChain) {
            const yc = yChain.children;
            const wanted = hashSet(flattenWrappers(yc, elem));
            let last: number | null = null;
            for (let k = aEnd - 1; k >= c.i; k--) {
                if (wanted.has(a[k].hash)) { last = k - c.i; break; }
            }
            if (last === null && yc.length === 1 && looksLike(yc[0], x)) last = 0;
            if (last !== null) {
                const count = last + 1;
                ops.push({ op: 'wrap', path, count, wrapper: y.value, via: yChain.via, slot: yChain.slot });
                diffLists(a.slice(c.i, c.i + count), yc, [...path, ...yChain.via], yChain.slot, elem, ops);
                c.i += count; c.j++; c.pos++;
                continue;
            }
        }

        // "Type change": same child kind and some shared children → unwrap
        // the old container and wrap the children in the new one.
        const xm = mainSlot(x);
        const ym = mainSlot(y);
        if (xm && ym && xm.elem === ym.elem && xm.children.length > 0) {
            const shared = hashSet(ym.children);
            if (xm.children.some((ch) => shared.has(ch.hash))) {
                ops.push({ op: 'unwrap', path, via: [], slot: xm.name });
                ops.push({ op: 'wrap', path, count: xm.children.length, wrapper: y.value, via: [], slot: ym.name });
                diffLists(xm.children, ym.children, path, ym.name, ym.elem, ops);
                c.i++; c.j++; c.pos++;
                continue;
            }
        }

        // No structural relation: delete `x` or insert `y`, whichever lets
        // the other pair up with a same-kind node sooner.
        const da = b.slice(c.j + 1, bEnd).findIndex((v) => v.kind === x.kind);
        const db = a.slice(c.i + 1, aEnd).findIndex((v) => v.kind === y.kind);
        const insertFirst = da !== -1 && (db === -1 || da < db);
        if (insertFirst) {
            ops.push({ op: 'insert', path, node: y.value });
            c.j++; c.pos++;
        } else {
            ops.push({ op: 'delete', path });
            c.i++;
        }
    }
}

/**
 * `a` is plausibly the same node as `b` after an edit: same kind, or same
 * child kind with at least one child in common (a type change, e.g. Para
 * → Plain when a paragraph becomes a tight list item).
 */
function looksLike(a: View, b: View): boolean {
    if (a.kind === b.kind) return true;
    const am = mainSlot(a);
    const bm = mainSlot(b);
    if (!am || !bm || am.elem !== bm.elem) return false;
    const shared = hashSet(bm.children);
    return am.children.some((ch) => shared.has(ch.hash));
}

// --- split / merge detection ----------------------------------------------

function sameHashes(a: View[], b: View[]): boolean {
    return a.length === b.length && a.every((x, i) => x.hash === b[i].hash);
}

function isSuffix(part: View[], whole: View[]): boolean {
    return part.length > 0 && part.length <= whole.length && sameHashes(part, whole.slice(whole.length - part.length));
}

function firstChildIn(v: View, list: View[]): boolean {
    const first = mainChildren(v)[0];
    return first !== undefined && list.some((l) => l.hash === first.hash);
}

/**
 * `x` and `y` have the same kind. Returns `at` when `y` is a proper prefix
 * of `x` and the remainder of `x` shows up later in the gap: in a same-kind
 * node (as a suffix of the remainder, or sharing its first child), or
 * directly as a sibling — the container shrank and released those nodes,
 * which the unwrap rule then handles after the split.
 */
function splitPoint(x: View, y: View, restB: View[]): number | null {
    if (x.text !== null && y.text !== null) {
        const xc = Array.from(x.text);
        const yc = Array.from(y.text);
        if (yc.length === 0 || yc.length >= xc.length) return null;
        for (let k = 0; k < yc.length; k++) if (xc[k] !== yc[k]) return null;
        const remainder = xc.slice(yc.length).join('');
        const found = restB.some((v) => v.kind === x.kind && v.text !== null && v.text.length > 0 && remainder.endsWith(v.text));
        return found ? yc.length : null;
    }
    const xm = mainSlot(x);
    const ym = mainSlot(y);
    if (!xm || !ym) return null;
    // `k` may be 0: the container emptied and released everything (an
    // opener immediately closed, with the old content now outside it).
    const k = ym.children.length;
    if (k >= xm.children.length || !sameHashes(xm.children.slice(0, k), ym.children)) return null;
    const remainder = xm.children.slice(k);
    const suffix = restB.some((v) => v.kind === x.kind && isSuffix(mainChildren(v), remainder));
    const nextOverlaps = restB.length > 0 && restB[0].kind === x.kind && firstChildIn(restB[0], remainder);
    // The released nodes may reappear directly, or wrapped (the container
    // shrank and a new container took over its tail), or lightly edited.
    const gap = flattenWrappers(restB, x.elem);
    const releasedNodes = flattenWrappers(remainder, x.elem);
    const released = releasedNodes.some((r) => gap.some((g) => g.hash === r.hash || looksLike(r, g)));
    return suffix || nextOverlaps || released ? k : null;
}

/**
 * `views` plus, for every view that is a wrapper of `elem` nodes, the nodes
 * it (transitively) wraps. Lets grow/shrink detection see through a new
 * container that appeared around the moved nodes at the same time.
 */
function flattenWrappers(views: View[], elem: Elem): View[] {
    const out: View[] = [];
    const visit = (v: View) => {
        out.push(v);
        const chain = wrapChain(v, elem);
        if (chain) for (const c of chain.children) visit(c);
    };
    for (const v of views) visit(v);
    return out;
}

type MergeTarget =
    /** Merge `x` with `restA[index]`, deleting anything in between. */
    | { kind: 'sibling'; index: number }
    /** `x` grew to absorb the next `count` siblings themselves. */
    | { kind: 'absorb'; count: number };

/**
 * `x` and `y` have the same kind, and `x` is a proper prefix of `y`. Says
 * where the rest of `y` came from: a later same-kind sibling of `x` (merge
 * with it), or the siblings right after `x` themselves (the container grew
 * to absorb them).
 */
function mergeTarget(x: View, y: View, restA: View[]): MergeTarget | null {
    if (x.text !== null && y.text !== null) {
        const xc = Array.from(x.text);
        const yc = Array.from(y.text);
        if (xc.length === 0 || xc.length >= yc.length) return null;
        for (let k = 0; k < xc.length; k++) if (yc[k] !== xc[k]) return null;
        const remainder = yc.slice(xc.length).join('');
        const idx = restA.findIndex((v) => v.kind === x.kind && v.text !== null && v.text.length > 0 && remainder.endsWith(v.text));
        return idx === -1 ? null : { kind: 'sibling', index: idx };
    }
    const xm = mainSlot(x);
    const ym = mainSlot(y);
    if (!xm || !ym) return null;
    // `n` may be 0: an empty container swallowing what follows it.
    const n = xm.children.length;
    if (n >= ym.children.length || !sameHashes(ym.children.slice(0, n), xm.children)) return null;
    const remainder = ym.children.slice(n);
    const idx = restA.findIndex((v) => v.kind === x.kind && isSuffix(mainChildren(v), remainder));
    if (idx !== -1) return { kind: 'sibling', index: idx };
    const next = restA[0];
    if (next !== undefined && next.kind === x.kind && firstChildIn(next, remainder)) return { kind: 'sibling', index: 0 };
    // The remainder is (built from) the run of siblings that follows `x`:
    // take the leading siblings that occur in it — in any position, possibly
    // inside a new wrapper, possibly lightly edited — so that nodes the
    // container also gained, dropped or re-wrapped along the way don't hide
    // the absorption.
    const gained = flattenWrappers(remainder, x.elem);
    let count = 0;
    while (count < restA.length && gained.some((g) => g.hash === restA[count].hash || looksLike(restA[count], g))) count++;
    return count > 0 ? { kind: 'absorb', count } : null;
}

function splitView(x: View, at: number): [View, View] {
    if (x.text !== null) {
        const chars = Array.from(x.text);
        return [withText(x, chars.slice(0, at).join('')), withText(x, chars.slice(at).join(''))];
    }
    const children = mainChildren(x);
    return [withMainChildren(x, children.slice(0, at)), withMainChildren(x, children.slice(at))];
}

function mergeViews(x: View, next: View): View {
    if (x.text !== null && next.text !== null) return withText(x, x.text + next.text);
    return withMainChildren(x, [...mainChildren(x), ...mainChildren(next)]);
}
