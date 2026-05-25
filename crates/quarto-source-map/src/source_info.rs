//! Source information with transformation tracking

use crate::types::{FileId, Range};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::sync::Arc;

/// Source information tracking a location and its transformation history
///
/// This enum stores only byte offsets. Row and column information is computed
/// on-demand via `map_offset()` using the FileInformation line break index.
///
/// Design notes:
/// - Original: Points directly to a file with byte offsets
/// - Substring: Points to a range within a parent SourceInfo (offsets are relative to parent)
/// - Concat: Combines multiple SourceInfo pieces (preserves provenance when coalescing text)
/// - Generated: Produced by a pipeline transform. `by` records the producer; `from`
///   records source-side anchors (empty for pure synthesis, `Invocation` for
///   shortcode-style resolutions).
///
/// The Transformed variant was removed because it's not used in production code.
/// Text transformations (smart quotes, em-dashes) use Original SourceInfo pointing
/// to the pre-transformation text, accepting that the byte offsets are approximate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceInfo {
    /// Direct position in an original file
    ///
    /// Stores only byte offsets. Use `map_offset()` to get row/column information.
    Original {
        file_id: FileId,
        start_offset: usize,
        end_offset: usize,
    },
    /// Substring extraction from a parent source
    ///
    /// Offsets are relative to the parent's text.
    /// The chain of Substrings always resolves to an Original.
    Substring {
        parent: Arc<SourceInfo>,
        start_offset: usize,
        end_offset: usize,
    },
    /// Concatenation of multiple sources
    ///
    /// Used when coalescing adjacent text nodes while preserving
    /// the fact that they came from different source locations.
    Concat { pieces: Vec<SourcePiece> },
    /// Node produced by a pipeline transform
    ///
    /// `by` records the producer ("which transform made me"); `from` is a
    /// list of typed, role-labeled source-info pointers ("which source
    /// bytes contributed to me"). Empty `from` means pure synthesis
    /// (sectionize wrappers, filter constructions, title-block h1).
    /// An `Invocation` anchor present means there is a source-side
    /// preimage (every shortcode resolution).
    Generated {
        by: By,
        #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
        from: SmallVec<[Anchor; 2]>,
    },
}

/// Producer identity for a [`SourceInfo::Generated`] node.
///
/// `kind` is a short, kebab-case identifier describing which transform
/// produced the node ("filter", "shortcode", "sectionize", ...). Third
/// parties should namespace as `ext/<extension>/<kind>`.
///
/// `data` is per-kind configuration that is **not** a source-info pointer.
/// Source-side anchors live in the parent `Generated.from` list, not here.
/// `Null` for kinds that don't carry per-instance data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct By {
    /// Short kind tag, kebab-case. Examples: "filter", "shortcode",
    /// "sectionize", "user-edit", "title-block".
    /// Third-party kinds should namespace: "ext/my-extension/foo".
    pub kind: String,

    /// Per-kind configuration that is NOT a source-info pointer.
    /// Anchors live in `Generated.from`, not here.
    /// `Null` for kinds that don't carry per-instance data.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
}

/// Role describing what kind of source-side contribution an anchor records.
///
/// The known roles are load-bearing — `Invocation` is what the writer's
/// preimage walk and attribution consult; `ValueSource` is diagnostic-only.
/// `Other(String)` is an open escape hatch for extension-defined roles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnchorRole {
    /// The user-written construct that triggered this node's creation
    /// (e.g. the `{{< meta foo >}}` token in the active document).
    /// Load-bearing: the writer's `preimage_in` and attribution's
    /// `resolve_byte_range` consult the first anchor with this role.
    /// At most one per node by convention.
    Invocation,

    /// Where the VALUE this node carries was defined, when distinct
    /// from the invocation site (e.g. `footer:` in `_metadata.yml` for
    /// a `{{< meta footer >}}` resolution). Diagnostic-only — does not
    /// affect the writer or attribution decisions in v1.
    ValueSource,

    /// Extension-defined or future role we haven't enumerated.
    /// String is kebab-case, namespaced (`ext/<name>/<role>`).
    ///
    /// **`preimage_in` does not walk this role.** Future anchor roles
    /// default to non-walked unless explicitly added to
    /// [`SourceInfo::preimage_in`]'s `Generated` arm. Extensions adding
    /// `Other("…")` should treat this as a feature: attribution data
    /// attached via `Other` is not accidentally consulted by the writer's
    /// byte-copying path. If a role *does* contribute to body-text
    /// preimage in `target`, it must be explicitly enumerated in
    /// `preimage_in`.
    Other(String),
}

/// A single typed, role-labeled source-info pointer attached to a
/// [`SourceInfo::Generated`] node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub role: AnchorRole,
    pub source_info: Arc<SourceInfo>,
}

/// A piece of a concatenated source
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourcePiece {
    /// Source information for this piece
    pub source_info: SourceInfo,
    /// Where this piece starts in the concatenated string
    pub offset_in_concat: usize,
    /// Length of this piece
    pub length: usize,
}

impl Default for SourceInfo {
    fn default() -> Self {
        SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 0,
        }
    }
}

impl SourceInfo {
    /// Create source info for a position in an original file (from offsets)
    pub fn original(file_id: FileId, start_offset: usize, end_offset: usize) -> Self {
        SourceInfo::Original {
            file_id,
            start_offset,
            end_offset,
        }
    }

    /// Create source info for a position in an original file (from Range)
    ///
    /// This is a compatibility helper for code that still uses Range.
    /// The row and column information in the Range is ignored; only offsets are stored.
    pub fn from_range(file_id: FileId, range: Range) -> Self {
        SourceInfo::Original {
            file_id,
            start_offset: range.start.offset,
            end_offset: range.end.offset,
        }
    }

    /// Create source info for a substring extraction
    pub fn substring(parent: SourceInfo, start: usize, end: usize) -> Self {
        SourceInfo::Substring {
            parent: Arc::new(parent),
            start_offset: start,
            end_offset: end,
        }
    }

    /// Create source info for concatenated sources
    pub fn concat(pieces: Vec<(SourceInfo, usize)>) -> Self {
        let source_pieces: Vec<SourcePiece> = pieces
            .into_iter()
            .map(|(source_info, length)| SourcePiece {
                source_info,
                offset_in_concat: 0, // Will be calculated based on cumulative lengths
                length,
            })
            .collect();

        // Calculate cumulative offsets
        let mut cumulative_offset = 0;
        let pieces_with_offsets: Vec<SourcePiece> = source_pieces
            .into_iter()
            .map(|mut piece| {
                piece.offset_in_concat = cumulative_offset;
                cumulative_offset += piece.length;
                piece
            })
            .collect();

        SourceInfo::Concat {
            pieces: pieces_with_offsets,
        }
    }

