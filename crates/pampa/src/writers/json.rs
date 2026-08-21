/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::{AttrSourceInfo, TargetSourceInfo, is_empty_attr};
use crate::pandoc::shortcode::shortcode_to_span;
use crate::pandoc::{
    ASTContext, Attr, Block, Caption, CitationMode, Inline, Inlines, ListAttributes, Pandoc,
};
use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::{AnchorRole, By, FileId, SourceInfo};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

/// Per-node attribution record consumed by the JSON writer. Populated
/// by `quarto_core::transforms::AttributionRenderTransform` and threaded
/// in through [`JsonConfig::attribution_by_node`]. Mirrors the HTML
/// writer's `HtmlAttributionRecord` (same fields, separate type so
/// the writer-local Cargo deps stay clean).
#[derive(Debug, Clone)]
pub struct JsonAttributionRecord {
    pub actor: Arc<str>,
    pub time: i64,
}

/// Per-actor identity (display name + colour) carried in the
/// `astContext.attributionActors` table. Joined per-record by the
/// hub-client consumer; not duplicated per record on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonAttributionIdentity {
    pub display_name: String,
    pub color: String,
}

/// Configuration for JSON output format.
#[derive(Debug, Clone, Default)]
pub struct JsonConfig {
    /// If true, include resolved source locations ('l' field) in each node.
    /// The 'l' field contains an object with:
    /// - 'f': file_id (usize)
    /// - 'b': begin position {o: offset, l: line (1-based), c: column (1-based)}
    /// - 'e': end position {o: offset, l: line (1-based), c: column (1-based)}
    pub include_inline_locations: bool,

    /// Pointer-keyed lookup populated by
    /// `quarto_core::transforms::AttributionRenderTransform`. Keys are
    /// `&Block` / `&Inline` cast through `*const ()` to `usize`. At
    /// each node visit the writer does a single `HashMap::get` and, on
    /// hit, accumulates `{ s, actor, time }` for the eventual
    /// `astContext.attribution` array. `None` means attribution is off
    /// (the off-path JSON output is byte-identical to today's).
    pub attribution_by_node: Option<Arc<HashMap<usize, JsonAttributionRecord>>>,

    /// Actor → `(name, color)` table. Total over every actor referenced
    /// by `attribution_by_node`, including warning-path placeholders.
    /// Emitted as `astContext.attributionActors` when any entry is used.
    pub attribution_actors: Option<Arc<HashMap<Arc<str>, JsonAttributionIdentity>>>,

    /// If true, emit the pampa-native `raw-json` format (bd-en2hvrwn,
    /// GH #11) instead of the Pandoc-superset shape:
    ///
    /// - the `pampa-json-format` envelope marker is emitted first and
    ///   `pandoc-api-version` is omitted;
    /// - pampa AST extensions (standalone `Attr`, `NoteReference`,
    ///   CriticMarkup inlines, `Shortcode`, `CaptionBlock`) are written
    ///   with native tags instead of being desugared or rejected;
    /// - metadata is written faithfully (full `ConfigValue`: Path/Glob/
    ///   Expr kinds, merge ops, scalar types, entry order) as a single
    ///   config-value node instead of a sorted Pandoc-style meta object;
    /// - `astContext` carries `exampleListCounter`.
    ///
    /// The contract is that write-then-read is the identity on the AST.
    /// Only the streaming writer implements raw mode; use the
    /// `writers::raw_json` / `readers::raw_json` entry points.
    pub raw: bool,
}

// ============================================================================
// JSON Output Structs
// ============================================================================
//
// These structs define the JSON output format with explicit field ordering.
// Serde serializes struct fields in declaration order, so fields are ordered
// alphabetically to ensure deterministic output regardless of serde_json's
// `preserve_order` feature.

/// Top-level Pandoc JSON document structure.
/// Field order matches expected alphabetical JSON key order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PandocDocumentJson {
    ast_context: AstContextJson,
    blocks: Vec<Value>,
    meta: Value,
    #[serde(rename = "pandoc-api-version")]
    pandoc_api_version: [u32; 3],
}

/// AST context with source info pool.
/// Fields ordered alphabetically.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AstContextJson {
    files: Vec<FileEntryJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta_top_level_key_sources: Option<Value>,
    #[serde(rename = "p", skip_serializing_if = "Vec::is_empty")]
    source_info_pool: Vec<SourceInfoJson>,
}

/// File entry in AST context.
/// Fields ordered alphabetically.
#[derive(Serialize)]
struct FileEntryJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    line_breaks: Option<Vec<usize>>,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_length: Option<usize>,
}

/// Source info entry in the pool.
/// Fields ordered alphabetically: d, r, t
#[derive(Serialize)]
struct SourceInfoJson {
    d: Value,      // data (file_id, parent_id, pieces, or Generated { by, from })
    r: [usize; 2], // range [start, end]
    // type code:
    //   0 = Original
    //   1 = Substring
    //   2 = Concat
    //   3 = Legacy (read-only — old Transformed + buggy FilterProvenance)
    //   4 = Generated { by, from }
    t: u8,
}

/// Generic node with type, optional content, and source info.
/// Fields ordered alphabetically: c, s, t
#[derive(Serialize)]
struct NodeJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    c: Option<Value>, // content
    s: usize,  // source info ID
    t: String, // type name
}

/// Attribute source info with alphabetically ordered fields.
/// Fields: classes, id, kvs
#[derive(Serialize)]
struct AttrSourceJson {
    classes: Vec<Value>,
    id: Option<Value>,
    kvs: Vec<[Option<Value>; 2]>,
}

/// Node with attribute source info.
/// Fields ordered alphabetically: a, c, s, t
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeWithAttrJson {
    #[serde(rename = "a")]
    attr_s: AttrSourceJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    c: Option<Value>,
    s: usize,
    t: String,
}

/// Serializable version of SourceInfo that uses ID references instead of Rc pointers.
///
/// This structure is used during JSON serialization to avoid duplicating parent chains.
/// Each unique SourceInfo is assigned an ID and stored in a pool. References to parent
/// SourceInfo objects are replaced with parent_id integers.
///
/// Serializes in compact format: {"r": [2 offset values], "t": type_code, "d": type_data}
/// The ID is implicit from the array index in the pool.
///
/// Note: Row/column information is not stored in the serialized format.
/// To get row/column, the reader must map offsets through the SourceContext.
struct SerializableSourceInfo {
    id: usize,
    start_offset: usize,
    end_offset: usize,
    mapping: SerializableSourceMapping,
}

impl SerializableSourceInfo {
    /// Convert to SourceInfoJson for serialization with deterministic field order.
    fn to_json(&self) -> SourceInfoJson {
        let (t, d) = match &self.mapping {
            SerializableSourceMapping::Original { file_id } => (0, json!(file_id.0)),
            SerializableSourceMapping::Substring { parent_id } => (1, json!(parent_id)),
            SerializableSourceMapping::Concat { pieces } => {
                let piece_arrays: Vec<[usize; 3]> = pieces
                    .iter()
                    .map(|p| [p.source_info_id, p.offset_in_concat, p.length])
                    .collect();
                (2, json!(piece_arrays))
            }
            SerializableSourceMapping::Generated { by, from } => {
                let mut by_json = json!({ "kind": by.kind });
                if !by.data.is_null() {
                    by_json["data"] = by.data.clone();
                }
                let mut d_obj = serde_json::Map::new();
                d_obj.insert("by".to_string(), by_json);
                if !from.is_empty() {
                    let arr: Vec<Value> = from
                        .iter()
                        .map(|(role, si_id)| {
                            json!({
                                "role": serialize_anchor_role(role),
                                "si_id": si_id,
                            })
                        })
                        .collect();
                    d_obj.insert("from".to_string(), Value::Array(arr));
                }
                (4, Value::Object(d_obj))
            }
        };
        SourceInfoJson {
            d,
            r: [self.start_offset, self.end_offset],
            t,
        }
    }
}

/// Serialize an [`AnchorRole`] to its wire-format string.
///
/// Inverse of `parse_anchor_role` in `crates/pampa/src/readers/json.rs`.
/// The two must agree on the string forms — see also the TS mirror at
/// `ts-packages/preview-renderer/src/types/sourceInfo.ts`.
fn serialize_anchor_role(role: &AnchorRole) -> String {
    match role {
        AnchorRole::Invocation => "invocation".to_string(),
        AnchorRole::ValueSource => "value-source".to_string(),
        AnchorRole::Other(s) => format!("other:{}", s),
    }
}

/// Serializable version of SourceMapping that uses parent_id instead of Rc<SourceInfo>.
enum SerializableSourceMapping {
    Original {
        file_id: FileId,
    },
    Substring {
        parent_id: usize,
    },
    Concat {
        pieces: Vec<SerializableSourcePiece>,
    },
    /// Wire-code 4: a pipeline transform's output.
    ///
    /// `by` carries the producer identity (kebab-case `kind` + optional
    /// JSON `data`). `from` is an ordered list of `(role, si_id)`
    /// pairs — each `si_id` points to another pool entry that already
    /// exists (interned strictly before this entry).
    Generated {
        by: By,
        from: Vec<(AnchorRole, usize)>,
    },
}

/// Serializable version of SourcePiece that uses source_info_id instead of SourceInfo.
struct SerializableSourcePiece {
    source_info_id: usize,
    offset_in_concat: usize,
    length: usize,
}

/// Reserved source-info pool slot for React-constructed (user-edit) content.
///
/// Serializer that builds a pool of unique SourceInfo objects and assigns IDs.
///
/// During AST traversal, each SourceInfo is interned into the pool. Rc-shared
/// SourceInfo objects get the same ID (using pointer equality). Parent references
/// are serialized as parent_id integers instead of full nested objects.
///
/// This approach reduces JSON size by ~93% for documents with many nodes sharing
/// the same parent chains (e.g., YAML metadata with siblings).
///
struct SourceInfoSerializer<'a> {
    pool: Vec<SerializableSourceInfo>,
    // Dedup cache for `Substring` parent edges, keyed by `Arc::as_ptr(parent)`.
    // The Arc inside a Substring is owned by AST nodes for the full serialization
    // lifetime, so its inner address is stable — unlike a raw `*const SourceInfo`
    // borrowed from a by-value AST field, which is what the previous design cached
    // and what caused the 2026-01-13 memory-reuse bug.
    arc_parent_ids: HashMap<*const SourceInfo, usize>,
    context: &'a ASTContext,
    config: &'a JsonConfig,
    // Diagnostic counters for the intern hotspot (bd-h5l7). Printed on Drop when
    // QUARTO_PERF_STATS=1. Free when unused.
    stat_intern_calls: usize,
    stat_arc_parent_hits: usize,
}

impl<'a> SourceInfoSerializer<'a> {
    fn new(context: &'a ASTContext, config: &'a JsonConfig) -> Self {
        SourceInfoSerializer {
            pool: Vec::new(),
            arc_parent_ids: HashMap::new(),
            context,
            config,
            stat_intern_calls: 0,
            stat_arc_parent_hits: 0,
        }
    }

    /// Intern a SourceInfo into the pool, returning its ID.
    ///
    /// Each call allocates a fresh pool entry. The one cache, `arc_parent_ids`,
    /// dedups `Substring` parent Arcs at the recursion edge — the only place
    /// pointer identity is genuinely stable across calls. Pool entries are not
    /// deduplicated by content: two structurally-equal SourceInfo values
    /// arriving through different call sites will get different pool IDs. See
    /// `claude-notes/plans/2026-04-22-sourceinfo-eq-hotspot.md` for why.
    ///
    fn intern(&mut self, source_info: &SourceInfo) -> usize {
        self.stat_intern_calls += 1;

        // Extract offsets and recursively intern parents to build the serializable mapping
        let (start_offset, end_offset, mapping) = match source_info {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => (
                *start_offset,
                *end_offset,
                SerializableSourceMapping::Original { file_id: *file_id },
            ),
            SourceInfo::Substring {
                parent,
                start_offset,
                end_offset,
            } => {
                // Dedup the parent edge by Arc identity. `Arc::as_ptr` is stable
                // for the lifetime of any reference holding the Arc; here the AST
                // owns the Arc for the whole serialization.
                let parent_arc_ptr = std::sync::Arc::as_ptr(parent);
                let parent_id = if let Some(&id) = self.arc_parent_ids.get(&parent_arc_ptr) {
                    self.stat_arc_parent_hits += 1;
                    id
                } else {
                    let id = self.intern(parent);
                    self.arc_parent_ids.insert(parent_arc_ptr, id);
                    id
                };
                (
                    *start_offset,
                    *end_offset,
                    SerializableSourceMapping::Substring { parent_id },
                )
            }
            SourceInfo::Concat { pieces } => {
                let serializable_pieces = pieces
                    .iter()
                    .map(|piece| SerializableSourcePiece {
                        source_info_id: self.intern(&piece.source_info),
                        offset_in_concat: piece.offset_in_concat,
                        length: piece.length,
                    })
                    .collect();
                (
                    0,
                    pieces.iter().map(|p| p.length).sum(),
                    SerializableSourceMapping::Concat {
                        pieces: serializable_pieces,
                    },
                )
            }
            SourceInfo::Generated { by, from } => {
                // Anchors are interned *before* this Generated entry so that
                // every si_id is strictly less than the resulting pool index
                // — the reader's `si_id < current_index` guard depends on it.
                //
                // Dedup keyed by `Arc::as_ptr(&anchor.source_info)`, sharing
                // the same `arc_parent_ids` cache used for `Substring.parent`.
                // Multi-inline shortcode resolutions whose anchors point at a
                // shared `Arc` collapse to a single pool entry on the write
                // side; deserialization rebuilds each anchor with a fresh
                // Arc, so this is a write-time optimization only (see Plan 5
                // §"Risk areas" → anchor-dedup-invariant).
                let from_ids: Vec<(AnchorRole, usize)> = from
                    .iter()
                    .map(|anchor| {
                        let arc_ptr = std::sync::Arc::as_ptr(&anchor.source_info);
                        let id = if let Some(&id) = self.arc_parent_ids.get(&arc_ptr) {
                            self.stat_arc_parent_hits += 1;
                            id
                        } else {
                            let id = self.intern(&anchor.source_info);
                            self.arc_parent_ids.insert(arc_ptr, id);
                            id
                        };
                        (anchor.role.clone(), id)
                    })
                    .collect();
                (
                    0,
                    0,
                    SerializableSourceMapping::Generated {
                        by: by.clone(),
                        from: from_ids,
                    },
                )
            }
        };

        let id = self.pool.len();
        self.pool.push(SerializableSourceInfo {
            id,
            start_offset,
            end_offset,
            mapping,
        });

        id
    }

    /// Serialize a SourceInfo as a JSON reference: just the id number
    fn to_json_ref(&mut self, source_info: &SourceInfo) -> Value {
        let id = self.intern(source_info);
        json!(id)
    }

    /// Add source info fields to a JSON object.
    /// Always adds 's' field (source info ID).
    /// If config.include_inline_locations is true, also adds 'l' field with resolved location.
    fn add_source_info(
        &mut self,
        obj: &mut serde_json::Map<String, Value>,
        source_info: &SourceInfo,
    ) {
        let id = self.intern(source_info);
        obj.insert("s".to_string(), json!(id));

        if self.config.include_inline_locations
            && let Some(location) = resolve_location(source_info, self.context)
        {
            obj.insert("l".to_string(), location);
        }
    }
}

impl<'a> Drop for SourceInfoSerializer<'a> {
    fn drop(&mut self) {
        if std::env::var_os("QUARTO_PERF_STATS").is_some_and(|v| v == "1") {
            eprintln!(
                "perf.intern intern_calls={} arc_parent_hits={} pool_size={}",
                self.stat_intern_calls,
                self.stat_arc_parent_hits,
                self.pool.len(),
            );
        }
    }
}

/// Accumulated per-node attribution record. Pushed during AST walk
/// by `JsonWriterContext::maybe_record_attribution`; emitted in
/// `astContext.attribution` by `stream_write_pandoc`.
///
/// `s` is the source-info pool ID returned by `intern` for the node's
/// `source_info`. `actor` is an `Arc<str>` pointer-equal to the key in
/// `JsonConfig::attribution_actors` (the Phase 1 interning invariant).
struct AttributionRecordOut {
    s: usize,
    actor: Arc<str>,
    time: i64,
}

/// Context for JSON writer containing both source info serialization and error collection.
///
/// This struct combines the SourceInfoSerializer (for building the source info pool)
/// with error accumulation during AST traversal. Separating these concerns makes the
/// dual purpose of the writer more explicit.
struct JsonWriterContext<'a> {
    serializer: SourceInfoSerializer<'a>,
    errors: Vec<DiagnosticMessage>,
    /// Records collected during the AST walk. Each entry corresponds to
    /// a Block or Inline node whose pointer was found in
    /// `config.attribution_by_node`. Empty when attribution is off.
    attribution_records: Vec<AttributionRecordOut>,
}

impl<'a> JsonWriterContext<'a> {
    fn new(ast_context: &'a ASTContext, config: &'a JsonConfig) -> Self {
        JsonWriterContext {
            serializer: SourceInfoSerializer::new(ast_context, config),
            errors: Vec::new(),
            attribution_records: Vec::new(),
        }
    }

    /// If attribution is enabled and `source_info`'s field address is
    /// keyed in `attribution_by_node`, record `(s_id, actor, time)`.
    /// No-op when attribution is off or no map entry exists.
    ///
    /// The key is `source_info as *const SourceInfo as usize` — the
    /// address of the `source_info` field inside the owning Block /
    /// Inline. That address is stable as long as the AST isn't moved,
    /// which holds across the JSON serializer's read-only walk (and is
    /// guaranteed by `AttributionRenderTransform`'s position at the end
    /// of the Finalization Phase). The render transform uses the same
    /// key when populating `attribution_by_node`.
    fn maybe_record_attribution_for(&mut self, source_info: &SourceInfo, s_id: usize) {
        let Some(map) = self.serializer.config.attribution_by_node.as_ref() else {
            return;
        };
        let key = source_info as *const SourceInfo as usize;
        if let Some(rec) = map.get(&key) {
            self.attribution_records.push(AttributionRecordOut {
                s: s_id,
                actor: Arc::clone(&rec.actor),
                time: rec.time,
            });
        }
    }
}

/// Resolve source info to fully resolved location with file_id, line, column, and offset.
///
/// Returns None if the source info cannot be mapped (e.g., synthetic nodes).
///
/// The returned JSON has the structure:
/// ```json
/// {
///   "f": file_id,
///   "b": {"o": offset, "l": line (1-based), "c": column (1-based)},
///   "e": {"o": offset, "l": line (1-based), "c": column (1-based)}
/// }
/// ```
fn resolve_location(source_info: &SourceInfo, context: &ASTContext) -> Option<Value> {
    // Map both start and end offsets through the transformation chain
    let (start_mapped, end_mapped) =
        source_info.map_range(0, source_info.length(), &context.source_context)?;

    // Convert from 0-indexed (internal) to 1-based (output) for line and column
    Some(json!({
        "f": start_mapped.file_id.0,
        "b": {
            "o": start_mapped.location.offset,
            "l": start_mapped.location.row + 1,
            "c": start_mapped.location.column + 1
        },
        "e": {
            "o": end_mapped.location.offset,
            "l": end_mapped.location.row + 1,
            "c": end_mapped.location.column + 1
        }
    }))
}

/// Build Inlines for a Path ConfigValue variant.
///
/// Path values are serialized as a simple Str inline containing the path string.
fn build_path_inlines(path: &str, source_info: &SourceInfo) -> Inlines {
    vec![crate::pandoc::Inline::Str(crate::pandoc::Str {
        text: path.to_string(),
        source_info: source_info.clone(),
    })]
}

/// Build Inlines for a Glob ConfigValue variant.
///
/// Glob values are serialized as a Span with class="yaml-tagged-string" and tag="glob".
fn build_glob_inlines(glob: &str, source_info: &SourceInfo) -> Inlines {
    let mut attributes = LinkedHashMap::new();
    attributes.insert("tag".to_string(), "glob".to_string());
    vec![crate::pandoc::Inline::Span(crate::pandoc::Span {
        attr: (
            String::new(),
            vec!["yaml-tagged-string".to_string()],
            attributes,
        ),
        content: vec![crate::pandoc::Inline::Str(crate::pandoc::Str {
            text: glob.to_string(),
            source_info: source_info.clone(),
        })],
        // Wrapper around the glob scalar — reuse the value's range
        // so attribution points at the YAML.
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })]
}

/// Build Inlines for an Expr ConfigValue variant.
///
/// Expr values are serialized as a Span with class="yaml-tagged-string" and tag="expr".
fn build_expr_inlines(expr: &str, source_info: &SourceInfo) -> Inlines {
    let mut attributes = LinkedHashMap::new();
    attributes.insert("tag".to_string(), "expr".to_string());
    vec![crate::pandoc::Inline::Span(crate::pandoc::Span {
        attr: (
            String::new(),
            vec!["yaml-tagged-string".to_string()],
            attributes,
        ),
        content: vec![crate::pandoc::Inline::Str(crate::pandoc::Str {
            text: expr.to_string(),
            source_info: source_info.clone(),
        })],
        // Wrapper around the expr scalar — same reasoning as the
        // glob branch above.
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })]
}

/// Helper to build a node JSON object with type, optional content, and source info.
///
/// This centralizes the pattern of creating nodes with 'c', 's', 't', and optionally 'l' fields.
/// Fields are ordered alphabetically for deterministic JSON output.
fn node_with_source(
    t: &str,
    c: Option<Value>,
    source_info: &SourceInfo,
    ctx: &mut JsonWriterContext,
) -> Value {
    let id = ctx.serializer.intern(source_info);

    // Build base node with alphabetically ordered fields: c, s, t
    let node = NodeJson {
        c,
        s: id,
        t: t.to_string(),
    };

    // Convert to Value and add 'l' field if needed
    let mut value = serde_json::to_value(node).unwrap();

    // Add location field if configured
    if ctx.serializer.config.include_inline_locations
        && let Some(location) = resolve_location(source_info, ctx.serializer.context)
        && let Value::Object(ref mut obj) = value
    {
        obj.insert("l".to_string(), location);
    }

    value
}

