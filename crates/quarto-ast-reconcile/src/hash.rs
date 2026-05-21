/*
 * hash.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Structural hashing for AST reconciliation.
 *
 * This module provides hash computation for Pandoc AST nodes that:
 * - Includes all semantic content (type, text, attributes, children)
 * - Excludes all source location information
 * - Uses address-based memoization for the original AST
 */

use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::{Attr, Block, ConfigMapEntry, ConfigValue, ConfigValueKind, Inline};
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// Opaque pointer used as a cache key for memoization.
///
/// We use the memory address of AST nodes as cache keys. This is safe because:
/// 1. The original AST is borrowed immutably for the entire reconciliation
/// 2. Rust guarantees borrowed values don't move
/// 3. We only use this as a HashMap key, never dereference it
#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct NodePtr(*const ());

impl NodePtr {
    /// Create a NodePtr from a reference to any type.
    #[inline]
    pub fn from_ref<T>(r: &T) -> Self {
        NodePtr(r as *const T as *const ())
    }
}

/// Cache for structural hashes, tied to the lifetime of an AST.
///
/// The PhantomData ensures this cache cannot outlive the AST it references.
pub struct HashCache<'a> {
    cache: FxHashMap<NodePtr, u64>,
    _marker: PhantomData<&'a ()>,
}

impl<'a> HashCache<'a> {
    /// Create a new empty hash cache.
    pub fn new() -> Self {
        Self {
            cache: FxHashMap::default(),
            _marker: PhantomData,
        }
    }

    /// Get or compute the structural hash for a block.
    pub fn hash_block(&mut self, block: &'a Block) -> u64 {
        let ptr = NodePtr::from_ref(block);
        if let Some(&hash) = self.cache.get(&ptr) {
            return hash;
        }
        let hash = compute_block_hash_inner(block, self);
        self.cache.insert(ptr, hash);
        hash
    }

    /// Get or compute the structural hash for an inline.
    pub fn hash_inline(&mut self, inline: &'a Inline) -> u64 {
        let ptr = NodePtr::from_ref(inline);
        if let Some(&hash) = self.cache.get(&ptr) {
            return hash;
        }
        let hash = compute_inline_hash_inner(inline, self);
        self.cache.insert(ptr, hash);
        hash
    }

    /// Compute structural hash for a sequence of blocks.
    /// Used for hashing list items in the original list.
    ///
    /// Note: This uses a different caching strategy than hash_block.
    /// We use the slice's pointer as the cache key, but only if the slice
    /// hasn't been cached before. In practice, this is fine because list
    /// items are typically accessed once per reconciliation pass.
    pub fn hash_blocks(&mut self, blocks: &'a [Block]) -> u64 {
        use rustc_hash::FxHasher;
        use std::hash::Hasher;
        let mut hasher = FxHasher::default();
        hasher.write_usize(blocks.len());
        for block in blocks {
            hasher.write_u64(self.hash_block(block));
        }
        hasher.finish()
    }
}

impl Default for HashCache<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute structural hash for a block without caching.
/// Used for the executed AST which is only traversed once.
pub fn compute_block_hash_fresh(block: &Block) -> u64 {
    let mut cache = HashCache::new();
    compute_block_hash_inner(block, &mut cache)
}

/// Compute structural hash for an inline without caching.
pub fn compute_inline_hash_fresh(inline: &Inline) -> u64 {
    let mut cache = HashCache::new();
    compute_inline_hash_inner(inline, &mut cache)
}

/// Compute structural hash for a sequence of blocks without caching.
/// Used for hashing list items in the executed list.
pub fn compute_blocks_hash_fresh(blocks: &[Block]) -> u64 {
    use rustc_hash::FxHasher;
    use std::hash::Hasher;
    let mut cache = HashCache::new();
    let mut hasher = FxHasher::default();
    hasher.write_usize(blocks.len());
    for block in blocks {
        hasher.write_u64(compute_block_hash_inner(block, &mut cache));
    }
    hasher.finish()
}

/// Internal: compute block hash with a cache reference.
fn compute_block_hash_inner(block: &Block, cache: &mut HashCache<'_>) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();

    // Hash the discriminant (block type)
    std::mem::discriminant(block).hash(&mut hasher);

    // Hash content based on type
    match block {
        Block::Plain(p) => {
            hash_inlines(&p.content, cache, &mut hasher);
        }
        Block::Paragraph(p) => {
            hash_inlines(&p.content, cache, &mut hasher);
        }
        Block::LineBlock(lb) => {
            lb.content.len().hash(&mut hasher);
            for line in &lb.content {
                hash_inlines(line, cache, &mut hasher);
            }
        }
        Block::CodeBlock(cb) => {
            hash_attr(&cb.attr, &mut hasher);
            cb.text.hash(&mut hasher);
        }
        Block::RawBlock(rb) => {
            rb.format.hash(&mut hasher);
            rb.text.hash(&mut hasher);
        }
        Block::BlockQuote(bq) => {
            hash_blocks(&bq.content, cache, &mut hasher);
        }
        Block::OrderedList(ol) => {
            // Hash list attributes (start number, style, delimiter)
            ol.attr.0.hash(&mut hasher);
            std::mem::discriminant(&ol.attr.1).hash(&mut hasher);
            std::mem::discriminant(&ol.attr.2).hash(&mut hasher);
            // Hash items
            ol.content.len().hash(&mut hasher);
            for item in &ol.content {
                hash_blocks(item, cache, &mut hasher);
            }
        }
        Block::BulletList(bl) => {
            bl.content.len().hash(&mut hasher);
            for item in &bl.content {
                hash_blocks(item, cache, &mut hasher);
            }
        }
        Block::DefinitionList(dl) => {
            dl.content.len().hash(&mut hasher);
            for (term, definitions) in &dl.content {
                hash_inlines(term, cache, &mut hasher);
                definitions.len().hash(&mut hasher);
                for def in definitions {
                    hash_blocks(def, cache, &mut hasher);
                }
            }
        }
        Block::Header(h) => {
            h.level.hash(&mut hasher);
            hash_attr(&h.attr, &mut hasher);
            hash_inlines(&h.content, cache, &mut hasher);
        }
        Block::HorizontalRule(_) => {
            // No content to hash beyond the discriminant
        }
        Block::Table(t) => {
            // Hash table structure
            hash_attr(&t.attr, &mut hasher);
            // Caption
            if let Some(short) = &t.caption.short {
                hash_inlines(short, cache, &mut hasher);
            }
            if let Some(long) = &t.caption.long {
                hash_blocks(long, cache, &mut hasher);
            }
            // ColSpecs
            t.colspec.len().hash(&mut hasher);
            for (align, width) in &t.colspec {
                std::mem::discriminant(align).hash(&mut hasher);
                std::mem::discriminant(width).hash(&mut hasher);
            }
            // Head
            hash_attr(&t.head.attr, &mut hasher);
            hash_table_rows(&t.head.rows, cache, &mut hasher);
            // Bodies
            t.bodies.len().hash(&mut hasher);
            for body in &t.bodies {
                hash_attr(&body.attr, &mut hasher);
                body.rowhead_columns.hash(&mut hasher);
                hash_table_rows(&body.head, cache, &mut hasher);
                hash_table_rows(&body.body, cache, &mut hasher);
            }
            // Foot
            hash_attr(&t.foot.attr, &mut hasher);
            hash_table_rows(&t.foot.rows, cache, &mut hasher);
        }
        Block::Figure(f) => {
            hash_attr(&f.attr, &mut hasher);
            // Caption
            if let Some(short) = &f.caption.short {
                hash_inlines(short, cache, &mut hasher);
            }
            if let Some(long) = &f.caption.long {
                hash_blocks(long, cache, &mut hasher);
            }
            // Content
            hash_blocks(&f.content, cache, &mut hasher);
        }
        Block::Div(d) => {
            hash_attr(&d.attr, &mut hasher);
            hash_blocks(&d.content, cache, &mut hasher);
        }
        Block::BlockMetadata(m) => {
            // Hash the meta value structure (simplified - just hash the debug repr)
            // A more sophisticated approach would recurse into MetaValue
            format!("{:?}", m.meta).hash(&mut hasher);
        }
        Block::NoteDefinitionPara(n) => {
            n.id.hash(&mut hasher);
            hash_inlines(&n.content, cache, &mut hasher);
        }
        Block::NoteDefinitionFencedBlock(n) => {
            n.id.hash(&mut hasher);
            hash_blocks(&n.content, cache, &mut hasher);
        }
        Block::CaptionBlock(c) => {
            hash_inlines(&c.content, cache, &mut hasher);
        }
        Block::Custom(cn) => {
            hash_custom_node(cn, cache, &mut hasher);
        }
    }

    hasher.finish()
}

