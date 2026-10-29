/**
 * Identity-based React keys for AST children.
 *
 * By default `renderChildren` keys children by index, which is right when
 * every render receives a brand-new AST (q2-preview): React reuses DOM by
 * position. When a renderer instead keeps one AST and patches it in place
 * (q2-debug via `astdiff/reconcileAst`), unchanged nodes are the *same
 * objects* across renders, and keying by identity lets React keep their
 * DOM through insertions, deletions and wraps around them.
 *
 * Opt in with `enableStableNodeKeys()` from the renderer entry. Keys are
 * handed out lazily from a WeakMap, so they never leak and a node keeps
 * its key for as long as it exists.
 */

let stableKeys = false;
let nextId = 1;
const ids = new WeakMap<object, number>();

export function enableStableNodeKeys(): void {
    stableKeys = true;
}

/** True when `value` has been handed a key before (i.e. was rendered already). */
export function hasNodeKey(value: unknown): boolean {
    return value !== null && typeof value === 'object' && ids.has(value);
}

/** Key for the `index`-th child `value` of a node's child list. */
export function childKey(value: unknown, index: number): React.Key {
    if (!stableKeys || value === null || typeof value !== 'object') return index;
    let id = ids.get(value);
    if (id === undefined) {
        id = nextId++;
        ids.set(value, id);
    }
    return `n${id}`;
}
