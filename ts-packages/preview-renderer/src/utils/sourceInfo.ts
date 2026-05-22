/**
 * Pure-function accessors for the source-info pool. Used by Plan 2B's
 * atomic-aware dispatcher gate (in `framework/dispatch.tsx`'s `Node`)
 * and by future features that need source-mapped lookups (preimage
 * navigation, source-mapped diagnostics).
 *
 * Sync contract: `ATOMIC_KINDS` mirrors the kinds returned by
 * `By::is_atomic_kind()` on the Rust side
 * (`crates/quarto-source-map/src/source_info.rs`). Update both together.
 */

import type { SourceInfoEntry, SourceInfoPool } from '../types/sourceInfo';

/**
 * Lookup the source-info entry referenced by `node.s`. Returns
 * `undefined` if the node has no `s`, the pool is missing, or `s` is
 * out of bounds.
 */
export function entryFor(
    node: { s?: number },
    pool: SourceInfoPool | undefined,
): SourceInfoEntry | undefined {
    if (!pool || node.s === undefined) return undefined;
    return pool[node.s];
}

/**
 * True iff the entry indicates an atomic transform — a `Generated`
 * entry (code 4) whose `By::kind` is in the atomic-producer set.
 *
 * Used by Plan 2B's atomic-aware dispatcher gate to decide whether
 * `setLocalAst` should be a no-op for the subtree.
 */
export function isAtomicSourceInfo(
    node: { s?: number },
    pool: SourceInfoPool | undefined,
    atomicKinds: ReadonlySet<string>,
): boolean {
    const entry = entryFor(node, pool);
    if (!entry) return false;
    if (entry.t === 4) return atomicKinds.has(entry.d.by.kind);
    return false;
}

/**
 * Atomic producer kinds that mark entire `Generated` subtrees as
 * read-only on the iframe side.
 *
 * Sync contract: mirrors `By::is_atomic_kind()` on the Rust side
 * (`crates/quarto-source-map/src/source_info.rs`). The Rust function
 * and this set must agree on which kinds are atomic; otherwise
 * q2-preview's edit-back gate desyncs from the pipeline's expectation.
 */
export const ATOMIC_KINDS: ReadonlySet<string> = new Set<string>([
    'filter',
    'shortcode',
    'title-block',
    'tree-sitter-postprocess',
]);