/// Internal: compute inline hash with a cache reference.
fn compute_inline_hash_inner(inline: &Inline, cache: &mut HashCache<'_>) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();

    // Hash the discriminant (inline type)
    std::mem::discriminant(inline).hash(&mut hasher);

    // Hash content based on type
    match inline {
        Inline::Str(s) => {
            s.text.hash(&mut hasher);
        }
        Inline::Emph(e) => {
            hash_inlines(&e.content, cache, &mut hasher);
        }
        Inline::Underline(u) => {
            hash_inlines(&u.content, cache, &mut hasher);
        }
        Inline::Strong(s) => {
            hash_inlines(&s.content, cache, &mut hasher);
        }
        Inline::Strikeout(s) => {
            hash_inlines(&s.content, cache, &mut hasher);
        }
        Inline::Superscript(s) => {
            hash_inlines(&s.content, cache, &mut hasher);
        }
        Inline::Subscript(s) => {
            hash_inlines(&s.content, cache, &mut hasher);
        }
        Inline::SmallCaps(s) => {
            hash_inlines(&s.content, cache, &mut hasher);
        }
        Inline::Quoted(q) => {
            std::mem::discriminant(&q.quote_type).hash(&mut hasher);
            hash_inlines(&q.content, cache, &mut hasher);
        }
        Inline::Cite(c) => {
            c.citations.len().hash(&mut hasher);
            for citation in &c.citations {
                citation.id.hash(&mut hasher);
                hash_inlines(&citation.prefix, cache, &mut hasher);
                hash_inlines(&citation.suffix, cache, &mut hasher);
                std::mem::discriminant(&citation.mode).hash(&mut hasher);
            }
            hash_inlines(&c.content, cache, &mut hasher);
        }
        Inline::Code(c) => {
            hash_attr(&c.attr, &mut hasher);
            c.text.hash(&mut hasher);
        }
        Inline::Space(_) => {
            // No content beyond discriminant
        }
        Inline::SoftBreak(_) => {
            // No content beyond discriminant
        }
        Inline::LineBreak(_) => {
            // No content beyond discriminant
        }
        Inline::Math(m) => {
            std::mem::discriminant(&m.math_type).hash(&mut hasher);
            m.text.hash(&mut hasher);
        }
        Inline::RawInline(r) => {
            r.format.hash(&mut hasher);
            r.text.hash(&mut hasher);
        }
        Inline::Link(l) => {
            hash_attr(&l.attr, &mut hasher);
            hash_inlines(&l.content, cache, &mut hasher);
            l.target.0.hash(&mut hasher);
            l.target.1.hash(&mut hasher);
        }
        Inline::Image(i) => {
            hash_attr(&i.attr, &mut hasher);
            hash_inlines(&i.content, cache, &mut hasher);
            i.target.0.hash(&mut hasher);
            i.target.1.hash(&mut hasher);
        }
        Inline::Note(n) => {
            // Note contains Blocks
            hash_blocks(&n.content, cache, &mut hasher);
        }
        Inline::Span(s) => {
            hash_attr(&s.attr, &mut hasher);
            hash_inlines(&s.content, cache, &mut hasher);
        }
        Inline::Shortcode(sc) => {
            sc.name.hash(&mut hasher);
            sc.positional_args.len().hash(&mut hasher);
            // Just hash the debug repr for args (simplified)
            format!("{:?}", sc.positional_args).hash(&mut hasher);
            format!("{:?}", sc.keyword_args).hash(&mut hasher);
        }
        Inline::NoteReference(nr) => {
            nr.id.hash(&mut hasher);
        }
        Inline::Attr(a) => {
            hash_attr(&a.attr, &mut hasher);
        }
        Inline::Insert(i) => {
            hash_attr(&i.attr, &mut hasher);
            hash_inlines(&i.content, cache, &mut hasher);
        }
        Inline::Delete(d) => {
            hash_attr(&d.attr, &mut hasher);
            hash_inlines(&d.content, cache, &mut hasher);
        }
        Inline::Highlight(h) => {
            hash_attr(&h.attr, &mut hasher);
            hash_inlines(&h.content, cache, &mut hasher);
        }
        Inline::EditComment(e) => {
            hash_attr(&e.attr, &mut hasher);
            hash_inlines(&e.content, cache, &mut hasher);
        }
        Inline::Custom(cn) => {
            hash_custom_node(cn, cache, &mut hasher);
        }
    }

    hasher.finish()
}

/// Hash an attribute tuple (id, classes, key-value pairs).
fn hash_attr(attr: &Attr, hasher: &mut impl Hasher) {
    attr.0.hash(hasher); // id
    attr.1.len().hash(hasher);
    for class in &attr.1 {
        class.hash(hasher);
    }
    attr.2.len().hash(hasher);
    for (k, v) in &attr.2 {
        k.hash(hasher);
        v.hash(hasher);
    }
}

/// Hash a sequence of blocks.
fn hash_blocks(blocks: &[Block], cache: &mut HashCache<'_>, hasher: &mut impl Hasher) {
    blocks.len().hash(hasher);
    for block in blocks {
        // Compute hash fresh for each block since we can't tie lifetimes
        compute_block_hash_inner(block, cache).hash(hasher);
    }
}

/// Hash a sequence of inlines.
fn hash_inlines(inlines: &[Inline], cache: &mut HashCache<'_>, hasher: &mut impl Hasher) {
    inlines.len().hash(hasher);
    for inline in inlines {
        // Compute hash fresh for each inline since we can't tie lifetimes
        compute_inline_hash_inner(inline, cache).hash(hasher);
    }
}

/// Hash table rows.
fn hash_table_rows(
    rows: &[quarto_pandoc_types::table::Row],
    cache: &mut HashCache<'_>,
    hasher: &mut impl Hasher,
) {
    rows.len().hash(hasher);
    for row in rows {
        hash_attr(&row.attr, hasher);
        row.cells.len().hash(hasher);
        for cell in &row.cells {
            hash_attr(&cell.attr, hasher);
            std::mem::discriminant(&cell.alignment).hash(hasher);
            cell.row_span.hash(hasher);
            cell.col_span.hash(hasher);
            hash_blocks(&cell.content, cache, hasher);
        }
    }
}

/// Hash a CustomNode.
///
/// This hashes the type_name, attr, plain_data (via JSON serialization),
/// and all slots with their names and contents.
fn hash_custom_node(cn: &CustomNode, cache: &mut HashCache<'_>, hasher: &mut impl Hasher) {
    // Hash type name
    cn.type_name.hash(hasher);

    // Hash attr
    hash_attr(&cn.attr, hasher);

    // Hash plain_data via JSON serialization for canonical form
    // Using to_string for deterministic output
    if let Ok(json) = serde_json::to_string(&cn.plain_data) {
        json.hash(hasher);
    } else {
        // Fallback: hash the debug representation
        format!("{:?}", cn.plain_data).hash(hasher);
    }

    // Hash slots in order (LinkedHashMap preserves insertion order)
    cn.slots.len().hash(hasher);
    for (name, slot) in &cn.slots {
        name.hash(hasher);
        hash_slot(slot, cache, hasher);
    }
}

/// Hash a slot from a CustomNode.
fn hash_slot(slot: &Slot, cache: &mut HashCache<'_>, hasher: &mut impl Hasher) {
    // Hash discriminant to distinguish slot types
    std::mem::discriminant(slot).hash(hasher);

    match slot {
        Slot::Block(b) => {
            compute_block_hash_inner(b, cache).hash(hasher);
        }
        Slot::Inline(i) => {
            compute_inline_hash_inner(i, cache).hash(hasher);
        }
        Slot::Blocks(bs) => {
            hash_blocks(bs, cache, hasher);
        }
        Slot::Inlines(is) => {
            hash_inlines(is, cache, hasher);
        }
    }
}

// =============================================================================
// Meta (ConfigValue) Hashing
// =============================================================================
//
// Idempotence checks (Plan 3) need a structural hash of the document
// `meta` field that:
//
// - excludes `source_info` and `key_source` so Plan-4 source-info
//   churn doesn't affect the contract;
// - hashes `Map` entries in *insertion order* with no sort, so a
//   transform that stuffs a `HashMap` into meta is *detectable* (a
//   sort would silently mask that class of non-determinism — exactly
//   the bug an idempotence test is meant to catch);
// - includes `merge_op` so a transform that flips merge semantics
//   non-deterministically shows up;
// - recurses into `PandocInlines` / `PandocBlocks` via the existing
//   inline/block hashers (which already exclude source_info).

/// Compute a structural hash of a `ConfigValue` tree.
///
/// Source-info-agnostic: skips `ConfigValue::source_info` and
/// `ConfigMapEntry::key_source`. See module-level note above for the
/// design rationale (insertion-order maps, `merge_op` participates).
pub fn compute_meta_hash_fresh(meta: &ConfigValue) -> u64 {
    let mut cache = HashCache::new();
    let mut hasher = rustc_hash::FxHasher::default();
    hash_config_value(meta, &mut cache, &mut hasher);
    hasher.finish()
}

