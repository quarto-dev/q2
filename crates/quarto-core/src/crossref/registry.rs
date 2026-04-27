/*
 * crossref/registry.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Ref-type registry — the authoritative set of crossref prefixes.
 */

//! Ref-type registry.
//!
//! A [`RefTypeRegistry`] is the authoritative list of crossref ref-type
//! prefixes in effect for a document: built-ins (`fig`, `tbl`, `lst`, ...)
//! plus any extensions from `crossref.custom` in merged metadata, plus any
//! prefixes used by `crossref.ids`-promised ids (see design plan D6/D7).
//!
//! The registry is the sole thing that distinguishes a crossref `Cite` from a
//! bibliographic `Cite` — `@fig-myplot` is a crossref iff `"fig"` is
//! registered; `@smith-2020` is a citation iff `"smith"` is *not* registered.
//!
//! Lifecycle:
//!
//! 1. Seed with built-ins via [`RefTypeRegistry::builtin`].
//! 2. In the pre-engine sugaring stage, extend from metadata via
//!    [`RefTypeRegistry::extend_from_metadata`].
//! 3. Then extend from any [`crate::crossref::PromisedId`] entries via
//!    [`RefTypeRegistry::extend_from_promised`].
//! 4. After that, the registry is **frozen** — transforms downstream consume
//!    it by reference only.

use quarto_source_map::SourceInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::index::PromisedId;

/// Authoritative set of registered crossref ref-types for one document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefTypeRegistry {
    entries: HashMap<String, RefTypeDef>,
}

/// One registered ref-type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefTypeDef {
    /// The id prefix, e.g. `"fig"`. This is what matches before the first `-`
    /// in a crossref identifier like `"fig-myplot"`.
    pub ref_type: String,

    /// Human / display name, e.g. `"Figure"`. Used by writers when emitting
    /// "Figure 1" style text.
    pub kind: String,

    /// How this entry was introduced.
    pub source: RefTypeSource,

    /// Where this entry was declared (for diagnostics). `None` for built-ins.
    pub source_info: Option<SourceInfo>,
}

/// Origin of a [`RefTypeDef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefTypeSource {
    /// Built into Quarto (e.g. `fig`, `tbl`, `thm`).
    BuiltIn,
    /// Declared by the user in `crossref.custom` metadata.
    CustomFromMetadata,
    /// Implied by a `crossref.ids` manifest entry whose prefix isn't
    /// otherwise registered. This is narrow — it lets the resolver classify
    /// `@custom-dynamic` even when the user forgot to declare the category —
    /// but the indexer still refuses to number undeclared-category targets.
    Promised,
}

/// Built-in ref-types registered by default.
///
/// These mirror the categories supported by Q1 out of the box. The display
/// names are the canonical English forms; localization is a separate concern
/// handled by format-specific renderers.
const BUILTINS: &[(&str, &str)] = &[
    ("fig", "Figure"),
    ("tbl", "Table"),
    ("lst", "Listing"),
    ("eq", "Equation"),
    ("sec", "Section"),
    ("thm", "Theorem"),
    ("lem", "Lemma"),
    ("cor", "Corollary"),
    ("prp", "Proposition"),
    ("cnj", "Conjecture"),
    ("def", "Definition"),
    ("exm", "Example"),
    ("exr", "Exercise"),
    ("sol", "Solution"),
    ("rem", "Remark"),
    ("nte", "Note"),
    ("wrn", "Warning"),
    ("tip", "Tip"),
    ("imp", "Important"),
    ("cau", "Caution"),
];

impl RefTypeRegistry {
    /// Seed with built-ins only. Call [`Self::extend_from_metadata`] and
    /// [`Self::extend_from_promised`] next to complete the registry.
    pub fn builtin() -> Self {
        let mut entries = HashMap::with_capacity(BUILTINS.len());
        for (ref_type, kind) in BUILTINS {
            entries.insert(
                (*ref_type).to_string(),
                RefTypeDef {
                    ref_type: (*ref_type).to_string(),
                    kind: (*kind).to_string(),
                    source: RefTypeSource::BuiltIn,
                    source_info: None,
                },
            );
        }
        Self { entries }
    }