// NOTE: This function is currently unused and would need a SourceContext parameter
// to map offsets to row/column positions. Commenting out for now.
// fn write_location(source_info: &quarto_source_map::SourceInfo, ctx: &SourceContext) -> Value {
//     // Extract filename index by walking to the Original mapping
//     let filename_index = source_info.root_file_id().map(|fid| fid.0);
//
//     // Map start and end offsets to locations with row/column
//     let start_mapped = source_info.map_offset(0, ctx).unwrap();
//     let end_mapped = source_info.map_offset(source_info.length(), ctx).unwrap();
//
//     json!({
//         "start": {
//             "offset": source_info.start_offset(),
//             "row": start_mapped.location.row,
//             "column": start_mapped.location.column,
//         },
//         "end": {
//             "offset": source_info.end_offset(),
//             "row": end_mapped.location.row,
//             "column": end_mapped.location.column,
//         },
//         "filenameIndex": filename_index,
//     })
// }

fn write_attr(attr: &Attr) -> Value {
    json!([
        attr.0, // id
        attr.1, // classes
        attr.2
            .iter()
            .map(|(k, v)| json!([k, v]))
            .collect::<Vec<_>>()  // key-value pairs
    ])
}

/// Serialize AttrSourceInfo as JSON with alphabetically ordered fields.
///
/// Format: {
///   "classes": [<source_info_ref or null>, ...],
///   "id": <source_info_ref or null>,
///   "kvs": [[<key_ref or null>, <value_ref or null>], ...]
/// }
fn write_attr_source(attr_source: &AttrSourceInfo, ctx: &mut JsonWriterContext) -> Value {
    let result = AttrSourceJson {
        classes: attr_source
            .classes
            .iter()
            .map(|cls| {
                cls.as_ref()
                    .map_or(Value::Null, |s| ctx.serializer.to_json_ref(s))
            })
            .collect(),
        id: attr_source
            .id
            .as_ref()
            .map(|s| ctx.serializer.to_json_ref(s)),
        kvs: attr_source
            .attributes
            .iter()
            .map(|(k, v)| {
                [
                    k.as_ref().map(|s| ctx.serializer.to_json_ref(s)),
                    v.as_ref().map(|s| ctx.serializer.to_json_ref(s)),
                ]
            })
            .collect(),
    };
    serde_json::to_value(result).unwrap()
}

fn write_target_source(target_source: &TargetSourceInfo, ctx: &mut JsonWriterContext) -> Value {
    json!([
        target_source
            .url
            .as_ref()
            .map(|s| ctx.serializer.to_json_ref(s)),
        target_source
            .title
            .as_ref()
            .map(|s| ctx.serializer.to_json_ref(s))
    ])
}

fn write_citation_mode(mode: &CitationMode) -> Value {
    match mode {
        CitationMode::NormalCitation => json!({"t": "NormalCitation"}),
        CitationMode::AuthorInText => json!({"t": "AuthorInText"}),
        CitationMode::SuppressAuthor => json!({"t": "SuppressAuthor"}),
    }
}

fn write_inline(inline: &Inline, ctx: &mut JsonWriterContext) -> Value {
    match inline {
        Inline::Str(s) => node_with_source(
            "Str",
            Some(json!(s.text)),
            &s.source_info,
            ctx,
        ),
        Inline::Space(space) => node_with_source(
            "Space",
            None,
            &space.source_info,
            ctx,
        ),
        Inline::LineBreak(lb) => node_with_source(
            "LineBreak",
            None,
            &lb.source_info,
            ctx,
        ),
        Inline::SoftBreak(sb) => node_with_source(
            "SoftBreak",
            None,
            &sb.source_info,
            ctx,
        ),
        Inline::Emph(e) => node_with_source(
            "Emph",
            Some(write_inlines(&e.content, ctx)),
            &e.source_info,
            ctx,
        ),
        Inline::Strong(s) => node_with_source(
            "Strong",
            Some(write_inlines(&s.content, ctx)),
            &s.source_info,
            ctx,
        ),
        Inline::Code(c) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Code"));
            obj.insert("c".to_string(), json!([write_attr(&c.attr), c.text]));
            ctx.serializer.add_source_info(&mut obj, &c.source_info);
            obj.insert("a".to_string(), write_attr_source(&c.attr_source, ctx));
            Value::Object(obj)
        }
        Inline::Math(m) => {
            let math_type = match m.math_type {
                crate::pandoc::MathType::InlineMath => json!({"t": "InlineMath"}),
                crate::pandoc::MathType::DisplayMath => json!({"t": "DisplayMath"}),
            };
            node_with_source(
                "Math",
                Some(json!([math_type, m.text])),
                &m.source_info,
            ctx,
            )
        }
        Inline::Underline(u) => node_with_source(
            "Underline",
            Some(write_inlines(&u.content, ctx)),
            &u.source_info,
            ctx,
        ),
        Inline::Strikeout(s) => node_with_source(
            "Strikeout",
            Some(write_inlines(&s.content, ctx)),
            &s.source_info,
            ctx,
        ),
        Inline::Superscript(s) => node_with_source(
            "Superscript",
            Some(write_inlines(&s.content, ctx)),
            &s.source_info,
            ctx,
        ),
        Inline::Subscript(s) => node_with_source(
            "Subscript",
            Some(write_inlines(&s.content, ctx)),
            &s.source_info,
            ctx,
        ),
        Inline::SmallCaps(s) => node_with_source(
            "SmallCaps",
            Some(write_inlines(&s.content, ctx)),
            &s.source_info,
            ctx,
        ),
        Inline::Quoted(q) => {
            let quote_type = match q.quote_type {
                crate::pandoc::QuoteType::SingleQuote => json!({"t": "SingleQuote"}),
                crate::pandoc::QuoteType::DoubleQuote => json!({"t": "DoubleQuote"}),
            };
            node_with_source(
                "Quoted",
                Some(json!([quote_type, write_inlines(&q.content, ctx)])),
                &q.source_info,
            ctx,
            )
        }
        Inline::Link(link) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Link"));
            obj.insert("c".to_string(), json!([
                write_attr(&link.attr),
                write_inlines(&link.content, ctx),
                [link.target.0, link.target.1]
            ]));
            ctx.serializer.add_source_info(&mut obj, &link.source_info);
            obj.insert("a".to_string(), write_attr_source(&link.attr_source, ctx));
            obj.insert("targetS".to_string(), write_target_source(&link.target_source, ctx));
            Value::Object(obj)
        }
        Inline::RawInline(raw) => node_with_source(
            "RawInline",
            Some(json!([raw.format.clone(), raw.text.clone()])),
            &raw.source_info,
            ctx,
        ),
        Inline::Image(image) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Image"));
            obj.insert("c".to_string(), json!([
                write_attr(&image.attr),
                write_inlines(&image.content, ctx),
                [image.target.0, image.target.1]
            ]));
            ctx.serializer.add_source_info(&mut obj, &image.source_info);
            obj.insert("a".to_string(), write_attr_source(&image.attr_source, ctx));
            obj.insert("targetS".to_string(), write_target_source(&image.target_source, ctx));
            Value::Object(obj)
        }
        Inline::Span(span) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Span"));
            obj.insert("c".to_string(), json!([
                write_attr(&span.attr),
                write_inlines(&span.content, ctx)
            ]));
            ctx.serializer.add_source_info(&mut obj, &span.source_info);
            obj.insert("a".to_string(), write_attr_source(&span.attr_source, ctx));
            Value::Object(obj)
        }
        Inline::Note(note) => node_with_source(
            "Note",
            Some(write_blocks(&note.content, ctx)),
            &note.source_info,
            ctx,
        ),
        // we can't test this just yet because
        // our citationNoteNum counter doesn't match Pandoc's
        Inline::Cite(cite) => node_with_source(
            "Cite",
            Some(json!([
                cite.citations.iter().map(|citation| {
                    json!({
                        "citationId": citation.id.clone(),
                        "citationPrefix": write_inlines(&citation.prefix, ctx),
                        "citationSuffix": write_inlines(&citation.suffix, ctx),
                        "citationMode": write_citation_mode(&citation.mode),
                        "citationHash": citation.hash,
                        "citationNoteNum": citation.note_num,
                        "citationIdS": citation.id_source.as_ref().map(|s| ctx.serializer.to_json_ref(s))
                    })
                }).collect::<Vec<_>>(),
                write_inlines(&cite.content, ctx)
            ])),
            &cite.source_info,
            ctx,
        ),
        Inline::Shortcode(shortcode) => {
            // Convert shortcode to span representation for JSON format output
            let span = shortcode_to_span(shortcode.clone());
            let attr = (span.attr.0.clone(), span.attr.1.clone(), span.attr.2.clone());
            node_with_source(
                "Span",
                Some(json!([write_attr(&attr), write_inlines(&span.content, ctx)])),
                &shortcode.source_info,
                ctx,
            )
        }
        Inline::NoteReference(note_ref) => {
            // Defensive: Should be converted to Span in postprocessing
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed note reference in JSON writer")
                    .with_code("Q-3-31")
                    .with_location(note_ref.source_info.clone())
                    .problem(format!("Note reference `[^{}]` was not converted during postprocessing", note_ref.id))
                    .add_detail("Note references should be processed before JSON output")
                    .add_hint("This may indicate a bug in the processing pipeline")
                    .build()
            );
            let mut attr_hash = LinkedHashMap::new();
            attr_hash.insert("data-ref".to_string(), note_ref.id.clone());
            let attr = (String::new(), vec!["footnote-ref".to_string()], attr_hash);
            node_with_source("Span", Some(json!([write_attr(&attr), []])), &note_ref.source_info, ctx)
        }
        Inline::Attr(inline_attr) => {
            // Defensive: Standalone attributes should not reach JSON writer
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Standalone attribute not supported in JSON format")
                    .with_code("Q-3-32")
                    .with_location(inline_attr.attr_source.id.clone().unwrap_or_default())
                    .problem("Cannot render standalone attributes in JSON format")
                    .add_detail("Standalone attributes should be attached to elements during parsing")
                    .add_hint("This may indicate a parsing issue or unsupported syntax")
                    .build()
            );
            json!({"t": "Str", "c": ""})  // Empty string placeholder
        }
        Inline::Insert(ins) => {
            // Defensive: Editorial marks should be desugared to Span
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed Insert markup in JSON writer")
                    .with_code("Q-3-33")
                    .with_location(ins.source_info.clone())
                    .problem("Insert markup `{++...++}` was not desugared during postprocessing")
                    .add_detail("CriticMarkup should be processed before JSON output")
                    .add_hint("Enable CriticMarkup processing or use a different output format")
                    .build()
            );
            let attr = (String::new(), vec!["critic-insert".to_string()], LinkedHashMap::new());
            node_with_source("Span", Some(json!([write_attr(&attr), write_inlines(&ins.content, ctx)])), &ins.source_info, ctx)
        }
        Inline::Delete(del) => {
            // Defensive: Editorial marks should be desugared to Span
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed Delete markup in JSON writer")
                    .with_code("Q-3-34")
                    .with_location(del.source_info.clone())
                    .problem("Delete markup `{--...--}` was not desugared during postprocessing")
                    .add_detail("CriticMarkup should be processed before JSON output")
                    .add_hint("Enable CriticMarkup processing or use a different output format")
                    .build()
            );
            let attr = (String::new(), vec!["critic-delete".to_string()], LinkedHashMap::new());
            node_with_source("Span", Some(json!([write_attr(&attr), write_inlines(&del.content, ctx)])), &del.source_info, ctx)
        }
        Inline::Highlight(hl) => {
            // Defensive: Editorial marks should be desugared to Span
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed Highlight markup in JSON writer")
                    .with_code("Q-3-35")
                    .with_location(hl.source_info.clone())
                    .problem("Highlight markup `{==...==}` was not desugared during postprocessing")
                    .add_detail("CriticMarkup should be processed before JSON output")
                    .add_hint("Enable CriticMarkup processing or use a different output format")
                    .build()
            );
            let attr = (String::new(), vec!["critic-highlight".to_string()], LinkedHashMap::new());
            node_with_source("Span", Some(json!([write_attr(&attr), write_inlines(&hl.content, ctx)])), &hl.source_info, ctx)
        }
        Inline::EditComment(ec) => {
            // Defensive: Editorial marks should be desugared to Span
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed EditComment markup in JSON writer")
                    .with_code("Q-3-36")
                    .with_location(ec.source_info.clone())
                    .problem("EditComment markup `{>>...<<}` was not desugared during postprocessing")
                    .add_detail("CriticMarkup should be processed before JSON output")
                    .add_hint("Enable CriticMarkup processing or use a different output format")
                    .build()
            );
            let attr = (String::new(), vec!["critic-comment".to_string()], LinkedHashMap::new());
            node_with_source("Span", Some(json!([write_attr(&attr), write_inlines(&ec.content, ctx)])), &ec.source_info, ctx)
        }
        Inline::Custom(custom) => {
            // Serialize CustomNode as wrapper Span with __quarto_custom_node class
            write_custom_inline(custom, ctx)
        }
    }
}

fn write_inlines(inlines: &Inlines, ctx: &mut JsonWriterContext) -> Value {
    json!(
        inlines
            .iter()
            .map(|inline| write_inline(inline, ctx))
            .collect::<Vec<_>>()
    )
}

fn write_list_attributes(attr: &ListAttributes) -> Value {
    let number_style = match attr.1 {
        crate::pandoc::ListNumberStyle::Decimal => json!({"t": "Decimal"}),
        crate::pandoc::ListNumberStyle::LowerAlpha => json!({"t": "LowerAlpha"}),
        crate::pandoc::ListNumberStyle::UpperAlpha => json!({"t": "UpperAlpha"}),
        crate::pandoc::ListNumberStyle::LowerRoman => json!({"t": "LowerRoman"}),
        crate::pandoc::ListNumberStyle::UpperRoman => json!({"t": "UpperRoman"}),
        crate::pandoc::ListNumberStyle::Example => json!({"t": "Example"}),
        crate::pandoc::ListNumberStyle::Default => json!({"t": "Default"}),
    };
    let number_delimiter = match attr.2 {
        crate::pandoc::ListNumberDelim::Period => json!({"t": "Period"}),
        crate::pandoc::ListNumberDelim::OneParen => json!({"t": "OneParen"}),
        crate::pandoc::ListNumberDelim::TwoParens => json!({"t": "TwoParens"}),
        crate::pandoc::ListNumberDelim::Default => json!({"t": "Default"}),
    };
    json!([attr.0, number_style, number_delimiter])
}

fn write_blockss(blockss: &[Vec<Block>], ctx: &mut JsonWriterContext) -> Value {
    json!(
        blockss
            .iter()
            .map(|blocks| blocks
                .iter()
                .map(|block| write_block(block, ctx))
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()
    )
}

// Write caption as Pandoc array format: [short, long]
fn write_caption(caption: &Caption, ctx: &mut JsonWriterContext) -> Value {
    json!([
        &caption.short.as_ref().map(|s| write_inlines(s, ctx)),
        &caption
            .long
            .as_ref()
            .map_or_else(|| json!([]), |l| write_blocks(l, ctx)),
    ])
}

// Write caption source info separately
fn write_caption_source(caption: &Caption, ctx: &mut JsonWriterContext) -> Value {
    json!(ctx.serializer.to_json_ref(&caption.source_info))
}

fn write_alignment(alignment: &crate::pandoc::table::Alignment) -> Value {
    match alignment {
        crate::pandoc::table::Alignment::Left => json!({"t": "AlignLeft"}),
        crate::pandoc::table::Alignment::Center => json!({"t": "AlignCenter"}),
        crate::pandoc::table::Alignment::Right => json!({"t": "AlignRight"}),
        crate::pandoc::table::Alignment::Default => json!({"t": "AlignDefault"}),
    }
}

fn write_colwidth(colwidth: &crate::pandoc::table::ColWidth) -> Value {
    match colwidth {
        crate::pandoc::table::ColWidth::Default => json!({"t": "ColWidthDefault"}),
        crate::pandoc::table::ColWidth::Percentage(p) => json!({"t": "ColWidth", "c": p}),
    }
}

fn write_colspec(colspec: &crate::pandoc::table::ColSpec) -> Value {
    json!([write_alignment(&colspec.0), write_colwidth(&colspec.1)])
}

// Write cell as Pandoc array format: [attr, alignment, rowSpan, colSpan, content]
fn write_cell(cell: &crate::pandoc::table::Cell, ctx: &mut JsonWriterContext) -> Value {
    json!([
        write_attr(&cell.attr),
        write_alignment(&cell.alignment),
        cell.row_span,
        cell.col_span,
        write_blocks(&cell.content, ctx)
    ])
}

// Write cell source info separately
fn write_cell_source(cell: &crate::pandoc::table::Cell, ctx: &mut JsonWriterContext) -> Value {
    json!({
        "s": ctx.serializer.to_json_ref(&cell.source_info),
        "a": write_attr_source(&cell.attr_source, ctx)
    })
}

// Write row as Pandoc array format: [attr, cells]
fn write_row(row: &crate::pandoc::table::Row, ctx: &mut JsonWriterContext) -> Value {
    json!([
        write_attr(&row.attr),
        row.cells
            .iter()
            .map(|cell| write_cell(cell, ctx))
            .collect::<Vec<_>>()
    ])
}

// Write row source info separately
fn write_row_source(row: &crate::pandoc::table::Row, ctx: &mut JsonWriterContext) -> Value {
    json!({
        "s": ctx.serializer.to_json_ref(&row.source_info),
        "a": write_attr_source(&row.attr_source, ctx),
        "cellsS": row.cells
            .iter()
            .map(|cell| write_cell_source(cell, ctx))
            .collect::<Vec<_>>()
    })
}

// Write table head as Pandoc array format: [attr, rows]
fn write_table_head(head: &crate::pandoc::table::TableHead, ctx: &mut JsonWriterContext) -> Value {
    json!([
        write_attr(&head.attr),
        head.rows
            .iter()
            .map(|row| write_row(row, ctx))
            .collect::<Vec<_>>()
    ])
}

// Write table head source info separately
fn write_table_head_source(
    head: &crate::pandoc::table::TableHead,
    ctx: &mut JsonWriterContext,
) -> Value {
    json!({
        "s": ctx.serializer.to_json_ref(&head.source_info),
        "a": write_attr_source(&head.attr_source, ctx),
        "rowsS": head.rows
            .iter()
            .map(|row| write_row_source(row, ctx))
            .collect::<Vec<_>>()
    })
}

// Write table body as Pandoc array format: [attr, rowHeadColumns, head, body]
fn write_table_body(body: &crate::pandoc::table::TableBody, ctx: &mut JsonWriterContext) -> Value {
    json!([
        write_attr(&body.attr),
        body.rowhead_columns,
        body.head
            .iter()
            .map(|row| write_row(row, ctx))
            .collect::<Vec<_>>(),
        body.body
            .iter()
            .map(|row| write_row(row, ctx))
            .collect::<Vec<_>>()
    ])
}

// Write table body source info separately
fn write_table_body_source(
    body: &crate::pandoc::table::TableBody,
    ctx: &mut JsonWriterContext,
) -> Value {
    json!({
        "s": ctx.serializer.to_json_ref(&body.source_info),
        "a": write_attr_source(&body.attr_source, ctx),
        "headS": body.head
            .iter()
            .map(|row| write_row_source(row, ctx))
            .collect::<Vec<_>>(),
        "bodyS": body.body
            .iter()
            .map(|row| write_row_source(row, ctx))
            .collect::<Vec<_>>()
    })
}

// Write table foot as Pandoc array format: [attr, rows]
fn write_table_foot(foot: &crate::pandoc::table::TableFoot, ctx: &mut JsonWriterContext) -> Value {
    json!([
        write_attr(&foot.attr),
        foot.rows
            .iter()
            .map(|row| write_row(row, ctx))
            .collect::<Vec<_>>()
    ])
}

// Write table foot source info separately
fn write_table_foot_source(
    foot: &crate::pandoc::table::TableFoot,
    ctx: &mut JsonWriterContext,
) -> Value {
    json!({
        "s": ctx.serializer.to_json_ref(&foot.source_info),
        "a": write_attr_source(&foot.attr_source, ctx),
        "rowsS": foot.rows
            .iter()
            .map(|row| write_row_source(row, ctx))
            .collect::<Vec<_>>()
    })
}

fn write_block(block: &Block, ctx: &mut JsonWriterContext) -> Value {
    match block {
        Block::Figure(figure) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Figure"));
            obj.insert(
                "c".to_string(),
                json!([
                    write_attr(&figure.attr),
                    write_caption(&figure.caption, ctx),
                    write_blocks(&figure.content, ctx)
                ]),
            );
            ctx.serializer
                .add_source_info(&mut obj, &figure.source_info);
            obj.insert("a".to_string(), write_attr_source(&figure.attr_source, ctx));
            // Plan 7f Phase 4: emit `captionS` so the strict reader can
            // recover the caption's source_info. Same shape as Table's
            // `captionS` sibling.
            obj.insert(
                "captionS".to_string(),
                write_caption_source(&figure.caption, ctx),
            );
            Value::Object(obj)
        }
        Block::DefinitionList(deflist) => node_with_source(
            "DefinitionList",
            Some(json!(
                deflist
                    .content
                    .iter()
                    .map(|(term, definition)| {
                        json!([write_inlines(term, ctx), write_blockss(definition, ctx),])
                    })
                    .collect::<Vec<_>>()
            )),
            &deflist.source_info,
            ctx,
        ),
        Block::OrderedList(orderedlist) => node_with_source(
            "OrderedList",
            Some(json!([
                write_list_attributes(&orderedlist.attr),
                write_blockss(&orderedlist.content, ctx),
            ])),
            &orderedlist.source_info,
            ctx,
        ),
        Block::RawBlock(raw) => node_with_source(
            "RawBlock",
            Some(json!([raw.format.clone(), raw.text.clone()])),
            &raw.source_info,
            ctx,
        ),
        Block::HorizontalRule(block) => {
            node_with_source("HorizontalRule", None, &block.source_info, ctx)
        }
        Block::Table(table) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Table"));
            obj.insert(
                "c".to_string(),
                json!([
                    write_attr(&table.attr),
                    write_caption(&table.caption, ctx),
                    table.colspec.iter().map(write_colspec).collect::<Vec<_>>(),
                    write_table_head(&table.head, ctx),
                    table
                        .bodies
                        .iter()
                        .map(|body| write_table_body(body, ctx))
                        .collect::<Vec<_>>(),
                    write_table_foot(&table.foot, ctx)
                ]),
            );
            ctx.serializer.add_source_info(&mut obj, &table.source_info);
            obj.insert("a".to_string(), write_attr_source(&table.attr_source, ctx));
            obj.insert(
                "captionS".to_string(),
                write_caption_source(&table.caption, ctx),
            );
            obj.insert(
                "headS".to_string(),
                write_table_head_source(&table.head, ctx),
            );
            obj.insert(
                "bodiesS".to_string(),
                json!(
                    table
                        .bodies
                        .iter()
                        .map(|body| write_table_body_source(body, ctx))
                        .collect::<Vec<_>>()
                ),
            );
            obj.insert(
                "footS".to_string(),
                write_table_foot_source(&table.foot, ctx),
            );
            Value::Object(obj)
        }

        Block::Div(div) => {
            // Insert fields in alphabetical order: a, c, s, t
            let mut obj = serde_json::Map::new();
            obj.insert("a".to_string(), write_attr_source(&div.attr_source, ctx));
            obj.insert(
                "c".to_string(),
                json!([write_attr(&div.attr), write_blocks(&div.content, ctx)]),
            );
            ctx.serializer.add_source_info(&mut obj, &div.source_info);
            obj.insert("t".to_string(), json!("Div"));
            Value::Object(obj)
        }
        Block::BlockQuote(quote) => node_with_source(
            "BlockQuote",
            Some(write_blocks(&quote.content, ctx)),
            &quote.source_info,
            ctx,
        ),
        Block::LineBlock(lineblock) => node_with_source(
            "LineBlock",
            Some(json!(
                lineblock
                    .content
                    .iter()
                    .map(|inlines| write_inlines(inlines, ctx))
                    .collect::<Vec<_>>()
            )),
            &lineblock.source_info,
            ctx,
        ),
        Block::Paragraph(para) => node_with_source(
            "Para",
            Some(write_inlines(&para.content, ctx)),
            &para.source_info,
            ctx,
        ),
        Block::Header(header) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Header"));
            obj.insert(
                "c".to_string(),
                json!([
                    header.level,
                    write_attr(&header.attr),
                    write_inlines(&header.content, ctx)
                ]),
            );
            ctx.serializer
                .add_source_info(&mut obj, &header.source_info);
            obj.insert("a".to_string(), write_attr_source(&header.attr_source, ctx));
            Value::Object(obj)
        }
        Block::CodeBlock(codeblock) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("CodeBlock"));
            obj.insert(
                "c".to_string(),
                json!([write_attr(&codeblock.attr), codeblock.text]),
            );
            ctx.serializer
                .add_source_info(&mut obj, &codeblock.source_info);
            obj.insert(
                "a".to_string(),
                write_attr_source(&codeblock.attr_source, ctx),
            );
            Value::Object(obj)
        }
        Block::Plain(plain) => node_with_source(
            "Plain",
            Some(write_inlines(&plain.content, ctx)),
            &plain.source_info,
            ctx,
        ),
        Block::BulletList(bulletlist) => node_with_source(
            "BulletList",
            Some(json!(
                bulletlist
                    .content
                    .iter()
                    .map(|blocks| blocks
                        .iter()
                        .map(|block| write_block(block, ctx))
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            )),
            &bulletlist.source_info,
            ctx,
        ),
        Block::BlockMetadata(meta) => {
            // Phase 5: Write ConfigValue directly without MetaValueWithSourceInfo conversion
            node_with_source(
                "BlockMetadata",
                Some(write_config_value(&meta.meta, ctx)),
                &meta.source_info,
                ctx,
            )
        }
        Block::NoteDefinitionPara(refdef) => node_with_source(
            "NoteDefinitionPara",
            Some(json!([refdef.id, write_inlines(&refdef.content, ctx)])),
            &refdef.source_info,
            ctx,
        ),
        Block::NoteDefinitionFencedBlock(refdef) => node_with_source(
            "NoteDefinitionFencedBlock",
            Some(json!([refdef.id, write_blocks(&refdef.content, ctx)])),
            &refdef.source_info,
            ctx,
        ),
        Block::CaptionBlock(caption) => {
            // Defensive: CaptionBlocks should be attached to figures/tables in postprocessing
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Orphaned caption block in JSON writer")
                    .with_code("Q-3-21")
                    .with_location(caption.source_info.clone())
                    .problem("Caption block is not attached to a figure or table")
                    .add_detail("Captions should be associated with figures/tables during postprocessing")
                    .add_hint("This may indicate a postprocessing issue or filter-generated orphaned caption")
                    .build()
            );
            // Render as a plain paragraph to avoid losing content
            node_with_source(
                "Plain",
                Some(write_inlines(&caption.content, ctx)),
                &caption.source_info,
                ctx,
            )
        }
        Block::Custom(custom) => {
            // Serialize CustomNode as wrapper Div with __quarto_custom_node class
            write_custom_block(custom, ctx)
        }
    }
}