/// Compute a structural hash of a `ConfigValue` tree, excluding the
/// top-level `rendered` map entry.
///
/// Used by the q2-preview idempotence gate: chrome transforms
/// (navbar / sidebar / footer / page-nav), `IncludeResolveStage`, the
/// favicon transform, and the Bootstrap/clipboard injection stages
/// populate `meta.rendered.*` with HTML-string side outputs. Two
/// runs may produce HTML strings whose *bytes* differ but whose
/// rendered shape is equivalent (attribute order, whitespace); that
/// case belongs to an HTML-canonicalization concern, not to the
/// pipeline-determinism contract this hash defends.
///
/// The exclusion only applies at the document root. A `rendered`
/// key nested deeper in the tree is hashed normally — meta is
/// structured as a single top-level Map in practice, so a nested
/// `rendered` would be intentional content.
pub fn compute_meta_hash_fresh_excluding_rendered(meta: &ConfigValue) -> u64 {
    let mut cache = HashCache::new();
    let mut hasher = rustc_hash::FxHasher::default();
    hash_config_value_excluding(meta, &["rendered"], &mut cache, &mut hasher);
    hasher.finish()
}

fn hash_config_value(value: &ConfigValue, cache: &mut HashCache<'_>, hasher: &mut impl Hasher) {
    hash_config_value_excluding(value, &[], cache, hasher);
}

/// Hash a `ConfigValue`, optionally skipping certain top-level map
/// keys. `top_skip` is only consulted for the `Map` variant at this
/// call's root and is not propagated into recursion: nested values
/// see an empty skip list.
fn hash_config_value_excluding(
    value: &ConfigValue,
    top_skip: &[&str],
    cache: &mut HashCache<'_>,
    hasher: &mut impl Hasher,
) {
    // `merge_op` participates. The enum doesn't derive Hash, so
    // route through its discriminant + the byte tag.
    std::mem::discriminant(&value.merge_op).hash(hasher);

    hash_config_value_kind(&value.value, top_skip, cache, hasher);
}

fn hash_config_value_kind(
    kind: &ConfigValueKind,
    top_skip: &[&str],
    cache: &mut HashCache<'_>,
    hasher: &mut impl Hasher,
) {
    std::mem::discriminant(kind).hash(hasher);

    match kind {
        ConfigValueKind::Scalar(yaml) => {
            yaml.hash(hasher);
        }
        ConfigValueKind::PandocInlines(inlines) => {
            hash_inlines(inlines, cache, hasher);
        }
        ConfigValueKind::PandocBlocks(blocks) => {
            hash_blocks(blocks, cache, hasher);
        }
        ConfigValueKind::Path(s) | ConfigValueKind::Glob(s) | ConfigValueKind::Expr(s) => {
            s.hash(hasher);
        }
        ConfigValueKind::Array(items) => {
            items.len().hash(hasher);
            for item in items {
                hash_config_value(item, cache, hasher);
            }
        }
        ConfigValueKind::Map(entries) => {
            // Insertion-order, filtered by `top_skip`. Skip set is
            // intentionally NOT propagated into recursion.
            let kept_len = entries
                .iter()
                .filter(|e| !top_skip.contains(&e.key.as_str()))
                .count();
            kept_len.hash(hasher);
            for entry in entries {
                if top_skip.contains(&entry.key.as_str()) {
                    continue;
                }
                hash_config_map_entry(entry, cache, hasher);
            }
        }
    }
}

fn hash_config_map_entry(
    entry: &ConfigMapEntry,
    cache: &mut HashCache<'_>,
    hasher: &mut impl Hasher,
) {
    entry.key.hash(hasher);
    // `key_source` deliberately not hashed.
    hash_config_value(&entry.value, cache, hasher);
}

// =============================================================================
// Divergence Localization
// =============================================================================

/// First place two documents' structural hashes diverge.
///
/// Returned by [`find_first_divergence`] to make idempotence failures
/// debuggable: the test driver embeds this in its panic message so
/// the sub-agent investigation prompt arrives with "block index 7"
/// or "meta.listings.foo" instead of just "hash mismatch."
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DivergencePoint {
    /// Blocks at the same index hash differently. `path` is intentionally
    /// flat: we don't dig into block subtrees because the per-block hash
    /// already provides enough localization for triage.
    Block {
        index: usize,
        hash_a: u64,
        hash_b: u64,
    },
    /// A meta key path hashes differently. The path walks insertion
    /// order through nested Maps; the last element is the leaf key
    /// whose recursive hash diverges.
    MetaKey {
        path: Vec<String>,
        hash_a: u64,
        hash_b: u64,
    },
    /// The two documents' top-level hashes agree on both blocks and
    /// meta. The caller should never see this if it was reached via
    /// "hashes differ, find me a divergence" — it would indicate a
    /// hasher bug. Returned for completeness.
    None,
}

/// Find the first structural divergence between two documents.
///
/// `blocks` are compared in order by per-block fresh hash; the first
/// index whose hashes disagree yields a `Block` variant. If the
/// blocks all match, `meta` is walked in insertion order with the
/// same `rendered.*` exclusion the
/// [`compute_meta_hash_fresh_excluding_rendered`] hash uses; the
/// first map key whose recursive hash diverges yields a `MetaKey`
/// variant.
///
/// This lives in `quarto-ast-reconcile` next to the hashers so the
/// localization logic shares the source-info-exclusion contract by
/// construction. The caller (Plan 3's `idempotence.rs` test driver)
/// supplies `&[Block]` + `&ConfigValue` rather than passing the
/// crate's `DocumentAst` type, which is owned by `quarto-core`.
pub fn find_first_divergence(
    blocks_a: &[Block],
    meta_a: &ConfigValue,
    blocks_b: &[Block],
    meta_b: &ConfigValue,
) -> DivergencePoint {
    // Block walk: linear scan with the existing per-block hasher.
    // If block counts differ we still report the first mismatching
    // index (or the boundary index for the longer side).
    let common = blocks_a.len().min(blocks_b.len());
    for index in 0..common {
        let hash_a = compute_block_hash_fresh(&blocks_a[index]);
        let hash_b = compute_block_hash_fresh(&blocks_b[index]);
        if hash_a != hash_b {
            return DivergencePoint::Block {
                index,
                hash_a,
                hash_b,
            };
        }
    }
    if blocks_a.len() != blocks_b.len() {
        // Report the first "missing" position as a divergence at
        // index `common`. We synthesize a hash for the empty side as
        // 0 — it just needs to be observably different from the
        // present side's hash.
        let (hash_a, hash_b) = if blocks_a.len() > blocks_b.len() {
            (compute_block_hash_fresh(&blocks_a[common]), 0)
        } else {
            (0, compute_block_hash_fresh(&blocks_b[common]))
        };
        return DivergencePoint::Block {
            index: common,
            hash_a,
            hash_b,
        };
    }

    // Meta walk: recursive insertion-order traversal that excludes
    // top-level `rendered`. Matches the excluding-variant hash so a
    // failure reported here is reproducible from the hash itself.
    if let Some(point) = find_meta_divergence(meta_a, meta_b, &["rendered"], &mut Vec::new()) {
        return point;
    }

    DivergencePoint::None
}

fn find_meta_divergence(
    a: &ConfigValue,
    b: &ConfigValue,
    top_skip: &[&str],
    path: &mut Vec<String>,
) -> Option<DivergencePoint> {
    // Fast path: equal recursive hashes -> no divergence in this
    // subtree.
    let hash_a = meta_subtree_hash(a, top_skip);
    let hash_b = meta_subtree_hash(b, top_skip);
    if hash_a == hash_b {
        return None;
    }

    // Different. Drill down through Maps in insertion order; report
    // the deepest meaningful path.
    match (&a.value, &b.value) {
        (ConfigValueKind::Map(entries_a), ConfigValueKind::Map(entries_b)) => {
            for entry_a in entries_a {
                if top_skip.contains(&entry_a.key.as_str()) {
                    continue;
                }
                match entries_b.iter().find(|e| e.key == entry_a.key) {
                    Some(entry_b) => {
                        path.push(entry_a.key.clone());
                        if let Some(point) =
                            find_meta_divergence(&entry_a.value, &entry_b.value, &[], path)
                        {
                            return Some(point);
                        }
                        path.pop();
                    }
                    None => {
                        // Key present in `a`, missing in `b`. Report
                        // as a leaf divergence at this path.
                        let mut full = path.clone();
                        full.push(entry_a.key.clone());
                        return Some(DivergencePoint::MetaKey {
                            path: full,
                            hash_a: meta_subtree_hash(&entry_a.value, &[]),
                            hash_b: 0,
                        });
                    }
                }
            }
            // Any keys in `b` not in `a`?
            for entry_b in entries_b {
                if top_skip.contains(&entry_b.key.as_str()) {
                    continue;
                }
                if !entries_a.iter().any(|e| e.key == entry_b.key) {
                    let mut full = path.clone();
                    full.push(entry_b.key.clone());
                    return Some(DivergencePoint::MetaKey {
                        path: full,
                        hash_a: 0,
                        hash_b: meta_subtree_hash(&entry_b.value, &[]),
                    });
                }
            }
            // Hashes differed but no key-level divergence found
            // (e.g. value of a present key changed but the recursion
            // bottomed out without finding a Map to descend into):
            // report at the current path.
            Some(DivergencePoint::MetaKey {
                path: path.clone(),
                hash_a,
                hash_b,
            })
        }
        _ => Some(DivergencePoint::MetaKey {
            path: path.clone(),
            hash_a,
            hash_b,
        }),
    }
}

