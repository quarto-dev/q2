/*
 * annotate.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Convert an AST diff into a change-annotated AST.
 *
 * Given two Pandoc ASTs (before and after), this module computes a
 * reconciliation plan (the AST diff) and produces a NEW AST in which the
 * changes are represented with nodes qmd can already round-trip:
 *
 * - added inlines    -> wrapped in `Inline::Insert`  (qmd: `[++ ...]`)
 * - removed inlines  -> wrapped in `Inline::Delete`  (qmd: `[-- ...]`)
 * - added blocks     -> wrapped in a `Div` with class `added`   (qmd: `::: {.added}`)
 * - removed blocks   -> wrapped in a `Div` with class `removed` (qmd: `::: {.removed}`)
 *
 * Unchanged content is cloned as-is. Containers whose children changed are
 * recursed into, so the annotations appear at the finest granularity the
 * reconciliation plan provides.
 *
 * Deletions are implicit in a ReconciliationPlan (before-indices never
 * referenced by any alignment). They are re-inserted positionally: while
 * walking the after-ordered alignments, any not-yet-emitted unmatched
 * before-index smaller than the current alignment's before-index is flushed
 * as removed content at that point; leftovers are flushed at the end.
 */

use crate::compute::compute_reconciliation;
use crate::types::{
    BlockAlignment, InlineAlignment, InlineReconciliationPlan, ListItemAlignment,
    ReconciliationPlan,
};
use quarto_pandoc_types::attr::{AttrSourceInfo, empty_attr};
use quarto_pandoc_types::inline::{Delete, Insert};
use quarto_pandoc_types::{Block, Div, Inline, Pandoc};
use quarto_source_map::{By, SourceInfo};

/// Class used on Divs wrapping added blocks.
pub const ADDED_CLASS: &str = "added";
/// Class used on Divs wrapping removed blocks.
pub const REMOVED_CLASS: &str = "removed";

/// Provenance for the wrapper nodes this module synthesizes.
fn generated_by_annotate() -> SourceInfo {
    SourceInfo::generated(By {
        kind: "diff-annotate".to_string(),
        data: serde_json::Value::Null,
    })
}

/// Diff `before` against `after` and produce a change-annotated AST.
///
/// The result's metadata is taken from `after`; metadata changes are not
/// annotated (v1).
pub fn annotate_diff(before: &Pandoc, after: &Pandoc) -> Pandoc {
    let plan = compute_reconciliation(before, after);
    Pandoc {
        meta: after.meta.clone(),
        blocks: diff_blocks(&before.blocks, &after.blocks, &plan),
    }
}