/// Serialize a CustomNode as a wrapper Div with __quarto_custom_node class.
///
/// Format:
/// - Wrapper Div with class `__quarto_custom_node`
/// - Attribute `data-custom-type`: the type_name
/// - Attribute `data-custom-slots`: JSON mapping slot names to types
/// - Attribute `data-custom-data`: JSON-serialized plain_data
/// - Content: slot contents in order, each wrapped in a Div with `data-slot-name`
fn write_custom_block(custom: &crate::pandoc::CustomNode, ctx: &mut JsonWriterContext) -> Value {
    // Build the slot metadata (name -> type mapping)
    let slot_meta: serde_json::Map<String, Value> = custom
        .slots
        .iter()
        .map(|(name, slot)| {
            let slot_type = match slot {
                crate::pandoc::Slot::Block(_) => "Block",
                crate::pandoc::Slot::Inline(_) => "Inline",
                crate::pandoc::Slot::Blocks(_) => "Blocks",
                crate::pandoc::Slot::Inlines(_) => "Inlines",
            };
            (name.clone(), json!(slot_type))
        })
        .collect();

    // Start with the original attr's key-value pairs and add custom node attributes
    let mut wrapper_attr_kvs = custom.attr.2.clone();
    wrapper_attr_kvs.insert("data-custom-type".to_string(), custom.type_name.clone());
    wrapper_attr_kvs.insert(
        "data-custom-slots".to_string(),
        serde_json::to_string(&slot_meta).unwrap_or_else(|_| "{}".to_string()),
    );
    if !custom.plain_data.is_null() {
        wrapper_attr_kvs.insert(
            "data-custom-data".to_string(),
            serde_json::to_string(&custom.plain_data).unwrap_or_else(|_| "null".to_string()),
        );
    }

    // Start with the original attr and add the custom node class
    let mut classes = custom.attr.1.clone();
    classes.insert(0, "__quarto_custom_node".to_string());

    let wrapper_attr = (custom.attr.0.clone(), classes, wrapper_attr_kvs);

    // Build content: each slot wrapped in a Div with data-slot-name.
    //
    // The Plain and Div wrappers we synthesize here are wire-format
    // machinery — they have no source bytes of their own. They reference
    // the parent CustomNode's source_info so the strict reader (plan 7f
    // Phase 4) sees a valid `s:` on every wire-format node.
    let mut content: Vec<Value> = Vec::new();
    for (name, slot) in &custom.slots {
        let slot_content = match slot {
            crate::pandoc::Slot::Block(block) => {
                vec![write_block(block, ctx)]
            }
            crate::pandoc::Slot::Inline(inline) => {
                // Wrap single inline in a Plain block, carrying the parent's `s:`.
                let mut plain_obj = serde_json::Map::new();
                plain_obj.insert("t".to_string(), json!("Plain"));
                plain_obj.insert("c".to_string(), json!([write_inline(inline, ctx)]));
                ctx.serializer
                    .add_source_info(&mut plain_obj, &custom.source_info);
                vec![Value::Object(plain_obj)]
            }
            crate::pandoc::Slot::Blocks(blocks) => {
                blocks.iter().map(|b| write_block(b, ctx)).collect()
            }
            crate::pandoc::Slot::Inlines(inlines) => {
                // Wrap inlines in a Plain block, carrying the parent's `s:`.
                let mut plain_obj = serde_json::Map::new();
                plain_obj.insert("t".to_string(), json!("Plain"));
                plain_obj.insert("c".to_string(), json!(write_inlines(inlines, ctx)));
                ctx.serializer
                    .add_source_info(&mut plain_obj, &custom.source_info);
                vec![Value::Object(plain_obj)]
            }
        };

        // Each slot is wrapped in a Div with data-slot-name attribute.
        let mut slot_attr_kvs = LinkedHashMap::new();
        slot_attr_kvs.insert("data-slot-name".to_string(), name.clone());
        let slot_wrapper_attr = (String::new(), vec![], slot_attr_kvs);

        let mut slot_div = serde_json::Map::new();
        slot_div.insert("t".to_string(), json!("Div"));
        slot_div.insert(
            "c".to_string(),
            json!([write_attr(&slot_wrapper_attr), slot_content]),
        );
        ctx.serializer
            .add_source_info(&mut slot_div, &custom.source_info);
        content.push(Value::Object(slot_div));
    }

    let mut obj = serde_json::Map::new();
    obj.insert("t".to_string(), json!("Div"));
    obj.insert("c".to_string(), json!([write_attr(&wrapper_attr), content]));
    ctx.serializer
        .add_source_info(&mut obj, &custom.source_info);
    Value::Object(obj)
}

/// Serialize a CustomNode as a wrapper Span with __quarto_custom_node class.
///
/// Similar to write_custom_block but uses Span as the wrapper element.
fn write_custom_inline(custom: &crate::pandoc::CustomNode, ctx: &mut JsonWriterContext) -> Value {
    // Build the slot metadata (name -> type mapping)
    let slot_meta: serde_json::Map<String, Value> = custom
        .slots
        .iter()
        .map(|(name, slot)| {
            let slot_type = match slot {
                crate::pandoc::Slot::Block(_) => "Block",
                crate::pandoc::Slot::Inline(_) => "Inline",
                crate::pandoc::Slot::Blocks(_) => "Blocks",
                crate::pandoc::Slot::Inlines(_) => "Inlines",
            };
            (name.clone(), json!(slot_type))
        })
        .collect();

    // Start with the original attr's key-value pairs and add custom node attributes
    let mut wrapper_attr_kvs = custom.attr.2.clone();
    wrapper_attr_kvs.insert("data-custom-type".to_string(), custom.type_name.clone());
    wrapper_attr_kvs.insert(
        "data-custom-slots".to_string(),
        serde_json::to_string(&slot_meta).unwrap_or_else(|_| "{}".to_string()),
    );
    if !custom.plain_data.is_null() {
        wrapper_attr_kvs.insert(
            "data-custom-data".to_string(),
            serde_json::to_string(&custom.plain_data).unwrap_or_else(|_| "null".to_string()),
        );
    }

    // Start with the original attr and add the custom node class
    let mut classes = custom.attr.1.clone();
    classes.insert(0, "__quarto_custom_node".to_string());

    let wrapper_attr = (custom.attr.0.clone(), classes, wrapper_attr_kvs);

    // Build content: for inline custom nodes, slots contain inlines
    // Each slot wrapped in a Span with data-slot-name
    let mut content: Vec<Value> = Vec::new();
    for (name, slot) in &custom.slots {
        let slot_content = match slot {
            crate::pandoc::Slot::Inline(inline) => {
                vec![write_inline(inline, ctx)]
            }
            crate::pandoc::Slot::Inlines(inlines) => {
                inlines.iter().map(|i| write_inline(i, ctx)).collect()
            }
            crate::pandoc::Slot::Block(_) | crate::pandoc::Slot::Blocks(_) => {
                // Block slots in inline custom nodes shouldn't happen,
                // but we can emit a warning and render as placeholder
                ctx.errors.push(
                    DiagnosticMessageBuilder::error("Block slot in inline custom node")
                        .with_code("Q-3-39")
                        .with_location(custom.source_info.clone())
                        .problem(format!(
                            "Custom inline node `{}` has block-level slot `{}`",
                            custom.type_name, name
                        ))
                        .add_detail("Inline custom nodes should only have inline slots")
                        .build(),
                );
                {
                    let mut placeholder = serde_json::Map::new();
                    placeholder.insert("t".to_string(), json!("Str"));
                    placeholder.insert("c".to_string(), json!("[block content]"));
                    ctx.serializer
                        .add_source_info(&mut placeholder, &custom.source_info);
                    vec![Value::Object(placeholder)]
                }
            }
        };

        // Each slot is wrapped in a Span with data-slot-name attribute,
        // carrying the parent CustomNode's `s:` (wire-format machinery,
        // no source bytes of its own).
        let mut slot_attr_kvs = LinkedHashMap::new();
        slot_attr_kvs.insert("data-slot-name".to_string(), name.clone());
        let slot_wrapper_attr = (String::new(), vec![], slot_attr_kvs);

        let mut slot_span = serde_json::Map::new();
        slot_span.insert("t".to_string(), json!("Span"));
        slot_span.insert(
            "c".to_string(),
            json!([write_attr(&slot_wrapper_attr), slot_content]),
        );
        ctx.serializer
            .add_source_info(&mut slot_span, &custom.source_info);
        content.push(Value::Object(slot_span));
    }

    let mut obj = serde_json::Map::new();
    obj.insert("t".to_string(), json!("Span"));
    obj.insert("c".to_string(), json!([write_attr(&wrapper_attr), content]));
    ctx.serializer
        .add_source_info(&mut obj, &custom.source_info);
    Value::Object(obj)
}

/// Helper to create a meta value node with alphabetically ordered fields (c, s, t)
fn meta_node(t: &str, c: Value, s: Value) -> Value {
    serde_json::to_value(NodeJson {
        c: Some(c),
        s: 0, // placeholder, will be replaced
        t: t.to_string(),
    })
    .map(|mut v| {
        // Replace the placeholder 's' with actual value
        if let Value::Object(ref mut obj) = v {
            obj.insert("s".to_string(), s);
        }
        v
    })
    .unwrap()
}

/// Write a ConfigValue directly to JSON format with alphabetically ordered fields
fn write_config_value(value: &ConfigValue, ctx: &mut JsonWriterContext) -> Value {
    let s = ctx.serializer.to_json_ref(&value.source_info);
    match &value.value {
        ConfigValueKind::Scalar { yaml, .. } => match yaml {
            yaml_rust2::Yaml::String(str_val) => meta_node("MetaString", json!(str_val), s),
            yaml_rust2::Yaml::Boolean(b) => meta_node("MetaBool", json!(b), s),
            yaml_rust2::Yaml::Integer(i) => meta_node("MetaString", json!(i.to_string()), s),
            yaml_rust2::Yaml::Real(r) => meta_node("MetaString", json!(r), s),
            yaml_rust2::Yaml::Null => meta_node("MetaString", json!(""), s),
            _ => meta_node("MetaString", json!(""), s),
        },
        ConfigValueKind::PandocInlines(inlines) => {
            meta_node("MetaInlines", write_inlines(inlines, ctx), s)
        }
        ConfigValueKind::PandocBlocks(blocks) => {
            meta_node("MetaBlocks", write_blocks(blocks, ctx), s)
        }
        // Path/Glob/Expr: synthesize Inlines on the fly and serialize them normally.
        // The cloned SourceInfo inside the synthesized Str/Span will be interned as
        // a fresh pool entry, which is fine — the `SourceInfoSerializer` no longer
        // requires address-stable clones (see bd-h5l7).
        ConfigValueKind::Path(p) => {
            let inlines = build_path_inlines(p, &value.source_info);
            meta_node("MetaInlines", write_inlines(&inlines, ctx), s)
        }
        ConfigValueKind::Glob(g) => {
            let inlines = build_glob_inlines(g, &value.source_info);
            meta_node("MetaInlines", write_inlines(&inlines, ctx), s)
        }
        ConfigValueKind::Expr(e) => {
            let inlines = build_expr_inlines(e, &value.source_info);
            meta_node("MetaInlines", write_inlines(&inlines, ctx), s)
        }
        ConfigValueKind::Array(items) => {
            let c: Vec<Value> = items
                .iter()
                .map(|item| write_config_value(item, ctx))
                .collect();
            meta_node("MetaList", json!(c), s)
        }
        ConfigValueKind::Map(entries) => {
            let c: Vec<Value> = entries
                .iter()
                .map(|entry| {
                    // Map entries have alphabetical order: key, key_source, value
                    json!({
                        "key": entry.key,
                        "key_source": ctx.serializer.to_json_ref(&entry.key_source),
                        "value": write_config_value(&entry.value, ctx)
                    })
                })
                .collect();
            meta_node("MetaMap", json!(c), s)
        }
    }
}

/// Write ConfigValue as top-level metadata map with sorted keys
fn write_config_value_as_meta(meta: &ConfigValue, ctx: &mut JsonWriterContext) -> Value {
    match &meta.value {
        ConfigValueKind::Map(entries) => {
            // Sort entries by key for deterministic output
            let mut sorted: Vec<_> = entries
                .iter()
                .map(|entry| (entry.key.clone(), write_config_value(&entry.value, ctx)))
                .collect();
            sorted.sort_by(|(a, _), (b, _)| a.cmp(b));
            let map: serde_json::Map<String, Value> = sorted.into_iter().collect();
            Value::Object(map)
        }
        _ => {
            // Defensive: Pandoc.meta should always be Map
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Invalid metadata structure in JSON writer")
                    .with_code("Q-3-40")
                    .problem("Pandoc metadata is not a Map structure")
                    .add_hint("This may indicate a malformed AST or parsing error")
                    .build(),
            );
            Value::Object(serde_json::Map::new())
        }
    }
}

fn write_blocks(blocks: &[Block], ctx: &mut JsonWriterContext) -> Value {
    json!(
        blocks
            .iter()
            .map(|block| write_block(block, ctx))
            .collect::<Vec<_>>()
    )
}

/// Generate JSON representation of a Pandoc document.
///
/// This function is used internally by the HTML writer to build the source map.
/// Raw mode (`JsonConfig::raw`) is implemented only on the streaming path;
/// this function's callers never set it (asserted below).
pub(crate) fn write_pandoc(
    pandoc: &Pandoc,
    ast_context: &ASTContext,
    config: &JsonConfig,
) -> Result<Value, Vec<DiagnosticMessage>> {
    debug_assert!(
        !config.raw,
        "raw mode is only implemented on the streaming writer; use writers::raw_json"
    );
    // Create the JSON writer context
    let mut ctx = JsonWriterContext::new(ast_context, config);

    // Serialize AST, which will build the pool
    let meta_json = write_config_value_as_meta(&pandoc.meta, &mut ctx);
    let blocks_json = write_blocks(&pandoc.blocks, &mut ctx);

    // Check if any errors were accumulated
    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }

    // Extract top-level key sources from metadata using the serializer
    use quarto_pandoc_types::ConfigValueKind;
    let meta_top_level_key_sources: Option<Value> =
        if let ConfigValueKind::Map(ref entries) = pandoc.meta.value {
            // Sort entries by key for deterministic output
            let mut sorted_entries: Vec<_> = entries
                .iter()
                .map(|entry| {
                    (
                        entry.key.clone(),
                        ctx.serializer.to_json_ref(&entry.key_source),
                    )
                })
                .collect();
            sorted_entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            let map: serde_json::Map<String, Value> = sorted_entries.into_iter().collect();
            if map.is_empty() {
                None
            } else {
                Some(Value::Object(map))
            }
        } else {
            None
        };

    // Build file entries with alphabetically ordered fields
    let files: Vec<FileEntryJson> = (0..ast_context.filenames.len())
        .map(|idx| {
            let filename = &ast_context.filenames[idx];
            let file_info = ast_context
                .source_context
                .get_file(quarto_source_map::FileId(idx))
                .and_then(|file| file.file_info.as_ref());

            if let Some(info) = file_info {
                FileEntryJson {
                    line_breaks: Some(info.line_breaks().to_vec()),
                    name: filename.clone(),
                    total_length: Some(info.total_length()),
                }
            } else {
                FileEntryJson {
                    line_breaks: None,
                    name: filename.clone(),
                    total_length: None,
                }
            }
        })
        .collect();

    // Convert source info pool to SourceInfoJson for deterministic ordering
    let source_info_pool: Vec<SourceInfoJson> = ctx
        .serializer
        .pool
        .iter()
        .map(|info| info.to_json())
        .collect();

    // Build astContext with deterministic field ordering
    let ast_context_json = AstContextJson {
        files,
        meta_top_level_key_sources,
        source_info_pool,
    };

    // Build final document with deterministic field ordering
    let document = PandocDocumentJson {
        ast_context: ast_context_json,
        blocks: blocks_json.as_array().cloned().unwrap_or_default(),
        meta: meta_json,
        pandoc_api_version: [1, 23, 1],
    };

    Ok(serde_json::to_value(document).unwrap())
}

/// Write Pandoc AST to JSON with custom configuration.
///
/// Uses the streaming implementation (bd-wgup) that emits bytes directly
/// without building a `serde_json::Value` intermediate. The legacy
/// `write_pandoc(...) -> Value` function is retained for HTML writer
/// consumers that inspect the AST-as-Value for source-map construction.
pub fn write_with_config<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
    config: &JsonConfig,
) -> Result<(), Vec<DiagnosticMessage>> {
    let mut stream_writer = JsonStreamWriter::new(writer);
    stream_write_pandoc(&mut stream_writer, pandoc, context, config)
}

/// Write Pandoc AST to JSON with default configuration.
pub fn write<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
) -> Result<(), Vec<DiagnosticMessage>> {
    write_with_config(pandoc, context, writer, &JsonConfig::default())
}

