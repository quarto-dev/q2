/**
 * Type-keyed CustomNode components for q2-preview.
 *
 * Each per-type export name matches a canonical CustomNode
 * `type_name` (per `crates/quarto-core/src/crossref/mod.rs:60-92`
 * plus the `CustomNode::new("Callout", ...)` site at
 * `crates/quarto-core/src/transforms/callout.rs:233`). The
 * `dispatchers.tsx`'s `CustomBlock` / `CustomInline` look up these
 * names on `previewRegistry` keyed by `node.type_name`.
 *
 * Synthetic-key entries live alongside:
 *  - `Fallback` is registered under `__fallback__` (Plan 2C).
 *  - `PreviewTitleBlock` is registered under `__title_block__`
 *    (Plan 2D); receives `AstProps`, not `NodeArgs`.
 *
 * IncludeExpansion has no entry here; it currently routes through
 * `Fallback` until Plan 8 ships its own component.
 */

export { Callout } from './Callout';
export { Theorem } from './Theorem';
export { Proof } from './Proof';
export { FloatRefTarget } from './FloatRefTarget';
export { Equation } from './Equation';
export { CrossrefResolvedRef } from './CrossrefResolvedRef';
export { Fallback } from './Fallback';
export { PreviewTitleBlock } from './PreviewTitleBlock';