fn class_div(class: &str, blocks: Vec<Block>) -> Block {
    Block::Div(Div {
        attr: (String::new(), vec![class.to_string()], Default::default()),
        content: blocks,
        source_info: generated_by_annotate(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn insert_inline(content: Vec<Inline>) -> Inline {
    Inline::Insert(Insert {
        attr: empty_attr(),
        content,
        source_info: generated_by_annotate(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn delete_inline(content: Vec<Inline>) -> Inline {
    Inline::Delete(Delete {
        attr: empty_attr(),
        content,
        source_info: generated_by_annotate(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Which before-indices does this plan's alignment list reference?
fn matched_before_indices_blocks(plan: &ReconciliationPlan, before_len: usize) -> Vec<bool> {
    let mut matched = vec![false; before_len];
    for alignment in &plan.block_alignments {
        match alignment {
            BlockAlignment::KeepBefore(i) => matched[*i] = true,
            BlockAlignment::RecurseIntoContainer { before_idx, .. } => matched[*before_idx] = true,
            BlockAlignment::UseAfter(_) => {}
        }
    }
    matched
}

fn diff_blocks(before: &[Block], after: &[Block], plan: &ReconciliationPlan) -> Vec<Block> {
    let matched = matched_before_indices_blocks(plan, before.len());
    let mut result: Vec<Block> = Vec::new();
    let mut added_run: Vec<Block> = Vec::new();
    // Next before-index not yet considered for removal flushing.
    let mut removed_cursor = 0usize;

    // Flush removed blocks with index < upto (in before order), then the
    // pending added run. Removed-before-added mirrors conventional diff
    // hunks.
    let sync = |upto: usize,
                removed_cursor: &mut usize,
                added_run: &mut Vec<Block>,
                result: &mut Vec<Block>| {
        let mut removed: Vec<Block> = Vec::new();
        while *removed_cursor < upto.min(before.len()) {
            if !matched[*removed_cursor] {
                removed.push(before[*removed_cursor].clone());
            }
            *removed_cursor += 1;
        }
        if !removed.is_empty() {
            result.push(class_div(REMOVED_CLASS, removed));
        }
        if !added_run.is_empty() {
            result.push(class_div(ADDED_CLASS, std::mem::take(added_run)));
        }
    };

    for (alignment_idx, alignment) in plan.block_alignments.iter().enumerate() {
        match alignment {
            BlockAlignment::UseAfter(after_idx) => {
                added_run.push(after[*after_idx].clone());
            }
            BlockAlignment::KeepBefore(before_idx) => {
                sync(
                    *before_idx,
                    &mut removed_cursor,
                    &mut added_run,
                    &mut result,
                );
                result.push(before[*before_idx].clone());
            }
            BlockAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                sync(
                    *before_idx,
                    &mut removed_cursor,
                    &mut added_run,
                    &mut result,
                );
                result.extend(diff_recursed_block(
                    &before[*before_idx],
                    &after[*after_idx],
                    plan,
                    alignment_idx,
                ));
            }
        }
    }
    sync(
        before.len(),
        &mut removed_cursor,
        &mut added_run,
        &mut result,
    );
    result
}

/// A block pair the plan told us to recurse into. Returns one block when the
/// container kind is supported, or a removed+added pair as fallback.
fn diff_recursed_block(
    before: &Block,
    after: &Block,
    plan: &ReconciliationPlan,
    alignment_idx: usize,
) -> Vec<Block> {
    let fallback = || {
        vec![
            class_div(REMOVED_CLASS, vec![before.clone()]),
            class_div(ADDED_CLASS, vec![after.clone()]),
        ]
    };

    if let Some(nested) = plan.block_container_plans.get(&alignment_idx) {
        match (before, after) {
            (Block::Div(b), Block::Div(a)) => {
                let mut out = a.clone();
                out.content = diff_blocks(&b.content, &a.content, nested);
                vec![Block::Div(out)]
            }
            (Block::BlockQuote(b), Block::BlockQuote(a)) => {
                let mut out = a.clone();
                out.content = diff_blocks(&b.content, &a.content, nested);
                vec![Block::BlockQuote(out)]
            }
            (Block::Figure(b), Block::Figure(a)) => {
                let mut out = a.clone();
                out.content = diff_blocks(&b.content, &a.content, nested);
                vec![Block::Figure(out)]
            }
            (Block::BulletList(b), Block::BulletList(a)) => {
                diff_list_split(&b.content, &a.content, nested, &|items| {
                    let mut out = a.clone();
                    out.content = items;
                    Block::BulletList(out)
                })
            }
            (Block::OrderedList(b), Block::OrderedList(a)) => {
                diff_list_split(&b.content, &a.content, nested, &|items| {
                    let mut out = a.clone();
                    out.content = items;
                    Block::OrderedList(out)
                })
            }
            _ => fallback(),
        }
    } else if let Some(inline_plan) = plan.inline_plans.get(&alignment_idx) {
        match (before, after) {
            (Block::Paragraph(b), Block::Paragraph(a)) => {
                let mut out = a.clone();
                out.content = diff_inlines(&b.content, &a.content, inline_plan);
                vec![Block::Paragraph(out)]
            }
            (Block::Plain(b), Block::Plain(a)) => {
                let mut out = a.clone();
                out.content = diff_inlines(&b.content, &a.content, inline_plan);
                vec![Block::Plain(out)]
            }
            (Block::Header(b), Block::Header(a)) => {
                let mut out = a.clone();
                out.content = diff_inlines(&b.content, &a.content, inline_plan);
                vec![Block::Header(out)]
            }
            _ => fallback(),
        }
    } else {
        // Tables, custom nodes, definition lists: v1 falls back to a
        // whole-node removed+added pair.
        fallback()
    }
}

/// Diff a list by splitting it into segments so the list marker travels
/// with the annotation: kept/reconciled items stay in list segments, while
/// runs of added or removed items become their own sub-list wrapped in a
/// `.added` / `.removed` div. (`* ::: {.added} … :::` — a bullet outside
/// the div — is exactly what this avoids.)
///
/// A `Reconcile` item whose before-side has no blocks (e.g. a bare `- `
/// marker) is treated as an added item: its entire content is new and the
/// empty original renders as nothing anyway.
///
/// `make_list` rebuilds a list block of the right kind from a subset of
/// items. For ordered lists the restart numbering of later segments is
/// knowingly off — this is a diff visualization, not a round-trip.
fn diff_list_split(
    before_items: &[Vec<Block>],
    after_items: &[Vec<Block>],
    plan: &ReconciliationPlan,
    make_list: &dyn Fn(Vec<Vec<Block>>) -> Block,
) -> Vec<Block> {
    let mut matched = vec![false; before_items.len()];
    for alignment in &plan.list_item_alignments {
        match alignment {
            ListItemAlignment::KeepOriginal(i) | ListItemAlignment::Reconcile(i) => {
                matched[*i] = true
            }
            ListItemAlignment::UseExecuted => {}
        }
    }

    let mut segments: Vec<Block> = Vec::new();
    let mut kept_run: Vec<Vec<Block>> = Vec::new();
    let mut added_run: Vec<Vec<Block>> = Vec::new();
    let mut removed_cursor = 0usize;

    let collect_removed = |upto: usize, removed_cursor: &mut usize| -> Vec<Vec<Block>> {
        let mut out = Vec::new();
        while *removed_cursor < upto.min(before_items.len()) {
            if !matched[*removed_cursor] {
                out.push(before_items[*removed_cursor].clone());
            }
            *removed_cursor += 1;
        }
        out
    };

    // Emit pending removed items (before-index < `upto`) and, when
    // `flush_added` is set, the pending added run — closing the current
    // kept segment first so document order is preserved
    // (kept, removed, added, kept, ...).
    let boundary = |upto: usize,
                    flush_added: bool,
                    removed_cursor: &mut usize,
                    kept_run: &mut Vec<Vec<Block>>,
                    added_run: &mut Vec<Vec<Block>>,
                    segments: &mut Vec<Block>| {
        let removed = collect_removed(upto, removed_cursor);
        let flushing_added = flush_added && !added_run.is_empty();
        if removed.is_empty() && !flushing_added {
            return;
        }
        if !kept_run.is_empty() {
            segments.push(make_list(std::mem::take(kept_run)));
        }
        if !removed.is_empty() {
            segments.push(class_div(REMOVED_CLASS, vec![make_list(removed)]));
        }
        if flushing_added {
            segments.push(class_div(
                ADDED_CLASS,
                vec![make_list(std::mem::take(added_run))],
            ));
        }
    };

    for (exec_idx, alignment) in plan.list_item_alignments.iter().enumerate() {
        match alignment {
            ListItemAlignment::KeepOriginal(before_idx) => {
                boundary(
                    *before_idx,
                    true,
                    &mut removed_cursor,
                    &mut kept_run,
                    &mut added_run,
                    &mut segments,
                );
                kept_run.push(before_items[*before_idx].clone());
            }
            ListItemAlignment::Reconcile(before_idx) => {
                if before_items[*before_idx].is_empty() {
                    // Entirely new content in a previously empty item: treat
                    // as added; keep accumulating the added run.
                    boundary(
                        *before_idx,
                        false,
                        &mut removed_cursor,
                        &mut kept_run,
                        &mut added_run,
                        &mut segments,
                    );
                    added_run.push(after_items[exec_idx].clone());
                } else {
                    boundary(
                        *before_idx,
                        true,
                        &mut removed_cursor,
                        &mut kept_run,
                        &mut added_run,
                        &mut segments,
                    );
                    if let Some(nested) = plan.list_item_plans.get(&exec_idx) {
                        kept_run.push(diff_blocks(
                            &before_items[*before_idx],
                            &after_items[exec_idx],
                            nested,
                        ));
                    } else {
                        kept_run.push(vec![
                            class_div(REMOVED_CLASS, before_items[*before_idx].clone()),
                            class_div(ADDED_CLASS, after_items[exec_idx].clone()),
                        ]);
                    }
                }
            }
            ListItemAlignment::UseExecuted => {
                added_run.push(after_items[exec_idx].clone());
            }
        }
    }

    // Trailing: kept segment first (it precedes trailing removals/additions
    // in document order), then removed, then added.
    if !kept_run.is_empty() {
        segments.push(make_list(std::mem::take(&mut kept_run)));
    }
    let removed = collect_removed(before_items.len(), &mut removed_cursor);
    if !removed.is_empty() {
        segments.push(class_div(REMOVED_CLASS, vec![make_list(removed)]));
    }
    if !added_run.is_empty() {
        segments.push(class_div(ADDED_CLASS, vec![make_list(added_run)]));
    }
    segments
}

fn diff_inlines(
    before: &[Inline],
    after: &[Inline],
    plan: &InlineReconciliationPlan,
) -> Vec<Inline> {
    let mut matched = vec![false; before.len()];
    for alignment in &plan.inline_alignments {
        match alignment {
            InlineAlignment::KeepBefore(i) => matched[*i] = true,
            InlineAlignment::RecurseIntoContainer { before_idx, .. } => matched[*before_idx] = true,
            InlineAlignment::UseAfter(_) => {}
        }
    }

    let mut result: Vec<Inline> = Vec::new();
    let mut added_run: Vec<Inline> = Vec::new();
    let mut removed_cursor = 0usize;

    let sync = |upto: usize,
                removed_cursor: &mut usize,
                added_run: &mut Vec<Inline>,
                result: &mut Vec<Inline>| {
        let mut removed: Vec<Inline> = Vec::new();
        while *removed_cursor < upto.min(before.len()) {
            if !matched[*removed_cursor] {
                removed.push(before[*removed_cursor].clone());
            }
            *removed_cursor += 1;
        }
        if !removed.is_empty() {
            result.push(delete_inline(removed));
        }
        if !added_run.is_empty() {
            result.push(insert_inline(std::mem::take(added_run)));
        }
    };

    for (alignment_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        match alignment {
            InlineAlignment::UseAfter(after_idx) => {
                added_run.push(after[*after_idx].clone());
            }
            InlineAlignment::KeepBefore(before_idx) => {
                sync(
                    *before_idx,
                    &mut removed_cursor,
                    &mut added_run,
                    &mut result,
                );
                result.push(before[*before_idx].clone());
            }
            InlineAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                sync(
                    *before_idx,
                    &mut removed_cursor,
                    &mut added_run,
                    &mut result,
                );
                result.extend(diff_recursed_inline(
                    &before[*before_idx],
                    &after[*after_idx],
                    plan,
                    alignment_idx,
                ));
            }
        }
    }
    sync(
        before.len(),
        &mut removed_cursor,
        &mut added_run,
        &mut result,
    );
    result
}

/// An inline pair the plan told us to recurse into. Returns one inline when
/// the container kind is supported, or a delete+insert pair as fallback.
fn diff_recursed_inline(
    before: &Inline,
    after: &Inline,
    plan: &InlineReconciliationPlan,
    alignment_idx: usize,
) -> Vec<Inline> {
    let fallback = || {
        vec![
            delete_inline(vec![before.clone()]),
            insert_inline(vec![after.clone()]),
        ]
    };

    if let Some(block_plan) = plan.note_block_plans.get(&alignment_idx) {
        match (before, after) {
            (Inline::Note(b), Inline::Note(a)) => {
                let mut out = a.clone();
                out.content = diff_blocks(&b.content, &a.content, block_plan);
                vec![Inline::Note(out)]
            }
            _ => fallback(),
        }
    } else if let Some(nested) = plan.inline_container_plans.get(&alignment_idx) {
        macro_rules! recurse_content {
            ($variant:ident, $b:expr, $a:expr) => {{
                let mut out = $a.clone();
                out.content = diff_inlines(&$b.content, &$a.content, nested);
                vec![Inline::$variant(out)]
            }};
        }
        match (before, after) {
            (Inline::Emph(b), Inline::Emph(a)) => recurse_content!(Emph, b, a),
            (Inline::Strong(b), Inline::Strong(a)) => recurse_content!(Strong, b, a),
            (Inline::Underline(b), Inline::Underline(a)) => recurse_content!(Underline, b, a),
            (Inline::Strikeout(b), Inline::Strikeout(a)) => recurse_content!(Strikeout, b, a),
            (Inline::Superscript(b), Inline::Superscript(a)) => {
                recurse_content!(Superscript, b, a)
            }
            (Inline::Subscript(b), Inline::Subscript(a)) => recurse_content!(Subscript, b, a),
            (Inline::SmallCaps(b), Inline::SmallCaps(a)) => recurse_content!(SmallCaps, b, a),
            (Inline::Quoted(b), Inline::Quoted(a)) => recurse_content!(Quoted, b, a),
            (Inline::Cite(b), Inline::Cite(a)) => recurse_content!(Cite, b, a),
            (Inline::Link(b), Inline::Link(a)) => recurse_content!(Link, b, a),
            (Inline::Image(b), Inline::Image(a)) => recurse_content!(Image, b, a),
            (Inline::Span(b), Inline::Span(a)) => recurse_content!(Span, b, a),
            (Inline::Insert(b), Inline::Insert(a)) => recurse_content!(Insert, b, a),
            (Inline::Delete(b), Inline::Delete(a)) => recurse_content!(Delete, b, a),
            (Inline::Highlight(b), Inline::Highlight(a)) => recurse_content!(Highlight, b, a),
            (Inline::EditComment(b), Inline::EditComment(a)) => {
                recurse_content!(EditComment, b, a)
            }
            _ => fallback(),
        }
    } else {
        // Custom inline nodes (or missing plan): whole-node delete+insert.
        fallback()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::structural_eq_blocks;
    use quarto_pandoc_types::{BulletList, CodeBlock, Paragraph, Space, Str};

    fn str_inline(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })
    }

    fn space() -> Inline {
        Inline::Space(Space {
            source_info: SourceInfo::for_test(),
        })
    }

    /// Build a paragraph of words separated by spaces ("a b" -> [Str a, Space, Str b]).
    fn para(text: &str) -> Block {
        let mut content: Vec<Inline> = Vec::new();
        for (i, word) in text.split(' ').enumerate() {
            if i > 0 {
                content.push(space());
            }
            content.push(str_inline(word));
        }
        Block::Paragraph(Paragraph {
            content,
            source_info: SourceInfo::for_test(),
        })
    }

    fn code_block(code: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: empty_attr(),
            text: code.to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc {
            meta: Default::default(),
            blocks,
        }
    }

    /// Assert a block is a Div with exactly the given class, and return its content.
    fn expect_class_div<'a>(block: &'a Block, class: &str) -> &'a [Block] {
        match block {
            Block::Div(div) => {
                assert_eq!(
                    div.attr.1,
                    vec![class.to_string()],
                    "expected Div with class .{class}, got classes {:?}",
                    div.attr.1
                );
                &div.content
            }
            other => panic!("expected Div .{class}, got {other:?}"),
        }
    }

    fn expect_paragraph(block: &Block) -> &[Inline] {
        match block {
            Block::Paragraph(p) => &p.content,
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    fn expect_str(inline: &Inline, text: &str) {
        match inline {
            Inline::Str(s) => assert_eq!(s.text, text),
            other => panic!("expected Str({text:?}), got {other:?}"),
        }
    }

    #[test]
    fn identical_docs_produce_no_annotations() {
        let before = doc(vec![para("one two"), code_block("x <- 1")]);
        let after = doc(vec![para("one two"), code_block("x <- 1")]);

        let result = annotate_diff(&before, &after);

        assert!(
            structural_eq_blocks(&result.blocks, &before.blocks),
            "identical docs must produce an unannotated copy; got {:?}",
            result.blocks
        );
    }

    #[test]
    fn added_block_wrapped_in_added_div() {
        let before = doc(vec![para("one")]);
        let after = doc(vec![para("one"), para("two")]);

        let result = annotate_diff(&before, &after);

        assert_eq!(result.blocks.len(), 2, "got {:?}", result.blocks);
        expect_str(&expect_paragraph(&result.blocks[0])[0], "one");
        let added = expect_class_div(&result.blocks[1], ADDED_CLASS);
        assert_eq!(added.len(), 1);
        expect_str(&expect_paragraph(&added[0])[0], "two");
    }

    #[test]
    fn consecutive_added_blocks_grouped_in_one_div() {
        let before = doc(vec![para("one")]);
        let after = doc(vec![para("one"), para("two"), code_block("y")]);

        let result = annotate_diff(&before, &after);

        assert_eq!(result.blocks.len(), 2, "got {:?}", result.blocks);
        let added = expect_class_div(&result.blocks[1], ADDED_CLASS);
        assert_eq!(added.len(), 2, "both new blocks share one .added div");
        expect_str(&expect_paragraph(&added[0])[0], "two");
        assert!(matches!(&added[1], Block::CodeBlock(cb) if cb.text == "y"));
    }

    #[test]
    fn removed_block_wrapped_in_removed_div_at_position() {
        let before = doc(vec![para("one"), para("two"), para("three")]);
        let after = doc(vec![para("one"), para("three")]);

        let result = annotate_diff(&before, &after);

        assert_eq!(result.blocks.len(), 3, "got {:?}", result.blocks);
        expect_str(&expect_paragraph(&result.blocks[0])[0], "one");
        let removed = expect_class_div(&result.blocks[1], REMOVED_CLASS);
        assert_eq!(removed.len(), 1);
        expect_str(&expect_paragraph(&removed[0])[0], "two");
        expect_str(&expect_paragraph(&result.blocks[2])[0], "three");
    }

    #[test]
    fn changed_paragraph_produces_inline_insert_delete() {
        let before = doc(vec![para("The cat sat")]);
        let after = doc(vec![para("The dog sat")]);

        let result = annotate_diff(&before, &after);

        assert_eq!(result.blocks.len(), 1, "got {:?}", result.blocks);
        let content = expect_paragraph(&result.blocks[0]);
        // [Str The, Space, Delete[Str cat], Insert[Str dog], Space, Str sat]
        assert_eq!(content.len(), 6, "got {content:?}");
        expect_str(&content[0], "The");
        assert!(matches!(&content[1], Inline::Space(_)));
        match &content[2] {
            Inline::Delete(d) => {
                assert_eq!(d.content.len(), 1);
                expect_str(&d.content[0], "cat");
            }
            other => panic!("expected Delete, got {other:?}"),
        }
        match &content[3] {
            Inline::Insert(ins) => {
                assert_eq!(ins.content.len(), 1);
                expect_str(&ins.content[0], "dog");
            }
            other => panic!("expected Insert, got {other:?}"),
        }
        assert!(matches!(&content[4], Inline::Space(_)));
        expect_str(&content[5], "sat");
    }

    #[test]
    fn type_changed_block_produces_removed_added_pair() {
        let before = doc(vec![para("hello")]);
        let after = doc(vec![code_block("hello()")]);

        let result = annotate_diff(&before, &after);

        assert_eq!(result.blocks.len(), 2, "got {:?}", result.blocks);
        let removed = expect_class_div(&result.blocks[0], REMOVED_CLASS);
        expect_str(&expect_paragraph(&removed[0])[0], "hello");
        let added = expect_class_div(&result.blocks[1], ADDED_CLASS);
        assert!(matches!(&added[0], Block::CodeBlock(cb) if cb.text == "hello()"));
    }

    #[test]
    fn nested_div_recursion_annotates_inside() {
        fn wrap_div(blocks: Vec<Block>) -> Block {
            Block::Div(Div {
                attr: (
                    String::new(),
                    vec!["wrapper".to_string()],
                    Default::default(),
                ),
                content: blocks,
                source_info: SourceInfo::for_test(),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        // Inner paragraph fully replaced (no shared words), sibling kept.
        let before = doc(vec![wrap_div(vec![para("alpha"), para("keep")])]);
        let after = doc(vec![wrap_div(vec![para("beta"), para("keep")])]);

        let result = annotate_diff(&before, &after);

        assert_eq!(result.blocks.len(), 1, "got {:?}", result.blocks);
        let inner = expect_class_div(&result.blocks[0], "wrapper");
        assert_eq!(inner.len(), 3, "got {inner:?}");
        let removed = expect_class_div(&inner[0], REMOVED_CLASS);
        expect_str(&expect_paragraph(&removed[0])[0], "alpha");
        let added = expect_class_div(&inner[1], ADDED_CLASS);
        expect_str(&expect_paragraph(&added[0])[0], "beta");
        expect_str(&expect_paragraph(&inner[2])[0], "keep");
    }

    #[test]
    fn list_item_added_and_removed() {
        fn bullet_list(items: Vec<Vec<Block>>) -> Block {
            Block::BulletList(BulletList {
                content: items,
                source_info: SourceInfo::for_test(),
            })
        }
        let before = doc(vec![bullet_list(vec![
            vec![para("one")],
            vec![para("two")],
            vec![para("three")],
        ])]);
        let after = doc(vec![bullet_list(vec![
            vec![para("one")],
            vec![para("three")],
            vec![para("four")],
        ])]);

        let result = annotate_diff(&before, &after);

        // The list is split into segments so the bullet marker stays inside
        // the annotation div:
        //   list[one], .removed[list[two]], list[three], .added[list[four]]
        assert_eq!(result.blocks.len(), 4, "got {:?}", result.blocks);
        let items_of = |block: &Block| -> Vec<Vec<Block>> {
            match block {
                Block::BulletList(bl) => bl.content.clone(),
                other => panic!("expected BulletList, got {other:?}"),
            }
        };
        expect_str(
            &expect_paragraph(&items_of(&result.blocks[0])[0][0])[0],
            "one",
        );
        let removed = expect_class_div(&result.blocks[1], REMOVED_CLASS);
        expect_str(&expect_paragraph(&items_of(&removed[0])[0][0])[0], "two");
        expect_str(
            &expect_paragraph(&items_of(&result.blocks[2])[0][0])[0],
            "three",
        );
        let added = expect_class_div(&result.blocks[3], ADDED_CLASS);
        expect_str(&expect_paragraph(&items_of(&added[0])[0][0])[0], "four");
    }
}