/// Serialize inlines to a self-contained Pandoc-canonical JSON value with
/// **all source-location information dropped** (no `s` pool ids, no resolved
/// `l` locations, no `attrS` attribute-source sidecars).
///
/// This reuses the maintained [`write_inlines`] match logic and then strips
/// the source-tracking keys, so the result is the same `{"t":…,"c":…}` Pandoc
/// shape the hub-client wire format uses, minus the source noise. The
/// `context` is only consulted for location resolution (which is off here), so
/// any `ASTContext` — including [`ASTContext::default`] — is acceptable.
///
/// Used by `q2 get-config --output pandoc` (bd-xoaic, GH #256).
pub fn inlines_to_source_free_json(inlines: &Inlines, context: &ASTContext) -> Value {
    let config = JsonConfig::default();
    let mut ctx = JsonWriterContext::new(context, &config);
    strip_source_keys(write_inlines(inlines, &mut ctx))
}

/// Block-level counterpart of [`inlines_to_source_free_json`].
pub fn blocks_to_source_free_json(blocks: &[Block], context: &ASTContext) -> Value {
    let config = JsonConfig::default();
    let mut ctx = JsonWriterContext::new(context, &config);
    strip_source_keys(write_blocks(blocks, &mut ctx))
}

/// Recursively remove the source-tracking keys the JSON writer attaches to
/// every Pandoc node: `s` (source-info pool id), `l` (resolved location), and
/// `attrS` (attribute-source sidecar). Content lives under `c`/`t` and the
/// real attribute triple is inside `c`, so dropping these keys yields a
/// source-free Pandoc fragment without losing structure.
fn strip_source_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(k, _)| k != "s" && k != "l" && k != "attrS")
                .map(|(k, v)| (k, strip_source_keys(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(strip_source_keys).collect()),
        other => other,
    }
}

// =============================================================================
// Streaming implementation (bd-wgup)
//
// Emits JSON bytes directly via super::json_stream::JsonStreamWriter without
// materializing a serde_json::Value tree. This is the hub-client hot path
// (parse_qmd_to_ast -> pampa::writers::json::write_with_config); the legacy
// Value-returning functions above remain for HTML writer callers that still
// consume a Value for source-map construction.
//
// All object keys are emitted in alphabetical order (deterministic), which may
// differ from the legacy serializer's key order in edge cases but preserves
// structural JSON-value equality. See
// claude-notes/plans/2026-04-22-serde-json-value-intermediate.md.
// =============================================================================

use super::json_stream::JsonStreamWriter;
use std::io;

/// Intern `source_info` into the pool and emit its u64 id as the current value.
#[inline]
fn stream_source_ref<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    ctx: &mut JsonWriterContext,
    source_info: &SourceInfo,
) -> io::Result<()> {
    let id = ctx.serializer.intern(source_info);
    w.u64_value(id as u64)
}

/// Emit `null` if opt is None, otherwise intern the SourceInfo and emit its id.
#[inline]
fn stream_opt_source_ref<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    ctx: &mut JsonWriterContext,
    opt: Option<&SourceInfo>,
) -> io::Result<()> {
    match opt {
        Some(si) => stream_source_ref(w, ctx, si),
        None => w.null_value(),
    }
}

/// Emit the resolved location object `{b, e, f}` (each of b/e is `{c, l, o}`).
/// Returns Ok(true) if emitted, Ok(false) if the source info couldn't be mapped
/// (in which case the caller should not have emitted an "l" key).
fn stream_write_location<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    source_info: &SourceInfo,
    context: &ASTContext,
) -> io::Result<bool> {
    let Some((start_mapped, end_mapped)) =
        source_info.map_range(0, source_info.length(), &context.source_context)
    else {
        return Ok(false);
    };
    w.begin_object()?;
    w.key("b")?;
    w.begin_object()?;
    w.key("c")?;
    w.u64_value((start_mapped.location.column + 1) as u64)?;
    w.key("l")?;
    w.u64_value((start_mapped.location.row + 1) as u64)?;
    w.key("o")?;
    w.u64_value(start_mapped.location.offset as u64)?;
    w.end_object()?;
    w.key("e")?;
    w.begin_object()?;
    w.key("c")?;
    w.u64_value((end_mapped.location.column + 1) as u64)?;
    w.key("l")?;
    w.u64_value((end_mapped.location.row + 1) as u64)?;
    w.key("o")?;
    w.u64_value(end_mapped.location.offset as u64)?;
    w.end_object()?;
    w.key("f")?;
    w.u64_value(start_mapped.file_id.0 as u64)?;
    w.end_object()?;
    Ok(true)
}

/// Emit `attr` as `[id, [classes...], [[k, v]...]]`.
fn stream_write_attr<W: io::Write>(w: &mut JsonStreamWriter<W>, attr: &Attr) -> io::Result<()> {
    w.begin_array()?;
    w.str_value(&attr.0)?;
    w.begin_array()?;
    for cls in &attr.1 {
        w.str_value(cls)?;
    }
    w.end_array()?;
    w.begin_array()?;
    for (k, v) in &attr.2 {
        w.begin_array()?;
        w.str_value(k)?;
        w.str_value(v)?;
        w.end_array()?;
    }
    w.end_array()?;
    w.end_array()?;
    Ok(())
}

/// Emit AttrSourceInfo as `{classes: [id?...], id: id?, kvs: [[id?, id?]...]}`.
fn stream_write_attr_source<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    attr_source: &AttrSourceInfo,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_object()?;
    w.key("classes")?;
    w.begin_array()?;
    for cls in &attr_source.classes {
        stream_opt_source_ref(w, ctx, cls.as_ref())?;
    }
    w.end_array()?;
    w.key("id")?;
    stream_opt_source_ref(w, ctx, attr_source.id.as_ref())?;
    w.key("kvs")?;
    w.begin_array()?;
    for (k, v) in &attr_source.attributes {
        w.begin_array()?;
        stream_opt_source_ref(w, ctx, k.as_ref())?;
        stream_opt_source_ref(w, ctx, v.as_ref())?;
        w.end_array()?;
    }
    w.end_array()?;
    w.end_object()?;
    Ok(())
}

/// Emit TargetSourceInfo as `[url_id?, title_id?]`.
fn stream_write_target_source<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    target_source: &TargetSourceInfo,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    stream_opt_source_ref(w, ctx, target_source.url.as_ref())?;
    stream_opt_source_ref(w, ctx, target_source.title.as_ref())?;
    w.end_array()?;
    Ok(())
}

/// Emit a CitationMode tag as `{"t": "..."}`.
fn stream_write_citation_mode<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    mode: &CitationMode,
) -> io::Result<()> {
    w.begin_object()?;
    w.key("t")?;
    w.str_value(match mode {
        CitationMode::NormalCitation => "NormalCitation",
        CitationMode::AuthorInText => "AuthorInText",
        CitationMode::SuppressAuthor => "SuppressAuthor",
    })?;
    w.end_object()?;
    Ok(())
}

/// Emit ListAttributes as `[start, {"t": style}, {"t": delim}]`.
fn stream_write_list_attributes<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    attr: &ListAttributes,
) -> io::Result<()> {
    use crate::pandoc::{ListNumberDelim, ListNumberStyle};
    let style = match attr.1 {
        ListNumberStyle::Decimal => "Decimal",
        ListNumberStyle::LowerAlpha => "LowerAlpha",
        ListNumberStyle::UpperAlpha => "UpperAlpha",
        ListNumberStyle::LowerRoman => "LowerRoman",
        ListNumberStyle::UpperRoman => "UpperRoman",
        ListNumberStyle::Example => "Example",
        ListNumberStyle::Default => "Default",
    };
    let delim = match attr.2 {
        ListNumberDelim::Period => "Period",
        ListNumberDelim::OneParen => "OneParen",
        ListNumberDelim::TwoParens => "TwoParens",
        ListNumberDelim::Default => "Default",
    };
    w.begin_array()?;
    w.u64_value(attr.0 as u64)?;
    w.begin_object()?;
    w.key("t")?;
    w.str_value(style)?;
    w.end_object()?;
    w.begin_object()?;
    w.key("t")?;
    w.str_value(delim)?;
    w.end_object()?;
    w.end_array()?;
    Ok(())
}

fn stream_write_alignment<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    alignment: &crate::pandoc::table::Alignment,
) -> io::Result<()> {
    use crate::pandoc::table::Alignment;
    w.begin_object()?;
    w.key("t")?;
    w.str_value(match alignment {
        Alignment::Left => "AlignLeft",
        Alignment::Center => "AlignCenter",
        Alignment::Right => "AlignRight",
        Alignment::Default => "AlignDefault",
    })?;
    w.end_object()?;
    Ok(())
}

fn stream_write_colwidth<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    colwidth: &crate::pandoc::table::ColWidth,
) -> io::Result<()> {
    use crate::pandoc::table::ColWidth;
    w.begin_object()?;
    match colwidth {
        ColWidth::Default => {
            w.key("t")?;
            w.str_value("ColWidthDefault")?;
        }
        ColWidth::Percentage(p) => {
            w.key("c")?;
            w.f64_value(*p)?;
            w.key("t")?;
            w.str_value("ColWidth")?;
        }
    }
    w.end_object()?;
    Ok(())
}

fn stream_write_colspec<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    colspec: &crate::pandoc::table::ColSpec,
) -> io::Result<()> {
    w.begin_array()?;
    stream_write_alignment(w, &colspec.0)?;
    stream_write_colwidth(w, &colspec.1)?;
    w.end_array()?;
    Ok(())
}

/// Emit a simple node `{c, l?, s, t}` where `s` is the interned source id.
/// Alphabetical key order. `content` is invoked to emit the `c` value.
fn stream_write_simple_node<W: io::Write, F>(
    w: &mut JsonStreamWriter<W>,
    type_name: &str,
    source_info: &SourceInfo,
    ctx: &mut JsonWriterContext,
    content: F,
) -> io::Result<()>
where
    F: FnOnce(&mut JsonStreamWriter<W>, &mut JsonWriterContext) -> io::Result<()>,
{
    let s_id = ctx.serializer.intern(source_info);
    ctx.maybe_record_attribution_for(source_info, s_id);
    w.begin_object()?;
    w.key("c")?;
    content(w, ctx)?;
    if ctx.serializer.config.include_inline_locations {
        let ast_context = ctx.serializer.context;
        stream_write_location_key_if_mapped(w, "l", source_info, ast_context)?;
    }
    w.key("s")?;
    w.u64_value(s_id as u64)?;
    w.key("t")?;
    w.str_value(type_name)?;
    w.end_object()?;
    Ok(())
}

/// Emit a simple node without a `c` field: `{l?, s, t}`.
fn stream_write_simple_node_no_content<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    type_name: &str,
    source_info: &SourceInfo,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    let s_id = ctx.serializer.intern(source_info);
    ctx.maybe_record_attribution_for(source_info, s_id);
    w.begin_object()?;
    if ctx.serializer.config.include_inline_locations {
        let ast_context = ctx.serializer.context;
        stream_write_location_key_if_mapped(w, "l", source_info, ast_context)?;
    }
    w.key("s")?;
    w.u64_value(s_id as u64)?;
    w.key("t")?;
    w.str_value(type_name)?;
    w.end_object()?;
    Ok(())
}

/// If `source_info` can be mapped, emit `<key>: {b, e, f}` into the current object.
/// Returns whether the key was emitted. Caller must be inside an object.
fn stream_write_location_key_if_mapped<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    key: &str,
    source_info: &SourceInfo,
    context: &ASTContext,
) -> io::Result<bool> {
    let Some((start_mapped, end_mapped)) =
        source_info.map_range(0, source_info.length(), &context.source_context)
    else {
        return Ok(false);
    };
    w.key(key)?;
    w.begin_object()?;
    w.key("b")?;
    w.begin_object()?;
    w.key("c")?;
    w.u64_value((start_mapped.location.column + 1) as u64)?;
    w.key("l")?;
    w.u64_value((start_mapped.location.row + 1) as u64)?;
    w.key("o")?;
    w.u64_value(start_mapped.location.offset as u64)?;
    w.end_object()?;
    w.key("e")?;
    w.begin_object()?;
    w.key("c")?;
    w.u64_value((end_mapped.location.column + 1) as u64)?;
    w.key("l")?;
    w.u64_value((end_mapped.location.row + 1) as u64)?;
    w.key("o")?;
    w.u64_value(end_mapped.location.offset as u64)?;
    w.end_object()?;
    w.key("f")?;
    w.u64_value(start_mapped.file_id.0 as u64)?;
    w.end_object()?;
    Ok(true)
}

/// Emit a node with attr_source: `{a, c, l?, s, t}`. Alphabetical.
fn stream_write_attrs_node<W: io::Write, FC>(
    w: &mut JsonStreamWriter<W>,
    type_name: &str,
    source_info: &SourceInfo,
    attr_source: &AttrSourceInfo,
    ctx: &mut JsonWriterContext,
    content: FC,
) -> io::Result<()>
where
    FC: FnOnce(&mut JsonStreamWriter<W>, &mut JsonWriterContext) -> io::Result<()>,
{
    let s_id = ctx.serializer.intern(source_info);
    ctx.maybe_record_attribution_for(source_info, s_id);
    w.begin_object()?;
    w.key("a")?;
    stream_write_attr_source(w, attr_source, ctx)?;
    w.key("c")?;
    content(w, ctx)?;
    if ctx.serializer.config.include_inline_locations {
        let ast_context = ctx.serializer.context;
        stream_write_location_key_if_mapped(w, "l", source_info, ast_context)?;
    }
    w.key("s")?;
    w.u64_value(s_id as u64)?;
    w.key("t")?;
    w.str_value(type_name)?;
    w.end_object()?;
    Ok(())
}

/// Raw mode: emit a Shortcode's body object
/// `{isEscaped, keywordArgs, name, positionalArgs}` (alphabetical).
///
/// `keywordArgs` is an array of `[key, arg]` pairs in `LinkedHashMap`
/// insertion order (the qmd writer relies on that order for source-order
/// roundtrips; so do we).
fn stream_write_shortcode_body<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    shortcode: &quarto_pandoc_types::Shortcode,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_object()?;
    w.key("isEscaped")?;
    w.bool_value(shortcode.is_escaped)?;
    w.key("keywordArgs")?;
    w.begin_array()?;
    for (key, arg) in &shortcode.keyword_args {
        w.begin_array()?;
        w.str_value(key)?;
        stream_write_shortcode_arg(w, arg, ctx)?;
        w.end_array()?;
    }
    w.end_array()?;
    w.key("name")?;
    w.str_value(&shortcode.name)?;
    w.key("positionalArgs")?;
    w.begin_array()?;
    for arg in &shortcode.positional_args {
        stream_write_shortcode_arg(w, arg, ctx)?;
    }
    w.end_array()?;
    w.end_object()?;
    Ok(())
}

/// Raw mode: emit one ShortcodeArg as a tagged node.
///
/// `String`/`Number`/`Boolean` are `{c, t}`; a nested `Shortcode` is a
/// full `{c, s, t}` node (its own source info interned into the pool);
/// `KeyValue` is `{c: [[key, arg]...], t}` with entries sorted by key —
/// the underlying `HashMap` has no stable order, and sorting keeps the
/// output deterministic (map equality on read-back is order-independent).
fn stream_write_shortcode_arg<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    arg: &quarto_pandoc_types::ShortcodeArg,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    use quarto_pandoc_types::ShortcodeArg;
    match arg {
        ShortcodeArg::String(s) => {
            w.begin_object()?;
            w.key("c")?;
            w.str_value(s)?;
            w.key("t")?;
            w.str_value("String")?;
            w.end_object()?;
            Ok(())
        }
        ShortcodeArg::Number(n) => {
            w.begin_object()?;
            w.key("c")?;
            w.f64_value(*n)?;
            w.key("t")?;
            w.str_value("Number")?;
            w.end_object()?;
            Ok(())
        }
        ShortcodeArg::Boolean(b) => {
            w.begin_object()?;
            w.key("c")?;
            w.bool_value(*b)?;
            w.key("t")?;
            w.str_value("Boolean")?;
            w.end_object()?;
            Ok(())
        }
        ShortcodeArg::Shortcode(sc) => stream_write_simple_node(
            w,
            "Shortcode",
            &sc.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_shortcode_body(w, sc, ctx)
            },
        ),
        ShortcodeArg::KeyValue(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            w.begin_object()?;
            w.key("c")?;
            w.begin_array()?;
            for key in keys {
                w.begin_array()?;
                w.str_value(key)?;
                stream_write_shortcode_arg(w, &map[key], ctx)?;
                w.end_array()?;
            }
            w.end_array()?;
            w.key("t")?;
            w.str_value("KeyValue")?;
            w.end_object()?;
            Ok(())
        }
    }
}

/// Raw mode: emit a Span-shaped extension node `{a, c: [attr, inlines], s, t}`.
/// Used for the four CriticMarkup inlines, whose payload is exactly a
/// Span's (attr + inline content) under their own tag.
fn stream_write_span_like_raw<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    type_name: &str,
    attr: &Attr,
    content: &Vec<Inline>,
    source_info: &SourceInfo,
    attr_source: &AttrSourceInfo,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    stream_write_attrs_node(
        w,
        type_name,
        source_info,
        attr_source,
        ctx,
        |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
            w.begin_array()?;
            stream_write_attr(w, attr)?;
            stream_write_inlines(w, content, ctx)?;
            w.end_array()?;
            Ok(())
        },
    )
}

/// Emit an Inlines array: `[<inline>...]`.
fn stream_write_inlines<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    inlines: &Inlines,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    for inline in inlines {
        stream_write_inline(w, inline, ctx)?;
    }
    w.end_array()?;
    Ok(())
}

/// Emit a Blocks array: `[<block>...]`.
fn stream_write_blocks<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    blocks: &[Block],
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    for block in blocks {
        stream_write_block(w, block, ctx)?;
    }
    w.end_array()?;
    Ok(())
}

/// Emit a `[[Block]...]` (list of block groups — used by ordered/bullet/definition lists).
fn stream_write_blockss<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    blockss: &[Vec<Block>],
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    for blocks in blockss {
        stream_write_blocks(w, blocks, ctx)?;
    }
    w.end_array()?;
    Ok(())
}

// --- List-item block attrs (<li class>) — bd-aeyss6p5 --------------------
//
// A list item is a bare `[Block,…]` array with no object to hang an `attr` key
// on (unlike `Para`). So a per-item block attr (an authored `- item {.foo}`,
// which lands as a trailing `Inline::Attr` in the item's last block) is hoisted
// into a parallel sibling key `itemAttr` on the list node, mirroring the table
// `rowsS` precedent. `itemAttr` is parallel-indexed to the items array, each
// entry an `Attr` triple or `null`, and emitted only when some item has a
// non-empty attr (so ordinary lists are byte-for-byte unchanged).

/// For each item, collect its hoisted block attr and (when non-empty) a stripped
/// clone of its last block. See `block_attr::split_list_item_attr`.
fn list_item_attrs(items: &[Vec<Block>]) -> Vec<(Attr, Option<Block>)> {
    items
        .iter()
        .map(|item| super::block_attr::split_list_item_attr(item))
        .collect()
}

/// Whether any item carries a non-empty hoisted attr.
fn any_item_attr(strips: &[(Attr, Option<Block>)]) -> bool {
    strips.iter().any(|(attr, _)| !is_empty_attr(attr))
}

/// Emit the `[[Block]…]` items array, substituting each item's stripped last
/// block (trailing `Inline::Attr` removed) when one was hoisted — so the inner
/// `Para`/`Plain` writer never re-emits the attr.
fn stream_write_list_items<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    items: &[Vec<Block>],
    strips: &[(Attr, Option<Block>)],
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    for (item, (_attr, stripped)) in items.iter().zip(strips) {
        match stripped {
            Some(last) => {
                w.begin_array()?;
                for b in &item[..item.len() - 1] {
                    stream_write_block(w, b, ctx)?;
                }
                stream_write_block(w, last, ctx)?;
                w.end_array()?;
            }
            None => stream_write_blocks(w, item, ctx)?,
        }
    }
    w.end_array()?;
    Ok(())
}

/// Emit the `itemAttr` sibling key (parallel to the items array) when any item
/// carries a non-empty attr. Caller must be inside the list node's object and
/// place this in alphabetical key order (after `c`, before `l`/`s`/`t`).
fn stream_write_item_attr_key<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    strips: &[(Attr, Option<Block>)],
) -> io::Result<()> {
    if !any_item_attr(strips) {
        return Ok(());
    }
    w.key("itemAttr")?;
    w.begin_array()?;
    for (attr, _) in strips {
        if is_empty_attr(attr) {
            w.null_value()?;
        } else {
            stream_write_attr(w, attr)?;
        }
    }
    w.end_array()?;
    Ok(())
}

/// Emit Caption as `[short?, long]` where long is `[]` if missing.
fn stream_write_caption<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    caption: &Caption,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    match caption.short.as_ref() {
        Some(inlines) => stream_write_inlines(w, inlines, ctx)?,
        None => w.null_value()?,
    }
    match caption.long.as_ref() {
        Some(blocks) => stream_write_blocks(w, blocks, ctx)?,
        None => {
            w.begin_array()?;
            w.end_array()?;
        }
    }
    w.end_array()?;
    Ok(())
}

fn stream_write_caption_source<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    caption: &Caption,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    stream_source_ref(w, ctx, &caption.source_info)
}

fn stream_write_cell<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    cell: &crate::pandoc::table::Cell,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    stream_write_attr(w, &cell.attr)?;
    stream_write_alignment(w, &cell.alignment)?;
    w.u64_value(cell.row_span as u64)?;
    w.u64_value(cell.col_span as u64)?;
    stream_write_blocks(w, &cell.content, ctx)?;
    w.end_array()?;
    Ok(())
}

fn stream_write_cell_source<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    cell: &crate::pandoc::table::Cell,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_object()?;
    w.key("a")?;
    stream_write_attr_source(w, &cell.attr_source, ctx)?;
    w.key("s")?;
    stream_source_ref(w, ctx, &cell.source_info)?;
    w.end_object()?;
    Ok(())
}

