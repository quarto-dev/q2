/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::{AttrSourceInfo, TargetSourceInfo};
use crate::pandoc::{
    ASTContext, Attr, Block, Caption, CitationMode, Inline, Inlines, ListAttributes, Pandoc,
};
use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::{FileId, SourceInfo};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Configuration for JSON output format.
#[derive(Debug, Clone, Default)]
pub struct JsonConfig {
    /// If true, include resolved source locations ('l' field) in each node.
    /// The 'l' field contains an object with:
    /// - 'f': file_id (usize)
    /// - 'b': begin position {o: offset, l: line (1-based), c: column (1-based)}
    /// - 'e': end position {o: offset, l: line (1-based), c: column (1-based)}
    pub include_inline_locations: bool,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
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
    d: Value,      // data (file_id, parent_id, pieces, or filter info)
    r: [usize; 2], // range [start, end]
    t: u8,         // type code (0=Original, 1=Substring, 2=Concat, 3=FilterProvenance)
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
/// Fields ordered alphabetically: attrS, c, s, t
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeWithAttrJson {
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
            SerializableSourceMapping::FilterProvenance { filter_path, line } => {
                (3, json!((filter_path, line)))
            }
        };
        SourceInfoJson {
            d,
            r: [self.start_offset, self.end_offset],
            t,
        }
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
    FilterProvenance {
        filter_path: String,
        line: usize,
    },
}

/// Serializable version of SourcePiece that uses source_info_id instead of SourceInfo.
struct SerializableSourcePiece {
    source_info_id: usize,
    offset_in_concat: usize,
    length: usize,
}

/// Serializer that builds a pool of unique SourceInfo objects and assigns IDs.
///
/// During AST traversal, each SourceInfo is interned into the pool. Rc-shared
/// SourceInfo objects get the same ID (using pointer equality). Parent references
/// are serialized as parent_id integers instead of full nested objects.
///
/// This approach reduces JSON size by ~93% for documents with many nodes sharing
/// the same parent chains (e.g., YAML metadata with siblings).
struct SourceInfoSerializer<'a> {
    pool: Vec<SerializableSourceInfo>,
    id_map: HashMap<*const SourceInfo, usize>,
    // Store clones of SourceInfo for content-based deduplication.
    // When a clone is created (e.g., in write_config_value for Path), the pointer lookup
    // will fail, so we fall back to checking content equality against this list.
    content_map: Vec<(SourceInfo, usize)>,
    context: &'a ASTContext,
    config: &'a JsonConfig,
}

impl<'a> SourceInfoSerializer<'a> {
    fn new(context: &'a ASTContext, config: &'a JsonConfig) -> Self {
        SourceInfoSerializer {
            pool: Vec::new(),
            id_map: HashMap::new(),
            content_map: Vec::new(),
            context,
            config,
        }
    }

    /// Intern a SourceInfo into the pool, returning its ID.
    ///
    /// If this SourceInfo (or an Rc-equivalent) has already been interned,
    /// returns the existing ID. Otherwise, recursively interns parents and
    /// adds this SourceInfo to the pool with a new ID.
    fn intern(&mut self, source_info: &SourceInfo) -> usize {
        // For Rc-shared SourceInfo objects, we need to detect if they point to the same
        // underlying data. We use the data pointer address for this.
        let ptr = source_info as *const SourceInfo;

        // Check if already interned by pointer
        if let Some(&id) = self.id_map.get(&ptr) {
            return id;
        }

        // Fallback: check for content equality against previously interned SourceInfos.
        // This handles cases where SourceInfo is cloned (e.g., in write_config_value for Path).
        // Clones have different addresses but identical content.
        for (existing, id) in &self.content_map {
            if existing == source_info {
                // Cache this pointer for future lookups
                self.id_map.insert(ptr, *id);
                return *id;
            }
        }

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
                let parent_id = self.intern(parent);
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
            SourceInfo::FilterProvenance { filter_path, line } => (
                0,
                0,
                SerializableSourceMapping::FilterProvenance {
                    filter_path: filter_path.clone(),
                    line: *line,
                },
            ),
        };

        // Calculate ID after recursion completes
        let id = self.pool.len();

        // Add to pool
        self.pool.push(SerializableSourceInfo {
            id,
            start_offset,
            end_offset,
            mapping,
        });