    /// Create a [`SourceInfo::Generated`] with an empty anchor list.
    ///
    /// Use [`SourceInfo::append_anchor`] to add anchors after construction.
    /// For Generated nodes that need to carry anchors at construction
    /// time, build the variant directly: `SourceInfo::Generated { by, from }`.
    pub fn generated(by: By) -> Self {
        SourceInfo::Generated {
            by,
            from: SmallVec::new(),
        }
    }

    /// If this is a [`SourceInfo::Generated`], return the first anchor whose
    /// role is [`AnchorRole::Invocation`].
    ///
    /// Returns `None` otherwise (including for non-`Generated` variants).
    /// By convention there is at most one `Invocation` anchor per node.
    pub fn invocation_anchor(&self) -> Option<&Arc<SourceInfo>> {
        match self {
            SourceInfo::Generated { from, .. } => from
                .iter()
                .find(|a| matches!(a.role, AnchorRole::Invocation))
                .map(|a| &a.source_info),
            _ => None,
        }
    }

    /// If this is a [`SourceInfo::Generated`], return the first anchor whose
    /// role is [`AnchorRole::ValueSource`].
    ///
    /// Returns `None` otherwise. By convention there is at most one
    /// `ValueSource` anchor per node.
    pub fn value_source_anchor(&self) -> Option<&Arc<SourceInfo>> {
        match self {
            SourceInfo::Generated { from, .. } => from
                .iter()
                .find(|a| matches!(a.role, AnchorRole::ValueSource))
                .map(|a| &a.source_info),
            _ => None,
        }
    }

    /// Iterate over every anchor in this [`SourceInfo::Generated`] whose role
    /// equals `role`.
    ///
    /// Returns an empty iterator for non-`Generated` variants. Iteration order
    /// is the append order.
    pub fn anchors_with_role<'a>(
        &'a self,
        role: &'a AnchorRole,
    ) -> Box<dyn Iterator<Item = &'a Arc<SourceInfo>> + 'a> {
        match self {
            SourceInfo::Generated { from, .. } => Box::new(
                from.iter()
                    .filter(move |a| &a.role == role)
                    .map(|a| &a.source_info),
            ),
            _ => Box::new(std::iter::empty()),
        }
    }

    /// Append `(role, source_info)` to this [`SourceInfo::Generated`]'s
    /// anchor list.
    ///
    /// Panics if `self` is not [`SourceInfo::Generated`]. By convention there
    /// is at most one anchor per known role; appending a second anchor with
    /// the same role does not replace the first — accessors that find by
    /// role return the earliest match.
    pub fn append_anchor(&mut self, role: AnchorRole, source_info: Arc<SourceInfo>) {
        match self {
            SourceInfo::Generated { from, .. } => {
                from.push(Anchor { role, source_info });
            }
            _ => panic!("append_anchor called on non-Generated SourceInfo"),
        }
    }

    /// Combine two SourceInfo objects representing adjacent text
    ///
    /// This creates a Concat mapping that preserves both sources.
    /// The resulting SourceInfo spans from the start of self to the end of other.
    pub fn combine(&self, other: &SourceInfo) -> Self {
        let self_length = self.length();
        let other_length = other.length();

        SourceInfo::concat(vec![
            (self.clone(), self_length),
            (other.clone(), other_length),
        ])
    }

    /// Get the length (in bytes) represented by this SourceInfo
    pub fn length(&self) -> usize {
        match self {
            SourceInfo::Original {
                start_offset,
                end_offset,
                ..
            } => end_offset - start_offset,
            SourceInfo::Substring {
                start_offset,
                end_offset,
                ..
            } => end_offset - start_offset,
            SourceInfo::Concat { pieces } => pieces.iter().map(|p| p.length).sum(),
            SourceInfo::Generated { .. } => 0,
        }
    }

    /// Get the start offset for this SourceInfo
    ///
    /// For Original and Substring, returns the start_offset field.
    /// For Concat, returns 0 (the concat represents a new text starting at 0).
    /// For Generated, returns 0.
    pub fn start_offset(&self) -> usize {
        match self {
            SourceInfo::Original { start_offset, .. } => *start_offset,
            SourceInfo::Substring { start_offset, .. } => *start_offset,
            SourceInfo::Concat { .. } => 0,
            SourceInfo::Generated { .. } => 0,
        }
    }

    /// Get the end offset for this SourceInfo
    ///
    /// For Original and Substring, returns the end_offset field.
    /// For Concat, returns the total length.
    /// For Generated, returns 0.
    pub fn end_offset(&self) -> usize {
        match self {
            SourceInfo::Original { end_offset, .. } => *end_offset,
            SourceInfo::Substring { end_offset, .. } => *end_offset,
            SourceInfo::Concat { .. } => self.length(),
            SourceInfo::Generated { .. } => 0,
        }
    }

    /// Chain-resolve to `(file_id, start_offset, end_offset)` in the
    /// root source file.
    ///
    /// Returns `None` for `Concat` — Concat doesn't map cleanly to a
    /// single contiguous byte range. For `Generated`, delegates to the
    /// first `Invocation` anchor and recurses (`None` when no
    /// `Invocation` anchor is present). The attribution v1 sidecar
    /// relies on this contract; project-scoped (v2) features that need
    /// the full chain resolver should use `map_offset` against a
    /// `SourceContext` instead.
    pub fn resolve_byte_range(&self) -> Option<(usize, usize, usize)> {
        match self {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => Some((file_id.0, *start_offset, *end_offset)),
            SourceInfo::Substring {
                parent,
                start_offset,
                end_offset,
            } => {
                let (fid, parent_start, _) = parent.resolve_byte_range()?;
                Some((fid, parent_start + start_offset, parent_start + end_offset))
            }
            SourceInfo::Concat { .. } => None,
            SourceInfo::Generated { .. } => self
                .invocation_anchor()
                .and_then(|si| si.resolve_byte_range()),
        }
    }

    /// Byte range in `target` that this `SourceInfo`'s preimage covers, if any.
    ///
    /// This is the writer's "can I Verbatim-copy bytes from `target` for the
    /// node carrying this source_info?" check.
    ///
    /// Semantics by variant:
    /// - `Original` → `Some(start..end)` iff the file matches `target`, else `None`.
    /// - `Substring` → recurse the parent; offsets compose additively.
    /// - `Concat` → every piece must resolve into `target` AND the resolved
    ///   ranges must be byte-contiguous (no gaps, no overlaps). A gappy Concat
    ///   returns `None` — the writer can't Verbatim-copy a non-contiguous span.
    /// - `Generated` → walk the `Invocation` anchor only via
    ///   [`invocation_anchor`](Self::invocation_anchor). **No other anchor
    ///   role is consulted** — not `ValueSource` (Plan 9), not future
    ///   `Dispatch` (Plan 10), not `AnchorRole::Other`. See the
    ///   role-asymmetry section below.
    ///
    /// # Role asymmetry
    ///
    /// `preimage_in` only walks `AnchorRole::Invocation`. This is load-bearing:
    /// copying bytes from a `ValueSource` source range would emit raw YAML
    /// metadata (or whatever the value lived in) into the body — a hard
    /// correctness bug. The same applies to `Dispatch` (which points at Lua
    /// source) and to any extension-defined `Other` role.
    ///
    /// **Future anchor roles default to non-walked.** Extensions introducing
    /// `AnchorRole::Other("…")` should treat this as a feature: their
    /// attribution metadata is not accidentally consulted by the writer's
    /// byte-copying path. If a role *does* contribute to body-text preimage,
    /// it must be explicitly added to this function's `Generated` arm.
    pub fn preimage_in(&self, target: FileId) -> Option<std::ops::Range<usize>> {
        match self {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } if *file_id == target => Some(*start_offset..*end_offset),
            SourceInfo::Original { .. } => None,
            SourceInfo::Substring {
                parent,
                start_offset,
                end_offset,
            } => {
                let parent_range = parent.preimage_in(target)?;
                Some(parent_range.start + start_offset..parent_range.start + end_offset)
            }
            SourceInfo::Concat { pieces } => {
                let ranges: Vec<std::ops::Range<usize>> = pieces
                    .iter()
                    .map(|p| p.source_info.preimage_in(target))
                    .collect::<Option<Vec<_>>>()?;
                if ranges.is_empty() {
                    return None;
                }
                if ranges.windows(2).all(|w| w[0].end == w[1].start) {
                    let first = ranges.first().unwrap().start;
                    let last = ranges.last().unwrap().end;
                    Some(first..last)
                } else {
                    None
                }
            }
            SourceInfo::Generated { .. } => self
                .invocation_anchor()
                .and_then(|si| si.preimage_in(target)),
        }
    }

    /// Remap every `FileId` referenced by this `SourceInfo` (including those
    /// inside `Substring` parents and `Concat` pieces) using the provided
    /// mapping function.
    ///
    /// Used when merging ASTs that were parsed against different files into a
    /// single `ASTContext` with a shared filename table — callers shift each
    /// AST's `FileId`s to their slot in the merged table before combining.
    pub fn remap_file_ids<F>(&mut self, map: &F)
    where
        F: Fn(FileId) -> FileId,
    {
        match self {
            SourceInfo::Original { file_id, .. } => {
                *file_id = map(*file_id);
            }
            SourceInfo::Substring { parent, .. } => {
                // Arc::make_mut clones if there are other references.
                let parent = Arc::make_mut(parent);
                parent.remap_file_ids(map);
            }
            SourceInfo::Concat { pieces } => {
                for piece in pieces {
                    piece.source_info.remap_file_ids(map);
                }
            }
            SourceInfo::Generated { from, .. } => {
                for anchor in from {
                    // Arc::make_mut clones if there are other references.
                    let inner = Arc::make_mut(&mut anchor.source_info);
                    inner.remap_file_ids(map);
                }
            }
        }
    }

    /// First `FileId` reachable from this `SourceInfo`'s root.
    ///
    /// - `Original` → `Some(file_id)`.
    /// - `Substring` → recurse parent.
    /// - `Concat` → `pieces.iter().find_map(|p| p.source_info.root_file_id())`
    ///   (`find_map` semantics — skips Generated holes and empty pieces).
    /// - `Generated` → `invocation_anchor().and_then(|si| si.root_file_id())`;
    ///   `None` when no `Invocation` anchor is present.
    pub fn root_file_id(&self) -> Option<FileId> {
        match self {
            SourceInfo::Original { file_id, .. } => Some(*file_id),
            SourceInfo::Substring { parent, .. } => parent.root_file_id(),
            SourceInfo::Concat { pieces } => {
                pieces.iter().find_map(|p| p.source_info.root_file_id())
            }
            SourceInfo::Generated { .. } => {
                self.invocation_anchor().and_then(|si| si.root_file_id())
            }
        }
    }

    /// Insert every `FileId` reachable from this `SourceInfo` into `out`.
    ///
    /// Walks every `Original`, every `Substring` parent, every `Concat`
    /// piece, and every `Generated` anchor (all roles — `Invocation`,
    /// `ValueSource`, `Other`).
    pub fn collect_file_ids(&self, out: &mut std::collections::HashSet<FileId>) {
        match self {
            SourceInfo::Original { file_id, .. } => {
                out.insert(*file_id);
            }
            SourceInfo::Substring { parent, .. } => parent.collect_file_ids(out),
            SourceInfo::Concat { pieces } => {
                for piece in pieces {
                    piece.source_info.collect_file_ids(out);
                }
            }
            SourceInfo::Generated { from, .. } => {
                for anchor in from {
                    anchor.source_info.collect_file_ids(out);
                }
            }
        }
    }
}

