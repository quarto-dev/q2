import type { BlockNode } from '../../framework';
import type { PreviewContextValue } from '../PreviewContext';

/**
 * Returns true iff the leading block of a tight list item (`<li>`) or
 * definition-body (`<dd>`) is borrowable: its pool-id should be promoted
 * onto the parent list-item element as the editable affordance.
 *
 * ## Why Plain-only? (Amendment A1)
 *
 * A Para-leading item is "loose" — Pandoc wraps its content in a `<p>` which
 * already carries its own pool-id. Borrowing the `<p>`'s id onto the `<li>`
 * too would create a duplicate pool-id surface (two DOM elements claiming the
 * same byte range), confusing the edit affordance. Plain-leading ("tight")
 * items have NO inner `<p>` wrapper, so the `<li>` is the only DOM element
 * that can surface the block's pool-id.
 *
 * ## Predicate (all four conditions must hold)
 *
 * 1. `block.t === 'Plain'` — tight block only (see Amendment A1 rationale above).
 * 2. `block.s !== undefined` — the block has a pool-id to borrow.
 * 3. `!ctx?.editingDisabled` — the context has not disabled the edit surface.
 * 4. `ctx?.resolveSource?.(block)` returns non-null with `reachabilityClass !== 'Opaque'`
 *    — the source is reachable (not from an included file that cannot be edited).
 *
 * Returns `false` if any condition fails, or if `ctx` is null/undefined.
 *
 * @param block  The leading block of a list item or definition body. Typed as
 *               `BlockNode` but the function casts to `any` to read the
 *               runtime `.s` pool-id field (not present in the static type).
 * @param ctx    The `PreviewContextValue` from `useContext(PreviewContext)`,
 *               which may be `null` (no context) or the live context value.
 */
export function isLeadingBlockBorrowable(
    block: BlockNode,
    ctx: PreviewContextValue | null | undefined,
): boolean {
    if ((block as any).t !== 'Plain') return false;
    const s = (block as any).s as string | number | undefined;
    if (s === undefined) return false;
    if (ctx?.editingDisabled) return false;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(block) : null;
    return resolved != null && resolved.reachabilityClass !== 'Opaque';
}
