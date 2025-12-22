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

use crate::custom::{CustomNode, Slot};
use crate::{Attr, Block, Inline};
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
        Inline::Attr(attr, _attr_source) => {
            hash_attr(attr, &mut hasher);
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
    rows: &[crate::table::Row],
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
        (Block::CodeBlock(a), Block::CodeBlock(b)) => {
            attr_eq(&a.attr, &b.attr) && a.text == b.text
        }
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
                && a.content.iter().zip(&b.content).all(|((ta, da), (tb, db))| {
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
        (Inline::Attr(a, _), Inline::Attr(b, _)) => attr_eq(a, b),
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
    use crate::{Paragraph, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn dummy_source() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn other_source() -> SourceInfo {
        SourceInfo::original(FileId(1), 100, 200)
    }

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

        let plain = Block::Plain(crate::Plain {
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
}
