/*
 * block_attr.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Shared collection of trailing standalone `Inline::Attr` nodes into a single
 * block-level `Attr`, so writers can attach attributes to Pandoc blocks that
 * have no native `Attr` field (`Paragraph`, `Plain`, list items). See
 * bd-itqcfxc3 and claude-notes/plans/2026-06-17-block-level-attrs-inline-attr.md.
 */

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::{Attr, empty_attr, is_empty_attr};
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;

/// Split a block's inline content into `(retained_content, merged_attr)` by
/// collecting the **trailing run** of standalone [`Inline::Attr`] nodes — plus
/// any whitespace (`Space`/`SoftBreak`) interleaved with, or immediately
/// preceding, that run — and merging the attrs into one [`Attr`].
///
/// The retained content is always a prefix of `inlines` (the trailing run sits
/// at the end), so the return is a borrowed slice — no clone.
///
/// If the block carries no trailing `Inline::Attr`, `inlines` is returned
/// unchanged with an empty attr (and no whitespace is stripped). Callers should
/// treat an empty attr (`is_empty_attr`) as "no block attribute".
///
/// Merge rule (mirrors the `Header` fold in `postprocess.rs`): processing the
/// collected attrs in document order, `id` = last non-empty wins, `classes`
/// accumulate in order (deduplicated), key-values = last wins. Empty attrs in
/// the run contribute nothing.
///
/// Only a *trailing* run is collected (matching the heading/table precedent); a
/// standalone `Inline::Attr` in the middle of content is left in place for the
/// caller to handle (the inline writers already drop it from output).
pub(crate) fn split_trailing_block_attr(inlines: &[Inline]) -> (&[Inline], Attr) {
    // Walk back from the end over a maximal run of (Attr | whitespace),
    // remembering where the run starts and whether it contains any Attr.
    let mut run_start = inlines.len();
    let mut saw_attr = false;
    for (idx, inline) in inlines.iter().enumerate().rev() {
        match inline {
            Inline::Attr(_) => {
                saw_attr = true;
                run_start = idx;
            }
            Inline::Space(_) | Inline::SoftBreak(_) => {
                run_start = idx;
            }
            _ => break,
        }
    }

    if !saw_attr {
        // No trailing attr — leave content (and any trailing whitespace) intact.
        return (inlines, empty_attr());
    }

    let mut id = String::new();
    let mut classes: Vec<String> = Vec::new();
    let mut kvs: LinkedHashMap<String, String> = LinkedHashMap::new();
    for inline in &inlines[run_start..] {
        if let Inline::Attr(a) = inline {
            let (a_id, a_classes, a_kvs) = &*a.attr;
            if !a_id.is_empty() {
                id = a_id.clone();
            }
            for c in a_classes {
                if !classes.contains(c) {
                    classes.push(c.clone());
                }
            }
            for (k, v) in a_kvs {
                kvs.insert(k.clone(), v.clone());
            }
        }
    }

    (&inlines[..run_start], (id, classes, kvs).into())
}

