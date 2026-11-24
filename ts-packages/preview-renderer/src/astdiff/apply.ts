/**
 * Apply an edit script to a Pandoc-JSON AST *in place*.
 *
 * Only the nodes an op names are touched; every other node object keeps its
 * identity, which is what lets React (keyed by node identity) leave their
 * DOM alone and lets callers hold stable references into the AST.
 */

import type { PandocAST } from '../framework/types';
import { formatPath, type Op, type Path, type Step } from './ops';
import { describe, type Desc, type Elem, type SlotAccess } from './view';

type Located = { value: unknown; elem: Elem };

function rootDesc(root: PandocAST): Desc {
    const r = root as unknown as Record<string, unknown>;
    return {
        kind: 'root',
        elem: 'block',
        attr: null,
        props: {},
        text: null,
        slots: [{
            name: 'blocks',
            elem: 'block',
            read: () => (r.blocks as unknown[]) ?? [],
            write: () => { if (!Array.isArray(r.blocks)) r.blocks = []; return r.blocks as unknown[]; },
            replace: (next) => { r.blocks = next; },
        }],
        main: 'blocks',
    };
}

function slotOf(desc: Desc, name: string, path: Path): SlotAccess {
    const slot = desc.slots.find((s) => s.name === name);
    if (!slot) throw new Error(`astdiff: no slot "${name}" on ${desc.kind} at ${formatPath(path)}`);
    return slot;
}

function descAt(root: PandocAST, steps: Step[]): { desc: Desc; node: Located } {
    let desc = rootDesc(root);
    let node: Located = { value: root, elem: 'block' };
    for (let k = 0; k < steps.length; k++) {
        const step = steps[k];
        const slot = slotOf(desc, step.slot, steps.slice(0, k + 1));
        const list = slot.write();
        if (step.index >= list.length) {
            throw new Error(`astdiff: index ${step.index} out of range in ${step.slot} at ${formatPath(steps.slice(0, k + 1))}`);
        }
        node = { value: list[step.index], elem: slot.elem };
        desc = describe(node.value, node.elem);
    }
    return { desc, node };
}

/** The live child list containing the node at `path`, plus its index. */
function listOf(root: PandocAST, path: Path): { list: unknown[]; index: number; elem: Elem } {
    if (path.length === 0) throw new Error('astdiff: op path must not be empty');
    const last = path[path.length - 1];
    const { desc } = descAt(root, path.slice(0, -1));
    const slot = slotOf(desc, last.slot, path);
    return { list: slot.write(), index: last.index, elem: slot.elem };
}

/**
 * Copy of a node that shares no *container* with the original: the node,
 * its `c`, and any array directly inside `c` (list attributes, link
 * targets, captions) are fresh, so setting a property on the copy never
 * writes through to the original. Child nodes themselves are shared — the
 * caller replaces the main child list anyway.
 */
function cloneShell(value: unknown): unknown {
    const sliceInner = (arr: unknown[]) => arr.map((x) => (Array.isArray(x) ? x.slice() : x));
    if (Array.isArray(value)) return sliceInner(value);
    const obj = value as Record<string, unknown>;
    const copy: Record<string, unknown> = { ...obj };
    if (Array.isArray(obj.c)) copy.c = sliceInner(obj.c as unknown[]);
    return copy;
}

/** Follow `via` (single-child steps) from `node`, then return `slot`. */
function innerSlot(node: unknown, elem: Elem, via: Step[], slot: string, path: Path): SlotAccess {
    let value = node;
    let desc = describe(value, elem);
    for (const step of via) {
        const s = slotOf(desc, step.slot, path);
        const list = s.write();
        if (step.index >= list.length) throw new Error(`astdiff: ${step.slot}[${step.index}] missing inside ${desc.kind} at ${formatPath(path)}`);
        value = list[step.index];
        desc = describe(value, s.elem);
    }
    return slotOf(desc, slot, path);
}

/**
 * Which node objects an edit script touched, for change highlighting.
 * Node objects are the AST values themselves (the document root for the
 * top-level list), so a renderer that keeps per-node DOM can map them to
 * elements directly.
 */