fn stream_write_row<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    row: &crate::pandoc::table::Row,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    stream_write_attr(w, &row.attr)?;
    w.begin_array()?;
    for cell in &row.cells {
        stream_write_cell(w, cell, ctx)?;
    }
    w.end_array()?;
    w.end_array()?;
    Ok(())
}

fn stream_write_row_source<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    row: &crate::pandoc::table::Row,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_object()?;
    w.key("a")?;
    stream_write_attr_source(w, &row.attr_source, ctx)?;
    w.key("cellsS")?;
    w.begin_array()?;
    for cell in &row.cells {
        stream_write_cell_source(w, cell, ctx)?;
    }
    w.end_array()?;
    w.key("s")?;
    stream_source_ref(w, ctx, &row.source_info)?;
    w.end_object()?;
    Ok(())
}

fn stream_write_table_head<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    head: &crate::pandoc::table::TableHead,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    stream_write_attr(w, &head.attr)?;
    w.begin_array()?;
    for row in &head.rows {
        stream_write_row(w, row, ctx)?;
    }
    w.end_array()?;
    w.end_array()?;
    Ok(())
}

fn stream_write_table_head_source<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    head: &crate::pandoc::table::TableHead,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_object()?;
    w.key("a")?;
    stream_write_attr_source(w, &head.attr_source, ctx)?;
    w.key("rowsS")?;
    w.begin_array()?;
    for row in &head.rows {
        stream_write_row_source(w, row, ctx)?;
    }
    w.end_array()?;
    w.key("s")?;
    stream_source_ref(w, ctx, &head.source_info)?;
    w.end_object()?;
    Ok(())
}

fn stream_write_table_body<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    body: &crate::pandoc::table::TableBody,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    stream_write_attr(w, &body.attr)?;
    w.u64_value(body.rowhead_columns as u64)?;
    w.begin_array()?;
    for row in &body.head {
        stream_write_row(w, row, ctx)?;
    }
    w.end_array()?;
    w.begin_array()?;
    for row in &body.body {
        stream_write_row(w, row, ctx)?;
    }
    w.end_array()?;
    w.end_array()?;
    Ok(())
}

fn stream_write_table_body_source<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    body: &crate::pandoc::table::TableBody,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_object()?;
    w.key("a")?;
    stream_write_attr_source(w, &body.attr_source, ctx)?;
    w.key("bodyS")?;
    w.begin_array()?;
    for row in &body.body {
        stream_write_row_source(w, row, ctx)?;
    }
    w.end_array()?;
    w.key("headS")?;
    w.begin_array()?;
    for row in &body.head {
        stream_write_row_source(w, row, ctx)?;
    }
    w.end_array()?;
    w.key("s")?;
    stream_source_ref(w, ctx, &body.source_info)?;
    w.end_object()?;
    Ok(())
}

fn stream_write_table_foot<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    foot: &crate::pandoc::table::TableFoot,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    stream_write_attr(w, &foot.attr)?;
    w.begin_array()?;
    for row in &foot.rows {
        stream_write_row(w, row, ctx)?;
    }
    w.end_array()?;
    w.end_array()?;
    Ok(())
}

fn stream_write_table_foot_source<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    foot: &crate::pandoc::table::TableFoot,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    w.begin_object()?;
    w.key("a")?;
    stream_write_attr_source(w, &foot.attr_source, ctx)?;
    w.key("rowsS")?;
    w.begin_array()?;
    for row in &foot.rows {
        stream_write_row_source(w, row, ctx)?;
    }
    w.end_array()?;
    w.key("s")?;
    stream_source_ref(w, ctx, &foot.source_info)?;
    w.end_object()?;
    Ok(())
}

/// Emit an Inline node in compact JSON form.
fn stream_write_inline<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    inline: &Inline,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    match inline {
        Inline::Str(s) => stream_write_simple_node(w, "Str", &s.source_info, ctx, |w, _ctx| {
            w.str_value(&s.text)
        }),
        Inline::Space(space) => {
            stream_write_simple_node_no_content(w, "Space", &space.source_info, ctx)
        }
        Inline::LineBreak(lb) => {
            stream_write_simple_node_no_content(w, "LineBreak", &lb.source_info, ctx)
        }
        Inline::SoftBreak(sb) => {
            stream_write_simple_node_no_content(w, "SoftBreak", &sb.source_info, ctx)
        }
        Inline::Emph(e) => stream_write_simple_node(
            w,
            "Emph",
            &e.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_inlines(w, &e.content, ctx)
            },
        ),
        Inline::Strong(s) => stream_write_simple_node(
            w,
            "Strong",
            &s.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_inlines(w, &s.content, ctx)
            },
        ),
        Inline::Code(c) => {
            stream_write_attrs_node(w, "Code", &c.source_info, &c.attr_source, ctx, |w, _ctx| {
                w.begin_array()?;
                stream_write_attr(w, &c.attr)?;
                w.str_value(&c.text)?;
                w.end_array()?;
                Ok(())
            })
        }
        Inline::Math(m) => stream_write_simple_node(
            w,
            "Math",
            &m.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, _ctx: &mut JsonWriterContext| {
                use crate::pandoc::MathType;
                w.begin_array()?;
                w.begin_object()?;
                w.key("t")?;
                w.str_value(match m.math_type {
                    MathType::InlineMath => "InlineMath",
                    MathType::DisplayMath => "DisplayMath",
                })?;
                w.end_object()?;
                w.str_value(&m.text)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Inline::Underline(u) => stream_write_simple_node(
            w,
            "Underline",
            &u.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_inlines(w, &u.content, ctx)
            },
        ),
        Inline::Strikeout(s) => stream_write_simple_node(
            w,
            "Strikeout",
            &s.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_inlines(w, &s.content, ctx)
            },
        ),
        Inline::Superscript(s) => stream_write_simple_node(
            w,
            "Superscript",
            &s.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_inlines(w, &s.content, ctx)
            },
        ),
        Inline::Subscript(s) => stream_write_simple_node(
            w,
            "Subscript",
            &s.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_inlines(w, &s.content, ctx)
            },
        ),
        Inline::SmallCaps(s) => stream_write_simple_node(
            w,
            "SmallCaps",
            &s.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_inlines(w, &s.content, ctx)
            },
        ),
        Inline::Quoted(q) => stream_write_simple_node(
            w,
            "Quoted",
            &q.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                use crate::pandoc::QuoteType;
                w.begin_array()?;
                w.begin_object()?;
                w.key("t")?;
                w.str_value(match q.quote_type {
                    QuoteType::SingleQuote => "SingleQuote",
                    QuoteType::DoubleQuote => "DoubleQuote",
                })?;
                w.end_object()?;
                stream_write_inlines(w, &q.content, ctx)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Inline::Link(link) => {
            let s_id = ctx.serializer.intern(&link.source_info);
            ctx.maybe_record_attribution_for(&link.source_info, s_id);
            w.begin_object()?;
            w.key("a")?;
            stream_write_attr_source(w, &link.attr_source, ctx)?;
            w.key("c")?;
            w.begin_array()?;
            stream_write_attr(w, &link.attr)?;
            stream_write_inlines(w, &link.content, ctx)?;
            w.begin_array()?;
            w.str_value(&link.target.0)?;
            w.str_value(&link.target.1)?;
            w.end_array()?;
            w.end_array()?;
            if ctx.serializer.config.include_inline_locations {
                let ast_context = ctx.serializer.context;
                stream_write_location_key_if_mapped(w, "l", &link.source_info, ast_context)?;
            }
            w.key("s")?;
            w.u64_value(s_id as u64)?;
            w.key("t")?;
            w.str_value("Link")?;
            w.key("targetS")?;
            stream_write_target_source(w, &link.target_source, ctx)?;
            w.end_object()?;
            Ok(())
        }
        Inline::RawInline(raw) => stream_write_simple_node(
            w,
            "RawInline",
            &raw.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, _ctx: &mut JsonWriterContext| {
                w.begin_array()?;
                w.str_value(&raw.format)?;
                w.str_value(&raw.text)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Inline::Image(image) => {
            let s_id = ctx.serializer.intern(&image.source_info);
            ctx.maybe_record_attribution_for(&image.source_info, s_id);
            w.begin_object()?;
            w.key("a")?;
            stream_write_attr_source(w, &image.attr_source, ctx)?;
            w.key("c")?;
            w.begin_array()?;
            stream_write_attr(w, &image.attr)?;
            stream_write_inlines(w, &image.content, ctx)?;
            w.begin_array()?;
            w.str_value(&image.target.0)?;
            w.str_value(&image.target.1)?;
            w.end_array()?;
            w.end_array()?;
            if ctx.serializer.config.include_inline_locations {
                let ast_context = ctx.serializer.context;
                stream_write_location_key_if_mapped(w, "l", &image.source_info, ast_context)?;
            }
            w.key("s")?;
            w.u64_value(s_id as u64)?;
            w.key("t")?;
            w.str_value("Image")?;
            w.key("targetS")?;
            stream_write_target_source(w, &image.target_source, ctx)?;
            w.end_object()?;
            Ok(())
        }
        Inline::Span(span) => stream_write_attrs_node(
            w,
            "Span",
            &span.source_info,
            &span.attr_source,
            ctx,
            |w, ctx| {
                w.begin_array()?;
                stream_write_attr(w, &span.attr)?;
                stream_write_inlines(w, &span.content, ctx)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Inline::Note(note) => stream_write_simple_node(
            w,
            "Note",
            &note.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_blocks(w, &note.content, ctx)
            },
        ),
        Inline::Cite(cite) => stream_write_simple_node(
            w,
            "Cite",
            &cite.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                w.begin_array()?;
                w.begin_array()?;
                for citation in &cite.citations {
                    w.begin_object()?;
                    w.key("citationHash")?;
                    w.u64_value(citation.hash as u64)?;
                    w.key("citationId")?;
                    w.str_value(&citation.id)?;
                    w.key("citationIdS")?;
                    stream_opt_source_ref(w, ctx, citation.id_source.as_ref())?;
                    w.key("citationMode")?;
                    stream_write_citation_mode(w, &citation.mode)?;
                    w.key("citationNoteNum")?;
                    w.u64_value(citation.note_num as u64)?;
                    w.key("citationPrefix")?;
                    stream_write_inlines(w, &citation.prefix, ctx)?;
                    w.key("citationSuffix")?;
                    stream_write_inlines(w, &citation.suffix, ctx)?;
                    w.end_object()?;
                }
                w.end_array()?;
                stream_write_inlines(w, &cite.content, ctx)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Inline::Shortcode(shortcode) => {
            if ctx.serializer.config.raw {
                return stream_write_simple_node(
                    w,
                    "Shortcode",
                    &shortcode.source_info,
                    ctx,
                    |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                        stream_write_shortcode_body(w, shortcode, ctx)
                    },
                );
            }
            let span = shortcode_to_span(shortcode.clone());
            let attr = (
                span.attr.0.clone(),
                span.attr.1.clone(),
                span.attr.2.clone(),
            );
            stream_write_simple_node(
                w,
                "Span",
                &shortcode.source_info,
                ctx,
                |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                    w.begin_array()?;
                    stream_write_attr(w, &attr)?;
                    stream_write_inlines(w, &span.content, ctx)?;
                    w.end_array()?;
                    Ok(())
                },
            )
        }
        Inline::NoteReference(note_ref) => {
            if ctx.serializer.config.raw {
                return stream_write_simple_node(
                    w,
                    "NoteReference",
                    &note_ref.source_info,
                    ctx,
                    |w: &mut JsonStreamWriter<W>, _ctx: &mut JsonWriterContext| {
                        w.str_value(&note_ref.id)
                    },
                );
            }
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed note reference in JSON writer")
                    .with_code("Q-3-31")
                    .with_location(note_ref.source_info.clone())
                    .problem(format!(
                        "Note reference `[^{}]` was not converted during postprocessing",
                        note_ref.id
                    ))
                    .add_detail("Note references should be processed before JSON output")
                    .add_hint("This may indicate a bug in the processing pipeline")
                    .build(),
            );
            let mut attr_hash = LinkedHashMap::new();
            attr_hash.insert("data-ref".to_string(), note_ref.id.clone());
            let attr = (String::new(), vec!["footnote-ref".to_string()], attr_hash);
            stream_write_simple_node(
                w,
                "Span",
                &note_ref.source_info,
                ctx,
                |w: &mut JsonStreamWriter<W>, _ctx: &mut JsonWriterContext| {
                    w.begin_array()?;
                    stream_write_attr(w, &attr)?;
                    w.begin_array()?;
                    w.end_array()?;
                    w.end_array()?;
                    Ok(())
                },
            )
        }
        Inline::Attr(inline_attr) => {
            if ctx.serializer.config.raw {
                return stream_write_attrs_node(
                    w,
                    "Attr",
                    &inline_attr.source_info,
                    &inline_attr.attr_source,
                    ctx,
                    |w: &mut JsonStreamWriter<W>, _ctx: &mut JsonWriterContext| {
                        stream_write_attr(w, &inline_attr.attr)
                    },
                );
            }
            ctx.errors.push(
                DiagnosticMessageBuilder::error(
                    "Standalone attribute not supported in JSON format",
                )
                .with_code("Q-3-32")
                .with_location(inline_attr.attr_source.id.clone().unwrap_or_default())
                .problem("Cannot render standalone attributes in JSON format")
                .add_detail("Standalone attributes should be attached to elements during parsing")
                .add_hint("This may indicate a parsing issue or unsupported syntax")
                .build(),
            );
            // Placeholder: emit {"c": "", "t": "Str"} (matches the legacy fallback).
            w.begin_object()?;
            w.key("c")?;
            w.str_value("")?;
            w.key("t")?;
            w.str_value("Str")?;
            w.end_object()?;
            Ok(())
        }
        Inline::Insert(ins) => {
            if ctx.serializer.config.raw {
                return stream_write_span_like_raw(
                    w,
                    "Insert",
                    &ins.attr,
                    &ins.content,
                    &ins.source_info,
                    &ins.attr_source,
                    ctx,
                );
            }
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed Insert markup in JSON writer")
                    .with_code("Q-3-33")
                    .with_location(ins.source_info.clone())
                    .problem("Insert markup `{++...++}` was not desugared during postprocessing")
                    .add_detail("CriticMarkup should be processed before JSON output")
                    .add_hint("Enable CriticMarkup processing or use a different output format")
                    .build(),
            );
            let attr = (
                String::new(),
                vec!["critic-insert".to_string()],
                LinkedHashMap::new(),
            );
            stream_write_simple_node(
                w,
                "Span",
                &ins.source_info,
                ctx,
                |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                    w.begin_array()?;
                    stream_write_attr(w, &attr)?;
                    stream_write_inlines(w, &ins.content, ctx)?;
                    w.end_array()?;
                    Ok(())
                },
            )
        }
        Inline::Delete(del) => {
            if ctx.serializer.config.raw {
                return stream_write_span_like_raw(
                    w,
                    "Delete",
                    &del.attr,
                    &del.content,
                    &del.source_info,
                    &del.attr_source,
                    ctx,
                );
            }
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed Delete markup in JSON writer")
                    .with_code("Q-3-34")
                    .with_location(del.source_info.clone())
                    .problem("Delete markup `{--...--}` was not desugared during postprocessing")
                    .add_detail("CriticMarkup should be processed before JSON output")
                    .add_hint("Enable CriticMarkup processing or use a different output format")
                    .build(),
            );
            let attr = (
                String::new(),
                vec!["critic-delete".to_string()],
                LinkedHashMap::new(),
            );
            stream_write_simple_node(
                w,
                "Span",
                &del.source_info,
                ctx,
                |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                    w.begin_array()?;
                    stream_write_attr(w, &attr)?;
                    stream_write_inlines(w, &del.content, ctx)?;
                    w.end_array()?;
                    Ok(())
                },
            )
        }
        Inline::Highlight(hl) => {
            if ctx.serializer.config.raw {
                return stream_write_span_like_raw(
                    w,
                    "Highlight",
                    &hl.attr,
                    &hl.content,
                    &hl.source_info,
                    &hl.attr_source,
                    ctx,
                );
            }
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed Highlight markup in JSON writer")
                    .with_code("Q-3-35")
                    .with_location(hl.source_info.clone())
                    .problem("Highlight markup `{==...==}` was not desugared during postprocessing")
                    .add_detail("CriticMarkup should be processed before JSON output")
                    .add_hint("Enable CriticMarkup processing or use a different output format")
                    .build(),
            );
            let attr = (
                String::new(),
                vec!["critic-highlight".to_string()],
                LinkedHashMap::new(),
            );
            stream_write_simple_node(
                w,
                "Span",
                &hl.source_info,
                ctx,
                |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                    w.begin_array()?;
                    stream_write_attr(w, &attr)?;
                    stream_write_inlines(w, &hl.content, ctx)?;
                    w.end_array()?;
                    Ok(())
                },
            )
        }
        Inline::EditComment(ec) => {
            if ctx.serializer.config.raw {
                return stream_write_span_like_raw(
                    w,
                    "EditComment",
                    &ec.attr,
                    &ec.content,
                    &ec.source_info,
                    &ec.attr_source,
                    ctx,
                );
            }
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Unprocessed EditComment markup in JSON writer")
                    .with_code("Q-3-36")
                    .with_location(ec.source_info.clone())
                    .problem(
                        "EditComment markup `{>>...<<}` was not desugared during postprocessing",
                    )
                    .add_detail("CriticMarkup should be processed before JSON output")
                    .add_hint("Enable CriticMarkup processing or use a different output format")
                    .build(),
            );
            let attr = (
                String::new(),
                vec!["critic-comment".to_string()],
                LinkedHashMap::new(),
            );
            stream_write_simple_node(
                w,
                "Span",
                &ec.source_info,
                ctx,
                |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                    w.begin_array()?;
                    stream_write_attr(w, &attr)?;
                    stream_write_inlines(w, &ec.content, ctx)?;
                    w.end_array()?;
                    Ok(())
                },
            )
        }
        Inline::Custom(custom) => stream_write_custom_inline(w, custom, ctx),
    }
}