fn meta_subtree_hash(value: &ConfigValue, top_skip: &[&str]) -> u64 {
    let mut cache = HashCache::new();
    let mut hasher = rustc_hash::FxHasher::default();
    hash_config_value_excluding(value, top_skip, &mut cache, &mut hasher);
    hasher.finish()
}

// =============================================================================
// Structural Equality (for hash collision verification)
// =============================================================================

/// Check structural equality of two blocks, ignoring source locations.
pub fn structural_eq_block(a: &Block, b: &Block) -> bool {
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return false;
    }

    match (a, b) {
        (Block::Plain(a), Block::Plain(b)) => structural_eq_inlines(&a.content, &b.content),
        (Block::Paragraph(a), Block::Paragraph(b)) => structural_eq_inlines(&a.content, &b.content),
        (Block::LineBlock(a), Block::LineBlock(b)) => {
            a.content.len() == b.content.len()
                && a.content
                    .iter()
                    .zip(&b.content)
                    .all(|(a, b)| structural_eq_inlines(a, b))
        }
        (Block::CodeBlock(a), Block::CodeBlock(b)) => attr_eq(&a.attr, &b.attr) && a.text == b.text,
        (Block::RawBlock(a), Block::RawBlock(b)) => a.format == b.format && a.text == b.text,
        (Block::BlockQuote(a), Block::BlockQuote(b)) => {
            structural_eq_blocks(&a.content, &b.content)
        }
        (Block::OrderedList(a), Block::OrderedList(b)) => {
            a.attr == b.attr
                && a.content.len() == b.content.len()
                && a.content
                    .iter()
                    .zip(&b.content)
                    .all(|(a, b)| structural_eq_blocks(a, b))
        }
        (Block::BulletList(a), Block::BulletList(b)) => {
            a.content.len() == b.content.len()
                && a.content
                    .iter()
                    .zip(&b.content)
                    .all(|(a, b)| structural_eq_blocks(a, b))
        }
        (Block::DefinitionList(a), Block::DefinitionList(b)) => {
            a.content.len() == b.content.len()
                && a.content
                    .iter()
                    .zip(&b.content)
                    .all(|((ta, da), (tb, db))| {
                        structural_eq_inlines(ta, tb)
                            && da.len() == db.len()
                            && da.iter().zip(db).all(|(a, b)| structural_eq_blocks(a, b))
                    })
        }
        (Block::Header(a), Block::Header(b)) => {
            a.level == b.level
                && attr_eq(&a.attr, &b.attr)
                && structural_eq_inlines(&a.content, &b.content)
        }
        (Block::HorizontalRule(_), Block::HorizontalRule(_)) => true,
        (Block::Table(a), Block::Table(b)) => {
            // Simplified table comparison
            attr_eq(&a.attr, &b.attr)
                && a.colspec == b.colspec
                && option_blocks_eq(&a.caption.long, &b.caption.long)
        }
        (Block::Figure(a), Block::Figure(b)) => {
            attr_eq(&a.attr, &b.attr)
                && option_blocks_eq(&a.caption.long, &b.caption.long)
                && structural_eq_blocks(&a.content, &b.content)
        }
        (Block::Div(a), Block::Div(b)) => {
            attr_eq(&a.attr, &b.attr) && structural_eq_blocks(&a.content, &b.content)
        }
        (Block::BlockMetadata(a), Block::BlockMetadata(b)) => {
            // Simplified: compare debug repr
            format!("{:?}", a.meta) == format!("{:?}", b.meta)
        }
        (Block::NoteDefinitionPara(a), Block::NoteDefinitionPara(b)) => {
            a.id == b.id && structural_eq_inlines(&a.content, &b.content)
        }
        (Block::NoteDefinitionFencedBlock(a), Block::NoteDefinitionFencedBlock(b)) => {
            a.id == b.id && structural_eq_blocks(&a.content, &b.content)
        }
        (Block::CaptionBlock(a), Block::CaptionBlock(b)) => {
            structural_eq_inlines(&a.content, &b.content)
        }
        (Block::Custom(a), Block::Custom(b)) => structural_eq_custom_node(a, b),
        _ => false,
    }
}

/// Check structural equality of two inlines, ignoring source locations.
pub fn structural_eq_inline(a: &Inline, b: &Inline) -> bool {
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return false;
    }

    match (a, b) {
        (Inline::Str(a), Inline::Str(b)) => a.text == b.text,
        (Inline::Emph(a), Inline::Emph(b)) => structural_eq_inlines(&a.content, &b.content),
        (Inline::Underline(a), Inline::Underline(b)) => {
            structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Strong(a), Inline::Strong(b)) => structural_eq_inlines(&a.content, &b.content),
        (Inline::Strikeout(a), Inline::Strikeout(b)) => {
            structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Superscript(a), Inline::Superscript(b)) => {
            structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Subscript(a), Inline::Subscript(b)) => {
            structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::SmallCaps(a), Inline::SmallCaps(b)) => {
            structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Quoted(a), Inline::Quoted(b)) => {
            a.quote_type == b.quote_type && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Cite(a), Inline::Cite(b)) => {
            a.citations.len() == b.citations.len()
                && a.citations.iter().zip(&b.citations).all(|(a, b)| {
                    a.id == b.id
                        && a.mode == b.mode
                        && structural_eq_inlines(&a.prefix, &b.prefix)
                        && structural_eq_inlines(&a.suffix, &b.suffix)
                })
                && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Code(a), Inline::Code(b)) => attr_eq(&a.attr, &b.attr) && a.text == b.text,
        (Inline::Space(_), Inline::Space(_)) => true,
        (Inline::SoftBreak(_), Inline::SoftBreak(_)) => true,
        (Inline::LineBreak(_), Inline::LineBreak(_)) => true,
        (Inline::Math(a), Inline::Math(b)) => a.math_type == b.math_type && a.text == b.text,
        (Inline::RawInline(a), Inline::RawInline(b)) => a.format == b.format && a.text == b.text,
        (Inline::Link(a), Inline::Link(b)) => {
            attr_eq(&a.attr, &b.attr)
                && a.target == b.target
                && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Image(a), Inline::Image(b)) => {
            attr_eq(&a.attr, &b.attr)
                && a.target == b.target
                && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Note(a), Inline::Note(b)) => structural_eq_blocks(&a.content, &b.content),
        (Inline::Span(a), Inline::Span(b)) => {
            attr_eq(&a.attr, &b.attr) && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Shortcode(a), Inline::Shortcode(b)) => {
            a.name == b.name
                && a.positional_args == b.positional_args
                && a.keyword_args == b.keyword_args
        }
        (Inline::NoteReference(a), Inline::NoteReference(b)) => a.id == b.id,
        (Inline::Attr(a), Inline::Attr(b)) => attr_eq(&a.attr, &b.attr),
        (Inline::Insert(a), Inline::Insert(b)) => {
            attr_eq(&a.attr, &b.attr) && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Delete(a), Inline::Delete(b)) => {
            attr_eq(&a.attr, &b.attr) && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Highlight(a), Inline::Highlight(b)) => {
            attr_eq(&a.attr, &b.attr) && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::EditComment(a), Inline::EditComment(b)) => {
            attr_eq(&a.attr, &b.attr) && structural_eq_inlines(&a.content, &b.content)
        }
        (Inline::Custom(a), Inline::Custom(b)) => structural_eq_custom_node(a, b),
        _ => false,
    }
}

/// Check structural equality of block sequences.
pub fn structural_eq_blocks(a: &[Block], b: &[Block]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| structural_eq_block(a, b))
}

/// Check structural equality of inline sequences.
pub fn structural_eq_inlines(a: &[Inline], b: &[Inline]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| structural_eq_inline(a, b))
}