impl By {
    /// Producer kind for a node constructed by a Lua filter
    /// (e.g. `pandoc.Str("decoration")` inside a filter callback).
    ///
    /// `filter_path` is the path the Lua engine reported via
    /// `debug.getinfo(...).source` (with the leading "@" stripped);
    /// `line` is the line number inside that file where the constructor
    /// ran. Until Lua-file-registration lands (bd-36fr9), `(filter_path,
    /// line)` lives in `by.data`; afterwards it migrates to a `Dispatch`
    /// anchor and `by.data` shrinks to `{}`.
    pub fn filter(filter_path: impl Into<String>, line: usize) -> Self {
        Self {
            kind: "filter".to_string(),
            data: serde_json::json!({
                "filter_path": filter_path.into(),
                "line": line,
            }),
        }
    }

    /// Producer kind for the `SectionizeTransform`'s synthesized section
    /// Divs. Children remain editable; the wrapper itself is structural.
    pub fn sectionize() -> Self {
        Self {
            kind: "sectionize".to_string(),
            data: serde_json::Value::Null,
        }
    }

    /// Producer kind for React-constructed (user-typed) content reaching
    /// the AST through the q2-preview client.
    pub fn user_edit() -> Self {
        Self {
            kind: "user-edit".to_string(),
            data: serde_json::Value::Null,
        }
    }

    /// Producer kind for shortcode resolutions.
    ///
    /// **Invariant.** Every `Generated { by: shortcode(...), .. }` must
    /// carry at least one `Invocation` anchor in `from` pointing at the
    /// source token's byte range. Use only inside a `Generated` whose
    /// anchor list is populated; constructing the bare shape with empty
    /// `from` is rejected by Plan 6's audit-completion test and trips
    /// Plan 7's writer `debug_assert!`.
    pub fn shortcode(name: impl Into<String>) -> Self {
        Self {
            kind: "shortcode".to_string(),
            data: serde_json::json!({ "name": name.into() }),
        }
    }

