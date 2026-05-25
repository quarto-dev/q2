//! Registry of `CustomNode` type names that q2-preview's incremental writer
//! treats as **atomic**.
//!
//! An atomic CustomNode is a single replaceable unit. Users can swap or
//! delete one wholesale via a React-side component menu, but they cannot
//! type *inside* it — there is no editable text region the writer can map
//! back to source bytes. The writer treats edits *into* an atomic
//! CustomNode as a soft-drop (Q-3-43); UseAfter on an atomic CustomNode
//! is let-user-win (the qmd writer's CustomNode arm serializes the fresh
//! `plain_data`).
//!
//! See Plan 7 §"`is_atomic_custom_node` registry" for the design and the
//! `is_editable_inside` consumer in `pampa::writers::incremental`.
//!
//! Lives in `quarto-pandoc-types` (not `quarto-core` as Plan 7 originally
//! suggested) because `pampa` consumes it and `pampa` sits below
//! `quarto-core` in the dependency graph.
//!
//! The TypeScript hand-mirror lives at
//! `ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts` and must
//! be kept in lockstep with this list.

/// `CustomNode` type names that q2-preview treats as atomic.
///
/// Today: just `"CrossrefResolvedRef"` (kept in lockstep with
/// `quarto_core::crossref::CROSSREF_RESOLVED_REF` — see the cross-check
/// test in `quarto-core::crossref`). Plan 8 will add `"IncludeExpansion"`.
///
/// Extension-contributed atomic types are out of scope for this const;
/// a future plan adds a runtime registry sourced from `_extension.yml`.
pub const ATOMIC_CUSTOM_NODES: &[&str] = &["CrossrefResolvedRef"];

/// Return `true` iff `type_name` names a CustomNode the incremental
/// writer must treat as atomic.
///
/// See [`ATOMIC_CUSTOM_NODES`] for the list and the module doc-comment
/// for what atomicity means in this context.
pub fn is_atomic_custom_node(type_name: &str) -> bool {
    ATOMIC_CUSTOM_NODES.contains(&type_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossref_resolved_ref_is_atomic() {
        assert!(is_atomic_custom_node("CrossrefResolvedRef"));
    }

    #[test]
    fn unknown_type_name_is_not_atomic() {
        assert!(!is_atomic_custom_node("FloatRefTarget"));
        assert!(!is_atomic_custom_node("Theorem"));
        assert!(!is_atomic_custom_node("Callout"));
        assert!(!is_atomic_custom_node(""));
    }

    #[test]
    fn registry_contains_crossref_resolved_ref() {
        assert!(ATOMIC_CUSTOM_NODES.contains(&"CrossrefResolvedRef"));
    }
}