export interface Touched {
    /** Nodes inserted with their whole subtree. */
    insertedSubtrees: unknown[];
    /** Nodes that are new but whose children are not (wrappers, split halves, temporary shells). */
    createdNodes: unknown[];
    /** Existing nodes whose own fields or text changed, or that merged or split. */
    changedNodes: unknown[];
    /** Nodes deleted with their subtree (no longer in the tree). */
    removedNodes: unknown[];
    /** Containers that had a child unwrapped out of them. */
    removedFrom: unknown[];
    /** `change_text` ops, by node, with the old and new text. */
    textChanges: { node: unknown; old: string; new: string }[];
    /**
     * Deleted text-like inlines (Str / Space / SoftBreak), with the siblings
     * that surrounded them at the moment of deletion, so a renderer can
     * show the removed characters in place.
     */
    removedText: { node: unknown; text: string; before: unknown | null; after: unknown | null }[];
    /**
     * Nodes whose child lists changed (the document root for the top-level
     * list): a renderer that mirrors the AST by node identity re-places
     * exactly these nodes' children and leaves everything else alone.
     */
    relisted: unknown[];
    /** A text leaf split in two: `node` keeps chars `[0, at)`, `second` gets the rest. */
    textSplits: { node: unknown; second: unknown; at: number }[];
    /** A text leaf merged into the one before it: `absorbed`'s text now starts at `at` in `node`. */
    textMerges: { node: unknown; absorbed: unknown; at: number }[];
}

export function emptyTouched(): Touched {
    return { insertedSubtrees: [], createdNodes: [], changedNodes: [], removedNodes: [], removedFrom: [], textChanges: [], removedText: [], relisted: [], textSplits: [], textMerges: [] };
}

function textOf(node: unknown): string {
    const n = node as { t?: unknown; c?: unknown };
    if (n.t === 'Str') return String(n.c);
    if (n.t === 'Space') return ' ';
    if (n.t === 'SoftBreak') return '\n';
    return '';
}

function parentOf(root: PandocAST, path: Path): unknown {
    return descAt(root, path.slice(0, -1)).node.value;
}

export function applyOp(root: PandocAST, op: Op, touched: Touched = emptyTouched()): Touched {
    switch (op.op) {
        case 'change_attr': {
            const { desc, node } = descAt(root, op.path);
            if (!desc.attr) throw new Error(`astdiff: change_attr on ${desc.kind} without attr`);
            desc.attr.set(op.attr);
            touched.changedNodes.push(node.value);
            return touched;
        }
        case 'change_property': {
            const { desc, node } = descAt(root, op.path);
            const prop = desc.props[op.name];
            if (!prop) throw new Error(`astdiff: no property "${op.name}" on ${desc.kind}`);
            prop.set(op.new);
            touched.changedNodes.push(node.value);
            return touched;
        }
        case 'change_text': {
            const { desc, node } = descAt(root, op.path);
            if (!desc.text) throw new Error(`astdiff: change_text on ${desc.kind} without text`);
            desc.text.set(op.new);
            touched.changedNodes.push(node.value);
            touched.textChanges.push({ node: node.value, old: op.old, new: op.new });
            // A renderer may show text-like inlines as part of a run rather
            // than one by one; the run belongs to the parent's child list.
            if (isTextLike(node.value)) touched.relisted.push(parentOf(root, op.path));
            return touched;
        }
        case 'insert': {
            const { list, index } = listOf(root, op.path);
            if (index > list.length) throw new Error(`astdiff: insert index out of range at ${formatPath(op.path)}`);
            list.splice(index, 0, op.node);
            touched.insertedSubtrees.push(op.node);
            touched.relisted.push(parentOf(root, op.path));
            return touched;
        }
        case 'delete': {
            const { list, index } = listOf(root, op.path);
            if (index >= list.length) throw new Error(`astdiff: delete index out of range at ${formatPath(op.path)}`);
            const [removed] = list.splice(index, 1);
            if (isTextLike(removed)) {
                // Painted character by character by the renderer's text runs.
                touched.removedText.push({ node: removed, text: textOf(removed), before: list[index] ?? null, after: list[index - 1] ?? null });
            } else {
                touched.removedNodes.push(removed);
            }
            touched.relisted.push(parentOf(root, op.path));
            return touched;
        }
        case 'wrap': {
            const { list, index, elem } = listOf(root, op.path);
            if (index + op.count > list.length) throw new Error(`astdiff: wrap range out of range at ${formatPath(op.path)}`);
            const inner = list.splice(index, op.count, op.wrapper);
            const target = innerSlot(op.wrapper, elem, op.via, op.slot, op.path).write();
            target.length = 0;
            target.push(...inner);
            touched.createdNodes.push(op.wrapper);
            touched.relisted.push(parentOf(root, op.path), op.wrapper);
            // Intermediate single-child levels of the wrapper (e.g. the list
            // item of a one-item bullet list) are new as well.
            let value = op.wrapper;
            let desc = describe(value, elem);
            for (const step of op.via) {
                const slot = slotOf(desc, step.slot, op.path);
                value = slot.write()[step.index];
                desc = describe(value, slot.elem);
                touched.createdNodes.push(value);
                touched.relisted.push(value);
            }
            return touched;
        }
        case 'unwrap': {
            const { list, index, elem } = listOf(root, op.path);
            const children = innerSlot(list[index], elem, op.via, op.slot, op.path).read();
            list.splice(index, 1, ...children);
            if (!op.replaced) touched.removedFrom.push(parentOf(root, op.path));
            touched.relisted.push(parentOf(root, op.path));
            return touched;
        }
        case 'split': {
            const { list, index, elem } = listOf(root, op.path);
            const node = list[index];
            const desc = describe(node, elem);
            touched.changedNodes.push(node);
            touched.relisted.push(node, parentOf(root, op.path));
            if (desc.text) {
                const chars = Array.from(String(desc.text.get()));
                const second = cloneShell(node);
                desc.text.set(chars.slice(0, op.at).join(''));
                describe(second, elem).text!.set(chars.slice(op.at).join(''));
                list.splice(index + 1, 0, second);
                touched.createdNodes.push(second);
                touched.textSplits.push({ node, second, at: op.at });
                return touched;
            }
            if (desc.main === null) throw new Error(`astdiff: split ${desc.kind}, which has no main slot`);
            const main = slotOf(desc, desc.main, op.path);
            const rest = main.write().splice(op.at);
            if (main.replace) {
                const second = cloneShell(node);
                slotOf(describe(second, elem), desc.main, op.path).replace!(rest);
                list.splice(index + 1, 0, second);
                touched.createdNodes.push(second);
            } else {
                // The node *is* its child list (list item, line, definition).
                list.splice(index + 1, 0, rest);
                touched.createdNodes.push(rest);
            }
            return touched;
        }
        case 'merge': {
            const { list, index, elem } = listOf(root, op.path);
            if (index + 1 >= list.length) throw new Error(`astdiff: merge needs a next sibling at ${formatPath(op.path)}`);
            const node = list[index];
            const next = list.splice(index + 1, 1)[0];
            const desc = describe(node, elem);
            const nextDesc = describe(next, elem);
            touched.changedNodes.push(node);
            touched.relisted.push(node, parentOf(root, op.path));
            if (desc.text && nextDesc.text) {
                const own = String(desc.text.get());
                desc.text.set(own + String(nextDesc.text.get()));
                touched.textMerges.push({ node, absorbed: next, at: Array.from(own).length });
                return touched;
            }
            if (desc.main === null || nextDesc.main === null) throw new Error(`astdiff: merge ${desc.kind} without main slot`);
            slotOf(desc, desc.main, op.path).write().push(...slotOf(nextDesc, nextDesc.main, op.path).read());
            return touched;
        }
    }
}

