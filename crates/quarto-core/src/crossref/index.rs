/*
 * crossref/index.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Per-document crossref index.
 */

//! Per-document crossref index types.
//!
//! The [`CrossrefIndex`] is built by `CrossrefIndexTransform` walking the AST
//! and collecting float / block / inline crossref targets. It is then consumed
//! by `CrossrefResolveTransform` and by back-end renderers.
//!
//! ## Multi-file future
//!
//! These types are serializable so per-file indices can be persisted under
//! `.quarto/xref/<file-id>.json` and merged into a project-wide index. This is
//! out of scope for the initial crossref delivery but the data model must not
//! foreclose it — see Phase 4 of the design plan.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::inline::Inlines;
use quarto_source_map::{FileId, SourceInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Per-document crossref state built by the crossref phase of the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossrefIndex {
    /// File this index was built for. Used to namespace ids across files in
    /// multi-file (book/website) merges.
    pub file_id: FileId,

    /// All crossref targets declared in this file, keyed by `identifier`
    /// (e.g. `"fig-myplot"`).
    ///
    /// `LinkedHashMap` preserves insertion order so numbering is stable across
    /// serialize/deserialize round trips.
    pub entries: LinkedHashMap<String, CrossrefEntry>,

    /// Section numbering state: the stack of section counters in effect at the
    /// *end* of the document walk.
    ///
    /// During indexing, this is mutated as headings are encountered; each
    /// crossref entry snapshots the current stack into its [`Order::section`].
    pub sections: Vec<u32>,

    /// Next-order counters, per ref_type. Incremented by the index builder as
    /// each target is encountered.
    pub next_order: HashMap<String, u32>,

    /// Heading records, kept for cross-file heading link fixup in book mode.
    /// Not populated in single-file mode beyond what back-end renderers need.
    pub headings: Vec<HeadingRecord>,

    /// Static manifest of ids promised by `output: asis` blocks via
    /// `crossref.ids` in document metadata. See design plan D6.
    pub promised_ids: Vec<PromisedId>,
}

impl CrossrefIndex {
    /// Create an empty index for the given file.
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            entries: LinkedHashMap::new(),
            sections: Vec::new(),
            next_order: HashMap::new(),
            headings: Vec::new(),
            promised_ids: Vec::new(),
        }
    }

    /// Look up an entry by identifier (e.g. `"fig-myplot"`).
    pub fn get(&self, identifier: &str) -> Option<&CrossrefEntry> {
        self.entries.get(identifier)
    }

    /// Insert an entry. Returns the previous entry if one existed with the
    /// same identifier — callers should emit a duplicate-id diagnostic when
    /// this happens.
    pub fn insert(&mut self, entry: CrossrefEntry) -> Option<CrossrefEntry> {
        self.entries.insert(entry.identifier.clone(), entry)
    }
}

/// One crossref target recorded during indexing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossrefEntry {
    /// Full identifier, e.g. `"fig-myplot"`.
    pub identifier: String,

    /// Prefix only (e.g. `"fig"`) — matches the `ref_type` column of the
    /// [`crate::crossref::RefTypeRegistry`].
    pub ref_type: String,

    /// For subfloats: the identifier of the parent float. `None` for top-level
    /// targets.
    pub parent: Option<String>,

    /// Order + section path at the point this target was indexed.
    pub order: Order,

    /// Caption inlines, for link text when resolving references. `None` when
    /// the target has no caption (permitted for listings, etc.).
    pub caption: Option<Inlines>,

    /// Whether this target appears under an appendix section. The exact
    /// semantics are format- and project-specific; this is a raw flag.
    pub in_appendix: bool,

    /// Source location of the original target in the authored document, kept
    /// so diagnostics (duplicate ids, unresolved refs) can point at it.
    pub source_info: SourceInfo,
}

/// Position of a target in document reading order, plus the section path in
/// effect when it was encountered.
///
/// `section` mirrors the heading stack: `[1, 2]` means "section 1.2". Combined
/// with `order`, this is enough for writers to emit "Figure 1.2.3" style
/// numbering where appropriate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    /// Stack of section counters in effect at this point (e.g. `[1, 2]` for
    /// section 1.2). Empty before the first heading.
    pub section: Vec<u32>,

    /// 1-based order within `ref_type` across the document. If
    /// `crossref.chapters` is enabled, this may be reset per chapter — that
    /// policy lives in the index builder, not in this struct.
    pub order: u32,
}

