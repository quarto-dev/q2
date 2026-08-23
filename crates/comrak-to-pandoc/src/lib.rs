/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Convert comrak's CommonMark AST to quarto-pandoc-types AST.
 *
 * This crate provides direct conversion from comrak's arena-based AST
 * to our owned Pandoc AST structures. Only the CommonMark subset is
 * supported; GFM extensions will panic.
 */

mod block;
mod compare;
mod inline;
pub mod source_location;
mod text;

pub mod normalize;

pub use block::{convert_document, convert_document_with_source};
pub use compare::ast_eq_ignore_source;
pub use normalize::normalize;
pub use source_location::SourceLocationContext;

use hashlink::LinkedHashMap;
use quarto_pandoc_types::Attr;
use quarto_source_map::{By, SourceInfo};

/// The `SourceInfo` for a node this crate has no source location for.
///
/// Every caller is a conversion running without a [`SourceLocationContext`],
/// where comrak's positions were never asked for:
/// - `block.rs` and `inline.rs`, via `source_ctx.map_or_else(…)`;
/// - `text.rs`'s `tokenize_text`, the no-source overload of the text splitter.
///
/// (`normalize.rs` is **not** a caller, despite defining a function of the
/// same name: that one is `#[cfg(test)]`-local to its test module and still
/// returns the old `Original` shape. `normalize.rs` has no `use crate`
/// imports at all. Erasing source info to compare two ASTs on content alone
/// is `ast_eq_ignore_source`'s job, not `normalize`'s — `normalize` strips
/// heading IDs, unwraps `Figure`, strips autolink classes and normalizes
/// code-block attrs.)
///
/// Returns a `Generated`, **not** `Original { FileId(0), 0..0 }` (Plan 3
/// Phase 2, `bd-mxa44voa`). The old value was a well-formed span at the start
/// of file 0: nothing downstream could tell it from a real node there, offset
/// arithmetic consumed it happily, and it is the exact shape
/// `quarto_config::span_assert` flags as `SpanProblem::SuspiciousDefault`
/// (`quarto-config/src/span_assert.rs:74`, checked at `:265`). `Generated`
/// puts "no location" in the type, where `root_file_id()` and `preimage_in()`
/// answer `None` instead of pointing at file 0. Pinned by
/// `no_source_context_yields_generated_source_info`.
///
/// This is a better shape, not a clean one: an anchor-less `Generated` is
/// itself flagged, as `SpanProblem::Generated` (`span_assert.rs:267`). That
/// is the intended trade — a value that announces it has no location is
/// worth more than one that quietly claims a real one — but do not read the
/// new return value as passing `span_assert`.
pub(crate) fn empty_source_info() -> SourceInfo {
    SourceInfo::generated(By {
        kind: "comrak-to-pandoc".to_string(),
        data: serde_json::Value::Null,
    })
}

