/**
 * Edit-script vocabulary for Pandoc-JSON ASTs.
 *
 * A diff is an `Op[]` applied in order; every `path` refers to the tree
 * *as modified by the ops before it*. The vocabulary is deliberately
 * small: edit a node in place (`change_attr` / `change_property` /
 * `change_text`), add or remove a subtree (`insert` / `delete`), add or
 * remove a container around existing siblings (`wrap` / `unwrap`), and
 * divide or join nodes of one kind (`split` / `merge`). There is no
 * "move" (delete + insert) and no "change type" (unwrap + wrap).
 *
 *  - `insert` / `wrap` payloads are the live node objects from the
 *    "after" tree (not copies), so applying a diff moves those subtrees
 *    into the "before" tree by reference.
 *  - `change_attr` also carries the full new `attr` so the applier can
 *    assign it wholesale; `changes` is the human-readable breakdown.
 */

import type { Attr } from '../framework/types';

export interface Step {
    slot: string;
    index: number;
}

/** `[{slot:'blocks',index:2},{slot:'content',index:0}]` → `blocks[2]/content[0]`. */
export type Path = Step[];

export type AttrChange =
    | { change: 'set_id'; old: string; new: string }
    | { change: 'add_class'; class: string }
    | { change: 'remove_class'; class: string }
    | { change: 'set_attribute'; key: string; old: string | null; new: string }
    | { change: 'remove_attribute'; key: string; old: string };

export type Op =
    | { op: 'change_attr'; path: Path; changes: AttrChange[]; attr: Attr }
    | { op: 'change_property'; path: Path; name: string; old: unknown; new: unknown }
    | { op: 'change_text'; path: Path; old: string; new: string }
    /** Insert `node` (a subtree from the after-tree) so it ends up at `path`. */
    | { op: 'insert'; path: Path; node: unknown }
    | { op: 'delete'; path: Path }
    /**
     * Insert `wrapper` at `path` and move the `count` siblings that started
     * at `path` into the child list reached from the wrapper by following
     * `via` (intermediate single-child steps, e.g. `items[0]` of a bullet
     * list) and then `slot` (e.g. `content`). That list is replaced; the
     * rest of the wrapper is taken as-is. `via` is empty for a plain
     * one-level wrapper such as a Div.
     */
    | { op: 'wrap'; path: Path; count: number; wrapper: unknown; via: Step[]; slot: string }
    /**
     * Delete the node at `path`, splicing the children reached by `via` +
     * `slot` (see `wrap`) into its place. Everything else in the node is
     * dropped.
     */
    | {
        op: 'unwrap'; path: Path; via: Step[]; slot: string;
        /** Set by the applier when the next op re-wraps at the same path (a type change). */
        replaced?: boolean;
    }
    /** Split the node at `path` after `at` main children (or text chars). */
    | { op: 'split'; path: Path; at: number }
    /** Merge the node at `path` with its next sibling of the same kind. */
    | { op: 'merge'; path: Path };

export function formatPath(path: Path): string {
    if (path.length === 0) return '<root>';
    return path.map((s) => `${s.slot}[${s.index}]`).join('/');
}

function describeNode(node: unknown): string {
    if (Array.isArray(node)) return 'item';
    if (node !== null && typeof node === 'object') {
        const n = node as { t?: string; c?: unknown; type_name?: string };
        if (n.t === 'Str' && typeof n.c === 'string') return `Str(${JSON.stringify(n.c.slice(0, 24))})`;
        if (n.t === 'Header' && Array.isArray(n.c)) return `Header(level ${String(n.c[0])})`;
        if (typeof n.type_name === 'string') return `Custom(${n.type_name})`;
        if (typeof n.t === 'string') return n.t;
        if (typeof (node as { citationId?: string }).citationId === 'string') {
            return `Citation(${(node as { citationId: string }).citationId})`;
        }
    }
    return 'node';
}

/** One-line human-readable rendering of an op, for logs and debugging. */
export function describeOp(op: Op): string {
    const p = formatPath(op.path);
    switch (op.op) {
        case 'change_attr': {
            const parts = op.changes.map((c) => {
                switch (c.change) {
                    case 'set_id': return `id ${JSON.stringify(c.old)}→${JSON.stringify(c.new)}`;
                    case 'add_class': return `+.${c.class}`;
                    case 'remove_class': return `-.${c.class}`;
                    case 'set_attribute':
                        return c.old === null
                            ? `+${c.key}=${JSON.stringify(c.new)}`
                            : `${c.key}=${JSON.stringify(c.old)}→${JSON.stringify(c.new)}`;
                    case 'remove_attribute': return `-${c.key}`;
                }
            });
            return `${p}: change attr: ${parts.join(' ')}`;
        }
        case 'change_property':
            return `${p}: change ${op.name} ${JSON.stringify(op.old)}→${JSON.stringify(op.new)}`;
        case 'change_text':
            return `${p}: change text ${JSON.stringify(op.old)}→${JSON.stringify(op.new)}`;
        case 'insert': return `${p}: insert ${describeNode(op.node)}`;
        case 'delete': return `${p}: delete`;
        case 'wrap': {
            const inner = op.via.length === 0 ? '' : ` (into ${formatPath(op.via)}/${op.slot})`;
            return `${p}: wrap ${op.count} node(s) in ${describeNode(op.wrapper)}${inner}`;
        }
        case 'unwrap': {
            const inner = op.via.length === 0 ? '' : ` (from ${formatPath(op.via)}/${op.slot})`;
            return `${p}: unwrap${inner}`;
        }
        case 'split': return `${p}: split at ${op.at}`;
        case 'merge': return `${p}: merge with next sibling`;
    }
}