    /// Producer kind for `IncludeStage`'s expansion wrapper. Note that
    /// most include-related synthesized content keeps its `Original`
    /// `source_info` (inherited from the include-line Paragraph) — this
    /// kind is only used where a `Generated` is explicitly required.
    pub fn include() -> Self {
        Self {
            kind: "include".to_string(),
            data: serde_json::Value::Null,
        }
    }

    /// Producer kind for the title-block stage's synthesized title `h1`.
    pub fn title_block() -> Self {
        Self {
            kind: "title-block".to_string(),
            data: serde_json::Value::Null,
        }
    }

    /// Producer kind for the footnotes stage's container Div.
    pub fn footnotes() -> Self {
        Self {
            kind: "footnotes".to_string(),
            data: serde_json::Value::Null,
        }
    }

    /// Producer kind for the appendix-structure stage's wrapper Div.
    pub fn appendix() -> Self {
        Self {
            kind: "appendix".to_string(),
            data: serde_json::Value::Null,
        }
    }

    /// Producer kind for parser-side synthetic Spaces inserted by the
    /// tree-sitter post-processing pass.
    pub fn tree_sitter_postprocess() -> Self {
        Self {
            kind: "tree-sitter-postprocess".to_string(),
            data: serde_json::Value::Null,
        }
    }

    /// Escape-hatch constructor for any `kind` string — including built-in
    /// names and extension-defined kinds (`ext/<extension>/<kind>`).
    ///
    /// Forgery (an extension calling `By::raw("shortcode", …)` without the
    /// required `Invocation` anchor) is caught downstream by Plan 6's
    /// audit-completion test and Plan 7's `debug_assert!`. The convention
    /// for third-party kinds is `ext/<extension>/<kind>`.
    pub fn raw(kind: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            data,
        }
    }

    /// True if a `Generated { by: <self>, .. }` node should be treated
    /// as atomic by the incremental writer.
    ///
    /// Atomic nodes are produced by the pipeline and represent content
    /// the user shouldn't edit through React (filter constructions,
    /// shortcode resolutions, synthesized title h1, tree-sitter-inserted
    /// spaces). Atomicity is determined by `kind` alone — orthogonal to
    /// anchor-presence.
    ///
    /// Extensions that contribute new `by.kind` values are not atomic by
    /// default in v1.
    pub fn is_atomic_kind(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "filter" | "shortcode" | "title-block" | "tree-sitter-postprocess"
        )
    }

    /// True if this `By`'s `kind` equals `kind`.
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind == kind
    }

    /// If `self.kind == "filter"`, return `(filter_path, line)`.
    ///
    /// Returns `None` for any other kind, or when the data payload is
    /// malformed (missing or non-string `filter_path`, missing or
    /// non-integer `line`).
    pub fn as_filter(&self) -> Option<(&str, usize)> {
        if self.kind != "filter" {
            return None;
        }
        let path = self.data.get("filter_path")?.as_str()?;
        let line = self.data.get("line")?.as_u64()? as usize;
        Some((path, line))
    }
}

impl Anchor {
    /// Construct an [`AnchorRole::Invocation`] anchor.
    pub fn invocation(source_info: Arc<SourceInfo>) -> Self {
        Self {
            role: AnchorRole::Invocation,
            source_info,
        }
    }

    /// Construct an [`AnchorRole::ValueSource`] anchor.
    pub fn value_source(source_info: Arc<SourceInfo>) -> Self {
        Self {
            role: AnchorRole::ValueSource,
            source_info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileId, Location, Range};

    #[test]
    fn test_original_source_info() {
        let file_id = FileId(0);
        let range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 10,
                row: 0,
                column: 10,
            },
        };

        let info = SourceInfo::from_range(file_id, range.clone());