/// Create an empty attribute tuple.
pub(crate) fn empty_attr() -> Attr {
    (String::new(), vec![], LinkedHashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{Arena, Options, parse_document};
    use quarto_source_map::{FileId, SourceContext};

    fn parse_comrak(markdown: &str) -> quarto_pandoc_types::Pandoc {
        let arena = Arena::new();
        // Pure CommonMark, no GFM extensions (default is CommonMark-only)
        let options = Options::default();
        let root = parse_document(&arena, markdown, &options);
        convert_document(root)
    }

    /// T3 (Plan 3 Phase 2, `bd-mxa44voa`). `convert_document` runs with no
    /// `SourceLocationContext`, so no node it produces has a location. Before
    /// this test, every one of them reported `Original { FileId(0), 0..0 }` —
    /// a well-formed span at the start of file 0, indistinguishable from a
    /// real node there, and the exact shape `quarto_config::span_assert`
    /// flags as `SpanProblem::SuspiciousDefault`
    /// (`quarto-config/src/span_assert.rs:74`, checked at `:265`).
    /// `Generated` says "no location" in the type instead of encoding it as
    /// a coordinate that arithmetic will happily consume.
    ///
    /// The fixture carries emphasis and a code span deliberately. Both
    /// construction paths must be exercised, and a plain `"Hello world."`
    /// reaches only one of them: it is all `Str`/`Space`, which come from
    /// `text.rs`'s `tokenize_text` (`:38`-`:77`). The `source_info` that
    /// `inline.rs:23`'s `map_or_else(empty_source_info, …)` produces is
    /// computed at `:46` but *discarded* by the `NodeValue::Text` arm
    /// (`:49`-`:54`), so reverting that branch alone would not redden a
    /// text-only fixture. `Emph` (`:70`) and `Code` (`:66`) do consume it.
    #[test]
    fn no_source_context_yields_generated_source_info() {
        let pandoc = parse_comrak("Hello *world* and `code`.\n");
        let quarto_pandoc_types::Block::Paragraph(p) = &pandoc.blocks[0] else {
            panic!("expected a paragraph, got {:?}", pandoc.blocks[0]);
        };
        assert!(
            matches!(p.source_info, SourceInfo::Generated { .. }),
            "block source_info should be Generated, got {:?}",
            p.source_info,
        );
        assert!(
            p.content
                .iter()
                .any(|i| matches!(i, quarto_pandoc_types::Inline::Emph(_))),
            "fixture must reach inline.rs:23's branch via Emph, got {:?}",
            p.content,
        );
        for inline in &p.content {
            assert!(
                matches!(inline.source_info(), SourceInfo::Generated { .. }),
                "inline source_info should be Generated, got {:?}",
                inline.source_info(),
            );
        }
    }

    #[test]
    fn test_simple_paragraph() {
        let md = "Hello world.\n";
        let pandoc = parse_comrak(md);
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            quarto_pandoc_types::Block::Paragraph(p) => {
                // Should be: Str("Hello"), Space, Str("world.")
                assert_eq!(p.content.len(), 3);
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_heading() {
        let md = "# Hello\n";
        let pandoc = parse_comrak(md);
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            quarto_pandoc_types::Block::Header(h) => {
                assert_eq!(h.level, 1);
            }
            _ => panic!("Expected Header"),
        }
    }

    #[test]
    fn test_emphasis() {
        let md = "*hello*\n";
        let pandoc = parse_comrak(md);
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            quarto_pandoc_types::Block::Paragraph(p) => {
                assert_eq!(p.content.len(), 1);
                match &p.content[0] {
                    quarto_pandoc_types::Inline::Emph(e) => {
                        assert_eq!(e.content.len(), 1);
                    }
                    _ => panic!("Expected Emph"),
                }
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    // ===================================================================
    // T5 / T6 (Plan 3 Phase 7, `bd-mxa44voa`) — comrak `NodeValue::Text`
    // lockstep provenance.
    //
    // Shared harness. `map_offset` needs a `SourceContext`, which the
    // seven `text.rs` unit tests do not have (they pass a bare `FileId`),
    // so these two tests register the fixture text in a context and drive
    // the full `convert_document_with_source` path.
    // ===================================================================

    /// Parse `markdown` with source tracking, returning the document and a
    /// `SourceContext` in which its spans can be resolved.
    fn parse_comrak_with_source(markdown: &str) -> (quarto_pandoc_types::Pandoc, SourceContext) {
        let arena = Arena::new();
        let options = Options::default();
        let root = parse_document(&arena, markdown, &options);

        let mut ctx = SourceContext::new();
        let file_id = ctx.add_file("test.md".to_string(), Some(markdown.to_string()));
        let source_ctx = SourceLocationContext::new(markdown, file_id);

        (convert_document_with_source(root, Some(&source_ctx)), ctx)
    }

    /// Collect every `Str` inline reachable from `blocks`, in document order.
    ///
    /// The recursion covers exactly the two block shapes these two tests
    /// use — `Paragraph` and `BlockQuote` — and silently ignores every
    /// other `Block` variant. It is a fixture-shaped test helper, not a
    /// general AST visitor.
    fn collect_strs(blocks: &[quarto_pandoc_types::Block]) -> Vec<&quarto_pandoc_types::Str> {
        use quarto_pandoc_types::{Block, Inline};
        let mut out = Vec::new();
        for block in blocks {
            match block {
                Block::Paragraph(p) => {
                    for inline in &p.content {
                        if let Inline::Str(s) = inline {
                            out.push(s);
                        }
                    }
                }
                Block::BlockQuote(bq) => out.extend(collect_strs(&bq.content)),
                _ => {}
            }
        }
        out
    }

    /// The one `Str` in `strs` whose text is `text`. Panics if the fixture
    /// produced zero or more than one such `Str`, so a fixture that stops
    /// exercising what the test names cannot pass silently.
    fn only_str<'a>(
        strs: &[&'a quarto_pandoc_types::Str],
        text: &str,
    ) -> &'a quarto_pandoc_types::Str {
        let matches: Vec<_> = strs.iter().filter(|s| s.text == text).collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one Str({text:?}) in the fixture, found {}: {:?}",
            matches.len(),
            strs.iter().map(|s| &s.text).collect::<Vec<_>>(),
        );
        matches[0]
    }

    /// Where `s`'s first content byte came from, per the accessor rule in
    /// `claude-notes/research/2026-08-21-provenance-audit-findings.md` § 1:
    /// positions go through `map_offset`, never `start_offset()`, because
    /// a lockstep-tiled span may be a `Concat`.
    fn origin_of(s: &quarto_pandoc_types::Str, ctx: &SourceContext) -> usize {
        let mapped = s
            .source_info
            .map_offset(0, ctx)
            .unwrap_or_else(|| panic!("Str({:?}) has no mappable origin", s.text));
        assert_eq!(mapped.file_id, FileId(0), "Str({:?}) file id", s.text);
        mapped.location.offset
    }

    /// T5. A single comrak `Text` node whose decoded content is shorter
    /// than its source: `\*` is 2 source bytes decoding to 1, `&amp;` is 5
    /// decoding to 1. Before the lockstep walker, `tokenize_text_with_source`
    /// computed each token's offset as `base_offset + byte_idx` over the
    /// *decoded* string, so the error accumulated: −1 after the escape and
    /// −5 after the entity.
    ///
    /// The fixture is § 7's measured case. Only `dd` and `ee` are asserted.
    /// `aa*bb` is deliberately **not** asserted: it is reported `0..5`
    /// against a true `0..6`, an end-short error only, so `map_offset(0)`
    /// on it is 0 both before and after the fix and the assertion would
    /// pass without the fix.
    #[test]
    fn t5_text_node_offsets_survive_escape_and_entity() {
        //                    0123456789...
        let markdown = "aa\\*bb cc &amp; dd ee\n";
        let (pandoc, ctx) = parse_comrak_with_source(markdown);
        let strs = collect_strs(&pandoc.blocks);

        // source:  a a \ * b b _ c c _ &  a  m  p  ;  _  d  d  _  e  e
        // offset:  0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20
        assert_eq!(origin_of(only_str(&strs, "dd"), &ctx), 16);
        assert_eq!(origin_of(only_str(&strs, "ee"), &ctx), 19);
    }

    /// T6 — an **upstream-behaviour pin on comrak**, not a test of q2 code.
    ///
    /// Nothing in `comrak-to-pandoc` makes the drift reset at a
    /// `SoftBreak`; comrak's per-line `Text` nodes do. Its "revert" is a
    /// comrak version bump, not a q2 hunk, so do not read a green T6 as
    /// covering our walker. What it protects is the walker's *premise*:
    /// § 7 fact 1 — a `Text` node's span is contiguous and single-line, so
    /// the block prefix (`> `) never sits inside one and lockstep needs no
    /// deletion rule. If this test ever reddens, the lockstep design is
    /// wrong and needs a deletion rule; it should fail loudly rather than
    /// be adjusted.
    ///
    /// Discrimination, measured: **only the `ee` half discriminates.**
    /// `dd` reports 14..16 correctly *before* the lockstep walker too —
    /// resetting at the `SoftBreak` is precisely what comrak does — so
    /// `assert dd == 14` survives its own revert. `ee` is 19 pre-fix and
    /// 23 post-fix. The `dd` half is kept because the pin is about the
    /// reset property, which is exactly what `dd` states.
    #[test]
    fn t6_comrak_upstream_pin_text_node_spans_reset_at_softbreak() {
        // line 1:  >  _  a  a  \  *  b  b  _  c  c  \n
        // offset:  0  1  2  3  4  5  6  7  8  9  10 11
        // line 2:  >  _  d  d  _  &  a  m  p  ;  _  e  e  \n
        // offset:  12 13 14 15 16 17 18 19 20 21 22 23 24 25
        let markdown = "> aa\\*bb cc\n> dd &amp; ee\n";
        let (pandoc, ctx) = parse_comrak_with_source(markdown);
        let strs = collect_strs(&pandoc.blocks);

        // Non-discriminating half: correct before the fix as well.
        assert_eq!(origin_of(only_str(&strs, "dd"), &ctx), 14);
        // Discriminating half: 19 pre-fix, 23 post-fix.
        assert_eq!(origin_of(only_str(&strs, "ee"), &ctx), 23);
    }
}