/// Check equality of optional block sequences.
fn option_blocks_eq(a: &Option<Vec<Block>>, b: &Option<Vec<Block>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => structural_eq_blocks(a, b),
        _ => false,
    }
}

/// Check structural equality of two CustomNodes.
pub fn structural_eq_custom_node(a: &CustomNode, b: &CustomNode) -> bool {
    // Type name must match
    if a.type_name != b.type_name {
        return false;
    }

    // Attr must match
    if !attr_eq(&a.attr, &b.attr) {
        return false;
    }

    // Plain data must match
    if a.plain_data != b.plain_data {
        return false;
    }

    // Slots must match by name and content
    if a.slots.len() != b.slots.len() {
        return false;
    }

    for (name, slot_a) in &a.slots {
        let Some(slot_b) = b.slots.get(name) else {
            return false;
        };
        if !structural_eq_slot(slot_a, slot_b) {
            return false;
        }
    }

    true
}

/// Check structural equality of two slots.
pub fn structural_eq_slot(a: &Slot, b: &Slot) -> bool {
    match (a, b) {
        (Slot::Block(a), Slot::Block(b)) => structural_eq_block(a, b),
        (Slot::Inline(a), Slot::Inline(b)) => structural_eq_inline(a, b),
        (Slot::Blocks(a), Slot::Blocks(b)) => structural_eq_blocks(a, b),
        (Slot::Inlines(a), Slot::Inlines(b)) => structural_eq_inlines(a, b),
        _ => false, // Different slot types
    }
}