function isTextLike(node: unknown): boolean {
    const t = (node as { t?: unknown } | null)?.t;
    return t === 'Str' || t === 'Space' || t === 'SoftBreak';
}

function samePath(a: Path, b: Path): boolean {
    return a.length === b.length && a.every((s, i) => s.slot === b[i].slot && s.index === b[i].index);
}

export function applyOps(root: PandocAST, ops: Op[]): Touched {
    const touched = emptyTouched();
    for (let k = 0; k < ops.length; k++) {
        const op = ops[k];
        const next = ops[k + 1];
        // An unwrap immediately re-wrapped at the same path is a type change
        // (Para → Plain, Emph → Strong): nothing was removed from the parent.
        if (op.op === 'unwrap' && next?.op === 'wrap' && samePath(op.path, next.path)) {
            applyOp(root, { ...op, replaced: true }, touched);
            continue;
        }
        applyOp(root, op, touched);
    }
    // A node created by one op and removed by a later one (a temporary
    // shell merged away) is not on screen; drop it.
    const present = new Set<unknown>();
    const mark = (v: unknown, elem: Elem) => {
        present.add(v);
        for (const slot of describe(v, elem).slots) for (const c of slot.read()) mark(c, slot.elem);
    };
    for (const b of root.blocks) mark(b, 'block');
    present.add(root);
    const keep = (list: unknown[]) => Array.from(new Set(list.filter((v) => present.has(v))));
    return {
        insertedSubtrees: keep(touched.insertedSubtrees),
        createdNodes: keep(touched.createdNodes),
        changedNodes: keep(touched.changedNodes),
        // Removed nodes are absent by definition; only drop ones that were
        // created *and* removed within this script (never on screen).
        removedNodes: Array.from(new Set(touched.removedNodes.filter(
            (v) => !touched.createdNodes.includes(v) && !touched.insertedSubtrees.includes(v),
        ))),
        removedFrom: keep(touched.removedFrom),
        textChanges: touched.textChanges.filter((c) => present.has(c.node)),
        removedText: touched.removedText,
        relisted: keep(touched.relisted),
        textSplits: touched.textSplits,
        textMerges: touched.textMerges,
    };
}