        assert_eq!(info.start_offset(), 0);
        assert_eq!(info.end_offset(), 10);
        assert_eq!(info.length(), 10);
        match info {
            SourceInfo::Original {
                file_id: mapped_id, ..
            } => {
                assert_eq!(mapped_id, file_id);
            }
            _ => panic!("Expected Original mapping"),
        }
    }

    #[test]
    fn test_remap_file_ids_original() {
        let mut info = SourceInfo::original(FileId(0), 0, 10);
        info.remap_file_ids(&|id| FileId(id.0 + 1));
        match info {
            SourceInfo::Original { file_id, .. } => assert_eq!(file_id, FileId(1)),
            _ => panic!("Expected Original"),
        }
    }

    #[test]
    fn test_remap_file_ids_substring() {
        let parent = SourceInfo::original(FileId(0), 0, 100);
        let mut info = SourceInfo::substring(parent, 5, 20);
        info.remap_file_ids(&|id| FileId(id.0 + 7));
        match info {
            SourceInfo::Substring { parent, .. } => match &*parent {
                SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, FileId(7)),
                _ => panic!("Expected Original parent"),
            },
            _ => panic!("Expected Substring"),
        }
    }

    #[test]
    fn test_remap_file_ids_concat() {
        let a = SourceInfo::original(FileId(0), 0, 5);
        let b = SourceInfo::original(FileId(3), 5, 10);
        let mut info = SourceInfo::concat(vec![(a, 5), (b, 5)]);
        info.remap_file_ids(&|id| FileId(id.0 + 10));
        match info {
            SourceInfo::Concat { pieces } => {
                match &pieces[0].source_info {
                    SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, FileId(10)),
                    _ => panic!("Expected Original"),
                }
                match &pieces[1].source_info {
                    SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, FileId(13)),
                    _ => panic!("Expected Original"),
                }
            }
            _ => panic!("Expected Concat"),
        }
    }

    #[test]
    fn test_remap_file_ids_generated_empty_from_is_noop() {
        let mut info = SourceInfo::generated(By::filter("foo.lua", 42));
        info.remap_file_ids(&|_| FileId(99));
        match info {
            SourceInfo::Generated { by, from } => {
                assert!(from.is_empty());
                let (path, line) = by.as_filter().unwrap();
                assert_eq!(path, "foo.lua");
                assert_eq!(line, 42);
            }
            _ => panic!("Expected Generated"),
        }
    }

    // -------------------------------------------------------------------------
    // Plan 4 — By / Anchor / Generated coverage
    // -------------------------------------------------------------------------

    #[test]
    fn test_by_filter_builder() {
        let by = By::filter("a.lua", 7);
        assert_eq!(by.kind, "filter");
        assert_eq!(by.as_filter(), Some(("a.lua", 7)));
    }

    #[test]
    fn test_by_sectionize_builder() {
        let by = By::sectionize();
        assert_eq!(by.kind, "sectionize");
        assert!(by.data.is_null());
    }

    #[test]
    fn test_by_user_edit_builder() {
        assert_eq!(By::user_edit().kind, "user-edit");
    }

    #[test]
    fn test_by_shortcode_builder_records_name() {
        let by = By::shortcode("meta");
        assert_eq!(by.kind, "shortcode");
        assert_eq!(by.data.get("name").and_then(|v| v.as_str()), Some("meta"));
    }

    #[test]
    fn test_by_include_title_footnotes_appendix_tree_sitter_builders() {
        assert_eq!(By::include().kind, "include");
        assert_eq!(By::title_block().kind, "title-block");
        assert_eq!(By::footnotes().kind, "footnotes");
        assert_eq!(By::appendix().kind, "appendix");
        assert_eq!(
            By::tree_sitter_postprocess().kind,
            "tree-sitter-postprocess"
        );
    }

    #[test]
    fn test_by_raw_builder_accepts_any_kind() {
        let by = By::raw("ext/my-plugin/foo", serde_json::json!({"k": 1}));
        assert_eq!(by.kind, "ext/my-plugin/foo");
        assert_eq!(by.data.get("k").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    fn test_by_is_atomic_kind() {
        assert!(By::filter("x.lua", 1).is_atomic_kind());
        assert!(By::shortcode("meta").is_atomic_kind());
        assert!(By::title_block().is_atomic_kind());
        assert!(By::tree_sitter_postprocess().is_atomic_kind());

        assert!(!By::sectionize().is_atomic_kind());
        assert!(!By::user_edit().is_atomic_kind());
        assert!(!By::include().is_atomic_kind());
        assert!(!By::footnotes().is_atomic_kind());
        assert!(!By::appendix().is_atomic_kind());
        assert!(!By::raw("ext/anywhere/foo", serde_json::Value::Null).is_atomic_kind());
    }

    #[test]
    fn test_by_is_kind() {
        let by = By::shortcode("meta");
        assert!(by.is_kind("shortcode"));
        assert!(!by.is_kind("filter"));
    }

    #[test]
    fn test_by_as_filter_rejects_non_filter() {
        assert!(By::sectionize().as_filter().is_none());
        // Malformed filter (missing line) → None.
        let by = By {
            kind: "filter".to_string(),
            data: serde_json::json!({ "filter_path": "x.lua" }),
        };
        assert!(by.as_filter().is_none());
    }

    #[test]
    fn test_anchor_invocation_value_source_constructors() {
        let original = Arc::new(SourceInfo::original(FileId(1), 0, 5));
        let inv = Anchor::invocation(Arc::clone(&original));
        let vs = Anchor::value_source(Arc::clone(&original));
        assert!(matches!(inv.role, AnchorRole::Invocation));
        assert!(matches!(vs.role, AnchorRole::ValueSource));
    }

    #[test]
    fn test_by_json_round_trip() {
        let by = By::shortcode("meta");
        let json = serde_json::to_string(&by).unwrap();
        let back: By = serde_json::from_str(&json).unwrap();
        assert_eq!(by, back);
    }

    #[test]
    fn test_anchor_json_round_trip() {
        let anchor = Anchor::invocation(Arc::new(SourceInfo::original(FileId(2), 10, 20)));
        let json = serde_json::to_string(&anchor).unwrap();
        let back: Anchor = serde_json::from_str(&json).unwrap();
        assert_eq!(anchor, back);
    }

    #[test]
    fn test_generated_json_round_trip_empty_from() {
        let info = SourceInfo::generated(By::sectionize());
        let json = serde_json::to_string(&info).unwrap();
        let back: SourceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn test_generated_json_round_trip_with_invocation_anchor() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(5), 100, 110)),
        );
        let json = serde_json::to_string(&info).unwrap();
        let back: SourceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn test_generated_json_round_trip_multi_anchor() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(5), 100, 110)),
        );
        info.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(7), 200, 220)),
        );
        let json = serde_json::to_string(&info).unwrap();
        let back: SourceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn test_generated_length_start_end_are_zero() {
        let info = SourceInfo::generated(By::sectionize());
        assert_eq!(info.length(), 0);
        assert_eq!(info.start_offset(), 0);
        assert_eq!(info.end_offset(), 0);
    }

    #[test]
    fn test_generated_resolve_byte_range_recurses_through_substring() {
        let parent = SourceInfo::original(FileId(42), 100, 200);
        let sub = SourceInfo::substring(parent, 10, 20);
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(AnchorRole::Invocation, Arc::new(sub));
        assert_eq!(info.resolve_byte_range(), Some((42, 110, 120)));
    }

    #[test]
    fn test_generated_resolve_byte_range_empty_returns_none() {
        let info = SourceInfo::generated(By::sectionize());
        assert!(info.resolve_byte_range().is_none());
    }

    #[test]
    fn test_generated_resolve_byte_range_value_source_only_returns_none() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(5), 100, 110)),
        );
        assert!(info.resolve_byte_range().is_none());
    }

    #[test]
    fn test_generated_remap_file_ids_walks_anchors() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(0), 0, 5)),
        );
        info.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(3), 10, 20)),
        );
        info.remap_file_ids(&|id| FileId(id.0 + 10));
        match &info {
            SourceInfo::Generated { from, .. } => {
                assert_eq!(from.len(), 2);
                match from[0].source_info.as_ref() {
                    SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, FileId(10)),
                    _ => panic!("Expected Original anchor 0"),
                }
                match from[1].source_info.as_ref() {
                    SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, FileId(13)),
                    _ => panic!("Expected Original anchor 1"),
                }
            }
            _ => panic!("Expected Generated"),
        }
    }

    #[test]
    fn test_root_file_id_per_variant() {
        // Original
        let original = SourceInfo::original(FileId(7), 0, 5);
        assert_eq!(original.root_file_id(), Some(FileId(7)));

        // Substring → recurse parent
        let sub = SourceInfo::substring(original.clone(), 0, 5);
        assert_eq!(sub.root_file_id(), Some(FileId(7)));

        // Concat find_map skips Generated holes
        let empty_gen = SourceInfo::generated(By::sectionize());
        let real = SourceInfo::original(FileId(42), 0, 5);
        let concat = SourceInfo::concat(vec![(empty_gen, 0), (real, 5)]);
        assert_eq!(concat.root_file_id(), Some(FileId(42)));

        // Generated with Invocation
        let mut g = SourceInfo::generated(By::shortcode("meta"));
        g.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(9), 0, 1)),
        );
        assert_eq!(g.root_file_id(), Some(FileId(9)));

        // Generated with no Invocation
        let mut g2 = SourceInfo::generated(By::shortcode("meta"));
        g2.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(9), 0, 1)),
        );
        assert_eq!(g2.root_file_id(), None);

        // Generated empty
        let g3 = SourceInfo::generated(By::sectionize());
        assert_eq!(g3.root_file_id(), None);
    }

    #[test]
    fn test_collect_file_ids_walks_every_anchor_role() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(1), 0, 1)),
        );
        info.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(2), 0, 1)),
        );
        info.append_anchor(
            AnchorRole::Other("dispatch".to_string()),
            Arc::new(SourceInfo::original(FileId(3), 0, 1)),
        );
        let mut out = std::collections::HashSet::new();
        info.collect_file_ids(&mut out);
        assert!(out.contains(&FileId(1)));
        assert!(out.contains(&FileId(2)));
        assert!(out.contains(&FileId(3)));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_collect_file_ids_walks_concat_and_substring() {
        let inner = SourceInfo::original(FileId(5), 0, 100);
        let sub = SourceInfo::substring(inner, 10, 20);
        let other = SourceInfo::original(FileId(11), 0, 5);
        let concat = SourceInfo::concat(vec![(sub, 10), (other, 5)]);
        let mut out = std::collections::HashSet::new();
        concat.collect_file_ids(&mut out);
        assert!(out.contains(&FileId(5)));
        assert!(out.contains(&FileId(11)));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn test_invocation_anchor_accessor() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        assert!(info.invocation_anchor().is_none());
        info.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(2), 0, 1)),
        );
        assert!(info.invocation_anchor().is_none());
        info.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(1), 0, 1)),
        );
        assert!(info.invocation_anchor().is_some());
        // Non-Generated returns None.
        assert!(
            SourceInfo::original(FileId(0), 0, 0)
                .invocation_anchor()
                .is_none()
        );
    }

    #[test]
    fn test_value_source_anchor_accessor() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        assert!(info.value_source_anchor().is_none());
        info.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(1), 0, 1)),
        );
        assert!(info.value_source_anchor().is_none());
        info.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(2), 0, 1)),
        );
        assert!(info.value_source_anchor().is_some());
    }

    #[test]
    fn test_anchors_with_role() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(1), 0, 1)),
        );
        info.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(2), 0, 1)),
        );
        info.append_anchor(
            AnchorRole::Other("ext/foo".to_string()),
            Arc::new(SourceInfo::original(FileId(3), 0, 1)),
        );
        assert_eq!(info.anchors_with_role(&AnchorRole::Invocation).count(), 1);
        assert_eq!(info.anchors_with_role(&AnchorRole::ValueSource).count(), 1);
        assert_eq!(
            info.anchors_with_role(&AnchorRole::Other("ext/foo".to_string()))
                .count(),
            1
        );
        assert_eq!(
            info.anchors_with_role(&AnchorRole::Other("missing".to_string()))
                .count(),
            0
        );
    }

    #[test]
    fn test_append_anchor_preserves_order() {
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::original(FileId(1), 0, 1)),
        );
        info.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(FileId(2), 0, 1)),
        );
        match info {
            SourceInfo::Generated { from, .. } => {
                assert_eq!(from.len(), 2);
                assert!(matches!(from[0].role, AnchorRole::Invocation));
                assert!(matches!(from[1].role, AnchorRole::ValueSource));
            }
            _ => panic!("Expected Generated"),
        }
    }

    #[test]
    fn test_combine_with_generated_is_zero_length_piece() {
        let original = SourceInfo::original(FileId(0), 10, 20);
        let generated = SourceInfo::generated(By::sectionize());
        let combined = original.combine(&generated);
        match &combined {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[1].length, 0);
            }
            _ => panic!("Expected Concat"),
        }
        // Length of the combined value equals only the Original side.
        assert_eq!(combined.length(), 10);
    }

    #[test]
    fn test_source_info_serialization() {
        let file_id = FileId(0);
        let range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 10,
                row: 0,
                column: 10,
            },
        };

        let info = SourceInfo::from_range(file_id, range);
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SourceInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info, deserialized);
    }

    #[test]
    fn test_substring_source_info() {
        let file_id = FileId(0);
        let parent_range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 100,
                row: 0,
                column: 100,
            },
        };
        let parent = SourceInfo::from_range(file_id, parent_range);

        let substring = SourceInfo::substring(parent, 10, 20);

        assert_eq!(substring.start_offset(), 10);
        assert_eq!(substring.end_offset(), 20);
        assert_eq!(substring.length(), 10);

        match substring {
            SourceInfo::Substring {
                start_offset,
                end_offset,
                ..
            } => {
                assert_eq!(start_offset, 10);
                assert_eq!(end_offset, 20);
            }
            _ => panic!("Expected Substring mapping"),
        }
    }

    #[test]
    fn test_concat_source_info() {
        let file_id1 = FileId(0);
        let file_id2 = FileId(1);

        let info1 = SourceInfo::from_range(
            file_id1,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 10,
                    row: 0,
                    column: 10,
                },
            },
        );

        let info2 = SourceInfo::from_range(
            file_id2,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 15,
                    row: 0,
                    column: 15,
                },
            },
        );

        let concat = SourceInfo::concat(vec![(info1, 10), (info2, 15)]);

        assert_eq!(concat.start_offset(), 0);
        assert_eq!(concat.end_offset(), 25); // 10 + 15
        assert_eq!(concat.length(), 25);

        match concat {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[0].offset_in_concat, 0);
                assert_eq!(pieces[0].length, 10);
                assert_eq!(pieces[1].offset_in_concat, 10);
                assert_eq!(pieces[1].length, 15);
            }
            _ => panic!("Expected Concat mapping"),
        }
    }

    #[test]
    fn test_combine_two_sources() {
        let file_id = FileId(0);

        // Create two separate source info objects
        let info1 = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 10,
                    row: 0,
                    column: 10,
                },
            },
        );

        let info2 = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 15,
                    row: 0,
                    column: 15,
                },
                end: Location {
                    offset: 25,
                    row: 0,
                    column: 25,
                },
            },
        );

        // Combine them
        let combined = info1.combine(&info2);

        // Should create a Concat with total length = 10 + 10 = 20
        assert_eq!(combined.start_offset(), 0);
        assert_eq!(combined.end_offset(), 20);
        assert_eq!(combined.length(), 20);

        match combined {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[0].length, 10);
                assert_eq!(pieces[0].offset_in_concat, 0);
                assert_eq!(pieces[1].length, 10);
                assert_eq!(pieces[1].offset_in_concat, 10);
            }
            _ => panic!("Expected Concat mapping"),
        }
    }

    #[test]
    fn test_combine_preserves_source_tracking() {
        // Combine sources from different files
        let file_id1 = FileId(5);
        let file_id2 = FileId(10);

        let info1 = SourceInfo::from_range(
            file_id1,
            Range {
                start: Location {
                    offset: 100,
                    row: 5,
                    column: 0,
                },
                end: Location {
                    offset: 105,
                    row: 5,
                    column: 5,
                },
            },
        );

        let info2 = SourceInfo::from_range(
            file_id2,
            Range {
                start: Location {
                    offset: 200,
                    row: 10,
                    column: 0,
                },
                end: Location {
                    offset: 207,
                    row: 10,
                    column: 7,
                },
            },
        );

        let combined = info1.combine(&info2);

        // Verify both sources are preserved in the Concat
        match combined {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);

                // First piece should come from file_id1
                match &pieces[0].source_info {
                    SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, file_id1),
                    _ => panic!("Expected Original mapping for first piece"),
                }

                // Second piece should come from file_id2
                match &pieces[1].source_info {
                    SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, file_id2),
                    _ => panic!("Expected Original mapping for second piece"),
                }
            }
            _ => panic!("Expected Concat mapping"),
        }
    }

    /// Test JSON serialization of Original mapping
    #[test]
    fn test_json_serialization_original() {
        let file_id = FileId(0);
        let range = Range {
            start: Location {
                offset: 10,
                row: 1,
                column: 5,
            },
            end: Location {
                offset: 50,
                row: 3,
                column: 10,
            },
        };

        let info = SourceInfo::from_range(file_id, range);
        let json = serde_json::to_value(&info).unwrap();

        // Verify JSON structure
        assert_eq!(json["Original"]["file_id"], 0);
        assert_eq!(json["Original"]["start_offset"], 10);
        assert_eq!(json["Original"]["end_offset"], 50);

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(info, deserialized);
    }

    /// Test JSON serialization of Substring mapping
    #[test]
    fn test_json_serialization_substring() {
        let file_id = FileId(0);
        let parent_range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 100,
                row: 5,
                column: 20,
            },
        };
        let parent = SourceInfo::from_range(file_id, parent_range);

        let substring = SourceInfo::substring(parent, 10, 30);
        let json = serde_json::to_value(&substring).unwrap();

        // Verify JSON structure
        assert_eq!(json["Substring"]["start_offset"], 10);
        assert_eq!(json["Substring"]["end_offset"], 30);

        // Verify parent is serialized (with Rc, it's a full copy in JSON)
        assert!(json["Substring"]["parent"].is_object());
        assert_eq!(json["Substring"]["parent"]["Original"]["file_id"], 0);

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(substring, deserialized);
    }

    /// Test JSON serialization of nested Substring mappings (simulates .qmd frontmatter)
    #[test]
    fn test_json_serialization_nested_substring() {
        let file_id = FileId(0);

        // Level 1: Original file
        let file_range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 200,
                row: 10,
                column: 0,
            },
        };
        let file_info = SourceInfo::from_range(file_id, file_range);

        // Level 2: YAML frontmatter (substring of file)
        let yaml_info = SourceInfo::substring(file_info, 4, 150);

        // Level 3: YAML value (substring of frontmatter)
        let value_info = SourceInfo::substring(yaml_info, 20, 35);

        let json = serde_json::to_value(&value_info).unwrap();

        // Verify nested structure
        assert_eq!(json["Substring"]["start_offset"], 20);
        assert_eq!(json["Substring"]["end_offset"], 35);
        assert_eq!(json["Substring"]["parent"]["Substring"]["start_offset"], 4);
        assert_eq!(
            json["Substring"]["parent"]["Substring"]["parent"]["Original"]["file_id"],
            0
        );

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(value_info, deserialized);
    }

    /// Test JSON serialization of Concat mapping
    #[test]
    fn test_json_serialization_concat() {
        let file_id1 = FileId(0);
        let file_id2 = FileId(1);

        let info1 = SourceInfo::from_range(
            file_id1,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 10,
                    row: 0,
                    column: 10,
                },
            },
        );

        let info2 = SourceInfo::from_range(
            file_id2,
            Range {
                start: Location {
                    offset: 20,
                    row: 2,
                    column: 0,
                },
                end: Location {
                    offset: 30,
                    row: 2,
                    column: 10,
                },
            },
        );

        let combined = info1.combine(&info2);
        let json = serde_json::to_value(&combined).unwrap();

        // Verify JSON structure
        assert!(json["Concat"]["pieces"].is_array());
        let pieces = json["Concat"]["pieces"].as_array().unwrap();
        assert_eq!(pieces.len(), 2);

        // First piece
        assert_eq!(pieces[0]["offset_in_concat"], 0);
        assert_eq!(pieces[0]["length"], 10);
        assert_eq!(pieces[0]["source_info"]["Original"]["file_id"], 0);

        // Second piece
        assert_eq!(pieces[1]["offset_in_concat"], 10);
        assert_eq!(pieces[1]["length"], 10);
        assert_eq!(pieces[1]["source_info"]["Original"]["file_id"], 1);

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(combined, deserialized);
    }

    /// Test JSON serialization of complex nested structure (real-world example)
    #[test]
    fn test_json_serialization_complex_nested() {
        let file_id = FileId(0);

        // Simulate a .qmd file structure
        let qmd_file = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 500,
                    row: 20,
                    column: 0,
                },
            },
        );

        // YAML frontmatter is a substring
        let yaml_frontmatter = SourceInfo::substring(qmd_file.clone(), 4, 200);

        // A YAML key is a substring of frontmatter
        let yaml_key = SourceInfo::substring(yaml_frontmatter.clone(), 10, 20);

        // A YAML value is another substring of frontmatter
        let yaml_value = SourceInfo::substring(yaml_frontmatter, 25, 50);

        // Combine key and value (simulating metadata entry)
        let combined = yaml_key.combine(&yaml_value);

        let json = serde_json::to_value(&combined).unwrap();

        // Verify this complex structure serializes
        assert!(json.is_object());
        assert!(json["Concat"].is_object());

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(combined, deserialized);
    }

    // -------------------------------------------------------------------------
    // Plan 7 — preimage_in accessor
    // -------------------------------------------------------------------------

    #[test]
    fn test_preimage_in_original_same_file() {
        let info = SourceInfo::original(FileId(0), 10, 25);
        assert_eq!(info.preimage_in(FileId(0)), Some(10..25));
    }

    #[test]
    fn test_preimage_in_original_different_file_returns_none() {
        let info = SourceInfo::original(FileId(0), 10, 25);
        assert_eq!(info.preimage_in(FileId(1)), None);
    }

    #[test]
    fn test_preimage_in_substring_composes_offsets() {
        // Parent points at bytes 100..200 in file 0.
        // Substring takes bytes 5..15 *relative to parent*.
        // Preimage in file 0 should be 105..115.
        let parent = SourceInfo::original(FileId(0), 100, 200);
        let info = SourceInfo::substring(parent, 5, 15);
        assert_eq!(info.preimage_in(FileId(0)), Some(105..115));
    }

    #[test]
    fn test_preimage_in_substring_different_file_returns_none() {
        let parent = SourceInfo::original(FileId(0), 100, 200);
        let info = SourceInfo::substring(parent, 5, 15);
        assert_eq!(info.preimage_in(FileId(7)), None);
    }

    #[test]
    fn test_preimage_in_substring_chain() {
        // Original 1000..2000 in file 0; Substring 100..500 relative; Substring 10..50 relative.
        // Expected preimage in file 0: 1100 + 10 .. 1100 + 50 = 1110..1150.
        let root = SourceInfo::original(FileId(0), 1000, 2000);
        let mid = SourceInfo::substring(root, 100, 500);
        let leaf = SourceInfo::substring(mid, 10, 50);
        assert_eq!(leaf.preimage_in(FileId(0)), Some(1110..1150));
    }

    #[test]
    fn test_preimage_in_concat_contiguous() {
        // Two adjacent pieces of file 0: 10..15 and 15..25 → contiguous → 10..25.
        let a = SourceInfo::original(FileId(0), 10, 15);
        let b = SourceInfo::original(FileId(0), 15, 25);
        let info = SourceInfo::concat(vec![(a, 5), (b, 10)]);
        assert_eq!(info.preimage_in(FileId(0)), Some(10..25));
    }

    #[test]
    fn test_preimage_in_concat_gappy_returns_none() {
        // 10..15 then 20..25 → gap between 15 and 20 → None.
        let a = SourceInfo::original(FileId(0), 10, 15);
        let b = SourceInfo::original(FileId(0), 20, 25);
        let info = SourceInfo::concat(vec![(a, 5), (b, 5)]);
        assert_eq!(info.preimage_in(FileId(0)), None);
    }

    #[test]
    fn test_preimage_in_concat_overlapping_returns_none() {
        // 10..20 then 15..25 → overlap → not byte-contiguous → None.
        let a = SourceInfo::original(FileId(0), 10, 20);
        let b = SourceInfo::original(FileId(0), 15, 25);
        let info = SourceInfo::concat(vec![(a, 10), (b, 10)]);
        assert_eq!(info.preimage_in(FileId(0)), None);
    }

    #[test]
    fn test_preimage_in_concat_mixed_files_returns_none() {
        // One piece in file 0, another in file 1 → resolving in file 0 fails
        // because the file-1 piece can't be resolved.
        let a = SourceInfo::original(FileId(0), 10, 15);
        let b = SourceInfo::original(FileId(1), 15, 25);
        let info = SourceInfo::concat(vec![(a, 5), (b, 10)]);
        assert_eq!(info.preimage_in(FileId(0)), None);
    }

    #[test]
    fn test_preimage_in_generated_no_anchors_returns_none() {
        // Sectionize-style wrapper, footnotes-container, etc.: Generated with
        // empty `from`. No Invocation anchor → no preimage.
        let info = SourceInfo::generated(By::sectionize());
        assert_eq!(info.preimage_in(FileId(0)), None);
    }

    #[test]
    fn test_preimage_in_generated_with_invocation_in_target() {
        // Shortcode resolution: Generated with an Invocation anchor pointing
        // at the {{< meta foo >}} token bytes.
        let token = SourceInfo::original(FileId(0), 50, 70);
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(AnchorRole::Invocation, Arc::new(token));
        assert_eq!(info.preimage_in(FileId(0)), Some(50..70));
    }

    #[test]
    fn test_preimage_in_generated_with_invocation_outside_target() {
        // Invocation anchor points at file 0; query asks about file 1 → None.
        let token = SourceInfo::original(FileId(0), 50, 70);
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(AnchorRole::Invocation, Arc::new(token));
        assert_eq!(info.preimage_in(FileId(1)), None);
    }

    #[test]
    fn test_preimage_in_generated_walks_through_substring_in_invocation() {
        // Invocation anchor is itself a Substring chain. preimage_in must
        // walk through it correctly.
        let root = SourceInfo::original(FileId(0), 100, 200);
        let token = SourceInfo::substring(root, 10, 30);
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(AnchorRole::Invocation, Arc::new(token));
        assert_eq!(info.preimage_in(FileId(0)), Some(110..130));
    }

    // -------------------------------------------------------------------------
    // Plan 7 — preimage_in role-asymmetry: only Invocation is walked.
    // -------------------------------------------------------------------------

    #[test]
    fn test_preimage_in_generated_value_source_only_returns_none() {
        // Plan 9-shape: Generated whose only anchor is ValueSource (points at
        // YAML metadata bytes). The writer must NOT copy those bytes into the
        // body — preimage_in returns None.
        let meta_si = SourceInfo::original(FileId(0), 10, 25);
        let mut info = SourceInfo::generated(By::appendix());
        info.append_anchor(AnchorRole::ValueSource, Arc::new(meta_si));
        assert_eq!(info.preimage_in(FileId(0)), None);
    }

    #[test]
    fn test_preimage_in_generated_other_only_returns_none() {
        // Extension-defined Other role. preimage_in must not walk it.
        let lua_si = SourceInfo::original(FileId(0), 10, 25);
        let mut info = SourceInfo::generated(By::filter("upper.lua", 14));
        info.append_anchor(
            AnchorRole::Other("ext/my-ext/dispatch".to_string()),
            Arc::new(lua_si),
        );
        assert_eq!(info.preimage_in(FileId(0)), None);
    }

    #[test]
    fn test_preimage_in_generated_invocation_plus_value_source_walks_invocation_only() {
        // Plan 2/Plan 9 mixed shape: Invocation in file 0 + ValueSource in
        // file 1. Query file 0 → Invocation resolves → Some(token range).
        // Query file 1 → Invocation resolves to file 0 (not 1) → None.
        // (The writer must not see the value-source range when asked about
        // any file, even the file the ValueSource points into.)
        let token = SourceInfo::original(FileId(0), 50, 70);
        let value = SourceInfo::original(FileId(1), 200, 215);
        let mut info = SourceInfo::generated(By::shortcode("meta"));
        info.append_anchor(AnchorRole::Invocation, Arc::new(token));
        info.append_anchor(AnchorRole::ValueSource, Arc::new(value));

        assert_eq!(info.preimage_in(FileId(0)), Some(50..70));
        assert_eq!(info.preimage_in(FileId(1)), None);
    }
}