/// Emit a Block node in compact JSON form.
fn stream_write_block<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    block: &Block,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    match block {
        Block::Figure(figure) => {
            // Inlined (rather than via `stream_write_attrs_node`) so we can
            // emit `captionS` after `c` — wire-format key order is
            // alphabetical (a, c, captionS, s, t). Plan 7f Phase 4: the
            // strict reader needs `captionS` to recover the caption's
            // source_info.
            let s_id = ctx.serializer.intern(&figure.source_info);
            ctx.maybe_record_attribution_for(&figure.source_info, s_id);
            w.begin_object()?;
            w.key("a")?;
            stream_write_attr_source(w, &figure.attr_source, ctx)?;
            w.key("c")?;
            w.begin_array()?;
            stream_write_attr(w, &figure.attr)?;
            stream_write_caption(w, &figure.caption, ctx)?;
            stream_write_blocks(w, &figure.content, ctx)?;
            w.end_array()?;
            w.key("captionS")?;
            stream_write_caption_source(w, &figure.caption, ctx)?;
            if ctx.serializer.config.include_inline_locations {
                let ast_context = ctx.serializer.context;
                stream_write_location_key_if_mapped(w, "l", &figure.source_info, ast_context)?;
            }
            w.key("s")?;
            w.u64_value(s_id as u64)?;
            w.key("t")?;
            w.str_value("Figure")?;
            w.end_object()?;
            Ok(())
        }
        Block::DefinitionList(deflist) => stream_write_simple_node(
            w,
            "DefinitionList",
            &deflist.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                w.begin_array()?;
                for (term, definition) in &deflist.content {
                    w.begin_array()?;
                    stream_write_inlines(w, term, ctx)?;
                    stream_write_blockss(w, definition, ctx)?;
                    w.end_array()?;
                }
                w.end_array()?;
                Ok(())
            },
        ),
        Block::OrderedList(ol) => {
            let strips = list_item_attrs(&ol.content);
            if !any_item_attr(&strips) {
                // Fast path: ordinary list, byte-for-byte unchanged.
                stream_write_simple_node(
                    w,
                    "OrderedList",
                    &ol.source_info,
                    ctx,
                    |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                        w.begin_array()?;
                        stream_write_list_attributes(w, &ol.attr)?;
                        stream_write_blockss(w, &ol.content, ctx)?;
                        w.end_array()?;
                        Ok(())
                    },
                )
            } else {
                // Inlined for alphabetical wire order (c, itemAttr, l?, s, t).
                // `itemAttr` is parallel to the items array (`c[1]`).
                let s_id = ctx.serializer.intern(&ol.source_info);
                ctx.maybe_record_attribution_for(&ol.source_info, s_id);
                w.begin_object()?;
                w.key("c")?;
                w.begin_array()?;
                stream_write_list_attributes(w, &ol.attr)?;
                stream_write_list_items(w, &ol.content, &strips, ctx)?;
                w.end_array()?;
                stream_write_item_attr_key(w, &strips)?;
                if ctx.serializer.config.include_inline_locations {
                    let ast_context = ctx.serializer.context;
                    stream_write_location_key_if_mapped(w, "l", &ol.source_info, ast_context)?;
                }
                w.key("s")?;
                w.u64_value(s_id as u64)?;
                w.key("t")?;
                w.str_value("OrderedList")?;
                w.end_object()?;
                Ok(())
            }
        }
        Block::RawBlock(raw) => stream_write_simple_node(
            w,
            "RawBlock",
            &raw.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, _ctx: &mut JsonWriterContext| {
                w.begin_array()?;
                w.str_value(&raw.format)?;
                w.str_value(&raw.text)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Block::HorizontalRule(b) => {
            stream_write_simple_node_no_content(w, "HorizontalRule", &b.source_info, ctx)
        }
        Block::Table(table) => {
            let s_id = ctx.serializer.intern(&table.source_info);
            ctx.maybe_record_attribution_for(&table.source_info, s_id);
            w.begin_object()?;
            w.key("a")?;
            stream_write_attr_source(w, &table.attr_source, ctx)?;
            w.key("bodiesS")?;
            w.begin_array()?;
            for body in &table.bodies {
                stream_write_table_body_source(w, body, ctx)?;
            }
            w.end_array()?;
            w.key("c")?;
            w.begin_array()?;
            stream_write_attr(w, &table.attr)?;
            stream_write_caption(w, &table.caption, ctx)?;
            w.begin_array()?;
            for cs in &table.colspec {
                stream_write_colspec(w, cs)?;
            }
            w.end_array()?;
            stream_write_table_head(w, &table.head, ctx)?;
            w.begin_array()?;
            for body in &table.bodies {
                stream_write_table_body(w, body, ctx)?;
            }
            w.end_array()?;
            stream_write_table_foot(w, &table.foot, ctx)?;
            w.end_array()?;
            w.key("captionS")?;
            stream_write_caption_source(w, &table.caption, ctx)?;
            w.key("footS")?;
            stream_write_table_foot_source(w, &table.foot, ctx)?;
            w.key("headS")?;
            stream_write_table_head_source(w, &table.head, ctx)?;
            if ctx.serializer.config.include_inline_locations {
                let ast_context = ctx.serializer.context;
                stream_write_location_key_if_mapped(w, "l", &table.source_info, ast_context)?;
            }
            w.key("s")?;
            w.u64_value(s_id as u64)?;
            w.key("t")?;
            w.str_value("Table")?;
            w.end_object()?;
            Ok(())
        }
        Block::Div(div) => stream_write_attrs_node(
            w,
            "Div",
            &div.source_info,
            &div.attr_source,
            ctx,
            |w, ctx| {
                w.begin_array()?;
                stream_write_attr(w, &div.attr)?;
                stream_write_blocks(w, &div.content, ctx)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Block::BlockQuote(quote) => stream_write_simple_node(
            w,
            "BlockQuote",
            &quote.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_blocks(w, &quote.content, ctx)
            },
        ),
        Block::LineBlock(lineblock) => stream_write_simple_node(
            w,
            "LineBlock",
            &lineblock.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                w.begin_array()?;
                for inlines in &lineblock.content {
                    stream_write_inlines(w, inlines, ctx)?;
                }
                w.end_array()?;
                Ok(())
            },
        ),
        Block::Paragraph(p) => {
            // A `Paragraph` may carry a trailing standalone `Inline::Attr`
            // injected by a filter (e.g. `<p class="caption">`). Pandoc's `Para`
            // has no `Attr` field, so we collect that trailing run into a single
            // block attr and emit it as an extra `attr` object key — the same
            // "safe extra key" channel as `s`/`l`, which Pandoc ignores while
            // our React preview renderer reads it. See bd-itqcfxc3.
            let (content, attr) = super::block_attr::split_trailing_block_attr(&p.content);
            if is_empty_attr(&attr) {
                stream_write_simple_node(
                    w,
                    "Para",
                    &p.source_info,
                    ctx,
                    |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                        stream_write_inlines(w, &p.content, ctx)
                    },
                )
            } else {
                // Inlined so we can emit the extra `attr` key in alphabetical
                // wire order (attr, c, l?, s, t).
                let s_id = ctx.serializer.intern(&p.source_info);
                ctx.maybe_record_attribution_for(&p.source_info, s_id);
                w.begin_object()?;
                w.key("attr")?;
                stream_write_attr(w, &attr)?;
                w.key("c")?;
                w.begin_array()?;
                for inline in content {
                    stream_write_inline(w, inline, ctx)?;
                }
                w.end_array()?;
                if ctx.serializer.config.include_inline_locations {
                    let ast_context = ctx.serializer.context;
                    stream_write_location_key_if_mapped(w, "l", &p.source_info, ast_context)?;
                }
                w.key("s")?;
                w.u64_value(s_id as u64)?;
                w.key("t")?;
                w.str_value("Para")?;
                w.end_object()?;
                Ok(())
            }
        }
        Block::Header(h) => stream_write_attrs_node(
            w,
            "Header",
            &h.source_info,
            &h.attr_source,
            ctx,
            |w, ctx| {
                w.begin_array()?;
                w.u64_value(h.level as u64)?;
                stream_write_attr(w, &h.attr)?;
                stream_write_inlines(w, &h.content, ctx)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Block::CodeBlock(cb) => stream_write_attrs_node(
            w,
            "CodeBlock",
            &cb.source_info,
            &cb.attr_source,
            ctx,
            |w, _ctx| {
                w.begin_array()?;
                stream_write_attr(w, &cb.attr)?;
                w.str_value(&cb.text)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Block::Plain(plain) => stream_write_simple_node(
            w,
            "Plain",
            &plain.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_inlines(w, &plain.content, ctx)
            },
        ),
        Block::BulletList(bl) => {
            let strips = list_item_attrs(&bl.content);
            if !any_item_attr(&strips) {
                // Fast path: ordinary list, byte-for-byte unchanged.
                stream_write_simple_node(
                    w,
                    "BulletList",
                    &bl.source_info,
                    ctx,
                    |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                        stream_write_blockss(w, &bl.content, ctx)
                    },
                )
            } else {
                // Inlined so the extra `itemAttr` key lands in alphabetical wire
                // order (c, itemAttr, l?, s, t).
                let s_id = ctx.serializer.intern(&bl.source_info);
                ctx.maybe_record_attribution_for(&bl.source_info, s_id);
                w.begin_object()?;
                w.key("c")?;
                stream_write_list_items(w, &bl.content, &strips, ctx)?;
                stream_write_item_attr_key(w, &strips)?;
                if ctx.serializer.config.include_inline_locations {
                    let ast_context = ctx.serializer.context;
                    stream_write_location_key_if_mapped(w, "l", &bl.source_info, ast_context)?;
                }
                w.key("s")?;
                w.u64_value(s_id as u64)?;
                w.key("t")?;
                w.str_value("BulletList")?;
                w.end_object()?;
                Ok(())
            }
        }
        Block::BlockMetadata(meta) => stream_write_simple_node(
            w,
            "BlockMetadata",
            &meta.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                stream_write_config_value(w, &meta.meta, ctx)
            },
        ),
        Block::NoteDefinitionPara(refdef) => stream_write_simple_node(
            w,
            "NoteDefinitionPara",
            &refdef.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                w.begin_array()?;
                w.str_value(&refdef.id)?;
                stream_write_inlines(w, &refdef.content, ctx)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Block::NoteDefinitionFencedBlock(refdef) => stream_write_simple_node(
            w,
            "NoteDefinitionFencedBlock",
            &refdef.source_info,
            ctx,
            |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                w.begin_array()?;
                w.str_value(&refdef.id)?;
                stream_write_blocks(w, &refdef.content, ctx)?;
                w.end_array()?;
                Ok(())
            },
        ),
        Block::CaptionBlock(caption) => {
            if ctx.serializer.config.raw {
                return stream_write_simple_node(
                    w,
                    "CaptionBlock",
                    &caption.source_info,
                    ctx,
                    |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                        stream_write_inlines(w, &caption.content, ctx)
                    },
                );
            }
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Orphaned caption block in JSON writer")
                    .with_code("Q-3-21")
                    .with_location(caption.source_info.clone())
                    .problem("Caption block is not attached to a figure or table")
                    .add_detail(
                        "Captions should be associated with figures/tables during postprocessing",
                    )
                    .add_hint(
                        "This may indicate a postprocessing issue or filter-generated orphaned caption",
                    )
                    .build(),
            );
            stream_write_simple_node(
                w,
                "Plain",
                &caption.source_info,
                ctx,
                |w: &mut JsonStreamWriter<W>, ctx: &mut JsonWriterContext| {
                    stream_write_inlines(w, &caption.content, ctx)
                },
            )
        }
        Block::Custom(custom) => stream_write_custom_block(w, custom, ctx),
    }
}

/// Serialize a CustomNode as a wrapper Div with the __quarto_custom_node class.
fn stream_write_custom_block<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    custom: &crate::pandoc::CustomNode,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    // Build the slot metadata (name -> type mapping)
    let slot_meta: serde_json::Map<String, Value> = custom
        .slots
        .iter()
        .map(|(name, slot)| {
            let slot_type = match slot {
                crate::pandoc::Slot::Block(_) => "Block",
                crate::pandoc::Slot::Inline(_) => "Inline",
                crate::pandoc::Slot::Blocks(_) => "Blocks",
                crate::pandoc::Slot::Inlines(_) => "Inlines",
            };
            (name.clone(), json!(slot_type))
        })
        .collect();

    let mut wrapper_attr_kvs = custom.attr.2.clone();
    wrapper_attr_kvs.insert("data-custom-type".to_string(), custom.type_name.clone());
    wrapper_attr_kvs.insert(
        "data-custom-slots".to_string(),
        serde_json::to_string(&slot_meta).unwrap_or_else(|_| "{}".to_string()),
    );
    if !custom.plain_data.is_null() {
        wrapper_attr_kvs.insert(
            "data-custom-data".to_string(),
            serde_json::to_string(&custom.plain_data).unwrap_or_else(|_| "null".to_string()),
        );
    }

    let mut classes = custom.attr.1.clone();
    classes.insert(0, "__quarto_custom_node".to_string());

    let wrapper_attr = (custom.attr.0.clone(), classes, wrapper_attr_kvs);

    let s_id = ctx.serializer.intern(&custom.source_info);
    ctx.maybe_record_attribution_for(&custom.source_info, s_id);
    w.begin_object()?;
    w.key("c")?;
    w.begin_array()?;
    stream_write_attr(w, &wrapper_attr)?;
    // content: list of Div-wrapped slots. The Plain/Div wrappers we
    // synthesize here are wire-format machinery — they reference the
    // parent CustomNode's `s_id` so the strict reader (plan 7f Phase 4)
    // sees a valid `s:` on every node.
    w.begin_array()?;
    for (name, slot) in &custom.slots {
        let mut slot_attr_kvs = LinkedHashMap::new();
        slot_attr_kvs.insert("data-slot-name".to_string(), name.clone());
        let slot_wrapper_attr = (String::new(), vec![], slot_attr_kvs);
        w.begin_object()?;
        w.key("c")?;
        w.begin_array()?;
        stream_write_attr(w, &slot_wrapper_attr)?;
        // slot content
        w.begin_array()?;
        match slot {
            crate::pandoc::Slot::Block(block) => {
                stream_write_block(w, block, ctx)?;
            }
            crate::pandoc::Slot::Inline(inline) => {
                // Wrap in a Plain block: {"c": [<inline>], "s": s_id, "t": "Plain"}
                w.begin_object()?;
                w.key("c")?;
                w.begin_array()?;
                stream_write_inline(w, inline, ctx)?;
                w.end_array()?;
                w.key("s")?;
                w.u64_value(s_id as u64)?;
                w.key("t")?;
                w.str_value("Plain")?;
                w.end_object()?;
            }
            crate::pandoc::Slot::Blocks(blocks) => {
                for b in blocks {
                    stream_write_block(w, b, ctx)?;
                }
            }
            crate::pandoc::Slot::Inlines(inlines) => {
                // Wrap in a Plain block: {"c": [<inline>...], "s": s_id, "t": "Plain"}
                w.begin_object()?;
                w.key("c")?;
                stream_write_inlines(w, inlines, ctx)?;
                w.key("s")?;
                w.u64_value(s_id as u64)?;
                w.key("t")?;
                w.str_value("Plain")?;
                w.end_object()?;
            }
        }
        w.end_array()?;
        w.end_array()?;
        w.key("s")?;
        w.u64_value(s_id as u64)?;
        w.key("t")?;
        w.str_value("Div")?;
        w.end_object()?;
    }
    w.end_array()?;
    w.end_array()?;
    w.key("s")?;
    w.u64_value(s_id as u64)?;
    w.key("t")?;
    w.str_value("Div")?;
    w.end_object()?;
    Ok(())
}

fn stream_write_custom_inline<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    custom: &crate::pandoc::CustomNode,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    let slot_meta: serde_json::Map<String, Value> = custom
        .slots
        .iter()
        .map(|(name, slot)| {
            let slot_type = match slot {
                crate::pandoc::Slot::Block(_) => "Block",
                crate::pandoc::Slot::Inline(_) => "Inline",
                crate::pandoc::Slot::Blocks(_) => "Blocks",
                crate::pandoc::Slot::Inlines(_) => "Inlines",
            };
            (name.clone(), json!(slot_type))
        })
        .collect();

    let mut wrapper_attr_kvs = custom.attr.2.clone();
    wrapper_attr_kvs.insert("data-custom-type".to_string(), custom.type_name.clone());
    wrapper_attr_kvs.insert(
        "data-custom-slots".to_string(),
        serde_json::to_string(&slot_meta).unwrap_or_else(|_| "{}".to_string()),
    );
    if !custom.plain_data.is_null() {
        wrapper_attr_kvs.insert(
            "data-custom-data".to_string(),
            serde_json::to_string(&custom.plain_data).unwrap_or_else(|_| "null".to_string()),
        );
    }

    let mut classes = custom.attr.1.clone();
    classes.insert(0, "__quarto_custom_node".to_string());

    let wrapper_attr = (custom.attr.0.clone(), classes, wrapper_attr_kvs);

    let s_id = ctx.serializer.intern(&custom.source_info);
    ctx.maybe_record_attribution_for(&custom.source_info, s_id);
    w.begin_object()?;
    w.key("c")?;
    w.begin_array()?;
    stream_write_attr(w, &wrapper_attr)?;
    w.begin_array()?;
    for (name, slot) in &custom.slots {
        let mut slot_attr_kvs = LinkedHashMap::new();
        slot_attr_kvs.insert("data-slot-name".to_string(), name.clone());
        let slot_wrapper_attr = (String::new(), vec![], slot_attr_kvs);
        w.begin_object()?;
        w.key("c")?;
        w.begin_array()?;
        stream_write_attr(w, &slot_wrapper_attr)?;
        w.begin_array()?;
        match slot {
            crate::pandoc::Slot::Inline(inline) => {
                stream_write_inline(w, inline, ctx)?;
            }
            crate::pandoc::Slot::Inlines(inlines) => {
                for i in inlines {
                    stream_write_inline(w, i, ctx)?;
                }
            }
            crate::pandoc::Slot::Block(_) | crate::pandoc::Slot::Blocks(_) => {
                ctx.errors.push(
                    DiagnosticMessageBuilder::error("Block slot in inline custom node")
                        .with_code("Q-3-39")
                        .with_location(custom.source_info.clone())
                        .problem(format!(
                            "Custom inline node `{}` has block-level slot `{}`",
                            custom.type_name, name
                        ))
                        .add_detail("Inline custom nodes should only have inline slots")
                        .build(),
                );
                // Placeholder: {"c": "[block content]", "s": s_id, "t": "Str"}
                w.begin_object()?;
                w.key("c")?;
                w.str_value("[block content]")?;
                w.key("s")?;
                w.u64_value(s_id as u64)?;
                w.key("t")?;
                w.str_value("Str")?;
                w.end_object()?;
            }
        }
        w.end_array()?;
        w.end_array()?;
        w.key("s")?;
        w.u64_value(s_id as u64)?;
        w.key("t")?;
        w.str_value("Span")?;
        w.end_object()?;
    }
    w.end_array()?;
    w.end_array()?;
    w.key("s")?;
    w.u64_value(s_id as u64)?;
    w.key("t")?;
    w.str_value("Span")?;
    w.end_object()?;
    Ok(())
}

/// Emit a meta node `{c, s, t}` with alphabetical key ordering.
/// Emit a config-value node: `{c, m?, s, t}` (alphabetical).
///
/// `s` comes from the value's own `source_info`. In raw mode, a
/// non-default merge op is carried in `m` ("prefer"; the default,
/// "concat", is omitted to keep output lean). The Pandoc-superset mode
/// never emits `m` — merge ops are not representable there.
fn stream_write_meta_node<W: io::Write, FC>(
    w: &mut JsonStreamWriter<W>,
    type_name: &str,
    value: &ConfigValue,
    ctx: &mut JsonWriterContext,
    content: FC,
) -> io::Result<()>
where
    FC: FnOnce(&mut JsonStreamWriter<W>, &mut JsonWriterContext) -> io::Result<()>,
{
    let s_id = ctx.serializer.intern(&value.source_info);
    w.begin_object()?;
    w.key("c")?;
    content(w, ctx)?;
    if ctx.serializer.config.raw && value.merge_op != quarto_pandoc_types::MergeOp::default() {
        w.key("m")?;
        w.str_value(match value.merge_op {
            quarto_pandoc_types::MergeOp::Prefer => "prefer",
            quarto_pandoc_types::MergeOp::Concat => "concat",
        })?;
    }
    w.key("s")?;
    w.u64_value(s_id as u64)?;
    w.key("t")?;
    w.str_value(type_name)?;
    w.end_object()?;
    Ok(())
}

fn stream_write_config_value<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    value: &ConfigValue,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    let raw = ctx.serializer.config.raw;
    match &value.value {
        ConfigValueKind::Scalar { yaml, .. } => match yaml {
            yaml_rust2::Yaml::String(s) => {
                stream_write_meta_node(w, "MetaString", value, ctx, |w, _ctx| w.str_value(s))
            }
            yaml_rust2::Yaml::Boolean(b) => {
                stream_write_meta_node(w, "MetaBool", value, ctx, |w, _ctx| w.bool_value(*b))
            }
            yaml_rust2::Yaml::Integer(i) if raw => {
                stream_write_meta_node(w, "MetaInt", value, ctx, |w, _ctx| w.i64_value(*i))
            }
            yaml_rust2::Yaml::Integer(i) => {
                let text = i.to_string();
                stream_write_meta_node(w, "MetaString", value, ctx, |w, _ctx| w.str_value(&text))
            }
            // yaml_rust2 stores reals as their raw source string; carrying
            // the string keeps the raw roundtrip byte-faithful.
            yaml_rust2::Yaml::Real(r) if raw => {
                stream_write_meta_node(w, "MetaReal", value, ctx, |w, _ctx| w.str_value(r))
            }
            yaml_rust2::Yaml::Real(r) => {
                stream_write_meta_node(w, "MetaString", value, ctx, |w, _ctx| w.str_value(r))
            }
            yaml_rust2::Yaml::Null if raw => {
                stream_write_meta_node(w, "MetaNull", value, ctx, |w, _ctx| w.null_value())
            }
            yaml_rust2::Yaml::Null => {
                stream_write_meta_node(w, "MetaString", value, ctx, |w, _ctx| w.str_value(""))
            }
            other if raw => {
                // Raw mode must not silently degrade: a Scalar holding a
                // non-scalar YAML value has no faithful encoding, so fail
                // the write loudly.
                ctx.errors.push(
                    DiagnosticMessageBuilder::error(
                        "Unserializable YAML scalar in raw JSON writer",
                    )
                    .with_code("Q-3-57")
                    .with_location(value.source_info.clone())
                    .problem(format!(
                        "Scalar metadata holds a non-scalar YAML value ({:?}), which raw-json cannot represent faithfully",
                        std::mem::discriminant(other)
                    ))
                    .add_detail("Arrays and maps should use ConfigValueKind::Array / Map, not Scalar")
                    .add_hint("This may indicate a bug in metadata construction")
                    .build(),
                );
                stream_write_meta_node(w, "MetaNull", value, ctx, |w, _ctx| w.null_value())
            }
            _ => stream_write_meta_node(w, "MetaString", value, ctx, |w, _ctx| w.str_value("")),
        },
        ConfigValueKind::PandocInlines(inlines) => {
            stream_write_meta_node(w, "MetaInlines", value, ctx, |w, ctx| {
                stream_write_inlines(w, inlines, ctx)
            })
        }
        ConfigValueKind::PandocBlocks(blocks) => {
            stream_write_meta_node(w, "MetaBlocks", value, ctx, |w, ctx| {
                stream_write_blocks(w, blocks, ctx)
            })
        }
        ConfigValueKind::Path(p) if raw => {
            stream_write_meta_node(w, "MetaPath", value, ctx, |w, _ctx| w.str_value(p))
        }
        ConfigValueKind::Path(p) => {
            let inlines = build_path_inlines(p, &value.source_info);
            stream_write_meta_node(w, "MetaInlines", value, ctx, |w, ctx| {
                stream_write_inlines(w, &inlines, ctx)
            })
        }
        ConfigValueKind::Glob(g) if raw => {
            stream_write_meta_node(w, "MetaGlob", value, ctx, |w, _ctx| w.str_value(g))
        }
        ConfigValueKind::Glob(g) => {
            let inlines = build_glob_inlines(g, &value.source_info);
            stream_write_meta_node(w, "MetaInlines", value, ctx, |w, ctx| {
                stream_write_inlines(w, &inlines, ctx)
            })
        }
        ConfigValueKind::Expr(e) if raw => {
            stream_write_meta_node(w, "MetaExpr", value, ctx, |w, _ctx| w.str_value(e))
        }
        ConfigValueKind::Expr(e) => {
            let inlines = build_expr_inlines(e, &value.source_info);
            stream_write_meta_node(w, "MetaInlines", value, ctx, |w, ctx| {
                stream_write_inlines(w, &inlines, ctx)
            })
        }
        ConfigValueKind::Array(items) => {
            stream_write_meta_node(w, "MetaList", value, ctx, |w, ctx| {
                w.begin_array()?;
                for item in items {
                    stream_write_config_value(w, item, ctx)?;
                }
                w.end_array()?;
                Ok(())
            })
        }
        ConfigValueKind::Map(entries) => {
            stream_write_meta_node(w, "MetaMap", value, ctx, |w, ctx| {
                w.begin_array()?;
                for entry in entries {
                    w.begin_object()?;
                    w.key("key")?;
                    w.str_value(&entry.key)?;
                    w.key("key_source")?;
                    stream_source_ref(w, ctx, &entry.key_source)?;
                    w.key("value")?;
                    stream_write_config_value(w, &entry.value, ctx)?;
                    w.end_object()?;
                }
                w.end_array()?;
                Ok(())
            })
        }
    }
}

