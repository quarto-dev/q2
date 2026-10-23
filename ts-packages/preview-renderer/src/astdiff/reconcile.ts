/**
 * Reconcile a freshly parsed AST into the AST a renderer already holds.
 *
 * `reconcileAst(current, fresh)` diffs the two, applies the edit script to
 * `current` in place, then copies the writer's source annotations (`s`,
 * `l`, ...) from `fresh` onto the matching `current` nodes — those shift
 * on every edit even for untouched content, so they must always come from
 * the latest parse. Document-level fields (`meta`, `astContext`, ...) are
 * taken from `fresh` wholesale; metadata is outside the differ's scope.
 *
 * If anything goes wrong (an applier error, or the reconciled tree not
 * matching `fresh` structurally) the caller gets `fresh` back and a
 * warning is logged — correctness never depends on the diff.
 */

import type { PandocAST } from '../framework/types';
import { applyOps, emptyTouched, type Touched } from './apply';
import { diffBlocks } from './diff';
import { describeOp, type Op } from './ops';
import { ANNOTATION_KEYS, buildView, type View } from './view';

export interface ReconcileResult {
    /** The AST to render: `current` (mutated) when reused, else `fresh`. */
    ast: PandocAST;
    ops: Op[];
    /** False on first render or when reconciliation fell back to `fresh`. */
    reused: boolean;
    /** Node objects the edit script touched (empty unless `reused`). */
    touched: Touched;
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
    return v !== null && typeof v === 'object' && !Array.isArray(v);
}

/**
 * Walk two structurally identical views in lockstep, copying annotation
 * keys from `fresh` onto `current`. Returns false on any structural
 * mismatch (which means the edit script did not reproduce `fresh`).
 */
function syncViews(current: View, fresh: View): boolean {
    if (current.kind !== fresh.kind || current.hash !== fresh.hash) return false;
    if (isPlainObject(current.value) && isPlainObject(fresh.value)) {
        for (const key of ANNOTATION_KEYS) {
            if (key in fresh.value) current.value[key] = fresh.value[key];
            else if (key in current.value) delete current.value[key];
        }
    }
    if (current.slots.length !== fresh.slots.length) return false;
    for (let s = 0; s < current.slots.length; s++) {
        const a = current.slots[s].children;
        const b = fresh.slots[s].children;
        if (a.length !== b.length) return false;
        for (let k = 0; k < a.length; k++) {
            if (!syncViews(a[k], b[k])) return false;
        }
    }
    return true;
}

function syncAnnotations(current: PandocAST, fresh: PandocAST): boolean {
    if (current.blocks.length !== fresh.blocks.length) return false;
    for (let k = 0; k < current.blocks.length; k++) {
        if (!syncViews(buildView(current.blocks[k], 'block'), buildView(fresh.blocks[k], 'block'))) return false;
    }
    return true;
}

export function reconcileAst(current: PandocAST | null, fresh: PandocAST): ReconcileResult {
    if (current === null) return { ast: fresh, ops: [], reused: false, touched: emptyTouched() };

    const ops = diffBlocks(current.blocks, fresh.blocks);
    let touched: Touched;
    try {
        touched = applyOps(current, ops);
    } catch (err) {
        console.warn('[astdiff] applying the edit script failed; falling back to the fresh AST', err, ops.map(describeOp));
        return { ast: fresh, ops, reused: false, touched: emptyTouched() };
    }
    if (!syncAnnotations(current, fresh)) {
        console.warn('[astdiff] reconciled AST does not match the fresh parse; falling back to the fresh AST', ops.map(describeOp));
        return { ast: fresh, ops, reused: false, touched: emptyTouched() };
    }

    // Everything outside `blocks` (meta, astContext, pandoc-api-version, ...)
    // comes from the fresh document.
    const cur = current as unknown as Record<string, unknown>;
    const nxt = fresh as unknown as Record<string, unknown>;
    for (const key of Object.keys(cur)) if (key !== 'blocks' && !(key in nxt)) delete cur[key];
    for (const key of Object.keys(nxt)) if (key !== 'blocks') cur[key] = nxt[key];

    return { ast: current, ops, reused: true, touched };
}
