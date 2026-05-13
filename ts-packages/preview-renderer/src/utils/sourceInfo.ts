/**
 * Pure-function accessors for the source-info pool. Used by Plan 2B's
 * atomic-aware dispatcher gate (in `framework/dispatch.tsx`'s `Node`)
 * and by future features that need source-mapped lookups (preimage
 * navigation, source-mapped diagnostics).
 *
 * Sync contract: `ATOMIC_SYNTHETIC_KINDS` mirrors the kinds returned
 * by `By::is_atomic_synthesizer()` on the Rust side (Plan 4 / 6
 * landing). Update both together.
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
 * True iff the entry is a Derived (wire code 5) entry. Plan 6 populates
 * Derived entries on shortcode resolutions.
 */
export function isDerived(
    node: { s?: number },
    pool: SourceInfoPool | undefined,
): boolean {
    const entry = entryFor(node, pool);
    return entry?.t === 5;
}

/**
 * True iff the entry indicates an atomic transform — either Derived
 * (always atomic) or Synthetic (code 4) whose `By::kind` is in the
 * atomic-synthesizer set.
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
    if (entry.t === 5) return true;
    if (entry.t === 4) return atomicKinds.has(entry.d.kind);
    return false;
}

/**
 * Atomic-synthesizer kinds that mark entire Synthetic subtrees as
 * read-only on the iframe side. Empty in 2A — Plan 4 / 6 populate this
 * set as their `By` variants land.
 *
 * Sync contract: mirrors `By::is_atomic_synthesizer()` on the Rust
 * side. The Rust function and this set must agree on which kinds are
 * atomic; otherwise q2-preview's edit-back gate desyncs from the
 * pipeline's expectation.
 */
export const ATOMIC_SYNTHETIC_KINDS: ReadonlySet<string> = new Set<string>();