/// Emit the top-level meta map (sorted by key).
fn stream_write_config_value_as_meta<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    meta: &ConfigValue,
    ctx: &mut JsonWriterContext,
) -> io::Result<()> {
    match &meta.value {
        ConfigValueKind::Map(entries) => {
            // Sort by key for deterministic output.
            let mut sorted_indices: Vec<usize> = (0..entries.len()).collect();
            sorted_indices.sort_by(|&a, &b| entries[a].key.cmp(&entries[b].key));
            w.begin_object()?;
            for idx in sorted_indices {
                let entry = &entries[idx];
                w.key(&entry.key)?;
                stream_write_config_value(w, &entry.value, ctx)?;
            }
            w.end_object()?;
            Ok(())
        }
        _ => {
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Invalid metadata structure in JSON writer")
                    .with_code("Q-3-40")
                    .problem("Pandoc metadata is not a Map structure")
                    .add_hint("This may indicate a malformed AST or parsing error")
                    .build(),
            );
            w.begin_object()?;
            w.end_object()?;
            Ok(())
        }
    }
}

/// Emit the pool as the `p` (sourceInfoPool) array.
fn stream_write_source_info_pool<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    ctx: &JsonWriterContext,
) -> io::Result<()> {
    w.begin_array()?;
    for entry in &ctx.serializer.pool {
        // SourceInfoJson: {"d": ..., "r": [start, end], "t": type_code}
        w.begin_object()?;
        w.key("d")?;
        match &entry.mapping {
            SerializableSourceMapping::Original { file_id } => {
                w.u64_value(file_id.0 as u64)?;
            }
            SerializableSourceMapping::Substring { parent_id } => {
                w.u64_value(*parent_id as u64)?;
            }
            SerializableSourceMapping::Concat { pieces } => {
                w.begin_array()?;
                for piece in pieces {
                    w.begin_array()?;
                    w.u64_value(piece.source_info_id as u64)?;
                    w.u64_value(piece.offset_in_concat as u64)?;
                    w.u64_value(piece.length as u64)?;
                    w.end_array()?;
                }
                w.end_array()?;
            }
            SerializableSourceMapping::Generated { by, from } => {
                // Mirror SerializableSourceInfo::to_json byte-for-byte.
                // Object shape: { "by": { "kind": ..., "data": ... },
                //                 "from": [ { "role": ..., "si_id": N }, ... ] }
                // `data` is skipped when null; `from` is skipped when empty.
                w.begin_object()?;
                w.key("by")?;
                w.begin_object()?;
                w.key("kind")?;
                w.str_value(&by.kind)?;
                if !by.data.is_null() {
                    w.key("data")?;
                    stream_write_json_value(w, &by.data)?;
                }
                w.end_object()?;
                if !from.is_empty() {
                    w.key("from")?;
                    w.begin_array()?;
                    for (role, si_id) in from {
                        w.begin_object()?;
                        w.key("role")?;
                        w.str_value(&serialize_anchor_role(role))?;
                        w.key("si_id")?;
                        w.u64_value(*si_id as u64)?;
                        w.end_object()?;
                    }
                    w.end_array()?;
                }
                w.end_object()?;
            }
        }
        w.key("r")?;
        w.begin_array()?;
        w.u64_value(entry.start_offset as u64)?;
        w.u64_value(entry.end_offset as u64)?;
        w.end_array()?;
        w.key("t")?;
        w.u64_value(match &entry.mapping {
            SerializableSourceMapping::Original { .. } => 0,
            SerializableSourceMapping::Substring { .. } => 1,
            SerializableSourceMapping::Concat { .. } => 2,
            SerializableSourceMapping::Generated { .. } => 4,
        })?;
        w.end_object()?;
    }
    w.end_array()?;
    Ok(())
}

/// Recursively stream-write an arbitrary `serde_json::Value` via the
/// `JsonStreamWriter`. Used to emit the `By.data` payload inside a
/// `Generated` pool entry without materializing a serialized buffer.
fn stream_write_json_value<W: io::Write>(w: &mut JsonStreamWriter<W>, v: &Value) -> io::Result<()> {
    match v {
        Value::Null => w.null_value(),
        Value::Bool(b) => w.bool_value(*b),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                w.u64_value(u)
            } else if let Some(i) = n.as_i64() {
                w.i64_value(i)
            } else if let Some(f) = n.as_f64() {
                w.f64_value(f)
            } else {
                // Unreachable: serde_json::Number always converts to one of
                // the three numeric forms above. Emit null defensively.
                w.null_value()
            }
        }
        Value::String(s) => w.str_value(s),
        Value::Array(arr) => {
            w.begin_array()?;
            for item in arr {
                stream_write_json_value(w, item)?;
            }
            w.end_array()
        }
        Value::Object(obj) => {
            w.begin_object()?;
            for (k, val) in obj {
                w.key(k)?;
                stream_write_json_value(w, val)?;
            }
            w.end_object()
        }
    }
}