    /// True if `ref_type` (a prefix, e.g. `"fig"`) is registered.
    pub fn contains(&self, ref_type: &str) -> bool {
        self.entries.contains_key(ref_type)
    }

    /// Number of registered ref-types (for tracing / introspection).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if the registry is empty. Note that [`Self::builtin`] — the
    /// usual starting point — is never empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up a ref-type definition by prefix.
    pub fn get(&self, ref_type: &str) -> Option<&RefTypeDef> {
        self.entries.get(ref_type)
    }

    /// Classify an identifier (Cite id, element id, ...). Returns `Some` if
    /// the id has the shape `"<prefix>-<rest>"` and `<prefix>` is registered.
    ///
    /// Examples (with built-ins registered):
    /// - `"fig-myplot"` → `Some(Figure)` — crossref.
    /// - `"smith2020"` → `None` — no dash, bibliographic Cite.
    /// - `"mycustom-foo"` → `Some(...)` iff `mycustom` was registered; else
    ///   `None` (treat as citation key).
    /// - `"fig-"` → `Some(Figure)` — prefix ok; suffix emptiness is the
    ///   caller's problem (emit an "empty crossref id" diagnostic).
    pub fn classify_cite_id(&self, id: &str) -> Option<&RefTypeDef> {
        let (prefix, _rest) = id.split_once('-')?;
        self.entries.get(prefix)
    }

    /// Add a user-declared ref-type from `crossref.custom`. Returns an error
    /// via [`RegistryError`] if the entry conflicts with an existing one.
    ///
    /// Built-ins cannot be overridden through this entrypoint. Duplicates
    /// across custom entries are also an error.
    pub fn register_custom(
        &mut self,
        ref_type: impl Into<String>,
        kind: impl Into<String>,
        source_info: Option<SourceInfo>,
    ) -> Result<(), RegistryError> {
        let ref_type = ref_type.into();
        if let Some(existing) = self.entries.get(&ref_type) {
            return Err(RegistryError::Duplicate {
                ref_type,
                existing: existing.source,
            });
        }
        self.entries.insert(
            ref_type.clone(),
            RefTypeDef {
                ref_type,
                kind: kind.into(),
                source: RefTypeSource::CustomFromMetadata,
                source_info,
            },
        );
        Ok(())
    }

    /// Extend the registry from `crossref.custom`-style entries.
    ///
    /// `entries` is a sequence of `(ref_type, kind, source_info)` tuples —
    /// parsing the YAML is the caller's responsibility so this stays decoupled
    /// from any particular metadata representation. Errors are collected so
    /// one bad entry doesn't hide others.
    pub fn extend_from_metadata<I>(&mut self, entries: I) -> Vec<RegistryError>
    where
        I: IntoIterator<Item = (String, String, Option<SourceInfo>)>,
    {
        let mut errors = Vec::new();
        for (ref_type, kind, src) in entries {
            if let Err(e) = self.register_custom(ref_type, kind, src) {
                errors.push(e);
            }
        }
        errors
    }

    /// Register any promised-id prefixes that aren't already known.
    ///
    /// Promised ids whose prefix *is* already registered are a no-op; the
    /// existing category handles them. Promised ids whose prefix is *not*
    /// registered get a `Promised` entry so the resolver can still classify
    /// them — but `kind` is just the prefix itself because we have no display
    /// name for a category the user never declared. The indexer is expected
    /// to flag this as a diagnostic rather than silently inventing numbering.
    pub fn extend_from_promised(&mut self, promised: &[PromisedId]) {
        for p in promised {
            if self.entries.contains_key(&p.ref_type) {
                continue;
            }
            self.entries.insert(
                p.ref_type.clone(),
                RefTypeDef {
                    ref_type: p.ref_type.clone(),
                    kind: p.ref_type.clone(),
                    source: RefTypeSource::Promised,
                    source_info: Some(p.source_info.clone()),
                },
            );
        }
    }
}

impl Default for RefTypeRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