        // Record this pointer's ID for future lookups
        self.id_map.insert(ptr, id);

        // Also store a clone for content-based deduplication of future clones
        self.content_map.push((source_info.clone(), id));

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

/// Context for JSON writer containing both source info serialization and error collection.
///
/// This struct combines the SourceInfoSerializer (for building the source info pool)
/// with error accumulation during AST traversal. Separating these concerns makes the
/// dual purpose of the writer more explicit.
struct JsonWriterContext<'a> {
    serializer: SourceInfoSerializer<'a>,
    errors: Vec<DiagnosticMessage>,
    /// Pre-serialized JSON for ConfigValue Path/Glob/Expr variants.
    /// Keys are pointers to original ConfigValues in the AST; values are already-serialized JSON.
    /// This prevents memory reuse bugs where temporary Inlines created during serialization
    /// get dropped and their memory addresses get reused by subsequent allocations.
    /// By pre-serializing during the precomputation phase, we ensure all SourceInfos from
    /// these variants are interned first, and we store the resulting JSON for later retrieval.
    precomputed_json: HashMap<*const ConfigValue, Value>,
}

impl<'a> JsonWriterContext<'a> {
    fn new(ast_context: &'a ASTContext, config: &'a JsonConfig) -> Self {
        JsonWriterContext {
            serializer: SourceInfoSerializer::new(ast_context, config),
            errors: Vec::new(),
            precomputed_json: HashMap::new(),
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
        source_info: SourceInfo::default(),
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
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    })]
}

/// Walk a ConfigValue and pre-serialize Inlines for Path/Glob/Expr variants.
///
/// This function is called at the start of serialization to build all Inlines
/// upfront, serialize them to JSON (which interns their SourceInfos into the pool),
/// and store the resulting JSON for later retrieval.
///
/// CRITICAL: The `inlines_keeper` parameter collects all temporary Inlines created
/// during precomputation. These MUST be kept alive until precomputation completes
/// to prevent memory reuse bugs. Without this, the allocator can reuse a freed
/// clone's address for a subsequent clone, causing the SourceInfoSerializer's
/// pointer cache to return incorrect IDs.
///
/// See: claude-notes/plans/2026-01-13-precomputation-memory-reuse-bug.md
fn precompute_config_value_json(
    config_value: &ConfigValue,
    ctx: &mut JsonWriterContext,
    inlines_keeper: &mut Vec<Inlines>,
) {
    match &config_value.value {
        ConfigValueKind::Path(s) => {
            let inlines = build_path_inlines(s, &config_value.source_info);
            let json = write_inlines(&inlines, ctx);
            ctx.precomputed_json
                .insert(config_value as *const ConfigValue, json);
            inlines_keeper.push(inlines); // Keep alive until precomputation completes
        }
        ConfigValueKind::Glob(s) => {
            let inlines = build_glob_inlines(s, &config_value.source_info);
            let json = write_inlines(&inlines, ctx);
            ctx.precomputed_json
                .insert(config_value as *const ConfigValue, json);
            inlines_keeper.push(inlines); // Keep alive until precomputation completes
        }
        ConfigValueKind::Expr(s) => {
            let inlines = build_expr_inlines(s, &config_value.source_info);
            let json = write_inlines(&inlines, ctx);
            ctx.precomputed_json
                .insert(config_value as *const ConfigValue, json);
            inlines_keeper.push(inlines); // Keep alive until precomputation completes
        }
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                precompute_config_value_json(&entry.value, ctx, inlines_keeper);
            }
        }
        ConfigValueKind::Array(items) => {
            for item in items {
                precompute_config_value_json(item, ctx, inlines_keeper);
            }
        }
        // Other variants don't need pre-computation
        _ => {}
    }
}

