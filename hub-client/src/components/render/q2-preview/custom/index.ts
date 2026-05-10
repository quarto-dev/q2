/**
 * Type-keyed CustomNode components for q2-preview.
 *
 * Each export name matches a canonical CustomNode `type_name` (per
 * `crates/quarto-core/src/crossref/mod.rs:60-92` plus the
 * `CustomNode::new("Callout", ...)` site at
 * `crates/quarto-core/src/transforms/callout.rs:233`). The
 * `dispatchers.tsx`'s `CustomBlock` / `CustomInline` look up these
 * names on `previewRegistry` keyed by `node.type_name`.
 *
 * `Fallback` is registered separately under the `__fallback__` key.
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