/// Errors produced when building the registry.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RegistryError {
    /// A `crossref.custom` entry tried to register a ref_type that already
    /// exists — either a built-in or an earlier custom entry.
    #[error("duplicate crossref ref_type `{ref_type}` (already registered from {existing:?})")]
    Duplicate {
        ref_type: String,
        existing: RefTypeSource,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::index::PromisedIdSource;
    use quarto_source_map::FileId;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    #[test]
    fn builtin_covers_common_categories() {
        let reg = RefTypeRegistry::builtin();
        for key in ["fig", "tbl", "lst", "eq", "thm", "lem", "exm", "def"] {
            assert!(reg.contains(key), "builtin registry missing `{key}`");
        }
        assert_eq!(reg.get("fig").unwrap().kind, "Figure");
        assert_eq!(reg.get("fig").unwrap().source, RefTypeSource::BuiltIn);
    }

    #[test]
    fn classify_crossref() {
        let reg = RefTypeRegistry::builtin();
        assert_eq!(reg.classify_cite_id("fig-myplot").unwrap().kind, "Figure");
        assert_eq!(reg.classify_cite_id("tbl-x").unwrap().kind, "Table");
    }

    #[test]
    fn classify_citation_no_dash() {
        let reg = RefTypeRegistry::builtin();
        assert!(reg.classify_cite_id("smith2020").is_none());
        assert!(reg.classify_cite_id("mycustomfoo2020").is_none());
    }

    #[test]
    fn classify_unregistered_prefix_is_citation() {
        let reg = RefTypeRegistry::builtin();
        assert!(reg.classify_cite_id("mycustom-foo").is_none());
    }

    #[test]
    fn classify_empty_suffix_still_crossref() {
        // The registry doesn't judge suffixes — emitting an "empty id"
        // diagnostic is the caller's job.
        let reg = RefTypeRegistry::builtin();
        assert!(reg.classify_cite_id("fig-").is_some());
    }

    #[test]
    fn register_custom_succeeds() {
        let mut reg = RefTypeRegistry::builtin();
        reg.register_custom("dia", "Diagram", Some(dummy_source_info()))
            .unwrap();
        let def = reg.classify_cite_id("dia-one").unwrap();
        assert_eq!(def.kind, "Diagram");
        assert_eq!(def.source, RefTypeSource::CustomFromMetadata);
    }

    #[test]
    fn register_custom_rejects_builtin_clash() {
        let mut reg = RefTypeRegistry::builtin();
        let err = reg.register_custom("fig", "MyFigure", None).unwrap_err();
        match err {
            RegistryError::Duplicate { ref_type, existing } => {
                assert_eq!(ref_type, "fig");
                assert_eq!(existing, RefTypeSource::BuiltIn);
            }
        }
    }

    #[test]
    fn register_custom_rejects_duplicate_custom() {
        let mut reg = RefTypeRegistry::builtin();
        reg.register_custom("dia", "Diagram", None).unwrap();
        let err = reg.register_custom("dia", "Other", None).unwrap_err();
        assert!(matches!(err, RegistryError::Duplicate { .. }));
    }

    #[test]
    fn extend_from_metadata_collects_errors() {
        let mut reg = RefTypeRegistry::builtin();
        let errs = reg.extend_from_metadata([
            ("dia".into(), "Diagram".into(), None),
            ("fig".into(), "Shadow".into(), None), // clashes with built-in
            ("dia".into(), "DuplicateCustom".into(), None), // clashes with first custom
            ("plot".into(), "Plot".into(), None),
        ]);
        assert_eq!(errs.len(), 2);
        assert!(reg.contains("dia"));
        assert!(reg.contains("plot"));
        assert_eq!(reg.get("fig").unwrap().kind, "Figure"); // unchanged
    }

    #[test]
    fn extend_from_promised_adds_unknown_prefixes() {
        let mut reg = RefTypeRegistry::builtin();
        let promised = vec![
            PromisedId {
                identifier: "fig-x".into(),
                ref_type: "fig".into(),
                source_info: dummy_source_info(),
                source: PromisedIdSource::DocumentMetadata,
            },
            PromisedId {
                identifier: "custom-y".into(),
                ref_type: "custom".into(),
                source_info: dummy_source_info(),
                source: PromisedIdSource::DocumentMetadata,
            },
        ];
        reg.extend_from_promised(&promised);
        assert_eq!(reg.get("fig").unwrap().source, RefTypeSource::BuiltIn); // not shadowed
        assert_eq!(reg.get("custom").unwrap().source, RefTypeSource::Promised);
        assert_eq!(reg.get("custom").unwrap().kind, "custom"); // placeholder
    }
}