/// Walk a Block and pre-serialize JSON for any ConfigValue Path/Glob/Expr variants.
///
/// See `precompute_config_value_json` for why `inlines_keeper` is required.
fn precompute_block_json(
    block: &Block,
    ctx: &mut JsonWriterContext,
    inlines_keeper: &mut Vec<Inlines>,
) {
    match block {
        Block::BlockMetadata(meta) => {
            precompute_config_value_json(&meta.meta, ctx, inlines_keeper);
        }
        // Recursively walk blocks that contain other blocks
        Block::BlockQuote(bq) => {
            for b in &bq.content {
                precompute_block_json(b, ctx, inlines_keeper);
            }
        }
        Block::OrderedList(ol) => {
            for item in &ol.content {
                for b in item {
                    precompute_block_json(b, ctx, inlines_keeper);
                }
            }
        }
        Block::BulletList(bl) => {
            for item in &bl.content {
                for b in item {
                    precompute_block_json(b, ctx, inlines_keeper);
                }
            }
        }
        Block::DefinitionList(dl) => {
            for (_, blocks_list) in &dl.content {
                for blocks in blocks_list {
                    for b in blocks {
                        precompute_block_json(b, ctx, inlines_keeper);
                    }
                }
            }
        }
        Block::Div(div) => {
            for b in &div.content {
                precompute_block_json(b, ctx, inlines_keeper);
            }
        }
        Block::Figure(fig) => {
            for b in &fig.content {
                precompute_block_json(b, ctx, inlines_keeper);
            }
        }
        Block::Table(table) => {
            // Walk table bodies (head and body rows of each TableBody)
            for table_body in &table.bodies {
                for row in &table_body.head {
                    for cell in &row.cells {
                        for b in &cell.content {
                            precompute_block_json(b, ctx, inlines_keeper);
                        }
                    }
                }
                for row in &table_body.body {
                    for cell in &row.cells {
                        for b in &cell.content {
                            precompute_block_json(b, ctx, inlines_keeper);
                        }
                    }
                }
            }
            // Walk table head
            for row in &table.head.rows {
                for cell in &row.cells {
                    for b in &cell.content {
                        precompute_block_json(b, ctx, inlines_keeper);
                    }
                }
            }
            // Walk table foot
            for row in &table.foot.rows {
                for cell in &row.cells {
                    for b in &cell.content {
                        precompute_block_json(b, ctx, inlines_keeper);
                    }
                }
            }
        }
        Block::Custom(custom) => {
            // Walk custom node slots for blocks
            for slot in custom.slots.values() {
                match slot {
                    crate::pandoc::Slot::Block(b) => {
                        precompute_block_json(b, ctx, inlines_keeper);
                    }
                    crate::pandoc::Slot::Blocks(blocks) => {
                        for b in blocks {
                            precompute_block_json(b, ctx, inlines_keeper);
                        }
                    }
                    // Inlines don't contain blocks
                    crate::pandoc::Slot::Inline(_) | crate::pandoc::Slot::Inlines(_) => {}
                }
            }
        }
        Block::NoteDefinitionFencedBlock(note) => {
            for b in &note.content {
                precompute_block_json(b, ctx, inlines_keeper);
            }
        }
        // Leaf blocks that don't contain other blocks
        Block::Plain(_)
        | Block::Paragraph(_)
        | Block::LineBlock(_)
        | Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::Header(_)
        | Block::HorizontalRule(_)
        | Block::NoteDefinitionPara(_)
        | Block::CaptionBlock(_) => {}
    }
}

/// Pre-serialize all Path/Glob/Expr ConfigValues in the entire Pandoc structure.
///
/// This must be called at the start of serialization. It builds temporary Inlines
/// for Path/Glob/Expr variants, serializes them to JSON (which interns their
/// SourceInfos into the pool), and stores the resulting JSON for later retrieval.
///
/// CRITICAL: All temporary Inlines are kept alive in `inlines_keeper` until this
/// function returns. This prevents a memory reuse bug where the allocator could
/// reuse a freed clone's address for a subsequent clone. When that happens, the
/// SourceInfoSerializer's pointer cache (`id_map`) returns a stale ID for the
/// new clone, causing incorrect source info references in the output.
///
/// The bug manifests as non-deterministic `s` values in the JSON output because
/// memory reuse depends on allocator state, which varies between runs.
///
/// See: claude-notes/plans/2026-01-13-precomputation-memory-reuse-bug.md
fn precompute_all_json(pandoc: &Pandoc, ctx: &mut JsonWriterContext) {
    // Keep all temporary Inlines alive until precomputation is complete.
    // This prevents memory reuse where a dropped clone's address could be
    // reused by a subsequent clone, causing stale pointer cache hits.
    let mut inlines_keeper: Vec<Inlines> = Vec::new();

    // Walk top-level metadata
    precompute_config_value_json(&pandoc.meta, ctx, &mut inlines_keeper);

    // Walk all blocks for BlockMetadata nodes
    for block in &pandoc.blocks {
        precompute_block_json(block, ctx, &mut inlines_keeper);
    }

    // inlines_keeper is dropped here, AFTER all precomputation is done.
    // At this point, all SourceInfos have been interned and their IDs are
    // safely stored in precomputed_json. Memory can now be safely reused.
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
    if ctx.serializer.config.include_inline_locations {
        if let Some(location) = resolve_location(source_info, ctx.serializer.context) {
            if let Value::Object(ref mut obj) = value {
                obj.insert("l".to_string(), location);
            }
        }
    }

    value
}