/// Collect a block-level attribute for a **list item** (`&[Block]`) so a writer
/// can hoist it onto the `<li>`.
///
/// A Pandoc list item is `Vec<Block>` with no per-item `Attr` field, so an
/// authored `- item {.foo}` lands as a trailing [`Inline::Attr`] inside the
/// item's **last block**. This collects that trailing run (via
/// [`split_trailing_block_attr`]) **only when the last block is a `Paragraph` or
/// `Plain`** — the blocks that carry inline content.
///
/// Returns `(attr, stripped_last)`:
/// - When a non-empty attr is found, `attr` is the merged block attr and
///   `stripped_last` is `Some(clone_of_last_block_with_the_trailing_run_removed)`.
///   The caller renders `item[..last] ++ stripped_last`, so the inner
///   `Para`/`Plain` writer never sees the trailing attr (the class lands on the
///   `<li>`, not an inner `<p>` — avoiding the precedence trap).
/// - Otherwise returns `(empty_attr(), None)` and the caller renders the item
///   unchanged. This covers: no trailing attr; an empty trailing attr; or a last
///   block that is not a `Paragraph`/`Plain`.
///
/// The clone is paid only on the (rare) attributed-item path, and only for the
/// single last block.
pub(crate) fn split_list_item_attr(item: &[Block]) -> (Attr, Option<Block>) {
    let Some(last) = item.last() else {
        return (empty_attr(), None);
    };
    let content = match last {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        _ => return (empty_attr(), None),
    };
    let (retained, attr) = split_trailing_block_attr(content);
    if is_empty_attr(&attr) {
        return (empty_attr(), None);
    }
    let retained = retained.to_vec();
    let stripped = match last {
        Block::Paragraph(p) => {
            let mut p = p.clone();
            p.content = retained;
            Block::Paragraph(p)
        }
        Block::Plain(p) => {
            let mut p = p.clone();
            p.content = retained;
            Block::Plain(p)
        }
        _ => unreachable!("last block kind checked above"),
    };
    (attr, Some(stripped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::{AttrSourceInfo, is_empty_attr};
    use quarto_pandoc_types::inline::{InlineAttr, Space, Str};
    use quarto_source_map::SourceInfo;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn str_(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: si(),
        })
    }

    fn space() -> Inline {
        Inline::Space(Space { source_info: si() })
    }

    fn attr_node(id: &str, classes: &[&str], kvs: &[(&str, &str)]) -> Inline {
        let mut m = LinkedHashMap::new();
        for (k, v) in kvs {
            m.insert(k.to_string(), v.to_string());
        }
        let attr: Attr = (
            id.to_string(),
            classes.iter().map(|s| s.to_string()).collect(),
            m,
        )
            .into();
        Inline::Attr(Box::new(InlineAttr::new(
            attr,
            AttrSourceInfo::empty(),
            si(),
        )))
    }

    fn texts(inlines: &[Inline]) -> Vec<String> {
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(s) => format!("Str({})", s.text),
                Inline::Space(_) => "Space".to_string(),
                Inline::Attr(_) => "Attr".to_string(),
                _ => "?".to_string(),
            })
            .collect()
    }

    #[test]
    fn no_trailing_attr_is_unchanged() {
        let inlines = vec![str_("hello"), space()];
        let (content, attr) = split_trailing_block_attr(&inlines);
        assert_eq!(texts(content), vec!["Str(hello)", "Space"]);
        assert!(is_empty_attr(&attr));
    }

    #[test]
    fn trailing_attr_with_preceding_space_is_stripped() {
        let inlines = vec![str_("caption"), space(), attr_node("", &["caption"], &[])];
        let (content, attr) = split_trailing_block_attr(&inlines);
        assert_eq!(texts(content), vec!["Str(caption)"]);
        assert_eq!(attr.1, vec!["caption".to_string()]);
        assert!(attr.0.is_empty());
    }

    #[test]
    fn trailing_whitespace_after_attr_is_stripped() {
        let inlines = vec![str_("x"), space(), attr_node("", &["c"], &[]), space()];
        let (content, _) = split_trailing_block_attr(&inlines);
        assert_eq!(texts(content), vec!["Str(x)"]);
    }

    #[test]
    fn multiple_attrs_merge_in_document_order() {
        // id: last non-empty wins; classes accumulate (dedup); kv: last wins.
        let inlines = vec![
            str_("x"),
            attr_node("first", &["a", "b"], &[("k", "1")]),
            attr_node("second", &["b", "c"], &[("k", "2")]),
        ];
        let (content, attr) = split_trailing_block_attr(&inlines);
        assert_eq!(texts(content), vec!["Str(x)"]);
        assert_eq!(attr.0, "second");
        assert_eq!(
            attr.1,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(attr.2.get("k"), Some(&"2".to_string()));
    }

    #[test]
    fn empty_attr_node_collects_to_nothing() {
        let inlines = vec![str_("x"), attr_node("", &[], &[])];
        let (content, attr) = split_trailing_block_attr(&inlines);
        // The (empty) attr node is still stripped from content...
        assert_eq!(texts(content), vec!["Str(x)"]);
        // ...but contributes no id/classes/kvs.
        assert!(is_empty_attr(&attr));
    }

    #[test]
    fn mid_content_attr_is_left_in_place() {
        let inlines = vec![attr_node("", &["mid"], &[]), str_("x")];
        let (content, attr) = split_trailing_block_attr(&inlines);
        assert_eq!(texts(content), vec!["Attr", "Str(x)"]);
        assert!(is_empty_attr(&attr));
    }

    // --- split_list_item_attr (list-item hoist) ---------------------------

    use quarto_pandoc_types::block::{Paragraph, Plain};

    fn para(inlines: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content: inlines,
            source_info: si(),
        })
    }

    fn plain(inlines: Vec<Inline>) -> Block {
        Block::Plain(Plain {
            content: inlines,
            source_info: si(),
        })
    }

    /// Extract the `(kind, inline-texts)` of a block for assertions.
    fn block_content_texts(block: &Block) -> (&'static str, Vec<String>) {
        match block {
            Block::Paragraph(p) => ("Para", texts(&p.content)),
            Block::Plain(p) => ("Plain", texts(&p.content)),
            _ => ("?", vec![]),
        }
    }

    #[test]
    fn list_item_tight_plain_hoists_and_strips() {
        // Tight item: a single `Plain` ending in a trailing attr.
        let item = vec![plain(vec![
            str_("item"),
            space(),
            attr_node("", &["foo"], &[]),
        ])];
        let (attr, stripped) = split_list_item_attr(&item);
        assert_eq!(attr.1, vec!["foo".to_string()]);
        let stripped = stripped.expect("attributed item yields a stripped last block");
        assert_eq!(
            block_content_texts(&stripped),
            ("Plain", vec!["Str(item)".to_string()])
        );
    }

    #[test]
    fn list_item_loose_para_hoists_and_strips() {
        // Loose item: a single `Para` ending in a trailing attr.
        let item = vec![para(vec![
            str_("item"),
            attr_node("the-id", &["foo"], &[("k", "v")]),
        ])];
        let (attr, stripped) = split_list_item_attr(&item);
        assert_eq!(attr.0, "the-id");
        assert_eq!(attr.1, vec!["foo".to_string()]);
        assert_eq!(attr.2.get("k"), Some(&"v".to_string()));
        let stripped = stripped.expect("attributed item yields a stripped last block");
        assert_eq!(
            block_content_texts(&stripped),
            ("Para", vec!["Str(item)".to_string()])
        );
    }

    #[test]
    fn list_item_hoists_from_last_block_only() {
        // Multi-block item: the attr rides in the LAST block; earlier blocks
        // are untouched and the stripped clone replaces only the last.
        let item = vec![
            para(vec![str_("first")]),
            para(vec![str_("second"), attr_node("", &["foo"], &[])]),
        ];
        let (attr, stripped) = split_list_item_attr(&item);
        assert_eq!(attr.1, vec!["foo".to_string()]);
        let stripped = stripped.expect("attributed item yields a stripped last block");
        assert_eq!(
            block_content_texts(&stripped),
            ("Para", vec!["Str(second)".to_string()])
        );
    }

    #[test]
    fn list_item_without_trailing_attr_is_noop() {
        let item = vec![plain(vec![str_("item")])];
        let (attr, stripped) = split_list_item_attr(&item);
        assert!(is_empty_attr(&attr));
        assert!(stripped.is_none());
    }

    #[test]
    fn list_item_with_empty_trailing_attr_is_noop() {
        let item = vec![plain(vec![str_("item"), attr_node("", &[], &[])])];
        let (attr, stripped) = split_list_item_attr(&item);
        assert!(is_empty_attr(&attr));
        // No hoist: an empty attr means there is nothing to put on the `<li>`.
        assert!(stripped.is_none());
    }

    #[test]
    fn list_item_non_para_plain_last_block_is_noop() {
        // Last block is a nested BulletList (no inline content to carry an attr).
        let inner = Block::BulletList(quarto_pandoc_types::block::BulletList {
            content: vec![vec![plain(vec![str_("nested")])]],
            source_info: si(),
        });
        let item = vec![para(vec![str_("lead")]), inner];
        let (attr, stripped) = split_list_item_attr(&item);
        assert!(is_empty_attr(&attr));
        assert!(stripped.is_none());
    }

    #[test]
    fn list_item_empty_is_noop() {
        let item: Vec<Block> = vec![];
        let (attr, stripped) = split_list_item_attr(&item);
        assert!(is_empty_attr(&attr));
        assert!(stripped.is_none());
    }
}
