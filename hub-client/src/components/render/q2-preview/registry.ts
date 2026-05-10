import type { FormatRegistry } from '../framework';
import * as Blocks from './blocks';
import * as Inlines from './inlines';
import * as Custom from './custom';
import { Block, Inline, CustomBlock, CustomInline } from './dispatchers';
import { PreviewDocument } from './PreviewDocument';

/**
 * q2-preview format registry. The framework reserves the keys 'Ast',
 * 'Block', and 'Inline'; each format must register all three.
 *
 * 2B populates the registry with every Pandoc base-type leaf
 * (14 blocks + 20 inlines). 2C extends with the type-keyed CustomNode
 * components (`Callout`, `Theorem`, `Proof`, `FloatRefTarget`,
 * `Equation`, `CrossrefResolvedRef`) plus a `__fallback__` entry, and
 * the `CustomBlock` / `CustomInline` dispatchers that look them up
 * by `node.type_name`.
 *
 * Pandoc tag names and CustomNode `type_name`s are namespace-disjoint
 * by project policy (locked at build time by the namespace-disjoint
 * test in `registry.test.ts`). One merged map carries both — a user
 * TSX override of either kind layers on via the same merge site in
 * `entry.tsx`'s PreviewRoot.
 *
 * `mergedPreviewRegistry` (in entry.tsx's PreviewRoot) layers user-TSX
 * exports on top via `{ ...previewRegistry, ...customRegistry }`, so
 * a user override of `Para` / `Image` / `Callout` / etc. wins over
 * the built-in.
 */
export const previewRegistry: FormatRegistry = {
    ...Blocks,
    ...Inlines,
    ...Custom, // Callout, Theorem, Proof, FloatRefTarget, Equation, CrossrefResolvedRef, Fallback
    __fallback__: Custom.Fallback,
    Block,
    Inline,
    CustomBlock,
    CustomInline,
    Ast: PreviewDocument,
};