// NOTE: This function is currently unused and would need a SourceContext parameter
// to map offsets to row/column positions. Commenting out for now.
// fn write_location(source_info: &quarto_source_map::SourceInfo, ctx: &SourceContext) -> Value {
//     // Extract filename index by walking to the Original mapping
//     let filename_index = crate::pandoc::location::extract_filename_index(source_info);
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
                    .map(|s| ctx.serializer.to_json_ref(s))
                    .unwrap_or(Value::Null)
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
            obj.insert("attrS".to_string(), write_attr_source(&c.attr_source, ctx));
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
            obj.insert("attrS".to_string(), write_attr_source(&link.attr_source, ctx));
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
            obj.insert("attrS".to_string(), write_attr_source(&image.attr_source, ctx));
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
            obj.insert("attrS".to_string(), write_attr_source(&span.attr_source, ctx));
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
            // Defensive: Shortcodes should not reach JSON writer
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Shortcode not supported in JSON format")
                    .with_code("Q-3-30")
                    .problem(format!("Cannot render shortcode `{{{{< {} >}}}}` in JSON format", shortcode.name))
                    .add_detail("Shortcodes are Quarto-specific and not representable in Pandoc JSON")
                    .add_hint("Use native format or process shortcodes before writing JSON")
                    .build()
            );
            let mut attr_hash = LinkedHashMap::new();
            attr_hash.insert("data-shortcode".to_string(), shortcode.name.clone());
            let attr = (String::new(), vec!["shortcode".to_string()], attr_hash);
            node_with_source("Span", Some(json!([write_attr(&attr), []])), &SourceInfo::default(), ctx)
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
        Inline::Attr(_attr, attr_source) => {
            // Defensive: Standalone attributes should not reach JSON writer
            ctx.errors.push(
                DiagnosticMessageBuilder::error("Standalone attribute not supported in JSON format")
                    .with_code("Q-3-32")
                    .with_location(attr_source.id.clone().unwrap_or_default())
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
        "attrS": write_attr_source(&cell.attr_source, ctx)
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
        "attrS": write_attr_source(&row.attr_source, ctx),
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
        "attrS": write_attr_source(&head.attr_source, ctx),
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
        "attrS": write_attr_source(&body.attr_source, ctx),
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
        "attrS": write_attr_source(&foot.attr_source, ctx),
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
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&figure.attr_source, ctx),
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
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&table.attr_source, ctx),
            );
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
            // Insert fields in alphabetical order: attrS, c, s, t
            let mut obj = serde_json::Map::new();
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&div.attr_source, ctx),
            );
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
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&header.attr_source, ctx),
            );
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
                "attrS".to_string(),
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

    // Build content: each slot wrapped in a Div with data-slot-name
    let mut content: Vec<Value> = Vec::new();
    for (name, slot) in &custom.slots {
        let slot_content = match slot {
            crate::pandoc::Slot::Block(block) => {
                vec![write_block(block, ctx)]
            }
            crate::pandoc::Slot::Inline(inline) => {
                // Wrap single inline in a Plain block
                vec![json!({
                    "t": "Plain",
                    "c": [write_inline(inline, ctx)]
                })]
            }
            crate::pandoc::Slot::Blocks(blocks) => {
                blocks.iter().map(|b| write_block(b, ctx)).collect()
            }
            crate::pandoc::Slot::Inlines(inlines) => {
                // Wrap inlines in a Plain block
                vec![json!({
                    "t": "Plain",
                    "c": write_inlines(inlines, ctx)
                })]
            }
        };

        // Each slot is wrapped in a Div with data-slot-name attribute
        let mut slot_attr_kvs = LinkedHashMap::new();
        slot_attr_kvs.insert("data-slot-name".to_string(), name.clone());
        let slot_wrapper_attr = (String::new(), vec![], slot_attr_kvs);

        content.push(json!({
            "t": "Div",
            "c": [write_attr(&slot_wrapper_attr), slot_content]
        }));
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
                vec![json!({"t": "Str", "c": "[block content]"})]
            }
        };

        // Each slot is wrapped in a Span with data-slot-name attribute
        let mut slot_attr_kvs = LinkedHashMap::new();
        slot_attr_kvs.insert("data-slot-name".to_string(), name.clone());
        let slot_wrapper_attr = (String::new(), vec![], slot_attr_kvs);

        content.push(json!({
            "t": "Span",
            "c": [write_attr(&slot_wrapper_attr), slot_content]
        }));
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
        ConfigValueKind::Scalar(yaml) => match yaml {
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
        // Path/Glob/Expr: retrieve pre-serialized JSON from the precomputation phase.
        // The Inlines for these variants were built and serialized during precompute_all_json(),
        // which ensures their SourceInfos are interned before any memory reuse can occur.
        ConfigValueKind::Path(_) | ConfigValueKind::Glob(_) | ConfigValueKind::Expr(_) => {
            let ptr = value as *const ConfigValue;
            let precomputed_content = ctx
                .precomputed_json
                .get(&ptr)
                .expect("Path/Glob/Expr ConfigValue should have precomputed JSON")
                .clone();
            meta_node("MetaInlines", precomputed_content, s)
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
pub(crate) fn write_pandoc(
    pandoc: &Pandoc,
    ast_context: &ASTContext,
    config: &JsonConfig,
) -> Result<Value, Vec<DiagnosticMessage>> {
    // Create the JSON writer context
    let mut ctx = JsonWriterContext::new(ast_context, config);

    // Pre-serialize all Path/Glob/Expr ConfigValue variants.
    // This builds temporary Inlines, serializes them to JSON (interning their
    // SourceInfos into the pool), and stores the resulting JSON for later retrieval.
    // This prevents memory reuse bugs where temporary Inlines created during
    // the main serialization pass could have their memory addresses reused by
    // subsequent allocations, causing the SourceInfoSerializer's pointer cache
    // to return incorrect IDs.
    precompute_all_json(pandoc, &mut ctx);

    // Phase 5: Write ConfigValue directly without MetaValueWithSourceInfo conversion
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
pub fn write_with_config<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
    config: &JsonConfig,
) -> Result<(), Vec<DiagnosticMessage>> {
    let json = write_pandoc(pandoc, context, config)?;
    serde_json::to_writer(writer, &json).map_err(|e| {
        vec![quarto_error_reporting::DiagnosticMessage {
            code: Some("Q-3-38".to_string()),
            title: "JSON serialization failed".to_string(),
            kind: quarto_error_reporting::DiagnosticKind::Error,
            problem: Some(format!("Failed to serialize AST to JSON: {}", e).into()),
            details: vec![],
            hints: vec![],
            location: None,
        }]
    })?;
    Ok(())
}

/// Write Pandoc AST to JSON with default configuration.
pub fn write<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
) -> Result<(), Vec<DiagnosticMessage>> {
    write_with_config(pandoc, context, writer, &JsonConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, SourceInfo};
    use std::sync::Arc;

    fn make_test_context() -> ASTContext {
        ASTContext::anonymous()
    }

    fn make_test_config() -> JsonConfig {
        JsonConfig::default()
    }

    #[test]
    fn test_source_info_pool_original() {
        // Test that a single Original SourceInfo is added to the pool correctly
        let context = make_test_context();
        let config = make_test_config();
        let mut serializer = SourceInfoSerializer::new(&context, &config);

        let source_info = SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 10,
        };

        let id = serializer.intern(&source_info);

        // Should get ID 0 for the first entry
        assert_eq!(id, 0);
        assert_eq!(serializer.pool.len(), 1);

        // Verify the pool entry
        let entry = &serializer.pool[0];
        assert_eq!(entry.start_offset, 0);
        assert_eq!(entry.end_offset, 10);
        match &entry.mapping {
            SerializableSourceMapping::Original { file_id } => {
                assert_eq!(*file_id, FileId(0));
            }
            _ => panic!("Expected Original mapping"),
        }

        // Interning the same SourceInfo again should return the same ID
        let id2 = serializer.intern(&source_info);
        assert_eq!(id2, 0);
        assert_eq!(serializer.pool.len(), 1); // No new entry added
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

        // Parent should be interned first (ID 0), child second (ID 1)
        assert_eq!(child_id, 1);
        assert_eq!(serializer.pool.len(), 2);

        // Verify parent entry
        let parent_entry = &serializer.pool[0];
        assert_eq!(parent_entry.start_offset, 0);
        assert_eq!(parent_entry.end_offset, 100);
        match &parent_entry.mapping {
            SerializableSourceMapping::Original { file_id } => {
                assert_eq!(*file_id, FileId(0));
            }
            _ => panic!("Expected Original mapping"),
        }

        // Verify child entry
        let child_entry = &serializer.pool[1];
        assert_eq!(child_entry.start_offset, 10);
        assert_eq!(child_entry.end_offset, 20);
        match &child_entry.mapping {
            SerializableSourceMapping::Substring { parent_id } => {
                assert_eq!(*parent_id, 0); // References parent
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

        // Parent should be ID 0, children should be 1, 2, 3
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        assert_eq!(serializer.pool.len(), 4); // 1 parent + 3 children

        // All children should reference the same parent (ID 0)
        for child_id in [1, 2, 3] {
            let child_entry = &serializer.pool[child_id];
            match &child_entry.mapping {
                SerializableSourceMapping::Substring { parent_id } => {
                    assert_eq!(*parent_id, 0);
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

        // Should have 6 entries total (0-5)
        assert_eq!(deepest_id, 5);
        assert_eq!(serializer.pool.len(), 6);

        // Verify the chain: each level should reference its parent
        for i in 1..=5 {
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

        // Should have 3 entries: piece1, piece2, concat
        assert_eq!(concat_id, 2);
        assert_eq!(serializer.pool.len(), 3);

        // Verify concat entry
        let concat_entry = &serializer.pool[2];
        match &concat_entry.mapping {
            SerializableSourceMapping::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[0].source_info_id, 0); // References piece1
                assert_eq!(pieces[0].offset_in_concat, 0);
                assert_eq!(pieces[0].length, 10);
                assert_eq!(pieces[1].source_info_id, 1); // References piece2
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

        // Should have 3 entries: parent (once), child1, child2
        assert_eq!(serializer.pool.len(), 3);

        // Both children should reference the same parent ID
        match &serializer.pool[1].mapping {
            SerializableSourceMapping::Substring { parent_id } => {
                assert_eq!(*parent_id, 0);
            }
            _ => panic!("Expected Substring"),
        }

        match &serializer.pool[2].mapping {
            SerializableSourceMapping::Substring { parent_id } => {
                assert_eq!(*parent_id, 0); // Same parent ID as child1
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
                        source_info: SourceInfo::default(),
                    })]),
                );
                slots.insert(
                    "content".to_string(),
                    Slot::Blocks(vec![Block::Paragraph(Paragraph {
                        content: vec![crate::pandoc::Inline::Str(Str {
                            text: "Be careful!".to_string(),
                            source_info: SourceInfo::default(),
                        })],
                        source_info: SourceInfo::default(),
                    })]),
                );
                slots
            },
            plain_data: serde_json::json!({"type": "warning", "appearance": "simple"}),
            attr: empty_attr(),
            source_info: SourceInfo::default(),
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
                        source_info: SourceInfo::default(),
                    })]),
                );
                slots
            },
            plain_data: serde_json::json!({"tip": "This is a tooltip"}),
            attr: empty_attr(),
            source_info: SourceInfo::default(),
        };

        let inline = Inline::Custom(custom);

        // Create a minimal Pandoc document with this inline in a paragraph
        let pandoc = crate::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![inline],
                source_info: SourceInfo::default(),
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
            source_info: SourceInfo::default(),
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
}