/// Check attribute equality (ignoring source info which isn't in Attr itself).
fn attr_eq(a: &Attr, b: &Attr) -> bool {
    a.0 == b.0 && a.1 == b.1 && a.2 == b.2
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::custom::{CustomNode, Slot};
    use quarto_pandoc_types::{
        AttrSourceInfo, BlockQuote, BulletList, Code, CodeBlock, DefinitionList, Div, Emph, Header,
        HorizontalRule, Image, LineBlock, LineBreak, Link, Math, MathType, Note, OrderedList,
        Paragraph, Plain, QuoteType, Quoted, RawBlock, RawInline, SmallCaps, SoftBreak, Space,
        Span, Str, Strikeout, Strong, Subscript, Superscript, TargetSourceInfo, Underline,
    };
    use quarto_pandoc_types::{ListNumberDelim, ListNumberStyle};
    use quarto_source_map::{FileId, SourceInfo};

    fn dummy_source() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn other_source() -> SourceInfo {
        SourceInfo::original(FileId(1), 100, 200)
    }

    fn empty_attr() -> Attr {
        (String::new(), vec![], LinkedHashMap::new())
    }

    fn make_str(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source(),
        })
    }

    // ==================== Basic Hash Tests ====================

    #[test]
    fn test_same_content_same_hash() {
        let block1 = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let block2 = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: other_source(), // Different source location
            })],
            source_info: other_source(), // Different source location
        });

        assert_eq!(
            compute_block_hash_fresh(&block1),
            compute_block_hash_fresh(&block2)
        );
    }

    #[test]
    fn test_different_content_different_hash() {
        let block1 = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let block2 = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "world".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        assert_ne!(
            compute_block_hash_fresh(&block1),
            compute_block_hash_fresh(&block2)
        );
    }

    #[test]
    fn test_different_type_different_hash() {
        let para = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: dummy_source(),
        });

        let plain = Block::Plain(Plain {
            content: vec![],
            source_info: dummy_source(),
        });

        assert_ne!(
            compute_block_hash_fresh(&para),
            compute_block_hash_fresh(&plain)
        );
    }

    #[test]
    fn test_cache_returns_same_hash() {
        let block = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let mut cache = HashCache::new();
        let hash1 = cache.hash_block(&block);
        let hash2 = cache.hash_block(&block);

        assert_eq!(hash1, hash2);
    }

    // ==================== Block Type Hash Tests ====================

    #[test]
    fn test_hash_code_block() {
        let cb1 = Block::CodeBlock(CodeBlock {
            attr: (
                "id".to_string(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "print('hello')".to_string(),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let cb2 = Block::CodeBlock(CodeBlock {
            attr: (
                "id".to_string(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "print('hello')".to_string(),
            source_info: other_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let cb3 = Block::CodeBlock(CodeBlock {
            attr: (
                "id".to_string(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "print('world')".to_string(),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        // Same content, different source -> same hash
        assert_eq!(
            compute_block_hash_fresh(&cb1),
            compute_block_hash_fresh(&cb2)
        );

        // Different text -> different hash
        assert_ne!(
            compute_block_hash_fresh(&cb1),
            compute_block_hash_fresh(&cb3)
        );
    }

    #[test]
    fn test_hash_raw_block() {
        let rb1 = Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: "<div>hello</div>".to_string(),
            source_info: dummy_source(),
        });

        let rb2 = Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: "<div>world</div>".to_string(),
            source_info: dummy_source(),
        });

        assert_ne!(
            compute_block_hash_fresh(&rb1),
            compute_block_hash_fresh(&rb2)
        );
    }

    #[test]
    fn test_hash_block_quote() {
        let bq1 = Block::BlockQuote(BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("quoted")],
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let bq2 = Block::BlockQuote(BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("quoted")],
                source_info: other_source(),
            })],
            source_info: other_source(),
        });

        // Same content -> same hash
        assert_eq!(
            compute_block_hash_fresh(&bq1),
            compute_block_hash_fresh(&bq2)
        );
    }

    #[test]
    fn test_hash_ordered_list() {
        let ol1 = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![vec![Block::Paragraph(Paragraph {
                content: vec![make_str("item 1")],
                source_info: dummy_source(),
            })]],
            source_info: dummy_source(),
        });

        let ol2 = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![vec![Block::Paragraph(Paragraph {
                content: vec![make_str("item 1")],
                source_info: other_source(),
            })]],
            source_info: other_source(),
        });

        assert_eq!(
            compute_block_hash_fresh(&ol1),
            compute_block_hash_fresh(&ol2)
        );
    }

    #[test]
    fn test_hash_bullet_list() {
        let bl1 = Block::BulletList(BulletList {
            content: vec![
                vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("item 1")],
                    source_info: dummy_source(),
                })],
                vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("item 2")],
                    source_info: dummy_source(),
                })],
            ],
            source_info: dummy_source(),
        });

        let bl2 = Block::BulletList(BulletList {
            content: vec![vec![Block::Paragraph(Paragraph {
                content: vec![make_str("item 1")],
                source_info: dummy_source(),
            })]],
            source_info: dummy_source(),
        });

        // Different number of items -> different hash
        assert_ne!(
            compute_block_hash_fresh(&bl1),
            compute_block_hash_fresh(&bl2)
        );
    }

    #[test]
    fn test_hash_definition_list() {
        let dl = Block::DefinitionList(DefinitionList {
            content: vec![(
                vec![make_str("term")],
                vec![vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("definition")],
                    source_info: dummy_source(),
                })]],
            )],
            source_info: dummy_source(),
        });

        let hash = compute_block_hash_fresh(&dl);
        // Just verify it doesn't panic and produces a hash
        let _ = hash; // Ensure hash is computed
    }

    #[test]
    fn test_hash_header() {
        let h1 = Block::Header(Header {
            level: 1,
            attr: empty_attr(),
            content: vec![make_str("Title")],
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let h2 = Block::Header(Header {
            level: 2,
            attr: empty_attr(),
            content: vec![make_str("Title")],
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        // Different level -> different hash
        assert_ne!(compute_block_hash_fresh(&h1), compute_block_hash_fresh(&h2));
    }

    #[test]
    fn test_hash_horizontal_rule() {
        let hr1 = Block::HorizontalRule(HorizontalRule {
            source_info: dummy_source(),
        });

        let hr2 = Block::HorizontalRule(HorizontalRule {
            source_info: other_source(),
        });

        // Same type, no content -> same hash
        assert_eq!(
            compute_block_hash_fresh(&hr1),
            compute_block_hash_fresh(&hr2)
        );
    }

    #[test]
    fn test_hash_div() {
        let div1 = Block::Div(Div {
            attr: (
                "my-div".to_string(),
                vec!["note".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("content")],
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let div2 = Block::Div(Div {
            attr: (
                "my-div".to_string(),
                vec!["note".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("different")],
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        // Different content -> different hash
        assert_ne!(
            compute_block_hash_fresh(&div1),
            compute_block_hash_fresh(&div2)
        );
    }

    #[test]
    fn test_hash_line_block() {
        let lb = Block::LineBlock(LineBlock {
            content: vec![vec![make_str("line 1")], vec![make_str("line 2")]],
            source_info: dummy_source(),
        });

        let hash = compute_block_hash_fresh(&lb);
        let _ = hash;
    }

    // ==================== Inline Type Hash Tests ====================

    #[test]
    fn test_hash_inline_emph() {
        let emph1 = Inline::Emph(Emph {
            content: vec![make_str("emphasized")],
            source_info: dummy_source(),
        });

        let emph2 = Inline::Emph(Emph {
            content: vec![make_str("emphasized")],
            source_info: other_source(),
        });

        assert_eq!(
            compute_inline_hash_fresh(&emph1),
            compute_inline_hash_fresh(&emph2)
        );
    }

    #[test]
    fn test_hash_inline_strong() {
        let strong = Inline::Strong(Strong {
            content: vec![make_str("bold")],
            source_info: dummy_source(),
        });

        let hash = compute_inline_hash_fresh(&strong);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_underline() {
        let u = Inline::Underline(Underline {
            content: vec![make_str("underlined")],
            source_info: dummy_source(),
        });

        let hash = compute_inline_hash_fresh(&u);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_strikeout() {
        let s = Inline::Strikeout(Strikeout {
            content: vec![make_str("deleted")],
            source_info: dummy_source(),
        });

        let hash = compute_inline_hash_fresh(&s);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_superscript() {
        let sup = Inline::Superscript(Superscript {
            content: vec![make_str("2")],
            source_info: dummy_source(),
        });

        let hash = compute_inline_hash_fresh(&sup);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_subscript() {
        let sub = Inline::Subscript(Subscript {
            content: vec![make_str("i")],
            source_info: dummy_source(),
        });

        let hash = compute_inline_hash_fresh(&sub);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_smallcaps() {
        let sc = Inline::SmallCaps(SmallCaps {
            content: vec![make_str("text")],
            source_info: dummy_source(),
        });

        let hash = compute_inline_hash_fresh(&sc);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_quoted() {
        let q1 = Inline::Quoted(Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![make_str("quoted")],
            source_info: dummy_source(),
        });

        let q2 = Inline::Quoted(Quoted {
            quote_type: QuoteType::SingleQuote,
            content: vec![make_str("quoted")],
            source_info: dummy_source(),
        });

        // Different quote type -> different hash
        assert_ne!(
            compute_inline_hash_fresh(&q1),
            compute_inline_hash_fresh(&q2)
        );
    }

    #[test]
    fn test_hash_inline_code() {
        let code1 = Inline::Code(Code {
            attr: empty_attr(),
            text: "x = 1".to_string(),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let code2 = Inline::Code(Code {
            attr: empty_attr(),
            text: "x = 2".to_string(),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        assert_ne!(
            compute_inline_hash_fresh(&code1),
            compute_inline_hash_fresh(&code2)
        );
    }

    #[test]
    fn test_hash_inline_space_softbreak_linebreak() {
        let space = Inline::Space(Space {
            source_info: dummy_source(),
        });
        let softbreak = Inline::SoftBreak(SoftBreak {
            source_info: dummy_source(),
        });
        let linebreak = Inline::LineBreak(LineBreak {
            source_info: dummy_source(),
        });

        // All should have different hashes due to different discriminants
        let h1 = compute_inline_hash_fresh(&space);
        let h2 = compute_inline_hash_fresh(&softbreak);
        let h3 = compute_inline_hash_fresh(&linebreak);

        assert_ne!(h1, h2);
        assert_ne!(h2, h3);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_hash_inline_math() {
        let math1 = Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x^2".to_string(),
            source_info: dummy_source(),
        });

        let math2 = Inline::Math(Math {
            math_type: MathType::DisplayMath,
            text: "x^2".to_string(),
            source_info: dummy_source(),
        });

        // Different math type -> different hash
        assert_ne!(
            compute_inline_hash_fresh(&math1),
            compute_inline_hash_fresh(&math2)
        );
    }

    #[test]
    fn test_hash_inline_raw() {
        let raw = Inline::RawInline(RawInline {
            format: "tex".to_string(),
            text: "\\alpha".to_string(),
            source_info: dummy_source(),
        });

        let hash = compute_inline_hash_fresh(&raw);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_link() {
        let link1 = Inline::Link(Link {
            attr: empty_attr(),
            content: vec![make_str("click")],
            target: ("https://example.com".to_string(), "title".to_string()),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });

        let link2 = Inline::Link(Link {
            attr: empty_attr(),
            content: vec![make_str("click")],
            target: ("https://other.com".to_string(), "title".to_string()),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });

        // Different URL -> different hash
        assert_ne!(
            compute_inline_hash_fresh(&link1),
            compute_inline_hash_fresh(&link2)
        );
    }

    #[test]
    fn test_hash_inline_image() {
        let img = Inline::Image(Image {
            attr: empty_attr(),
            content: vec![make_str("alt text")],
            target: ("image.png".to_string(), String::new()),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });

        let hash = compute_inline_hash_fresh(&img);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_note() {
        let note = Inline::Note(Note {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("footnote")],
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let hash = compute_inline_hash_fresh(&note);
        let _ = hash;
    }

    #[test]
    fn test_hash_inline_span() {
        let span = Inline::Span(Span {
            attr: (
                "id".to_string(),
                vec!["class".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![make_str("spanned")],
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let hash = compute_inline_hash_fresh(&span);
        let _ = hash;
    }

    // ==================== Cache Tests ====================

    #[test]
    fn test_inline_cache_returns_same_hash() {
        let inline = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: dummy_source(),
        });

        let mut cache = HashCache::new();
        let hash1 = cache.hash_inline(&inline);
        let hash2 = cache.hash_inline(&inline);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_cache_default() {
        let cache: HashCache = Default::default();
        assert!(cache.cache.is_empty());
    }

    // ==================== Structural Equality Tests ====================

    #[test]
    fn test_structural_eq_ignores_source() {
        let block1 = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let block2 = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: other_source(),
            })],
            source_info: other_source(),
        });

        assert!(structural_eq_block(&block1, &block2));
    }

    #[test]
    fn test_structural_eq_different_content() {
        let block1 = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let block2 = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "world".to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        assert!(!structural_eq_block(&block1, &block2));
    }

    #[test]
    fn test_structural_eq_different_types() {
        let para = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: dummy_source(),
        });

        let plain = Block::Plain(Plain {
            content: vec![],
            source_info: dummy_source(),
        });

        assert!(!structural_eq_block(&para, &plain));
    }

    #[test]
    fn test_structural_eq_code_block() {
        let cb1 = Block::CodeBlock(CodeBlock {
            attr: (
                String::new(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "code".to_string(),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let cb2 = Block::CodeBlock(CodeBlock {
            attr: (
                String::new(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "code".to_string(),
            source_info: other_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let cb3 = Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec!["r".to_string()], LinkedHashMap::new()),
            text: "code".to_string(),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        assert!(structural_eq_block(&cb1, &cb2));
        assert!(!structural_eq_block(&cb1, &cb3)); // Different class
    }

    #[test]
    fn test_structural_eq_raw_block() {
        let rb1 = Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: "<div>".to_string(),
            source_info: dummy_source(),
        });

        let rb2 = Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: "<div>".to_string(),
            source_info: other_source(),
        });

        assert!(structural_eq_block(&rb1, &rb2));
    }

    #[test]
    fn test_structural_eq_block_quote() {
        let bq1 = Block::BlockQuote(BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("quote")],
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let bq2 = Block::BlockQuote(BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("quote")],
                source_info: other_source(),
            })],
            source_info: other_source(),
        });

        assert!(structural_eq_block(&bq1, &bq2));
    }

    #[test]
    fn test_structural_eq_ordered_list() {
        let ol1 = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![vec![Block::Paragraph(Paragraph {
                content: vec![make_str("item")],
                source_info: dummy_source(),
            })]],
            source_info: dummy_source(),
        });

        let ol2 = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![vec![Block::Paragraph(Paragraph {
                content: vec![make_str("item")],
                source_info: other_source(),
            })]],
            source_info: other_source(),
        });

        let ol3 = Block::OrderedList(OrderedList {
            attr: (5, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![vec![Block::Paragraph(Paragraph {
                content: vec![make_str("item")],
                source_info: dummy_source(),
            })]],
            source_info: dummy_source(),
        });

        assert!(structural_eq_block(&ol1, &ol2));
        assert!(!structural_eq_block(&ol1, &ol3)); // Different start number
    }

    #[test]
    fn test_structural_eq_bullet_list() {
        let bl1 = Block::BulletList(BulletList {
            content: vec![vec![Block::Paragraph(Paragraph {
                content: vec![make_str("item")],
                source_info: dummy_source(),
            })]],
            source_info: dummy_source(),
        });

        let bl2 = Block::BulletList(BulletList {
            content: vec![vec![Block::Paragraph(Paragraph {
                content: vec![make_str("item")],
                source_info: other_source(),
            })]],
            source_info: other_source(),
        });

        assert!(structural_eq_block(&bl1, &bl2));
    }

    #[test]
    fn test_structural_eq_definition_list() {
        let dl1 = Block::DefinitionList(DefinitionList {
            content: vec![(
                vec![make_str("term")],
                vec![vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("def")],
                    source_info: dummy_source(),
                })]],
            )],
            source_info: dummy_source(),
        });

        let dl2 = Block::DefinitionList(DefinitionList {
            content: vec![(
                vec![make_str("term")],
                vec![vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("def")],
                    source_info: other_source(),
                })]],
            )],
            source_info: other_source(),
        });

        assert!(structural_eq_block(&dl1, &dl2));
    }

    #[test]
    fn test_structural_eq_header() {
        let h1 = Block::Header(Header {
            level: 1,
            attr: empty_attr(),
            content: vec![make_str("Title")],
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let h2 = Block::Header(Header {
            level: 1,
            attr: empty_attr(),
            content: vec![make_str("Title")],
            source_info: other_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let h3 = Block::Header(Header {
            level: 2,
            attr: empty_attr(),
            content: vec![make_str("Title")],
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        assert!(structural_eq_block(&h1, &h2));
        assert!(!structural_eq_block(&h1, &h3)); // Different level
    }

    #[test]
    fn test_structural_eq_horizontal_rule() {
        let hr1 = Block::HorizontalRule(HorizontalRule {
            source_info: dummy_source(),
        });

        let hr2 = Block::HorizontalRule(HorizontalRule {
            source_info: other_source(),
        });

        assert!(structural_eq_block(&hr1, &hr2));
    }

    #[test]
    fn test_structural_eq_div() {
        let div1 = Block::Div(Div {
            attr: ("id".to_string(), vec![], LinkedHashMap::new()),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("text")],
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let div2 = Block::Div(Div {
            attr: ("id".to_string(), vec![], LinkedHashMap::new()),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("text")],
                source_info: other_source(),
            })],
            source_info: other_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        assert!(structural_eq_block(&div1, &div2));
    }

    #[test]
    fn test_structural_eq_line_block() {
        let lb1 = Block::LineBlock(LineBlock {
            content: vec![vec![make_str("line")]],
            source_info: dummy_source(),
        });

        let lb2 = Block::LineBlock(LineBlock {
            content: vec![vec![make_str("line")]],
            source_info: other_source(),
        });

        assert!(structural_eq_block(&lb1, &lb2));
    }

    // ==================== Inline Structural Equality Tests ====================

    #[test]
    fn test_structural_eq_inline_str() {
        let s1 = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: dummy_source(),
        });

        let s2 = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: other_source(),
        });

        assert!(structural_eq_inline(&s1, &s2));
    }

    #[test]
    fn test_structural_eq_inline_emph() {
        let e1 = Inline::Emph(Emph {
            content: vec![make_str("text")],
            source_info: dummy_source(),
        });

        let e2 = Inline::Emph(Emph {
            content: vec![make_str("text")],
            source_info: other_source(),
        });

        assert!(structural_eq_inline(&e1, &e2));
    }

    #[test]
    fn test_structural_eq_inline_code() {
        let c1 = Inline::Code(Code {
            attr: empty_attr(),
            text: "code".to_string(),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        let c2 = Inline::Code(Code {
            attr: empty_attr(),
            text: "code".to_string(),
            source_info: other_source(),
            attr_source: AttrSourceInfo::empty(),
        });

        assert!(structural_eq_inline(&c1, &c2));
    }

    #[test]
    fn test_structural_eq_inline_space() {
        let s1 = Inline::Space(Space {
            source_info: dummy_source(),
        });

        let s2 = Inline::Space(Space {
            source_info: other_source(),
        });

        assert!(structural_eq_inline(&s1, &s2));
    }

    #[test]
    fn test_structural_eq_inline_math() {
        let m1 = Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x".to_string(),
            source_info: dummy_source(),
        });

        let m2 = Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x".to_string(),
            source_info: other_source(),
        });

        let m3 = Inline::Math(Math {
            math_type: MathType::DisplayMath,
            text: "x".to_string(),
            source_info: dummy_source(),
        });

        assert!(structural_eq_inline(&m1, &m2));
        assert!(!structural_eq_inline(&m1, &m3)); // Different math type
    }

    #[test]
    fn test_structural_eq_inline_link() {
        let l1 = Inline::Link(Link {
            attr: empty_attr(),
            content: vec![make_str("text")],
            target: ("url".to_string(), "title".to_string()),
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });

        let l2 = Inline::Link(Link {
            attr: empty_attr(),
            content: vec![make_str("text")],
            target: ("url".to_string(), "title".to_string()),
            source_info: other_source(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });

        assert!(structural_eq_inline(&l1, &l2));
    }

    #[test]
    fn test_structural_eq_inline_note() {
        let n1 = Inline::Note(Note {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("note")],
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        });

        let n2 = Inline::Note(Note {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("note")],
                source_info: other_source(),
            })],
            source_info: other_source(),
        });

        assert!(structural_eq_inline(&n1, &n2));
    }

    // ==================== Helper Function Tests ====================

    #[test]
    fn test_structural_eq_blocks() {
        let blocks1 = vec![
            Block::Paragraph(Paragraph {
                content: vec![make_str("a")],
                source_info: dummy_source(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![make_str("b")],
                source_info: dummy_source(),
            }),
        ];

        let blocks2 = vec![
            Block::Paragraph(Paragraph {
                content: vec![make_str("a")],
                source_info: other_source(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![make_str("b")],
                source_info: other_source(),
            }),
        ];

        assert!(structural_eq_blocks(&blocks1, &blocks2));
    }

    #[test]
    fn test_structural_eq_inlines() {
        let inlines1 = vec![make_str("hello"), make_str("world")];
        let inlines2 = vec![
            Inline::Str(Str {
                text: "hello".to_string(),
                source_info: other_source(),
            }),
            Inline::Str(Str {
                text: "world".to_string(),
                source_info: other_source(),
            }),
        ];

        assert!(structural_eq_inlines(&inlines1, &inlines2));
    }

    #[test]
    fn test_attr_eq() {
        let a1: Attr = (
            "id".to_string(),
            vec!["class".to_string()],
            LinkedHashMap::new(),
        );
        let a2: Attr = (
            "id".to_string(),
            vec!["class".to_string()],
            LinkedHashMap::new(),
        );
        let a3: Attr = (
            "other".to_string(),
            vec!["class".to_string()],
            LinkedHashMap::new(),
        );

        assert!(attr_eq(&a1, &a2));
        assert!(!attr_eq(&a1, &a3));
    }

    // ==================== CustomNode Tests ====================

    #[test]
    fn test_hash_custom_node() {
        let cn = CustomNode {
            type_name: "Callout".to_string(),
            attr: empty_attr(),
            plain_data: serde_json::json!({"type": "note"}),
            slots: LinkedHashMap::new(),
            source_info: dummy_source(),
        };

        let block = Block::Custom(cn);
        let hash = compute_block_hash_fresh(&block);
        let _ = hash;
    }

    #[test]
    fn test_hash_custom_node_with_slots() {
        let mut slots = LinkedHashMap::new();
        slots.insert(
            "content".to_string(),
            Slot::Blocks(vec![Block::Paragraph(Paragraph {
                content: vec![make_str("callout content")],
                source_info: dummy_source(),
            })]),
        );

        let cn = CustomNode {
            type_name: "Callout".to_string(),
            attr: empty_attr(),
            plain_data: serde_json::json!({}),
            slots,
            source_info: dummy_source(),
        };

        let block = Block::Custom(cn);
        let hash = compute_block_hash_fresh(&block);
        let _ = hash;
    }

    #[test]
    fn test_structural_eq_custom_node() {
        let cn1 = CustomNode {
            type_name: "Callout".to_string(),
            attr: empty_attr(),
            plain_data: serde_json::json!({"type": "note"}),
            slots: LinkedHashMap::new(),
            source_info: dummy_source(),
        };

        let cn2 = CustomNode {
            type_name: "Callout".to_string(),
            attr: empty_attr(),
            plain_data: serde_json::json!({"type": "note"}),
            slots: LinkedHashMap::new(),
            source_info: other_source(),
        };

        let cn3 = CustomNode {
            type_name: "Callout".to_string(),
            attr: empty_attr(),
            plain_data: serde_json::json!({"type": "warning"}),
            slots: LinkedHashMap::new(),
            source_info: dummy_source(),
        };

        assert!(structural_eq_custom_node(&cn1, &cn2));
        assert!(!structural_eq_custom_node(&cn1, &cn3)); // Different plain_data
    }

    #[test]
    fn test_structural_eq_slot() {
        let slot1 = Slot::Block(Box::new(Block::Paragraph(Paragraph {
            content: vec![make_str("text")],
            source_info: dummy_source(),
        })));

        let slot2 = Slot::Block(Box::new(Block::Paragraph(Paragraph {
            content: vec![make_str("text")],
            source_info: other_source(),
        })));

        let slot3 = Slot::Inline(Box::new(make_str("text")));

        assert!(structural_eq_slot(&slot1, &slot2));
        assert!(!structural_eq_slot(&slot1, &slot3)); // Different slot type
    }

    #[test]
    fn test_structural_eq_slot_blocks() {
        let slot1 = Slot::Blocks(vec![Block::Paragraph(Paragraph {
            content: vec![make_str("text")],
            source_info: dummy_source(),
        })]);

        let slot2 = Slot::Blocks(vec![Block::Paragraph(Paragraph {
            content: vec![make_str("text")],
            source_info: other_source(),
        })]);

        assert!(structural_eq_slot(&slot1, &slot2));
    }

    #[test]
    fn test_structural_eq_slot_inlines() {
        let slot1 = Slot::Inlines(vec![make_str("hello"), make_str("world")]);
        let slot2 = Slot::Inlines(vec![
            Inline::Str(Str {
                text: "hello".to_string(),
                source_info: other_source(),
            }),
            Inline::Str(Str {
                text: "world".to_string(),
                source_info: other_source(),
            }),
        ]);

        assert!(structural_eq_slot(&slot1, &slot2));
    }

    // ==================== NodePtr Tests ====================

    #[test]
    fn test_node_ptr_from_ref() {
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: dummy_source(),
        });

        let ptr1 = NodePtr::from_ref(&block);
        let ptr2 = NodePtr::from_ref(&block);

        assert_eq!(ptr1, ptr2);
    }

    // ==================== Meta Hash Tests ====================

    use quarto_pandoc_types::MergeOp;
    use yaml_rust2::Yaml;

    fn scalar_str(s: &str) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(s.to_string())),
            source_info: dummy_source(),
            merge_op: MergeOp::default(),
        }
    }

    fn scalar_int(i: i64) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Integer(i)),
            source_info: dummy_source(),
            merge_op: MergeOp::default(),
        }
    }

    fn map_of(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        map_of_with_source(entries, dummy_source())
    }

    fn map_of_with_source(entries: Vec<(&str, ConfigValue)>, src: SourceInfo) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: src.clone(),
                value: v,
            })
            .collect();
        ConfigValue {
            value: ConfigValueKind::Map(entries),
            source_info: src,
            merge_op: MergeOp::default(),
        }
    }

    #[test]
    fn meta_hash_same_content_same_hash() {
        let a = map_of(vec![("title", scalar_str("hello")), ("toc", scalar_int(3))]);
        let b = map_of(vec![("title", scalar_str("hello")), ("toc", scalar_int(3))]);
        assert_eq!(compute_meta_hash_fresh(&a), compute_meta_hash_fresh(&b));
    }

    #[test]
    fn meta_hash_different_content_different_hash() {
        let a = map_of(vec![("title", scalar_str("hello"))]);
        let b = map_of(vec![("title", scalar_str("world"))]);
        assert_ne!(compute_meta_hash_fresh(&a), compute_meta_hash_fresh(&b));
    }

    #[test]
    fn meta_hash_excludes_source_info_and_key_source() {
        // Same content, different SourceInfo on values and on keys.
        let a = map_of_with_source(vec![("title", scalar_str("hello"))], dummy_source());
        let b = map_of_with_source(vec![("title", scalar_str("hello"))], other_source());
        // Also flip the inner scalar's source_info.
        let mut b = b;
        if let ConfigValueKind::Map(entries) = &mut b.value {
            entries[0].value.source_info = other_source();
        }
        assert_eq!(compute_meta_hash_fresh(&a), compute_meta_hash_fresh(&b));
    }

    #[test]
    fn meta_hash_excluding_rendered_ignores_top_level_rendered() {
        let a = map_of(vec![
            ("title", scalar_str("hello")),
            (
                "rendered",
                map_of(vec![("navbar", scalar_str("<nav>a</nav>"))]),
            ),
        ]);
        let b = map_of(vec![
            ("title", scalar_str("hello")),
            (
                "rendered",
                map_of(vec![("navbar", scalar_str("<nav>b</nav>"))]),
            ),
        ]);
        assert_ne!(
            compute_meta_hash_fresh(&a),
            compute_meta_hash_fresh(&b),
            "the non-excluding hash must observe the difference",
        );
        assert_eq!(
            compute_meta_hash_fresh_excluding_rendered(&a),
            compute_meta_hash_fresh_excluding_rendered(&b),
            "the excluding-rendered hash must ignore top-level rendered.* divergence",
        );
    }

    #[test]
    fn meta_hash_excluding_rendered_does_not_propagate_to_nested_rendered() {
        // A nested `rendered` key is part of the content and must
        // still participate in the hash.
        let a = map_of(vec![(
            "listings",
            map_of(vec![("rendered", scalar_str("a"))]),
        )]);
        let b = map_of(vec![(
            "listings",
            map_of(vec![("rendered", scalar_str("b"))]),
        )]);
        assert_ne!(
            compute_meta_hash_fresh_excluding_rendered(&a),
            compute_meta_hash_fresh_excluding_rendered(&b),
        );
    }

    #[test]
    fn meta_hash_map_insertion_order_matters() {
        // Regression guard for the no-sort choice: a transform that
        // stuffs a HashMap into meta would produce different
        // insertion orders across runs; the hash must catch that.
        let a = map_of(vec![("a", scalar_int(1)), ("b", scalar_int(2))]);
        let b = map_of(vec![("b", scalar_int(2)), ("a", scalar_int(1))]);
        assert_ne!(
            compute_meta_hash_fresh(&a),
            compute_meta_hash_fresh(&b),
            "different Map insertion order must produce different hashes",
        );
    }

    #[test]
    fn meta_hash_merge_op_participates() {
        let a = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String("x".into())),
            source_info: dummy_source(),
            merge_op: MergeOp::Concat,
        };
        let b = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String("x".into())),
            source_info: dummy_source(),
            merge_op: MergeOp::Prefer,
        };
        assert_ne!(compute_meta_hash_fresh(&a), compute_meta_hash_fresh(&b));
    }

    // ==================== Divergence Localization Tests ====================

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![make_str(text)],
            source_info: dummy_source(),
        })
    }

    #[test]
    fn divergence_identical_docs_returns_none() {
        let blocks = vec![para("alpha"), para("beta")];
        let meta = map_of(vec![("title", scalar_str("t"))]);
        let point = find_first_divergence(&blocks, &meta, &blocks, &meta);
        assert_eq!(point, DivergencePoint::None);
    }

    #[test]
    fn divergence_reports_first_block_mismatch() {
        let a = vec![para("alpha"), para("beta"), para("gamma")];
        let b = vec![para("alpha"), para("DIFFERENT"), para("gamma")];
        let meta = map_of(vec![]);
        let point = find_first_divergence(&a, &meta, &b, &meta);
        match point {
            DivergencePoint::Block {
                index,
                hash_a,
                hash_b,
            } => {
                assert_eq!(index, 1);
                assert_ne!(hash_a, hash_b);
            }
            other => panic!("expected Block divergence, got {:?}", other),
        }
    }

    #[test]
    fn divergence_reports_meta_key_path() {
        let meta_a = map_of(vec![(
            "listings",
            map_of(vec![("foo", map_of(vec![("title", scalar_str("a"))]))]),
        )]);
        let meta_b = map_of(vec![(
            "listings",
            map_of(vec![("foo", map_of(vec![("title", scalar_str("b"))]))]),
        )]);
        let blocks: Vec<Block> = vec![];
        let point = find_first_divergence(&blocks, &meta_a, &blocks, &meta_b);
        match point {
            DivergencePoint::MetaKey {
                path,
                hash_a,
                hash_b,
            } => {
                assert_eq!(path, vec!["listings", "foo", "title"]);
                assert_ne!(hash_a, hash_b);
            }
            other => panic!("expected MetaKey divergence, got {:?}", other),
        }
    }

    #[test]
    fn divergence_skips_rendered_top_level() {
        // Only `rendered.*` differs at the top level -> no divergence.
        let meta_a = map_of(vec![
            ("title", scalar_str("hello")),
            ("rendered", map_of(vec![("navbar", scalar_str("a"))])),
        ]);
        let meta_b = map_of(vec![
            ("title", scalar_str("hello")),
            ("rendered", map_of(vec![("navbar", scalar_str("b"))])),
        ]);
        let blocks: Vec<Block> = vec![];
        let point = find_first_divergence(&blocks, &meta_a, &blocks, &meta_b);
        assert_eq!(point, DivergencePoint::None);
    }
}
