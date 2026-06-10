/*
 * crossref/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Crossref data structures and registry for Quarto 2.
 */

//! Crossref data structures for Quarto 2.
//!
//! This module provides the front-end representation of crossref state that is
//! built during the crossref phase of the transform pipeline and consumed by
//! both reference resolution and back-end renderers.
//!
//! Design lives in `claude-notes/plans/2026-04-15-crossref-design.md`.
//!
//! ## Trace emission
//!
//! The crossref index is surfaced to the pipeline trace through
//! [`PipelineObserver::on_auxiliary_data`] (see
//! `crates/quarto-core/src/stage/observer.rs`). Convention:
//!
//! - `stage`: `"crossref-index"` (the transform name).
//! - `kind`: `"CrossrefIndex"` (well-known tag — see
//!   [`TRACE_KIND_CROSSREF_INDEX`]).
//! - `data`: `serde_json::to_value(&CrossrefIndex)` — the same JSON shape
//!   that will later be persisted to `.quarto/xref/<file-id>.json` for
//!   multi-file merges. One shape for both pathways means Phase 4 is
//!   additive.
//!
//! `JsonTraceObserver` records this as a `TraceEntry` with `stage: "aux:..."`
//! and the `CrossrefIndex` payload. Phase 1.3's `CrossrefIndexTransform` is
//! the actual caller; this constant is the contract it commits to.

/// Well-known `kind` tag for the crossref-index payload on
/// [`crate::stage::PipelineObserver::on_auxiliary_data`].
pub const TRACE_KIND_CROSSREF_INDEX: &str = "CrossrefIndex";

pub mod codeblock_shorthand;
pub mod index;
pub mod metadata;
pub mod registry;
pub mod target;

#[cfg(test)]
mod roundtrip_tests;

pub use index::{CrossrefEntry, CrossrefIndex, HeadingRecord, Order, PromisedId, PromisedIdSource};
pub use metadata::{CrossrefMetadata, MetadataError};
pub use registry::{RefTypeDef, RefTypeRegistry, RefTypeSource};
pub use target::{
    CrossrefTargetView, crossref_target_view, crossref_target_view_inline, identifier_of,
    ref_type_of,
};

/// The `type_name` used on `CustomNode` for float-ref targets.
///
/// Figures, tables, listings, and user-defined float categories all use this
/// custom node type post-sugaring. The specific category is stored in
/// `plain_data.kind` (display name) and `plain_data.ref_type` (id prefix).
pub const FLOAT_REF_TARGET: &str = "FloatRefTarget";

/// The `type_name` used on `CustomNode` for theorem-like blocks.
///
/// Covers theorems, lemmas, corollaries, propositions, conjectures,
/// definitions, examples, and exercises. The specific kind is stored in
/// `plain_data.kind`; the id prefix in `plain_data.ref_type` (`thm`,
/// `lem`, `cor`, `prp`, `cnj`, `def`, `exm`, `exr`).
pub const THEOREM: &str = "Theorem";

/// The `type_name` used on `CustomNode` for proof blocks.
///
/// Proofs are not numbered in the default Quarto flow; they render with
/// an italicized "Proof." prefix. This custom node intentionally does
/// **not** populate `plain_data.ref_type`, so the indexer skips it and
/// the ref-resolver won't mistake it for a crossref target even if the
/// author attaches an id.
pub const PROOF: &str = "Proof";

/// The `type_name` used on `CustomNode` for labelled display equations.
///
/// Produced by `EquationLabelTransform` from a `Span.quarto-math-with-attribute`
/// wrapping a `DisplayMath` inline. The specific numbering is stored in
/// `plain_data.order` (set by the indexer); the id prefix is always `"eq"`.
pub const EQUATION: &str = "Equation";

/// The `type_name` used on `CustomNode` for resolved crossref references in
/// the front-end AST.
///
/// Produced by `CrossrefResolveTransform` when it rewrites a `Cite` whose id
/// classifies as a crossref (per [`RefTypeRegistry`]). Back-end renderers
/// convert this into a format-specific link or reference.
///
/// Kept in lockstep with
/// [`quarto_pandoc_types::ATOMIC_CUSTOM_NODES`] — the q2-preview incremental
/// writer treats this type_name as atomic. A cross-check test below pins
/// the two literals together.
pub const CROSSREF_RESOLVED_REF: &str = "CrossrefResolvedRef";

#[cfg(test)]
mod atomic_lockstep_tests {
    use super::CROSSREF_RESOLVED_REF;

    /// Pin that the `CROSSREF_RESOLVED_REF` literal here matches the entry
    /// in `quarto_pandoc_types::ATOMIC_CUSTOM_NODES`. If either string
    /// changes, the writer's atomicity gate silently mis-fires; this test
    /// fails noisily.
    #[test]
    fn crossref_resolved_ref_is_in_atomic_registry() {
        assert!(
            quarto_pandoc_types::ATOMIC_CUSTOM_NODES.contains(&CROSSREF_RESOLVED_REF),
            "CROSSREF_RESOLVED_REF (`{}`) must appear in \
             quarto_pandoc_types::ATOMIC_CUSTOM_NODES; the q2-preview \
             writer relies on the lockstep.",
            CROSSREF_RESOLVED_REF
        );
    }
}