/// Emit the whole document. Streaming order:
/// `{blocks, meta, pandoc-api-version, astContext}` — alphabetical-friendly
/// except astContext last (it carries `p` (the sourceInfoPool) which is only
/// complete after we've walked `blocks` and `meta`). Object keys are unordered
/// in the JSON specification, so any consumer that does property lookup gets
/// the same data.
fn stream_write_pandoc<W: io::Write>(
    w: &mut JsonStreamWriter<W>,
    pandoc: &Pandoc,
    ast_context: &ASTContext,
    config: &JsonConfig,
) -> Result<(), Vec<DiagnosticMessage>> {
    let mut ctx = JsonWriterContext::new(ast_context, config);

    let res: io::Result<()> = (|| {
        w.begin_object()?;
        if config.raw {
            // The marker MUST be the first key: it is the format's
            // self-identification, visible in the first line of output.
            // `pandoc-api-version` is deliberately absent in raw mode so
            // Pandoc-JSON consumers fail fast instead of half-parsing.
            w.key("pampa-json-format")?;
            w.begin_object()?;
            w.key("version")?;
            w.u64_value(crate::writers::raw_json::RAW_JSON_FORMAT_VERSION)?;
            w.end_object()?;
        }
        w.key("blocks")?;
        stream_write_blocks(w, &pandoc.blocks, &mut ctx)?;
        w.key("meta")?;
        if config.raw {
            // Full-fidelity meta: a single config-value node (entry order,
            // key sources, merge ops, and the top-level value's own
            // source info all inline). The Pandoc-style sorted object
            // cannot preserve entry order on read (serde_json maps are
            // BTreeMaps), hence the array-of-entries encoding.
            stream_write_config_value(w, &pandoc.meta, &mut ctx)?;
        } else {
            stream_write_config_value_as_meta(w, &pandoc.meta, &mut ctx)?;
            w.key("pandoc-api-version")?;
            w.begin_array()?;
            w.u64_value(1)?;
            w.u64_value(23)?;
            w.u64_value(1)?;
            w.end_array()?;
        }
        w.key("astContext")?;
        w.begin_object()?;
        // files (alphabetical: files, metaTopLevelKeySources?, p)
        w.key("files")?;
        w.begin_array()?;
        for idx in 0..ast_context.filenames.len() {
            let filename = &ast_context.filenames[idx];
            let file_info = ast_context
                .source_context
                .get_file(quarto_source_map::FileId(idx))
                .and_then(|file| file.file_info.as_ref());
            w.begin_object()?;
            if let Some(info) = file_info {
                w.key("line_breaks")?;
                w.begin_array()?;
                for lb in info.line_breaks() {
                    w.u64_value(*lb as u64)?;
                }
                w.end_array()?;
            }
            w.key("name")?;
            w.str_value(filename)?;
            if let Some(info) = file_info {
                w.key("total_length")?;
                w.u64_value(info.total_length() as u64)?;
            }
            w.end_object()?;
        }
        w.end_array()?;

        // Roundtrip-relevant ASTContext state beyond the files table
        // (raw mode only): the example-list counter.
        if config.raw {
            w.key("exampleListCounter")?;
            w.u64_value(ast_context.example_list_counter.get() as u64)?;
        }

        // metaTopLevelKeySources (only if non-empty; not in raw mode,
        // where key sources ride inline in the meta entry encoding)
        if !config.raw
            && let ConfigValueKind::Map(entries) = &pandoc.meta.value
            && !entries.is_empty()
        {
            w.key("metaTopLevelKeySources")?;
            let mut sorted_indices: Vec<usize> = (0..entries.len()).collect();
            sorted_indices.sort_by(|&a, &b| entries[a].key.cmp(&entries[b].key));
            w.begin_object()?;
            for idx in sorted_indices {
                let entry = &entries[idx];
                w.key(&entry.key)?;
                stream_source_ref(w, &mut ctx, &entry.key_source)?;
            }
            w.end_object()?;
        }

        // p (sourceInfoPool)
        if !ctx.serializer.pool.is_empty() {
            w.key("p")?;
            stream_write_source_info_pool(w, &ctx)?;
        }

        // attribution (Phase 5 — q2-debug wire shape).
        //
        // Records accumulated during the AST walk via
        // `ctx.maybe_record_attribution_for`. Emitted only when the
        // attribution-render transform produced a non-empty
        // `JsonConfig::attribution_by_node` (off-path keys are absent,
        // making the JSON byte-identical to today's output — see Phase 0
        // test #10).
        if !ctx.attribution_records.is_empty() {
            w.key("attribution")?;
            w.begin_array()?;
            for rec in &ctx.attribution_records {
                w.begin_object()?;
                w.key("actor")?;
                w.str_value(&rec.actor)?;
                w.key("s")?;
                w.u64_value(rec.s as u64)?;
                w.key("time")?;
                w.i64_value(rec.time)?;
                w.end_object()?;
            }
            w.end_array()?;
        }

        // attributionActors (actor → { name, color }). Pre-pruned by
        // the render transform; we emit verbatim, sorted by actor key
        // for deterministic output.
        if let Some(actors) = ctx.serializer.config.attribution_actors.as_ref()
            && !actors.is_empty()
        {
            let mut keys: Vec<&Arc<str>> = actors.keys().collect();
            keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
            w.key("attributionActors")?;
            w.begin_object()?;
            for k in keys {
                let v = &actors[k];
                w.key(k.as_ref())?;
                w.begin_object()?;
                w.key("color")?;
                w.str_value(&v.color)?;
                w.key("name")?;
                w.str_value(&v.display_name)?;
                w.end_object()?;
            }
            w.end_object()?;
        }

        w.end_object()?; // astContext
        w.end_object()?; // top-level
        Ok(())
    })();

    // Map io::Error into a DiagnosticMessage
    if let Err(e) = res {
        ctx.errors.push(DiagnosticMessage {
            code: Some("Q-3-38".to_string()),
            title: "JSON serialization failed".to_string(),
            kind: quarto_error_reporting::DiagnosticKind::Error,
            problem: Some(format!("Failed to write AST JSON: {}", e).into()),
            details: vec![],
            hints: vec![],
            location: None,
        });
    }

    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{Anchor, AnchorRole, By, FileId, SourceInfo};
    use smallvec::SmallVec;
    use std::sync::Arc;

    fn make_test_context() -> ASTContext {
        ASTContext::anonymous()
    }

    fn make_test_config() -> JsonConfig {
        JsonConfig::default()
    }

    #[test]
    fn test_source_info_pool_original() {
        // Test that a single Original SourceInfo is added to the pool correctly.
        //
        // Post-bd-h5l7: each intern call allocates a fresh pool entry. Only
        // `Substring` parent Arcs get deduped (see `test_source_info_pool_deduplication`).
        // Two by-value interns of the same SourceInfo now produce two separate
        // pool IDs — pool-ID equality no longer implies structural equality. Consumers
        // that need "same source range?" should resolve both IDs through the pool and
        // compare the resulting SourceInfo values structurally.
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let source_info = SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 10,
        };

        let id = serializer.intern(&source_info);

        // Pool starts at slot 0; the first intern
        // lands at slot 1.
        let first_user_id = 0;
        assert_eq!(id, first_user_id);
        assert_eq!(serializer.pool.len(), first_user_id + 1);

        // Verify the pool entry
        let entry = &serializer.pool[first_user_id];
        assert_eq!(entry.start_offset, 0);
        assert_eq!(entry.end_offset, 10);
        match &entry.mapping {
            SerializableSourceMapping::Original { file_id } => {
                assert_eq!(*file_id, FileId(0));
            }
            _ => panic!("Expected Original mapping"),
        }

        // Interning the same SourceInfo again produces a fresh pool entry.
        let id2 = serializer.intern(&source_info);
        assert_eq!(id2, first_user_id + 1);
        assert_eq!(serializer.pool.len(), first_user_id + 2);

        // Both entries resolve to structurally-equal pool values.
        assert_eq!(
            serializer.pool[first_user_id].start_offset,
            serializer.pool[first_user_id + 1].start_offset
        );
        assert_eq!(
            serializer.pool[first_user_id].end_offset,
            serializer.pool[first_user_id + 1].end_offset
        );
        match (
            &serializer.pool[first_user_id].mapping,
            &serializer.pool[first_user_id + 1].mapping,
        ) {
            (
                SerializableSourceMapping::Original { file_id: a },
                SerializableSourceMapping::Original { file_id: b },
            ) => assert_eq!(a, b),
            _ => panic!("Expected both to be Original"),
        }
    }

    #[test]
    fn test_source_info_pool_substring() {
        // Test Substring with parent reference
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let parent = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 100,
        });

        let child = SourceInfo::Substring {
            parent: Arc::clone(&parent),
            start_offset: 10,
            end_offset: 20,
        };

        let child_id = serializer.intern(&child);

        // Parent is interned
        // first at slot 1, child second at slot 2.
        let parent_id = 0;
        assert_eq!(child_id, parent_id + 1);
        assert_eq!(serializer.pool.len(), parent_id + 2);

        // Verify parent entry
        let parent_entry = &serializer.pool[parent_id];
        assert_eq!(parent_entry.start_offset, 0);
        assert_eq!(parent_entry.end_offset, 100);
        match &parent_entry.mapping {
            SerializableSourceMapping::Original { file_id } => {
                assert_eq!(*file_id, FileId(0));
            }
            _ => panic!("Expected Original mapping"),
        }

        // Verify child entry
        let child_entry = &serializer.pool[parent_id + 1];
        assert_eq!(child_entry.start_offset, 10);
        assert_eq!(child_entry.end_offset, 20);
        match &child_entry.mapping {
            SerializableSourceMapping::Substring {
                parent_id: parent_ref,
            } => {
                assert_eq!(*parent_ref, parent_id); // References parent
            }
            _ => panic!("Expected Substring mapping"),
        }
    }

    #[test]
    fn test_source_info_pool_siblings() {
        // Test multiple nodes sharing the same parent
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let parent = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 100,
        });

        let child1 = SourceInfo::Substring {
            parent: Arc::clone(&parent),
            start_offset: 10,
            end_offset: 20,
        };

        let child2 = SourceInfo::Substring {
            parent: Arc::clone(&parent),
            start_offset: 30,
            end_offset: 40,
        };

        let child3 = SourceInfo::Substring {
            parent: Arc::clone(&parent),
            start_offset: 50,
            end_offset: 60,
        };

        let id1 = serializer.intern(&child1);
        let id2 = serializer.intern(&child2);
        let id3 = serializer.intern(&child3);

        // Parent at slot 0, children at slots 1/2/3.
        let parent_id = 0;
        assert_eq!(id1, parent_id + 1);
        assert_eq!(id2, parent_id + 2);
        assert_eq!(id3, parent_id + 3);
        assert_eq!(serializer.pool.len(), parent_id + 4); // reserved + parent + 3 children

        // All children should reference the same parent
        for child_id in [id1, id2, id3] {
            let child_entry = &serializer.pool[child_id];
            match &child_entry.mapping {
                SerializableSourceMapping::Substring {
                    parent_id: parent_ref,
                } => {
                    assert_eq!(*parent_ref, parent_id);
                }
                _ => panic!("Expected Substring mapping"),
            }
        }
    }

    #[test]
    fn test_source_info_pool_nested_deep() {
        // Test deeply nested structure (5+ levels)
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        // Build a chain: Original -> Sub1 -> Sub2 -> Sub3 -> Sub4 -> Sub5
        let level0 = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 1000,
        });

        let level1 = Arc::new(SourceInfo::Substring {
            parent: Arc::clone(&level0),
            start_offset: 100,
            end_offset: 900,
        });

        let level2 = Arc::new(SourceInfo::Substring {
            parent: Arc::clone(&level1),
            start_offset: 200,
            end_offset: 800,
        });

        let level3 = Arc::new(SourceInfo::Substring {
            parent: Arc::clone(&level2),
            start_offset: 300,
            end_offset: 700,
        });

        let level4 = Arc::new(SourceInfo::Substring {
            parent: Arc::clone(&level3),
            start_offset: 400,
            end_offset: 600,
        });

        let level5 = SourceInfo::Substring {
            parent: Arc::clone(&level4),
            start_offset: 450,
            end_offset: 550,
        };

        let deepest_id = serializer.intern(&level5);

        // level0..level5 interned at slots 0..5.
        let level0_id = 0;
        assert_eq!(deepest_id, level0_id + 5);
        assert_eq!(serializer.pool.len(), level0_id + 6);

        // Verify the chain: each level should reference its parent
        for offset in 1..=5 {
            let i = level0_id + offset;
            let entry = &serializer.pool[i];
            match &entry.mapping {
                SerializableSourceMapping::Substring { parent_id } => {
                    assert_eq!(
                        *parent_id,
                        i - 1,
                        "Level {} should reference parent {}",
                        i,
                        i - 1
                    );
                }
                _ => panic!("Expected Substring mapping at level {}", i),
            }
        }
    }

    #[test]
    fn test_source_info_pool_concat() {
        // Test Concat mapping with multiple pieces
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let piece1_source = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 10,
        });

        let piece2_source = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 20,
            end_offset: 30,
        });

        let concat = SourceInfo::Concat {
            pieces: vec![
                quarto_source_map::SourcePiece {
                    source_info: (*piece1_source).clone(),
                    offset_in_concat: 0,
                    length: 10,
                },
                quarto_source_map::SourcePiece {
                    source_info: (*piece2_source).clone(),
                    offset_in_concat: 10,
                    length: 10,
                },
            ],
        };

        let concat_id = serializer.intern(&concat);

        // piece1, piece2, concat interned at slots 0, 1, 2.
        let piece1_id = 0;
        assert_eq!(concat_id, piece1_id + 2);
        assert_eq!(serializer.pool.len(), piece1_id + 3);

        // Verify concat entry
        let concat_entry = &serializer.pool[concat_id];
        match &concat_entry.mapping {
            SerializableSourceMapping::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[0].source_info_id, piece1_id); // References piece1
                assert_eq!(pieces[0].offset_in_concat, 0);
                assert_eq!(pieces[0].length, 10);
                assert_eq!(pieces[1].source_info_id, piece1_id + 1); // References piece2
                assert_eq!(pieces[1].offset_in_concat, 10);
                assert_eq!(pieces[1].length, 10);
            }
            _ => panic!("Expected Concat mapping"),
        }
    }

    #[test]
    fn test_source_info_pool_deduplication() {
        // Test that the same Rc gets the same ID (deduplication)
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let parent = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 100,
        });

        // Create multiple Substrings sharing the same parent Rc
        let child1 = SourceInfo::Substring {
            parent: Arc::clone(&parent),
            start_offset: 10,
            end_offset: 20,
        };

        let child2 = SourceInfo::Substring {
            parent: Arc::clone(&parent),
            start_offset: 30,
            end_offset: 40,
        };

        serializer.intern(&child1);
        serializer.intern(&child2);

        // parent at slot 0, child1 at 1, child2 at 2.
        let parent_id = 0;
        assert_eq!(serializer.pool.len(), parent_id + 3);

        // Both children should reference the same parent ID
        match &serializer.pool[parent_id + 1].mapping {
            SerializableSourceMapping::Substring {
                parent_id: parent_ref,
            } => {
                assert_eq!(*parent_ref, parent_id);
            }
            _ => panic!("Expected Substring"),
        }

        match &serializer.pool[parent_id + 2].mapping {
            SerializableSourceMapping::Substring {
                parent_id: parent_ref,
            } => {
                assert_eq!(*parent_ref, parent_id); // Same parent ID as child1
            }
            _ => panic!("Expected Substring"),
        }

        // Verify the parent was only added once
        let original_count = serializer
            .pool
            .iter()
            .filter(|entry| matches!(entry.mapping, SerializableSourceMapping::Original { .. }))
            .count();
        assert_eq!(original_count, 1, "Parent should only appear once in pool");
    }

    #[test]
    fn test_custom_block_json_roundtrip() {
        use crate::pandoc::attr::empty_attr;
        use crate::pandoc::{Block, CustomNode, Paragraph, Slot, Str};
        use crate::readers::json as json_reader;

        // Create a custom block node with slots
        let custom = CustomNode {
            type_name: "Callout".to_string(),
            slots: {
                let mut slots = hashlink::LinkedHashMap::new();
                slots.insert(
                    "title".to_string(),
                    Slot::Inlines(vec![crate::pandoc::Inline::Str(Str {
                        text: "Warning".to_string(),
                        source_info: SourceInfo::for_test(),
                    })]),
                );
                slots.insert(
                    "content".to_string(),
                    Slot::Blocks(vec![Block::Paragraph(Paragraph {
                        content: vec![crate::pandoc::Inline::Str(Str {
                            text: "Be careful!".to_string(),
                            source_info: SourceInfo::for_test(),
                        })],
                        source_info: SourceInfo::for_test(),
                    })]),
                );
                slots
            },
            plain_data: serde_json::json!({"type": "warning", "appearance": "simple"}),
            attr: empty_attr(),
            source_info: SourceInfo::for_test(),
        };

        let block = Block::Custom(custom);

        // Create a minimal Pandoc document with this block
        let pandoc = crate::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![block],
        };

        // Write to JSON
        let context = make_test_context();
        let config = make_test_config();
        let mut output = Vec::new();
        write_with_config(&pandoc, &context, &mut output, &config).unwrap();

        // Read back
        let (read_pandoc, _) = json_reader::read(&mut output.as_slice()).unwrap();

        // Verify we got a Custom block back
        assert_eq!(read_pandoc.blocks.len(), 1);
        match &read_pandoc.blocks[0] {
            Block::Custom(read_custom) => {
                assert_eq!(read_custom.type_name, "Callout");
                assert_eq!(read_custom.slots.len(), 2);
                assert!(read_custom.slots.contains_key("title"));
                assert!(read_custom.slots.contains_key("content"));
                assert_eq!(read_custom.plain_data["type"], "warning");
                assert_eq!(read_custom.plain_data["appearance"], "simple");
            }
            other => panic!("Expected Custom block, got {:?}", other),
        }
    }

    #[test]
    fn test_custom_inline_json_roundtrip() {
        use crate::pandoc::attr::empty_attr;
        use crate::pandoc::{Block, CustomNode, Inline, Paragraph, Slot, Str};
        use crate::readers::json as json_reader;

        // Create a custom inline node with slots
        let custom = CustomNode {
            type_name: "Tooltip".to_string(),
            slots: {
                let mut slots = hashlink::LinkedHashMap::new();
                slots.insert(
                    "text".to_string(),
                    Slot::Inlines(vec![Inline::Str(Str {
                        text: "hover me".to_string(),
                        source_info: SourceInfo::for_test(),
                    })]),
                );
                slots
            },
            plain_data: serde_json::json!({"tip": "This is a tooltip"}),
            attr: empty_attr(),
            source_info: SourceInfo::for_test(),
        };

        let inline = Inline::Custom(custom);

        // Create a minimal Pandoc document with this inline in a paragraph
        let pandoc = crate::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![inline],
                source_info: SourceInfo::for_test(),
            })],
        };

        // Write to JSON
        let context = make_test_context();
        let config = make_test_config();
        let mut output = Vec::new();
        write_with_config(&pandoc, &context, &mut output, &config).unwrap();

        // Read back
        let (read_pandoc, _) = json_reader::read(&mut output.as_slice()).unwrap();

        // Verify we got a Custom inline back
        assert_eq!(read_pandoc.blocks.len(), 1);
        match &read_pandoc.blocks[0] {
            Block::Paragraph(para) => {
                assert_eq!(para.content.len(), 1);
                match &para.content[0] {
                    Inline::Custom(read_custom) => {
                        assert_eq!(read_custom.type_name, "Tooltip");
                        assert_eq!(read_custom.slots.len(), 1);
                        assert!(read_custom.slots.contains_key("text"));
                        assert_eq!(read_custom.plain_data["tip"], "This is a tooltip");
                    }
                    other => panic!("Expected Custom inline, got {:?}", other),
                }
            }
            other => panic!("Expected Paragraph, got {:?}", other),
        }
    }

    #[test]
    fn test_custom_block_preserves_attr() {
        use crate::pandoc::{Block, CustomNode};
        use crate::readers::json as json_reader;

        // Create a custom node with custom id and classes
        let attr = (
            "my-callout".to_string(),
            vec!["callout-warning".to_string(), "important".to_string()],
            {
                let mut kvs = hashlink::LinkedHashMap::new();
                kvs.insert("data-foo".to_string(), "bar".to_string());
                kvs
            },
        );

        let custom = CustomNode {
            type_name: "Callout".to_string(),
            slots: hashlink::LinkedHashMap::new(),
            plain_data: serde_json::Value::Null,
            attr,
            source_info: SourceInfo::for_test(),
        };

        let block = Block::Custom(custom);

        // Create a minimal Pandoc document
        let pandoc = crate::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![block],
        };

        // Write and read back
        let context = make_test_context();
        let config = make_test_config();
        let mut output = Vec::new();
        write_with_config(&pandoc, &context, &mut output, &config).unwrap();
        let (read_pandoc, _) = json_reader::read(&mut output.as_slice()).unwrap();

        // Verify attr was preserved
        match &read_pandoc.blocks[0] {
            Block::Custom(read_custom) => {
                assert_eq!(read_custom.attr.0, "my-callout");
                assert_eq!(
                    read_custom.attr.1,
                    vec!["callout-warning".to_string(), "important".to_string()]
                );
                assert_eq!(read_custom.attr.2.get("data-foo"), Some(&"bar".to_string()));
            }
            _ => panic!("Expected Custom block"),
        }
    }

    #[test]
    fn test_paragraph_trailing_attr_emits_attr_key() {
        // A `Paragraph` ending in a trailing standalone `Inline::Attr` (as a
        // filter would inject for `<p class="caption">`) must serialize to a
        // Pandoc-valid `Para` node carrying an extra `attr` object key — the
        // same "safe extra key" channel the `s`/`l` source-info keys use, which
        // Pandoc ignores. The trailing `Inline::Attr` (and the `Space` before
        // it) are stripped from `c`. See bd-itqcfxc3.
        use crate::pandoc::inline::InlineAttr;
        use crate::pandoc::{Block, Inline, Paragraph, Space, Str};

        let attr: Attr = (
            String::new(),
            vec!["caption".to_string()],
            LinkedHashMap::new(),
        );
        let para = Paragraph {
            content: vec![
                Inline::Str(Str {
                    text: "This is a caption.".to_string(),
                    source_info: SourceInfo::for_test(),
                }),
                Inline::Space(Space {
                    source_info: SourceInfo::for_test(),
                }),
                Inline::Attr(InlineAttr::new(
                    attr,
                    AttrSourceInfo::empty(),
                    SourceInfo::for_test(),
                )),
            ],
            source_info: SourceInfo::for_test(),
        };
        let pandoc = crate::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(para)],
        };

        let context = make_test_context();
        let config = make_test_config();
        let mut output = Vec::new();
        write_with_config(&pandoc, &context, &mut output, &config).unwrap();

        let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
        let para_node = &v["blocks"][0];
        assert_eq!(para_node["t"], "Para");
        // The collected block attr rides as an extra `attr` key: [id, classes, kvs].
        assert_eq!(para_node["attr"], json!(["", ["caption"], []]));
        // The trailing `Inline::Attr` and its preceding `Space` are gone from `c`.
        let c = para_node["c"].as_array().expect("c is an array");
        assert_eq!(c.len(), 1, "expected only the Str to remain, got {:?}", c);
        assert_eq!(c[0]["t"], "Str");
        assert_eq!(c[0]["c"], "This is a caption.");
    }

    #[test]
    fn test_paragraph_trailing_attr_roundtrips() {
        // AST -> JSON -> AST must preserve a Para's block attr: the writer emits
        // it as the `attr` key (stripping the trailing `Inline::Attr` from `c`),
        // and the reader must fold it back into a trailing `Inline::Attr` so a
        // re-serialization re-emits the same `attr` key. See bd-itqcfxc3.
        use crate::pandoc::inline::InlineAttr;
        use crate::pandoc::{Block, Inline, Paragraph, Space, Str};
        use crate::readers::json as json_reader;

        let attr: Attr = (
            "cap1".to_string(),
            vec!["caption".to_string()],
            LinkedHashMap::new(),
        );
        let para = Paragraph {
            content: vec![
                Inline::Str(Str {
                    text: "Cap".to_string(),
                    source_info: SourceInfo::for_test(),
                }),
                Inline::Space(Space {
                    source_info: SourceInfo::for_test(),
                }),
                Inline::Attr(InlineAttr::new(
                    attr,
                    AttrSourceInfo::empty(),
                    SourceInfo::for_test(),
                )),
            ],
            source_info: SourceInfo::for_test(),
        };
        let pandoc = crate::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(para)],
        };

        let context = make_test_context();
        let config = make_test_config();
        let mut output = Vec::new();
        write_with_config(&pandoc, &context, &mut output, &config).unwrap();

        let (read_pandoc, _) = json_reader::read(&mut output.as_slice()).unwrap();
        match &read_pandoc.blocks[0] {
            Block::Paragraph(p) => {
                // The trailing Inline::Attr is restored from the `attr` key.
                match p.content.last() {
                    Some(Inline::Attr(a)) => {
                        assert_eq!(a.attr.0, "cap1");
                        assert_eq!(a.attr.1, vec!["caption".to_string()]);
                    }
                    other => panic!("expected trailing Inline::Attr, got {:?}", other),
                }
            }
            other => panic!("expected Paragraph, got {:?}", other),
        }
    }

    // ----------------------------------------------------------------
    // List-item block attrs (<li class>) — bd-aeyss6p5
    // ----------------------------------------------------------------

    fn li_attr_plain(text: &str, attr: Option<Attr>) -> Block {
        use crate::pandoc::inline::InlineAttr;
        use crate::pandoc::{Inline, Plain, Space, Str};
        let mut content = vec![Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })];
        if let Some(attr) = attr {
            content.push(Inline::Space(Space {
                source_info: SourceInfo::for_test(),
            }));
            content.push(Inline::Attr(InlineAttr::new(
                attr,
                AttrSourceInfo::empty(),
                SourceInfo::for_test(),
            )));
        }
        Block::Plain(Plain {
            content,
            source_info: SourceInfo::for_test(),
        })
    }

    fn write_blocks_to_json(blocks: Vec<Block>) -> serde_json::Value {
        let pandoc = crate::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };
        let context = make_test_context();
        let config = make_test_config();
        let mut output = Vec::new();
        write_with_config(&pandoc, &context, &mut output, &config).unwrap();
        serde_json::from_slice(&output).unwrap()
    }

    #[test]
    fn test_bullet_list_item_attr_emits_itemattr_key() {
        use crate::pandoc::{Block, BulletList};

        let foo: Attr = (String::new(), vec!["foo".to_string()], LinkedHashMap::new());
        let list = Block::BulletList(BulletList {
            content: vec![
                vec![li_attr_plain("item one", Some(foo))],
                vec![li_attr_plain("item two", None)],
            ],
            source_info: SourceInfo::for_test(),
        });
        let v = write_blocks_to_json(vec![list]);
        let node = &v["blocks"][0];
        assert_eq!(node["t"], "BulletList");
        // Parallel-indexed sibling key: attr for item0, null for item1.
        assert_eq!(node["itemAttr"], json!([["", ["foo"], []], null]));
        // item0's inner Plain has the trailing attr + space stripped from `c`.
        let item0 = node["c"][0].as_array().expect("item0 is an array");
        let plain0 = &item0[0];
        assert_eq!(plain0["t"], "Plain");
        let plain0_c = plain0["c"].as_array().expect("Plain c is an array");
        assert_eq!(
            plain0_c.len(),
            1,
            "trailing attr+space stripped, got {:?}",
            plain0_c
        );
        assert_eq!(plain0_c[0]["t"], "Str");
    }

    #[test]
    fn test_ordinary_bullet_list_emits_no_itemattr_key() {
        use crate::pandoc::{Block, BulletList};
        let list = Block::BulletList(BulletList {
            content: vec![
                vec![li_attr_plain("a", None)],
                vec![li_attr_plain("b", None)],
            ],
            source_info: SourceInfo::for_test(),
        });
        let v = write_blocks_to_json(vec![list]);
        let node = &v["blocks"][0];
        assert!(
            node.get("itemAttr").is_none(),
            "ordinary list must not carry itemAttr, got {:?}",
            node
        );
    }

    #[test]
    fn test_ordered_list_item_attr_emits_itemattr_key() {
        use crate::pandoc::{Block, ListNumberDelim, ListNumberStyle, OrderedList};
        let foo: Attr = (String::new(), vec!["foo".to_string()], LinkedHashMap::new());
        let list = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![
                vec![li_attr_plain("first", None)],
                vec![li_attr_plain("second", Some(foo))],
            ],
            source_info: SourceInfo::for_test(),
        });
        let v = write_blocks_to_json(vec![list]);
        let node = &v["blocks"][0];
        assert_eq!(node["t"], "OrderedList");
        // itemAttr is parallel to the items array (c[1]).
        assert_eq!(node["itemAttr"], json!([null, ["", ["foo"], []]]));
        // The items still live at c[1]; list attributes at c[0].
        let item1 = node["c"][1][1].as_array().expect("item1 is an array");
        let plain1 = &item1[0];
        assert_eq!(plain1["c"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_bullet_list_item_attr_roundtrips() {
        use crate::pandoc::{Block, BulletList, Inline};
        use crate::readers::json as json_reader;

        let foo: Attr = (
            "li-id".to_string(),
            vec!["foo".to_string()],
            LinkedHashMap::new(),
        );
        let list = Block::BulletList(BulletList {
            content: vec![
                vec![li_attr_plain("one", Some(foo))],
                vec![li_attr_plain("two", None)],
            ],
            source_info: SourceInfo::for_test(),
        });
        let pandoc = crate::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![list],
        };
        let context = make_test_context();
        let config = make_test_config();
        let mut output = Vec::new();
        write_with_config(&pandoc, &context, &mut output, &config).unwrap();
        let (read_pandoc, _) = json_reader::read(&mut output.as_slice()).unwrap();

        match &read_pandoc.blocks[0] {
            Block::BulletList(bl) => {
                // Item 0's last block regains a trailing Inline::Attr.
                match bl.content[0].last().unwrap() {
                    Block::Plain(p) => match p.content.last() {
                        Some(Inline::Attr(a)) => {
                            assert_eq!(a.attr.0, "li-id");
                            assert_eq!(a.attr.1, vec!["foo".to_string()]);
                        }
                        other => panic!("expected trailing Inline::Attr, got {:?}", other),
                    },
                    other => panic!("expected Plain, got {:?}", other),
                }
                // Item 1 (no attr) is unchanged: just the Str.
                match bl.content[1].last().unwrap() {
                    Block::Plain(p) => {
                        assert!(
                            !matches!(p.content.last(), Some(Inline::Attr(_))),
                            "item without attr must not gain one"
                        );
                    }
                    other => panic!("expected Plain, got {:?}", other),
                }
            }
            other => panic!("expected BulletList, got {:?}", other),
        }
    }

    // ----------------------------------------------------------------
    // Plan 5 Phase 3+4 — writer-side Generated emission
    // ----------------------------------------------------------------

    /// `Generated { by, from: [] }` interns as a single code-4 pool entry
    /// with `r = (0, 0)` and the right `by` shape.
    #[test]
    fn test_source_info_pool_generated_no_anchors() {
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let gen_info = SourceInfo::Generated {
            by: By::sectionize(),
            from: SmallVec::new(),
        };
        let id = serializer.intern(&gen_info);

        // Pool starts at 0; this intern lands
        // at slot 1.
        let first_user_id = 0;
        assert_eq!(id, first_user_id);
        assert_eq!(serializer.pool.len(), first_user_id + 1);
        let entry = &serializer.pool[id];
        assert_eq!(entry.start_offset, 0);
        assert_eq!(entry.end_offset, 0);
        match &entry.mapping {
            SerializableSourceMapping::Generated { by, from } => {
                assert_eq!(by.kind, "sectionize");
                assert!(by.data.is_null());
                assert!(from.is_empty());
            }
            _ => panic!("Expected Generated mapping"),
        }
    }

    /// `Generated { by: filter, from: [] }` carries `by.data` through.
    #[test]
    fn test_source_info_pool_generated_filter_with_data() {
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let gen_info = SourceInfo::generated(By::filter("/x.lua", 42));
        let id = serializer.intern(&gen_info);

        let entry = &serializer.pool[id];
        match &entry.mapping {
            SerializableSourceMapping::Generated { by, .. } => {
                assert_eq!(by.kind, "filter");
                assert_eq!(by.as_filter(), Some(("/x.lua", 42)));
            }
            _ => panic!("Expected Generated mapping"),
        }
    }

    /// Anchors must be interned strictly *before* their owning Generated
    /// entry — the reader's `si_id < current_index` guard requires it.
    #[test]
    fn test_source_info_pool_generated_with_invocation_anchor() {
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let target = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 5,
            end_offset: 12,
        });
        let mut from = SmallVec::<[Anchor; 2]>::new();
        from.push(Anchor::invocation(Arc::clone(&target)));
        let gen_info = SourceInfo::Generated {
            by: By::shortcode("meta"),
            from,
        };

        let id = serializer.intern(&gen_info);
        // anchor target interned at slot 0, Generated at 1.
        let target_id = 0;
        assert_eq!(id, target_id + 1);
        assert!(matches!(
            serializer.pool[target_id].mapping,
            SerializableSourceMapping::Original { .. }
        ));
        match &serializer.pool[id].mapping {
            SerializableSourceMapping::Generated { by, from } => {
                assert_eq!(by.kind, "shortcode");
                assert_eq!(from.len(), 1);
                assert!(matches!(from[0].0, AnchorRole::Invocation));
                assert_eq!(from[0].1, target_id); // si_id points to the target
            }
            _ => panic!("Expected Generated mapping"),
        }
    }

    /// Multi-inline shortcode resolution: N Generated nodes sharing one
    /// `Arc<SourceInfo>` anchor target collapse to a single pool entry on
    /// the write side. The dedup is keyed by `Arc::as_ptr`.
    #[test]
    fn test_source_info_pool_generated_anchor_dedup() {
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let shared = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 10,
        });

        // Three sibling Generated entries each pointing at `shared`.
        let make = || {
            let mut from = SmallVec::<[Anchor; 2]>::new();
            from.push(Anchor::invocation(Arc::clone(&shared)));
            SourceInfo::Generated {
                by: By::shortcode("meta"),
                from,
            }
        };
        let id1 = serializer.intern(&make());
        let id2 = serializer.intern(&make());
        let id3 = serializer.intern(&make());

        // shared(0), gen1(1), gen2(2), gen3(3) — shared
        // interned once.
        let shared_id = 0;
        assert_eq!(serializer.pool.len(), shared_id + 4);
        let original_count = serializer
            .pool
            .iter()
            .filter(|e| matches!(e.mapping, SerializableSourceMapping::Original { .. }))
            .count();
        assert_eq!(original_count, 1, "shared target must intern exactly once");

        for id in [id1, id2, id3] {
            match &serializer.pool[id].mapping {
                SerializableSourceMapping::Generated { from, .. } => {
                    assert_eq!(from.len(), 1);
                    assert_eq!(from[0].1, shared_id); // all reference the same si_id
                }
                _ => panic!("Expected Generated"),
            }
        }
    }

    /// `Concat { pieces: [Generated, ...] }` round-trips: each piece's
    /// Generated source_info interns through the new code-4 path; the
    /// outer Concat references those IDs.
    #[test]
    fn test_source_info_pool_concat_of_generated() {
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let g1 = SourceInfo::generated(By::filter("/a.lua", 1));
        let g2 = SourceInfo::generated(By::filter("/b.lua", 2));
        let concat = SourceInfo::concat(vec![(g1, 5), (g2, 7)]);

        let id = serializer.intern(&concat);
        // two Generated entries at 0, 1; Concat at 2.
        let g1_id = 0;
        assert_eq!(id, g1_id + 2);
        assert!(matches!(
            serializer.pool[g1_id].mapping,
            SerializableSourceMapping::Generated { .. }
        ));
        assert!(matches!(
            serializer.pool[g1_id + 1].mapping,
            SerializableSourceMapping::Generated { .. }
        ));
        match &serializer.pool[id].mapping {
            SerializableSourceMapping::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[0].source_info_id, g1_id);
                assert_eq!(pieces[1].source_info_id, g1_id + 1);
            }
            _ => panic!("Expected Concat"),
        }
    }

    /// `Substring { parent: Arc<Generated>, ... }` interns the Generated
    /// parent first; the Substring references it by ID.
    #[test]
    fn test_source_info_pool_substring_of_generated() {
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let parent = Arc::new(SourceInfo::generated(By::filter("/x.lua", 1)));
        let child = SourceInfo::Substring {
            parent: Arc::clone(&parent),
            start_offset: 0,
            end_offset: 4,
        };
        let id = serializer.intern(&child);

        // Generated parent at 0, Substring at 1.
        let parent_id = 0;
        assert_eq!(id, parent_id + 1);
        assert!(matches!(
            serializer.pool[parent_id].mapping,
            SerializableSourceMapping::Generated { .. }
        ));
        match &serializer.pool[id].mapping {
            SerializableSourceMapping::Substring {
                parent_id: parent_ref,
            } => {
                assert_eq!(*parent_ref, parent_id);
            }
            _ => panic!("Expected Substring"),
        }
    }

    /// `to_json` emits the Generated entry as `{"t":4, "r":[0,0], "d": ...}`
    /// with the expected `by`/`from` shape.
    #[test]
    fn test_to_json_generated_emits_code_4() {
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let target = Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 5,
            end_offset: 12,
        });
        let mut from = SmallVec::<[Anchor; 2]>::new();
        from.push(Anchor::invocation(Arc::clone(&target)));
        let gen_info = SourceInfo::Generated {
            by: By::shortcode("meta"),
            from,
        };
        let _ = serializer.intern(&gen_info);

        // anchor target at 0, Generated at 1.
        let target_id = 0;
        let gen_entry_json = serializer.pool[target_id + 1].to_json();
        assert_eq!(gen_entry_json.t, 4);
        assert_eq!(gen_entry_json.r, [0, 0]);

        // Expected wire shape:
        //   { "by": { "kind": "shortcode", "data": { "name": "meta" } },
        //     "from": [ { "role": "invocation", "si_id": <target_id> } ] }
        let expected = json!({
            "by": { "kind": "shortcode", "data": { "name": "meta" } },
            "from": [ { "role": "invocation", "si_id": target_id } ]
        });
        assert_eq!(gen_entry_json.d, expected);
    }

    /// `to_json` skips `"data"` when `by.data` is null and skips `"from"`
    /// when the anchor list is empty.
    #[test]
    fn test_to_json_generated_skips_null_data_and_empty_from() {
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let gen_info = SourceInfo::generated(By::sectionize());
        let id = serializer.intern(&gen_info);
        let entry_json = serializer.pool[id].to_json();
        assert_eq!(entry_json.t, 4);
        // Exactly: { "by": { "kind": "sectionize" } } — no data, no from.
        let expected = json!({ "by": { "kind": "sectionize" } });
        assert_eq!(entry_json.d, expected);
    }

    /// AnchorRole round-trip via the writer's `serialize_anchor_role` —
    /// every known role plus an extension-defined `Other` survives.
    #[test]
    fn test_serialize_anchor_role_all_roles() {
        assert_eq!(serialize_anchor_role(&AnchorRole::Invocation), "invocation");
        assert_eq!(
            serialize_anchor_role(&AnchorRole::ValueSource),
            "value-source"
        );
        assert_eq!(
            serialize_anchor_role(&AnchorRole::Other("ext/foo/bar".to_string())),
            "other:ext/foo/bar"
        );
    }
}