/// A promised crossref id lifted from `crossref.ids` in document metadata.
///
/// Declaring ids in the manifest is the only way an `output: asis` block can
/// legally produce a crossref target. See design plan D6 / O6.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromisedId {
    /// Identifier promised to appear, e.g. `"tbl-dynamic"`.
    pub identifier: String,

    /// Prefix portion (e.g. `"tbl"`). Cached here so downstream consumers
    /// don't re-parse. Must match a registered ref_type.
    pub ref_type: String,

    /// Where the promise was declared (source info on the YAML entry).
    pub source_info: SourceInfo,

    /// How the promise was introduced.
    pub source: PromisedIdSource,
}

/// How a [`PromisedId`] came to be declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromisedIdSource {
    /// From `crossref.ids` in the document's merged metadata.
    DocumentMetadata,
    /// From a project-level manifest (reserved for future multi-file work).
    ProjectMetadata,
}

/// A heading recorded during indexing. Kept for cross-file heading link fixup
/// in book/website mode; not yet consumed in single-file mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadingRecord {
    /// Heading identifier (`id` attribute), if any.
    pub identifier: Option<String>,
    /// Heading level (1-6).
    pub level: u8,
    /// Section path at this heading, e.g. `[1, 2]`.
    pub section: Vec<u32>,
    /// Source location of the heading in the authored document.
    pub source_info: SourceInfo,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    #[test]
    fn index_new_is_empty() {
        let idx = CrossrefIndex::new(FileId(0));
        assert!(idx.entries.is_empty());
        assert!(idx.sections.is_empty());
        assert!(idx.next_order.is_empty());
        assert!(idx.headings.is_empty());
        assert!(idx.promised_ids.is_empty());
    }

    #[test]
    fn insert_and_get() {
        let mut idx = CrossrefIndex::new(FileId(0));
        let entry = CrossrefEntry {
            identifier: "fig-1".into(),
            ref_type: "fig".into(),
            parent: None,
            order: Order {
                section: vec![1],
                order: 1,
            },
            caption: None,
            in_appendix: false,
            source_info: dummy_source_info(),
        };
        assert!(idx.insert(entry.clone()).is_none());
        assert_eq!(idx.get("fig-1").unwrap().identifier, "fig-1");
    }

    #[test]
    fn duplicate_insert_returns_previous() {
        let mut idx = CrossrefIndex::new(FileId(0));
        let entry = |src_offset| CrossrefEntry {
            identifier: "fig-1".into(),
            ref_type: "fig".into(),
            parent: None,
            order: Order {
                section: vec![],
                order: 1,
            },
            caption: None,
            in_appendix: false,
            source_info: SourceInfo::original(FileId(0), src_offset, src_offset),
        };
        assert!(idx.insert(entry(10)).is_none());
        let prev = idx.insert(entry(20)).expect("second insert replaces");
        assert_eq!(prev.source_info.start_offset(), 10);
        assert_eq!(idx.get("fig-1").unwrap().source_info.start_offset(), 20);
    }

    #[test]
    fn json_round_trip() {
        let mut idx = CrossrefIndex::new(FileId(7));
        idx.insert(CrossrefEntry {
            identifier: "fig-roundtrip".into(),
            ref_type: "fig".into(),
            parent: None,
            order: Order {
                section: vec![1, 2],
                order: 3,
            },
            caption: None,
            in_appendix: false,
            source_info: dummy_source_info(),
        });
        idx.sections = vec![1, 2];
        idx.next_order.insert("fig".into(), 4);
        idx.promised_ids.push(PromisedId {
            identifier: "tbl-dynamic".into(),
            ref_type: "tbl".into(),
            source_info: dummy_source_info(),
            source: PromisedIdSource::DocumentMetadata,
        });

        let json = serde_json::to_string(&idx).unwrap();
        let back: CrossrefIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(back.file_id, idx.file_id);
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.promised_ids.len(), 1);
        assert_eq!(back.next_order.get("fig").copied(), Some(4));
    }
}
